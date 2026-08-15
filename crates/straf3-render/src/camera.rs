//! Where the eye is, and how the world is projected onto the screen.
//!
//! # The camera is downstream of the simulation, never upstream
//!
//! Everything here is a pure function of two [`PlayerState`]s and an
//! interpolation factor. It reads the simulation; it never writes to it, and
//! it never advances it — spec D2. That is what lets the frame rate be
//! whatever the display feels like while the simulation stays on its fixed
//! 8 ms cadence (criterion 5): a frame between two commands is *drawn*
//! between them, not *simulated* between them.

use glam::Mat4;
use straf3_sim::PlayerState;
use straf3_sim::num::{Scalar, Vec3, s, vec3};

use crate::InterpolationAlpha;
use crate::arena::{EYE_HEIGHT, EYE_HEIGHT_CROUCHED};

/// Degrees to radians, as Q3's `AngleVectors` spells it.
const DEG_TO_RAD: Scalar = s(core::f32::consts::PI * 2.0 / 360.0);

/// Near plane. Q3's, and it matters: a near plane much further out clips the
/// player's own hull when they stand against a wall.
const Z_NEAR: Scalar = s(4.0);

/// Far plane. The arena's diagonal is about 4700 units; this clears it with
/// room to spare so nothing pops.
const Z_FAR: Scalar = s(16384.0);

/// Default horizontal field of view, in degrees — Q3's `cg_fov` default.
pub const DEFAULT_FOV_X: Scalar = s(90.0);

/// A first-person camera in Quake's Z-up world.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Camera {
    /// Eye position in world space.
    pub eye: Vec3,
    /// Look up/down, degrees. **Negative is up**, as in Quake.
    pub pitch: Scalar,
    /// Look left/right, degrees. 0 is +X, 90 is +Y.
    pub yaw: Scalar,
    /// Horizontal field of view, degrees.
    pub fov_x: Scalar,
}

impl Camera {
    /// The camera for a frame sitting `alpha` of the way from `prev` to `curr`.
    ///
    /// Both the eye position and the view angles are interpolated. Interpolating
    /// the angles matters more than it looks: view angles arrive at 125 Hz from
    /// the command stream, and a 240 Hz display showing each of them twice is
    /// visible as mouse judder even though the simulation is perfectly smooth.
    #[must_use]
    pub fn between(prev: &PlayerState, curr: &PlayerState, alpha: InterpolationAlpha) -> Self {
        let t = s(alpha.0).clamp(s(0.0), s(1.0));
        let origin = prev.origin + (curr.origin - prev.origin) * t;
        // Eye height follows the state the frame is closest to. Lerping it
        // instead would make standing up look like a lift rather than a snap,
        // and the simulation's hull changes in one step, so a smooth eye would
        // be a lie about where the player is.
        let crouched = if t < s(0.5) {
            prev.crouched
        } else {
            curr.crouched
        };
        let eye_height = if crouched {
            EYE_HEIGHT_CROUCHED
        } else {
            EYE_HEIGHT
        };

        Self {
            eye: origin + vec3(s(0.0), s(0.0), eye_height),
            pitch: lerp_angle(prev.view.pitch, curr.view.pitch, t),
            yaw: lerp_angle(prev.view.yaw, curr.view.yaw, t),
            fov_x: DEFAULT_FOV_X,
        }
    }

    /// Unit vector the camera is looking along.
    ///
    /// The same expression as `straf3-sim`'s `angle_vectors`, deliberately: if
    /// the camera and the movement disagreed about what "forward" means, every
    /// strafe angle would be a lie and the game would be unplayable in exactly
    /// the way that is hardest to diagnose.
    #[must_use]
    pub fn forward(&self) -> Vec3 {
        let (sy, cy) = (self.yaw * DEG_TO_RAD).sin_cos();
        let (sp, cp) = (self.pitch * DEG_TO_RAD).sin_cos();
        vec3(cp * cy, cp * sy, -sp)
    }

    /// World-to-clip matrix for a viewport of the given aspect ratio.
    #[must_use]
    pub fn view_proj(&self, aspect: Scalar) -> Mat4 {
        let aspect = if aspect.is_finite() && aspect > s(0.0) {
            aspect
        } else {
            s(1.0)
        };
        // Q3 quotes FOV horizontally; a projection matrix wants it vertically,
        // so wider windows show more of the world rather than stretching it.
        let fov_y = s(2.0) * ((self.fov_x * s(0.5) * DEG_TO_RAD).tan() / aspect).atan();
        // `directx` is glam's name for the Z-in-0..1, Y-up NDC convention,
        // which is also WebGPU's — and so wgpu's, on every backend.
        let proj = glam::camera::rh::proj::directx::perspective(fov_y, aspect, Z_NEAR, Z_FAR);
        // Z is up. This takes the look *direction*, not a target, so looking
        // straight up does not degenerate into a zero-length vector.
        let view = glam::camera::rh::view::look_to_mat4(self.eye, self.forward(), Vec3::Z);
        proj * view
    }
}

