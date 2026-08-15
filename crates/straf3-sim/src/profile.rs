//! Movement constants, as data.

use crate::num::{Scalar, Vec3, s, vec3};

/// Every number the movement code is allowed to have an opinion about.
///
/// # Why this is a plain struct and not a trait or an enum
///
/// Spec D1 ships both VQ3 and CPM. The tempting shape is a trait with two
/// implementors, or an enum the physics matches on. Both are wrong here:
///
/// - **The two profiles differ in constants, not in structure.** Where CPM
///   genuinely adds behaviour — air control, air-stop acceleration, double
///   jumps — the honest encoding is a parameter that VQ3 sets to zero or
///   `false`, because that *is* the relationship between them. One code path
///   with values that switch it off means VQ3 and CPM cannot drift into two
///   separately-maintained implementations of strafejumping.
/// - **These numbers will be tuned.** The VQ3 values are verified against id's
///   GPL source; the CPM values are community-reconstructed and will need
///   adjusting against reference demos. Data can be edited, serialised into a
///   replay, diffed and A/B tested. Behaviour compiled into a match arm cannot.
/// - **Recording it makes replays honest.** A run is only reproducible if the
///   constants it ran under are known. As data, the profile can be written
///   into a recording alongside the tick rate.
///
/// A field here is a promise that the value is genuinely a number the
/// simulation reads, not a switch that selects a different algorithm. If a
/// future behaviour cannot be expressed that way, that is worth an argument
/// before it is worth a `bool`.
///
/// # Verification status
///
/// Values marked *verified* come from id Software's Quake 3 GPL release.
/// Values marked `TODO` are placeholders whose confirmation is Wave 2's job;
/// each says what needs checking. Nothing here should be trusted for feel
/// until that pass is done.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PhysicsProfile {
    // ── ground movement ────────────────────────────────────────────────
    /// Ground acceleration (`pm_accelerate`). Verified: 10.
    pub accelerate: Scalar,
    /// Ground friction (`pm_friction`). Verified: 6.
    pub friction: Scalar,
    /// Speed below which friction is applied as if the player were moving at
    /// this speed (`pm_stopspeed`) — this is what makes stopping crisp rather
    /// than asymptotic. Verified: 100.
    pub stop_speed: Scalar,
    /// Maximum ground speed under player control (`pm_speed`).
    ///
    /// TODO(wave2): verify against `bg_pmove.c` — expected 320, but it was not
    /// in the verified constant set handed to Wave 1.
    pub max_speed: Scalar,
    /// Fraction of `max_speed` available while crouched (`pm_duckScale`).
    /// Verified: 0.25.
    pub duck_scale: Scalar,

    // ── air movement ───────────────────────────────────────────────────
    /// Air acceleration (`pm_airaccelerate`). Verified: 1 for VQ3.
    ///
    /// This single number is why strafejumping exists: acceleration is applied
    /// along the wish direction but capped by the projection of current
    /// velocity onto it, so turning while accelerating gains speed.
    pub air_accelerate: Scalar,
    /// Gravity, in units per second squared. Verified: 800.
    pub gravity: Scalar,
    /// Upward velocity applied by a jump (`JUMP_VELOCITY`). Verified: 270.
    pub jump_velocity: Scalar,

    // ── collision response ─────────────────────────────────────────────
    /// How far the player can step up without jumping (`STEPSIZE`).
    /// Verified: 18.
    pub step_height: Scalar,
    /// Velocity is clipped to slightly *beyond* a plane rather than exactly
    /// onto it (`OVERCLIP`). Verified: 1.001.
    ///
    /// This is not a fudge factor to be cleaned up: the excess is what pushes
    /// the player off surfaces they are pressed into, and it is the direct
    /// cause of overbounce and ramp boost behaviour.
    pub overclip: Scalar,
    /// How many planes the slide solver will consider before giving up
    /// (`MAX_CLIP_PLANES`). Verified: 5.
    pub max_clip_planes: u8,
    /// How far below the hull to probe when testing for ground contact.
    /// Verified: 0.25.
    pub ground_trace_probe: Scalar,
    /// Steepest ground the player can stand on, as the minimum Z component of
    /// the surface normal (`MIN_WALK_NORMAL`).
    ///
    /// TODO(wave2): verify against `bg_pmove.c` — expected 0.7.
    pub min_walk_normal: Scalar,

    // ── player hull ────────────────────────────────────────────────────
    /// Corner of the standing hull nearest the origin.
    ///
    /// TODO(wave2): verify against `bg_public.h` — expected (-15, -15, -24).
    pub hull_mins: Vec3,
    /// Far corner of the standing hull.
    ///
    /// TODO(wave2): verify against `bg_public.h` — expected (15, 15, 32).
    pub hull_maxs: Vec3,
    /// Height of the crouched hull's top, replacing `hull_maxs.z`.
    ///
    /// TODO(wave2): verify against `bg_pmove.c` — expected 16.
    pub crouched_height: Scalar,

    // ── CPM extensions (spec D1) ───────────────────────────────────────
    //
    // VQ3 sets these to zero or false. They are fields rather than a separate
    // profile type precisely so that "VQ3 is CPM with air control off" is
    // expressible, and so the two share one implementation of everything else.
    /// Strength of forward/back air control (`cpm_aircontrol`). Zero disables
    /// it, which is VQ3.
    ///
    /// TODO(wave2): community-reconstructed, not from id source. Expected
    /// around 150 for CPM; verify against a CPMA reference demo.
    pub air_control: Scalar,
    /// Acceleration applied when air-strafing with no forward input
    /// (`cpm_airstopaccelerate`). Zero disables it.
    ///
    /// TODO(wave2): community-reconstructed. Expected around 2.5 for CPM.
    pub air_stop_accelerate: Scalar,
    /// Acceleration used while air-strafing (`cpm_strafeaccelerate`), distinct
    /// from [`Self::air_accelerate`]. Zero means "use `air_accelerate`".
    ///
    /// TODO(wave2): community-reconstructed. Expected around 70 for CPM.
    pub strafe_accelerate: Scalar,
    /// Wish speed cap applied while air control is active.
    ///
    /// TODO(wave2): community-reconstructed. Expected around 30 for CPM.
    pub air_control_wish_speed_cap: Scalar,
    /// Whether a second jump shortly after landing gains extra height.
    pub double_jump_enabled: bool,
    /// How long after landing a double jump remains available, in
    /// milliseconds — an integer, because every timer in the simulation is
    /// (see [`crate::UserCmd::duration_ms`]).
    ///
    /// TODO(wave2): community-reconstructed. Expected around 400 ms for CPM.
    pub double_jump_window_ms: u16,
    /// Extra upward velocity a double jump adds on top of
    /// [`Self::jump_velocity`].
    ///
    /// TODO(wave2): community-reconstructed. Expected around 100 for CPM.
    pub double_jump_boost: Scalar,
}

