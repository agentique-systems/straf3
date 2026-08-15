//! The numeric seam: every number the simulation computes with passes
//! through here.
//!
//! # Why this module exists
//!
//! Spec section 4 promises determinism as *same binary, same machine,
//! bit-identical*, and explicitly does **not** promise cross-platform
//! bit-exactness, because getting there needs either fixed-point arithmetic
//! or strict float-op control. That is a real possibility later — netplay,
//! cross-machine ghost validation and RL environments all want it — and the
//! only thing that makes it affordable later is that the physics never names
//! `f32` directly.
//!
//! So: the physics says [`Scalar`] and [`Vec3`]. Swapping in a fixed-point
//! representation then means editing this file and fixing whatever fails to
//! compile, rather than auditing every arithmetic expression in the crate.
//!
//! # Why it is this small
//!
//! It is four items, and each one is here because a fixed-point swap breaks
//! without it:
//!
//! - [`Scalar`] / [`Vec3`] — the types themselves.
//! - [`s`] — float literals. `0.5` is not a valid fixed-point value, so a
//!   literal written bare in the physics is a compile error the day we swap.
//!   Written `s(0.5)` it is one function's problem.
//! - [`seconds_from_millis`] — the one place an integer millisecond duration
//!   becomes a scalar. Spec D2 makes command duration an integer; this is the
//!   single crossing point between that integer world and the arithmetic
//!   world, and it is the conversion Q3's rate-coupled behaviour hinges on.
//! - [`to_bits`] — the exact bit pattern, for checksums and determinism tests.
//!
//! There is deliberately no wrapper type, no operator-overload layer and no
//! trait. An abstraction tower here would cost readability in the one part of
//! the codebase that most needs to read like the Quake 3 source it is being
//! checked against.

/// The scalar type all simulation arithmetic uses.
///
/// `f32` and not `f64` because Quake 3 was `f32`, and the quirks we are
/// reproducing — overbounce, ramp boosts, edge clipping — are in part
/// artefacts of `f32` rounding. Widening would quietly file them off.
pub type Scalar = f32;

/// The vector type all simulation arithmetic uses.
///
/// Quake conventions: **Z is up**, units are Quake units (a player is 56 units
/// tall, gravity is 800 units/s²). Keeping the units identical to Q3 is what
/// lets the constants in [`crate::PhysicsProfile`] be compared against the GPL
/// source without a conversion factor standing in the way.
pub type Vec3 = glam::Vec3;

/// A scalar literal.
///
/// Write `s(0.5)` rather than `0.5` inside the physics. See the module docs:
/// this is the hook that makes a fixed-point swap a compile error to be fixed
/// in one place rather than a silent truncation everywhere.
#[inline]
#[must_use]
pub const fn s(v: f32) -> Scalar {
    v
}

/// Build a vector from scalars.
#[inline]
#[must_use]
pub const fn vec3(x: Scalar, y: Scalar, z: Scalar) -> Vec3 {
    Vec3::new(x, y, z)
}

/// The zero vector.
pub const ZERO: Vec3 = Vec3::ZERO;

/// Straight up, in the simulation's Z-up convention.
pub const UP: Vec3 = vec3(s(0.0), s(0.0), s(1.0));

/// Convert an integer-millisecond command duration to seconds.
///
/// **This is the D2 crossing point.** Command durations are integers
/// (see [`crate::UserCmd::duration_ms`]) because Quake 3 truncated frame
/// durations to whole milliseconds, and that truncation *is* the mechanism
/// behind the rate-coupled behaviour — `com_maxfps 125` jump height, the
/// framerate-dependent strafe efficiency. A float duration reproduces none of
/// it. Keeping the integer all the way to this single function means the
/// truncation happens exactly once, exactly where Q3's did
/// (`pml.frametime = pml.msec * 0.001`), and nowhere else.
#[inline]
#[must_use]
pub fn seconds_from_millis(ms: u32) -> Scalar {
    s(ms as f32) * s(0.001)
}

/// The exact bit pattern of a scalar.
///
/// Determinism here means bit-identical, not approximately equal, so the
/// tests and [`crate::SimState::checksum`] compare bits. Going through this
/// function keeps that true after a fixed-point swap.
#[inline]
#[must_use]
pub fn to_bits(v: Scalar) -> u32 {
    v.to_bits()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn millisecond_conversion_matches_quake3() {
        // Q3: pml.frametime = pml.msec * 0.001
        assert_eq!(seconds_from_millis(8), 8.0f32 * 0.001);
        assert_eq!(seconds_from_millis(0), s(0.0));
    }

    #[test]
    fn bits_distinguish_signed_zero() {
        // The reason determinism tests compare bits and not values: these two
        // are `==` but are not the same state.
        assert_ne!(to_bits(s(0.0)), to_bits(s(-0.0)));
    }
}
