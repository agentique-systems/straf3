//! The course the examples fly, compiled from the committed `.map` source.
//!
//! # Why this module exists at all
//!
//! Because the alternative is two copies of the geometry, and two copies drift.
//! The hardcoded arena this replaces was built to state that invariant — one
//! declaration feeding both the picture and the collision — and it stated it by
//! being a Rust array that `mesh.rs` and the tracer both read. A compiled `.map`
//! states the same thing more strongly: the mesh and the hulls are two fields of
//! one [`CompiledMap`], produced by one call, from one string.
//!
//! So this module compiles the map exactly once and hands out borrows of that
//! single value. Nothing here re-derives geometry, and there is no second path
//! by which an example could draw one world and collide with another.
//!
//! # Why `include_str!` and not a file read
//!
//! `web-demo` is a `wasm32` target with no filesystem. Embedding the source at
//! build time is the only way all three examples can share this module, and it
//! has the side benefit that the examples cannot be run against a `.map` that
//! is not the one in the repository.
//!
//! The game binary does the opposite — `straf3 --map <path>` reads bytes at
//! runtime — because a player choosing a map is the point there. Both go
//! through `straf3_map::compile`, which is the code path that matters.

use std::sync::OnceLock;

use straf3_map::{CompiledMap, HullWorld};
use straf3_sim::num::{Scalar, Vec3};

/// The course source, embedded at build time.
///
/// `assets/maps/coil.map`, relative to this file.
pub const SOURCE: &str = include_str!("../../../../assets/maps/coil.map");

/// The compiled course, and the collider built from its hulls.
pub struct Course {
    /// Hulls, mesh, triggers and entities — everything the compiler produced.
    pub map: CompiledMap,
    /// The same hulls, as something [`straf3_sim::step_in_place`] can sweep.
    ///
    /// [`CompiledMap::collider`] returns an owned value, so keeping it beside
    /// the map is what turns it into something with a `'static` borrow.
    ///
    /// `allow(dead_code)` because `offscreen` includes this module to draw the
    /// map and never simulates against it. Building the collider there anyway
    /// is deliberate: it is the same value the other two examples collide with,
    /// and dropping it for the drawing-only case would reintroduce exactly the
    /// two-sources-of-geometry split this module exists to prevent.
    #[allow(dead_code)]
    pub world: HullWorld,
}

static COURSE: OnceLock<Course> = OnceLock::new();

/// The compiled course, compiling it on first use.
///
/// # Panics
///
/// If the committed map does not compile. That is a broken repository, not a
/// runtime condition: the source is embedded, so it cannot have been changed
/// between the build and this call.
pub fn get() -> &'static Course {
    COURSE.get_or_init(|| {
        let map = straf3_map::compile(SOURCE)
            .expect("the committed course must compile — assets/maps/coil.map");
        let world = map.collider();
        Course { map, world }
    })
}

/// Where the player starts, and which way they face.
///
/// Straight off the map's `info_player_start`, already lifted clear of the
/// floor by the compiler's `SPAWN_CLEARANCE`.
pub fn spawn() -> (Vec3, Scalar) {
    let c = get();
    (c.map.spawn, c.map.spawn_yaw)
}
