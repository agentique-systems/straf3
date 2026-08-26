//! What a recording is bound to: the geometry it ran against and the physics
//! that moved it.
//!
//! # This module is contract item C6
//!
//! A ghost is only worth racing if the run it shows happened on the map you
//! are standing in. Nothing in a command stream says which map that was — the
//! commands are key presses and view angles, and they are perfectly happy to
//! be replayed against a wall that has moved. A recording bound to stale
//! collision geometry replays to a *different* run, and it does so silently:
//! the player lands short, the ghost lands long, and the time on screen is a
//! number that was never achieved anywhere.
//!
//! So a `.s3d` carries two identities, and both are checked before a single
//! command is handed out:
//!
//! - a [`WorldId`] — the exact compiled collision geometry, or the exact
//!   analytic world, the run was made against;
//! - a [`PhysicsId`] — the exact movement constants that were in effect.
//!
//! # Why a map's identity is its compiled hulls and not its file
//!
//! Hashing the `.map` source, or its path, or a map "version" someone
//! remembers to bump, all fail the same way: the geometry the physics ran
//! against is the *output* of `straf3-map`'s compiler, not its input. Change
//! how a plane is snapped, or which bevels a swept box gets, or the order
//! faces are emitted in, and identical source text produces a world where a
//! ramp sits a hundredth of a unit lower and a jump that used to land no
//! longer does. A recording bound to the source would replay against that with
//! a straight face.
//!
//! `straf3_map::CompiledMap::collision_digest` is exactly the right number
//! and it already exists: an FNV-1a fold over every compiled hull's plane
//! normals, plane distances, bounds and surface flags, in trace order, as
//! bits, plus every trigger volume — with the render mesh deliberately left
//! out, so recolouring a wall does not invalidate a leaderboard. Recompiling
//! the map changes it. Changing the *compiler* changes it too, even when the
//! `.map` file has not changed a byte. That is the property C6 needs.
//!
//! This crate does not depend on `straf3-map` to say so. It takes the `u64`,
//! because the alternative — every consumer of a saved run also linking a
//! Valve 220 parser — buys nothing: the number is the whole of the binding,
//! and a caller that has a `CompiledMap` writes
//! `WorldId::map(name, compiled.collision_digest())` at the one place a
//! recording is created.
//!
//! # Why the physics identity is derived, never declared
//!
//! [`PhysicsId::of`] folds the bits of every field of the
//! [`PhysicsProfile`] that was actually in effect. It cannot be handed a
//! digest, and the check at replay time recomputes it from the profile the
//! caller is about to simulate with — so unlike the world identity, there is
//! no way to assert a physics binding that the simulation will then not
//! honour. The name travels alongside for the error message and is *not* in
//! the digest: renaming `cpm` does not invalidate a run, and quietly changing
//! `pm_airaccelerate` while keeping the name does.
//!
//! The fold is written as an exhaustive destructuring with no `..` rest
//! pattern. Adding a field to [`PhysicsProfile`] therefore fails to compile
//! here, which is the only way to make "every constant is covered" a fact
//! rather than an intention.
//!
//! # What that costs: this digest identifies the representation, not the behaviour
//!
//! The exhaustive fold has a consequence that is easy to miss and expensive
//! when it bites. **Adding a field moves every profile's digest, including
//! profiles whose movement did not change by a single unit** — even when the
//! new field is added at a disabling value and no existing constant is
//! touched. The digest tracks the shape of the struct, not what the struct
//! makes the player do.
//!
//! This is measured rather than hypothetical, and the instance is worth
//! naming because it cost this repository a committed artefact. Commit
//! `a604820` added eight fields to [`PhysicsProfile`] — `slide_entry_speed`,
//! `slide_friction`, `slide_duration_ms`, `dash_speed`, `dash_window_ms`,
//! `wall_jump_velocity`, `wall_contact_window_ms`, `wall_normal_max` — for
//! candidate mechanics that were gated off. It changed no existing constant:
//! the only lines that commit removed from `profile.rs` were two lines of a
//! doc comment. Nothing about how `vq3` or `cpm` move was different
//! afterwards. Every `vq3` and `cpm` recording in existence stopped loading
//! anyway, because the struct got wider.
//!
//! A personal best committed as `runs/coil.cpm.s3d` in `2c17bcb` was
//! invalidated by `a604820` **the same day**, and nobody found out for nine
//! days — not through inattention, but because nothing in the tree loaded a
//! committed artefact, so there was no moment at which the breakage could
//! announce itself.
//!
//! # Why it is still written this way
//!
//! Because the error is in the safe direction and the alternative's error is
//! not. This digest over-invalidates: it refuses runs that would in fact have
//! replayed identically. A digest that tracked *behaviour* would have to know
//! which fields the movement code branches on, and the day that list is wrong
//! is the day a real divergence replays silently and a leaderboard reports a
//! time nobody achieved. Refusing a good run is a nuisance; accepting a bad
//! one is the thing this module exists to prevent.
//!
//! # The better answer, which this tree already uses elsewhere
//!
//! Separation. `crates/straf3-collision/tests/canon_frozen.rs` hit exactly
//! this problem from the other side and wrote the analysis out: folding every
//! field of `PlayerState` into `SimState::checksum()` means that adding slide
//! and dash timers changes the checksum "for `vq3` and `cpm` too, because the
//! struct they fold grew a field. Canon movement would not have moved a
//! millimetre and the literal gate would be red." Its answer was a *separate*
//! movement digest, narrower than the state checksum and answering the
//! question actually being asked.
//!
//! [`PhysicsId`] has no such separation, and giving it one — a behavioural
//! digest for "would this replay the same?" alongside the exhaustive one for
//! "is this the same build?" — is the known shape of the fix. It is
//! deliberately **not** being done here. Re-cutting replay identity is a
//! change competitive integrity rests on, and it is not a change to make
//! alongside a movement freeze. This section exists so that whoever does it
//! next starts from the analysis rather than rediscovering it from a broken
//! artefact.
//!
//! Until then, the mitigation is to make the breakage *loud* rather than to
//! make it rarer: a committed artefact is loaded by the ordinary test suite,
//! so a widened struct fails a test at the moment it is incurred instead of
//! being discovered nine days later by accident. When that test fires, read
//! this section first — **the physics may not have changed at all.**

