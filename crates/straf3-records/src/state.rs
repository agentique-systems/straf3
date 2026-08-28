//! What every handler shares.

use std::sync::Arc;

use sqlx::PgPool;

use crate::auth::Jwks;
use crate::config::Config;
use crate::simbuild::SimBuild;

/// The API process's shared state.
#[derive(Clone)]
pub struct AppState(Arc<Inner>);

struct Inner {
    pool: PgPool,
    jwks: Jwks,
    config: Config,
    sim_build: SimBuild,
    sim_build_id: i32,
}

impl AppState {
    /// Assemble it. `sim_build_id` is the `sim_builds` row this process stamps
    /// on every run it takes in — derived at startup by
    /// [`crate::seed::ensure_sim_build`], never configured.
    #[must_use]
    pub fn new(
        pool: PgPool,
        jwks: Jwks,
        config: Config,
        sim_build: SimBuild,
        sim_build_id: i32,
    ) -> Self {
        Self(Arc::new(Inner {
            pool,
            jwks,
            config,
            sim_build,
            sim_build_id,
        }))
    }

    /// The connection pool.
    #[must_use]
    pub fn pool(&self) -> &PgPool {
        &self.0.pool
    }

    /// The JWKS cache.
    #[must_use]
    pub fn jwks(&self) -> &Jwks {
        &self.0.jwks
    }

    /// Startup configuration. Contains credentials; do not print it.
    #[must_use]
    pub fn config(&self) -> &Config {
        &self.0.config
    }

    /// What this build is.
    #[must_use]
    pub fn sim_build(&self) -> &SimBuild {
        &self.0.sim_build
    }

    /// The `sim_builds.id` a newly submitted run is bound to.
    #[must_use]
    pub fn sim_build_id(&self) -> i32 {
        self.0.sim_build_id
    }
}
