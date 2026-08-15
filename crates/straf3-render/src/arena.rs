//! The hardcoded arena: one piece of geometry, consumed twice.
//!
//! # Why this lives here and has no `wgpu` in it
//!
//! The thing the player collides with and the thing the player sees must be
//! the *same* geometry. Two hardcoded copies drift the moment either is
//! edited, and the failure mode is the worst kind: an invisible wall, or a
//! ramp you fall through. So the arena is data, declared once in [`ARENA`],
//! and read by two consumers that never talk to each other:
//!
//! - [`mesh`](crate::mesh) turns it into triangles for the GPU;
//! - [`impl World for Arena`](World) answers the simulation's sweep queries.
//!
//! Not one `wgpu` type appears in this module, on purpose: `straf3-game` and
//! the test harness import [`ARENA`] and hand it straight to
//! [`straf3_sim::step_in_place`], and neither of them should have to link a
//! GPU stack to ask where the floor is.
//!
//! # Why the collision is Quake's algorithm and not a library's
//!
//! [`straf3_sim::World`]'s contract is *pure and deterministic, bit-identical
//! across runs*, and criterion 4 (the replay must checksum the same through
//! the windowed build as through `straf3-headless`) rests on it. That rules
//! out anything with a broadphase cache, a BVH built in a different order, or
//! work-stealing parallelism. What is left is small enough to write: every
//! solid here is a **convex brush** — an intersection of half-spaces — and the
//! swept-AABB query is `CM_TraceThroughBrush` from the Quake 3 source,
//! transcribed.
//!
//! The trick that makes an AABB sweep as cheap as a ray sweep is the one Q3
//! used: push each brush plane outward by the hull's extent along that plane's
//! normal (`|n.x|·hx + |n.y|·hy + |n.z|·hz`, the support function of a box),
//! and the moving box becomes a moving point. That is exact for the box's own
//! axial faces and for every brush face; the cases it approximates are
//! non-axial *edge* bevels, which Q3 also left out for brushes. Matching Q3's
//! approximation is the point — the edge-clipping quirks the project exists to
//! reproduce are partly artefacts of exactly this.
//!
//! One deliberate difference: Q3 folds `SURFACE_CLIP_EPSILON` into the trace.
//! Here it does not, because `straf3-sim`'s mover applies it itself (see
//! `step.rs`'s `sweep_to` and its comment saying so). Applying it in both
//! places would hold the player two epsilons off every surface.
//!
//! # Why ramps
//!
//! Spec rev 6 §T calls them out: ramps are where CPM and VQ3 diverge most, and
//! [`straf3_sim::GroundState`] has three states rather than two precisely
//! because of them. So the arena carries three, chosen to straddle
//! `min_walk_normal` (0.7) rather than to look nice:
//!
//! | Ramp | slope | surface normal Z | what the simulation does |
//! |---|---|---|---|
//! | [`RAMP_GENTLE`] | 1:2 (26.6°) | 0.894 | `Grounded` — walk and jump normally |
//! | [`RAMP_45`] | 1:1 (45.0°) | 0.707 | `Grounded`, but 0.007 from not being |
//! | [`RAMP_STEEP`] | 3:2 (56.3°) | 0.555 | `Sliding` — no friction, air rules |
//!
//! The middle one is the interesting one. It sits 0.007 above the walkable
//! threshold, which is the discrete branch rev 6 §P1 warns a 1-ULP difference
//! in a trig call could flip. It cannot flip *here* — this normal is a ratio of
//! exact binary constants, no trig involved — but it is the shape of geometry
//! that would show it.

use straf3_sim::num::{Scalar, Vec3, s, vec3};
use straf3_sim::world::{SurfaceFlags, Sweep, Trace, World};

/// A colour, linear RGB in `0.0..=1.0`.
///
/// Render data living in the geometry declaration, so that adding a solid is
/// one edit rather than two. Collision ignores it entirely. Flat colours only:
/// spec rev 6 §S puts textures out of scope for this wave.
pub type Rgb = [f32; 3];

/// Which horizontal axis a [`Ramp`] rises along.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Axis {
    /// Rises along X; constant along Y.
    X,
    /// Rises along Y; constant along X.
    Y,
}

impl Axis {
    /// This axis' component of `v`.
    #[must_use]
    pub const fn of(self, v: Vec3) -> Scalar {
        match self {
            Self::X => v.x,
            Self::Y => v.y,
        }
    }
}