impl PhysicsProfile {
    /// Vanilla Quake 3 physics.
    ///
    /// Minimal air acceleration, no air control, no double jump: speed comes
    /// almost entirely from the strafejump turn-rate technique.
    #[must_use]
    pub const fn vq3() -> Self {
        Self {
            // Verified against id's GPL source.
            accelerate: s(10.0),
            friction: s(6.0),
            stop_speed: s(100.0),
            max_speed: s(320.0), // TODO(wave2): verify
            duck_scale: s(0.25),
            air_accelerate: s(1.0),
            gravity: s(800.0),
            jump_velocity: s(270.0),
            step_height: s(18.0),
            overclip: s(1.001),
            max_clip_planes: 5,
            ground_trace_probe: s(0.25),
            min_walk_normal: s(0.7), // TODO(wave2): verify

            // TODO(wave2): verify the hull against bg_public.h.
            hull_mins: vec3(s(-15.0), s(-15.0), s(-24.0)),
            hull_maxs: vec3(s(15.0), s(15.0), s(32.0)),
            crouched_height: s(16.0),

            // VQ3 is CPM with the extensions switched off. This is the whole
            // reason they are data.
            air_control: s(0.0),
            air_stop_accelerate: s(0.0),
            strafe_accelerate: s(0.0),
            air_control_wish_speed_cap: s(0.0),
            double_jump_enabled: false,
            double_jump_window_ms: 0,
            double_jump_boost: s(0.0),
        }
    }

