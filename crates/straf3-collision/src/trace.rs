//! Quake 3's `CM_TraceThroughBrush`, generalised to hulls of any plane count.
//!
//! This is `arena.rs`'s `trace_brush` with the fixed `[Plane; 7]` removed and
//! nothing else changed. Every deliberate property of that transcription is
//! deliberate here, and each one is load-bearing for something above:
//!
//! - **No `SURFACE_CLIP_EPSILON`.** Q3 folds it into the trace; this does not,
//!   because `straf3-sim`'s mover applies it itself in `step.rs`'s `sweep_to`.
//!   Applying it in both places would hold the player two epsilons off every
//!   surface. (The one place the epsilon *does* appear here is the broadphase
//!   reject, where it only ever makes the test more forgiving.)
//! - **Ties go to the earlier hull, and the earlier plane.** The comparison is
//!   a strict `<`, so a later hull never displaces an equal-fraction hit. A map
//!   compiler emitting hulls in file order gets file order as its tie-break, and
//!   the digest that binds a recording to its geometry stays meaningful.
//! - **Starting inside a solid does not limit motion.** Q3 records `start_solid`
//!   and lets the mover move out; only a sweep that is inside for its whole
//!   length is `all_solid`, and that one stops dead. `PM_CorrectAllSolid` is
//!   written against exactly this distinction.
//! - **`all_solid` implies `fraction == 0`.** Stated as an invariant by
//!   ARCHITECTURE C8, asserted in debug builds here, and what lets the aborted
//!   step-up in `step.rs` be safe without a second rollback point.
//!
//! There is no acceleration structure. A linear scan over a fixed list has no
//! build order to get wrong, no cache to invalidate and no traversal that could
//! differ between two runs — which is the whole reason this crate answers the
//! `World` trait by hand instead of delegating to a library whose cross-target
//! determinism nobody has audited.

use straf3_sim::num::{Scalar, Vec3, s};
use straf3_sim::world::{Sweep, Trace, World};

use crate::hull::Hull;

/// Q3's `SURFACE_CLIP_EPSILON`, used *only* to widen the broadphase reject.
///
/// The trace itself does not apply it — see the module docs. Here it makes
/// `bounds_intersect` err towards tracing a hull it could have skipped, which
/// costs a few plane tests and can never cost a collision.
const BOUNDS_EPSILON: Scalar = s(0.125);

/// Sweep the player's box against one hull, narrowing `best` in place.
///
/// `best` carries the running earliest hit across every hull, exactly as Q3's
/// `trace_t` does across the brushes of a leaf. Start from [`Trace::clear`].
///
/// Exposed separately from [`HullWorld`] because "did this sweep touch *that*
/// volume" is a different question from "what stopped the player", and the run
/// clock will need the first one per trigger volume, inside the physics path,
/// without building a throwaway world for each.
pub fn trace_hull(hull: &Hull, sweep: &Sweep, best: &mut Trace) {
    // The hull's centre, not the player origin: Quake's player box is not
    // centred on its origin (see `Sweep::center_offset`).
    let start = sweep.start + sweep.center_offset;
    let end = sweep.end + sweep.center_offset;
    let h = sweep.half_extents.abs();

    // Q3's `CM_BoundsIntersect` guard from `CM_TraceThroughLeaf`. A hull whose
    // bounding box the swept box never reaches cannot be hit by it, and on a
    // real map this is what keeps a linear scan over a few thousand hulls from
    // costing a few thousand plane loops.
    if !bounds_intersect(start.min(end) - h, start.max(end) + h, hull.mins, hull.maxs) {
        return;
    }

    let mut enter_frac = s(-1.0);
    let mut leave_frac = s(1.0);
    let mut clip: Option<Vec3> = None;
    let mut getout = false;
    let mut startout = false;

    for plane in &hull.planes {
        // Push the plane out by the box's support along its normal, so the
        // swept box becomes a swept point.
        let dist = plane.dist + plane.support_offset(h);

        let d1 = start.dot(plane.normal) - dist;
        let d2 = end.dot(plane.normal) - dist;

        if d2 > s(0.0) {
            getout = true; // the endpoint is outside this face
        }
        if d1 > s(0.0) {
            startout = true; // the start is outside this face
        }

        // Both ends outside one face of a convex solid means the whole segment
        // is, so it cannot touch the solid at all.
        if d1 > s(0.0) && (d2 >= d1 || d2 >= s(0.0)) {
            return;
        }
        // Both ends inside this face: it never bounds the crossing.
        if d1 <= s(0.0) && d2 <= s(0.0) {
            continue;
        }

        let f = d1 / (d1 - d2);
        if d1 > d2 {
            if f > enter_frac {
                enter_frac = f;
                clip = Some(plane.normal);
            }
        } else if f < leave_frac {
            leave_frac = f;
        }
    }

    if !startout {
        // Started inside this solid. Q3 records that and does not let the hull
        // limit the motion — the mover has its own unstick path
        // (`PM_CorrectAllSolid`) and needs to be able to move out.
        best.start_solid = true;
        if !getout {
            best.all_solid = true;
            best.fraction = s(0.0);
        }
        return;
    }

    if enter_frac < leave_frac && enter_frac > s(-1.0) {
        let f = enter_frac.max(s(0.0));
        if f < best.fraction {
            best.fraction = f;
            best.normal = clip.unwrap_or(Vec3::ZERO);
            best.surface = hull.surface;
        }
    }

    debug_assert!(
        !best.all_solid || best.fraction == s(0.0),
        "all_solid must imply fraction == 0 (ARCHITECTURE C8)"
    );
}