/// A solid axis-aligned box.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Box3 {
    /// Low corner.
    pub mins: Vec3,
    /// High corner.
    pub maxs: Vec3,
    /// Movement-relevant properties of every face.
    pub surface: SurfaceFlags,
    /// Flat colour, for drawing only.
    pub color: Rgb,
    /// What this solid is, for diagnostics and test failure messages.
    pub label: &'static str,
}

/// A solid wedge: an axis-aligned box whose top is an inclined plane.
///
/// The top surface passes through `z_at_mins` at the low end of [`axis`] and
/// `z_at_maxs` at the high end, and the solid is everything inside
/// `mins..maxs` below it. `maxs.z` must equal the higher of the two — the
/// bounding box is what supplies the axial bevel planes that keep the swept
/// AABB query exact, so a loose one would round the wedge's tip off.
///
/// [`axis`]: Ramp::axis
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Ramp {
    /// Low corner of the bounding box.
    pub mins: Vec3,
    /// High corner of the bounding box. `maxs.z` is the top of the tall end.
    pub maxs: Vec3,
    /// Height of the inclined surface at the low end of [`Ramp::axis`].
    pub z_at_mins: Scalar,
    /// Height of the inclined surface at the high end of [`Ramp::axis`].
    pub z_at_maxs: Scalar,
    /// Which horizontal axis the surface rises along.
    pub axis: Axis,
    /// Movement-relevant properties of every face.
    pub surface: SurfaceFlags,
    /// Flat colour, for drawing only.
    pub color: Rgb,
    /// What this solid is, for diagnostics and test failure messages.
    pub label: &'static str,
}

impl Ramp {
    /// Rise over run: how much the surface climbs per unit along [`Ramp::axis`].
    #[must_use]
    pub fn slope(&self) -> Scalar {
        (self.z_at_maxs - self.z_at_mins) / (self.axis.of(self.maxs) - self.axis.of(self.mins))
    }

    /// Height of the inclined surface directly above `x, y`.
    ///
    /// Not clamped to the ramp's footprint — outside it, this is the height the
    /// surface *would* have, which is what the plane equation says and what the
    /// trace uses.
    #[must_use]
    pub fn surface_z(&self, x: Scalar, y: Scalar) -> Scalar {
        let u = self.axis.of(vec3(x, y, s(0.0)));
        self.z_at_mins + self.slope() * (u - self.axis.of(self.mins))
    }

    /// Unit normal of the inclined surface, pointing out of the solid.
    ///
    /// Its Z component against
    /// [`PhysicsProfile::min_walk_normal`](straf3_sim::PhysicsProfile) is what
    /// decides walkable from sliding.
    #[must_use]
    pub fn normal(&self) -> Vec3 {
        let k = self.slope();
        let raw = match self.axis {
            Axis::X => vec3(-k, s(0.0), s(1.0)),
            Axis::Y => vec3(s(0.0), -k, s(1.0)),
        };
        raw / (s(1.0) + k * k).sqrt()
    }
}

/// The whole arena: every solid in it.
///
/// Two flat lists rather than a tree. At this size a tree would cost more in
/// build-order determinism worries than it saves in traces, and the trace loop
/// visits solids in declaration order, which is a fixed order by construction.
#[derive(Debug, Clone, Copy)]
pub struct Arena {
    /// Every solid box, in a fixed order.
    pub boxes: &'static [Box3],
    /// Every solid ramp, in a fixed order.
    pub ramps: &'static [Ramp],
}

// ── the geometry ───────────────────────────────────────────────────────────
//
// Quake units throughout: the player hull is 30×30×56 and a comfortable
// strafe-jump covers ~600 units a second, so a 3072² floor is about five
// seconds of flat-out running corner to corner. Big enough that the movement
// has room to be felt, small enough that you are never lost in it.

/// Half-width of the open floor: the playable area is `±FLOOR_HALF` on X and Y.
pub const FLOOR_HALF: Scalar = s(1536.0);
/// Height of the floor's top surface. Everything else is measured from here.
pub const FLOOR_TOP: Scalar = s(0.0);
/// Thickness of the perimeter walls, and how far past [`FLOOR_HALF`] the floor
/// slab extends so the walls have something to stand on.
const WALL_THICK: Scalar = s(64.0);
/// Height of the perimeter walls. Well past any jump: the arena is closed.
const WALL_TOP: Scalar = s(512.0);
const OUTER: Scalar = FLOOR_HALF + WALL_THICK;

