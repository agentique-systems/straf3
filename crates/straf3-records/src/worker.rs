//! The verifier's work, in the library rather than in the binary.
//!
//! `straf3-records-verifier` is a thin `main` over this module. The work lives
//! here for one reason: the integration tests must drive **the** verdict, not a
//! copy of it. A test that re-implemented "claim a pending row, re-simulate,
//! write the verdict" would be asserting about its own second implementation,
//! and the two could drift in exactly the place that matters.
//!
//! ARCHITECTURE §7.1: the queue is `runs where status = 'pending'`, claimed with
//! `for update skip locked`. §4.3 puts the sustained rate in the single digits
//! per second, so there is no throughput case for anything more elaborate.

use std::collections::HashMap;
use std::time::Duration;

use sqlx::{PgPool, Row};
use straf3_map::CompiledMap;
use straf3_replay::{Recording, WorldId};
use uuid::Uuid;

use crate::verify::{RunStatus, Verdict, verify_against};
use crate::{catalog, db, digest16, limits};

/// Compiled maps, keyed by collision digest (§7.2 step 3: "cached; keyed by
/// `collision_digest`").
pub struct MapCache {
    dir: std::path::PathBuf,
    by_digest: HashMap<u64, (String, CompiledMap)>,
}

impl MapCache {
    /// Compile from this directory, on demand.
    #[must_use]
    pub fn new(dir: impl Into<std::path::PathBuf>) -> Self {
        Self {
            dir: dir.into(),
            by_digest: HashMap::new(),
        }
    }

    /// Compile every map in the directory once, and look one up by the digest
    /// a recording names.
    ///
    /// A recording whose digest matches nothing here is refused — never
    /// matched to a map by name, because a map's identity is its compiled
    /// hulls and not its file (`straf3-replay::identity`).
    pub fn get(&mut self, collision_digest: u64) -> Option<&(String, CompiledMap)> {
        if !self.by_digest.contains_key(&collision_digest) {
            self.reload();
        }
        self.by_digest.get(&collision_digest)
    }

    fn reload(&mut self) {
        let Ok(entries) = std::fs::read_dir(&self.dir) else {
            log::error!("maps directory {} is unreadable", self.dir.display());
            return;
        };
        for path in entries
            .filter_map(Result::ok)
            .map(|e| e.path())
            .filter(|p| p.extension().is_some_and(|e| e == "map"))
        {
            let Some(slug) = path
                .file_stem()
                .and_then(|s| s.to_str())
                .map(str::to_string)
            else {
                continue;
            };
            if !catalog::is_valid_slug(&slug) {
                continue;
            }
            let Ok(source) = std::fs::read_to_string(&path) else {
                continue;
            };
            match straf3_map::compile(&source) {
                Ok(compiled) => {
                    self.by_digest
                        .insert(compiled.collision_digest(), (slug, compiled));
                }
                Err(e) => log::error!("{}: {e}", path.display()),
            }
        }
    }
}

/// Claim one pending run and write its verdict. `Ok(None)` when the queue is
/// empty.
pub async fn claim_and_verify(
    pool: &PgPool,
    maps: &mut MapCache,
) -> Result<Option<Uuid>, sqlx::Error> {
    let mut tx = pool.begin().await?;

    // `skip locked` is what lets several verifiers run without coordinating.
    let claimed = sqlx::query(
        "select id, map_id, profile_id, demo_bytes_blob from runs \
          where status = 'pending' order by submitted_at asc \
          for update skip locked limit 1",
    )
    .fetch_optional(&mut *tx)
    .await?;

    let Some(row) = claimed else {
        tx.rollback().await?;
        return Ok(None);
    };

    let id: Uuid = row.try_get("id")?;
    let map_id: i32 = row.try_get("map_id")?;
    let profile_id: i32 = row.try_get("profile_id")?;
    let bytes: Vec<u8> = row.try_get("demo_bytes_blob")?;

    let verdict = adjudicate(maps, &bytes);
    write_verdict(&mut tx, id, map_id, profile_id, &verdict).await?;
    tx.commit().await?;

    if verdict.status != RunStatus::Verified {
        log::warn!(
            "run {id}: {} — {}",
            verdict.status.as_str(),
            verdict.reject_reason.as_deref().unwrap_or("")
        );
    }
    Ok(Some(id))
}

