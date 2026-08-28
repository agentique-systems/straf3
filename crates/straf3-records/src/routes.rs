//! The `/v1` surface: ARCHITECTURE §7.5, plus URLS.md §5's
//! `GET /v1/runs/by-digest/:digest16`.
//!
//! # Two things this module is careful about
//!
//! **Success with nothing in it is a `200`.** A board nobody has set a time on
//! answers `{"category": …, "entries": [], "total": 0}`. It is never a bare
//! `[]`, never a `204`, never a `404`. A failure is a non-2xx carrying
//! `{"error", "detail"}`. `web/site/app/api.js` distinguishes four outcomes and
//! it can only do that because those two cases never collapse into each other
//! here — which is the whole of requirement r9 on this side of the wire.
//!
//! **Every duration is integer milliseconds named `*_ms`.** ARCHITECTURE §3.6
//! and §5.1: no column, API field or JSON number in this platform is a duration
//! in seconds, and none is a float.
//!
//! # What is deliberately absent
//!
//! `/auth/:provider/start`, `/auth/:provider/callback` and `POST /auth/logout`
//! from §7.5. Neon Auth supersedes ARCHITECTURE §6 in full: the site signs the
//! player in and this service only ever *verifies* the resulting bearer token.

use axum::extract::{DefaultBodyLimit, Path, Query, State};
use axum::http::{HeaderMap, StatusCode, header};
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post};
use axum::{Json, Router};
use serde::Deserialize;
use serde_json::{Value, json};
use sqlx::{PgPool, Row};
use uuid::Uuid;

use crate::auth::{self, AuthedPlayer};
use crate::catalog::{self};
use crate::error::{ApiError, ApiResult};
use crate::limits;
use crate::state::AppState;
use crate::{digest16, intake, profiles};

/// Every path this service serves, in the order the router declares them.
///
/// Exposed so a test — and `loop`'s one-origin bring-up — can enumerate the
/// surface rather than rediscover it from the router's internals.
#[must_use]
pub fn paths() -> Vec<&'static str> {
    vec![
        "/v1/health",
        "/v1/meta",
        "/v1/maps",
        "/v1/maps/{slug}",
        "/v1/maps/{slug}/leaderboard",
        "/v1/maps/{slug}/leaderboard/me",
        "/v1/attempts",
        "/v1/runs",
        "/v1/runs/by-digest/{digest16}",
        "/v1/runs/{id}",
        "/v1/runs/{id}/demo",
        "/v1/players/{name}",
    ]
}

/// The whole `/v1` router.
#[must_use]
pub fn router(state: AppState) -> Router {
    Router::new()
        .route("/v1/health", get(health))
        .route("/v1/meta", get(meta))
        .route("/v1/maps", get(list_maps))
        .route("/v1/maps/{slug}", get(map_detail))
        .route("/v1/maps/{slug}/leaderboard", get(leaderboard))
        .route("/v1/maps/{slug}/leaderboard/me", get(leaderboard_me))
        .route("/v1/attempts", post(create_attempt))
        .route(
            "/v1/runs",
            post(submit_run).layer(DefaultBodyLimit::max(limits::MAX_DECOMPRESSED_BYTES)),
        )
        // Before `/v1/runs/{id}`, and distinguishable from it by shape rather
        // than by lookup: URLS.md §5's two spellings of a run's name.
        .route("/v1/runs/by-digest/{digest16}", get(run_by_digest))
        .route("/v1/runs/{id}", get(run_by_id))
        .route("/v1/runs/{id}/demo", get(run_demo))
        .route("/v1/players/{name}", get(player_profile))
        .fallback(not_found)
        .with_state(state)
}

async fn not_found(uri: axum::http::Uri) -> ApiError {
    ApiError {
        status: StatusCode::NOT_FOUND,
        code: "unknown_endpoint",
        detail: format!("`{}` is not a route this service serves.", uri.path()),
    }
}

// ── health and meta ─────────────────────────────────────────────────────────

/// `200` only when the database actually round-trips.
///
/// That is what makes every `503 database_unavailable` elsewhere honest: a
/// health check that reported on the process rather than on the database would
/// say "up" at exactly the moment the boards stopped being answerable.
async fn health(State(state): State<AppState>) -> ApiResult<Json<Value>> {
    crate::db::round_trips(state.pool())
        .await
        .map_err(|e| ApiError::database_unavailable(format!("the records database did not answer: {e}")))?;

    Ok(Json(json!({
        "status": "ok",
        "database": "ok",
        "sim_build": digest16::format(state.sim_build().build_hash),
        "native_verifier_ok": state.sim_build().native_verifier_ok,
    })))
}