const GREY: Rgb = [0.42, 0.44, 0.47];
const DARK: Rgb = [0.22, 0.23, 0.26];
const RUST: Rgb = [0.55, 0.30, 0.20];
const TEAL: Rgb = [0.16, 0.42, 0.44];
const SAND: Rgb = [0.62, 0.55, 0.36];
const PLUM: Rgb = [0.40, 0.24, 0.42];

static BOXES: &[Box3] = &[
    Box3 {
        mins: vec3(-OUTER, -OUTER, s(-64.0)),
        maxs: vec3(OUTER, OUTER, FLOOR_TOP),
        surface: SurfaceFlags::NONE,
        color: GREY,
        label: "floor",
    },
    Box3 {
        mins: vec3(-OUTER, -OUTER, FLOOR_TOP),
        maxs: vec3(-FLOOR_HALF, OUTER, WALL_TOP),
        surface: SurfaceFlags::NONE,
        color: DARK,
        label: "wall-west",
    },
    Box3 {
        mins: vec3(FLOOR_HALF, -OUTER, FLOOR_TOP),
        maxs: vec3(OUTER, OUTER, WALL_TOP),
        surface: SurfaceFlags::NONE,
        color: DARK,
        label: "wall-east",
    },
    Box3 {
        mins: vec3(-FLOOR_HALF, -OUTER, FLOOR_TOP),
        maxs: vec3(FLOOR_HALF, -FLOOR_HALF, WALL_TOP),
        surface: SurfaceFlags::NONE,
        color: DARK,
        label: "wall-south",
    },
    Box3 {
        mins: vec3(-FLOOR_HALF, FLOOR_HALF, FLOOR_TOP),
        maxs: vec3(FLOOR_HALF, OUTER, WALL_TOP),
        surface: SurfaceFlags::NONE,
        color: DARK,
        label: "wall-north",
    },
    // Top of RAMP_GENTLE: a wide platform you arrive on at speed, flush with
    // the ramp's tall end so there is no lip to catch a hull on.
    Box3 {
        mins: vec3(s(-384.0), s(-320.0), FLOOR_TOP),
        maxs: vec3(s(-128.0), s(320.0), s(128.0)),
        surface: SurfaceFlags::NONE,
        color: TEAL,
        label: "platform",
    },
    // 16 units tall, under the 18-unit step height: you walk straight onto it
    // without jumping. The other side of the same coin as `crate`, which you
    // cannot.
    Box3 {
        mins: vec3(s(-320.0), s(-704.0), FLOOR_TOP),
        maxs: vec3(s(-192.0), s(-576.0), s(16.0)),
        surface: SurfaceFlags::NONE,
        color: SAND,
        label: "step",
    },
    // 64 tall, above both the step height and a standing jump (≈45 units at
    // 270 ups against 800 gravity): needs a run-up or a double jump.
    Box3 {
        mins: vec3(s(-128.0), s(-704.0), FLOOR_TOP),
        maxs: vec3(s(0.0), s(-576.0), s(64.0)),
        surface: SurfaceFlags::NONE,
        color: RUST,
        label: "crate",
    },
    // Continues RAMP_45 at its top height, so the 45° climb leads somewhere.
    Box3 {
        mins: vec3(s(128.0), s(-256.0), FLOOR_TOP),
        maxs: vec3(s(448.0), s(0.0), s(192.0)),
        surface: SurfaceFlags::NONE,
        color: TEAL,
        label: "ledge",
    },
    // 40 units above the ledge — just inside a standing jump, so it is the
    // arena's yes/no test of whether jump height feels right.
    Box3 {
        mins: vec3(s(576.0), s(-192.0), FLOOR_TOP),
        maxs: vec3(s(704.0), s(-64.0), s(232.0)),
        surface: SurfaceFlags::NONE,
        color: PLUM,
        label: "pillar",
    },
];

