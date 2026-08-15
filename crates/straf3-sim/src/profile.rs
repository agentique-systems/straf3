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
///   jumps — the honest encoding is a parameter that VQ3 sets to zero, because
///   that *is* the relationship between them. One code path with values that
///   switch it off means VQ3 and CPM cannot drift into two
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
    /// Maximum speed under player control — Q3's `pm->ps->speed`, fed from the
    /// `g_speed` cvar whose default is 320.
    ///
    /// Not a `bg_pmove.c` literal: it is a server setting, which is exactly why
    /// it belongs here as data. It is the numerator of
    /// `PM_CmdScale`, so it scales *wish* speed, not the speed the player can
    /// reach — strafejumping exceeds it by design, because [`Self::accelerate`]
    /// clamps the projection of velocity onto the wish direction and nothing
    /// clamps the total.
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
    /// the surface normal (`MIN_WALK_NORMAL`). Verified: 0.7.
    ///
    /// A plane below this is still *touched* — velocity is clipped to it — but
    /// the player does not count as walking, so no ground friction is applied.
    /// That difference is the whole of ramp sliding: on a steep ramp only
    /// gravity bleeds speed. See [`crate::GroundState::Sliding`].
    pub min_walk_normal: Scalar,

    // ── player hull ────────────────────────────────────────────────────
    /// Corner of the standing hull nearest the origin. Verified: (-15, -15, -24).
    pub hull_mins: Vec3,
    /// Far corner of the standing hull. Verified: (15, 15, 32).
    pub hull_maxs: Vec3,
    /// Height of the crouched hull's top, replacing `hull_maxs.z`.
    /// Verified: 16 (`PM_CheckDuck`).
    pub crouched_height: Scalar,

    // ── CPM extensions (spec D1) ───────────────────────────────────────
    //
    // VQ3 sets these to zero. They are fields rather than a separate profile
    // type precisely so that "VQ3 is CPM with air control off" is expressible,
    // and so the two share one implementation of everything else.
    /// Strength of air control (`pm_aircontrol`), applied only when the player
    /// is holding forward or back with **no** strafe input. Zero disables it,
    /// which is VQ3.
    ///
    /// It steers the existing velocity vector towards the wish direction
    /// without changing its magnitude: `k = 32 * air_control * dot² * dt`,
    /// where `dot` is between the normalised horizontal velocity and the wish
    /// direction. Turning is therefore free of speed cost, which is why CPM
    /// air movement feels like flying rather than like sliding.
    ///
    /// TODO(wave2): community-reconstructed, not from id source. 150 is the
    /// value every community port carries; verify against a CPMA demo.
    pub air_control: Scalar,
    /// Acceleration used in the air when the wish direction opposes the
    /// current velocity (`pm_airstopaccelerate`) — the brake that makes CPM's
    /// mid-air direction changes sharp. Zero means "use
    /// [`Self::air_accelerate`]", which is VQ3.
    ///
    /// TODO(wave2): community-reconstructed. Expected 2.5 for CPM.
    pub air_stop_accelerate: Scalar,
    /// Acceleration used in the air while holding **pure strafe** — left or
    /// right with no forward or back (`pm_strafeaccelerate`). Zero means "use
    /// [`Self::air_accelerate`]", which is VQ3.
    ///
    /// This is the other half of CPM's air movement: a very large acceleration
    /// against a very small wish speed ([`Self::strafe_wish_speed_cap`]). The
    /// product is what sets how fast a strafe turn converts angle into speed.
    ///
    /// TODO(wave2): community-reconstructed. Expected 70 for CPM.
    pub strafe_accelerate: Scalar,
    /// Wish speed ceiling applied while [`Self::strafe_accelerate`] is in
    /// effect (`pm_wishspeed`), replacing the usual `max_speed * cmd_scale`.
    ///
    /// Because [`PhysicsProfile::accelerate`]-style acceleration only clamps
    /// the *projection* of velocity onto the wish direction, a low wish speed
    /// does not cap the player's speed — it caps how much of the wish
    /// direction's contribution counts as "already achieved", which is what
    /// keeps a strafe turn productive at 900 ups.
    ///
    /// TODO(wave2): community-reconstructed. Expected 30 for CPM.
    pub strafe_wish_speed_cap: Scalar,
    /// How long after landing *from a jump* a second jump still gains extra
    /// height, in milliseconds — an integer, because every timer in the
    /// simulation is (see [`crate::UserCmd::duration_ms`]).
    ///
    /// The window opens on landing and only if the player left the ground by
    /// jumping: walking off a ledge and jumping on contact is not a double
    /// jump. See [`crate::PlayerState::left_ground_by_jumping`].
    ///
    /// Zero disables double jumping, which is VQ3. There is deliberately no
    /// separate `double_jump_enabled` flag: a zero-length window already means
    /// "never available", and a bool alongside it would be a second source of
    /// truth the movement code could read instead of — or inconsistently with
    /// — this value.
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
            max_speed: s(320.0), // the `g_speed` cvar default
            duck_scale: s(0.25),
            air_accelerate: s(1.0),
            gravity: s(800.0),
            jump_velocity: s(270.0),
            step_height: s(18.0),
            overclip: s(1.001),
            max_clip_planes: 5,
            ground_trace_probe: s(0.25),
            min_walk_normal: s(0.7),

            hull_mins: vec3(s(-15.0), s(-15.0), s(-24.0)),
            hull_maxs: vec3(s(15.0), s(15.0), s(32.0)),
            crouched_height: s(16.0),

            // VQ3 is CPM with the extensions switched off. This is the whole
            // reason they are data.
            air_control: s(0.0),
            air_stop_accelerate: s(0.0),
            strafe_accelerate: s(0.0),
            strafe_wish_speed_cap: s(0.0),
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
            //
            // CPM raises ground acceleration too. This is the one value here
            // that overrides a *verified* VQ3 constant rather than switching on
            // an extension, so it is called out rather than left to be noticed:
            // `vq3()` keeps id's 10, and criterion 1 is asserted against that.
            accelerate: s(15.0),
            air_control: s(150.0),
            air_stop_accelerate: s(2.5),
            strafe_accelerate: s(70.0),
            strafe_wish_speed_cap: s(30.0),
            double_jump_window_ms: 400,
            double_jump_boost: s(100.0),
            ..Self::vq3()
        }
    }

    /// The player's collision box, standing or crouched.
    ///
    /// Crouching lowers the top of the hull to [`Self::crouched_height`] and
    /// leaves the underside where it is, exactly as `PM_CheckDuck` does — which
    /// is why crouching does not lift the player off the floor, and why
    /// standing up again can be blocked by a low ceiling.
    #[must_use]
    pub fn hull(&self, crouched: bool) -> Hull {
        let mut maxs = self.hull_maxs;
        if crouched {
            maxs.z = self.crouched_height;
        }
        Hull {
            half_extents: (maxs - self.hull_mins) * s(0.5),
            center_offset: (maxs + self.hull_mins) * s(0.5),
        }
    }

    /// Half the extents of the standing hull.
    #[must_use]
    pub fn hull_half_extents(&self) -> Vec3 {
        self.hull(false).half_extents
    }

    /// Offset from the player origin to the centre of the standing hull.
    ///
    /// Quake's hull is not centred on the origin (mins.z is -24, maxs.z is
    /// 32), so this is not zero: the box centre sits 4 units above the origin.
    #[must_use]
    pub fn hull_center_offset(&self) -> Vec3 {
        self.hull(false).center_offset
    }
}

