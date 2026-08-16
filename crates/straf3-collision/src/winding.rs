//! Turning a set of half-spaces back into polygons.
//!
//! # Why a collision crate computes polygons at all
//!
//! A `.map` brush is given as planes, and the trace in [`crate::trace`] wants
//! nothing but planes. Two things still need the *faces*:
//!
//! - the hull's bounding box, which the broadphase reject in
//!   [`trace_hull`](crate::trace::trace_hull) tests against, and which cannot
//!   be derived from planes without intersecting them;
//! - the edge bevels in [`crate::hull`], which are constructed from face edges.
//!
//! And one thing outside this crate does: the render mesh. `arena.rs` opens by
//! saying the thing you collide with and the thing you see must be the same
//! geometry, because two copies drift and the failure mode is an invisible wall
//! or a ramp you fall through. That argument does not weaken when the geometry
//! arrives from a file, so [`hull_windings`] is public: `straf3-map` can build
//! its render mesh from the same polygons the collision bounds came from,
//! rather than deriving a second set that agrees until it doesn't.
//!
//! # Why it is q3map's algorithm
//!
//! The obvious method — intersect every triple of planes, keep the points
//! inside all the others — is not what `q3map` does, and the difference shows
//! up on real maps. q3map starts each face as an enormous quad on its own plane
//! and clips it against every other plane in turn (`BaseWindingForPlane` and
//! `ChopWindingInPlace`), which is numerically better behaved on the near-
//! parallel planes that hand-built brushes are full of, and which keeps axial
//! faces *exactly* axial thanks to the special case in [`chop_winding`]. Since
//! the bevel planes derived from these windings become collision geometry, the
//! choice of algorithm is a choice about where the player is allowed to stand.

use straf3_sim::num::{Scalar, Vec3, s, vec3};

use crate::plane::Plane;

/// Half the size of the box `q3map` projects onto a plane to start a winding.
///
/// `MAX_MAP_BOUNDS`. Any value past the map's own extent works; this one is
/// Quake 3's own limit, so a winding starts at least as large as anything that
/// can legally be clipped out of it.
pub const MAX_MAP_BOUNDS: Scalar = s(65_536.0);

/// A convex polygon, as a ring of points.
pub type Winding = Vec<Vec3>;

/// `q3map`'s `BaseWindingForPlane`: an enormous quad lying on `plane`.
///
/// The starting point for clipping. It is deliberately far bigger than any map,
/// so that everything the other planes remove is a real removal rather than an
/// artefact of where the quad happened to end.
#[must_use]
pub fn base_winding_for_plane(plane: &Plane) -> Winding {
    // Find the major axis, and pick an up vector that is not parallel to it.
    let mut max = -MAX_MAP_BOUNDS;
    let mut x = 0usize;
    for i in 0..3 {
        let v = plane.normal[i].abs();
        if v > max {
            x = i;
            max = v;
        }
    }
    let mut vup = Vec3::ZERO;
    match x {
        2 => vup.x = s(1.0),
        _ => vup.z = s(1.0),
    }

    // Gram-Schmidt vup against the normal, then normalise.
    let v = vup.dot(plane.normal);
    vup += plane.normal * -v;
    let len = vup.dot(vup).sqrt();
    if len > s(0.0) {
        vup *= s(1.0) / len;
    }

    let org = plane.normal * plane.dist;
    let vright = vup.cross(plane.normal);

    let vup = vup * MAX_MAP_BOUNDS;
    let vright = vright * MAX_MAP_BOUNDS;

    vec![
        org - vright + vup,
        org + vright + vup,
        org + vright - vup,
        org - vright - vup,
    ]
}

/// Which side of a plane a point fell on.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Side {
    /// Outside the solid — the part a chop discards.
    Outside,
    /// Inside the solid — the part a chop keeps.
    Inside,
    /// On the plane, within `epsilon`.
    On,
}

