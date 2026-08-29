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
//! # Why the default is `cpm` and not `straf3`
//!
//! Because `straf3` is not a name the shipped client accepts. The canon freeze
//! landed `PhysicsProfile::straf3()` in the simulation and its client half never
//! did — `straf3-game` offers `cpm|vq3|experimental` — so a fixture headed
//! `profile straf3` would be rejected by the very binary that has to replay it.
//! `PhysicsProfile::straf3()` is today bit-for-bit identical to
//! `PhysicsProfile::cpm()`, which is why running under `cpm` costs this crate
//! nothing but a name. That is a finding about the tree, recorded here rather
//! than worked around: closing it is requirement r1's job, not this crate's.

use straf3_sim::PhysicsProfile;

/// Every profile spelling this crate accepts, for a usage or error message.
///
/// Deliberately the same list `straf3-game` advertises, and no longer: a name
/// this crate accepted and the replay parser did not would produce a command
/// stream nothing can verify.
pub const NAMES: &str = "cpm|vq3|experimental";

/// The default, and why: see the module docs.
pub const DEFAULT: &str = "cpm";

/// The constants a profile name stands for, or `None` for a name the shipped
/// client would refuse.
#[must_use]
pub fn by_name(name: &str) -> Option<PhysicsProfile> {
    match name {
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
    fn straf3_is_not_a_spelling_this_crate_offers() {
        // Not an oversight. The shipped `--replay` reader does not know the
        // name, so a fixture written under it could not be verified — see the
        // module docs. When r1 lands the client half, this test is the one that
        // has to change, deliberately.
        assert_eq!(by_name("straf3"), None);
    }

    #[test]
    fn the_canonical_profile_is_still_cpm_by_another_name() {
        // Recorded as a fact about this tree rather than assumed: it is why
        // defaulting to `cpm` costs the agent nothing. If this ever fails, the
        // constants moved and `docs/movement-agent.md` is stale.
        assert_eq!(PhysicsProfile::straf3(), PhysicsProfile::cpm());
    }
}
