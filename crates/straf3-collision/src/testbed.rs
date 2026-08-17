//! Shared test geometry: the worlds a movement measurement is taken in.
//!
//! # Why this is in the shipped crate and not in a test file
//!
//! Two seats need geometry this wave — the sim tests that *pin* ramp boost,
//! overbounce, edge clipping, step-up and slide, and the lab harness that
//! *quantifies* them. If those are two different sets of fixtures then the
//! pairing the spec asks for ("a sim test pins it and a lab measurement
//! quantifies it") is two descriptions of two different worlds, and neither
//! constrains the other. So the worlds live here, once, in the crate both can
//! depend on.
//!
//! # Why they are compiled brushes and not analytic half-spaces
//!
//! `crates/straf3-sim/tests/movement.rs` grew its own analytic `SlopedGround`
//! and `Boxes` fixtures, and they were the right call for what they tested: a
//! slope with no edges isolates the slope. But they are not the tracer the game
//! runs. Edge clipping and overbounce are *artefacts* of the brush tracer —
//! its bevel planes, its `SURFACE_CLIP_EPSILON` backoff, the exact plane
//! ordering that decides which normal answers a tie. A number measured on an
//! analytic half-space describes a world nobody plays in.
//!
//! Everything below therefore goes through [`compile_hull`], the same function
//! `straf3-map` puts every `.map` brush through, so these worlds carry the same
//! bevels and the same plane order a compiled map would have.
//!
//! # Conventions, stated because assertions are written against them
//!
//! - Quake units, **Z up**. A standing player's origin sits 24 units above the
//!   surface underfoot (`-hull_mins.z`), plus the mover's `SURFACE_CLIP_EPSILON`
//!   of 0.125 — so *resting on the floor of these worlds is `origin.z ≈
//!   24.125`*, not 0.
//! - Unless a constructor says otherwise, the **main floor's top face is at
//!   z = 0** ([`FLOOR_TOP`]).
//! - Worlds are long on **X** — that is the axis a run travels along, and the
//!   axis every feature (ramp foot, step riser, ledge edge) is placed on. They
//!   reach [`FAR`] in each direction, which is 8192 units: eight seconds of
//!   running at 1000 ups without leaving the world.
//! - Worlds are [`HALF_WIDTH`] = 1024 units wide on **Y**, except [`floor`] and
//!   [`corner`], which are square.
//! - Every surface is [`SurfaceFlags::NONE`]. Slick ground is a property of a
//!   surface, not of a shape; a test that wants it edits the hull it got back.
//! - There are no timing volumes. These are movement worlds; a run clock is a
//!   different question and [`HullWorld::with_triggers`] is how it gets asked.
//!
//! Brushes are 256 units thick below their top face, which is deep enough that
//! nothing this crate's tracer does can reach through one, and shallow enough
//! that a fall from a platform lands on the floor rather than inside a slab.

use straf3_sim::num::{Scalar, Vec3, s, sin_cos, vec3};
use straf3_sim::world::SurfaceFlags;

use crate::hull::{Hull, axial_planes, compile_hull};
use crate::plane::Plane;
use crate::trace::HullWorld;

/// Z of the main floor's top face in every world here.
pub const FLOOR_TOP: Scalar = s(0.0);

/// How far each world reaches along ±X from the feature at its centre.
///
/// 8192 units: a player holding 1000 ups leaves the world after eight seconds,
/// which is longer than any measurement here runs.
pub const FAR: Scalar = s(8192.0);

/// How far each world reaches along ±Y.
pub const HALF_WIDTH: Scalar = s(1024.0);

/// How thick every brush is below its top face.
pub const THICKNESS: Scalar = s(256.0);

/// Horizontal run of [`ramp`]'s slope, from its foot at x=0 to its top.
///
/// The rise follows from the angle — see [`ramp_rise`] — so that the *slope* is
/// the parameter and the ramp's length is fixed. A fixed run keeps the approach
/// and the top platform in the same place at every angle, which is what makes
/// two angles comparable.
pub const RAMP_RUN: Scalar = s(1024.0);

/// X of [`step`]'s riser face, and of the top of its lower floor.
pub const STEP_RISER_X: Scalar = s(0.0);

/// X of [`ledge`]'s edge: floor at [`FLOOR_TOP`] for x < this, lower beyond it.
pub const LEDGE_EDGE_X: Scalar = s(0.0);

/// X and Y of [`corner`]'s inside corner — both walls' inner faces.
pub const CORNER_INNER: Scalar = s(64.0);

