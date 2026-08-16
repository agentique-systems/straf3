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
//! # Why it is hand-written, and why parry3d is still in the manifest
//!
//! `World`'s contract is *pure and deterministic, bit-identical*. ARCHITECTURE
//! C8 adds to it: identical **across targets**, because a run recorded in a
//! browser has to check out against the same run replayed on a server. That
//! rules out a broadphase cache, a BVH built in a nondeterministic order,
//! work-stealing parallelism, and any `f32::sin`/`cos`/`tan`/`exp`/`powf` or
//! SIMD path that exists on one target and not another.
//!
//! "Is parry deterministic?" was already an open question the spec made
//! conditional on an audit. "Is parry deterministic *across targets*?" is a
//! materially harder one, and the whole point of the `World` seam is that it can
//! stay unanswered cheaply. So this crate answers by hand. Every operation in
//! the trace loop is an IEEE add, subtract, multiply, divide, compare or square
//! root, in a fixed order, over a fixed list.
//!
//! `parry3d` remains a declared dependency and is used by nothing. Its manifest
//! comment says it is confined here so a determinism audit has one place to
//! look; leaving it declared keeps that true, and keeps the decision reversible.
//! It is not on the trace path and must not become so without that being called
//! out as an architectural change rather than an implementation detail.
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
pub use trace::{HullWorld, trace_hull};
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