/// What physics the client should be running, and what build is answering.
async fn meta(State(state): State<AppState>) -> ApiResult<Json<Value>> {
    let all = catalog::all_profiles(state.pool()).await?;
    let build = state.sim_build();

    // The newest row per family is the one an unpinned category resolves to.
    let mut current: std::collections::HashMap<&str, u64> = std::collections::HashMap::new();
    for profile in &all {
        current.entry(profile.kind.as_str()).or_insert(profile.digest);
    }

    Ok(Json(json!({
        "sim_build": {
            "sim_version": build.sim_version,
            "git_sha": build.git_sha,
            "build_hash": digest16::format(build.build_hash),
            "native_verifier_ok": build.native_verifier_ok,
            // Null, not a plausible number: this service does not build the
            // browser bundle (wave contracts §E3).
            "wasm_hash": Value::Null,
        },
        "demo_format_version": straf3_replay::FORMAT_VERSION,
        "default_family": profiles::DEFAULT_FAMILY,
        "profiles": all.iter().map(|p| json!({
            "family": p.kind,
            "digest": digest16::format(p.digest),
            "label": p.label,
            "layout_version": p.layout_version,
            "created_at": p.created_at,
            "current": current.get(p.kind.as_str()) == Some(&p.digest),
        })).collect::<Vec<_>>(),
        "limits": {
            "max_commands": limits::MAX_COMMANDS,
            "max_compressed_bytes": limits::MAX_COMPRESSED_BYTES,
            "max_decompressed_bytes": limits::MAX_DECOMPRESSED_BYTES,
            "attempt_ttl_ms": limits::ATTEMPT_TTL.as_millis() as u64,
        },
    })))
}

// ── maps ────────────────────────────────────────────────────────────────────

async fn list_maps(State(state): State<AppState>) -> ApiResult<Json<Value>> {
    let maps = catalog::all_maps(state.pool()).await?;
    let mut out = Vec::with_capacity(maps.len());

    for map in &maps {
        // §7.5: "list, with per-profile record times". A map nobody has run is
        // a map with an empty `categories` list of records — not an absence.
        let rows = sqlx::query(
            "select pp.kind, pp.digest, pp.label, pp.id as profile_id, \
                    e.time_ms, e.run_id, r.verified_digest, pl.display_name, \
                    (select count(*) from leaderboard_entries le \
                       join players lp on lp.id = le.player_id \
                      where le.map_id = $1 and le.profile_id = pp.id and lp.banned_at is null) as entries \
               from physics_profiles pp \
               left join lateral ( \
                    select le.time_ms, le.run_id, le.player_id \
                      from leaderboard_entries le \
                      join players lp on lp.id = le.player_id \
                     where le.map_id = $1 and le.profile_id = pp.id and lp.banned_at is null \
                     order by le.time_ms asc, le.set_at asc limit 1 \
               ) e on true \
               left join players pl on pl.id = e.player_id \
               left join runs r on r.id = e.run_id \
              order by pp.kind, pp.created_at desc, pp.id desc",
        )
        .bind(map.id)
        .fetch_all(state.pool())
        .await?;

        let categories: Vec<Value> = rows
            .iter()
            .map(|row| -> Result<Value, sqlx::Error> {
                let family: String = row.try_get("kind")?;
                let digest = digest16::from_sql(row.try_get("digest")?);
                let time_ms: Option<i32> = row.try_get("time_ms")?;
                let run_id: Option<Uuid> = row.try_get("run_id")?;
                let run_digest: Option<i64> = row.try_get("verified_digest")?;
                let display_name: Option<String> = row.try_get("display_name")?;
                let entries: i64 = row.try_get("entries")?;
                Ok(json!({
                    "family": family,
                    "digest": digest16::format(digest),
                    "label": row.try_get::<String, _>("label")?,
                    "entries": entries,
                    "record": match (time_ms, run_id, run_digest, display_name) {
                        (Some(time_ms), Some(run_id), Some(run_digest), Some(player)) => json!({
                            "time_ms": time_ms,
                            "run_id": run_id,
                            "run_digest": digest16::format(digest16::from_sql(run_digest)),
                            "player": player,
                        }),
                        // Nobody has set a time here. Explicitly null, so the
                        // site renders "no record yet" rather than guessing.
                        _ => Value::Null,
                    },
                }))
            })
            .collect::<Result<_, _>>()?;

        out.push(map_json(map, categories));
    }

    Ok(Json(json!({ "maps": out, "total": maps.len() })))
}

