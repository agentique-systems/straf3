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

/// π/2 split into two `f64` pieces, so the reduction subtracts exactly
/// representable quantities: `PIO2_HI` is the `f64` nearest π/2, `PIO2_LO` the
/// residual it left behind.
const PIO2_HI: f64 = core::f64::consts::FRAC_PI_2;
const PIO2_LO: f64 = 6.123_233_995_736_766e-17;
const TWO_OVER_PI: f64 = core::f64::consts::FRAC_2_PI;

/// Reduce `x` radians to `(quadrant, remainder)` with `|remainder| ≤ π/4`,
/// using only IEEE-specified arithmetic.
///
/// The rounding of `x·2/π` to the nearest integer is done by adding ±0.5 and
/// truncating rather than by calling `round`, so that no libm entry point is
/// reached even for the reduction.
///
/// Float-to-integer casts in Rust saturate, which is what keeps this total:
/// an infinite or NaN argument produces a quadrant rather than a trap, and the
/// polynomial then propagates NaN. That behaviour is identical on every
/// target, including wasm, where rustc emits the saturating truncation
/// instructions rather than the trapping ones.
fn reduce(x: f64) -> (i64, f64) {
    let scaled = x * TWO_OVER_PI;
    let q = (scaled + if scaled >= 0.0 { 0.5 } else { -0.5 }) as i64;
    let qf = q as f64;
    let r = (x - qf * PIO2_HI) - qf * PIO2_LO;
    (q, r)
}

/// The Taylor series for sine, to r⁹, evaluated in Horner form.
fn poly_sin(r: f64) -> f64 {
    let r2 = r * r;
    // r - r³/6 + r⁵/120 - r⁷/5040 + r⁹/362880
    r * (1.0
        + r2 * (-1.0 / 6.0 + r2 * (1.0 / 120.0 + r2 * (-1.0 / 5040.0 + r2 * (1.0 / 362_880.0)))))
}

/// The Taylor series for cosine, to r⁸, evaluated in Horner form.
fn poly_cos(r: f64) -> f64 {
    let r2 = r * r;
    // 1 - r²/2 + r⁴/24 - r⁶/720 + r⁸/40320
    1.0 + r2 * (-0.5 + r2 * (1.0 / 24.0 + r2 * (-1.0 / 720.0 + r2 * (1.0 / 40_320.0))))
}

