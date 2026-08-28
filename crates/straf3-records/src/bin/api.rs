//! `straf3-records-api`: the `/v1` surface.
//!
//! Binds `127.0.0.1:8788` by default, overridable by `STRAF3_RECORDS_ADDR`. It
//! does not serve the site and it needs no CORS: every browser request arrives
//! same-origin through `web/dev/serve.mjs`, which proxies `/v1` to here.

use straf3_records::{AppState, auth::Jwks, config::Config, db, digest16, routes, seed, simbuild};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("info")).init();

    let config = Config::from_env()?;
    let pool = db::connect(&config.database_url).await?;
    db::migrate(&pool).await?;

    let build = simbuild::SimBuild::derive();
    let build_id = seed::ensure_sim_build(&pool, &build).await?;

    let jwks = Jwks::new(config.jwks_url.clone(), config.issuer.clone());
    if jwks.is_configured() {
        // Warm, but do not refuse to boot on failure: a records service that
        // will not start because the auth endpoint is briefly down would take
        // every anonymous board down with it.
        match jwks.warm().await {
            Ok(count) => log::info!("jwks warmed: {count} key(s)"),
            Err(e) => log::warn!("jwks not warmed: {e} (authenticated routes will retry)"),
        }
    } else {
        log::warn!(
            "NEON_AUTH_JWKS_URL is not set: every authenticated route will refuse, which is the \
             honest behaviour rather than accepting unverified tokens"
        );
    }

    let addr = config.addr;
    // `describe` prints what is configured, never what it is configured to.
    log::info!("{}", config.describe());
    log::info!(
        "sim_build #{build_id} {} · native_verifier_ok {}",
        digest16::format(build.build_hash),
        build.native_verifier_ok
    );

    let state = AppState::new(pool, jwks, config, build, build_id);
    let app = routes::router(state);

    let listener = tokio::net::TcpListener::bind(addr).await?;
    axum::serve(listener, app)
        .with_graceful_shutdown(shutdown())
        .await?;
    Ok(())
}

async fn shutdown() {
    let _ = tokio::signal::ctrl_c().await;
    log::info!("shutting down");
}
