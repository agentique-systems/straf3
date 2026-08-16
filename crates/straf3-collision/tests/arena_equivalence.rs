//! The hardcoded arena, rebuilt out of [`Hull`]s, answering the same questions.
//!
//! # Why this test exists
//!
//! `straf3-render`'s `arena.rs` carries a working transcription of Quake 3's
//! `CM_TraceThroughBrush` and nineteen movement tests in `straf3-sim` are
//! written against the behaviour it produces. This crate generalises that
//! transcription — arbitrary plane counts, bevels, a broadphase reject, hulls
//! from a file rather than from a `static`. Every one of those is a chance to
//! change an answer the movement tests depend on.
//!
//! So the arena is declared here a second time, from the same numbers, and
//! `arena.rs`'s own collision tests are ported onto it verbatim. If this file is
//! green, retiring the hardcoded arena is a substitution rather than a rewrite,
//! and the nineteen tests should not notice.
//!
//! It is a *second declaration*, which `arena.rs` opens by arguing against — but
//! the alternative is worse in both directions: `straf3-collision` cannot depend
//! on `straf3-render` (it is above the line, and the seam check would fail the
//! build), and the copy is meant to be temporary. When the integrator replaces
//! `impl World for Arena` with a compiled map, this file's job is done and it
//! should go with it.
//!
//! # What it establishes
//!
//! 1. Compiling each arena solid through [`compile_hull`] produces *exactly* the
//!    plane set `arena.rs` writes by hand — no bevel is added, nothing is
//!    reordered. The generalisation is a no-op on this geometry, which is why
//!    the movement tests can be expected to hold.
//! 2. The ported traces agree, to the bit where `arena.rs` asserted bits.

use straf3_collision::{Hull, HullWorld, Plane, axial_planes, compile_hull};
use straf3_sim::PhysicsProfile;
use straf3_sim::num::{Scalar, Vec3, s, to_bits, vec3};
use straf3_sim::world::{SurfaceFlags, Sweep, World};

// ── the arena, from arena.rs's own constants ───────────────────────────────

const FLOOR_HALF: Scalar = s(1536.0);
const FLOOR_TOP: Scalar = s(0.0);
const WALL_THICK: Scalar = s(64.0);
const WALL_TOP: Scalar = s(512.0);
const OUTER: Scalar = FLOOR_HALF + WALL_THICK;

const SPAWN: Vec3 = vec3(s(0.0), s(-768.0), s(24.125));
const SPAWN_YAW: Scalar = s(90.0);

/// A solid box, named as `arena.rs` names it.
struct Box3 {
    mins: Vec3,
    maxs: Vec3,
    label: &'static str,
}

/// Which horizontal axis a ramp rises along.
#[derive(Clone, Copy)]
enum Axis {
    X,
    Y,
}

impl Axis {
    fn of(self, v: Vec3) -> Scalar {
        match self {
            Self::X => v.x,
            Self::Y => v.y,
        }
    }
}

/// A wedge, as `arena.rs` declares one.
struct Ramp {
    mins: Vec3,
    maxs: Vec3,
    z_at_mins: Scalar,
    z_at_maxs: Scalar,
    axis: Axis,
    label: &'static str,
}

impl Ramp {
    fn slope(&self) -> Scalar {
        (self.z_at_maxs - self.z_at_mins) / (self.axis.of(self.maxs) - self.axis.of(self.mins))
    }

    fn surface_z(&self, x: Scalar, y: Scalar) -> Scalar {
        let u = self.axis.of(vec3(x, y, s(0.0)));
        self.z_at_mins + self.slope() * (u - self.axis.of(self.mins))
    }

    /// `arena.rs`'s `Ramp::normal`, arithmetic included — the division is not
    /// interchangeable with a multiply by the reciprocal if the assertions
    /// below are to compare bits.
    fn normal(&self) -> Vec3 {
        let k = self.slope();
        let raw = match self.axis {
            Axis::X => vec3(-k, s(0.0), s(1.0)),
            Axis::Y => vec3(s(0.0), -k, s(1.0)),
        };
        raw / (s(1.0) + k * k).sqrt()
    }

    /// The seven planes `arena.rs` builds: six axial, plus the inclined top.
    fn planes(&self) -> Vec<Plane> {
        let mut planes = axial_planes(self.mins, self.maxs).to_vec();
        let normal = self.normal();
        let anchor = vec3(self.mins.x, self.mins.y, self.z_at_mins);
        planes.push(Plane::through(normal, anchor));
        planes
    }
}