async fn map_detail(
    State(state): State<AppState>,
    Path(slug): Path<String>,
) -> ApiResult<Json<Value>> {
    let map = catalog::resolve_map(state.pool(), &slug).await?;
    let profiles_rows = catalog::all_profiles(state.pool()).await?;

    let categories: Vec<Value> = profiles_rows
        .iter()
        .map(|p| {
            json!({
                "family": p.kind,
                "digest": digest16::format(p.digest),
                "label": p.label,
                "key": format!("{}@{}", p.kind, digest16::format(p.digest)),
            })
        })
        .collect();

    let mut body = map_json(&map, categories);
    if let Some(object) = body.as_object_mut() {
        object.insert(
            "default_category".to_string(),
            json!(profiles::DEFAULT_FAMILY),
        );
        object.insert("leaderboard".to_string(), json!(format!("/v1/maps/{}/leaderboard", map.slug)));
    }
    Ok(Json(body))
}

fn map_json(map: &catalog::MapRow, categories: Vec<Value>) -> Value {
    json!({
        "slug": map.slug,
        "name": map.name,
        "author": map.author,
        // Both digests in the one spelling the whole platform uses.
        "collision_digest": digest16::format(map.collision_digest),
        "source_sha256": hex(&map.source_sha256),
        // No object store this wave: the key is the path the one origin serves
        // the `.map` from, which is what the browser client actually fetches.
        "source_url": map.source_key,
        "map_compiler_version": map.map_compiler_version,
        "has_start_trigger": map.has_start_trigger,
        "has_finish_trigger": map.has_finish_trigger,
        "has_timing": map.has_timing(),
        "categories": categories,
        "play": format!("/play/{}", map.slug),
    })
}

// ── leaderboards ────────────────────────────────────────────────────────────

/// `?profile=cpm` or `?profile=cpm@0123456789abcdef`, `?limit=`, `?offset=`.
#[derive(Debug, Deserialize)]
pub struct BoardQuery {
    /// The category key. Absent means the map's default family.
    #[serde(default)]
    pub profile: Option<String>,
    /// How many rows. Clamped to 200.
    #[serde(default)]
    pub limit: Option<i64>,
    /// Where to start.
    #[serde(default)]
    pub offset: Option<i64>,
}

async fn leaderboard(
    State(state): State<AppState>,
    Path(slug): Path<String>,
    Query(query): Query<BoardQuery>,
) -> ApiResult<Json<Value>> {
    let category =
        catalog::resolve_category(state.pool(), &slug, query.profile.as_deref()).await?;
    let limit = query.limit.unwrap_or(50).clamp(1, 200);
    let offset = query.offset.unwrap_or(0).max(0);

    // §5.3: rank is computed on read; boards are small. Ties break by who got
    // there first, which is both conventional and the only tiebreak that
    // cannot be gamed.
    let rows = sqlx::query(
        "select rank() over (order by e.time_ms asc, e.set_at asc) as rank, \
                p.display_name, e.time_ms, e.set_at, e.run_id, r.verified_digest \
           from leaderboard_entries e \
           join players p on p.id = e.player_id \
           join runs r on r.id = e.run_id \
          where e.map_id = $1 and e.profile_id = $2 and p.banned_at is null \
          order by e.time_ms asc, e.set_at asc \
          limit $3 offset $4",
    )
    .bind(category.map.id)
    .bind(category.profile.id)
    .bind(limit)
    .bind(offset)
    .fetch_all(state.pool())
    .await?;

    let total: i64 = sqlx::query_scalar(
        "select count(*) from leaderboard_entries e join players p on p.id = e.player_id \
          where e.map_id = $1 and e.profile_id = $2 and p.banned_at is null",
    )
    .bind(category.map.id)
    .bind(category.profile.id)
    .fetch_one(state.pool())
    .await?;

    let entries: Vec<Value> = rows
        .iter()
        .map(entry_json)
        .collect::<Result<_, _>>()?;

    // The shape r9 turns on. An empty board is this, with `entries: []` and
    // `total: 0` — a 200 that says "nobody has set a time", which is a
    // different sentence from "the service could not answer".
    Ok(Json(json!({
        "category": category.to_json(),
        "entries": entries,
        "total": total,
        "limit": limit,
        "offset": offset,
    })))
}

async fn leaderboard_me(
    State(state): State<AppState>,
    Path(slug): Path<String>,
    Query(query): Query<BoardQuery>,
    headers: HeaderMap,
) -> ApiResult<Json<Value>> {
    let player = auth::authenticate(state.pool(), state.jwks(), &headers).await?;
    let category =
        catalog::resolve_category(state.pool(), &slug, query.profile.as_deref()).await?;

    let row = sqlx::query(
        "select rank, display_name, time_ms, set_at, run_id, verified_digest from ( \
             select rank() over (order by e.time_ms asc, e.set_at asc) as rank, \
                    p.display_name, e.time_ms, e.set_at, e.run_id, r.verified_digest, e.player_id \
               from leaderboard_entries e \
               join players p on p.id = e.player_id \
               join runs r on r.id = e.run_id \
              where e.map_id = $1 and e.profile_id = $2 and p.banned_at is null \
         ) ranked where player_id = $3",
    )
    .bind(category.map.id)
    .bind(category.profile.id)
    .bind(player.id)
    .fetch_optional(state.pool())
    .await?;

    Ok(Json(json!({
        "category": category.to_json(),
        "player": player.display_name,
        // Null means "this player has not set a time here". It is not an
        // error, and it is not an empty board.
        "entry": row.as_ref().map(entry_json).transpose()?,
    })))
}