use straf3_sim::PhysicsProfile;
use straf3_sim::num::{Scalar, Vec3, to_bits};

use crate::digest::Fnv1a;

/// The world a run was made in, precisely enough to refuse a different one.
///
/// The three variants are the three worlds that exist below the seam:
/// `straf3_sim::world::EmptyWorld`, `straf3_sim::world::FlatGround`, and a
/// compiled map's `HullWorld`. A real timed run is always the third; the first
/// two exist because the determinism harness, the movement tests and this
/// crate's own cross-target check all record runs in them, and a format that
/// could not describe those would have to be bypassed to test itself.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum WorldId {
    /// No geometry at all.
    Empty,
    /// An infinite horizontal plane, identified by the **bits** of its height.
    ///
    /// Bits, not the value, for the same reason every other field here is:
    /// a plane at `-0.0` and a plane at `0.0` are `==` as floats, and the
    /// point of an identity is to notice when something changed.
    Flat {
        /// `f32::to_bits` of the plane's Z.
        height_bits: u32,
    },
    /// A compiled map, identified by
    /// `straf3_map::CompiledMap::collision_digest`.
    Map {
        /// What to call the map when refusing a mismatch. Informational: two
        /// `Map`s with the same digest are the same world whatever they are
        /// called.
        name: String,
        /// The fold over every compiled hull and trigger volume. Recompiling
        /// the map — or changing the compiler — changes this.
        collision_digest: u64,
    },
}

impl WorldId {
    /// An infinite plane at `height`.
    #[must_use]
    pub fn flat(height: Scalar) -> Self {
        Self::Flat {
            height_bits: to_bits(height),
        }
    }

