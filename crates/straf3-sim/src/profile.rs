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

    /// **Canonical Straf3 movement — the frozen competitive ruleset.**
    ///
    /// This is what the game plays by default and what a ranked record is set
    /// under. `docs/movement-canon.md` Part 3 argues it constant by constant;
    /// every value below carries either a citation to a source someone read
    /// **from bytes** or a stated reason Straf3 chose it.
    ///
    /// # It is numerically equal to [`Self::cpm`] today, and that is a result
    ///
    /// Three candidate mechanics — crouch slide, dash, wall jump — were
    /// implemented, measured across 7,168 published values, and judged against
    /// criteria written before any of them was measured. **All three were
    /// rejected** (canon Part 2): the slide and the dash on W3, whose
    /// entry-speed test they fail because arriving faster can leave the player
    /// slower; the dash also on W7; the wall jump on W4, being material in one
    /// context of seven. So no candidate constant is switched on here.
    ///
    /// And no *inherited* constant moved either, for a reason canon §1.8 point 2
    /// states rather than assumes: **tuning is a different activity from
    /// judging**, it needs the operator's hands rather than a sweep, and this
    /// wave judged. Changing a number merely to make `straf3` look different
    /// from `cpm` would be the mirror image of keeping one merely because CPM
    /// had it, and `docs/VISION.md` §4.1 rejects both.
    ///
    /// The consequence is worth stating because it is unusually good: this
    /// profile's physics digest **equals `cpm`'s**, so the freeze invalidates no
    /// existing recording, orphans no seeded leaderboard, and costs the browser
    /// client no rebuild.
    ///
    /// # Why it is spelled out rather than written `Self::cpm()`
    ///
    /// Because the equality is a *finding*, not a *link*, and the two profiles
    /// answer to different authorities. [`Self::cpm`] is a reconstruction of
    /// somebody else's game and should be corrected the day someone verifies it
    /// against a CPMA demo. `straf3()` is Straf3's own frozen ruleset and must
    /// **not** move because a reconstruction was corrected — that would be canon
    /// changing under the game by accident, which is the thing the freeze
    /// exists to prevent. Delegating would make every future correction to
    /// `cpm` a silent change to canon.
    ///
    /// `straf3_and_cpm_agree_today_but_are_not_linked` pins the equality so it
    /// cannot drift unnoticed, and says in one line what to do when it breaks.
    ///
    /// # Provenance, in brief — the argument is canon Part 3
    ///
    /// - **Sixteen constants at id's grade**, read from the Quake 3 GPL
    ///   release: `accelerate` (VQ3's 10), `friction`, `stop_speed`,
    ///   `max_speed`, `duck_scale`, `air_accelerate`, `gravity`,
    ///   `jump_velocity`, `step_height`, `overclip`, `max_clip_planes`,
    ///   `ground_trace_probe`, `min_walk_normal`, the hull and
    ///   `crouched_height`.
    /// - **Six at the CPM upstream's grade**, read from `cpm1_dev_docs`'
    ///   `bg_promode.c` (sha256 `589f1e89…`) — and read at the **assignment
    ///   site** in `CPM_UpdateSettings`, not at the file-scope declarations,
    ///   which say `aircontrol = 0` and would have been a confident citation to
    ///   the wrong thing.
    /// - **`friction` 6 is a Straf3 choice, not an inheritance.** The upstream's
    ///   CPM branch sets `pm_friction = 8`; its VQ3 branch sets 6. Straf3
    ///   carries 6 in both, following modern CPMA and DeFRaG rather than the
    ///   design document, because the game as played beats the document it was
    ///   built from.
    /// - **`double_jump_window_ms` 400 is split**: the magnitude is the
    ///   upstream's, the *quantity* is Straf3's own. CPM sets its timer at the
    ///   jump; Straf3 opens the window on the **landing**, gated on
    ///   [`crate::PlayerState::left_ground_by_jumping`], so walking off a ledge
    ///   and jumping on contact is not a double jump.
    #[must_use]
    pub const fn straf3() -> Self {
        Self {
            // ── id's, verified against the GPL release ────────────────────
            friction: s(6.0), // and see the note above: this one is *chosen*
            stop_speed: s(100.0),
            max_speed: s(320.0),
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

            // ── the CPM upstream's, at its assignment site ────────────────
            accelerate: s(15.0),
            air_control: s(150.0),
            air_stop_accelerate: s(2.5),
            strafe_accelerate: s(70.0),
            strafe_wish_speed_cap: s(30.0),
            double_jump_window_ms: 400,
            double_jump_boost: s(100.0),

            // ── the three candidates, all rejected (canon Part 2) ─────────
            //
            // Left at their disabling values, which is where a rejection and an
            // unjudgeable verdict both put them. `canon_frozen.rs` asserts by
            // exhaustive destructure that canon carries no candidate switched
            // on, and `straf3` is canon.
            slide_entry_speed: s(0.0),
            slide_friction: s(0.0),
            slide_duration_ms: 0,
            dash_speed: s(0.0),
            dash_window_ms: 0,
            wall_jump_velocity: s(0.0),
            wall_contact_window_ms: 0,
            wall_normal_max: s(0.0),
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
    /// Canonical Straf3 movement.
    ///
    /// This was [`PhysicsProfile::cpm`] until the movement freeze, per spec D1's
    /// "the default is the higher skill ceiling". It is now
    /// [`PhysicsProfile::straf3`], which satisfies D1's reason unchanged — the
    /// two are numerically equal — while making the default *Straf3's own
    /// ruleset* rather than a reconstruction of another game's. See
    /// `docs/movement-canon.md` Part 3.
    fn default() -> Self {
        Self::straf3()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_profile_is_canonical_straf3() {
        // Was `cpm()` until the movement freeze. Spec D1 asked for the higher
        // skill ceiling and got it: `straf3()` is numerically equal to `cpm()`,
        // so D1's reason is untouched and the default is now the game's own
        // ruleset rather than a reconstruction of somebody else's.
        assert_eq!(PhysicsProfile::default(), PhysicsProfile::straf3());
    }

    /// The equality between canon and the CPM reconstruction is a **finding**,
    /// not a link, and this is what keeps it from drifting unnoticed.
    ///
    /// Canon Part 2 rejected all three candidate mechanics and canon §1.8
    /// point 2 reserves *tuning* for the operator rather than a sweep, so no
    /// constant moved and `straf3()` came out equal to `cpm()`. That is worth a
    /// great deal — the physics digest does not move, so the freeze invalidates
    /// no recording and orphans no leaderboard.
    ///
    /// **If this test fails, do not "fix" it by delegating.** It means somebody
    /// changed one of the two. Decide which: correcting `cpm()` against a CPMA
    /// demo is expected and must *not* drag canon with it — in that case update
    /// this test to record that the two have diverged, and say so in
    /// `docs/movement-canon.md` Part 3. Changing `straf3()` is a change to
    /// frozen canon and needs Part 3 rewritten and every artefact re-cut.
    #[test]
    fn straf3_and_cpm_agree_today_but_are_not_linked() {
        assert_eq!(
            PhysicsProfile::straf3(),
            PhysicsProfile::cpm(),
            "canon and the CPM reconstruction have diverged; read this test's \
             doc comment before changing either"
        );
    }

    /// Canon carries no candidate mechanic switched on — asserted here beside
    /// the constants as well as in `canon_frozen.rs`, because a test next to
    /// the data is the one an editor of the data actually runs.
    #[test]
    fn canonical_straf3_carries_no_candidate_mechanic() {
        let p = PhysicsProfile::straf3();
        assert_eq!(p.slide_entry_speed, s(0.0));
        assert_eq!(p.slide_friction, s(0.0));
        assert_eq!(p.slide_duration_ms, 0);
        assert_eq!(p.dash_speed, s(0.0));
        assert_eq!(p.dash_window_ms, 0);
        assert_eq!(p.wall_jump_velocity, s(0.0));
        assert_eq!(p.wall_contact_window_ms, 0);
        assert_eq!(p.wall_normal_max, s(0.0));
    }

    /// The two constants Part 3 argues hardest, pinned with their grades.
    #[test]
    fn the_frozen_constants_part_3_argues_are_the_ones_it_publishes() {
        let p = PhysicsProfile::straf3();
        // A Straf3 choice: the CPM upstream's own CPM branch sets 8. Straf3
        // follows modern CPMA instead, and Part 3 §3.3 records why.
        assert_eq!(p.friction, s(6.0));
        // The upstream's magnitude, attached to Straf3's own quantity: the
        // window opens on the landing, not at the jump. Part 3 §3.7.
        assert_eq!(p.double_jump_window_ms, 400);
        assert_eq!(p.double_jump_boost, s(100.0));
        // Cited at the assignment site in `CPM_UpdateSettings`, never at the
        // file-scope declaration, which says 0.
        assert_eq!(p.air_control, s(150.0));
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
