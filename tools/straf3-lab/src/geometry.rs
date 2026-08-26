//! The worlds the lab measures in: the shared fixtures, not a copy of them.
//!
//! # Why this module is one `pub use` and not a set of constructors
//!
//! Session A's decisions of record (§4) rule that the ramp, step, ledge and
//! corner fixtures live **once**, in [`straf3_collision::testbed`], owned by the
//! sim seat and consumed both by its tests and by this crate. Criterion 3 asks
//! for each emergent behaviour to have a sim test pinning it *and* a lab
//! measurement quantifying it; those two are only saying something about the
//! same world if they import the same world. Two implementations asserted to be
//! identical is not the same claim, and the difference is exactly the kind that
//! survives a review and fails a wave later.
//!
//! This crate did carry a mirror for part of the wave — written from that
//! module's published API and coordinate table, because it was in the sim seat's
//! unmerged worktree at the time — and the results document said so as a stated
//! limit. The swap to this delegation moved no measurement: sections 3 and 5 of
//! `docs/movement-lab.md` are byte-identical either way, which is worth recording
//! because it is the only evidence that the two ever agreed.
//!
//! # Why brushes and not an analytic half-space
//!
//! (The reason the shared module exists at all, restated here because it is the
//! thing a future reader will be tempted to undo.) Edge clipping and overbounce
//! are not properties of *slopes*. They are artefacts of `straf3-collision`'s
//! brush tracer — its bevel planes, its broadphase epsilon, the `< 0.999`
//! duplicate-plane test, the tie-break that gives an equal entry fraction to the
//! earlier plane. Measured against `movement.rs`'s analytic `SlopedGround`, the
//! numbers would be true statements about a world nobody plays in.

pub use straf3_collision::testbed::{
    CORNER_INNER, FAR, FLOOR_TOP, HALF_WIDTH, LEDGE_EDGE_X, PLATFORM_EDGE_X, RAMP_RUN,
    STEP_RISER_X, THICKNESS, ceiling_at, corner, drop_from, floor, ledge, ramp, ramp_normal,
    ramp_rise, step,
};

use straf3_sim::num::{Scalar, s};

/// Whether these worlds are the lab's own copy of the shared fixtures or the
/// shared fixtures themselves. Printed in the results document: the pairing
/// criterion 3 asks for is only checkable if a reader knows which.
pub const MIRRORED: bool = false;

/// Where a standing player's origin comes to rest on a surface at height `z`.
///
/// 24 is `-hull_mins.z`; 0.125 is the mover's `SURFACE_CLIP_EPSILON`, which
/// holds the hull clear of whatever it is standing on. Lives here rather than in
/// `testbed` because it is a fact about the *mover*, not about the geometry —
/// the fixtures would be the same shape under a different hull — and because
/// only measurements that place a player without settling them first need it.
#[must_use]
pub fn resting_origin_z(surface_z: Scalar) -> Scalar {
    surface_z + s(24.0) + s(0.125)
}

#[cfg(test)]
mod tests {
    use super::*;
    use straf3_sim::num::vec3;
    use straf3_sim::world::{Sweep, Trace, World};

    fn probe_down<W: World>(world: &W, from: straf3_sim::num::Vec3) -> Trace {
        world.trace(&Sweep {
            start: from,
            end: from - vec3(s(0.0), s(0.0), s(512.0)),
            half_extents: vec3(s(15.0), s(15.0), s(28.0)),
            center_offset: vec3(s(0.0), s(0.0), s(4.0)),
        })
    }

    fn landed_z<W: World>(world: &W, from: straf3_sim::num::Vec3) -> Scalar {
        from.z - s(512.0) * probe_down(world, from).fraction
    }