    /// A compiled map. `collision_digest` must come from
    /// `straf3_map::CompiledMap::collision_digest` and nowhere else — see the
    /// module docs on why a source hash is not a substitute.
    #[must_use]
    pub fn map(name: impl Into<String>, collision_digest: u64) -> Self {
        Self::Map {
            name: name.into(),
            collision_digest,
        }
    }

    /// Whether this is the same world as `other`, ignoring a map's name.
    ///
    /// Not `PartialEq`, because `PartialEq` should mean "the same value" and a
    /// renamed map is not the same value even though it is the same world.
    #[must_use]
    pub fn is_same_world(&self, other: &Self) -> bool {
        match (self, other) {
            (Self::Empty, Self::Empty) => true,
            (Self::Flat { height_bits: a }, Self::Flat { height_bits: b }) => a == b,
            (
                Self::Map {
                    collision_digest: a,
                    ..
                },
                Self::Map {
                    collision_digest: b,
                    ..
                },
            ) => a == b,
            _ => false,
        }
    }

    /// The map's name, when this is a map.
    #[must_use]
    pub fn map_name(&self) -> Option<&str> {
        match self {
            Self::Map { name, .. } => Some(name),
            _ => None,
        }
    }
}

impl core::fmt::Display for WorldId {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            Self::Empty => write!(f, "the empty world"),
            Self::Flat { height_bits } => {
                write!(f, "flat ground at z={}", f32::from_bits(*height_bits))
            }
            Self::Map {
                name,
                collision_digest,
            } => write!(f, "map `{name}` (collision {collision_digest:016x})"),
        }
    }
}

/// The movement constants a run was made under.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PhysicsId {
    /// What the profile was called — `cpm`, `vq3`, or whatever a caller names
    /// its own. Informational only; see the module docs.
    pub name: String,
    /// A fold over the bits of every field of the [`PhysicsProfile`].
    pub digest: u64,
}

impl PhysicsId {
    /// Derive an identity from the profile that is actually in effect.
    #[must_use]
    pub fn of(name: impl Into<String>, profile: &PhysicsProfile) -> Self {
        Self {
            name: name.into(),
            digest: physics_digest(profile),
        }
    }
}

impl core::fmt::Display for PhysicsId {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        write!(f, "`{}` ({:016x})", self.name, self.digest)
    }
}

/// Fold the bits of every movement constant in `profile`.
///
/// # Adding a field to `PhysicsProfile` breaks this on purpose
///
/// The destructuring below has no `..`. A new constant in the profile is a new
/// way for two builds to disagree about a run, so it must be a new input to
/// this digest, and the compiler is the only reviewer guaranteed to notice.
/// When that happens: add the field to the fold, in place, and accept that
/// every previously saved recording now reports a physics mismatch — which is
/// the truth, because the physics changed.
#[must_use]
pub fn physics_digest(profile: &PhysicsProfile) -> u64 {
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

    let mut h = Fnv1a::new();
    // A tag, so that a change to *what* this digest covers is itself a change
    // to the digest rather than a silent redefinition. Same device
    // `straf3_map::CompiledMap::collision_digest` uses.
    h.bytes(b"straf3-replay/physics/1");
    let scalar = |v: Scalar, h: &mut Fnv1a| h.bytes(&to_bits(v).to_le_bytes());
    let vector = |v: Vec3, h: &mut Fnv1a| {
        h.bytes(&to_bits(v.x).to_le_bytes());
        h.bytes(&to_bits(v.y).to_le_bytes());
        h.bytes(&to_bits(v.z).to_le_bytes());
    };

    scalar(accelerate, &mut h);
    scalar(friction, &mut h);
    scalar(stop_speed, &mut h);
    scalar(max_speed, &mut h);
    scalar(duck_scale, &mut h);
    scalar(air_accelerate, &mut h);
    scalar(gravity, &mut h);
    scalar(jump_velocity, &mut h);
    scalar(step_height, &mut h);
    scalar(overclip, &mut h);
    h.bytes(&[max_clip_planes]);
    scalar(ground_trace_probe, &mut h);
    scalar(min_walk_normal, &mut h);
    vector(hull_mins, &mut h);
    vector(hull_maxs, &mut h);
    scalar(crouched_height, &mut h);
    scalar(air_control, &mut h);
    scalar(air_stop_accelerate, &mut h);
    scalar(strafe_accelerate, &mut h);
    scalar(strafe_wish_speed_cap, &mut h);
    h.bytes(&double_jump_window_ms.to_le_bytes());
    scalar(double_jump_boost, &mut h);
    // The candidate mechanics (spec rev 3, criterion 4). They are zero in both
    // canon profiles, and folding a zero still changes this digest, so every
    // `.s3d` written before they existed now reports a physics mismatch. That
    // is the truth and not a regression: the profile a run was made under is
    // no longer the profile this build simulates with, even though canon
    // *movement* is unmoved — which is precisely why D9 pins canon over a
    // separate movement digest rather than over this one.
    scalar(slide_entry_speed, &mut h);
    scalar(slide_friction, &mut h);
    h.bytes(&slide_duration_ms.to_le_bytes());
    scalar(dash_speed, &mut h);
    h.bytes(&dash_window_ms.to_le_bytes());
    scalar(wall_jump_velocity, &mut h);
    h.bytes(&wall_contact_window_ms.to_le_bytes());
    scalar(wall_normal_max, &mut h);
    h.finish()
}

