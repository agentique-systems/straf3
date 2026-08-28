//! `straf3-records-seed`: run the migrations and derive the seed rows.
//!
//! Every digest this writes is computed by running code — `straf3-replay` for
//! the physics identity, `straf3-map` for the collision identity,
//! `straf3-replay::crosstarget` for the build. Nothing is pasted. See
//! `straf3_records::seed` for why that is not a style preference.

use straf3_records::{config::Config, db, seed};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("info")).init();

    let config = Config::from_env()?;
    let pool = db::connect(&config.database_url).await?;
    db::migrate(&pool).await?;

    let report = seed::seed_all(&pool, &config.maps_dir, &config.maps_url_prefix).await?;
    print!("{}", report.render());

    if !report.map_failures.is_empty() {
        // Named, not swallowed: a map that will not compile is a map nobody
        // can play from a link, and finding out at seed time is the cheap
        // moment.
        std::process::exit(1);
    }
    Ok(())
}