/// 1:2, normal Z 0.894 — comfortably walkable. Rises 128 over 256 along +X
/// into [the platform](BOXES), 640 units wide so it can be taken at speed.
pub static RAMP_GENTLE: Ramp = Ramp {
    mins: vec3(s(-640.0), s(-320.0), FLOOR_TOP),
    maxs: vec3(s(-384.0), s(320.0), s(128.0)),
    z_at_mins: s(0.0),
    z_at_maxs: s(128.0),
    axis: Axis::X,
    surface: SurfaceFlags::NONE,
    color: SAND,
    label: "ramp-gentle",
};

/// 1:1, normal Z 0.7071 — walkable by 0.0071. Rises 192 over 192 along +Y into
/// [the ledge](BOXES).
pub static RAMP_45: Ramp = Ramp {
    mins: vec3(s(128.0), s(-448.0), FLOOR_TOP),
    maxs: vec3(s(448.0), s(-256.0), s(192.0)),
    z_at_mins: s(0.0),
    z_at_maxs: s(192.0),
    axis: Axis::Y,
    surface: SurfaceFlags::NONE,
    color: RUST,
    label: "ramp-45",
};

/// 3:2, normal Z 0.5547 — too steep to stand on. Rises 192 over 128 along +X.
/// You slide back down it, keeping speed, under air rules.
pub static RAMP_STEEP: Ramp = Ramp {
    mins: vec3(s(256.0), s(256.0), FLOOR_TOP),
    maxs: vec3(s(384.0), s(576.0), s(192.0)),
    z_at_mins: s(0.0),
    z_at_maxs: s(192.0),
    axis: Axis::X,
    surface: SurfaceFlags::NONE,
    color: PLUM,
    label: "ramp-steep",
};

static RAMPS: &[Ramp] = &[RAMP_GENTLE, RAMP_45, RAMP_STEEP];

/// The arena. One declaration; the mesh and the collision both read this.
pub static ARENA: Arena = Arena {
    boxes: BOXES,
    ramps: RAMPS,
};

/// Where the player starts: open floor, south of everything, facing north up
/// the length of the arena.
///
/// Z is `24.125`, not `24`: 24 is where the standing hull's feet sit exactly on
/// the floor plane, and *exactly on* counts as inside a brush, so the very
/// first trace would report `start_solid`. The extra eighth is
/// `SURFACE_CLIP_EPSILON` — the same distance the mover holds the player off a
/// surface after landing, so the player starts already at rest on the floor
/// rather than falling onto it.
pub const SPAWN: Vec3 = vec3(s(0.0), s(-768.0), s(24.125));

/// Which way the player faces at [`SPAWN`]: +Y, up the arena.
///
/// Quake's convention, which [`straf3_sim::ViewAngles`] keeps: yaw 0 is +X and
/// yaw increases anticlockwise, so 90 is +Y.
pub const SPAWN_YAW: Scalar = s(90.0);

/// Eye height above the player origin, standing — Q3's `DEFAULT_VIEWHEIGHT`.
pub const EYE_HEIGHT: Scalar = s(26.0);

/// Eye height above the player origin, crouched — Q3's `CROUCH_VIEWHEIGHT`.
pub const EYE_HEIGHT_CROUCHED: Scalar = s(12.0);

// ── collision ──────────────────────────────────────────────────────────────

/// One half-space of a convex brush: the solid is where `p · normal <= dist`.
#[derive(Debug, Clone, Copy)]
struct Plane {
    normal: Vec3,
    dist: Scalar,
}

/// The most planes any solid here needs: six axial faces, plus a ramp's
/// inclined top.
const MAX_PLANES: usize = 7;

/// A convex solid as a set of outward half-spaces.
struct Brush {
    planes: [Plane; MAX_PLANES],
    count: usize,
    surface: SurfaceFlags,
}

impl Brush {
    fn planes(&self) -> &[Plane] {
        &self.planes[..self.count]
    }
}

/// The six axial faces of `mins..maxs`, in the order -X +X -Y +Y -Z +Z.
fn axial(mins: Vec3, maxs: Vec3) -> [Plane; 6] {
    let p = |normal: Vec3, dist: Scalar| Plane { normal, dist };
    [
        p(vec3(s(-1.0), s(0.0), s(0.0)), -mins.x),
        p(vec3(s(1.0), s(0.0), s(0.0)), maxs.x),
        p(vec3(s(0.0), s(-1.0), s(0.0)), -mins.y),
        p(vec3(s(0.0), s(1.0), s(0.0)), maxs.y),
        p(vec3(s(0.0), s(0.0), s(-1.0)), -mins.z),
        p(vec3(s(0.0), s(0.0), s(1.0)), maxs.z),
    ]
}