/// X of [`drop_from`]'s platform edge; the platform occupies x below this.
pub const PLATFORM_EDGE_X: Scalar = s(-64.0);

/// Degrees to radians, as `step.rs` computes it (`M_PI * 2 / 360`).
const DEG_TO_RAD: Scalar = s(core::f32::consts::PI * 2.0 / 360.0);

/// One solid box, compiled the way `q3map` compiles a brush.
///
/// Not [`Hull::from_aabb`], which writes the six planes directly: for a box the
/// two agree exactly (`hull.rs` asserts it), and going the long way means these
/// worlds are built by the same call a real map is.
///
/// # Panics
///
/// If the planes enclose no volume — i.e. `mins` is not below `maxs` on every
/// axis. A fixture that silently compiled to nothing would make every test
/// against it pass by having no geometry at all.
#[must_use]
fn brush(mins: Vec3, maxs: Vec3) -> Hull {
    compile_hull(&axial_planes(mins, maxs), SurfaceFlags::NONE)
        .expect("testbed brush encloses no volume")
        .hull
}

/// The unit surface normal of a [`ramp`] of `degrees`.
///
/// Leans away from straight up towards −X, because the surface rises with X.
/// Published so a test can compare it against
/// [`PhysicsProfile::min_walk_normal`] without recomputing the trigonometry,
/// and so that the comparison uses the *same* sine and cosine the geometry was
/// built from.
///
/// [`PhysicsProfile::min_walk_normal`]: straf3_sim::PhysicsProfile::min_walk_normal
#[must_use]
pub fn ramp_normal(degrees: Scalar) -> Vec3 {
    let (sin, cos) = sin_cos(degrees * DEG_TO_RAD);
    vec3(-sin, s(0.0), cos)
}

/// Z of the top of a [`ramp`] of `degrees`, where its flat top platform begins.
///
/// `RAMP_RUN * tan(degrees)`, computed as `sin/cos` from the same [`sin_cos`]
/// the normal uses — `f32::tan` is whichever libm the target links, and this
/// crate's geometry may not depend on that (see [`straf3_sim::num::sin_cos`]).
#[must_use]
pub fn ramp_rise(degrees: Scalar) -> Scalar {
    let (sin, cos) = sin_cos(degrees * DEG_TO_RAD);
    RAMP_RUN * sin / cos
}

/// One large flat brush, top at [`FLOOR_TOP`], reaching [`FAR`] on both
/// horizontal axes.
///
/// The world in which nothing but the mover can explain a number.
#[must_use]
pub fn floor() -> HullWorld {
    HullWorld::new(vec![brush(
        vec3(-FAR, -FAR, FLOOR_TOP - THICKNESS),
        vec3(FAR, FAR, FLOOR_TOP),
    )])
}

/// A flat approach, a slope of `degrees` rising towards +X, and a flat top.
///
/// Three brushes, in trace order:
///
/// 1. **approach** — top at [`FLOOR_TOP`], from `-FAR` to x=0. Long enough to
///    reach any speed the movement code can produce before meeting the slope.
/// 2. **the wedge** — x from 0 to [`RAMP_RUN`]. Its top face is the plane
///    through the world origin with normal [`ramp_normal`], so the surface is
///    at z=0 where it meets the approach and at [`ramp_rise`] where it meets
///    the top. The wedge is a prism along Y: every edge is axis-parallel, so
///    `add_hull_bevels` adds the +Z bevel at the wedge's own bounding box and
///    no edge bevels at all.
/// 3. **top platform** — top at [`ramp_rise`], from [`RAMP_RUN`] to `FAR`.
///
/// The three tops meet exactly: z=0 at x=0, and `ramp_rise(degrees)` at
/// x=`RAMP_RUN`. Nothing is welded — they are three separate hulls meeting at a
/// seam, which is what a compiled `.map` gives the tracer and therefore what a
/// measurement of "crossing onto a ramp" has to be taken across.
///
/// # Panics
///
/// Unless `0 < degrees < 90`. A zero-degree ramp is [`floor`] and a
/// ninety-degree one is a wall; both are better asked for by name than produced
/// by a division that overflowed.
#[must_use]
pub fn ramp(degrees: Scalar) -> HullWorld {
    assert!(
        degrees > s(0.0) && degrees < s(90.0),
        "a testbed ramp must be steeper than flat and shallower than a wall, got {degrees}"
    );
    let rise = ramp_rise(degrees);
    let normal = ramp_normal(degrees);

    let approach = brush(
        vec3(-FAR, -HALF_WIDTH, FLOOR_TOP - THICKNESS),
        vec3(s(0.0), HALF_WIDTH, FLOOR_TOP),
    );

    // The wedge is given five faces and the slope — the six a mapper would draw.
    // Its +Z bevel is *not* supplied: `add_hull_bevels` derives it from the face
    // windings, which puts it exactly at the wedge's true top rather than at
    // whatever height this function guessed. A guessed-too-high +Z plane is a
    // looser half-space than the correct bevel, and a looser bevel is invisible
    // wall.
    let wedge_mins = vec3(s(0.0), -HALF_WIDTH, FLOOR_TOP - THICKNESS);
    let wedge_maxs = vec3(RAMP_RUN, HALF_WIDTH, rise);
    let axial = axial_planes(wedge_mins, wedge_maxs);
    let wedge_planes = vec![
        axial[0], // -X, at the foot
        axial[1], // +X, at the top
        axial[2], // -Y
        axial[3], // +Y
        axial[4], // -Z, the underside
        Plane::through(normal, vec3(s(0.0), s(0.0), FLOOR_TOP)),
    ];
    let wedge = compile_hull(&wedge_planes, SurfaceFlags::NONE)
        .expect("a ramp wedge encloses a volume")
        .hull;

    let top = brush(
        vec3(RAMP_RUN, -HALF_WIDTH, rise - THICKNESS),
        vec3(FAR, HALF_WIDTH, rise),
    );

    HullWorld::new(vec![approach, wedge, top])
}

