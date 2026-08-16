//! The digest the compiled map is identified by.
//!
//! # Why FNV-1a, and why it is spelled out here rather than pulled in
//!
//! [`SimState::checksum`] already folds the simulation's state with FNV-1a, for
//! reasons its doc comment states: order-dependent, no allocation, no
//! dependency, and identical on every platform for the same input bits. The
//! same three properties are what C7's `collision_digest` needs, so this is the
//! same fold over different bytes, deliberately written the same way.
//!
//! The one rule that matters: **everything folded here goes in as bits, never
//! as a value.** `0.0` and `-0.0` compare equal and are not the same geometry;
//! a hull whose plane distance changed in the last bit is not the same hull.
//! A digest that could not tell them apart would certify a recording that no
//! longer replays.
//!
//! [`SimState::checksum`]: straf3_sim::SimState::checksum

use glam::Vec3;
use straf3_collision::Hull;

/// An FNV-1a fold in progress.
///
/// Order-dependent by construction: folding the same planes in a different
/// order produces a different digest, which is exactly what C7 wants — the
/// order hulls are traced in is part of the geometry's behaviour, because
/// ties between coincident faces are resolved by index.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Fnv1a(u64);

const OFFSET: u64 = 0xcbf2_9ce4_8422_2325;
const PRIME: u64 = 0x0000_0100_0000_01b3;

impl Fnv1a {
    /// A fold that has seen nothing.
    #[must_use]
    pub const fn new() -> Self {
        Self(OFFSET)
    }

    /// Fold one byte.
    pub fn byte(&mut self, b: u8) {
        self.0 ^= b as u64;
        self.0 = self.0.wrapping_mul(PRIME);
    }

    /// Fold a byte string.
    pub fn bytes(&mut self, bytes: &[u8]) {
        for b in bytes {
            self.byte(*b);
        }
    }

    /// Fold a `u32`, little-endian.
    pub fn u32(&mut self, v: u32) {
        for b in v.to_le_bytes() {
            self.byte(b);
        }
    }

    /// Fold a `u64`, little-endian.
    pub fn u64(&mut self, v: u64) {
        for b in v.to_le_bytes() {
            self.byte(b);
        }
    }

    /// Fold a `usize` as a `u64`, so a 32-bit target and a 64-bit target agree.
    ///
    /// wasm is 32-bit. Folding a `usize` in its native width would give the
    /// browser a different digest for identical geometry, which is the exact
    /// failure C7 requirement 3 exists to prevent.
    pub fn len(&mut self, v: usize) {
        self.u64(v as u64);
    }

    /// Fold the **bits** of a scalar, not its value.
    pub fn f32(&mut self, v: f32) {
        self.u32(v.to_bits());
    }

    /// Fold the bits of all three components, in x, y, z order.
    pub fn vec3(&mut self, v: Vec3) {
        self.f32(v.x);
        self.f32(v.y);
        self.f32(v.z);
    }

    /// The digest so far.
    #[must_use]
    pub const fn finish(&self) -> u64 {
        self.0
    }
}

impl Default for Fnv1a {
    fn default() -> Self {
        Self::new()
    }
}

/// Fold one hull: every plane, its bounds, its surface flags.
///
/// A free function rather than a method because [`Hull`] belongs to
/// `straf3-collision` — the crate that traces it owns its shape, and the crate
/// that identifies it owns what identity means.
///
/// The planes folded here are the **compiled** ones: deduplicated, permuted
/// into q3map's canonical order and extended with bevels by `compile_hull`.
/// That is deliberate and it is the whole point of C7 requirement 4 — the
/// digest has to be over the geometry the physics actually ran against, not
/// over the plane list the `.map` file happened to list. A bevel that changes
/// is a wall that moves.
pub fn fold_hull(hull: &Hull, h: &mut Fnv1a) {
    h.len(hull.planes.len());
    for plane in &hull.planes {
        h.vec3(plane.normal);
        h.f32(plane.dist);
    }
    h.vec3(hull.mins);
    h.vec3(hull.maxs);
    h.u32(hull.surface.0);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn signed_zero_is_not_the_same_geometry() {
        // The reason every field goes in as bits. These two are `==` as floats
        // and are different planes.
        let mut a = Fnv1a::new();
        a.f32(0.0);
        let mut b = Fnv1a::new();
        b.f32(-0.0);
        assert_ne!(a.finish(), b.finish());
    }

    #[test]
    fn order_changes_the_digest() {
        let mut a = Fnv1a::new();
        a.f32(1.0);
        a.f32(2.0);
        let mut b = Fnv1a::new();
        b.f32(2.0);
        b.f32(1.0);
        assert_ne!(a.finish(), b.finish());
    }

    #[test]
    fn a_last_bit_change_is_visible() {
        let mut a = Fnv1a::new();
        a.f32(1024.0);
        let mut b = Fnv1a::new();
        b.f32(f32::from_bits(1024.0f32.to_bits() + 1));
        assert_ne!(a.finish(), b.finish());
    }

    #[test]
    fn lengths_fold_at_a_fixed_width() {
        // What makes the wasm digest match native's: `usize` never goes in at
        // its native width.
        let mut a = Fnv1a::new();
        a.len(7);
        let mut b = Fnv1a::new();
        b.u64(7);
        assert_eq!(a.finish(), b.finish());
    }
}