/// Q3's `CM_BoundsIntersect`, epsilon and all.
fn bounds_intersect(mins: Vec3, maxs: Vec3, mins2: Vec3, maxs2: Vec3) -> bool {
    !(maxs.x < mins2.x - BOUNDS_EPSILON
        || maxs.y < mins2.y - BOUNDS_EPSILON
        || maxs.z < mins2.z - BOUNDS_EPSILON
        || mins.x > maxs2.x + BOUNDS_EPSILON
        || mins.y > maxs2.y + BOUNDS_EPSILON
        || mins.z > maxs2.z + BOUNDS_EPSILON)
}

/// A world made of convex hulls: the [`World`] a compiled map answers with.
///
/// Owns its hulls rather than borrowing them. The alternative — a view over a
/// map's storage — buys nothing: this is built once when a map loads and held
/// for the whole run, exactly as the hardcoded `ARENA` static was, and lifetimes
/// on the type would propagate into every caller that holds a world.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct HullWorld {
    hulls: Vec<Hull>,
}

impl HullWorld {
    /// A world of these hulls, traced in the order given.
    ///
    /// The order is part of the answer, not an implementation detail: equal
    /// entry fractions are broken by it.
    #[must_use]
    pub fn new(hulls: Vec<Hull>) -> Self {
        Self { hulls }
    }

    /// The hulls, in trace order.
    #[must_use]
    pub fn hulls(&self) -> &[Hull] {
        &self.hulls
    }

    /// Whether this world has any geometry at all.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.hulls.is_empty()
    }

    /// The bounding box of every hull, or `None` when there is no geometry.
    #[must_use]
    pub fn bounds(&self) -> Option<(Vec3, Vec3)> {
        let mut hulls = self.hulls.iter();
        let first = hulls.next()?;
        let (mut mins, mut maxs) = (first.mins, first.maxs);
        for hull in hulls {
            mins = mins.min(hull.mins);
            maxs = maxs.max(hull.maxs);
        }
        Some((mins, maxs))
    }
}

impl FromIterator<Hull> for HullWorld {
    fn from_iter<I: IntoIterator<Item = Hull>>(iter: I) -> Self {
        Self::new(iter.into_iter().collect())
    }
}

