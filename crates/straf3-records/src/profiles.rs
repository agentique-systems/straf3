//! The physics profiles this build can honour, and the exact bits that make
//! each one what it is.
//!
//! # No digest here is ever typed by a human
//!
//! `physics_profiles.digest` is the `@digest16` in a `/m/coil/cpm@…` URL. It is
//! derived — [`straf3_replay::physics_digest`] folds the bits of every field of
//! the [`PhysicsProfile`] that is actually in effect — and this module's whole
//! job is to make sure the seeding path derives it the same way rather than
//! pasting a literal into SQL. A hand-typed digest in a migration is exactly
//! the failure `docs/web/URLS.md` §3 exists to prevent.
//!
//! # `profile_bits`, and why the test below is the point of it
//!
//! ARCHITECTURE §5.1 stores `profile_bits bytea` — "exact f32 bit patterns,
//! never decimal text" — so that a stored profile can be read back and compared
//! without going through a decimal round trip. [`profile_bits`] writes exactly
//! the byte stream `straf3_replay::physics_digest` folds, and
//! [`tests::the_stored_bits_are_the_bits_the_digest_is_taken_over`] proves it
//! by re-deriving the digest from the stored bytes alone. Without that test the
//! two could drift and the column would be storing a *different* profile than
//! the digest names.
//!
//! The destructure below has no `..` rest pattern, for the same reason
//! `straf3-replay`'s does: a new constant in `PhysicsProfile` fails to compile
//! here, which is the only way "every constant is covered" is a fact rather
//! than an intention. When that happens, add the field *and* bump
//! [`PROFILE_LAYOUT_VERSION`].

use straf3_sim::PhysicsProfile;
use straf3_sim::num::{Scalar, Vec3, to_bits};

/// ARCHITECTURE §3.2's `profile_layout_version`: bumped when
/// `PhysicsProfile` gains a field.
///
/// One, because this is the first schema written against the profile as it
/// stands. The exhaustive destructure in [`profile_bits`] is what forces
/// whoever widens the struct to come here.
///
/// It stayed at one through the candidate wave. `dash_entry_speed` was added
/// for canon §1.5's pre-registered dash retune, bumped this to two, and was
/// reverted whole when the dash was rejected — the layout version describes the
/// struct this build carries, not the struct that was considered.
pub const PROFILE_LAYOUT_VERSION: i16 = 1;

/// The two canon families. `experimental` is deliberately absent: spec D2 says
/// it is never comparable to canon, and a board it could rank on would be an
/// invitation to compare them.
pub const CANON_FAMILIES: [&str; 2] = ["vq3", "cpm"];

/// The family a bare `/m/<map>` resolves to (URLS.md §3).
pub const DEFAULT_FAMILY: &str = "cpm";

/// A profile this build implements, ready to be seeded.
#[derive(Debug, Clone)]
pub struct CanonProfile {
    /// `physics_profiles.kind` — `vq3` or `cpm`.
    pub kind: &'static str,
    /// The constants themselves.
    pub profile: PhysicsProfile,
}

impl CanonProfile {
    /// The derived digest. Never declared.
    #[must_use]
    pub fn digest(&self) -> u64 {
        straf3_replay::physics_digest(&self.profile)
    }

    /// The exact f32 bit patterns §5.1 stores.
    #[must_use]
    pub fn bits(&self) -> Vec<u8> {
        profile_bits(&self.profile)
    }
}

/// Every profile this build can verify a run under.
#[must_use]
pub fn canon() -> Vec<CanonProfile> {
    vec![
        CanonProfile {
            kind: "vq3",
            profile: PhysicsProfile::vq3(),
        },
        CanonProfile {
            kind: "cpm",
            profile: PhysicsProfile::cpm(),
        },
    ]
}

/// The profile this build implements for `digest`, if it implements one.
///
/// The verifier refuses anything this returns `None` for, naming the mismatch,
/// rather than substituting the nearest profile (ARCHITECTURE §7.2 step 2).
#[must_use]
pub fn by_digest(digest: u64) -> Option<PhysicsProfile> {
    canon()
        .into_iter()
        .find(|c| c.digest() == digest)
        .map(|c| c.profile)
}

