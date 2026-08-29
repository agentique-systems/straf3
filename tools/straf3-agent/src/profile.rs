//! Which movement profile a run is simulated under, by name.
//!
//! # Why this table is a copy, and what would happen if it drifted
//!
//! The authority is `straf3-game`'s `profile::by_name`, which is the reader the
//! shipped `--replay` path goes through. This crate cannot call it: that crate
//! is the game binary and links `winit` and `wgpu`, and an agent that dragged a
//! windowing stack in to look up a string would be paying for a GPU to plan a
//! course.
//!
//! So the spellings are repeated here, and the consequence of them drifting is
//! worth naming because it is benign: a fixture this crate writes carries the
//! profile name in its header, and the shipped parser refuses a name it does not
//! know. A drift shows up as `straf3 --replay` rejecting the file by name, not
//! as a run replayed under different physics.
//!
//! # `straf3` is now a name this crate offers, and the default is still `cpm`
//!
//! Those are two decisions, not one, and only the first of them changed.
//!
//! This module used to record that `straf3` was unusable here because the
//! shipped client did not know it: the canon freeze landed
//! `PhysicsProfile::straf3()` in the simulation and its client half never
//! arrived, so a fixture headed `profile straf3` would have been rejected by the
//! very binary that has to replay it. **That is no longer true.** r1 landed the
//! client half — `crates/straf3-game/src/profile.rs:102` now advertises
//! `straf3|cpm|vq3|experimental` and `:118` resolves the name — so the table
//! below follows it, which is the whole contract this module has.
//!
//! The default stays `cpm` for a narrower reason that has not changed. A fixture
//! this crate writes is evidence only if the shipped binary reads it back to the
//! same checksum, and **nobody in this tree has yet replayed a `profile
//! straf3`-headed stream end to end**. `PhysicsProfile::straf3()` is bit-for-bit
//! identical to `PhysicsProfile::cpm()` — asserted below, not assumed — so the
//! header name is the only thing at stake and moving it would buy nothing while
//! putting an unverified claim under r12. When someone verifies that replay, the
//! default is a one-line change and this paragraph is its trigger.

use straf3_sim::PhysicsProfile;

/// Every profile spelling this crate accepts, for a usage or error message.
///
/// Deliberately the same list `straf3-game` advertises, and no longer: a name
/// this crate accepted and the replay parser did not would produce a command
/// stream nothing can verify.
pub const NAMES: &str = "straf3|cpm|vq3|experimental";

/// The default, and why it is not `straf3`: see the module docs.
pub const DEFAULT: &str = "cpm";

/// The constants a profile name stands for, or `None` for a name the shipped
/// client would refuse.
#[must_use]
pub fn by_name(name: &str) -> Option<PhysicsProfile> {
    match name {
        // Straf3's own frozen canon. Spelled out rather than written as
        // `cpm()`, for the same reason `straf3-game`'s table spells it out: the
        // two are equal today and the name says which one was asked for.
        "straf3" => Some(PhysicsProfile::straf3()),
        "cpm" => Some(PhysicsProfile::cpm()),
        "vq3" => Some(PhysicsProfile::vq3()),
        // `straf3-game` resolves this through its own `experimental()`, which is
        // an inherent-over-trait shim that became `PhysicsProfile::experimental`
        // when the canon wave landed. Naming the simulation's constructor
        // directly is the same constants by the same path.
        "experimental" => Some(PhysicsProfile::experimental()),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_advertised_name_resolves_and_nothing_else_does() {
        for name in NAMES.split('|') {
            assert!(by_name(name).is_some(), "`{name}` is advertised but absent");
        }
        assert!(by_name(DEFAULT).is_some());
        assert_eq!(by_name("quake3"), None);
        assert_eq!(by_name("CPM"), None);
    }

    #[test]
    fn straf3_is_a_spelling_this_crate_offers_now_that_r1_has_landed() {
        // This test used to assert the opposite, and its own comment named the
        // condition under which it had to invert: "when r1 lands the client
        // half". It has. `straf3-game`'s table advertises the name and resolves
        // it, so refusing it here would make this crate the one component that
        // disagrees with the binary it writes files for.
        assert_eq!(by_name("straf3"), Some(PhysicsProfile::straf3()));
    }

    #[test]
    fn the_default_is_not_straf3_and_that_is_deliberate() {
        // Not an oversight either, and the reason is no longer about the parser
        // — it is that no seat has yet replayed a `profile straf3`-headed stream
        // through the shipped binary. Until one has, defaulting to a header
        // nobody has round-tripped would put an unverified claim under r12 to
        // buy nothing, the two profiles being numerically equal.
        assert_eq!(DEFAULT, "cpm");
    }

    #[test]
    fn the_canonical_profile_is_still_cpm_by_another_name() {
        // Recorded as a fact about this tree rather than assumed: it is why
        // defaulting to `cpm` costs the agent nothing. If this ever fails, the
        // constants moved and `docs/movement-agent.md` is stale.
        assert_eq!(PhysicsProfile::straf3(), PhysicsProfile::cpm());
    }
}
