//! An axis-aligned box, for the parts of a compiled map that are not one hull.
//!
//! A [`Hull`](straf3_collision::Hull) carries its own `mins`/`maxs`, computed
//! from its face polygons by `compile_hull` — this crate does not compute those
//! and must not second-guess them. What it does need is the *union*: the extent
//! of a whole map, and the extent of a trigger volume made of several convex
//! pieces, so a caller can reject early without asking every hull.

use glam::Vec3;

use crate::digest::Fnv1a;

/// An axis-aligned bounding box.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Aabb {
    /// Low corner.
    pub mins: Vec3,
    /// High corner.
    pub maxs: Vec3,
}

impl Aabb {
    /// A box that contains nothing, ready to be grown by [`Aabb::add_point`].
    #[must_use]
    pub fn empty() -> Self {
        Self {
            mins: Vec3::splat(f32::INFINITY),
            maxs: Vec3::splat(f32::NEG_INFINITY),
        }
    }

    /// Whether nothing has been added.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.mins.x > self.maxs.x || self.mins.y > self.maxs.y || self.mins.z > self.maxs.z
    }

    /// Grow to contain `p`.
    ///
    /// Componentwise `min`/`max`, so the result does not depend on the order
    /// points arrive in — the one place bounds could have become order-sensitive
    /// and therefore non-deterministic.
    pub fn add_point(&mut self, p: Vec3) {
        self.mins = self.mins.min(p);
        self.maxs = self.maxs.max(p);
    }

    /// Grow to contain the box `mins..maxs`.
    pub fn add(&mut self, mins: Vec3, maxs: Vec3) {
        self.add_point(mins);
        self.add_point(maxs);
    }

    /// Grow to contain `other`.
    pub fn add_box(&mut self, other: &Self) {
        if !other.is_empty() {
            self.add(other.mins, other.maxs);
        }
    }

    /// Whether the two boxes share any volume.
    #[must_use]
    pub fn intersects(&self, other: &Self) -> bool {
        self.mins.x <= other.maxs.x
            && other.mins.x <= self.maxs.x
            && self.mins.y <= other.maxs.y
            && other.mins.y <= self.maxs.y
            && self.mins.z <= other.maxs.z
            && other.mins.z <= self.maxs.z
    }

    pub(crate) fn fold(&self, h: &mut Fnv1a) {
        h.vec3(self.mins);
        h.vec3(self.maxs);
    }
}

impl Default for Aabb {
    fn default() -> Self {
        Self::empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bounds_grow_regardless_of_the_order_points_arrive() {
        let pts = [
            Vec3::new(3.0, -1.0, 7.0),
            Vec3::new(-4.0, 8.0, 0.0),
            Vec3::new(1.0, 2.0, -9.0),
        ];
        let mut a = Aabb::empty();
        for p in pts {
            a.add_point(p);
        }
        let mut b = Aabb::empty();
        for p in pts.iter().rev() {
            b.add_point(*p);
        }
        assert_eq!(a, b);
        assert_eq!(a.mins, Vec3::new(-4.0, -1.0, -9.0));
        assert_eq!(a.maxs, Vec3::new(3.0, 8.0, 7.0));
    }

    #[test]
    fn an_empty_box_contributes_nothing() {
        let mut a = Aabb::empty();
        a.add_box(&Aabb::empty());
        assert!(a.is_empty());
        a.add(Vec3::ZERO, Vec3::splat(64.0));
        assert!(!a.is_empty());
        assert_eq!(a.maxs, Vec3::splat(64.0));
    }

    #[test]
    fn overlap_is_inclusive_at_the_boundary() {
        let a = Aabb {
            mins: Vec3::ZERO,
            maxs: Vec3::splat(64.0),
        };
        let touching = Aabb {
            mins: Vec3::new(64.0, 0.0, 0.0),
            maxs: Vec3::splat(128.0),
        };
        let clear = Aabb {
            mins: Vec3::splat(65.0),
            maxs: Vec3::splat(128.0),
        };
        assert!(a.intersects(&touching));
        assert!(!a.intersects(&clear));
    }
}