/// A floor at [`FLOOR_TOP`] with a riser of `height` starting at
/// [`STEP_RISER_X`].
///
/// Two brushes: the floor across the whole world, and a block from x=0 to `FAR`
/// whose top is at `height`. So the walkable surface is z=0 for x<0 and
/// z=`height` for x>0, and the riser's vertical face is the block's −X plane at
/// x=0.
///
/// # Panics
///
/// If `height` is not above [`FLOOR_TOP`].
#[must_use]
pub fn step(height: Scalar) -> HullWorld {
    assert!(
        height > FLOOR_TOP,
        "a testbed step rises above the floor, got {height}"
    );
    HullWorld::new(vec![
        brush(
            vec3(-FAR, -HALF_WIDTH, FLOOR_TOP - THICKNESS),
            vec3(FAR, HALF_WIDTH, FLOOR_TOP),
        ),
        brush(
            vec3(STEP_RISER_X, -HALF_WIDTH, FLOOR_TOP - THICKNESS),
            vec3(FAR, HALF_WIDTH, height),
        ),
    ])
}

/// A floor at [`FLOOR_TOP`] that ends at [`LEDGE_EDGE_X`] and resumes `drop`
/// lower.
///
/// Two brushes with a shared vertical seam at x=0: the upper floor for x<0 with
/// its top at z=0, and the lower floor for x>0 with its top at z=`-drop`. The
/// corner a player runs off is the upper brush's +X edge, at x=0, z=0.
///
/// This is the shape edge clipping happens on, and the reason it happens is
/// visible in the plane list: an axis-aligned box brush carries no bevel beyond
/// its six faces, so the corner at (0, 0) is where the expanded +X plane and the
/// expanded +Z plane meet at a right angle with nothing rounding it off.
///
/// # Panics
///
/// If `drop` is not positive.
#[must_use]
pub fn ledge(drop: Scalar) -> HullWorld {
    assert!(drop > s(0.0), "a testbed ledge drops downward, got {drop}");
    HullWorld::new(vec![
        brush(
            vec3(-FAR, -HALF_WIDTH, FLOOR_TOP - THICKNESS),
            vec3(LEDGE_EDGE_X, HALF_WIDTH, FLOOR_TOP),
        ),
        brush(
            vec3(LEDGE_EDGE_X, -HALF_WIDTH, FLOOR_TOP - drop - THICKNESS),
            vec3(FAR, HALF_WIDTH, FLOOR_TOP - drop),
        ),
    ])
}

