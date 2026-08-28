//! Shared scaffolding for the integration tests.
//!
//! # Why these run against the real database
//!
//! Every claim these tests make is about SQL — a partial index deciding a
//! conflict, a `rank()` window, a trigger refusing an update, a `left join
//! lateral` returning null rather than nothing. A fake would be a second
//! implementation of the thing under test, and it would agree with itself.
//!
//! So each test gets its **own Postgres schema** on the Neon `straf3` database,
//! created, migrated and dropped around it. `search_path` isolation means the
//! migration runs verbatim — same DDL, same triggers, same index definitions —
//! without touching `public`, and certainly without touching `neon_auth`.
//!
//! Tests skip, loudly, when `DATABASE_URL` is absent. They never invent a
//! connection string and the credential never appears in this file.

#![allow(dead_code)]

pub mod fixture;

use std::sync::atomic::{AtomicU32, Ordering};

use axum::Router;
use axum::body::Body;
use axum::http::{Request, StatusCode};
use http_body_util::BodyExt;
use serde_json::Value;
use sqlx::postgres::PgPoolOptions;
use sqlx::{Executor, PgPool};
use straf3_records::auth::Jwks;
use straf3_records::config::Config;
use straf3_records::simbuild::SimBuild;
use straf3_records::{AppState, db, routes};
use tower::ServiceExt;
use uuid::Uuid;

/// A migrated, isolated schema plus a router over it.
pub struct Harness {
    pub pool: PgPool,
    pub app: Router,
    pub schema: String,
}

static SEQ: AtomicU32 = AtomicU32::new(0);

/// The connection string these tests use, or `None` when the environment has
/// not supplied one.
///
/// # Why this is not `DATABASE_URL` verbatim
///
/// `DATABASE_URL` names Neon's **pooled** endpoint — PgBouncer in
/// transaction-pooling mode. That is the right endpoint for the service, which
/// runs one statement at a time and holds no session state. These tests hold
/// session state on purpose: each one lives in its own schema, reached by `set
/// search_path`, and a transaction-mode pooler is free to hand the next
/// transaction a backend that never received it. So they use the direct
/// endpoint, which is the same host with `-pooler` removed.
///
/// **It is worth being precise about what that does and does not fix, because
/// the first run of this suite hung and it was tempting to blame the pooler.**
/// It was not the pooler. `main` held `pg_advisory_lock(999)` on one pooled
/// session and had a second pooled session's `pg_try_advisory_lock(999)`
/// correctly return false — advisory locks work through `-pooler`. The hang was
/// this suite's own concurrency: under `--test-threads=4`, a test held the
/// migrator's advisory lock while waiting for a second connection from a pool
/// every other parallel test had already drained. [`Harness::with_jwks`]
/// migrates on a dedicated connection now, so the lock never spans a pool
/// acquisition, which is the actual fix.
///
/// Override with `STRAF3_TEST_DATABASE_URL` if a deployment spells the direct
/// endpoint differently.
pub fn database_url() -> Option<String> {
    if let Some(explicit) = std::env::var("STRAF3_TEST_DATABASE_URL")
        .ok()
        .filter(|u| !u.is_empty())
    {
        return Some(explicit);
    }
    std::env::var("DATABASE_URL")
        .ok()
        .filter(|u| !u.is_empty())
        .map(|url| url.replace("-pooler.", "."))
}

/// Skip the calling test, saying so, when there is no database.
#[macro_export]
macro_rules! require_database {
    () => {
        match $crate::support::database_url() {
            Some(url) => url,
            None => {
                eprintln!(
                    "SKIPPED: DATABASE_URL is not set. These tests assert about SQL and will not \
                     pretend to pass without a database."
                );
                return;
            }
        }
    };
}

impl Harness {
    /// Build one. `label` only has to be unique enough to read in a log.
    pub async fn new(label: &str) -> Self {
        Self::with_jwks(label, Jwks::new(None, None)).await
    }

    /// As [`Self::new`], with a key set already loaded.
    pub async fn with_jwks(label: &str, jwks: Jwks) -> Self {
        let url = database_url().expect("require_database! should have skipped");
        let schema = format!(
            "t_{}_{}_{}",
            label,
            std::process::id(),
            SEQ.fetch_add(1, Ordering::Relaxed)
        );

        let admin = PgPool::connect(&url).await.expect("connect");
        admin
            .execute(format!("drop schema if exists {schema} cascade").as_str())
            .await
            .expect("drop schema");
        admin
            .execute(format!("create schema {schema}").as_str())
            .await
            .expect("create schema");
        admin.close().await;

        // Migrate on a connection of its own, outside the pool. `sqlx`'s
        // migrator takes a session-scoped `pg_advisory_lock` and holds it for
        // the whole run; doing that from inside a pool means a test can be
        // holding the lock while waiting for a connection that a *different*
        // test — also waiting on the lock — is holding. That is a deadlock, and
        // it is what wedged the first run of this suite for ten minutes at zero
        // CPU. Off the pool, the lock cannot span a pool acquisition.
        {
            use sqlx::Connection as _;
            let mut conn = sqlx::PgConnection::connect(&url).await.expect("migrator conn");
            conn.execute(format!("set search_path to {schema}").as_str())
                .await
                .expect("search_path");
            db::MIGRATIONS.run(&mut conn).await.expect("migrate");
            conn.close().await.expect("close migrator conn");
        }

        let owned = schema.clone();
        let pool = PgPoolOptions::new()
            .max_connections(4)
            .after_connect(move |conn, _| {
                let schema = owned.clone();
                Box::pin(async move {
                    // Isolation is `search_path`, so the migration ran verbatim
                    // rather than through a rewriter.
                    conn.execute(format!("set search_path to {schema}").as_str())
                        .await?;
                    Ok(())
                })
            })
            .connect(&url)
            .await
            .expect("pool");

        let app = Self::router_for(pool.clone(), jwks);
        Self { pool, app, schema }
    }

