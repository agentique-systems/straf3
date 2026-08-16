//! Convex hulls, and the bevel planes that make sweeping a box against one
//! correct.
//!
//! # The hull
//!
//! A [`Hull`] is a Quake brush: a solid described as the intersection of
//! outward half-spaces, plus the bounding box the broadphase rejects against
//! and the surface properties the simulation reads off an impact. The plane
//! count is a `Vec` and not a fixed array because `arena.rs`'s seven-plane
//! ceiling was a property of the arena (six axial faces and one ramp top), not
//! of `.map` brushes, which routinely run from four faces to a few dozen.
//!
//! # Why bevels are not optional
//!
//! The trace expands each plane outward by the player box's support along that
//! plane's normal, which turns a swept box into a swept point. That is exact
//! for the *faces* of a brush and wrong at its *edges*: the true swept volume
//! (the Minkowski sum of the brush and the box) has an extra face for every
//! pairing of a brush edge with a box edge, and simply offsetting the brush's
//! own planes produces a solid that bulges past it at each such edge. The
//! player is then stopped in mid-air, a hull-width from anything visible.
//!
//! `arena.rs` does not suffer from this and its module docs explain why in the
//! specific case it had: every edge of every arena solid runs along an axis, and
//! for an axis-parallel edge the missing bevel *is* one of the six axial planes,
//! which the arena's brushes all carry. Its wider claim — that Quake 3 left edge
//! bevels out for brushes too — does not hold up against the source. `q3map`'s
//! `AddBrushBevels` adds the axial bevels *and* the edge bevels at compile time,
//! and the runtime `CM_TraceThroughBrush` then traces against a plane set that
//! already contains them. A Quake 3 map has bevels; they are simply baked in
//! before the game ever sees the brush.
//!
//! So [`add_hull_bevels`] is that function, transcribed, and matching Q3 here
//! means running it. Skipping it would not preserve a Q3 quirk — it would invent
//! a new one, on exactly the diagonal geometry a Defrag map is full of and the
//! hardcoded arena had none of.
//!
//! The bevels also cost nothing in fidelity where the arena is concerned: for a
//! brush whose edges are all axial, `AddBrushBevels` finds every axial plane
//! already present and adds no edge bevels at all, so the plane set — and every
//! trace against it — is unchanged.

use straf3_sim::num::{Scalar, Vec3, s};
use straf3_sim::world::SurfaceFlags;

use crate::plane::Plane;
use crate::winding::{Winding, hull_windings, winding_bounds};

/// How far a point may sit past a candidate bevel and still count as behind it.
///
/// `q3map`'s literal `0.1` in the outer-hull test. A bevel is only valid if the
/// whole brush is behind it; this is the slack that keeps float error on a
/// shared corner from rejecting a bevel the brush genuinely needs.
const BEVEL_ON_EPSILON: Scalar = s(0.1);

/// The shortest edge worth deriving a bevel from, in map units.
///
/// `q3map`'s `VectorNormalize(...) < 0.5` guards. An edge shorter than half a
/// unit is a numerical artefact of clipping, and its direction is noise.
const MIN_EDGE_LENGTH: Scalar = s(0.5);

/// A convex solid: the intersection of its planes' half-spaces.
///
/// Fields are public because a map compiler builds these directly and there is
/// nothing to protect — but two of them carry obligations that
/// [`Hull::from_planes`] exists to discharge:
///
/// - `mins`/`maxs` **must contain the solid**. [`trace_hull`] rejects against
///   them before it looks at a single plane, so bounds that are too small are
///   not a slow trace, they are a hull the player walks through.
/// - `planes` is used in the order given, and ties in the entry fraction go to
///   the earliest plane. Nothing in this crate reorders a hull's planes behind
///   the caller's back; [`Hull::from_planes`] *does* reorder, because Q3's own
///   compiler does, and it says so.
///
/// [`trace_hull`]: crate::trace::trace_hull
#[derive(Debug, Clone, PartialEq)]
pub struct Hull {
    /// The outward half-spaces bounding the solid, in trace order.
    pub planes: Vec<Plane>,
    /// Low corner of the bounding box. Must contain the solid.
    pub mins: Vec3,
    /// High corner of the bounding box. Must contain the solid.
    pub maxs: Vec3,
    /// Movement-relevant properties of the solid's surfaces.
    pub surface: SurfaceFlags,
}