/// Sine and cosine of an angle in radians.
///
/// Not `f32::sin_cos`. std's implementation is whatever libm the target links,
/// and those disagree: glibc differs from the statically-linked `libm` that
/// wasm and musl builds use, by 1 ulp on roughly 1.3% of angles. A probe
/// measured that gap corrupting a recorded run after about 14 seconds of play
/// — enough to make a browser recording unverifiable on a glibc server.
///
/// Cody-Waite argument reduction plus Taylor polynomials, using only
/// `+ - * /`. Every operation is IEEE-specified and correctly rounded, so this
/// is identical on every conforming target by construction — which is a
/// stronger promise than "we all call the same library", and it costs no
/// dependency. Q3 shipped its own tables for substantially this reason.
///
/// # Accuracy is not the goal
///
/// The physics is defined as whatever this function returns. What cannot be
/// tolerated is two answers, not error against the real sine. The measured
/// numbers, for the record: within 1 ulp of both glibc's and musl's `sinf`
/// and `cosf` for every `f32` angle below 8192°, degrading past that (12 ulp
/// by 8192° for cosine, by 16384° for sine, and further beyond) because the
/// reduction's cancellation is bounded by `f64`'s 53 bits however precisely
/// π/2 is stored. Angles that large are unreachable once view angles are
/// 16-bit at the command boundary, and the degradation is deterministic
/// regardless — it is an accuracy limit, not a determinism one.
///
/// # This is not fixed-point-swap-clean
///
/// Unlike the rest of this module, the body names `f64` directly. A
/// fixed-point [`Scalar`] would need this rewritten rather than retyped —
/// which is the correct outcome: a fixed-point simulation wants a table or a
/// CORDIC, not a Taylor series in a wider float.
#[inline]
#[must_use]
pub fn sin_cos(radians: Scalar) -> (Scalar, Scalar) {
    let (q, r) = reduce(f64::from(radians));
    let (sp, cp) = (poly_sin(r), poly_cos(r));
    let (sin, cos) = match q.rem_euclid(4) {
        0 => (sp, cp),
        1 => (cp, -sp),
        2 => (-sp, -cp),
        _ => (-cp, sp),
    };
    (sin as Scalar, cos as Scalar)
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

    /// Degrees to radians, byte-for-byte the expression `step.rs` uses.
    ///
    /// Reproduced rather than imported because the constant is private to the
    /// module that owns `angle_vectors`. Both are compile-time folded from the
    /// identical expression, so this is the same `f32`, and the sweeps below
    /// therefore feed [`sin_cos`] exactly the arguments the simulation does —
    /// not idealised radian values chosen for convenience.
    const DEG_TO_RAD: f32 = core::f32::consts::PI * 2.0 / 360.0;

    /// Distance between two `f32`s counted in representable values.
    ///
    /// Sign-magnitude is remapped to a monotone ordering first, so the count
    /// stays meaningful across zero.
    fn ulp_gap(a: f32, b: f32) -> u32 {
        let ordered = |v: f32| -> i64 {
            let bits = i64::from(v.to_bits() as i32);
            if bits < 0 {
                i64::from(i32::MIN) - bits
            } else {
                bits
            }
        };
        (ordered(a) - ordered(b)).unsigned_abs() as u32
    }

    /// Worst-case gap against the host's libm over a bit-pattern sweep of
    /// degree values, returned as `(sin, cos)`.
    ///
    /// Strided over `f32` bit patterns rather than over evenly spaced reals,
    /// because that is what samples every octave equally — a linear sweep
    /// spends almost all its samples in the largest octave and would barely
    /// probe small angles at all.
    fn worst_gap_over_degrees(from_bits: u32, to_bits_incl: u32, stride: u32) -> (u32, u32) {
        let mut worst = (0, 0);
        let mut bits = from_bits;
        while bits <= to_bits_incl {
            for magnitude in [f32::from_bits(bits), -f32::from_bits(bits)] {
                let radians = magnitude * DEG_TO_RAD;
                let (own_sin, own_cos) = sin_cos(radians);
                let (libm_sin, libm_cos) = radians.sin_cos();
                worst.0 = worst.0.max(ulp_gap(own_sin, libm_sin));
                worst.1 = worst.1.max(ulp_gap(own_cos, libm_cos));
            }
            bits += stride;
        }
        worst
    }

    /// Spec acceptance criterion 1: within 1 ULP of libm across a dense sweep.
    ///
    /// The domain stops at 8192° because that is where a prior exhaustive
    /// probe (`probes/dettrig-accuracy/`, 570,429,352 samples against a
    /// double-double reference) measured the first crossing above 1 ULP —
    /// cosine at the 8192° octave, sine one octave later. Sweeping past it
    /// would assert a bound this implementation is known not to meet, and
    /// deliberately does not need to: see [`sin_cos`]'s own docs, and note
    /// that view angles that large are unreachable once they are 16-bit at
    /// the command boundary.
    ///
    /// This compares against *the host's* libm, whichever that is. glibc and
    /// musl were both measured within 1 ULP of true here, so both satisfy it.
    #[test]
    fn sin_cos_is_within_one_ulp_of_libm_across_the_reachable_domain() {
        // 2^-10° up to 8192°. Below 2^-10, sin(x) == x to full f32 precision
        // for any competent implementation; the spot checks below cover it.
        let (sin_gap, cos_gap) = worst_gap_over_degrees(0x3a80_0000, 0x4600_0000, 503);
        assert!(
            sin_gap <= 1 && cos_gap <= 1,
            "sin_cos drifted from libm: worst sin gap {sin_gap} ulp, cos {cos_gap} ulp",
        );
    }

    /// The angles a player actually holds, swept every representable value.
    #[test]
    fn sin_cos_is_within_one_ulp_of_libm_over_one_full_turn() {
        // Every f32 degree magnitude in [256, 512) — a whole octave straddling
        // the 360° a view angle lives in, exhaustively, no stride.
        let (sin_gap, cos_gap) = worst_gap_over_degrees(0x4380_0000, 0x4400_0000, 1);
        assert!(
            sin_gap <= 1 && cos_gap <= 1,
            "sin_cos drifted from libm near one turn: sin {sin_gap} ulp, cos {cos_gap} ulp",
        );
    }

    #[test]
    fn sin_cos_reproduces_the_quadrant_identities() {
        let quarter = core::f32::consts::FRAC_PI_2;
        for turn in -8i32..=8 {
            let x = quarter * turn as f32;
            let (sin, cos) = sin_cos(x);
            let expected = match turn.rem_euclid(4) {
                0 => (0.0, 1.0),
                1 => (1.0, 0.0),
                2 => (0.0, -1.0),
                _ => (-1.0, 0.0),
            };
            // The argument itself is only the f32 nearest a multiple of π/2,
            // so the exact identity is not reachable; 1e-6 is far tighter than
            // any tolerance the movement tests use and still passes.
            assert!(
                (sin - expected.0).abs() < 1e-6 && (cos - expected.1).abs() < 1e-6,
                "quadrant {turn}: got {sin}, {cos}, wanted {expected:?}",
            );
        }
    }

    #[test]
    fn sin_cos_is_total_on_the_values_that_break_naive_reduction() {
        // A trapping float-to-int cast in the reduction would abort on these.
        // Rust's casts saturate, so each one lands somewhere defined and the
        // same somewhere on every target. Only NaN-ness is asserted, not which
        // NaN: the point is that nothing traps and nothing is platform-chosen.
        for x in [f32::INFINITY, f32::NEG_INFINITY, f32::NAN, 1e38, -1e38] {
            let (sin, cos) = sin_cos(x);
            assert!(
                sin.is_nan() == cos.is_nan() || (sin.abs() <= 1.0 && cos.abs() <= 1.0),
                "sin_cos({x}) produced an inconsistent pair: {sin}, {cos}",
            );
        }
        assert_eq!(to_bits(sin_cos(0.0).0), to_bits(0.0));
        assert_eq!(sin_cos(0.0).1, 1.0);
    }

    #[test]
    fn sin_cos_matches_libm_on_tiny_and_subnormal_arguments() {
        for x in [
            f32::MIN_POSITIVE,
            f32::from_bits(1),
            1e-20,
            1e-10,
            -1e-10,
            1e-4,
        ] {
            let (own_sin, own_cos) = sin_cos(x);
            let (libm_sin, libm_cos) = x.sin_cos();
            assert!(
                ulp_gap(own_sin, libm_sin) <= 1 && ulp_gap(own_cos, libm_cos) <= 1,
                "tiny argument {x:e}: got {own_sin:e}/{own_cos}, libm {libm_sin:e}/{libm_cos}",
            );
        }
    }
}