    /// Challenge ProMode physics — the default (spec D1).
    ///
    /// Adds forward/back air control, air-stop acceleration and double jumps
    /// on top of the VQ3 base. Every value that differs from [`Self::vq3`] is
    /// community-reconstructed rather than taken from id source, and is
    /// therefore a `TODO` for Wave 2 to check against reference demos.
    #[must_use]
    pub const fn cpm() -> Self {
        Self {
            // TODO(wave2): every value in this block is community-
            // reconstructed. Verify against CPMA and reference demos before
            // anyone tunes movement feel against them.
            air_control: s(150.0),
            air_stop_accelerate: s(2.5),
            strafe_accelerate: s(70.0),
            air_control_wish_speed_cap: s(30.0),
            double_jump_enabled: true,
            double_jump_window_ms: 400,
            double_jump_boost: s(100.0),
            ..Self::vq3()
        }
    }

    /// Half the extents of the standing hull.
    #[must_use]
    pub fn hull_half_extents(&self) -> Vec3 {
        (self.hull_maxs - self.hull_mins) * s(0.5)
    }

    /// Offset from the player origin to the centre of the standing hull.
    ///
    /// Quake's hull is not centred on the origin (mins.z is -24, maxs.z is
    /// 32), so this is not zero: the box centre sits 4 units above the origin.
    #[must_use]
    pub fn hull_center_offset(&self) -> Vec3 {
        (self.hull_maxs + self.hull_mins) * s(0.5)
    }
}

impl Default for PhysicsProfile {
    /// CPM, per spec D1: the default is the higher skill ceiling.
    fn default() -> Self {
        Self::cpm()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_profile_is_cpm_per_d1() {
        assert_eq!(PhysicsProfile::default(), PhysicsProfile::cpm());
    }

    #[test]
    fn vq3_is_cpm_with_the_extensions_switched_off() {
        let vq3 = PhysicsProfile::vq3();
        assert_eq!(vq3.air_control, s(0.0));
        assert_eq!(vq3.air_stop_accelerate, s(0.0));
        assert!(!vq3.double_jump_enabled);

        // The shared base really is shared: the profiles differ only in the
        // CPM extension fields, so there is one set of movement constants
        // being maintained, not two.
        let as_vq3 = PhysicsProfile {
            air_control: s(0.0),
            air_stop_accelerate: s(0.0),
            strafe_accelerate: s(0.0),
            air_control_wish_speed_cap: s(0.0),
            double_jump_enabled: false,
            double_jump_window_ms: 0,
            double_jump_boost: s(0.0),
            ..PhysicsProfile::cpm()
        };
        assert_eq!(as_vq3, vq3);
    }

    #[test]
    fn verified_constants_match_the_gpl_source() {
        let p = PhysicsProfile::vq3();
        assert_eq!(p.accelerate, s(10.0));
        assert_eq!(p.air_accelerate, s(1.0));
        assert_eq!(p.friction, s(6.0));
        assert_eq!(p.stop_speed, s(100.0));
        assert_eq!(p.duck_scale, s(0.25));
        assert_eq!(p.jump_velocity, s(270.0));
        assert_eq!(p.gravity, s(800.0));
        assert_eq!(p.step_height, s(18.0));
        assert_eq!(p.overclip, s(1.001));
        assert_eq!(p.max_clip_planes, 5);
        assert_eq!(p.ground_trace_probe, s(0.25));
    }

    #[test]
    fn hull_geometry_is_derived_consistently() {
        let p = PhysicsProfile::vq3();
        assert_eq!(p.hull_half_extents(), vec3(s(15.0), s(15.0), s(28.0)));
        // The hull is not centred on the origin: (-24 + 32) / 2 = 4.
        assert_eq!(p.hull_center_offset(), vec3(s(0.0), s(0.0), s(4.0)));
    }
}