/// A hull, plus the face polygons the planes it was built from turned out to
/// bound.
///
/// `faces` is parallel to the *input* plane list — `faces[i]` is the polygon of
/// input plane `i`, and is empty when that plane bounds no face (it was
/// redundant, or a duplicate). That correspondence is what lets a map compiler
/// carry each face's texture through to the render mesh, and it is why the
/// faces are reported separately from `hull.planes`, which by then has been
/// deduplicated, reordered and extended with bevels.
#[derive(Debug, Clone, PartialEq)]
pub struct CompiledHull {
    /// The collision solid.
    pub hull: Hull,
    /// One polygon per input plane, in input order.
    pub faces: Vec<Winding>,
}

impl Hull {
    /// A solid axis-aligned box.
    #[must_use]
    pub fn from_aabb(mins: Vec3, maxs: Vec3, surface: SurfaceFlags) -> Self {
        Self {
            planes: axial_planes(mins, maxs).to_vec(),
            mins,
            maxs,
            surface,
        }
    }

    /// Compile a `.map` brush's planes into a hull ready to trace against.
    ///
    /// Returns `None` when the planes enclose no volume — a brush that would
    /// otherwise become an invisible obstacle at the origin, or nothing at all.
    ///
    /// See [`compile_hull`], which this delegates to, for what it does to the
    /// plane list on the way.
    #[must_use]
    pub fn from_planes(planes: &[Plane], surface: SurfaceFlags) -> Option<Self> {
        compile_hull(planes, surface).map(|c| c.hull)
    }

    /// The face polygons of this hull's planes.
    ///
    /// Bevel planes bound no face and come back empty, which is also how q3map
    /// distinguishes them.
    #[must_use]
    pub fn windings(&self) -> Vec<Winding> {
        hull_windings(&self.planes)
    }
}

/// The six axial planes of `mins..maxs`, in q3map's canonical order:
/// `-X +X -Y +Y -Z +Z`.
#[must_use]
pub fn axial_planes(mins: Vec3, maxs: Vec3) -> [Plane; 6] {
    let p = |normal: Vec3, dist: Scalar| Plane::new(normal, dist);
    [
        p(Vec3::new(s(-1.0), s(0.0), s(0.0)), -mins.x),
        p(Vec3::new(s(1.0), s(0.0), s(0.0)), maxs.x),
        p(Vec3::new(s(0.0), s(-1.0), s(0.0)), -mins.y),
        p(Vec3::new(s(0.0), s(1.0), s(0.0)), maxs.y),
        p(Vec3::new(s(0.0), s(0.0), s(-1.0)), -mins.z),
        p(Vec3::new(s(0.0), s(0.0), s(1.0)), maxs.z),
    ]
}

/// Turn a `.map` brush's planes into a [`CompiledHull`], the way q3map does.
///
/// In order:
///
/// 1. **Duplicate planes are dropped.** q3map's `ParseBrush` does this (normals
///    agreeing to within `0.999` and distances to within `0.01`), and a
///    duplicate would otherwise clip its own twin's face away to nothing.
/// 2. **Face polygons are built** by clipping each plane's base winding against
///    all the others — see [`crate::winding`].
/// 3. **The bounding box** comes from those polygons. No polygons at all means
///    the planes enclose nothing, and the brush is discarded (`None`).
/// 4. **Bevels are added** by [`add_hull_bevels`], which also permutes the plane
///    list so the six axial planes come first in the order `-X +X -Y +Y -Z +Z`.
///    That permutation is q3map's, and it is load-bearing for tie-breaking: when
///    two planes report the same entry fraction the earlier one wins, so
///    matching Q3's plane order is matching Q3's choice of impact normal.
#[must_use]
pub fn compile_hull(planes: &[Plane], surface: SurfaceFlags) -> Option<CompiledHull> {
    // 1. Drop duplicates, keeping a map back to the input index so the faces
    //    stay reportable in the caller's own order.
    let mut kept: Vec<Plane> = Vec::with_capacity(planes.len());
    let mut source: Vec<usize> = Vec::with_capacity(planes.len());
    for (i, plane) in planes.iter().enumerate() {
        let duplicate = kept.iter().any(|k| {
            k.normal.dot(plane.normal) > s(0.999) && (k.dist - plane.dist).abs() < s(0.01)
        });
        if !duplicate {
            kept.push(*plane);
            source.push(i);
        }
    }
    if kept.len() < 4 {
        return None; // fewer than four half-spaces can never bound a volume
    }

    // 2 & 3. Faces, then bounds from the faces.
    let mut faces = hull_windings(&kept);
    let (mins, maxs) = winding_bounds(&faces)?;

    // Report the faces against the caller's plane list, not the deduplicated
    // one: a dropped duplicate contributes no face.
    let mut input_faces = vec![Winding::new(); planes.len()];
    for (k, face) in faces.iter().enumerate() {
        input_faces[source[k]] = face.clone();
    }

    // 4. Bevels, on a working copy whose faces travel with their planes.
    add_hull_bevels(&mut kept, &mut faces, mins, maxs);

    Some(CompiledHull {
        hull: Hull {
            planes: kept,
            mins,
            maxs,
            surface,
        },
        faces: input_faces,
    })
}