/// Clip `winding` to the inside of `plane`, returning `None` if nothing is left.
///
/// `q3map`'s `ChopWindingInPlace`/`ClipWindingEpsilon`, with the sense already
/// flipped: q3map keeps the *front* of the plane it is handed and hands it the
/// mirrored plane, which is the same as keeping the side this crate calls
/// inside (`p · normal <= dist`).
///
/// The axial special case in the split-point loop is q3map's and is worth
/// keeping: on an axis-aligned plane the new point's coordinate is set to the
/// plane distance outright instead of being interpolated, so a box brush's
/// corners come out at exactly the integers the mapper typed.
#[must_use]
pub fn chop_winding(winding: &Winding, plane: &Plane, epsilon: Scalar) -> Option<Winding> {
    let n = winding.len();
    let mut dists = Vec::with_capacity(n + 1);
    let mut sides = Vec::with_capacity(n + 1);
    let mut counts = [0usize; 3];

    for p in winding {
        let d = plane.distance_to(*p);
        let side = if d < -epsilon {
            counts[Side::Inside as usize] += 1;
            Side::Inside
        } else if d > epsilon {
            counts[Side::Outside as usize] += 1;
            Side::Outside
        } else {
            counts[Side::On as usize] += 1;
            Side::On
        };
        dists.push(d);
        sides.push(side);
    }
    // The ring closes: point n is point 0 again.
    if n > 0 {
        sides.push(sides[0]);
        dists.push(dists[0]);
    }

    if counts[Side::Inside as usize] == 0 {
        return None; // entirely outside
    }
    if counts[Side::Outside as usize] == 0 {
        return Some(winding.clone()); // entirely inside; unchanged
    }

    let mut out: Winding = Vec::with_capacity(n + 4);
    for i in 0..n {
        let p1 = winding[i];
        if sides[i] == Side::On {
            out.push(p1);
            continue;
        }
        if sides[i] == Side::Inside {
            out.push(p1);
        }
        if sides[i + 1] == Side::On || sides[i + 1] == sides[i] {
            continue;
        }

        // The edge crosses the plane: generate the split point.
        let p2 = winding[(i + 1) % n];
        let dot = dists[i] / (dists[i] - dists[i + 1]);
        let mut mid = Vec3::ZERO;
        for j in 0..3 {
            if plane.normal[j] == s(1.0) {
                mid[j] = plane.dist;
            } else if plane.normal[j] == s(-1.0) {
                mid[j] = -plane.dist;
            } else {
                mid[j] = p1[j] + dot * (p2[j] - p1[j]);
            }
        }
        out.push(mid);
    }
    Some(out)
}

/// `q3map`'s `CreateBrushWindings`: the polygon of every face of a convex hull.
///
/// `windings[i]` is the face lying on `planes[i]`, in the same order, empty if
/// that plane contributes no face (it was redundant, or the hull is degenerate).
/// Points run in a consistent ring; the ring's orientation follows from the
/// plane's outward normal, so a renderer can triangulate a fan from point 0
/// without re-deriving which way is out.
///
/// A plane is never clipped by its own mirror image — a hull cannot be bounded
/// twice by the same surface facing both ways, and clipping by it would erase
/// the face. That check is q3map's `planenum ^ 1` test, expressed here as an
/// exact comparison against the flipped plane.
#[must_use]
pub fn hull_windings(planes: &[Plane]) -> Vec<Winding> {
    let mut out = Vec::with_capacity(planes.len());
    for (i, plane) in planes.iter().enumerate() {
        let mut w = Some(base_winding_for_plane(plane));
        for (j, other) in planes.iter().enumerate() {
            if i == j {
                continue;
            }
            if other.normal == -plane.normal && other.dist == -plane.dist {
                continue;
            }
            match w {
                Some(ref current) => w = chop_winding(current, other, s(0.0)),
                None => break,
            }
        }
        out.push(w.unwrap_or_default());
    }
    out
}