/// Decode and re-simulate, entirely in this function, so every path out of it
/// is a verdict rather than an exception.
pub fn adjudicate(maps: &mut MapCache, bytes: &[u8]) -> Verdict {
    let recording = match Recording::from_bytes(bytes) {
        Ok(recording) => recording,
        Err(e) => {
            return Verdict {
                status: RunStatus::Error,
                time_ms: None,
                client_time_ms: None,
                client_rolling_digest: 0,
                server_rolling_digest: None,
                divergence_at: None,
                reject_reason: Some(format!("the stored bytes no longer decode: {e}")),
                elapsed: Duration::ZERO,
            };
        }
    };

    let claimed = recording.claimed();
    let refuse = |reason: String| Verdict {
        status: RunStatus::Rejected,
        time_ms: None,
        client_time_ms: claimed.run_time_ms.and_then(|t| i32::try_from(t).ok()),
        client_rolling_digest: claimed.digest,
        server_rolling_digest: None,
        divergence_at: None,
        reject_reason: Some(reason),
        elapsed: Duration::ZERO,
    };

    let WorldId::Map {
        collision_digest, ..
    } = recording.world()
    else {
        return refuse(format!(
            "this run was made in {}, which is not a compiled map.",
            recording.world()
        ));
    };
    let collision_digest = *collision_digest;

    let Some((slug, compiled)) = maps.get(collision_digest) else {
        return refuse(format!(
            "no map this verifier can compile has collision digest {}. The run is not \
             re-simulated against a map that looks similar.",
            digest16::format(collision_digest)
        ));
    };
    let world_id = WorldId::map(slug.clone(), compiled.collision_digest());

    // The bound §7.3 asks for. A 150,000-command run is a few tens of
    // milliseconds, so a breach means something pathological rather than
    // something big — `verify_with_profile` records the elapsed time and the
    // verdict says `error` when it is past the deadline.
    let verdict = verify_against(&recording, compiled, &world_id);
    if verdict.elapsed > limits::VERIFY_DEADLINE {
        log::error!(
            "a verification took {:?}, past the {:?} deadline",
            verdict.elapsed,
            limits::VERIFY_DEADLINE
        );
    }
    verdict
}

/// Write the verdict, and — on a verified finish — the leaderboard rows §7.2
/// step 7 asks for.
pub async fn write_verdict(
    tx: &mut db::Tx<'_>,
    id: Uuid,
    map_id: i32,
    profile_id: i32,
    verdict: &Verdict,
) -> Result<(), sqlx::Error> {
    // `verified_digest` is claimed only when this service re-simulated the run
    // and the fold agreed. That is what makes the partial unique index on it an
    // ownership rule rather than a squatting opportunity: a run that never
    // verifies never owns a digest, so it can never block the player who
    // actually performs that run.
    let agreed = matches!(
        verdict.status,
        RunStatus::Verified | RunStatus::DidNotFinish
    );
    let verified_digest = agreed.then_some(verdict.server_rolling_digest).flatten();

    // A savepoint, because the collision below is a *verdict*, not an error:
    // the losing writer has to be able to record it in the same transaction.
    // Deliberately no `select` first — a check-then-write here is a
    // time-of-check/time-of-use race between two verifiers holding the same
    // run's two rows. The constraint is the mechanism.
    sqlx::query("savepoint verdict").execute(&mut **tx).await?;

    let written = write_row(tx, id, verdict, verified_digest.map(digest16::to_sql)).await;

    match written {
        Ok(()) => {}
        Err(e) if is_unique_violation(&e) => {
            sqlx::query("rollback to savepoint verdict")
                .execute(&mut **tx)
                .await?;
            let duplicate = Verdict {
                status: RunStatus::Rejected,
                time_ms: None,
                reject_reason: Some(format!(
                    "this run was already verified under digest {}. A run belongs to whoever \
                     was verified with it first.",
                    digest16::format(verdict.server_rolling_digest.unwrap_or_default())
                )),
                ..verdict.clone()
            };
            // No `verified_digest`: the row that lost does not own the number.
            write_row(tx, id, &duplicate, None).await?;
            return Ok(());
        }
        Err(e) => return Err(e),
    }

    let (RunStatus::Verified, Some(time_ms)) = (verdict.status, verdict.time_ms) else {
        return Ok(());
    };

    let player_id: Uuid = sqlx::query_scalar("select player_id from runs where id = $1")
        .bind(id)
        .fetch_one(&mut **tx)
        .await?;

    // §7.2 step 7: upsert the personal best only when it beats the existing
    // one. `leaderboard_entries` is derived and rebuildable from `runs` alone,
    // which is what makes a physics change recoverable (§5.4).
    sqlx::query(
        "insert into leaderboard_entries (map_id, profile_id, player_id, run_id, time_ms, set_at) \
         values ($1, $2, $3, $4, $5, now()) \
         on conflict (map_id, profile_id, player_id) do update set \
             run_id = excluded.run_id, time_ms = excluded.time_ms, set_at = excluded.set_at \
         where excluded.time_ms < leaderboard_entries.time_ms",
    )
    .bind(map_id)
    .bind(profile_id)
    .bind(player_id)
    .bind(id)
    .bind(time_ms)
    .execute(&mut **tx)
    .await?;

    update_record_history(tx, map_id, profile_id).await
}

