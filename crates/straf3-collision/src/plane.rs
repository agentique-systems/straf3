//! Half-spaces: the one primitive every convex hull is built from.
//!
//! A [`Plane`] here is Quake's `plane_t` and nothing more: an outward-facing
//! normal and a distance along it. The convention — **solid is where
//! `p · normal <= dist`** — is the one `arena.rs` used and the one Quake 3's
//! `cbrushside_t` uses, and it is what makes the trace in [`crate::trace`] a
//! transcription rather than an adaptation.
//!
//! The constructors here matter more than they look. A `.map` file does not
//! store planes; it stores three points per face, and turning those into a
//! plane is where a map compiler quietly diverges from Q3 if it improvises.
//! [`Plane::from_points`] is `q3map`'s `PlaneFromPoints` followed by the
//! `SnapPlane` that `FindFloatPlane` applies to every plane it registers, in
//! that order, because that pair is the path every face in a compiled Quake 3
//! map actually took.

use straf3_sim::num::{Scalar, Vec3, s};

/// How close a normal component must be to ±1 before snapping to exactly axial.
///
/// `q3map`'s `normalEpsilon`. Small on purpose: this is not a tolerance for
/// sloppy geometry, it is a correction for the float error in normalising a
/// face that was *meant* to be axial.
pub const NORMAL_EPSILON: Scalar = s(0.000_01);

/// How close a plane distance must be to a whole number before snapping to it.
///
/// `q3map`'s `distanceEpsilon`. Map geometry is authored on an integer grid, so
/// a distance of `128.000_01` is a rounding artefact, not a design.
pub const DIST_EPSILON: Scalar = s(0.01);

/// One outward half-space. The solid is where `p · normal <= dist`.
///
/// `normal` must be unit length. The trace's arithmetic is scale-invariant so a
/// non-unit normal would still produce the right *fraction*, but the normal is
/// handed back to the simulation, which clips velocity along it and compares
/// its Z against `min_walk_normal` — both of which are wrong to within whatever
/// the scale factor was. Use [`Plane::from_points`] or [`Plane::through`] with
/// a normalised normal.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Plane {
    /// Unit normal, pointing out of the solid.
    pub normal: Vec3,
    /// Distance from the origin along [`normal`](Self::normal).
    pub dist: Scalar,
}

impl Plane {
    /// A plane from a normal and a distance, taken as given.
    #[must_use]
    pub const fn new(normal: Vec3, dist: Scalar) -> Self {
        Self { normal, dist }
    }

    /// The plane with this normal passing through this point.
    #[must_use]
    pub fn through(normal: Vec3, point: Vec3) -> Self {
        Self {
            normal,
            dist: point.dot(normal),
        }
    }

    /// The plane of a `.map` face, from the three points the file lists.
    ///
    /// `q3map`'s `PlaneFromPoints`:
    ///
    /// ```c
    /// VectorSubtract( b, a, d1 );
    /// VectorSubtract( c, a, d2 );
    /// CrossProduct( d2, d1, plane );
    /// VectorNormalize( plane, plane );
    /// plane[3] = DotProduct( a, plane );
    /// ```
    ///
    /// followed by [`snapped`](Self::snapped), because `FindFloatPlane` — which
    /// is what q3map calls the moment it has a plane — snaps before it stores.
    /// The cross-product order is not interchangeable: it is what makes the
    /// normal point *out* of the brush given Quake's face winding.
    ///
    /// Returns `None` for three collinear or coincident points, which describe
    /// no plane. q3map only checks for a zero-length normal; the finiteness
    /// check here additionally refuses to hand back a hull full of `NaN`, which
    /// would silently answer "no hit" to every sweep for the rest of the run.
    #[must_use]
    pub fn from_points(a: Vec3, b: Vec3, c: Vec3) -> Option<Self> {
        let d1 = b - a;
        let d2 = c - a;
        let normal = d2.cross(d1);
        // Q3's VectorNormalize: length, then multiply by its reciprocal. Not
        // `normal / length` — the two round differently and this one is Q3's.
        let length = normal.dot(normal).sqrt();
        if length == s(0.0) {
            return None;
        }
        let normal = normal * (s(1.0) / length);
        if !normal.is_finite() {
            return None;
        }
        Some(Self::through(normal, a).snapped())
    }

    /// Signed distance from the plane: negative inside the solid, positive out.
    #[must_use]
    pub fn distance_to(&self, point: Vec3) -> Scalar {
        point.dot(self.normal) - self.dist
    }

    /// The same plane facing the other way.
    #[must_use]
    pub fn flipped(&self) -> Self {
        Self {
            normal: -self.normal,
            dist: -self.dist,
        }
    }