/// The bounding box of every point in every winding, or `None` if there are no
/// points at all — which means the planes enclose nothing.
#[must_use]
pub fn winding_bounds(windings: &[Winding]) -> Option<(Vec3, Vec3)> {
    let mut mins = vec3(MAX_MAP_BOUNDS, MAX_MAP_BOUNDS, MAX_MAP_BOUNDS);
    let mut maxs = -mins;
    let mut any = false;
    for w in windings {
        for p in w {
            any = true;
            mins = mins.min(*p);
            maxs = maxs.max(*p);
        }
    }
    if any { Some((mins, maxs)) } else { None }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The six axial planes of a box, in q3map's canonical order.
    fn box_planes(mins: Vec3, maxs: Vec3) -> Vec<Plane> {
        vec![
            Plane::new(vec3(s(-1.0), s(0.0), s(0.0)), -mins.x),
            Plane::new(vec3(s(1.0), s(0.0), s(0.0)), maxs.x),
            Plane::new(vec3(s(0.0), s(-1.0), s(0.0)), -mins.y),
            Plane::new(vec3(s(0.0), s(1.0), s(0.0)), maxs.y),
            Plane::new(vec3(s(0.0), s(0.0), s(-1.0)), -mins.z),
            Plane::new(vec3(s(0.0), s(0.0), s(1.0)), maxs.z),
        ]
    }

    #[test]
    fn a_base_winding_lies_on_its_plane_and_is_enormous() {
        let plane = Plane::new(vec3(s(0.0), s(0.0), s(1.0)), s(64.0));
        let w = base_winding_for_plane(&plane);
        assert_eq!(w.len(), 4);
        for p in &w {
            assert!(plane.distance_to(*p).abs() < s(0.01), "{p:?} is off-plane");
            assert!(p.z == s(64.0));
        }
        // Big enough that no map coordinate can escape it.
        assert!(w.iter().any(|p| p.x.abs() >= MAX_MAP_BOUNDS));
    }

    #[test]
    fn a_box_produces_six_quads_with_exact_integer_corners() {
        let mins = vec3(s(-64.0), s(-32.0), s(0.0));
        let maxs = vec3(s(64.0), s(32.0), s(128.0));
        let windings = hull_windings(&box_planes(mins, maxs));

        assert_eq!(windings.len(), 6);
        for (i, w) in windings.iter().enumerate() {
            assert_eq!(w.len(), 4, "face {i} should be a quad, got {w:?}");
            for p in w {
                // Exactly on the grid: no interpolation error anywhere.
                assert!(p.x == mins.x || p.x == maxs.x, "{p:?}");
                assert!(p.y == mins.y || p.y == maxs.y, "{p:?}");
                assert!(p.z == mins.z || p.z == maxs.z, "{p:?}");
            }
        }
        assert_eq!(winding_bounds(&windings), Some((mins, maxs)));
    }

    #[test]
    fn a_wedge_loses_the_face_the_slope_cuts_away() {
        // A box with a 45° cut across it: the +X face is clipped to nothing
        // where the slope reaches the corner, and the slope's own face appears.
        let mins = vec3(s(0.0), s(0.0), s(0.0));
        let maxs = vec3(s(128.0), s(64.0), s(128.0));
        let mut planes = box_planes(mins, maxs);
        let k = core::f32::consts::FRAC_1_SQRT_2;
        // Rises along +X: normal (-k, 0, k), through (0,0,0).
        planes.push(Plane::through(vec3(-k, s(0.0), k), mins));

        let windings = hull_windings(&planes);
        assert_eq!(windings.len(), 7);
        // The +Z face is entirely cut away by the slope: the slope meets the
        // top only along the single line x = 128, so no area is left.
        assert!(
            windings[5].len() < 3,
            "the top face should be gone, got {:?}",
            windings[5]
        );
        // The slope's own face is a quad spanning the full Y width.
        assert_eq!(windings[6].len(), 4);
        assert_eq!(winding_bounds(&windings), Some((mins, maxs)));
    }

    #[test]
    fn planes_enclosing_nothing_produce_no_bounds() {
        // Two parallel planes facing away from each other: the "solid" between
        // them is empty. A hull built from this must not become an invisible
        // wall, so the emptiness has to be detectable.
        let planes = vec![
            Plane::new(vec3(s(0.0), s(0.0), s(1.0)), s(-64.0)),
            Plane::new(vec3(s(0.0), s(0.0), s(-1.0)), s(-64.0)),
        ];
        assert_eq!(winding_bounds(&hull_windings(&planes)), None);
    }

    #[test]
    fn chopping_is_stable_when_the_plane_touches_a_single_corner() {
        let mins = vec3(s(0.0), s(0.0), s(0.0));
        let maxs = vec3(s(64.0), s(64.0), s(64.0));
        let windings = hull_windings(&box_planes(mins, maxs));
        // A plane through the +X face exactly: clipping the top by it leaves
        // the top untouched (everything is on or inside).
        let touching = Plane::new(vec3(s(1.0), s(0.0), s(0.0)), s(64.0));
        let chopped = chop_winding(&windings[5], &touching, s(0.0)).unwrap();
        assert_eq!(chopped.len(), 4);
    }
}