/// A floor at [`FLOOR_TOP`] and two walls meeting at an inside corner at
/// x=[`CORNER_INNER`], y=[`CORNER_INNER`].
///
/// The walls are 512 units tall and their inner faces are the +X wall's −X
/// plane and the +Y wall's −Y plane, both at 64. A player is 30 units wide, so
/// the furthest their origin can legally reach is 49 on each axis.
///
/// The two wall brushes overlap in the quadrant beyond the corner. That is what
/// a `.map` does — brushes are not required to be disjoint — and it is
/// deliberate here: the solver meets two planes at the same fraction, which is
/// the crease case.
#[must_use]
pub fn corner() -> HullWorld {
    HullWorld::new(vec![
        brush(
            vec3(-FAR, -FAR, FLOOR_TOP - THICKNESS),
            vec3(FAR, FAR, FLOOR_TOP),
        ),
        brush(
            vec3(CORNER_INNER, -FAR, FLOOR_TOP),
            vec3(FAR, FAR, FLOOR_TOP + s(512.0)),
        ),
        brush(
            vec3(-FAR, CORNER_INNER, FLOOR_TOP),
            vec3(FAR, FAR, FLOOR_TOP + s(512.0)),
        ),
    ])
}

/// A floor at [`FLOOR_TOP`] with a ceiling slab whose **underside** is at `z`.
///
/// For crouch and stand-up questions. Mind the arithmetic: the hull heights in
/// `PhysicsProfile` are measured from the *origin*, which stands 24 units above
/// the feet, so a player standing on this floor is 56 units tall and a crouched
/// one is 40 — not 32 and 16. A ceiling that admits a crouched player and
/// refuses to let them stand is therefore between 40 and 56, and 48 is the
/// obvious choice. A ceiling at 20 admits neither, and a test written against
/// one is testing that the player is stuck rather than that they are crouched.
///
/// # Panics
///
/// If `z` is not above [`FLOOR_TOP`].
#[must_use]
pub fn ceiling_at(z: Scalar) -> HullWorld {
    assert!(
        z > FLOOR_TOP,
        "a testbed ceiling is above the floor, got {z}"
    );
    HullWorld::new(vec![
        brush(
            vec3(-FAR, -HALF_WIDTH, FLOOR_TOP - THICKNESS),
            vec3(FAR, HALF_WIDTH, FLOOR_TOP),
        ),
        brush(
            vec3(-FAR, -HALF_WIDTH, z),
            vec3(FAR, HALF_WIDTH, z + THICKNESS),
        ),
    ])
}

/// A floor at [`FLOOR_TOP`] with a platform of `height` to run off.
///
/// The platform spans x from `-FAR` to [`PLATFORM_EDGE_X`] (−64) with its top at
/// `height`, so a player standing on it and moving towards +X leaves it at x=−64
/// and falls `height` units to the floor. The landing area is clear for the
/// whole remaining length of the world.
///
/// A controlled fall height is what an overbounce sweep needs, and a *platform*
/// rather than a spawn in mid-air is what makes the fall reachable by playing
/// rather than by editing state.
///
/// # Panics
///
/// If `height` is not above [`FLOOR_TOP`].
#[must_use]
pub fn drop_from(height: Scalar) -> HullWorld {
    assert!(
        height > FLOOR_TOP,
        "a testbed drop starts above the floor, got {height}"
    );
    HullWorld::new(vec![
        brush(
            vec3(-FAR, -HALF_WIDTH, FLOOR_TOP - THICKNESS),
            vec3(FAR, HALF_WIDTH, FLOOR_TOP),
        ),
        brush(
            vec3(-FAR, -HALF_WIDTH, height - THICKNESS),
            vec3(PLATFORM_EDGE_X, HALF_WIDTH, height),
        ),
    ])
}

#[cfg(test)]
mod tests {
    use super::*;
    use straf3_sim::num::to_bits;
    use straf3_sim::world::{Sweep, World};

    /// A downward sweep of the standing player hull, landing feet-first.
    fn drop_sweep(x: Scalar, from_z: Scalar, to_z: Scalar) -> Sweep {
        Sweep {
            start: vec3(x, s(0.0), from_z),
            end: vec3(x, s(0.0), to_z),
            half_extents: vec3(s(15.0), s(15.0), s(28.0)),
            center_offset: vec3(s(0.0), s(0.0), s(4.0)),
        }
    }

    /// Where a hull dropped at `x` comes to rest, as the *feet* height.
    fn surface_under<W: World>(world: &W, x: Scalar, from_z: Scalar) -> Option<Scalar> {
        let sweep = drop_sweep(x, from_z, s(-1000.0));
        let t = world.trace(&sweep);
        if !t.hit() {
            return None;
        }
        let travelled = (sweep.end - sweep.start) * t.fraction;
        Some(sweep.start.z + travelled.z - s(24.0))
    }