    /// The property every ramp measurement rests on: the normal the tracer
    /// reports is exactly the one [`ramp_normal`] names, so a measurement may
    /// compare it against `min_walk_normal` without reconstructing an angle.
    ///
    /// `testbed` pins this too. It is asserted again from this side because it
    /// is the assumption *this crate* makes, and a consumer's assumption that is
    /// only checked in the provider's tests is checked by nobody the day the
    /// provider's tests are refactored.
    #[test]
    fn the_ramp_normal_is_what_the_tracer_reports() {
        for degrees in [s(5.0), s(15.0), s(30.0), s(44.0), s(45.0), s(60.0)] {
            let world = ramp(degrees);
            let x = RAMP_RUN * s(0.5);
            let surface = ramp_rise(degrees) * s(0.5);
            let t = probe_down(&world, vec3(x, s(0.0), surface + s(64.0)));
            assert!(t.hit(), "no ramp under the probe at {degrees} degrees");
            assert_eq!(
                t.normal,
                ramp_normal(degrees),
                "ramp({degrees}) traced a different normal than it advertises"
            );
        }
    }

    /// The coordinates the measurements place players against, asserted from the
    /// consumer's side. If `testbed` ever moves a surface, this fails here
    /// rather than silently relocating a measurement.
    #[test]
    fn the_surfaces_are_where_the_measurements_expect_them() {
        assert!(
            (landed_z(&floor(), vec3(s(0.0), s(0.0), s(200.0))) - resting_origin_z(FLOOR_TOP))
                .abs()
                < s(0.5)
        );
        let stepped = step(s(18.0));
        assert!(
            (landed_z(&stepped, vec3(s(256.0), s(0.0), s(200.0))) - resting_origin_z(s(18.0)))
                .abs()
                < s(0.5)
        );
        assert!(
            (landed_z(&stepped, vec3(s(-256.0), s(0.0), s(200.0))) - resting_origin_z(FLOOR_TOP))
                .abs()
                < s(0.5)
        );
        let dropped = ledge(s(128.0));
        assert!(
            (landed_z(&dropped, vec3(s(256.0), s(0.0), s(200.0))) - resting_origin_z(s(-128.0)))
                .abs()
                < s(0.5)
        );
        let platform = drop_from(s(128.0));
        assert!(
            (landed_z(&platform, vec3(s(-256.0), s(0.0), s(400.0))) - resting_origin_z(s(128.0)))
                .abs()
                < s(0.5)
        );
        assert!(
            (landed_z(&platform, vec3(s(64.0), s(0.0), s(400.0))) - resting_origin_z(FLOOR_TOP))
                .abs()
                < s(0.5)
        );
    }

    /// Hull heights in `PhysicsProfile` are measured from the origin, which sits
    /// 24 above the feet, so a standing player is 56 tall and a crouched one is
    /// 40. The band admitting one and refusing the other is `40 < z < 56`;
    /// `ceiling_at(20)` admits neither, which is the correction to the framing
    /// in `crates/straf3-sim/tests/movement.rs`.
    #[test]
    fn a_ceiling_admits_a_crouched_hull_and_refuses_a_standing_one() {
        let at = |world: &straf3_collision::HullWorld, half_z: Scalar, centre_z: Scalar| {
            world.trace(&Sweep {
                start: vec3(s(0.0), s(0.0), resting_origin_z(FLOOR_TOP)),
                end: vec3(s(0.0), s(0.0), resting_origin_z(FLOOR_TOP)),
                half_extents: vec3(s(15.0), s(15.0), half_z),
                center_offset: vec3(s(0.0), s(0.0), centre_z),
            })
        };
        let world = ceiling_at(s(48.0));
        // Standing: mins.z −24, maxs.z 32 → half 28, centre +4. Spans 56.
        assert!(at(&world, s(28.0), s(4.0)).start_solid);
        // Crouched: mins.z −24, maxs.z 16 → half 20, centre −4. Spans 40.
        assert!(!at(&world, s(20.0), s(-4.0)).start_solid);
        // And 20 admits neither.
        assert!(at(&ceiling_at(s(20.0)), s(20.0), s(-4.0)).start_solid);
    }
}