fn entry_json(row: &sqlx::postgres::PgRow) -> Result<Value, sqlx::Error> {
    let run_digest = digest16::from_sql(row.try_get("verified_digest")?);
    Ok(json!({
        "rank": row.try_get::<i64, _>("rank")?,
        "player": row.try_get::<String, _>("display_name")?,
        "time_ms": row.try_get::<i32, _>("time_ms")?,
        "set_at": row.try_get::<chrono::DateTime<chrono::Utc>, _>("set_at")?,
        "run_id": row.try_get::<Uuid, _>("run_id")?,
        "run_digest": digest16::format(run_digest),
        // The durable name (URLS.md §5): a digest link survives a restore, a
        // re-import and a service that has not been written yet.
        "watch": format!("/watch/{}", digest16::format(run_digest)),
    }))
}

// ── attempts ────────────────────────────────────────────────────────────────

/// `POST /v1/attempts` body.
#[derive(Debug, Deserialize)]
pub struct AttemptRequest {
    /// The map slug the run is about to be on.
    pub map: String,
    /// The category key, `<family>[@<digest16>]`. Absent means the default.
    #[serde(default)]
    pub profile: Option<String>,
}

async fn create_attempt(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(body): Json<AttemptRequest>,
) -> ApiResult<Json<Value>> {
    let player = auth::authenticate(state.pool(), state.jwks(), &headers).await?;
    let category =
        catalog::resolve_category(state.pool(), &body.map, body.profile.as_deref()).await?;

    // §7.3: a small cap on live unconsumed tickets, so a bulk harvest cannot
    // precede a bulk resubmission.
    let live: i64 = sqlx::query_scalar(
        "select count(*) from attempts \
          where player_id = $1 and consumed_at is null and expires_at > now()",
    )
    .bind(player.id)
    .fetch_one(state.pool())
    .await?;
    if live >= limits::MAX_LIVE_ATTEMPTS_PER_PLAYER {
        return Err(ApiError::rate_limited(format!(
            "you already hold {live} live attempt tickets. Finish or abandon one before starting \
             another."
        )));
    }

    let id = Uuid::new_v4();
    let ttl_seconds = limits::ATTEMPT_TTL.as_secs() as i64;
    let row = sqlx::query(
        "insert into attempts (id, player_id, map_id, profile_id, expires_at) \
         values ($1, $2, $3, $4, now() + make_interval(secs => $5)) returning expires_at",
    )
    .bind(id)
    .bind(player.id)
    .bind(category.map.id)
    .bind(category.profile.id)
    .bind(ttl_seconds as f64)
    .fetch_one(state.pool())
    .await?;

    Ok(Json(json!({
        "ticket": id,
        "attempt_id": id,
        "category": category.to_json(),
        "expires_at": row.try_get::<chrono::DateTime<chrono::Utc>, _>("expires_at")?,
        "ttl_ms": limits::ATTEMPT_TTL.as_millis() as u64,
    })))
}

// ── submission ──────────────────────────────────────────────────────────────