#[cfg(test)]
mod tests {
    use super::*;
    use straf3_sim::num::{s, vec3};

    #[test]
    fn cpm_and_vq3_are_not_the_same_physics() {
        assert_ne!(
            physics_digest(&PhysicsProfile::cpm()),
            physics_digest(&PhysicsProfile::vq3())
        );
    }

    #[test]
    fn one_changed_constant_changes_the_identity() {
        // The whole point: a tweak to a movement constant must invalidate every
        // recording made before it, because the same commands no longer produce
        // the same run.
        let base = PhysicsProfile::cpm();
        let mut nudged = base;
        nudged.air_accelerate = f32::from_bits(base.air_accelerate.to_bits() + 1);
        assert_ne!(physics_digest(&base), physics_digest(&nudged));

        let mut integer_field = base;
        integer_field.double_jump_window_ms += 1;
        assert_ne!(physics_digest(&base), physics_digest(&integer_field));

        let mut byte_field = base;
        byte_field.max_clip_planes += 1;
        assert_ne!(physics_digest(&base), physics_digest(&byte_field));

        let mut hull = base;
        hull.hull_maxs.z = f32::from_bits(base.hull_maxs.z.to_bits() + 1);
        assert_ne!(physics_digest(&base), physics_digest(&hull));
    }

    #[test]
    fn signed_zero_is_not_the_same_constant() {
        // Folding values rather than bits would make these equal, and
        // `-0.0` reaches different code than `0.0` in more than one place in
        // the step (a zero air-control *value* is the VQ3 switch).
        let mut a = PhysicsProfile::cpm();
        a.air_control = s(0.0);
        let mut b = a;
        b.air_control = s(-0.0);
        assert_eq!(a.air_control, b.air_control);
        assert_ne!(physics_digest(&a), physics_digest(&b));
    }

    /// Spec D2: `experimental` is never comparable to canon, and the mechanism
    /// that enforces it is this digest — a recording made under `experimental`
    /// is refused by a `cpm` client before a command is handed out, whatever
    /// the file is named.
    #[test]
    fn experimental_is_not_the_same_physics_as_either_canon_profile() {
        let x = physics_digest(&PhysicsProfile::experimental());
        assert_ne!(x, physics_digest(&PhysicsProfile::cpm()));
        assert_ne!(x, physics_digest(&PhysicsProfile::vq3()));
    }

