//! `straf3-records-verifier`: the process that decides whether a run happened.
//!
//! Separate from the API for the reason ARCHITECTURE §7.1 gives — re-simulation
//! is unbounded CPU driven by untrusted input, and in the request path a
//! pathological submission becomes an outage rather than a slow job. The queue
//! is `runs where status = 'pending'`, claimed with `for update skip locked`,
//! because §4.3 puts the sustained rate in the single digits per second and
//! there is no throughput case for anything more elaborate.
//!
//! Same crate as the API, deliberately: a verifier linking a different
//! `straf3-sim` than the intake decoded against would be verifying a different
//! game.

use std::time::Duration;

use straf3_records::worker::{self, MapCache};
use straf3_records::{config::Config, db, digest16, seed, simbuild};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("info")).init();

    let config = Config::from_env()?;
    let pool = db::connect(&config.database_url).await?;
    db::migrate(&pool).await?;

    let build = simbuild::SimBuild::derive();
    let build_id = seed::ensure_sim_build(&pool, &build).await?;
    log::info!(
        "verifier up: sim_build #{build_id} {} · native_verifier_ok {} · maps from {}",
        digest16::format(build.build_hash),
        build.native_verifier_ok,
        config.maps_dir.display()
    );
    if !build.native_verifier_ok {
        log::error!(
            "this machine did not pass straf3-replay's own cross-target cases. Verdicts written \
             from here are suspect; investigate before trusting a board."
        );
    }

    let once = std::env::args().any(|a| a == "--once");
    let mut maps = MapCache::new(&config.maps_dir);

    loop {
        match worker::claim_and_verify(&pool, &mut maps).await {
            Ok(Some(id)) => log::info!("verified run {id}"),
            Ok(None) => {
                if once {
                    log::info!("no pending runs; --once, so stopping");
                    return Ok(());
                }
                tokio::time::sleep(Duration::from_millis(500)).await;
            }
            Err(e) => {
                log::error!("verification pass failed: {e}");
                if once {
                    return Err(e.into());
                }
                tokio::time::sleep(Duration::from_secs(2)).await;
            }
        }
    }
}
