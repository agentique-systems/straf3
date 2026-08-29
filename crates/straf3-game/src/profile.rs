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
//! # `straf3` is the canon this game is named after
//!
//! `docs/movement-canon.md` Part 3 froze [`PhysicsProfile::straf3`] as Straf3's
//! own ruleset and made it `straf3-sim`'s `Default`; this module is the client
//! half of that, which Part 3 §3.8 says in as many words is the client's change
//! and not canon's. `straf3` is therefore a name `--profile` accepts, the name
//! a session runs under when nobody says otherwise ([`crate::app::Options`]),
//! and a canon name for ranking ([`is_canon`]).
//!
//! **It is numerically equal to [`PhysicsProfile::cpm`] today** — Part 2
//! rejected all three candidate mechanics and no inherited constant moved — so
//! naming it the default invalidates nothing: the physics digest a `.s3d` is
//! bound to did not move, and `--profile cpm` still opens every file it always
//! did. What moved is the *name* a default run is filed under, from
//! `runs/<map>.cpm.s3d` to `runs/<map>.straf3.s3d`. PLAYING.md says what that
//! means for a `runs/` directory that predates the change.
//!
//! `cpm` and `vq3` keep their canon standing rather than being demoted to make
//! room. They are the games Straf3 was reconstructed beside, their times were
//! set under physics that has not moved, and demoting them would orphan every
//! one of those times to record a change that did not happen.
//!
//! # `experimental` is playable, recordable, and never ranked
//!
//! Spec decision D2, and it is now the one place the **rejected** candidates
//! are still playable. D2's premise was that straf3's own mechanics would land
//! here and stay out of the record books until they were judged. They were
//! judged: canon Part 2 rejected crouch slide, dash and wall jump, and Part 3
//! froze `straf3()` with all eight candidate constants at their disabling
//! zeros. `PhysicsProfile::experimental()` is where those three are still
//! switched on, which is what `tools/straf3-lab` measures against and what the
//! next canon wave reaches for. It did not lose its purpose when canon landed;
//! that *is* its purpose.
//!
//! So an experimental time is not a canon time — now for the strongest possible
//! reason, that it was set under mechanics canon does not have. Two things
//! enforce it, and they are independent on purpose:
//!
//! 1. **The file name.** A personal best is filed as
//!    `runs/<map>.<profile>.s3d`, so `runs/coil.experimental.s3d` is a
//!    different file from `runs/coil.straf3.s3d` and neither session ever opens
//!    the other's. Nothing is compared because nothing is loaded.
//! 2. **The recorded name.** A `.s3d` says which profile it was set under, and
//!    [`crate::app`] refuses one whose name is not the session's — so a file
//!    *renamed* into the other namespace is still refused rather than raced.
//!
//! For `experimental` the physics digest now catches it as well: canon and the
//! candidates differ, so they fold to different digests. That was not true
//! while `experimental` was a placeholder holding CPM's constants, which is why
//! the two rules above were built not to depend on it.
//!
//! **They are kept that way because the same hazard has simply moved to a
//! different pair.** `straf3` and `cpm` are numerically equal, so they fold to
//! the *same* digest: a `cpm` recording copied into `runs/coil.straf3.s3d`
//! matches on digest and is separated from a canon record by nothing but the
//! name it carries. Rule 2 is what refuses it. That is not a hypothetical
//! filing error — moving the default is precisely the event that leaves a
//! player with an old `runs/<map>.cpm.s3d` and an obvious-looking way to
//! "migrate" it.

use straf3_sim::PhysicsProfile;

/// Straf3's own frozen canon, as `--profile straf3`.
///
/// The accessor exists so that a reader of this crate has the same one-name
/// route to canon's constants that it has to `experimental`'s, without
/// depending on `straf3-sim` directly. The `committed_evidence` test is
/// written against it.
#[must_use]
pub fn straf3() -> PhysicsProfile {
    PhysicsProfile::straf3()
}

/// The experimental profile's constants: canon plus the three rejected
/// candidates, switched on.
///
/// A thin accessor and not a definition. `straf3-sim` owns the numbers — it
/// spells `experimental()` as `..Self::cpm()` with eight constants overridden,
/// precisely so that anything the lab measures between the two is attributable
/// to the mechanics and to nothing else.
#[must_use]
pub fn experimental() -> PhysicsProfile {
    PhysicsProfile::experimental()
}

/// Every profile spelling this build accepts, for a usage or error message.
///
/// Canon first, because it is the default and the answer to "which one do I
/// want".
pub const NAMES: &str = "straf3|cpm|vq3|experimental";