fn boxes() -> Vec<Box3> {
    vec![
        Box3 {
            mins: vec3(-OUTER, -OUTER, s(-64.0)),
            maxs: vec3(OUTER, OUTER, FLOOR_TOP),
            label: "floor",
        },
        Box3 {
            mins: vec3(-OUTER, -OUTER, FLOOR_TOP),
            maxs: vec3(-FLOOR_HALF, OUTER, WALL_TOP),
            label: "wall-west",
        },
        Box3 {
            mins: vec3(FLOOR_HALF, -OUTER, FLOOR_TOP),
            maxs: vec3(OUTER, OUTER, WALL_TOP),
            label: "wall-east",
        },
        Box3 {
            mins: vec3(-FLOOR_HALF, -OUTER, FLOOR_TOP),
            maxs: vec3(FLOOR_HALF, -FLOOR_HALF, WALL_TOP),
            label: "wall-south",
        },
        Box3 {
            mins: vec3(-FLOOR_HALF, FLOOR_HALF, FLOOR_TOP),
            maxs: vec3(FLOOR_HALF, OUTER, WALL_TOP),
            label: "wall-north",
        },
        Box3 {
            mins: vec3(s(-384.0), s(-320.0), FLOOR_TOP),
            maxs: vec3(s(-128.0), s(320.0), s(128.0)),
            label: "platform",
        },
        Box3 {
            mins: vec3(s(-448.0), s(-704.0), FLOOR_TOP),
            maxs: vec3(s(-320.0), s(-576.0), s(16.0)),
            label: "step",
        },
        Box3 {
            mins: vec3(s(-256.0), s(-704.0), FLOOR_TOP),
            maxs: vec3(s(-128.0), s(-576.0), s(64.0)),
            label: "crate",
        },
        Box3 {
            mins: vec3(s(128.0), s(-256.0), FLOOR_TOP),
            maxs: vec3(s(448.0), s(0.0), s(192.0)),
            label: "ledge",
        },
        Box3 {
            mins: vec3(s(576.0), s(-192.0), FLOOR_TOP),
            maxs: vec3(s(704.0), s(-64.0), s(232.0)),
            label: "pillar",
        },
    ]
}

fn ramp_gentle() -> Ramp {
    Ramp {
        mins: vec3(s(-640.0), s(-320.0), FLOOR_TOP),
        maxs: vec3(s(-384.0), s(320.0), s(128.0)),
        z_at_mins: s(0.0),
        z_at_maxs: s(128.0),
        axis: Axis::X,
        label: "ramp-gentle",
    }
}

fn ramps() -> Vec<Ramp> {
    vec![
        ramp_gentle(),
        Ramp {
            mins: vec3(s(128.0), s(-448.0), FLOOR_TOP),
            maxs: vec3(s(448.0), s(-256.0), s(192.0)),
            z_at_mins: s(0.0),
            z_at_maxs: s(192.0),
            axis: Axis::Y,
            label: "ramp-45",
        },
        Ramp {
            mins: vec3(s(256.0), s(256.0), FLOOR_TOP),
            maxs: vec3(s(384.0), s(576.0), s(192.0)),
            z_at_mins: s(0.0),
            z_at_maxs: s(192.0),
            axis: Axis::X,
            label: "ramp-steep",
        },
    ]
}

/// The arena as a `World`: boxes first, then ramps, exactly as `arena.rs`'s
/// trace loop visits them. The order is the tie-break, so it is part of the
/// reproduction.
fn arena() -> HullWorld {
    let mut hulls: Vec<Hull> = boxes()
        .iter()
        .map(|b| Hull::from_aabb(b.mins, b.maxs, SurfaceFlags::NONE))
        .collect();
    for r in ramps() {
        hulls.push(Hull::from_planes(&r.planes(), SurfaceFlags::NONE).expect(r.label));
    }
    HullWorld::new(hulls)
}

/// The standing hull, from the physics profile rather than restated — a sweep
/// the simulation would not make proves nothing.
fn sweep(from: Vec3, to: Vec3) -> Sweep {
    let hull = PhysicsProfile::vq3().hull(false);
    Sweep {
        start: from,
        end: to,
        half_extents: hull.half_extents,
        center_offset: hull.center_offset,
    }
}

// ── 1. the generalisation is a no-op on this geometry ──────────────────────