async fn submit_run(
    State(state): State<AppState>,
    headers: HeaderMap,
    body: axum::body::Bytes,
) -> ApiResult<Response> {
    let player = auth::authenticate(state.pool(), state.jwks(), &headers).await?;

    let ticket_header = headers
        .get("x-straf3-ticket")
        .ok_or_else(ApiError::missing_ticket)?
        .to_str()
        .map_err(|_| ApiError::missing_ticket())?;
    let ticket: Uuid = ticket_header
        .trim()
        .parse()
        .map_err(|_| ApiError::bad_ticket("that is not a ticket this service issued."))?;

    // §7.3's per-player rate limits, ahead of any decoding.
    enforce_submission_rate(state.pool(), &player).await?;

    let encoding = headers
        .get(header::CONTENT_ENCODING)
        .and_then(|v| v.to_str().ok());
    let submission = intake::decode_submission(&body, encoding)?;

    // The recording's own identities decide which category it belongs to. The
    // client does not get to name one: the map is looked up by the collision
    // digest the `.s3d` carries, and the profile by the physics digest.
    let world = submission.recording.world();
    let straf3_replay::WorldId::Map {
        collision_digest, ..
    } = world
    else {
        return Err(ApiError::unhonourable_recording(
            "unrankable_world",
            format!(
                "this run was made in {world}, which is not a map. Only runs on a compiled map \
                 can be ranked."
            ),
        ));
    };

    let map_row = sqlx::query(
        "select id, slug from maps where collision_digest = $1",
    )
    .bind(digest16::to_sql(*collision_digest))
    .fetch_optional(state.pool())
    .await?;
    let Some(map_row) = map_row else {
        return Err(ApiError::unhonourable_recording(
            "unknown_map",
            format!(
                "this run was made against collision geometry {}, which is not a map this service \
                 has. It is not ranked under a map that looks similar.",
                digest16::format(*collision_digest)
            ),
        ));
    };
    let map_id: i32 = map_row.try_get("id")?;

    let physics = submission.recording.physics();
    let profile_row = sqlx::query("select id from physics_profiles where digest = $1")
        .bind(digest16::to_sql(physics.digest))
        .fetch_optional(state.pool())
        .await?;
    let Some(profile_row) = profile_row else {
        return Err(ApiError::unhonourable_recording(
            "unknown_physics_digest",
            format!(
                "this run was made under physics {physics}, which this service has no profile \
                 for. It is not ranked under the nearest profile."
            ),
        ));
    };
    let profile_id: i32 = profile_row.try_get("id")?;

    // §7.2 step 1: a live, unconsumed ticket of this player's, for this
    // category.
    let attempt = sqlx::query(
        "select map_id, profile_id, consumed_at, expires_at from attempts \
          where id = $1 and player_id = $2",
    )
    .bind(ticket)
    .bind(player.id)
    .fetch_optional(state.pool())
    .await?;
    let Some(attempt) = attempt else {
        return Err(ApiError::bad_ticket(
            "that ticket was not issued to you.".to_string(),
        ));
    };
    if attempt
        .try_get::<Option<chrono::DateTime<chrono::Utc>>, _>("consumed_at")?
        .is_some()
    {
        return Err(ApiError::bad_ticket(
            "that ticket has already been used. Tickets are single use.".to_string(),
        ));
    }
    if attempt.try_get::<chrono::DateTime<chrono::Utc>, _>("expires_at")? <= chrono::Utc::now() {
        return Err(ApiError::bad_ticket(
            "that ticket has expired. Start a new attempt.".to_string(),
        ));
    }
    if attempt.try_get::<i32, _>("map_id")? != map_id
        || attempt.try_get::<i32, _>("profile_id")? != profile_id
    {
        return Err(ApiError::bad_ticket(
            "that ticket was issued for a different map or physics profile than this recording \
             was made on."
                .to_string(),
        ));
    }

    let claimed_digest = digest16::to_sql(submission.run_digest());
    let run_id = Uuid::new_v4();

    let mut tx = state.pool().begin().await?;

    // §7.2 step 3, in the shape the second migration's header argues for.
    // Ownership and idempotency were one index in §5.1 and are two questions
    // here, because the digest in the header is the *submitter's* claim: only
    // re-simulation derives the real one, and intake does not simulate.
    //
    // Idempotency, first: this player has uploaded this run before — a retry,
    // or the same run re-encoded from the compact form into the traced one.
    // Return the original rather than queueing the work again.
    let mine = sqlx::query(
        "select id from runs where player_id = $1 and claimed_digest = $2 \
          order by submitted_at asc limit 1",
    )
    .bind(player.id)
    .bind(claimed_digest)
    .fetch_optional(&mut *tx)
    .await?;

    // Ownership, second, and against the *verified* digest — the one this
    // service folded for itself. Checking the claimed digest here is what would
    // let anyone squat a record they never set (see the migration header).
    let already_ranked: Option<Uuid> =
        sqlx::query_scalar("select player_id from runs where verified_digest = $1")
            .bind(claimed_digest)
            .fetch_optional(&mut *tx)
            .await?;

    let (status_code, run_id) = match (mine, already_ranked) {
        (Some(row), _) => (StatusCode::OK, row.try_get::<Uuid, _>("id")?),
        (None, Some(owner)) if owner != player.id => {
            tx.rollback().await?;
            return Err(ApiError::run_already_submitted(&digest16::format(
                submission.run_digest(),
            )));
        }
        (None, _) => {
            sqlx::query(
                "insert into runs (id, player_id, map_id, profile_id, sim_build_id, \
                                   tick_rate_hz, commands, demo_sha256, claimed_digest, \
                                   demo_bytes_blob, demo_bytes, attempt_id, client_time_ms, \
                                   client_rolling_digest) \
                 values ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12, $13, $14)",
            )
            .bind(run_id)
            .bind(player.id)
            .bind(map_id)
            .bind(profile_id)
            .bind(state.sim_build_id())
            .bind(submission.tick_rate_hz())
            .bind(submission.commands())
            .bind(&submission.sha256)
            .bind(claimed_digest)
            .bind(&submission.bytes)
            .bind(i32::try_from(submission.bytes.len()).unwrap_or(i32::MAX))
            .bind(ticket)
            .bind(
                submission
                    .recording
                    .claimed()
                    .run_time_ms
                    .and_then(|t| i32::try_from(t).ok()),
            )
            .bind(claimed_digest)
            .execute(&mut *tx)
            .await?;
            (StatusCode::ACCEPTED, run_id)
        }
    };

    // §7.2 step 4: the ticket is consumed whichever of those happened.
    sqlx::query(
        "update attempts set consumed_at = now(), consumed_by = $2 \
          where id = $1 and consumed_at is null",
    )
    .bind(ticket)
    .bind(run_id)
    .execute(&mut *tx)
    .await?;

    tx.commit().await?;

    let body = json!({
        "run_id": run_id,
        // Null on purpose. A run's durable name is the digest this service
        // folded from the commands, and nothing has re-simulated it yet.
        "run_digest": Value::Null,
        "claimed_digest": digest16::format(submission.run_digest()),
        // Never "verified" from here, for the same reason.
        "status": "pending",
        "poll": format!("/v1/runs/{run_id}"),
    });
    Ok((status_code, Json(body)).into_response())
}

