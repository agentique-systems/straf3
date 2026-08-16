//! Swept box queries against static convex geometry: the crate that answers
//! [`straf3_sim::world::World`].
//!
//! Below the seam: no rendering, no windowing, no GPU, no filesystem, no clock.
//!
//! # What this crate is
//!
//! One algorithm and the data it needs. A [`Hull`] is a convex solid described
//! as the intersection of outward half-spaces — a Quake brush. A [`HullWorld`]
//! is a list of them, and it implements the simulation's `World` trait by
//! sweeping the player's box against each in turn with [`trace_hull`], which is
//! Quake 3's `CM_TraceThroughBrush` transcribed.
//!
//! Everything else here exists to build hulls out of `.map` brush planes and to
//! do it the way `q3map` did: [`plane`] for the face-to-plane conversion,
//! [`winding`] for turning planes back into polygons, and [`hull`] for the
//! bevel planes a swept *box* needs that a swept *point* does not.
//!
//! # Why it is hand-written, and why there is no geometry library here
//!
//! `World`'s contract is *pure and deterministic, bit-identical*. ARCHITECTURE
//! C8 adds to it: identical **across targets**, because a run recorded in a
//! browser has to check out against the same run replayed on a server. That
//! rules out a broadphase cache, a BVH built in a nondeterministic order,
//! work-stealing parallelism, and any `f32::sin`/`cos`/`tan`/`exp`/`powf` or
//! SIMD path that exists on one target and not another.
//!
//! `parry3d` was the candidate, and "is parry deterministic?" was already an
//! open question the spec made conditional on an audit. "Is parry deterministic
//! *across targets*?" is a materially harder one, and the whole point of the
//! `World` seam is that it can stay unanswered cheaply. So this crate answers by
//! hand. Every operation in the trace loop is an IEEE add, subtract, multiply,
//! divide, compare or square root, in a fixed order, over a fixed list.
//!
//! Because the answer is hand-written, parry was declared and called by nothing,
//! so it is no longer a dependency of this crate or of the workspace. The seam
//! is what keeps that decision reversible — not the manifest line. Putting any
//! geometry library on the trace path means running the across-target audit
//! first, and is an architectural change rather than an implementation detail.
//!
//! # Where the geometry comes from
//!
//! `straf3-map` compiles `.map` source into hulls and hands them over; nothing
//! in this crate reads a file or knows what a `.map` is. The recommended path
//! for one brush is [`hull::compile_hull`], which deduplicates the planes,
//! builds the face polygons, takes the bounding box from them, and adds the
//! bevels — the same sequence, in the same order, that `q3map` put every Quake 3
//! brush through before the game ever traced against it.

#![forbid(unsafe_code)]
#![warn(missing_docs)]
#![warn(clippy::all)]

pub mod hull;
pub mod plane;
pub mod trace;
pub mod winding;

pub use hull::{CompiledHull, Hull, add_hull_bevels, axial_planes, compile_hull};
pub use plane::Plane;
pub use trace::{HullWorld, TriggerHull, trace_hull};
pub use winding::{Winding, hull_windings, winding_bounds};

#[cfg(test)]
mod tests {
    use super::*;
    use straf3_sim::num::{s, vec3};
    use straf3_sim::world::{SurfaceFlags, Sweep, World};

    /// The crate's whole surface, in the shape a map compiler uses it: planes
    /// in, a world out, a sweep answered.
    #[test]
    fn planes_in_a_world_out() {
        let hull = Hull::from_planes(
            &axial_planes(
                vec3(s(-64.0), s(-64.0), s(-16.0)),
                vec3(s(64.0), s(64.0), s(0.0)),
            ),
            SurfaceFlags::NONE,
        )
        .unwrap();
        let world: HullWorld = [hull].into_iter().collect();

        let t = world.trace(&Sweep {
            start: vec3(s(0.0), s(0.0), s(64.0)),
            end: vec3(s(0.0), s(0.0), s(0.0)),
            half_extents: vec3(s(15.0), s(15.0), s(28.0)),
            center_offset: vec3(s(0.0), s(0.0), s(4.0)),
        });
        assert!(t.hit());
        assert_eq!(t.normal, vec3(s(0.0), s(0.0), s(1.0)));
    }
}