#[test]
fn compiling_an_arena_solid_adds_no_plane_and_moves_none() {
    for b in boxes() {
        let hand_written = axial_planes(b.mins, b.maxs).to_vec();
        let compiled = compile_hull(&hand_written, SurfaceFlags::NONE).expect(b.label);
        assert_eq!(
            compiled.hull.planes, hand_written,
            "{}: compiling changed the plane set",
            b.label
        );
        assert_eq!((compiled.hull.mins, compiled.hull.maxs), (b.mins, b.maxs));
    }

    for r in ramps() {
        let hand_written = r.planes();
        let compiled = compile_hull(&hand_written, SurfaceFlags::NONE).expect(r.label);
        assert_eq!(
            compiled.hull.planes, hand_written,
            "{}: compiling changed the plane set — every edge of a wedge runs \
             along an axis, so no edge bevel should survive",
            r.label
        );
        // The bounding box must come back exactly, or the broadphase reject
        // would trim geometry the hand-written tracer never trimmed.
        assert_eq!(
            (compiled.hull.mins, compiled.hull.maxs),
            (r.mins, r.maxs),
            "{}",
            r.label
        );
    }
}

// ── 2. arena.rs's own collision tests, ported ──────────────────────────────

#[test]
fn falling_onto_the_floor_stops_at_the_floor() {
    let t = arena().trace(&sweep(
        vec3(s(0.0), s(-768.0), s(124.0)),
        vec3(s(0.0), s(-768.0), s(-76.0)),
    ));
    assert!(t.hit());
    assert_eq!(t.normal, vec3(s(0.0), s(0.0), s(1.0)));
    assert_eq!(t.fraction, s(0.5));
    assert!(!t.start_solid);
}

#[test]
fn open_floor_at_head_height_is_clear() {
    let t = arena().trace(&sweep(
        vec3(s(0.0), s(-768.0), s(24.125)),
        vec3(s(0.0), s(-1200.0), s(24.125)),
    ));
    assert!(!t.hit(), "nothing is between spawn and the south wall");
}

const SPAWN_SIGHTLINE: Scalar = s(1024.0);

#[test]
fn the_lane_the_spawn_faces_is_open() {
    let yaw = SPAWN_YAW.to_radians();
    let ahead = vec3(yaw.cos(), yaw.sin(), s(0.0));
    let t = arena().trace(&sweep(SPAWN, SPAWN + ahead * SPAWN_SIGHTLINE));
    assert!(
        !t.hit(),
        "the player holds forward from the spawn and stops {} units in, \
         against a surface with normal {:?}",
        t.fraction * SPAWN_SIGHTLINE,
        t.normal,
    );
}

#[test]
fn the_spawn_lane_is_open_at_every_height_the_hull_occupies() {
    let world = arena();
    for feet in [s(0.0), s(16.0), s(45.0)] {
        let from = SPAWN + vec3(s(0.0), s(0.0), feet);
        let yaw = SPAWN_YAW.to_radians();
        let ahead = vec3(yaw.cos(), yaw.sin(), s(0.0));
        let t = world.trace(&sweep(from, from + ahead * SPAWN_SIGHTLINE));
        assert!(
            !t.hit(),
            "with the feet {feet} above the floor the spawn lane is blocked \
             after {} units",
            t.fraction * SPAWN_SIGHTLINE,
        );
    }
}

#[test]
fn the_perimeter_wall_stops_you_leaving() {
    let t = arena().trace(&sweep(
        vec3(s(0.0), s(-768.0), s(24.125)),
        vec3(s(0.0), s(-2000.0), s(24.125)),
    ));
    assert!(t.hit(), "the arena must be closed");
    assert_eq!(t.normal, vec3(s(0.0), s(1.0), s(0.0)));
    let travelled = t.fraction * s(1232.0);
    assert!(
        (travelled - s(753.0)).abs() < s(0.01),
        "stopped after {travelled} units, expected 753"
    );
}

#[test]
fn spawn_is_solid_free_and_standing_on_the_floor() {
    let world = arena();
    let t = world.trace(&sweep(SPAWN, SPAWN));
    assert!(!t.start_solid, "spawn is inside a solid");
    assert!(!t.all_solid);

    let probe = PhysicsProfile::vq3().ground_trace_probe;
    let g = world.trace(&sweep(SPAWN, SPAWN - vec3(s(0.0), s(0.0), probe)));
    assert!(g.hit(), "spawn is not standing on anything");
    assert_eq!(g.normal, vec3(s(0.0), s(0.0), s(1.0)));
}

#[test]
fn landing_on_a_ramp_reports_the_ramp_normal_not_the_floor() {
    let t = arena().trace(&sweep(
        vec3(s(-512.0), s(0.0), s(200.0)),
        vec3(s(-512.0), s(0.0), s(0.0)),
    ));
    assert!(t.hit());
    assert_eq!(
        to_bits(t.normal.z),
        to_bits(ramp_gentle().normal().z),
        "hit {:?}, expected the ramp surface",
        t.normal
    );
}