async fn enforce_submission_rate(pool: &PgPool, player: &AuthedPlayer) -> ApiResult<()> {
    let row = sqlx::query(
        "select count(*) filter (where submitted_at > now() - interval '1 minute') as last_minute, \
                count(*) filter (where submitted_at > now() - interval '1 day') as last_day \
           from runs where player_id = $1",
    )
    .bind(player.id)
    .fetch_one(pool)
    .await?;
    let recent: (i64, i64) = (row.try_get("last_minute")?, row.try_get("last_day")?);

    if recent.0 >= limits::MAX_SUBMISSIONS_PER_MINUTE {
        return Err(ApiError::rate_limited(format!(
            "at most {} submissions a minute.",
            limits::MAX_SUBMISSIONS_PER_MINUTE
        )));
    }
    if recent.1 >= limits::MAX_SUBMISSIONS_PER_DAY {
        return Err(ApiError::rate_limited(format!(
            "at most {} submissions a day.",
            limits::MAX_SUBMISSIONS_PER_DAY
        )));
    }
    Ok(())
}

// ── runs ────────────────────────────────────────────────────────────────────

const RUN_COLUMNS: &str = "r.id, r.status::text as status, r.time_ms, r.client_time_ms, \
                           r.commands, r.tick_rate_hz, r.claimed_digest, r.verified_digest, \
                           r.client_rolling_digest, \
                           r.server_rolling_digest, r.divergence_at, r.submitted_at, \
                           r.verified_at, r.reject_reason, r.demo_bytes, \
                           m.slug as map_slug, m.name as map_name, m.collision_digest, \
                           pp.kind as family, pp.digest as physics_digest, pp.label as \
                           physics_label, p.display_name, sb.build_hash, sb.native_verifier_ok";

const RUN_JOINS: &str = "from runs r \
                         join maps m on m.id = r.map_id \
                         join physics_profiles pp on pp.id = r.profile_id \
                         join players p on p.id = r.player_id \
                         join sim_builds sb on sb.id = r.sim_build_id";