/// `q3map`'s `AddBrushBevels`: add the planes a swept *box* needs that a swept
/// *point* does not.
///
/// `faces` must be parallel to `planes`; both are permuted together, and each
/// added bevel gets an empty face — which is how q3map marks a bevel too, and
/// how [`Hull::windings`] reports one afterwards.
///
/// Two passes, both Q3's:
///
/// - **Axial bevels.** Every one of the six axial directions that is not already
///   a face of the brush is added, at the brush's own bounding box. A slanted
///   brush therefore ends up carrying its bounding box's planes as well as its
///   own — that is the "the bounding box supplies the axial bevels" property
///   `arena.rs` states for its ramps, generalised.
/// - **Edge bevels.** For every non-axial edge of every non-axial face, and for
///   each of the six axial directions, the plane containing that edge and
///   perpendicular to that direction is a candidate. It is kept only if the
///   entire brush lies behind it — which is what makes it a genuine face of the
///   swept volume rather than a plane slicing through the solid.
///
/// A brush that is exactly six axial faces gets neither pass and comes out
/// untouched, which is why generalising the arena to this code changes nothing
/// about the arena.
pub fn add_hull_bevels(
    planes: &mut Vec<Plane>,
    faces: &mut Vec<Winding>,
    mins: Vec3,
    maxs: Vec3,
) {
    debug_assert_eq!(planes.len(), faces.len(), "faces must be parallel to planes");

    // ── the axial planes ───────────────────────────────────────────────────
    let mut order = 0usize;
    for axis in 0..3 {
        for dir in [s(-1.0), s(1.0)] {
            // Exact equality, as q3map has it: a normal that is only nearly
            // axial is not axial, and gets a bevel of its own. `Plane::snapped`
            // is what keeps that from happening to geometry that meant it.
            let existing = planes.iter().position(|p| p.normal[axis] == dir);
            let i = match existing {
                Some(i) => i,
                None => {
                    let mut normal = Vec3::ZERO;
                    normal[axis] = dir;
                    let dist = if dir == s(1.0) { maxs[axis] } else { -mins[axis] };
                    planes.push(Plane::new(normal, dist));
                    faces.push(Winding::new());
                    planes.len() - 1
                }
            };
            if i != order {
                planes.swap(order, i);
                faces.swap(order, i);
            }
            order += 1;
        }
    }

    if planes.len() == 6 {
        return; // pure axial: nothing an edge bevel could add
    }

    // ── the edge bevels ────────────────────────────────────────────────────
    //
    // Candidates are gathered before any are added, which is exactly what q3map
    // gets for free: the sides it appends inside this loop carry no winding, so
    // they contribute no edges of their own.
    let mut candidates: Vec<(Vec3, Vec3)> = Vec::new();
    for face in faces.iter().skip(6) {
        for j in 0..face.len() {
            let k = (j + 1) % face.len();
            let edge = face[j] - face[k];
            let length = edge.dot(edge).sqrt();
            if length < MIN_EDGE_LENGTH {
                continue;
            }
            let edge = snap_vector(edge * (s(1.0) / length));
            // Only non-axial edges need a bevel; an axial edge's bevels are the
            // axial planes, and those are all present by now.
            if (0..3).any(|a| edge[a] == s(1.0) || edge[a] == s(-1.0)) {
                continue;
            }
            candidates.push((face[j], edge));
        }
    }

    for (point, edge) in candidates {
        for axis in 0..3 {
            for dir in [s(-1.0), s(1.0)] {
                let mut along = Vec3::ZERO;
                along[axis] = dir;
                let normal = edge.cross(along);
                let length = normal.dot(normal).sqrt();
                if length < MIN_EDGE_LENGTH {
                    continue; // the edge is nearly parallel to this axis
                }
                let normal = normal * (s(1.0) / length);
                let candidate = Plane::through(normal, point);

                // Keep it only if every point of every face is behind it, and
                // it is not a plane the hull already has.
                let mut outer_hull = true;
                for (p, face) in planes.iter().zip(faces.iter()) {
                    if p.approx_eq(&candidate) {
                        outer_hull = false;
                        break;
                    }
                    if face
                        .iter()
                        .any(|v| candidate.distance_to(*v) > BEVEL_ON_EPSILON)
                    {
                        outer_hull = false;
                        break;
                    }
                }
                if outer_hull {
                    planes.push(candidate);
                    faces.push(Winding::new());
                }
            }
        }
    }
}