#[test]
fn a_hull_dropped_on_a_ramp_rests_on_its_uphill_edge() {
    let world = arena();
    let gentle = ramp_gentle();
    for x in [s(-620.0), s(-512.0), s(-400.0)] {
        let start = vec3(x, s(0.0), s(400.0));
        let end = vec3(x, s(0.0), s(-100.0));
        let t = world.trace(&sweep(start, end));
        assert!(t.hit(), "nothing under x={x}");
        let feet = (start + (end - start) * t.fraction).z - s(24.0);
        let want = gentle.surface_z(x + s(15.0), s(0.0));
        assert!(
            (feet - want).abs() < s(0.01),
            "at x={x} the hull stopped with feet at {feet}, uphill edge is at {want}"
        );
        assert!(feet >= gentle.surface_z(x, s(0.0)));
    }
}

#[test]
fn the_steep_ramp_is_solid_from_the_side_too() {
    let t = arena().trace(&sweep(
        vec3(s(700.0), s(400.0), s(120.0)),
        vec3(s(300.0), s(400.0), s(120.0)),
    ));
    assert!(t.hit());
    assert_eq!(t.normal, vec3(s(1.0), s(0.0), s(0.0)));
}

#[test]
fn starting_inside_a_solid_reports_it() {
    let inside = vec3(s(-192.0), s(-640.0), s(32.0));
    let t = arena().trace(&sweep(inside, inside));
    assert!(t.start_solid);
    assert!(t.all_solid);
    assert_eq!(t.fraction, s(0.0));
}

#[test]
fn the_step_is_low_enough_to_walk_onto_and_the_crate_is_not() {
    let profile = PhysicsProfile::vq3();
    let start = vec3(s(-64.0), s(-640.0), s(24.125));
    let t = arena().trace(&sweep(start, vec3(s(-500.0), s(-640.0), s(24.125))));
    assert!(!t.start_solid, "the sweep must begin on open floor");
    assert!(t.hit(), "the crate is in the way before the step is");
    let travelled = t.fraction * s(436.0);
    assert!(
        (travelled - s(49.0)).abs() < s(0.01),
        "stopped after {travelled} units, expected 49"
    );
    assert!(s(64.0) > profile.step_height);
}

#[test]
fn traces_are_bit_identical_when_repeated() {
    let world = arena();
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

/// A dense sweep over the whole arena, in one checksum.
///
/// The ported tests above each check a place someone thought to look. This one
/// checks everywhere: four thousand sweeps on a fixed lattice, folded bit by bit
/// into a single number. It is the shape of check `cargo xtask determinism`
/// wants for cross-target verification, and pinning the value here means a
/// change to the tracer that misses every named test still fails this one.
///
/// The pinned value was produced on `x86_64-unknown-linux-gnu`. Nothing in the
/// path that computes it is anything but an IEEE add, subtract, multiply,
/// divide, compare or square root, so it is *expected* to hold on musl, Windows
/// and wasm — and if it does not, that is the C2 cross-target check finding
/// something real rather than this test being wrong.
#[test]
fn a_lattice_of_sweeps_folds_to_a_stable_digest() {
    let world = arena();
    let mut digest: u64 = 0xcbf2_9ce4_8422_2325;
    let mut fold = |bits: u32| {
        digest ^= u64::from(bits);
        digest = digest.wrapping_mul(0x100_0000_01b3);
    };

    let mut n = 0u32;
    for i in 0..10 {
        for j in 0..10 {
            for k in 0..10 {
                let from = vec3(
                    s(-700.0) + s(i as f32) * s(150.0),
                    s(-700.0) + s(j as f32) * s(150.0),
                    s(8.0) + s(k as f32) * s(24.0),
                );
                // Four directions per point, so faces are met from both sides.
                for d in [
                    vec3(s(220.0), s(90.0), s(-160.0)),
                    vec3(s(-90.0), s(220.0), s(-40.0)),
                    vec3(s(0.0), s(0.0), s(-300.0)),
                    vec3(s(160.0), s(-160.0), s(60.0)),
                ] {
                    let t = world.trace(&sweep(from, from + d));
                    fold(to_bits(t.fraction));
                    fold(to_bits(t.normal.x));
                    fold(to_bits(t.normal.y));
                    fold(to_bits(t.normal.z));
                    fold(u32::from(t.start_solid) | (u32::from(t.all_solid) << 1));
                    n += 1;
                }
            }
        }
    }

    assert_eq!(n, 4000);
    assert_eq!(
        digest, 0xf059_37d6_3c75_c02f,
        "the arena's collision answers changed"
    );
}