/// The verdict write itself, so the duplicate path can reuse it verbatim.
async fn write_row(
    tx: &mut db::Tx<'_>,
    id: Uuid,
    verdict: &Verdict,
    verified_digest: Option<i64>,
) -> Result<(), sqlx::Error> {
    sqlx::query(
        "update runs set status = $2::run_status, time_ms = $3, client_time_ms = $4, \
                client_rolling_digest = $5, server_rolling_digest = $6, divergence_at = $7, \
                reject_reason = $8, verified_digest = $9, verified_at = now() \
          where id = $1",
    )
    .bind(id)
    .bind(verdict.status.as_str())
    .bind(verdict.time_ms)
    .bind(verdict.client_time_ms)
    .bind(digest16::to_sql(verdict.client_rolling_digest))
    .bind(verdict.server_rolling_digest.map(digest16::to_sql))
    .bind(verdict.divergence_at)
    .bind(verdict.reject_reason.as_deref())
    .bind(verified_digest)
    .execute(&mut **tx)
    .await
    .map(|_| ())
}

/// Whether this is the partial unique index on `verified_digest` firing.
fn is_unique_violation(error: &sqlx::Error) -> bool {
    matches!(
        error,
        sqlx::Error::Database(e) if e.code().as_deref() == Some("23505")
    )
}

/// Keep `record_history` describing who holds first place, so a record
/// survives being beaten (§5.1).
async fn update_record_history(
    tx: &mut db::Tx<'_>,
    map_id: i32,
    profile_id: i32,
) -> Result<(), sqlx::Error> {
    let best = sqlx::query(
        "select e.run_id, e.time_ms from leaderboard_entries e \
           join players p on p.id = e.player_id \
          where e.map_id = $1 and e.profile_id = $2 and p.banned_at is null \
          order by e.time_ms asc, e.set_at asc limit 1",
    )
    .bind(map_id)
    .bind(profile_id)
    .fetch_optional(&mut **tx)
    .await?;

    let Some(best) = best else { return Ok(()) };
    let run_id: Uuid = best.try_get("run_id")?;
    let time_ms: i32 = best.try_get("time_ms")?;

    let current: Option<Uuid> = sqlx::query_scalar(
        "select run_id from record_history \
          where map_id = $1 and profile_id = $2 and held_until is null",
    )
    .bind(map_id)
    .bind(profile_id)
    .fetch_optional(&mut **tx)
    .await?;

    if current == Some(run_id) {
        return Ok(());
    }

    sqlx::query(
        "update record_history set held_until = now() \
          where map_id = $1 and profile_id = $2 and held_until is null",
    )
    .bind(map_id)
    .bind(profile_id)
    .execute(&mut **tx)
    .await?;

    sqlx::query(
        "insert into record_history (map_id, profile_id, run_id, time_ms, held_from) \
         values ($1, $2, $3, $4, now()) \
         on conflict (map_id, profile_id, run_id) do update set held_until = null",
    )
    .bind(map_id)
    .bind(profile_id)
    .bind(run_id)
    .bind(time_ms)
    .execute(&mut **tx)
    .await?;

    Ok(())
}