    #[test]
    fn the_floor_is_where_the_docs_say_it_is() {
        let world = floor();
        assert_eq!(surface_under(&world, s(0.0), s(512.0)), Some(FLOOR_TOP));
        assert_eq!(
            surface_under(&world, FAR - s(64.0), s(512.0)),
            Some(FLOOR_TOP)
        );
        // And the world really does end where FAR says. Past the brush plus the
        // hull's own half-width there is nothing underneath.
        assert_eq!(surface_under(&world, FAR + s(64.0), s(512.0)), None);
    }

    /// The claim the whole ramp fixture rests on: the surface height is
    /// `x * tan(degrees)` at every x along the slope, and the three brushes'
    /// tops agree at both seams.
    #[test]
    fn a_ramp_rises_at_the_angle_it_was_asked_for() {
        for degrees in [s(10.0), s(26.0), s(45.0), s(46.0), s(60.0)] {
            let world = ramp(degrees);
            let rise = ramp_rise(degrees);

            // The approach and the top are flat, at the two stated heights.
            // Approximate, not exact: these sweeps are thousands of units long
            // on a steep ramp, and reconstructing the impact point from a
            // fraction over that distance costs a ten-thousandth of a unit.
            let approach = surface_under(&world, s(-512.0), rise + s(512.0)).expect("approach");
            assert!(
                (approach - FLOOR_TOP).abs() < s(0.01),
                "{degrees}: approach at {approach}"
            );
            let platform =
                surface_under(&world, RAMP_RUN + s(512.0), rise + s(512.0)).expect("top platform");
            assert!(
                (platform - rise).abs() < s(0.01),
                "{degrees}: top platform at {platform}, expected {rise}"
            );

            // And the slope between them. Sampled where the whole hull is over
            // the wedge, because a box straddling a seam rests on the higher of
            // the two surfaces, not on the one under its centre.
            let slope = ramp_normal(degrees);
            let expected_at = |x: Scalar| x * (-slope.x) / slope.z;
            for x in [s(128.0), s(384.0), s(640.0), s(896.0)] {
                let found = surface_under(&world, x, rise + s(512.0)).expect("on the wedge");
                // The hull is 30 wide, so its +X face is 15 further up the
                // slope than its centre and that is what it rests on.
                let want = expected_at(x + s(15.0));
                assert!(
                    (found - want).abs() < s(0.01),
                    "{degrees}: at x={x} the surface is {found}, expected {want}"
                );
            }
        }
    }

    #[test]
    fn the_ramp_normal_is_what_the_tracer_reports() {
        for degrees in [s(26.0), s(45.0), s(46.0), s(60.0)] {
            let world = ramp(degrees);
            let t = world.trace(&drop_sweep(s(512.0), ramp_rise(degrees) + s(64.0), s(0.0)));
            assert!(t.hit(), "{degrees}: nothing under the middle of the slope");
            // Bit-exact: `ramp_normal` is what a test compares against
            // `min_walk_normal`, so it has to be the same number the mover sees.
            assert_eq!(
                to_bits(t.normal.z),
                to_bits(ramp_normal(degrees).z),
                "{degrees}: tracer normal {:?} vs ramp_normal {:?}",
                t.normal,
                ramp_normal(degrees)
            );
        }
    }

    /// The wedge's plane set is the seven a Q3 prism has: six axial and the
    /// slope. Any edge bevel here would mean the wedge is not a prism, and the
    /// numbers taken on it would not be Q3's.
    #[test]
    fn the_wedge_carries_its_axial_bevel_and_no_edge_bevels() {
        let world = ramp(s(45.0));
        let wedge = &world.hulls()[1];
        assert_eq!(
            wedge.planes.len(),
            7,
            "wedge planes: {:?}",
            wedge.planes.iter().map(|p| p.normal).collect::<Vec<_>>()
        );
        // The +Z bevel sits exactly at the wedge's own top, which is the ramp's
        // rise — not at some height this fixture guessed.
        let top = wedge.planes[5];
        assert_eq!(top.normal, vec3(s(0.0), s(0.0), s(1.0)));
        assert!((top.dist - ramp_rise(s(45.0))).abs() < s(0.01));
    }

    #[test]
    fn a_step_is_a_riser_at_x_zero() {
        let world = step(s(18.0));
        assert_eq!(surface_under(&world, s(-512.0), s(512.0)), Some(FLOOR_TOP));
        assert_eq!(surface_under(&world, s(512.0), s(512.0)), Some(s(18.0)));
        // Right at the riser the hull overlaps both, and rests on the taller.
        assert_eq!(
            surface_under(&world, STEP_RISER_X - s(4.0), s(512.0)),
            Some(s(18.0))
        );
    }