impl World for HullWorld {
    /// Earliest hit across every hull, in the order they were given.
    ///
    /// Deterministic by construction: a fixed iteration order over a fixed list,
    /// no cache, no clock, no parallelism, and every operation in the loop is an
    /// IEEE add, subtract, multiply, divide or compare — no transcendental, no
    /// `mul_add`, nothing whose result depends on which target compiled it.
    fn trace(&self, sweep: &Sweep) -> Trace {
        let mut best = Trace::clear();
        for hull in &self.hulls {
            trace_hull(hull, sweep, &mut best);
        }
        best
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::plane::Plane;
    use straf3_sim::num::{to_bits, vec3};
    use straf3_sim::world::SurfaceFlags;

    /// The standing player hull, in the shape `Sweep` wants.
    fn sweep(from: Vec3, to: Vec3) -> Sweep {
        Sweep {
            start: from,
            end: to,
            half_extents: vec3(s(15.0), s(15.0), s(28.0)),
            center_offset: vec3(s(0.0), s(0.0), s(4.0)),
        }
    }

    fn floor() -> Hull {
        Hull::from_aabb(
            vec3(s(-512.0), s(-512.0), s(-64.0)),
            vec3(s(512.0), s(512.0), s(0.0)),
            SurfaceFlags::NONE,
        )
    }

    #[test]
    fn a_fall_onto_a_box_stops_at_its_top() {
        let world = HullWorld::new(vec![floor()]);
        // The origin sits 24 above the feet: feet from +100 to -100 crosses the
        // floor at exactly half.
        let t = world.trace(&sweep(
            vec3(s(0.0), s(0.0), s(124.0)),
            vec3(s(0.0), s(0.0), s(-76.0)),
        ));
        assert!(t.hit());
        assert_eq!(t.fraction, s(0.5));
        assert_eq!(t.normal, vec3(s(0.0), s(0.0), s(1.0)));
        assert!(!t.start_solid);
    }

    #[test]
    fn a_sweep_that_reaches_nothing_is_clear() {
        let world = HullWorld::new(vec![floor()]);
        let t = world.trace(&sweep(
            vec3(s(0.0), s(0.0), s(24.125)),
            vec3(s(400.0), s(0.0), s(24.125)),
        ));
        assert!(!t.hit());
        assert_eq!(t.normal, Vec3::ZERO);
    }

    #[test]
    fn starting_inside_a_hull_reports_it_without_stopping_the_move() {
        let world = HullWorld::new(vec![Hull::from_aabb(
            vec3(s(-64.0), s(-64.0), s(0.0)),
            vec3(s(64.0), s(64.0), s(128.0)),
            SurfaceFlags::NONE,
        )]);
        // Buried in the middle, moving out towards +X.
        let t = world.trace(&sweep(
            vec3(s(0.0), s(0.0), s(40.0)),
            vec3(s(400.0), s(0.0), s(40.0)),
        ));
        assert!(t.start_solid);
        assert!(
            !t.all_solid,
            "there is a way out, so motion must not be barred"
        );
        assert_eq!(t.fraction, s(1.0));
    }

    #[test]
    fn a_sweep_that_never_leaves_solid_is_all_solid_and_travels_nowhere() {
        let world = HullWorld::new(vec![Hull::from_aabb(
            vec3(s(-512.0), s(-512.0), s(0.0)),
            vec3(s(512.0), s(512.0), s(512.0)),
            SurfaceFlags::NONE,
        )]);
        let inside = vec3(s(0.0), s(0.0), s(128.0));
        let t = world.trace(&sweep(inside, inside + vec3(s(16.0), s(0.0), s(0.0))));
        assert!(t.start_solid);
        assert!(t.all_solid);
        // The C8 invariant, stated as a test rather than left incidental.
        assert_eq!(t.fraction, s(0.0));
    }

    #[test]
    fn the_surface_flags_of_whatever_was_hit_come_back() {
        let mut slick = floor();
        slick.surface = SurfaceFlags::SLICK;
        let world = HullWorld::new(vec![slick]);
        let t = world.trace(&sweep(
            vec3(s(0.0), s(0.0), s(124.0)),
            vec3(s(0.0), s(0.0), s(-76.0)),
        ));
        assert!(t.hit());
        assert!(t.surface.contains(SurfaceFlags::SLICK));
    }

    #[test]
    fn a_tie_goes_to_the_earlier_hull() {
        // Two hulls whose faces are at exactly the same place. The first one
        // listed must be the one that answers, because a map compiler binds its
        // digest to the order it emitted them in.
        let a = Hull::from_aabb(
            vec3(s(64.0), s(-64.0), s(0.0)),
            vec3(s(128.0), s(64.0), s(128.0)),
            SurfaceFlags::NONE,
        );
        let mut b = a.clone();
        b.surface = SurfaceFlags::SLICK;

        let forwards = HullWorld::new(vec![a.clone(), b.clone()]);
        let backwards = HullWorld::new(vec![b, a]);
        let s1 = sweep(
            vec3(s(0.0), s(0.0), s(24.0)),
            vec3(s(200.0), s(0.0), s(24.0)),
        );

        assert_eq!(forwards.trace(&s1).surface, SurfaceFlags::NONE);
        assert_eq!(backwards.trace(&s1).surface, SurfaceFlags::SLICK);
        assert_eq!(forwards.trace(&s1).fraction, backwards.trace(&s1).fraction);
    }

    #[test]
    fn the_broadphase_reject_changes_no_answer() {
        // The same geometry, once with honest bounds and once with bounds so
        // large the reject can never fire. If the reject ever culls a hull the
        // plane loop would have hit, these disagree.
        let honest = Hull::from_aabb(
            vec3(s(64.0), s(-64.0), s(0.0)),
            vec3(s(128.0), s(64.0), s(128.0)),
            SurfaceFlags::NONE,
        );
        let mut wide = honest.clone();
        wide.mins = vec3(s(-4096.0), s(-4096.0), s(-4096.0));
        wide.maxs = vec3(s(4096.0), s(4096.0), s(4096.0));

        let a = HullWorld::new(vec![honest]);
        let b = HullWorld::new(vec![wide]);
        for (from, to) in [
            (
                vec3(s(0.0), s(0.0), s(24.0)),
                vec3(s(200.0), s(0.0), s(24.0)),
            ),
            (
                vec3(s(96.0), s(0.0), s(400.0)),
                vec3(s(96.0), s(0.0), s(0.0)),
            ),
            (
                vec3(s(-100.0), s(-100.0), s(60.0)),
                vec3(s(300.0), s(120.0), s(60.0)),
            ),
            (
                vec3(s(0.0), s(0.0), s(24.0)),
                vec3(s(0.0), s(400.0), s(24.0)),
            ),
        ] {
            let s1 = sweep(from, to);
            let (ta, tb) = (a.trace(&s1), b.trace(&s1));
            assert_eq!(
                to_bits(ta.fraction),
                to_bits(tb.fraction),
                "{from:?}->{to:?}"
            );
            assert_eq!(ta.normal, tb.normal, "{from:?}->{to:?}");
            assert_eq!(ta.start_solid, tb.start_solid);
        }
    }

    /// The bevel argument, made a number.
    ///
    /// A brush rotated 45° about Z, walked into along +X at the height of its
    /// middle. The box's flat +X face meets the brush's west *vertex*, so what
    /// decides where it stops is the bevel plane through that vertex — here the
    /// axial `-X` plane on the brush's bounding box, which
    /// [`Hull::from_planes`] adds and a raw plane list does not have.
    ///
    /// Without it the two diagonal faces answer instead, each expanded by the
    /// box's support along its own normal — `15·(0.707 + 0.707)` rather than the
    /// 15 that is actually in the way — and the player stops dead 15 units from
    /// anything they can see. That is the failure mode this crate would have
    /// shipped by generalising `arena.rs`'s plane set without generalising
    /// q3map's bevels along with it.
    #[test]
    fn the_bevels_are_what_stop_the_box_at_the_wall_instead_of_short_of_it() {
        let d = core::f32::consts::FRAC_1_SQRT_2;
        let planes = vec![
            Plane::new(vec3(d, d, s(0.0)), s(64.0)),
            Plane::new(vec3(-d, d, s(0.0)), s(64.0)),
            Plane::new(vec3(-d, -d, s(0.0)), s(64.0)),
            Plane::new(vec3(d, -d, s(0.0)), s(64.0)),
            Plane::new(vec3(s(0.0), s(0.0), s(-1.0)), s(0.0)),
            Plane::new(vec3(s(0.0), s(0.0), s(1.0)), s(128.0)),
        ];
        let bevelled = Hull::from_planes(&planes, SurfaceFlags::NONE).unwrap();
        // The same solid, same bounds, but only the faces the mapper drew.
        let raw = Hull {
            planes,
            mins: bevelled.mins,
            maxs: bevelled.maxs,
            surface: SurfaceFlags::NONE,
        };

        let from = vec3(s(-400.0), s(0.0), s(24.0));
        let to = vec3(s(0.0), s(0.0), s(24.0));
        let stopped_at = |hull: Hull| {
            let t = HullWorld::new(vec![hull]).trace(&sweep(from, to));
            assert!(t.hit());
            (t, from + (to - from) * t.fraction)
        };

        let (bevelled_trace, with) = stopped_at(bevelled.clone());
        let (_, without) = stopped_at(raw);

        // Bevelled: the box's +X face lands exactly on the brush's west vertex.
        assert_eq!(bevelled_trace.normal, vec3(s(-1.0), s(0.0), s(0.0)));
        assert!(
            (with.x + s(15.0) - bevelled.mins.x).abs() < s(0.01),
            "stopped with the box face at {}, the wall is at {}",
            with.x + s(15.0),
            bevelled.mins.x
        );
        // Unbevelled: a wall of thin air, most of a player's width out.
        assert!(
            with.x - without.x > s(14.9),
            "the bevels should be worth ~15 units here, they were worth {}",
            with.x - without.x
        );
    }

    #[test]
    fn traces_are_bit_identical_when_repeated() {
        let world = HullWorld::new(vec![floor()]);
        let s1 = sweep(
            vec3(s(-512.0), s(31.5), s(200.0)),
            vec3(s(-380.0), s(-77.25), s(-40.0)),
        );
        let a = world.trace(&s1);
        let b = world.trace(&s1);
        assert_eq!(to_bits(a.fraction), to_bits(b.fraction));
        assert_eq!(to_bits(a.normal.x), to_bits(b.normal.x));
        assert_eq!(to_bits(a.normal.y), to_bits(b.normal.y));
        assert_eq!(to_bits(a.normal.z), to_bits(b.normal.z));
    }

    #[test]
    fn an_empty_world_stops_nothing() {
        let world = HullWorld::default();
        assert!(world.is_empty());
        assert_eq!(world.bounds(), None);
        let t = world.trace(&sweep(Vec3::ZERO, vec3(s(0.0), s(0.0), s(-1000.0))));
        assert!(!t.hit());
    }
}