/// `q3map`'s `SnapVector`: pull a nearly-axial direction onto the axis.
fn snap_vector(v: Vec3) -> Vec3 {
    let mut out = v;
    for axis in 0..3 {
        if (out[axis] - s(1.0)).abs() < crate::plane::NORMAL_EPSILON {
            out = Vec3::ZERO;
            out[axis] = s(1.0);
            break;
        }
        if (out[axis] + s(1.0)).abs() < crate::plane::NORMAL_EPSILON {
            out = Vec3::ZERO;
            out[axis] = s(-1.0);
            break;
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use straf3_sim::num::vec3;

    #[test]
    fn a_box_gains_no_bevels_and_keeps_its_planes() {
        let mins = vec3(s(-64.0), s(-64.0), s(0.0));
        let maxs = vec3(s(64.0), s(64.0), s(128.0));
        let compiled = compile_hull(&axial_planes(mins, maxs), SurfaceFlags::NONE).unwrap();
        assert_eq!(compiled.hull.planes.len(), 6);
        assert_eq!(compiled.hull.planes, axial_planes(mins, maxs).to_vec());
        assert_eq!((compiled.hull.mins, compiled.hull.maxs), (mins, maxs));
        // Six quads, in the caller's own plane order.
        assert!(compiled.faces.iter().all(|f| f.len() == 4));
    }

    /// A ramp is a prism: every edge runs along an axis, so the axial planes are
    /// all the bevels it needs and no edge bevel survives the outer-hull test.
    /// This is the property that makes generalising `arena.rs` a no-op for the
    /// arena's own geometry.
    #[test]
    fn an_axis_aligned_wedge_gains_no_edge_bevels() {
        let mins = vec3(s(-640.0), s(-320.0), s(0.0));
        let maxs = vec3(s(-384.0), s(320.0), s(128.0));
        let k = s(128.0) / s(256.0); // rise over run
        let normal = vec3(-k, s(0.0), s(1.0)) / (s(1.0) + k * k).sqrt();
        let mut planes = axial_planes(mins, maxs).to_vec();
        planes.push(Plane::through(normal, vec3(mins.x, mins.y, mins.z)));

        let compiled = compile_hull(&planes, SurfaceFlags::NONE).unwrap();
        assert_eq!(
            compiled.hull.planes.len(),
            7,
            "an axis-aligned wedge needs no bevel beyond its own bounding box"
        );
        assert_eq!(compiled.hull.planes, planes);
    }

    /// The case the arena never had, part one: a brush rotated 45° about Z.
    ///
    /// Its four vertical faces are diagonal, but every *edge* still runs along
    /// an axis — the verticals along Z, the rim edges along the two diagonals of
    /// the XY plane, whose bevels are the ±Z faces it already has. So what it
    /// needs is exactly the four axial planes it lacks, added at its bounding
    /// box, and no edge bevel survives the outer-hull test. `trace.rs` shows
    /// what those four axial planes are worth: 15 units of invisible wall.
    #[test]
    fn a_diagonal_brush_gains_the_axial_planes_it_lacks() {
        let d = core::f32::consts::FRAC_1_SQRT_2;
        let r = s(64.0);
        let planes = vec![
            Plane::new(vec3(d, d, s(0.0)), r),
            Plane::new(vec3(-d, d, s(0.0)), r),
            Plane::new(vec3(-d, -d, s(0.0)), r),
            Plane::new(vec3(d, -d, s(0.0)), r),
            Plane::new(vec3(s(0.0), s(0.0), s(-1.0)), s(0.0)),
            Plane::new(vec3(s(0.0), s(0.0), s(1.0)), s(128.0)),
        ];
        let compiled = compile_hull(&planes, SurfaceFlags::NONE).unwrap();

        assert_eq!(compiled.hull.planes.len(), 10, "six faces plus ±X and ±Y");
        // The first six are the axial ones, in q3map's canonical order.
        for (i, n) in [
            vec3(s(-1.0), s(0.0), s(0.0)),
            vec3(s(1.0), s(0.0), s(0.0)),
            vec3(s(0.0), s(-1.0), s(0.0)),
            vec3(s(0.0), s(1.0), s(0.0)),
            vec3(s(0.0), s(0.0), s(-1.0)),
            vec3(s(0.0), s(0.0), s(1.0)),
        ]
        .into_iter()
        .enumerate()
        {
            assert_eq!(compiled.hull.planes[i].normal, n, "plane {i}");
        }
        // …and the added ones sit on the bounding box, at 64·√2 out.
        assert_eq!(compiled.hull.planes[0].dist, -compiled.hull.mins.x);
        assert_eq!(compiled.hull.planes[1].dist, compiled.hull.maxs.x);
        assert!((compiled.hull.maxs.x - s(90.509_67)).abs() < s(0.001));
    }

    /// The case the arena never had, part two: a corner brush, whose cut face
    /// meets the axial faces along genuinely non-axial edges.
    ///
    /// This is the shape that needs edge bevels and cannot be rescued by axial
    /// ones, and it is ordinary map geometry — a chamfered wall corner, an
    /// angled ramp that also rises.
    #[test]
    fn a_corner_brush_gains_edge_bevels() {
        let planes = corner_brush();
        let compiled = compile_hull(&planes, SurfaceFlags::NONE).unwrap();

        // Four faces, plus the three axial planes it lacks, plus bevels.
        assert!(
            compiled.hull.planes.len() > 7,
            "a corner brush must gain edge bevels, got {} planes",
            compiled.hull.planes.len()
        );
        let bevels = &compiled.hull.planes[7..];
        assert!(
            bevels
                .iter()
                .any(|p| (0..3).all(|a| p.normal[a].abs() != s(1.0))),
            "at least one bevel must be non-axial: {bevels:?}"
        );
        // Every bevel must be a supporting plane of the brush, or it is a plane
        // slicing through the solid and the hull has been made smaller.
        for face in compiled.faces.iter() {
            for v in face {
                for p in &compiled.hull.planes {
                    assert!(
                        p.distance_to(*v) <= BEVEL_ON_EPSILON,
                        "plane {p:?} cuts through vertex {v:?}"
                    );
                }
            }
        }
    }

    /// A tetrahedron on the origin corner: `x, y, z >= 0` and `x + y + z <= 96`.
    pub(crate) fn corner_brush() -> Vec<Plane> {
        let t = s(1.0) / s(3.0).sqrt();
        vec![
            Plane::new(vec3(s(-1.0), s(0.0), s(0.0)), s(0.0)),
            Plane::new(vec3(s(0.0), s(-1.0), s(0.0)), s(0.0)),
            Plane::new(vec3(s(0.0), s(0.0), s(-1.0)), s(0.0)),
            Plane::new(vec3(t, t, t), s(96.0) * t),
        ]
    }

    #[test]
    fn duplicate_planes_are_dropped_and_report_no_face() {
        let mins = vec3(s(0.0), s(0.0), s(0.0));
        let maxs = vec3(s(64.0), s(64.0), s(64.0));
        let mut planes = axial_planes(mins, maxs).to_vec();
        planes.push(planes[5]); // the +Z face again
        let compiled = compile_hull(&planes, SurfaceFlags::NONE).unwrap();
        assert_eq!(compiled.hull.planes.len(), 6);
        assert_eq!(compiled.faces.len(), 7);
        assert!(compiled.faces[6].is_empty());
        assert_eq!(compiled.faces[5].len(), 4);
    }

    #[test]
    fn planes_enclosing_nothing_compile_to_nothing() {
        let planes = vec![
            Plane::new(vec3(s(0.0), s(0.0), s(1.0)), s(-64.0)),
            Plane::new(vec3(s(0.0), s(0.0), s(-1.0)), s(-64.0)),
            Plane::new(vec3(s(1.0), s(0.0), s(0.0)), s(64.0)),
            Plane::new(vec3(s(-1.0), s(0.0), s(0.0)), s(64.0)),
        ];
        assert!(compile_hull(&planes, SurfaceFlags::NONE).is_none());
        assert!(compile_hull(&[], SurfaceFlags::NONE).is_none());
    }

    #[test]
    fn compiling_is_bit_identical_when_repeated() {
        let d = core::f32::consts::FRAC_1_SQRT_2;
        let planes = vec![
            Plane::new(vec3(d, d, s(0.0)), s(64.0)),
            Plane::new(vec3(-d, d, s(0.0)), s(64.0)),
            Plane::new(vec3(-d, -d, s(0.0)), s(64.0)),
            Plane::new(vec3(d, -d, s(0.0)), s(64.0)),
            Plane::new(vec3(s(0.0), s(0.0), s(-1.0)), s(0.0)),
            Plane::new(vec3(s(0.0), s(0.0), s(1.0)), s(128.0)),
        ];
        let a = compile_hull(&planes, SurfaceFlags::NONE).unwrap();
        let b = compile_hull(&planes, SurfaceFlags::NONE).unwrap();
        assert_eq!(a, b);
    }
}