async fn run_by_id(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> ApiResult<Json<Value>> {
    let uuid: Uuid = id.parse().map_err(|_| ApiError::unknown_run(&id))?;
    let row = sqlx::query(&format!("select {RUN_COLUMNS} {RUN_JOINS} where r.id = $1"))
        .bind(uuid)
        .fetch_optional(state.pool())
        .await?
        .ok_or_else(|| ApiError::unknown_run(&id))?;
    Ok(Json(run_json(&row)?))
}

/// URLS.md §5. The same body as `GET /v1/runs/:id`, reached by the run's own
/// durable name — the rolling digest, which is computable from the file alone
/// with no service and no database.
async fn run_by_digest(
    State(state): State<AppState>,
    Path(digest): Path<String>,
) -> ApiResult<Json<Value>> {
    let parsed = digest16::parse(&digest).ok_or_else(|| ApiError::unknown_run(&digest))?;
    let row = sqlx::query(&format!(
        "select {RUN_COLUMNS} {RUN_JOINS} where r.verified_digest = $1"
    ))
    .bind(digest16::to_sql(parsed))
    .fetch_optional(state.pool())
    .await?
    .ok_or_else(|| ApiError::unknown_run(&digest))?;
    Ok(Json(run_json(&row)?))
}

fn run_json(row: &sqlx::postgres::PgRow) -> Result<Value, sqlx::Error> {
    let id: Uuid = row.try_get("id")?;
    // The run's durable name is the digest this service folded for itself, so
    // it is null until this service has folded one. What the file claimed is
    // kept alongside it, under `diagnostics`, where it cannot be mistaken for
    // the same thing.
    let verified_digest = row
        .try_get::<Option<i64>, _>("verified_digest")?
        .map(|d| digest16::format(digest16::from_sql(d)));
    let status: String = row.try_get("status")?;
    let verified = status == "verified";

    Ok(json!({
        "run_id": id,
        "run_digest": verified_digest,
        "status": status,
        // SERVER-COMPUTED, and null unless this service re-simulated the run
        // and agreed with it.
        "time_ms": row.try_get::<Option<i32>, _>("time_ms")?,
        "commands": row.try_get::<i32, _>("commands")?,
        "tick_rate_hz": row.try_get::<i16, _>("tick_rate_hz")?,
        "map": {
            "slug": row.try_get::<String, _>("map_slug")?,
            "name": row.try_get::<String, _>("map_name")?,
            "collision_digest": digest16::format(digest16::from_sql(row.try_get("collision_digest")?)),
        },
        "category": {
            "map": row.try_get::<String, _>("map_slug")?,
            "family": row.try_get::<String, _>("family")?,
            "digest": digest16::format(digest16::from_sql(row.try_get("physics_digest")?)),
            "label": row.try_get::<String, _>("physics_label")?,
            "key": format!(
                "{}@{}",
                row.try_get::<String, _>("family")?,
                digest16::format(digest16::from_sql(row.try_get("physics_digest")?))
            ),
        },
        "player": { "display_name": row.try_get::<String, _>("display_name")? },
        "submitted_at": row.try_get::<chrono::DateTime<chrono::Utc>, _>("submitted_at")?,
        "verified_at": row.try_get::<Option<chrono::DateTime<chrono::Utc>>, _>("verified_at")?,
        "reject_reason": row.try_get::<Option<String>, _>("reject_reason")?,
        "demo_bytes": row.try_get::<i32, _>("demo_bytes")?,
        // Available only once the run is verified and ranked (§8.3), so the
        // link is null until then rather than 404ing after a click.
        "demo": verified.then(|| format!("/v1/runs/{id}/demo")),
        "watch": verified_digest.as_ref().map(|d| format!("/watch/{d}")),
        // The diagnostics §5.1 keeps, grouped so nothing here can be mistaken
        // for the ranked result. `client_time_ms` is what the recording
        // claimed; it is never the time on a board.
        "diagnostics": {
            "client_time_ms": row.try_get::<Option<i32>, _>("client_time_ms")?,
            "claimed_digest": digest16::format(digest16::from_sql(row.try_get("claimed_digest")?)),
            "client_rolling_digest": row.try_get::<Option<i64>, _>("client_rolling_digest")?
                .map(|d| digest16::format(digest16::from_sql(d))),
            "server_rolling_digest": row.try_get::<Option<i64>, _>("server_rolling_digest")?
                .map(|d| digest16::format(digest16::from_sql(d))),
            "divergence_at": row.try_get::<Option<i32>, _>("divergence_at")?,
            "sim_build": digest16::format(digest16::from_sql(row.try_get("build_hash")?)),
            "native_verifier_ok": row.try_get::<bool, _>("native_verifier_ok")?,
        },
    }))
}

/// The `.s3d`, for ghosts and playback.
///
/// Unauthenticated, but only once the run is verified and ranked (§7.5, §8.3):
/// serving a pending run's bytes would publish a recording before anything had
/// agreed it describes a real run.
async fn run_demo(State(state): State<AppState>, Path(id): Path<String>) -> ApiResult<Response> {
    let uuid: Uuid = id.parse().map_err(|_| ApiError::unknown_run(&id))?;
    let row = sqlx::query(
        "select status::text as status, verified_digest, demo_bytes_blob from runs where id = $1",
    )
    .bind(uuid)
    .fetch_optional(state.pool())
    .await?
    .ok_or_else(|| ApiError::unknown_run(&id))?;

    let status: String = row.try_get("status")?;
    if status != "verified" {
        return Err(ApiError {
            status: StatusCode::CONFLICT,
            code: "run_not_verified",
            detail: format!(
                "this run is `{status}`. A recording is served once it has been re-simulated and \
                 agreed with, not before."
            ),
        });
    }

    let bytes: Vec<u8> = row.try_get("demo_bytes_blob")?;
    let digest = digest16::from_sql(row.try_get::<Option<i64>, _>("verified_digest")?.unwrap_or(0));
    Ok((
        [
            (header::CONTENT_TYPE, "application/vnd.straf3.demo".to_string()),
            (
                header::CONTENT_DISPOSITION,
                format!("inline; filename=\"{}.s3d\"", digest16::format(digest)),
            ),
            (header::CACHE_CONTROL, "public, max-age=31536000, immutable".to_string()),
        ],
        bytes,
    )
        .into_response())
}

// ── players ─────────────────────────────────────────────────────────────────

async fn player_profile(
    State(state): State<AppState>,
    Path(name): Path<String>,
) -> ApiResult<Json<Value>> {
    let row = sqlx::query(
        "select id, display_name, created_at, country from players \
          where lower(display_name) = lower($1) and banned_at is null",
    )
    .bind(&name)
    .fetch_optional(state.pool())
    .await?
    .ok_or_else(|| ApiError {
        status: StatusCode::NOT_FOUND,
        code: "unknown_player",
        detail: format!("no player is called `{name}`."),
    })?;

    let id: Uuid = row.try_get("id")?;

    let bests = sqlx::query(
        "select m.slug, pp.kind as family, pp.digest as physics_digest, pp.label, \
                e.time_ms, e.set_at, e.run_id, r.verified_digest, \
                (select count(*) from leaderboard_entries o \
                   join players op on op.id = o.player_id \
                  where o.map_id = e.map_id and o.profile_id = e.profile_id \
                    and op.banned_at is null \
                    and (o.time_ms < e.time_ms or (o.time_ms = e.time_ms and o.set_at < e.set_at)) \
                ) + 1 as rank \
           from leaderboard_entries e \
           join maps m on m.id = e.map_id \
           join physics_profiles pp on pp.id = e.profile_id \
           join runs r on r.id = e.run_id \
          where e.player_id = $1 \
          order by m.slug, pp.kind",
    )
    .bind(id)
    .fetch_all(state.pool())
    .await?;

    let personal_bests: Vec<Value> = bests
        .iter()
        .map(|row| -> Result<Value, sqlx::Error> {
            let run_digest = digest16::from_sql(row.try_get("verified_digest")?);
            let family: String = row.try_get("family")?;
            let physics = digest16::format(digest16::from_sql(row.try_get("physics_digest")?));
            Ok(json!({
                "map": row.try_get::<String, _>("slug")?,
                "category": { "family": family, "digest": physics, "label": row.try_get::<String, _>("label")? },
                "time_ms": row.try_get::<i32, _>("time_ms")?,
                "set_at": row.try_get::<chrono::DateTime<chrono::Utc>, _>("set_at")?,
                "rank": row.try_get::<i64, _>("rank")?,
                "run_id": row.try_get::<Uuid, _>("run_id")?,
                "run_digest": digest16::format(run_digest),
                "watch": format!("/watch/{}", digest16::format(run_digest)),
            }))
        })
        .collect::<Result<_, _>>()?;

    let counts_row = sqlx::query(
        "select count(*) as total, count(*) filter (where status = 'verified') as verified \
           from runs where player_id = $1",
    )
    .bind(id)
    .fetch_one(state.pool())
    .await?;
    let counts: (i64, i64) = (
        counts_row.try_get("total")?,
        counts_row.try_get("verified")?,
    );

    let records_held: i64 = sqlx::query_scalar(
        "select count(*) from record_history where held_until is null and run_id in \
           (select id from runs where player_id = $1)",
    )
    .bind(id)
    .fetch_one(state.pool())
    .await?;

    Ok(Json(json!({
        "display_name": row.try_get::<String, _>("display_name")?,
        "created_at": row.try_get::<chrono::DateTime<chrono::Utc>, _>("created_at")?,
        "country": row.try_get::<Option<String>, _>("country")?,
        // Empty because this player has set no times — a fact, not a failure.
        "personal_bests": personal_bests,
        "records_held": records_held,
        "runs": { "total": counts.0, "verified": counts.1 },
    })))
}

fn hex(bytes: &[u8]) -> String {
    use std::fmt::Write as _;
    bytes.iter().fold(String::with_capacity(bytes.len() * 2), |mut out, b| {
        let _ = write!(out, "{b:02x}");
        out
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// URLS.md §6: `/v1` never overlaps a site route. Building the router is
    /// what proves the paths do not conflict with each other — axum panics on
    /// an ambiguous insert — and `tests/api.rs` proves that
    /// `/v1/runs/by-digest/<16 hex>` and `/v1/runs/<uuid>` reach *different*
    /// handlers, which needs a live request and so lives there.
    #[test]
    fn every_v1_path_is_distinct() {
        let _ = paths();
        assert!(paths().iter().all(|p| p.starts_with("/v1/")));
        let mut sorted = paths();
        sorted.sort_unstable();
        let before = sorted.len();
        sorted.dedup();
        assert_eq!(before, sorted.len(), "a path is declared twice");
    }

    #[test]
    fn hex_is_lowercase_and_padded() {
        assert_eq!(hex(&[0x00, 0x0f, 0xff]), "000fff");
    }
}
