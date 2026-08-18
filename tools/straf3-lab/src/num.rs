//! Arithmetic the lab needs that the simulation does not provide, written so
//! that the lab's own output is machine-independent.
//!
//! # Why this module exists at all
//!
//! `straf3-sim` owns its trigonometry (`num::sin_cos`) precisely so that a
//! recorded run means the same thing on glibc, musl, Windows and wasm — the
//! measured gap was 1 ulp on 1.3% of angles, enough to corrupt a run after
//! about 14 seconds of play. The lab has the same problem for the same reason:
//! its published numbers are only reproducible if every arithmetic step that
//! produces them is fixed by IEEE 754 rather than by whichever libm the host
//! links.
//!
//! The simulation happens not to need an arctangent. The lab does: the strafe
//! technique is defined as "hold the view a fixed angle off the *current
//! velocity*", so measuring it means recovering a heading from a velocity
//! vector on every command. `f32::atan2` would put a libm call inside the
//! measurement loop, and the answer feeds straight back into the next
//! command's view angle — so a 1-ulp disagreement does not stay 1 ulp, it
//! becomes a different quantised view angle and then a different run.
//!
//! So [`atan2_degrees`] is written here in terms of `+ - * /` and comparison
//! only. Accuracy is not the goal — a single fixed answer is. See
//! [`straf3_sim::num::sin_cos`], which makes the same argument at greater
//! length.

use straf3_sim::num::{Scalar, Vec3};

/// Radians per degree, as `f64`.
const DEG_PER_RAD: f64 = 180.0 / core::f64::consts::PI;

/// `tan(π/8)`. Above this the argument is folded by the half-angle identity so
/// the series below only ever sees `|u| ≤ 0.293`.
const TAN_PI_8: f64 = 0.414_213_562_373_095_1;

/// `atan(u)` for `|u| ≤ 0.293`, as its Taylor series to `u²⁵`.
///
/// The tail after that term is below `0.293²⁷/27 ≈ 1.4e-16`, i.e. under one
/// `f64` ulp of the result, so extending it buys nothing. Horner form, because
/// the order of operations is part of the answer.
fn atan_series(u: f64) -> f64 {
    let u2 = u * u;
    // u - u³/3 + u⁵/5 - … + u²⁵/25
    u * (1.0
        + u2 * (-1.0 / 3.0
            + u2 * (1.0 / 5.0
                + u2 * (-1.0 / 7.0
                    + u2 * (1.0 / 9.0
                        + u2 * (-1.0 / 11.0
                            + u2 * (1.0 / 13.0
                                + u2 * (-1.0 / 15.0
                                    + u2 * (1.0 / 17.0
                                        + u2 * (-1.0 / 19.0
                                            + u2 * (1.0 / 21.0
                                                + u2 * (-1.0 / 23.0 + u2 * (1.0 / 25.0)))))))))))))
}

/// `atan(z)` for `0 ≤ z ≤ 1`.
fn atan_unit(z: f64) -> f64 {
    if z > TAN_PI_8 {
        // atan(z) = π/4 + atan((z−1)/(z+1)). For z in (tan π/8, 1] the folded
        // argument lands in (−0.2929, 0], which is where the series is fast.
        core::f64::consts::FRAC_PI_4 + atan_series((z - 1.0) / (z + 1.0))
    } else {
        atan_series(z)
    }
}

/// The angle of the vector `(x, y)` in degrees, in `(-180, 180]`.
///
/// Same convention as `f32::atan2` and therefore the same convention the
/// existing movement tests use, but computed from IEEE primitives alone. Zero
/// for the zero vector, which is a choice rather than a fact: a player who is
/// not moving has no heading, and the callers here want a number rather than a
/// branch.
#[must_use]
pub fn atan2_degrees(y: Scalar, x: Scalar) -> Scalar {
    let (y, x) = (f64::from(y), f64::from(x));
    let (ay, ax) = (y.abs(), x.abs());

    let mut r = if ay == 0.0 && ax == 0.0 {
        0.0
    } else if ax >= ay {
        atan_unit(ay / ax)
    } else {
        core::f64::consts::FRAC_PI_2 - atan_unit(ax / ay)
    };
    if x < 0.0 {
        r = core::f64::consts::PI - r;
    }
    if y < 0.0 {
        r = -r;
    }
    (r * DEG_PER_RAD) as Scalar
}

/// The direction the player is travelling, in degrees.
#[must_use]
pub fn heading_degrees(velocity: Vec3) -> Scalar {
    atan2_degrees(velocity.y, velocity.x)
}

/// Horizontal speed, in units per second — the number every technique is
/// judged on.
///
/// Deliberately not `velocity.length()`: vertical speed is gravity's, not the
/// player's, and folding it in would report a falling player as fast.
#[must_use]
pub fn horizontal_speed(velocity: Vec3) -> Scalar {
    (velocity.x * velocity.x + velocity.y * velocity.y).sqrt()
}

/// `cos` of an angle in degrees, through the simulation's own owned
/// trigonometry so that closed-form comparisons in the report are computed the
/// same way everywhere.
#[must_use]
pub fn cos_degrees(degrees: Scalar) -> Scalar {
    straf3_sim::num::sin_cos(degrees * (core::f32::consts::PI * 2.0 / 360.0)).1
}

/// `sin` of an angle in degrees. See [`cos_degrees`].
#[must_use]
pub fn sin_degrees(degrees: Scalar) -> Scalar {
    straf3_sim::num::sin_cos(degrees * (core::f32::consts::PI * 2.0 / 360.0)).0
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The bound that matters: the lab's arctangent must agree with the host's
    /// libm far more closely than the 0.0055° a 16-bit view angle can express,
    /// or a measurement would depend on which reduction ran.
    #[test]
    fn atan2_agrees_with_libm_far_inside_one_view_angle_quantum() {
        let quantum = 360.0f32 / 65_536.0;
        let mut worst = 0.0f32;
        for i in -720..=720 {
            let a = i as f32 * 0.25;
            let (y, x) = (a.to_radians().sin(), a.to_radians().cos());
            for scale in [1.0f32, 0.001, 1000.0] {
                let ours = atan2_degrees(y * scale, x * scale);
                let theirs = (y * scale).atan2(x * scale).to_degrees();
                // Both live in (−180, 180], so the only wrap to forgive is the
                // ±180 seam itself.
                let d = (ours - theirs).abs();
                let d = if d > 180.0 { (d - 360.0).abs() } else { d };
                worst = worst.max(d);
            }
        }
        assert!(
            worst < quantum / 100.0,
            "own atan2 drifted {worst} degrees from libm; one view-angle quantum is {quantum}"
        );
    }

    #[test]
    fn atan2_puts_the_cardinal_directions_where_they_belong() {
        assert_eq!(atan2_degrees(0.0, 1.0), 0.0);
        assert!((atan2_degrees(1.0, 0.0) - 90.0).abs() < 1e-4);
        assert!((atan2_degrees(0.0, -1.0).abs() - 180.0).abs() < 1e-4);
        assert!((atan2_degrees(-1.0, 0.0) + 90.0).abs() < 1e-4);
        assert!((atan2_degrees(1.0, 1.0) - 45.0).abs() < 1e-4);
        assert!((atan2_degrees(-1.0, -1.0) + 135.0).abs() < 1e-4);
        // No heading for no motion, rather than a NaN or a panic.
        assert_eq!(atan2_degrees(0.0, 0.0), 0.0);
    }
}