    /// Each candidate constant, folded on its own.
    ///
    /// One assertion per field rather than one over the whole profile: a fold
    /// that forgot exactly one constant would still pass a test that only
    /// compared `experimental` with `cpm`, because the other seven would carry
    /// it. That is the failure this exists to catch, and it is the likely one —
    /// eight fields were added to this fold in a single edit.
    #[test]
    fn every_candidate_constant_is_folded() {
        let base = PhysicsProfile::cpm();
        type Edit = (&'static str, fn(PhysicsProfile) -> PhysicsProfile);
        let edits: &[Edit] = &[
            ("slide_entry_speed", |p| PhysicsProfile {
                slide_entry_speed: s(400.0),
                ..p
            }),
            ("slide_friction", |p| PhysicsProfile {
                slide_friction: s(1.0),
                ..p
            }),
            ("slide_duration_ms", |p| PhysicsProfile {
                slide_duration_ms: 600,
                ..p
            }),
            ("dash_speed", |p| PhysicsProfile {
                dash_speed: s(400.0),
                ..p
            }),
            ("dash_window_ms", |p| PhysicsProfile {
                dash_window_ms: 400,
                ..p
            }),
            ("wall_jump_velocity", |p| PhysicsProfile {
                wall_jump_velocity: s(200.0),
                ..p
            }),
            ("wall_contact_window_ms", |p| PhysicsProfile {
                wall_contact_window_ms: 200,
                ..p
            }),
            ("wall_normal_max", |p| PhysicsProfile {
                wall_normal_max: s(0.3),
                ..p
            }),
        ];
        for (name, edit) in edits {
            assert_ne!(
                physics_digest(&base),
                physics_digest(&edit(base)),
                "`{name}` is not folded into the physics digest"
            );
        }
    }

    #[test]
    fn a_profiles_name_is_not_part_of_its_identity() {
        let p = PhysicsProfile::cpm();
        assert_eq!(
            PhysicsId::of("cpm", &p).digest,
            PhysicsId::of("something-else", &p).digest
        );
    }

    #[test]
    fn a_recompiled_map_is_a_different_world() {
        let before = WorldId::map("coil", 0x1234_5678_9abc_def0);
        let after = WorldId::map("coil", 0x1234_5678_9abc_def1);
        assert!(!before.is_same_world(&after));
        // ...and renaming it is not.
        assert!(before.is_same_world(&WorldId::map("coil-v2", 0x1234_5678_9abc_def0)));
    }

    #[test]
    fn the_analytic_worlds_do_not_pass_for_each_other() {
        assert!(!WorldId::Empty.is_same_world(&WorldId::flat(s(0.0))));
        assert!(!WorldId::flat(s(0.0)).is_same_world(&WorldId::flat(s(64.0))));
        assert!(!WorldId::flat(s(0.0)).is_same_world(&WorldId::flat(s(-0.0))));
        assert!(!WorldId::Empty.is_same_world(&WorldId::map("m", 0)));
        assert!(WorldId::flat(s(64.0)).is_same_world(&WorldId::flat(s(64.0))));
    }

    #[test]
    fn the_hull_vectors_fold_component_by_component() {
        // Guards against a fold that only looked at, say, `.z`.
        let base = PhysicsProfile::cpm();
        for axis in 0..3 {
            let mut p = base;
            let v: &mut Vec3 = &mut p.hull_mins;
            let component = match axis {
                0 => &mut v.x,
                1 => &mut v.y,
                _ => &mut v.z,
            };
            *component = f32::from_bits(component.to_bits() + 1);
            assert_ne!(
                physics_digest(&base),
                physics_digest(&p),
                "hull_mins component {axis} is not folded"
            );
        }
        assert_eq!(
            physics_digest(&base),
            physics_digest(&PhysicsProfile::cpm())
        );
        // And the fold is over the profile, not over some cached state.
        let mut zeroed = base;
        zeroed.hull_mins = vec3(s(0.0), s(0.0), s(0.0));
        assert_ne!(physics_digest(&base), physics_digest(&zeroed));
    }
}