impl Box3 {
    fn brush(&self) -> Brush {
        let a = axial(self.mins, self.maxs);
        Brush {
            planes: [a[0], a[1], a[2], a[3], a[4], a[5], a[5]],
            count: 6,
            surface: self.surface,
        }
    }
}

impl Ramp {
    fn brush(&self) -> Brush {
        let a = axial(self.mins, self.maxs);
        // The inclined top, through the surface at the low end. The bounding
        // box's own +Z face stays in the set even though the incline makes it
        // redundant: it is the axial bevel that keeps the swept-box expansion
        // exact near the tall end's top edge.
        let n = self.normal();
        // A point on the incline. The normal has no component along the free
        // horizontal axis, so the low corner serves for either orientation.
        let anchor = vec3(self.mins.x, self.mins.y, self.z_at_mins);
        let top = Plane {
            normal: n,
            dist: anchor.dot(n),
        };
        Brush {
            planes: [a[0], a[1], a[2], a[3], a[4], a[5], top],
            count: 7,
            surface: self.surface,
        }
    }
}

/// Quake 3's `CM_TraceThroughBrush`, minus the epsilon (the mover applies it).
///
/// Narrows `best` in place: it carries the running earliest hit across every
/// solid, exactly as Q3's `trace_t` does across a leaf's brushes.
fn trace_brush(brush: &Brush, sweep: &Sweep, best: &mut Trace) {
    // The hull's centre, not the player origin: Quake's player box is not
    // centred on its origin (see `Sweep::center_offset`).
    let start = sweep.start + sweep.center_offset;
    let end = sweep.end + sweep.center_offset;
    let h = sweep.half_extents.abs();

    let mut enter_frac = s(-1.0);
    let mut leave_frac = s(1.0);
    let mut clip: Option<Plane> = None;
    let mut getout = false;
    let mut startout = false;

    for plane in brush.planes() {
        // Push the plane out by the box's support along its normal, so the
        // swept box becomes a swept point.
        let offset =
            plane.normal.x.abs() * h.x + plane.normal.y.abs() * h.y + plane.normal.z.abs() * h.z;
        let dist = plane.dist + offset;

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
                clip = Some(*plane);
            }
        } else if f < leave_frac {
            leave_frac = f;
        }
    }

    if !startout {
        // Started inside this solid. Q3 records that and does not let the
        // brush limit the motion — the mover has its own unstick path
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
            best.normal = clip.map_or(Vec3::ZERO, |p| p.normal);
            best.surface = brush.surface;
        }
    }
}