    /// `q3map`'s `SnapPlane`: pull a nearly-axial normal onto the axis, and a
    /// nearly-integer distance onto the integer.
    ///
    /// This is not cosmetic. `min_walk_normal` is a comparison against the
    /// normal's Z, and the axial-bevel test in
    /// [`add_hull_bevels`](crate::hull::add_hull_bevels) is an *exact* `== 1.0`
    /// against a normal's component — a floor whose normal is `0.999_999_9`
    /// instead of `1.0` is a floor that gets a redundant bevel plane and a
    /// walkability margin it was never meant to have.
    #[must_use]
    pub fn snapped(&self) -> Self {
        let mut normal = self.normal;
        for axis in 0..3 {
            if (normal[axis] - s(1.0)).abs() < NORMAL_EPSILON {
                normal = Vec3::ZERO;
                normal[axis] = s(1.0);
                break;
            }
            if (normal[axis] + s(1.0)).abs() < NORMAL_EPSILON {
                normal = Vec3::ZERO;
                normal[axis] = s(-1.0);
                break;
            }
        }
        let rounded = self.dist.round();
        let dist = if (self.dist - rounded).abs() < DIST_EPSILON {
            rounded
        } else {
            self.dist
        };
        Self { normal, dist }
    }

    /// `q3map`'s `PlaneEqual`: the same plane to within the snapping epsilons.
    #[must_use]
    pub fn approx_eq(&self, other: &Self) -> bool {
        (self.normal.x - other.normal.x).abs() < NORMAL_EPSILON
            && (self.normal.y - other.normal.y).abs() < NORMAL_EPSILON
            && (self.normal.z - other.normal.z).abs() < NORMAL_EPSILON
            && (self.dist - other.dist).abs() < DIST_EPSILON
    }

    /// How far this plane must move outward to turn a swept box into a swept
    /// point: the support function of a box with these half extents.
    ///
    /// `|n.x|·hx + |n.y|·hy + |n.z|·hz`. This is the whole trick that makes an
    /// AABB sweep cost the same as a ray sweep, and it is exact for the brush's
    /// own faces. What it is *not* exact for is the edges where two faces meet,
    /// which is what bevel planes exist to supply — see [`crate::hull`].
    #[must_use]
    pub fn support_offset(&self, half_extents: Vec3) -> Scalar {
        self.normal.x.abs() * half_extents.x
            + self.normal.y.abs() * half_extents.y
            + self.normal.z.abs() * half_extents.z
    }

    /// Whether the normal is unit length to within a generous tolerance.
    ///
    /// Generous because this is a mistake detector, not a numeric assertion: a
    /// normal that is off by `1e-7` is a normalised float, and one that is off
    /// by `0.3` is an un-normalised face direction someone forgot to divide.
    #[must_use]
    pub fn is_unit(&self) -> bool {
        (self.normal.dot(self.normal) - s(1.0)).abs() < s(1e-4)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use straf3_sim::num::vec3;

    #[test]
    fn a_map_face_winds_its_normal_outward() {
        // The three points of a `.map` face are listed clockwise seen from
        // outside, so this triple must produce -Z, not +Z. Getting this
        // backwards inverts every brush in a map into a hollow shell that
        // nothing collides with — which is why it has a test of its own.
        let p = Plane::from_points(
            vec3(s(0.0), s(0.0), s(0.0)),
            vec3(s(1.0), s(0.0), s(0.0)),
            vec3(s(0.0), s(1.0), s(0.0)),
        )
        .unwrap();
        assert_eq!(p.normal, vec3(s(0.0), s(0.0), s(-1.0)));
        assert_eq!(p.dist, s(0.0));
    }

    #[test]
    fn collinear_points_describe_no_plane() {
        let a = vec3(s(0.0), s(0.0), s(0.0));
        let b = vec3(s(1.0), s(0.0), s(0.0));
        let c = vec3(s(2.0), s(0.0), s(0.0));
        assert!(Plane::from_points(a, b, c).is_none());
        assert!(Plane::from_points(a, a, a).is_none());
    }

    #[test]
    fn a_nearly_axial_normal_snaps_onto_the_axis() {
        let p = Plane::new(vec3(s(0.000_001), s(0.000_002), s(0.999_999_9)), s(64.000_001));
        let snapped = p.snapped();
        assert_eq!(snapped.normal, vec3(s(0.0), s(0.0), s(1.0)));
        assert_eq!(snapped.dist, s(64.0));
    }

    #[test]
    fn a_genuinely_slanted_normal_is_left_alone() {
        // A 45° ramp normal must survive snapping untouched, or the ramp the
        // whole movement model is tuned against stops being a ramp.
        let n = vec3(s(0.0), -core::f32::consts::FRAC_1_SQRT_2, core::f32::consts::FRAC_1_SQRT_2);
        let p = Plane::new(n, s(90.5));
        assert_eq!(p.snapped(), p);
    }

    #[test]
    fn the_support_offset_is_the_box_half_extent_for_an_axial_plane() {
        let h = vec3(s(15.0), s(15.0), s(28.0));
        assert_eq!(
            Plane::new(vec3(s(0.0), s(0.0), s(1.0)), s(0.0)).support_offset(h),
            s(28.0)
        );
        assert_eq!(
            Plane::new(vec3(s(-1.0), s(0.0), s(0.0)), s(0.0)).support_offset(h),
            s(15.0)
        );
    }

    #[test]
    fn distance_is_negative_inside_the_solid() {
        let p = Plane::new(vec3(s(0.0), s(0.0), s(1.0)), s(64.0));
        assert!(p.distance_to(vec3(s(0.0), s(0.0), s(0.0))) < s(0.0));
        assert!(p.distance_to(vec3(s(0.0), s(0.0), s(100.0))) > s(0.0));
    }
}
