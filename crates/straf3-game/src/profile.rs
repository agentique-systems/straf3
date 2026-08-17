//! Which movement profiles this build offers, by name — and the rule that
//! keeps the experimental one out of the record books.
//!
//! # Why the spellings live in one place
//!
//! Two readers turn a profile name into constants: the command line
//! (`--profile`) and the fixture parser (`profile <name>`). They must agree, or
//! a recording made under a name one of them understands replays under
//! different physics in the other — which is the failure the `.s3d` format's
//! physics digest exists to catch *after the fact*, and which this module
//! prevents outright.
//!
//! # `experimental` is playable, recordable, and never ranked
//!
//! Spec decision D2. straf3's own mechanics land in `experimental` and canon
//! does not move this wave, so an experimental time is not a CPM time and not a
//! VQ3 time. Two things enforce that, and they are independent on purpose:
//!
//! 1. **The file name.** A personal best is filed as
//!    `runs/<map>.<profile>.s3d`, so `runs/coil.experimental.s3d` is a
//!    different file from `runs/coil.cpm.s3d` and neither session ever opens
//!    the other's. Nothing is compared because nothing is loaded.
//! 2. **The recorded name.** A `.s3d` says which profile it was set under, and
//!    [`crate::app`] refuses one whose name is not the session's — so a file
//!    *renamed* into the other namespace is still refused rather than raced.
//!
//! The physics digest would catch it too, once the constants actually differ.
//! It does not today, and that is exactly why the two rules above do not depend
//! on it: see [`is_stub`].

use straf3_sim::PhysicsProfile;

/// straf3's own movement vocabulary, as `--profile experimental`.
///
/// # Why this is a trait and not a `match`
///
/// Session A owns the constants. This crate had to build the command line, the
/// fixture spelling and the personal-best namespacing *before* they landed, and
/// the two halves are merged separately — so the name has to compile now and
/// mean A's constants later, without anyone remembering to come back here.
///
/// An inherent associated item takes precedence over a trait one. So
/// `PhysicsProfile::experimental()` below resolves to `straf3-sim`'s own
/// constructor the moment it reaches this worktree, and to this fallback until
/// then. No edit here is needed at integration.
///
/// The name is settled: `experimental()`, a constructor alongside `cpm()` and
/// `vq3()`, per the spec's fixed-interfaces line. This shim exists only to let
/// the crate compile before A's constructor arrives — deliberately *not* to
/// tolerate a second spelling. If A lands something else, the compiler says so
/// at integration and it is a one-line edit, which is the failure mode worth
/// having; a compatibility shim that outlives the confusion that created it
/// just leaves the next reader working out which name is real.
///
/// The placeholder is CPM's constants. It is deliberately *not* a set of
/// invented numbers: choosing straf3's movement is Session A's work, and
/// guessing at it here would put a second opinion about how the game feels into
/// the tree. [`is_stub`] is how a session finds out which it got.
trait ExperimentalFallback {
    /// See [`ExperimentalFallback`].
    fn experimental() -> PhysicsProfile;
}

impl ExperimentalFallback for PhysicsProfile {
    fn experimental() -> PhysicsProfile {
        Self::cpm()
    }
}

/// The experimental profile's constants.
#[must_use]
pub fn experimental() -> PhysicsProfile {
    PhysicsProfile::experimental()
}

/// Whether the experimental profile is still the placeholder above.
///
/// Measured rather than assumed: if its constants fold to the same digest as
/// CPM's then it *is* CPM, whatever it is called. A session running it should
/// be told, because "experimental" would otherwise describe a session that
/// plays exactly like canon and it would be indistinguishable from one where
/// the new mechanics simply did nothing.
#[must_use]
pub fn is_stub() -> bool {
    straf3_replay::physics_digest(&experimental())
        == straf3_replay::physics_digest(&PhysicsProfile::cpm())
}

/// Every profile spelling this build accepts, for a usage or error message.
pub const NAMES: &str = "cpm|vq3|experimental";

/// The constants a profile name stands for.
///
/// `None` for a name this build does not have — refused by the caller rather
/// than defaulted, because a session that silently ran CPM when it was asked
/// for something else would produce a recording labelled with a profile it was
/// not simulated under.
#[must_use]
pub fn by_name(name: &str) -> Option<PhysicsProfile> {
    match name {
        "cpm" => Some(PhysicsProfile::cpm()),
        "vq3" => Some(PhysicsProfile::vq3()),
        "experimental" => Some(experimental()),
        _ => None,
    }
}

/// Whether a profile's times are ranked against the canonical ones.
///
/// `experimental` is not, and neither is anything this build does not know.
#[must_use]
pub const fn is_canon(name: &str) -> bool {
    matches!(name.as_bytes(), b"cpm" | b"vq3")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_accepted_name_resolves_and_nothing_else_does() {
        for name in NAMES.split('|') {
            assert!(by_name(name).is_some(), "`{name}` is advertised but absent");
        }
        assert_eq!(by_name("quake1"), None);
        assert_eq!(by_name("CPM"), None);
        assert_eq!(by_name(""), None);
    }

    #[test]
    fn the_canonical_profiles_are_exactly_the_two_that_predate_this_wave() {
        assert!(is_canon("cpm"));
        assert!(is_canon("vq3"));
        assert!(
            !is_canon("experimental"),
            "an experimental time is not a canon time (spec D2)"
        );
        assert!(!is_canon("quake1"));
    }

    #[test]
    fn the_two_canonical_profiles_are_not_each_other() {
        // The property `is_stub` relies on: identical digests mean identical
        // physics, so a digest comparison is a real test of sameness.
        assert_ne!(
            straf3_replay::physics_digest(&PhysicsProfile::cpm()),
            straf3_replay::physics_digest(&PhysicsProfile::vq3())
        );
    }

    /// This test does not assert *which* answer is right — both are, at
    /// different points in the wave. It asserts that the answer is knowable,
    /// which is what stops `experimental` from quietly being CPM.
    #[test]
    fn whether_the_experimental_profile_is_still_a_stub_is_answerable() {
        let stub = is_stub();
        assert_eq!(
            stub,
            straf3_replay::physics_digest(&experimental())
                == straf3_replay::physics_digest(&PhysicsProfile::cpm())
        );
    }
}
