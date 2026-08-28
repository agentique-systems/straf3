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
    /// the player off surfaces they are pressed into.
    ///
    /// It is **not**, however, the cause of overbounce and ramp boost, which an
    /// earlier version of this comment claimed. Session A measured both
    /// directions and the mechanism is the three closing lines of `PM_WalkMove`
    /// (`crate::step`, "clip to the ground plane, then restore the speed"):
    /// take the velocity's length, clip it to the ground plane, rescale back to
    /// the original length. Written for a player walking *along* a surface,
    /// they run on whatever velocity the player has, including a large downward
    /// one.
    ///
    /// Perpendicular onto flat ground the clip leaves nothing *but* the
    /// overclip excess, so the rescale blows that back to full length and the
    /// player is launched — which is why the excess looks causal there, and
    /// only there. Add any horizontal velocity, or tilt the floor, and the clip
    /// already leaves a large tangential component: setting `overclip` to 1.0
    /// then moves the answer by a fraction of a percent (340.00 → 340.00 on
    /// flat, −396.92 → −396.53 on a 26° ramp). Both directions are asserted in
    /// `crates/straf3-collision/tests/vocabulary.rs`, so the correction is
    /// enforced rather than merely written down. Anyone tuning ramp boost
    /// should be reaching for the rescale, not for this number.
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

    // ── candidate mechanics (spec rev 3, criterion 4) ───────────────────
    //
    // Crouch slide, dash and wall interaction: three mechanics being *judged*,
    // not three mechanics being shipped. Every one of them is zero in
    // [`Self::vq3`] and [`Self::cpm`] and non-zero only in
    // [`Self::experimental`], and `crates/straf3-collision/tests/canon_frozen.rs`
    // asserts that by exhaustive destructure so it cannot quietly stop being
    // true.
    //
    // They obey the same rule as the CPM block above: each is a *number* the
    // mover reads, and a zero switches the behaviour off rather than a `bool`
    // selecting it. There is no `if experimental { … }` in `crate::step` and
    // there must not be one — see this type's own doc comment on why.
    //
    // One of the eight is not an on/off number, and it is called out rather
    // than left to be noticed: see [`Self::wall_normal_max`].
    /// Minimum horizontal speed at which pressing crouch begins a slide.
    ///
    /// Deliberately set above [`Self::max_speed`] in
    /// [`Self::experimental`], which is what makes the slide a *technique*
    /// rather than a posture: ground acceleration alone cannot reach it, so a
    /// slide has to be entered out of a strafejump. It is also what stops the
    /// slide being a friction toggle — see [`Self::slide_duration_ms`].
    ///
    /// Read only when [`Self::slide_duration_ms`] is non-zero. Zero is not a
    /// disabling value here, because zero is a meaningful threshold ("slide
    /// from any speed").
    pub slide_entry_speed: Scalar,
    /// Friction applied while a slide is running, replacing [`Self::friction`].
    ///
    /// Not an addition to the friction model and not a second code path: it is
    /// the same `PM_Friction`, reading a different number for the duration of
    /// the slide.
    pub slide_friction: Scalar,
    /// How long one slide lasts, in milliseconds. **Zero disables the
    /// mechanic**, which is canon.
    ///
    /// A countdown rather than "hold crouch to keep sliding" because a slide
    /// the player can extend at will is a friction toggle, and a toggle has
    /// nothing to master. The duration bounds it; [`Self::slide_entry_speed`]
    /// bounds re-entry. Both are numbers, so how hard the slide is to chain is
    /// something the lab can measure and the operator can tune, rather than a
    /// rule buried in a branch.
    pub slide_duration_ms: u16,
    /// The wish speed a dash asks for along the current wish direction.
    /// **Zero disables the mechanic**, which is canon.
    ///
    /// Deliberately a *wish speed* fed through `PM_Accelerate`'s clamp rather
    /// than an impulse added to velocity. The difference is the whole
    /// character of the mechanic: an added impulse is worth the same at 1000
    /// ups as at rest and is therefore strictly correct to spend the instant it
    /// is available, which is one mandatory execution rather than a route
    /// choice. Clamped, a dash is worth nothing along a direction the player is
    /// already travelling at this speed and a great deal across it, so *where*
    /// it is aimed is the decision — the same clamp strafejumping is built on.
    pub dash_speed: Scalar,
    /// How long a dash stays available once armed, in milliseconds. **Zero
    /// disables the mechanic**, which is canon.
    ///
    /// Armed exactly as [`Self::double_jump_window_ms`] is: on a landing that
    /// ended a jump, provenance-gated through
    /// [`crate::PlayerState::left_ground_by_jumping`], counted down, and spent
    /// by the dash that uses it. **Not a cooldown** — "cooldown rotations that
    /// replace momentum mastery" is a confirmed anti-goal of the vision, and a
    /// dash on a timer that refills regardless of what the player did is
    /// exactly that.
    ///
    /// See [`crate::step`] for what spends it: a jump press *in the air*, so
    /// the dash costs the player the input their bunnyhop rhythm is already
    /// using and needs no button of its own.
    pub dash_window_ms: u16,
    /// Horizontal speed a landing must carry before it arms a dash.
    ///
    /// **This field exists because of a gate failure, and that is the whole
    /// reason it is here rather than in the original candidate set.**
    /// `docs/movement-canon.md` §1.3's G5(a) asks how often a mechanic becomes
    /// available to a player who never exceeds [`Self::max_speed`] on flat open
    /// ground, and fails the mechanic if the count is not zero. The dash armed
    /// on *any* landing that ended a jump, so a standing player could jump on
    /// the spot, land, and hold a dash window at zero speed indefinitely —
    /// measured at ten armings out of ten in
    /// `crates/straf3-sim/tests/canon_gates.rs`. Canon Part 1 disclosed this
    /// exposure in advance, from reading `step.rs`, before any candidate was
    /// measured.
    ///
    /// This is the single retune §1.5 permits the dash, pre-registered in
    /// canon §3.8 *before* the re-measurement rather than discovered after it.
    /// It mirrors [`Self::slide_entry_speed`] exactly, for the same reason
    /// stated there: set above `max_speed`, ground acceleration alone cannot
    /// reach it, so the window has to be bought with speed the player earned.
    ///
    /// **Zero imposes no floor**, which is the pre-retune behaviour and the
    /// value both canon profiles carry. Zero is therefore not an "off" for the
    /// dash — [`Self::dash_speed`] and [`Self::dash_window_ms`] are what
    /// disable it — which makes this the one threshold constant G8's
    /// clarification allows a candidate, alongside `wall_normal_max` for the
    /// wall jump.
    pub dash_entry_speed: Scalar,
    /// Velocity a wall jump adds along the wall's normal, on top of the
    /// ordinary [`Self::jump_velocity`] it sets vertically. **Zero disables the
    /// mechanic**, which is canon.
    ///
    /// Along the normal rather than upward because the gap this addresses is
    /// horizontal: Session A measured that traversing a ramp never *gains*
    /// speed and costs `entry · cos(angle)` at the seam, so the vocabulary has
    /// no way to convert a wall into speed. The slide solver has already
    /// removed the into-wall component by the time this is read, so what this
    /// adds is genuinely new outward speed rather than restored speed.
    pub wall_jump_velocity: Scalar,
    /// How long after touching a wall the wall jump remains available, in
    /// milliseconds. **Zero disables the mechanic**, which is canon.
    ///
    /// It is also what gates the *recording* of wall contact: with this at zero
    /// nothing is written to [`crate::PlayerState::wall_normal`] at all, so a
    /// canon run's state is bit-identical to its pre-wave self and not merely
    /// behaviourally identical.
    pub wall_contact_window_ms: u16,
    /// A plane counts as a wall when its normal's Z component is at or below
    /// this.
    ///
    /// **This is the one candidate constant that is not switched off by a
    /// zero**, and the exception is worth stating because this type's doc
    /// comment forbids a field that is really a switch. It is not a switch —
    /// it is a threshold, and zero is a meaningful threshold (only exactly
    /// vertical or overhanging planes). A threshold therefore cannot carry the
    /// "zero disables" convention, so the mechanic is gated on its two *effect*
    /// constants above and this is read only when they are non-zero.
    ///
    /// There is precedent in this very struct rather than a new rule:
    /// [`Self::strafe_wish_speed_cap`] is read only when
    /// [`Self::strafe_accelerate`] is non-zero, for exactly the same reason.
    /// Canon sets this to 0.0 so that a reader who checks finds a disabling
    /// value anyway.
    ///
    /// Compare [`Self::min_walk_normal`] at 0.7: a plane between these two
    /// values is a steep ramp — too steep to walk, not steep enough to push
    /// off. That band is deliberate, so that "wall" means something a player
    /// can identify by looking at it.
    pub wall_normal_max: Scalar,
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

            // Candidate mechanics, all off. `cpm()` inherits these unchanged,
            // so both canon profiles carry them at their disabling values and
            // `canon_frozen.rs` asserts it.
            slide_entry_speed: s(0.0),
            slide_friction: s(0.0),
            slide_duration_ms: 0,
            dash_speed: s(0.0),
            dash_window_ms: 0,
            dash_entry_speed: s(0.0),
            wall_jump_velocity: s(0.0),
            wall_contact_window_ms: 0,
            wall_normal_max: s(0.0),
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

    /// CPM plus the three candidate mechanics, for measurement only (spec
    /// rev 3, criteria 4 and 5).
    ///
    /// # This profile is not comparable to canon and is not meant to be
    ///
    /// Spec D2: `experimental` is playable and recordable but never comparable
    /// to `vq3` or `cpm`, and its personal bests save under a separate key
    /// (`runs/<map>.experimental.s3d`). Nothing here has earned a place in the
    /// canonical ruleset; the wave's honest outcome may be that none of it
    /// does. See `docs/candidate-mechanics.md` for the assessment.
    ///
    /// # Why it is `..Self::cpm()` and not a fresh set of numbers
    ///
    /// So that any difference the lab measures between `cpm` and
    /// `experimental` is attributable to the three mechanics and to nothing
    /// else. A profile that also retuned, say, `air_accelerate` would make
    /// every measurement a two-variable question, which is exactly the kind of
    /// evidence that cannot settle a design argument.
    ///
    /// # Where these eight numbers came from
    ///
    /// They are opening positions chosen to put each mechanic in a regime
    /// where it does something measurable, **not** tuned values, and not
    /// reconstructed from any other game. Their job is to give
    /// `tools/straf3-lab` something to measure; the assessment is written
    /// against what it measures, and a mechanic whose case depends on finding
    /// exactly the right constant has already failed "simple to invoke".
    #[must_use]
    pub const fn experimental() -> Self {
        Self {
            // ── crouch slide ──────────────────────────────────────────────
            // Entry above `max_speed` (320) on purpose: ground acceleration
            // cannot reach 400, so a slide must be entered out of a
            // strafejump. That single number is also the anti-chaining rule —
            // re-entering costs a command spent standing, at full friction.
            slide_entry_speed: s(400.0),
            // A sixth of canon friction: fast enough that a slide still ends,
            // slow enough that carrying speed under a low ceiling is worth
            // doing.
            slide_friction: s(1.0),
            slide_duration_ms: 600,

            // ── dash ──────────────────────────────────────────────────────
            // A wish speed, not an impulse. 400 is above `max_speed` so a dash
            // is worth taking on the ground, and the clamp makes it worth
            // little along a direction already travelled at speed.
            dash_speed: s(400.0),
            // The double-jump window, deliberately: the two are armed by the
            // same landing and compete for the same input, which is where the
            // choice between them lives.
            dash_window_ms: 400,
            // The §1.5 retune, pre-registered in canon §3.8 before it was
            // measured. Mirrors `slide_entry_speed` 400 exactly: above
            // `max_speed` 320, so ground acceleration alone cannot arm a dash
            // and G5(a)'s flat-ground count goes to zero.
            dash_entry_speed: s(400.0),

            // ── wall interaction ──────────────────────────────────────────
            // Below `jump_velocity` (270): a wall jump should be a redirect,
            // not a better jump.
            wall_jump_velocity: s(200.0),
            // Half the dash window. A wall jump is a reaction to geometry the
            // player is already touching, so it needs less slack than one
            // armed by an event a command earlier.
            wall_contact_window_ms: 200,
            // Steeper than 72°. Comfortably clear of `min_walk_normal` (0.7,
            // i.e. 45.6°) so that a ramp the player might try to walk up is
            // never also a wall they can push off.
            wall_normal_max: s(0.3),

            ..Self::cpm()
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