    #[test]
    fn a_ledge_drops_at_x_zero() {
        let world = ledge(s(64.0));
        assert_eq!(surface_under(&world, s(-512.0), s(512.0)), Some(FLOOR_TOP));
        assert_eq!(surface_under(&world, s(512.0), s(512.0)), Some(s(-64.0)));
        // A hull whose box still overlaps the upper floor rests on it, which is
        // the whole reason a player can stand with their toes over a drop.
        assert_eq!(
            surface_under(&world, LEDGE_EDGE_X + s(14.0), s(512.0)),
            Some(FLOOR_TOP)
        );
        // One unit further and there is nothing under them but the lower floor.
        assert_eq!(
            surface_under(&world, LEDGE_EDGE_X + s(16.0), s(512.0)),
            Some(s(-64.0))
        );
    }

    #[test]
    fn the_corner_walls_are_where_the_docs_say() {
        let world = corner();
        let sweep = Sweep {
            start: vec3(s(-256.0), s(0.0), s(24.125)),
            end: vec3(s(256.0), s(0.0), s(24.125)),
            half_extents: vec3(s(15.0), s(15.0), s(28.0)),
            center_offset: vec3(s(0.0), s(0.0), s(4.0)),
        };
        let t = world.trace(&sweep);
        assert!(t.hit());
        assert_eq!(t.normal, vec3(s(-1.0), s(0.0), s(0.0)));
        let stopped = sweep.start.x + (sweep.end.x - sweep.start.x) * t.fraction;
        assert!(
            (stopped - (CORNER_INNER - s(15.0))).abs() < s(0.01),
            "stopped at {stopped}, the wall is at {CORNER_INNER}"
        );
    }

    #[test]
    fn a_ceiling_admits_a_crouched_hull_and_refuses_a_standing_one() {
        let world = ceiling_at(s(48.0));
        let at = vec3(s(0.0), s(0.0), s(24.125));
        let fits = |maxs_z: Scalar| {
            !world
                .trace(&Sweep {
                    start: at,
                    end: at,
                    half_extents: vec3(s(15.0), s(15.0), (maxs_z + s(24.0)) * s(0.5)),
                    center_offset: vec3(s(0.0), s(0.0), (maxs_z - s(24.0)) * s(0.5)),
                })
                .start_solid
        };
        assert!(fits(s(16.0)), "the crouched hull should fit under z=48");
        assert!(!fits(s(32.0)), "the standing hull should not");
        // The trap this fixture exists to keep a caller out of: 20 is *not* the
        // crouch-admitting height, because a crouched player is 40 units tall.
        let too_low = ceiling_at(s(20.0));
        let probe = |maxs_z: Scalar| {
            too_low
                .trace(&Sweep {
                    start: at,
                    end: at,
                    half_extents: vec3(s(15.0), s(15.0), (maxs_z + s(24.0)) * s(0.5)),
                    center_offset: vec3(s(0.0), s(0.0), (maxs_z - s(24.0)) * s(0.5)),
                })
                .start_solid
        };
        assert!(probe(s(16.0)) && probe(s(32.0)), "z=20 admits neither hull");
    }

    #[test]
    fn a_platform_ends_where_it_says_and_the_floor_is_below_it() {
        let world = drop_from(s(256.0));
        assert_eq!(
            surface_under(&world, PLATFORM_EDGE_X - s(64.0), s(1024.0)),
            Some(s(256.0))
        );
        assert_eq!(
            surface_under(&world, PLATFORM_EDGE_X + s(64.0), s(1024.0)),
            Some(FLOOR_TOP)
        );
    }

    /// Building a world twice must produce the same bits, or two seats
    /// measuring "the same" world are measuring two of them.
    #[test]
    fn every_world_is_bit_identical_when_rebuilt() {
        assert_eq!(floor(), floor());
        assert_eq!(ramp(s(33.7)), ramp(s(33.7)));
        assert_eq!(step(s(18.0)), step(s(18.0)));
        assert_eq!(ledge(s(64.0)), ledge(s(64.0)));
        assert_eq!(corner(), corner());
        assert_eq!(ceiling_at(s(20.0)), ceiling_at(s(20.0)));
        assert_eq!(drop_from(s(256.0)), drop_from(s(256.0)));
    }
}
