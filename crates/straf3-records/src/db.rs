//! The pool, the migrations, and the one query that makes `/v1/health` honest.

use sqlx::postgres::{PgPoolOptions, PgQueryResult};
use sqlx::{Executor, PgPool, Postgres, Transaction};

/// The migrations, embedded at compile time from the repository root.
///
/// Embedded rather than read from disk so that the API binary and the verifier
/// binary cannot disagree about what schema they expect, and so that neither
/// depends on being started from a particular directory.
pub static MIGRATIONS: sqlx::migrate::Migrator = sqlx::migrate!("../../migrations");

/// A transaction over the records schema.
pub type Tx<'c> = Transaction<'c, Postgres>;

/// Open the pool.
///
/// # Errors
///
/// When the connection string is unusable or Postgres refuses the first
/// connection.
pub async fn connect(database_url: &str) -> Result<PgPool, sqlx::Error> {
    PgPoolOptions::new()
        // Small on purpose: §4.3 puts the sustained verification rate in the
        // single digits per second, and Neon's pooler is the thing actually
        // fanning out.
        .max_connections(8)
        .acquire_timeout(std::time::Duration::from_secs(10))
        .connect(database_url)
        .await
}

/// Apply any migration that has not run.
///
/// # Errors
///
/// When a migration fails or the recorded history does not match.
pub async fn migrate(pool: &PgPool) -> Result<(), sqlx::migrate::MigrateError> {
    MIGRATIONS.run(pool).await
}

/// Whether the database actually round-trips.
///
/// This is what `GET /v1/health` reports, and it is a real query rather than a
/// pool-state inspection: a pool can hold an idle connection to a Postgres that
/// has since stopped answering. Wave contracts §C — a `200` here is what makes
/// the `503` elsewhere honest.
///
/// # Errors
///
/// Whatever the database or the pool said.
pub async fn round_trips(pool: &PgPool) -> Result<(), sqlx::Error> {
    let value: i32 = sqlx::query_scalar("select 1").fetch_one(pool).await?;
    if value == 1 {
        Ok(())
    } else {
        Err(sqlx::Error::Protocol(format!(
            "`select 1` answered {value}"
        )))
    }
}

/// `set local statement_timeout`, so one pathological query cannot hold a
/// connection open forever.
///
/// # Errors
///
/// Whatever the database said.
pub async fn set_statement_timeout(
    tx: &mut Tx<'_>,
    millis: u32,
) -> Result<PgQueryResult, sqlx::Error> {
    tx.execute(format!("set local statement_timeout = {millis}").as_str())
        .await
}