/// The exact byte stream [`straf3_replay::physics_digest`] folds.
///
/// # Errors adding a field
///
/// There are none, because there is no `..` below: the compiler stops the
/// build instead.
#[must_use]
pub fn profile_bits(profile: &PhysicsProfile) -> Vec<u8> {
    let PhysicsProfile {
        accelerate,
        friction,
        stop_speed,
        max_speed,
        duck_scale,
        air_accelerate,
        gravity,
        jump_velocity,
        step_height,
        overclip,
        max_clip_planes,
        ground_trace_probe,
        min_walk_normal,
        hull_mins,
        hull_maxs,
        crouched_height,
        air_control,
        air_stop_accelerate,
        strafe_accelerate,
        strafe_wish_speed_cap,
        double_jump_window_ms,
        double_jump_boost,
        slide_entry_speed,
        slide_friction,
        slide_duration_ms,
        dash_speed,
        dash_window_ms,
        wall_jump_velocity,
        wall_contact_window_ms,
        wall_normal_max,
    } = *profile;

    let mut out = Vec::with_capacity(128);
    let scalar = |v: Scalar, out: &mut Vec<u8>| out.extend_from_slice(&to_bits(v).to_le_bytes());
    let vector = |v: Vec3, out: &mut Vec<u8>| {
        out.extend_from_slice(&to_bits(v.x).to_le_bytes());
        out.extend_from_slice(&to_bits(v.y).to_le_bytes());
        out.extend_from_slice(&to_bits(v.z).to_le_bytes());
    };

    scalar(accelerate, &mut out);
    scalar(friction, &mut out);
    scalar(stop_speed, &mut out);
    scalar(max_speed, &mut out);
    scalar(duck_scale, &mut out);
    scalar(air_accelerate, &mut out);
    scalar(gravity, &mut out);
    scalar(jump_velocity, &mut out);
    scalar(step_height, &mut out);
    scalar(overclip, &mut out);
    out.push(max_clip_planes);
    scalar(ground_trace_probe, &mut out);
    scalar(min_walk_normal, &mut out);
    vector(hull_mins, &mut out);
    vector(hull_maxs, &mut out);
    scalar(crouched_height, &mut out);
    scalar(air_control, &mut out);
    scalar(air_stop_accelerate, &mut out);
    scalar(strafe_accelerate, &mut out);
    scalar(strafe_wish_speed_cap, &mut out);
    out.extend_from_slice(&double_jump_window_ms.to_le_bytes());
    scalar(double_jump_boost, &mut out);
    scalar(slide_entry_speed, &mut out);
    scalar(slide_friction, &mut out);
    out.extend_from_slice(&slide_duration_ms.to_le_bytes());
    scalar(dash_speed, &mut out);
    out.extend_from_slice(&dash_window_ms.to_le_bytes());
    scalar(wall_jump_velocity, &mut out);
    out.extend_from_slice(&wall_contact_window_ms.to_le_bytes());
    scalar(wall_normal_max, &mut out);
    out
}

/// The label a player sees on a board, fixed at the moment the profile is
/// first seen and never edited afterwards (§5.4: rows are immutable).
#[must_use]
pub fn label_for(kind: &str, first_seen: chrono::DateTime<chrono::Utc>) -> String {
    format!("{} ({})", kind.to_uppercase(), first_seen.format("%Y-%m"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use straf3_replay::digest::Fnv1a;

    /// The one assertion that makes `profile_bits` trustworthy.
    ///
    /// `physics_profiles.profile_bits` claims to be the bits the digest was
    /// taken over. This re-derives the digest from those bytes alone — with
    /// only `straf3-replay`'s own domain tag in front — and requires it to
    /// equal what `straf3-replay` computed. If the two encoders ever disagree
    /// about a field's width, order or endianness, this fails, and the column
    /// stops silently describing a different profile than the digest names.
    #[test]
    fn the_stored_bits_are_the_bits_the_digest_is_taken_over() {
        for canon in canon() {
            let mut h = Fnv1a::new();
            h.bytes(b"straf3-replay/physics/1");
            h.bytes(&canon.bits());
            assert_eq!(
                h.finish(),
                canon.digest(),
                "`{}`: profile_bits is not the byte stream physics_digest folds",
                canon.kind
            );
        }
    }

    #[test]
    fn every_canon_family_has_exactly_one_profile() {
        let kinds: Vec<_> = canon().into_iter().map(|c| c.kind).collect();
        assert_eq!(kinds, CANON_FAMILIES.to_vec());
        assert!(CANON_FAMILIES.contains(&DEFAULT_FAMILY));
    }

    #[test]
    fn vq3_and_cpm_are_different_categories() {
        // §5.2: they are different games, which is exactly why they are
        // different boards. A schema that let them share a digest would let
        // them share a leaderboard.
        let profiles = canon();
        assert_ne!(profiles[0].digest(), profiles[1].digest());
    }

    #[test]
    fn a_digest_this_build_does_not_implement_resolves_to_nothing() {
        // The refusal ARCHITECTURE §7.2 step 2 requires: no nearest match, no
        // helpful substitution.
        let cpm = canon()[1].digest();
        assert!(by_digest(cpm).is_some());
        assert!(by_digest(cpm ^ 1).is_none());
    }

    #[test]
    fn experimental_is_not_seedable() {
        // Spec D2: `experimental` is never comparable to canon. If it were in
        // `canon()` it would get a board.
        let experimental = straf3_replay::physics_digest(&PhysicsProfile::experimental());
        assert!(by_digest(experimental).is_none());
    }
}