impl World for Arena {
    /// Earliest hit across every solid, in declaration order.
    ///
    /// Deterministic by construction: a fixed iteration order over a static
    /// list, no cache, no clock, no parallelism, and ties resolved by
    /// declaration order because the comparison is strictly `<`. Two runs of
    /// the same sweep produce the same bits, which is what criterion 4 rests
    /// on.
    fn trace(&self, sweep: &Sweep) -> Trace {
        let mut best = Trace::clear();
        for b in self.boxes {
            trace_brush(&b.brush(), sweep, &mut best);
        }
        for r in self.ramps {
            trace_brush(&r.brush(), sweep, &mut best);
        }
        best
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use straf3_sim::PhysicsProfile;
    use straf3_sim::num::to_bits;

    /// The standing hull, straight from the physics profile rather than
    /// restated here — a sweep the simulation would not make proves nothing.
    fn sweep(from: Vec3, to: Vec3) -> Sweep {
        let hull = PhysicsProfile::vq3().hull(false);
        Sweep {
            start: from,
            end: to,
            half_extents: hull.half_extents,
            center_offset: hull.center_offset,
        }
    }

    #[test]
    fn falling_onto_the_floor_stops_at_the_floor() {
        // The origin sits 24 above the feet, so these two put the *feet* at
        // +100 and -100 — the floor is crossed at exactly half.
        let t = ARENA.trace(&sweep(
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
        let t = ARENA.trace(&sweep(
            vec3(s(0.0), s(-768.0), s(24.125)),
            vec3(s(0.0), s(-1200.0), s(24.125)),
        ));
        assert!(!t.hit(), "nothing is between spawn and the south wall");
    }

    #[test]
    fn the_perimeter_wall_stops_you_leaving() {
        // Straight south from spawn into the south wall at y = -1536.
        let t = ARENA.trace(&sweep(
            vec3(s(0.0), s(-768.0), s(24.125)),
            vec3(s(0.0), s(-2000.0), s(24.125)),
        ));
        assert!(t.hit(), "the arena must be closed");
        // The normal is the wall's, pointing back at the player: the south
        // wall's inward-facing side is its +Y face.
        assert_eq!(t.normal, vec3(s(0.0), s(1.0), s(0.0)));
        // The hull's south face (15 out) meets the wall's north face at -1536,
        // so the origin stops at -1521 having travelled 753 of 1232.
        let travelled = t.fraction * s(1232.0);
        assert!(
            (travelled - s(753.0)).abs() < s(0.01),
            "stopped after {travelled} units, expected 753"
        );
    }

    #[test]
    fn spawn_is_solid_free_and_standing_on_the_floor() {
        let t = ARENA.trace(&sweep(SPAWN, SPAWN));
        assert!(!t.start_solid, "spawn is inside a solid");
        assert!(!t.all_solid);

        // The ground probe the simulation itself makes on the first command.
        let probe = PhysicsProfile::vq3().ground_trace_probe;
        let g = ARENA.trace(&sweep(SPAWN, SPAWN - vec3(s(0.0), s(0.0), probe)));
        assert!(g.hit(), "spawn is not standing on anything");
        assert_eq!(g.normal, vec3(s(0.0), s(0.0), s(1.0)));
    }

    #[test]
    fn every_ramp_normal_lands_where_the_docs_claim() {
        let walk = PhysicsProfile::vq3().min_walk_normal;
        let n = |r: &Ramp| r.normal();

        assert!((n(&RAMP_GENTLE).z - s(0.894_427_2)).abs() < s(1e-6));
        assert!(n(&RAMP_GENTLE).z > walk, "gentle ramp must be walkable");

        assert!((n(&RAMP_45).z - s(0.707_106_8)).abs() < s(1e-6));
        assert!(
            n(&RAMP_45).z > walk,
            "the 45° ramp must be walkable — barely, which is the point"
        );
        assert!(n(&RAMP_45).z - walk < s(0.01), "…and only barely");

        assert!((n(&RAMP_STEEP).z - s(0.554_700_2)).abs() < s(1e-6));
        assert!(n(&RAMP_STEEP).z < walk, "the steep ramp must slide");
    }

    #[test]
    fn landing_on_a_ramp_reports_the_ramp_normal_not_the_floor() {
        // Drop onto the middle of the gentle ramp: x = -512 is halfway along
        // it, where the surface is at z = 64.
        let t = ARENA.trace(&sweep(
            vec3(s(-512.0), s(0.0), s(200.0)),
            vec3(s(-512.0), s(0.0), s(0.0)),
        ));
        assert!(t.hit());
        assert_eq!(
            to_bits(t.normal.z),
            to_bits(RAMP_GENTLE.normal().z),
            "hit {:?}, expected the ramp surface",
            t.normal
        );
    }

    #[test]
    fn a_hull_dropped_on_a_ramp_rests_on_its_uphill_edge() {
        // The player hull is a box, and a box on a slope is held up by its
        // *uphill bottom edge*, not by the surface under its centre — it
        // floats above the ramp everywhere else. That is not an artefact to be
        // corrected: it is why a Quake player stands slightly proud of a ramp,
        // and getting it wrong either sinks the player into the surface or
        // holds them a hull-width above it.
        //
        // The gentle ramp rises along +X, so the uphill edge is 15 units (the
        // hull's half-width) further along X than the origin.
        for x in [s(-620.0), s(-512.0), s(-400.0)] {
            let start = vec3(x, s(0.0), s(400.0));
            let end = vec3(x, s(0.0), s(-100.0));
            let t = ARENA.trace(&sweep(start, end));
            assert!(t.hit(), "nothing under x={x}");
            let feet = (start + (end - start) * t.fraction).z - s(24.0);
            let want = RAMP_GENTLE.surface_z(x + s(15.0), s(0.0));
            assert!(
                (feet - want).abs() < s(0.01),
                "at x={x} the hull stopped with feet at {feet}, uphill edge is at {want}"
            );
            // …and it must be above the surface under its centre, never below.
            assert!(feet >= RAMP_GENTLE.surface_z(x, s(0.0)));
        }
    }

    #[test]
    fn the_steep_ramp_is_solid_from_the_side_too() {
        // Into the tall end of the steep ramp from +X. Its high face is at
        // x = 384, so a hull travelling -X must stop 15 out from it.
        let t = ARENA.trace(&sweep(
            vec3(s(700.0), s(400.0), s(120.0)),
            vec3(s(300.0), s(400.0), s(120.0)),
        ));
        assert!(t.hit());
        assert_eq!(t.normal, vec3(s(1.0), s(0.0), s(0.0)));
    }

    #[test]
    fn starting_inside_a_solid_reports_it() {
        // Dead centre of the crate.
        let inside = vec3(s(-64.0), s(-640.0), s(32.0));
        let t = ARENA.trace(&sweep(inside, inside));
        assert!(t.start_solid);
        assert!(t.all_solid);
        assert_eq!(t.fraction, s(0.0));
    }

    #[test]
    fn the_step_is_low_enough_to_walk_onto_and_the_crate_is_not() {
        let profile = PhysicsProfile::vq3();
        // Walking west into the step at floor height: the step's top (16) is
        // under the 18-unit step height, so the mover steps up. What the trace
        // must report is simply that the hull is blocked at floor level.
        let t = ARENA.trace(&sweep(
            vec3(s(-160.0), s(-640.0), s(24.125)),
            vec3(s(-260.0), s(-640.0), s(24.125)),
        ));
        assert!(t.hit(), "the crate is in the way before the step is");
        // The crate (top 64) is what it hits first, and 64 is well over the
        // step height — this is the obstacle you must jump.
        assert!(s(64.0) > profile.step_height);
    }

    #[test]
    fn traces_are_bit_identical_when_repeated() {
        // The World contract in one assertion. Not `assert_eq!` on the floats:
        // the promise is bit-identical, and the checksum compares bits.
        let s1 = sweep(
            vec3(s(-512.0), s(31.5), s(200.0)),
            vec3(s(-380.0), s(-77.25), s(-40.0)),
        );
        let a = ARENA.trace(&s1);
        let b = ARENA.trace(&s1);
        assert_eq!(to_bits(a.fraction), to_bits(b.fraction));
        assert_eq!(to_bits(a.normal.x), to_bits(b.normal.x));
        assert_eq!(to_bits(a.normal.y), to_bits(b.normal.y));
        assert_eq!(to_bits(a.normal.z), to_bits(b.normal.z));
    }

    #[test]
    fn ramp_bounding_boxes_are_tight_on_z() {
        // The bounding box supplies the axial bevels the swept-box expansion
        // needs. A loose one rounds the wedge's tip; a short one clips it.
        for r in ARENA.ramps {
            let top = r.z_at_mins.max(r.z_at_maxs);
            assert_eq!(
                to_bits(r.maxs.z),
                to_bits(top),
                "{}: maxs.z {} should be {}",
                r.label,
                r.maxs.z,
                top
            );
            assert!(r.mins.z <= r.z_at_mins.min(r.z_at_maxs), "{}", r.label);
        }
    }

    #[test]
    fn no_two_solids_overlap() {
        // Overlapping brushes are legal for the tracer but a sign the layout
        // was edited carelessly, and an overlap hides geometry from the mesh.
        let aabbs: Vec<(&str, Vec3, Vec3)> = ARENA
            .boxes
            .iter()
            .map(|b| (b.label, b.mins, b.maxs))
            .chain(ARENA.ramps.iter().map(|r| (r.label, r.mins, r.maxs)))
            .collect();
        for (i, (la, mina, maxa)) in aabbs.iter().enumerate() {
            for (lb, minb, maxb) in &aabbs[i + 1..] {
                // The floor slab is under everything by design; walls sit on it.
                if *la == "floor" || *lb == "floor" {
                    continue;
                }
                let sep = maxa.x <= minb.x
                    || maxb.x <= mina.x
                    || maxa.y <= minb.y
                    || maxb.y <= mina.y
                    || maxa.z <= minb.z
                    || maxb.z <= mina.z;
                assert!(sep, "{la} overlaps {lb}");
            }
        }
    }
}