/// Interpolate two angles in degrees the short way round.
///
/// Yaw is unbounded and wraps: a player spinning past 360 would otherwise make
/// the camera whip the long way round the circle for one frame.
fn lerp_angle(from: Scalar, to: Scalar, t: Scalar) -> Scalar {
    let mut delta = (to - from) % s(360.0);
    if delta > s(180.0) {
        delta -= s(360.0);
    } else if delta < s(-180.0) {
        delta += s(360.0);
    }
    from + delta * t
}

#[cfg(test)]
mod tests {
    use super::*;
    use straf3_sim::ViewAngles;

    fn player(origin: Vec3, pitch: Scalar, yaw: Scalar) -> PlayerState {
        PlayerState {
            origin,
            view: ViewAngles {
                pitch,
                yaw,
                roll: s(0.0),
            },
            ..PlayerState::default()
        }
    }

    #[test]
    fn forward_matches_the_simulations_own_angle_convention() {
        // Yaw 90 is +Y, and that is the whole reason SPAWN_YAW is 90.
        let c = Camera {
            eye: Vec3::ZERO,
            pitch: s(0.0),
            yaw: s(90.0),
            fov_x: DEFAULT_FOV_X,
        };
        let f = c.forward();
        assert!(f.y > s(0.99), "yaw 90 must look along +Y, got {f:?}");

        // Negative pitch is up, as in Quake.
        let up = Camera {
            pitch: s(-90.0),
            ..c
        };
        assert!(up.forward().z > s(0.99), "negative pitch must look up");
    }

    #[test]
    fn a_half_frame_sits_halfway_between_two_states() {
        let a = player(vec3(s(0.0), s(0.0), s(24.0)), s(0.0), s(0.0));
        let b = player(vec3(s(100.0), s(0.0), s(24.0)), s(0.0), s(0.0));
        let mid = Camera::between(&a, &b, InterpolationAlpha(0.5));
        assert_eq!(mid.eye.x, s(50.0));
        assert_eq!(mid.eye.z, s(24.0) + EYE_HEIGHT);
    }

    #[test]
    fn yaw_wraps_the_short_way_round() {
        let a = player(Vec3::ZERO, s(0.0), s(350.0));
        let b = player(Vec3::ZERO, s(0.0), s(10.0));
        let mid = Camera::between(&a, &b, InterpolationAlpha(0.5));
        // The short way is through 360, not backwards through 180.
        assert_eq!(mid.yaw, s(360.0), "spun the long way: {}", mid.yaw);
        assert!(mid.forward().x > s(0.98));
    }

    #[test]
    fn alpha_outside_the_unit_range_cannot_throw_the_camera_past_the_state() {
        let a = player(Vec3::ZERO, s(0.0), s(0.0));
        let b = player(vec3(s(100.0), s(0.0), s(0.0)), s(0.0), s(0.0));
        // The accumulator should never produce these, but a camera that
        // extrapolates on a hitch would put the eye inside a wall.
        assert_eq!(Camera::between(&a, &b, InterpolationAlpha(4.0)).eye.x, s(100.0));
        assert_eq!(Camera::between(&a, &b, InterpolationAlpha(-2.0)).eye.x, s(0.0));
    }

    #[test]
    fn crouching_lowers_the_eye() {
        let mut c = player(Vec3::ZERO, s(0.0), s(0.0));
        let stand = Camera::between(&c, &c, InterpolationAlpha(1.0)).eye.z;
        c.crouched = true;
        let duck = Camera::between(&c, &c, InterpolationAlpha(1.0)).eye.z;
        assert!(duck < stand);
        assert_eq!(stand - duck, EYE_HEIGHT - EYE_HEIGHT_CROUCHED);
    }

    #[test]
    fn a_point_ahead_projects_into_the_middle_of_the_screen() {
        let c = Camera {
            eye: Vec3::ZERO,
            pitch: s(0.0),
            yaw: s(0.0),
            fov_x: DEFAULT_FOV_X,
        };
        let m = c.view_proj(s(16.0) / s(9.0));
        let p = m * glam::Vec4::new(s(1000.0), s(0.0), s(0.0), s(1.0));
        let ndc = p.truncate() / p.w;
        assert!(ndc.x.abs() < s(1e-5) && ndc.y.abs() < s(1e-5), "{ndc:?}");
        assert!(
            (s(0.0)..=s(1.0)).contains(&ndc.z),
            "depth must land in wgpu's 0..1 clip range, got {}",
            ndc.z
        );
    }

    #[test]
    fn a_degenerate_aspect_ratio_does_not_produce_a_nan_matrix() {
        // A minimised window reports a zero-sized surface on some platforms.
        let c = Camera {
            eye: Vec3::ZERO,
            pitch: s(0.0),
            yaw: s(0.0),
            fov_x: DEFAULT_FOV_X,
        };
        for aspect in [s(0.0), Scalar::NAN, Scalar::INFINITY] {
            assert!(
                c.view_proj(aspect).is_finite(),
                "aspect {aspect} produced a non-finite matrix"
            );
        }
    }
}