/// The player's collision box, in the form [`crate::Sweep`] wants it.
///
/// A pair rather than mins/maxs because that is the shape the collision seam
/// asks for, and converting once here is better than every trace doing it.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Hull {
    /// Half the box's size on each axis.
    pub half_extents: Vec3,
    /// Origin-relative offset of the box's centre.
    pub center_offset: Vec3,
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
        // A zero-length window is how "no double jump" is spelled; there is no
        // separate flag to keep in sync with it.
        assert_eq!(vq3.double_jump_window_ms, 0);

        // The shared base really is shared: the profiles differ only in the
        // CPM extension fields and in ground acceleration, so there is one set
        // of movement constants being maintained, not two. `accelerate` is
        // listed here rather than hidden: it is the one verified VQ3 constant
        // CPM overrides, and that has to be visible in a test, not a comment.
        let as_vq3 = PhysicsProfile {
            accelerate: s(10.0),
            air_control: s(0.0),
            air_stop_accelerate: s(0.0),
            strafe_accelerate: s(0.0),
            strafe_wish_speed_cap: s(0.0),
            double_jump_window_ms: 0,
            double_jump_boost: s(0.0),
            ..PhysicsProfile::cpm()
        };
        assert_eq!(as_vq3, vq3);
    }

    /// Spec rev 3, acceptance criterion 1.
    ///
    /// Every value here was read out of id Software's GPL release. They are
    /// asserted numerically, and against `vq3()` specifically, because `cpm()`
    /// is a community reconstruction and must never be able to satisfy this
    /// test on VQ3's behalf.
    #[test]
    fn verified_constants_match_the_gpl_source() {
        let p = PhysicsProfile::vq3();
        assert_eq!(p.accelerate, s(10.0)); // pm_accelerate
        assert_eq!(p.air_accelerate, s(1.0)); // pm_airaccelerate
        assert_eq!(p.friction, s(6.0)); // pm_friction
        assert_eq!(p.stop_speed, s(100.0)); // pm_stopspeed
        assert_eq!(p.duck_scale, s(0.25)); // pm_duckScale
        assert_eq!(p.jump_velocity, s(270.0)); // JUMP_VELOCITY
        assert_eq!(p.gravity, s(800.0)); // g_gravity default
        assert_eq!(p.step_height, s(18.0)); // STEPSIZE
        assert_eq!(p.overclip, s(1.001)); // OVERCLIP
        assert_eq!(p.max_clip_planes, 5); // MAX_CLIP_PLANES
        assert_eq!(p.ground_trace_probe, s(0.25)); // PM_GroundTrace probe
        assert_eq!(p.min_walk_normal, s(0.7)); // MIN_WALK_NORMAL
        assert_eq!(p.max_speed, s(320.0)); // g_speed default
        assert_eq!(p.hull_mins, vec3(s(-15.0), s(-15.0), s(-24.0)));
        assert_eq!(p.hull_maxs, vec3(s(15.0), s(15.0), s(32.0)));
        assert_eq!(p.crouched_height, s(16.0)); // PM_CheckDuck
    }

    /// The CPM reconstruction, pinned so a change to it is a deliberate edit
    /// rather than a drift. These are *not* claimed to be verified.
    #[test]
    fn cpm_carries_the_reconstructed_promode_constants() {
        let p = PhysicsProfile::cpm();
        assert_eq!(p.accelerate, s(15.0));
        assert_eq!(p.air_control, s(150.0));
        assert_eq!(p.air_stop_accelerate, s(2.5));
        assert_eq!(p.strafe_accelerate, s(70.0));
        assert_eq!(p.strafe_wish_speed_cap, s(30.0));
        assert_eq!(p.double_jump_window_ms, 400);
        assert_eq!(p.double_jump_boost, s(100.0));
        // The base it inherits is still id's.
        assert_eq!(p.air_accelerate, s(1.0));
        assert_eq!(p.friction, s(6.0));
        assert_eq!(p.jump_velocity, s(270.0));
    }

    #[test]
    fn hull_geometry_is_derived_consistently() {
        let p = PhysicsProfile::vq3();
        assert_eq!(p.hull_half_extents(), vec3(s(15.0), s(15.0), s(28.0)));
        // The hull is not centred on the origin: (-24 + 32) / 2 = 4.
        assert_eq!(p.hull_center_offset(), vec3(s(0.0), s(0.0), s(4.0)));
        assert_eq!(p.hull(false).half_extents, p.hull_half_extents());
    }

    #[test]
    fn crouching_lowers_the_hull_top_and_leaves_the_underside_alone() {
        let p = PhysicsProfile::vq3();
        let stand = p.hull(false);
        let duck = p.hull(true);

        // Underside is unchanged: the feet stay on the floor when ducking.
        let underside = |h: &Hull| h.center_offset.z - h.half_extents.z;
        assert_eq!(underside(&stand), underside(&duck));
        // Top drops from 32 to 16.
        assert_eq!(stand.center_offset.z + stand.half_extents.z, s(32.0));
        assert_eq!(duck.center_offset.z + duck.half_extents.z, s(16.0));
        // The horizontal box is the same either way.
        assert_eq!(duck.half_extents.x, s(15.0));
    }
}