/// The constants a profile name stands for.
///
/// `None` for a name this build does not have — refused by the caller rather
/// than defaulted, because a session that silently ran canon when it was asked
/// for something else would produce a recording labelled with a profile it was
/// not simulated under.
#[must_use]
pub fn by_name(name: &str) -> Option<PhysicsProfile> {
    match name {
        // Named rather than `PhysicsProfile::default()`, for the reason canon
        // spelled `straf3()` out rather than writing `Self::cpm()`: the name on
        // the left is what gets recorded in the `.s3d`, and a table where the
        // two can drift apart resolves a name to constants it does not stand
        // for.
        "straf3" => Some(straf3()),
        "cpm" => Some(PhysicsProfile::cpm()),
        "vq3" => Some(PhysicsProfile::vq3()),
        "experimental" => Some(experimental()),
        _ => None,
    }
}

/// Whether a profile's times are ranked against the canonical ones.
///
/// `experimental` is not, and neither is anything this build does not know.
///
/// A separate table from [`by_name`] on purpose: "this build can simulate it"
/// and "a time set under it counts" are different questions, and `experimental`
/// is the profile that answers yes to the first and no to the second.
#[must_use]
pub const fn is_canon(name: &str) -> bool {
    matches!(name.as_bytes(), b"straf3" | b"cpm" | b"vq3")
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

    /// The name `straf3` means the frozen canon profile and nothing near it.
    ///
    /// This is the whole of `--profile straf3` and of the browser's `?p=straf3`
    /// — both reach [`by_name`] — so the name and the constants are checked
    /// against each other here once rather than at each caller.
    #[test]
    fn the_name_straf3_resolves_to_the_frozen_canon_profile() {
        assert_eq!(by_name("straf3"), Some(PhysicsProfile::straf3()));
        // `straf3-sim` made `straf3()` its `Default` at the freeze. A build
        // where this crate's `straf3` and that `Default` were different
        // profiles would be two answers to "what runs when nobody said".
        assert_eq!(by_name("straf3"), Some(PhysicsProfile::default()));
    }

    /// Renamed from `the_canonical_profiles_are_exactly_the_two_that_predate_this_wave`,
    /// which canon §3.8 flagged: it is the *name* that the freeze falsified,
    /// so the name is what changed rather than the assertions underneath it.
    #[test]
    fn straf3_is_canon_beside_the_two_profiles_that_predate_it() {
        // The first canonical profile that does not predate this wave, and the
        // only one that is Straf3's own rather than a reconstruction of
        // somebody else's game.
        assert!(is_canon("straf3"));
        assert!(is_canon("cpm"));
        assert!(is_canon("vq3"));
        assert!(
            !is_canon("experimental"),
            "an experimental time is not a canon time (spec D2), and canon \
             landing did not change that"
        );
        assert!(!is_canon("quake1"));
    }

    #[test]
    fn a_digest_comparison_tells_apart_two_profiles_that_differ() {
        // What the separation rules above are worth is that a digest is a real
        // test of sameness rather than a constant `true`.
        assert_ne!(
            straf3_replay::physics_digest(&PhysicsProfile::cpm()),
            straf3_replay::physics_digest(&PhysicsProfile::vq3())
        );
    }

    /// This replaces `whether_the_experimental_profile_is_still_a_stub_is_answerable`
    /// and the runtime `is_stub` check it guarded, both of which asked whether
    /// `straf3-sim` had landed the constants yet. It has, so the question is
    /// answered and the thing worth guarding is the answer.
    ///
    /// The measurement rather than the assumption, which is the instinct
    /// `is_stub` had right: if `experimental` ever folded to canon's digest it
    /// *would be* canon, whatever it is called, and every comparison
    /// `tools/straf3-lab` draws between the two would be measuring nothing
    /// while still producing numbers. A test fails on the commit that does
    /// that; a startup warning fails only if somebody happens to play it.
    #[test]
    fn experimental_is_the_rejected_candidates_switched_on_not_canon_renamed() {
        assert_ne!(
            straf3_replay::physics_digest(&experimental()),
            straf3_replay::physics_digest(&straf3()),
            "`experimental` folds to canon's digest, so it is canon under \
             another name and the lab is comparing a profile with itself"
        );
        // Named individually rather than as a digest, so the failure says
        // *which* mechanic was switched off. Canon carries all eight at zero
        // (`canonical_straf3_carries_no_candidate_mechanic`, in `straf3-sim`);
        // these three are the reason this profile exists.
        let p = experimental();
        assert_ne!(p.slide_entry_speed, straf3().slide_entry_speed);
        assert_ne!(p.dash_speed, straf3().dash_speed);
        assert_ne!(p.wall_jump_velocity, straf3().wall_jump_velocity);
    }
}