    /// A whole service whose database is not there.
    ///
    /// The pool is built lazily against a closed port, so every query fails the
    /// way a real outage fails — at connect — while every other part of the
    /// service is the real one. This is the only honest way to test r9's
    /// "could not answer" half: stubbing the handler would test the stub.
    pub fn unreachable_database() -> Self {
        let pool = PgPoolOptions::new()
            .acquire_timeout(std::time::Duration::from_secs(2))
            .connect_lazy("postgres://nobody:nothing@127.0.0.1:1/nowhere")
            .expect("a lazy pool does not connect");
        let app = Self::router_for(pool.clone(), Jwks::new(None, None));
        Self {
            pool,
            app,
            schema: String::new(),
        }
    }

    fn router_for(pool: PgPool, jwks: Jwks) -> Router {
        let build = SimBuild::derive();
        let config = Config {
            database_url: String::new(),
            addr: "127.0.0.1:0".parse().expect("a literal address"),
            jwks_url: None,
            issuer: None,
            maps_dir: maps_dir(),
            maps_url_prefix: "/assets/maps".to_string(),
        };
        // `sim_build_id` is filled by `seed()`; 1 is what the seed produces on
        // an empty schema and the tests that submit call `seed()` first.
        routes::router(AppState::new(pool, jwks, config, build, 1))
    }

    /// Run the derived seed: profiles, this build, and every map in
    /// `assets/maps`.
    pub async fn seed(&self) -> straf3_records::seed::SeedReport {
        straf3_records::seed::seed_all(&self.pool, &maps_dir(), "/assets/maps")
            .await
            .expect("seed")
    }

    /// `GET`, returning status and parsed JSON.
    pub async fn get(&self, path: &str) -> (StatusCode, Value) {
        self.send(Request::get(path).body(Body::empty()).expect("request"))
            .await
    }

    /// `GET` with a bearer token.
    pub async fn get_auth(&self, path: &str, token: &str) -> (StatusCode, Value) {
        self.send(
            Request::get(path)
                .header("authorization", format!("Bearer {token}"))
                .body(Body::empty())
                .expect("request"),
        )
        .await
    }

    /// `POST` a JSON body with a bearer token.
    pub async fn post_json(&self, path: &str, token: &str, body: Value) -> (StatusCode, Value) {
        self.send(
            Request::post(path)
                .header("authorization", format!("Bearer {token}"))
                .header("content-type", "application/json")
                .body(Body::from(body.to_string()))
                .expect("request"),
        )
        .await
    }

    /// `POST` raw `.s3d` bytes with a bearer token and a ticket.
    pub async fn post_demo(
        &self,
        token: &str,
        ticket: Uuid,
        bytes: Vec<u8>,
    ) -> (StatusCode, Value) {
        self.send(
            Request::post("/v1/runs")
                .header("authorization", format!("Bearer {token}"))
                .header("x-straf3-ticket", ticket.to_string())
                .header("content-type", "application/vnd.straf3.demo")
                .body(Body::from(bytes))
                .expect("request"),
        )
        .await
    }

    /// Send a raw request and read the body as bytes.
    pub async fn raw(&self, request: Request<Body>) -> (StatusCode, Vec<u8>) {
        let response = self
            .app
            .clone()
            .oneshot(request)
            .await
            .expect("the router answers");
        let status = response.status();
        let bytes = response
            .into_body()
            .collect()
            .await
            .expect("body")
            .to_bytes()
            .to_vec();
        (status, bytes)
    }

    async fn send(&self, request: Request<Body>) -> (StatusCode, Value) {
        let (status, bytes) = self.raw(request).await;
        let value = if bytes.is_empty() {
            Value::Null
        } else {
            serde_json::from_slice(&bytes).unwrap_or_else(|e| {
                panic!(
                    "every /v1 response is JSON; this one is not ({e}): {}",
                    String::from_utf8_lossy(&bytes)
                )
            })
        };
        (status, value)
    }

    /// Drop the schema. Called explicitly, so a failing test leaves its schema
    /// behind for inspection.
    pub async fn cleanup(self) {
        let schema = self.schema.clone();
        let _ = self
            .pool
            .execute(format!("drop schema if exists {schema} cascade").as_str())
            .await;
        self.pool.close().await;
    }
}

/// The repository's `assets/maps`, read-only.
pub fn maps_dir() -> std::path::PathBuf {
    std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join("..")
        .join("assets")
        .join("maps")
}

/// Insert a player and their Neon Auth identity directly.
///
/// Used by the tests that are about SQL rather than about tokens; the tests
/// that are about tokens go through `auth::authenticate` and mint a real one.
pub async fn insert_player(pool: &PgPool, subject: &str, display_name: &str) -> Uuid {
    let id = Uuid::new_v4();
    sqlx::query("insert into players (id, display_name) values ($1, $2)")
        .bind(id)
        .bind(display_name)
        .execute(pool)
        .await
        .expect("insert player");
    sqlx::query(
        "insert into identities (provider, provider_user_id, player_id) \
         values ('neon-auth', $1, $2)",
    )
    .bind(subject)
    .bind(id)
    .execute(pool)
    .await
    .expect("insert identity");
    id
}
