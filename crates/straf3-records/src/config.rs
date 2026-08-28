//! Everything the service is told at startup, and nothing it is told twice.
//!
//! # Credentials
//!
//! `DATABASE_URL` and the Neon Auth URLs live only in the operator's gitignored
//! `.env`. Nothing in this module prints one, and [`Config::describe`] exists
//! so that a startup log line can say what was configured without saying what
//! it was configured *to*.

use std::net::SocketAddr;
use std::path::PathBuf;
use std::time::Duration;

/// The port `web/dev/serve.mjs` proxies `/v1` to.
///
/// Agreed with the `site` seat through the coordinator: the one origin is
/// `http://localhost:8787`, and the records service sits behind it on 8788 and
/// is never addressed by a page directly.
pub const DEFAULT_ADDR: &str = "127.0.0.1:8788";

/// How long a cached JWKS is trusted before a scheduled re-fetch.
pub const JWKS_TTL: Duration = Duration::from_secs(15 * 60);

/// The floor between two JWKS re-fetches triggered by an unknown `kid`.
///
/// Re-fetching on an unknown key id is what makes rotation work without a
/// restart. Rate limiting it is what stops a forged `kid` from turning this
/// service into a request amplifier pointed at the auth endpoint.
pub const JWKS_MIN_REFETCH_INTERVAL: Duration = Duration::from_secs(30);

/// Startup configuration.
#[derive(Debug, Clone)]
pub struct Config {
    /// The Neon pooled connection string. Never logged.
    pub database_url: String,
    /// Where the API listens. `STRAF3_RECORDS_ADDR`, else [`DEFAULT_ADDR`].
    pub addr: SocketAddr,
    /// `NEON_AUTH_JWKS_URL`. Absent means bearer tokens cannot be verified and
    /// every authenticated route refuses — which is the honest behaviour, not
    /// a reason to accept unverified ones.
    pub jwks_url: Option<String>,
    /// The `iss` a token must carry, when one is configured.
    pub issuer: Option<String>,
    /// Where `.map` sources are read from when seeding.
    pub maps_dir: PathBuf,
    /// The path prefix the one origin serves maps under.
    pub maps_url_prefix: String,
}

impl Config {
    /// Read the environment.
    ///
    /// # Errors
    ///
    /// When `DATABASE_URL` is absent or `STRAF3_RECORDS_ADDR` does not parse.
    pub fn from_env() -> Result<Self, String> {
        let database_url = std::env::var("DATABASE_URL")
            .map_err(|_| "DATABASE_URL is not set. It lives in the gitignored `.env`.".to_string())
            .map(|url| direct_endpoint(&url))?;

        let addr_text =
            std::env::var("STRAF3_RECORDS_ADDR").unwrap_or_else(|_| DEFAULT_ADDR.to_string());
        let addr: SocketAddr = addr_text
            .parse()
            .map_err(|e| format!("STRAF3_RECORDS_ADDR (`{addr_text}`) is not an address: {e}"))?;

        let jwks_url = non_empty("NEON_AUTH_JWKS_URL");
        // Better Auth issues tokens whose `iss` is its base URL. Configurable
        // separately so a deployment that disagrees can say so rather than
        // having the check quietly skipped.
        let issuer = non_empty("NEON_AUTH_ISSUER").or_else(|| {
            non_empty("NEON_AUTH_BASE_URL").map(|base| base.trim_end_matches('/').to_string())
        });

        let maps_dir = std::env::var("STRAF3_MAPS_DIR")
            .map(PathBuf::from)
            .unwrap_or_else(|_| {
                PathBuf::from(env!("CARGO_MANIFEST_DIR"))
                    .join("..")
                    .join("..")
                    .join("assets")
                    .join("maps")
            });

        let maps_url_prefix =
            std::env::var("STRAF3_MAPS_URL_PREFIX").unwrap_or_else(|_| "/assets/maps".to_string());

        Ok(Self {
            database_url,
            addr,
            jwks_url,
            issuer,
            maps_dir,
            maps_url_prefix,
        })
    }

    /// A log line that says what is configured without saying what it is.
    #[must_use]
    pub fn describe(&self) -> String {
        format!(
            "listening on {}; database configured: yes; jwks configured: {}; issuer pinned: {}; \
             maps from {}",
            self.addr,
            if self.jwks_url.is_some() { "yes" } else { "no" },
            if self.issuer.is_some() { "yes" } else { "no" },
            self.maps_dir.display(),
        )
    }
}

/// Neon's direct endpoint for a pooled connection string.
///
/// # Why the service does not use the pooler
///
/// `DATABASE_URL` in the operator's `.env` names Neon's pooled endpoint —
/// PgBouncer in transaction-pooling mode. Most of what this service does is
/// perfectly happy there. One thing is not, and it is not a hypothetical:
///
/// `sqlx`'s migrator takes a **session-scoped** `pg_advisory_lock` for the
/// duration of a migration run. Through a transaction pooler the server backend
/// outlives the client that took it, and the lock goes back into the pool still
/// held. That happened here: `pg_locks` showed backend 1472 **idle**, holding
/// advisory lock `(328116550, 694340936)` — sqlx's migration lock — long after
/// the process that took it was gone, with the next migration blocked behind it
/// indefinitely. It took a `pg_terminate_backend` to clear.
///
/// The lock is not *broken* by the pooler — a second session correctly cannot
/// take it — it is *leaked* by it, which is worse, because the holder is
/// unreachable. Migrations run at startup in both binaries, so every restart is
/// exposed to it.
///
/// This is one local origin with a handful of connections; pooling buys nothing
/// here and costs that. The transformation is Neon's own convention and is done
/// in code rather than by editing the operator's `.env`, so nothing has to be
/// kept in sync by hand. `STRAF3_USE_POOLED_DATABASE=1` opts back in.
#[must_use]
pub fn direct_endpoint(url: &str) -> String {
    if std::env::var("STRAF3_USE_POOLED_DATABASE").is_ok_and(|v| v == "1") {
        return url.to_string();
    }
    url.replace("-pooler.", ".")
}

fn non_empty(key: &str) -> Option<String> {
    std::env::var(key)
        .ok()
        .map(|v| v.trim().to_string())
        .filter(|v| !v.is_empty())
}
