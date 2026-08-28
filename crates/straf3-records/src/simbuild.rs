//! What this verifier *is*, derived by running it rather than declared.
//!
//! `runs.sim_build_id` binds a ranked time to the simulation that produced the
//! verdict (§5.4), so the value has to change whenever the simulation's
//! observable behaviour or the recording format changes. A version string does
//! not have that property — someone has to remember to bump it — so this
//! module takes the number from `straf3_replay::crosstarget`, which records a
//! run, encodes it, decodes it, re-simulates it and folds every number the four
//! targets must agree on. Change an angle, a movement constant or a byte of the
//! layout and this moves.
//!
//! `native_verifier_ok` is likewise *run*, not asserted: it is true when every
//! `crosstarget` case round-tripped, verified, and refused a stale world on
//! this machine. A verifier that cannot reproduce its own recordings should say
//! so in the row it stamps on every run it ranks.
//!
//! `wasm_hash` stays null. This service does not build the browser bundle and
//! will not fill the column with a plausible number (wave contracts §E3).

use straf3_replay::crosstarget;

/// The identity of the simulation this process links.
#[derive(Debug, Clone)]
pub struct SimBuild {
    /// The workspace version.
    pub sim_version: String,
    /// The commit, when the build could see one.
    pub git_sha: String,
    /// A fold over every number `crosstarget` publishes.
    pub build_hash: u64,
    /// Whether every `crosstarget` case passed on this machine.
    pub native_verifier_ok: bool,
}

impl SimBuild {
    /// Derive it. Runs `crosstarget`'s cases once per process; they are cached
    /// behind a `OnceLock` inside `straf3-replay`.
    #[must_use]
    pub fn derive() -> Self {
        Self {
            sim_version: env!("CARGO_PKG_VERSION").to_string(),
            git_sha: env!("STRAF3_GIT_SHA").to_string(),
            build_hash: crosstarget::grand_digest(),
            native_verifier_ok: crosstarget::all_ok(),
        }
    }
}

/// The `maps.map_compiler_version` a seeded map is stamped with.
///
/// Deliberately *not* mixed with [`SimBuild::build_hash`]: the unique index on
/// `maps (source_sha256, collision_digest, map_compiler_version)` would then
/// admit a second row for the same map on every sim change, and `maps.slug` is
/// unique, so the reseed would fail rather than being idempotent. The number
/// that must move when the compiler changes is `collision_digest`, and it does
/// — `straf3_map::CompiledMap::collision_digest` folds the compiled hulls, not
/// the source.
#[must_use]
pub fn map_compiler_version() -> String {
    format!("straf3-map {}", env!("CARGO_PKG_VERSION"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_build_hash_is_derived_and_stable_within_a_process() {
        let a = SimBuild::derive();
        let b = SimBuild::derive();
        assert_eq!(a.build_hash, b.build_hash);
        assert_ne!(a.build_hash, 0, "a build hash of zero means nothing ran");
    }

    /// If this fails, the machine running the verifier cannot reproduce its own
    /// recordings — which is a determinism regression, not a service bug, and
    /// the `sim_builds` row would record it honestly rather than hide it.
    #[test]
    fn this_build_passes_its_own_cross_target_cases() {
        assert!(
            SimBuild::derive().native_verifier_ok,
            "straf3_replay::crosstarget did not pass on this machine: {}",
            crosstarget::render(std::env::consts::OS)
        );
    }
}
