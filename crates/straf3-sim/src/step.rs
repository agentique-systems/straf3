//! The step function: the one entry point the whole project depends on.
//!
//! # How to read this file
//!
//! It is a transcription of Quake 3's `bg_pmove.c` / `bg_slidemove.c`, with the
//! order of operations preserved, because the order *is* the behaviour: friction
//! before acceleration, acceleration before the slide solver, the ground probe
//! run twice per command. Function names mirror the originals in the comments
//! (`PM_Friction`, `PM_Accelerate`, `PM_SlideMove`, …) so a reader holding the
//! GPL source open can check this line by line, which is exactly what spec
//! criterion 1 asks for.
//!
//! Two things are deliberately *not* transcribed:
//!
//! - **Constants are never literals here.** Everything the two profiles differ
//!   on lives in [`PhysicsProfile`], and VQ3 is spelled as CPM with the
//!   extensions set to zero. There is no `if cpm { … }` anywhere below, and
//!   there must not be one: the operator will tune these numbers, and a number
//!   can be tuned where a branch cannot.
//! - **Q3's internal epsilons** (the `0.1` into-plane threshold, the `0.99`
//!   duplicate-plane test, `numbumps`) are named constants rather than profile
//!   fields, because they are not knobs id ever exposed and pretending they are
//!   tunable would misrepresent them.

use crate::cmd::{Buttons, UserCmd};
use crate::num::{self, Scalar, Vec3, s, vec3};
use crate::profile::{Hull, PhysicsProfile};
use crate::state::{GroundState, PlayerState, SimState};
use crate::world::{SurfaceFlags, Sweep, Trace, TriggerSet, World};

// ── Quake's internal constants ─────────────────────────────────────────────
//
// These are not profile fields: they are structural constants of the solver
// that id never exposed as cvars. See the module docs.

/// `numbumps` in `PM_SlideMove`: how many times the move is re-planned after
/// hitting something before the remainder of the frame is abandoned.
const SLIDE_BUMPS: u32 = 4;

/// `PM_SlideMove`'s threshold for "this move interacts with that plane".
/// Velocity heading into a plane by less than this is left alone.
const INTO_PLANE_EPSILON: Scalar = s(0.1);

/// `PM_SlideMove`'s test for "we have hit this plane already". Above this dot
/// product the two planes are treated as the same one and velocity is nudged
/// out along the normal instead of being clipped again.
const SAME_PLANE_DOT: Scalar = s(0.99);

/// Q3's `SURFACE_CLIP_EPSILON`: a sweep stops this far short of the surface,
/// measured along the surface normal.
///
/// In Q3 this lives inside the collision code. Here it lives in the mover,
/// because [`World`] is a seam the simulation does not control and an
/// implementor cannot be relied on to apply it. Without it the player comes to
/// rest exactly on the plane, float error puts them a hair inside it on the
/// next command, and every trace afterwards reports `start_solid`.
const SURFACE_CLIP_EPSILON: Scalar = s(0.125);

/// `PM_GroundTrace`: velocity heading out of the ground plane faster than this
/// means the player has been thrown off it and is airborne, whatever the probe
/// found.
const THROWN_OFF_GROUND_SPEED: Scalar = s(10.0);

/// `PM_Friction`: below this speed the horizontal velocity is zeroed outright
/// rather than scaled, so a player actually stops.
const MIN_FRICTION_SPEED: Scalar = s(1.0);

/// `PM_CheckJump`: Q3 spells "jump is pressed" as `cmd.upmove >= 10`.
const JUMP_UPMOVE: i8 = 10;

/// The `upmove` a jump contributes to `PM_CmdScale`. Q3's client sends 127 on
/// the jump axis; this API also has a [`Buttons::JUMP`] bit, and a command that
/// sets the bit without the axis is treated as if it had sent this, so the two
/// spellings produce the same physics.
const JUMP_UPMOVE_FULL: i8 = 127;

/// `PM_StepSlideMove`: how flat a surface has to be under the player before a
/// step up is allowed while still moving upwards. Q3 hardcodes 0.7 here
/// separately from `MIN_WALK_NORMAL`, and they are kept separate here too.
const STEP_MIN_NORMAL: Scalar = s(0.7);

/// The largest command-axis magnitude, which `PM_CmdScale` divides by.
const CMD_AXIS_MAX: Scalar = s(127.0);

/// Degrees to radians, as Q3's `AngleVectors` computes it (`M_PI * 2 / 360`).
const DEG_TO_RAD: Scalar = s(core::f32::consts::PI * 2.0 / 360.0);

/// The longest a single integration step may be, in whole milliseconds.
///
/// This is Q3's `Pmove` loop bound. `bg_pmove.c`'s `Pmove` — the *outer*
/// function, the one that calls `PmoveSingle` — reads:
///
/// ```text
/// // chop the move up if it is too long, to prevent framerate
/// // dependent behavior
/// while ( pmove->ps->commandTime != finalTime ) {
///     int msec = finalTime - pmove->ps->commandTime;
///     if ( pmove->pmove_fixed ) {
///         if ( msec > pmove->pmove_msec ) { msec = pmove->pmove_msec; }
///     } else {
///         if ( msec > 66 ) { msec = 66; }
///     }
///     pmove->cmd.serverTime = pmove->ps->commandTime + msec;
///     PmoveSingle( pmove );
///     ...
/// }
/// ```
///
/// # Why 66 and not `pmove_msec`
///
/// id has two bounds, and they answer different questions. `pmove_msec`
/// (8..=33, clamped in `ClientThink_real`) exists only when `pmove_fixed` is
/// set, and `pmove_fixed`'s job is to make *client prediction* agree with the
/// server by forcing both to integrate in identical chunks regardless of
/// framerate. Straf3 has no prediction to reconcile: [`UserCmd::duration_ms`]
/// is already a recorded, fixed-duration quantum (spec D2), so the tick rate
/// itself does what `pmove_fixed` was invented to do. What is left is id's
/// other bound, the unconditional one — the safety net that stops a single
/// enormous frame from being integrated in one go.
///
/// So this is 66, id's number for that job, verbatim.
///
/// # Why it is a constant and not a [`PhysicsProfile`] field
///
/// For the same reason [`SLIDE_BUMPS`] and [`INTO_PLANE_EPSILON`] are: it is a
/// property of *how* the solver is run, not a number that describes how the
/// game moves. Every command this project actually produces is shorter than
/// it — 8 ms at 125 Hz, 4 ms at 250 Hz, 13 ms at 76 Hz — so tuning it would
/// change nothing a player can feel, while making a bound that must be the
/// same on every target into a value a recording would have to carry.
///
/// # What the bound costs, stated honestly
///
/// A 66 ms sub-step is not fine. It is 8¼ commands' worth of integration at
/// 125 Hz, and a command that long behaves nothing like eight 8 ms ones. The
/// bound is not a claim that 66 ms is accurate; it is a claim that no duration
/// is *unbounded*, which is what a sub-step loop can promise and a single step
/// cannot. Callers that want fidelity ask for it by sending short commands.
pub const PMOVE_SUBSTEP_MAX_MS: u16 = 66;

/// Advance the simulation by exactly one command.
///
/// # The contract this function exists to state
///
/// The result depends on the four arguments and **nothing else**. No globals,
/// no clock, no filesystem, no unseeded randomness, no thread-local cache, no
/// interior mutability. Call it twice with the same inputs and you get the
/// same bits.
///
/// That is not fastidiousness; it is the property everything downstream is
/// built on:
///
/// - **Replays and ghosts** store inputs, not positions. A ghost is this
///   function re-run.
/// - **Regression tests** replay a recorded run and compare the result, which
///   only detects a change in movement feel if nothing else can vary.
/// - **Headless servers and RL environments** run this with no window, no GPU
///   and no frame loop, faster than real time, in parallel.
/// - **Debugging** a movement bug means replaying the input that caused it.
///   Once, exactly, rather than trying to reproduce it by hand.
///
/// Anything that would break it — reading a config file here, asking what time
/// it is, hashing a pointer address — must go somewhere above the line
/// instead. `cargo xtask check-seam` fails the build over it, and that check
/// is deliberately hard to argue with.
///
/// # Why a `World` and a `PhysicsProfile` are arguments
///
/// Because they are inputs, and inputs to a pure function are arguments. A
/// global "current map" or "current physics mode" would make two callers in
/// the same process interfere — which is exactly what a headless server
/// running many simulations at once, or a test suite running in parallel
/// threads, does.
#[must_use]
pub fn step<W>(state: &SimState, cmd: &UserCmd, world: &W, profile: &PhysicsProfile) -> SimState
where
    W: World + ?Sized,
{
    let mut next = *state;
    step_in_place(&mut next, cmd, world, profile);
    next
}

/// [`step`], writing into an existing state.
///
/// Identical in behaviour; it exists because a headless server stepping
/// thousands of simulations per second should not be forced to copy state it
/// is about to overwrite. `step` is defined in terms of this one, so there is
/// no second implementation to keep in agreement.
///
/// # The return value
///
/// The timing volumes the player's hull passed through during this command.
/// [`SimState::run`] is already advanced from [`TriggerSet::START`] and
/// [`TriggerSet::FINISH`] before this returns, so a caller that only wants a
/// time can ignore it — and every existing caller does.
///
/// It is returned rather than stored because checkpoint splits are a *caller's*
/// concern and [`SimState`] is the thing a recording's digest folds. Growing
/// `SimState` with a checkpoint table would change every digest ever taken, to
/// carry data the physics never reads. So the alphabet crosses the seam and the
/// bookkeeping stays above it.
///
/// [`SimState::run`]: crate::SimState::run
pub fn step_in_place<W>(
    state: &mut SimState,
    cmd: &UserCmd,
    world: &W,
    profile: &PhysicsProfile,
) -> TriggerSet
where
    W: World + ?Sized,
{
    step_bounded(state, cmd, world, profile, PMOVE_SUBSTEP_MAX_MS)
}

/// [`step_in_place`] with the sub-step bound as an argument.
///
/// The bound is a parameter of exactly one private function, and there is
/// exactly one caller that supplies anything but [`PMOVE_SUBSTEP_MAX_MS`]:
/// this module's tests, which pass `u16::MAX` to integrate a command in a
/// single step and so recover — through *this* code rather than through a copy
/// of it — the behaviour sub-stepping replaced. That is what lets "nothing at
/// or below the bound moved" be a measurement instead of an argument, and it
/// cannot drift from the shipped path the way a duplicated integrator would.
///
/// It is not a knob. Nothing above this function can reach it, and a bound
/// that varied between two runs of the same recording would be a determinism
/// bug of exactly the kind [`SimState::checksum`] exists to catch.
fn step_bounded<W>(
    state: &mut SimState,
    cmd: &UserCmd,
    world: &W,
    profile: &PhysicsProfile,
    bound: u16,
) -> TriggerSet
where
    W: World + ?Sized,
{
    // A zero-length command advances nothing. Returning early rather than
    // integrating by zero keeps `tick` counting commands that did something.
    if cmd.duration_ms == 0 {
        return TriggerSet::NONE;
    }

    // A zero bound would not terminate. One millisecond is also `PmoveSingle`'s
    // own floor — `if (pml.msec < 1) pml.msec = 1;` — so the smallest legal
    // sub-step here is the smallest legal one there.
    let bound = bound.max(1);

    // The view is player input, applied whole. Movement never rotates the
    // player: what you looked at is what the recording says you looked at.
    // Applied once for the command, not once per sub-step, because it is the
    // same value either way — the command carries one view.
    state.player.view = cmd.view;

    // ── Q3's `Pmove` loop: chop the move up if it is too long ──────────────
    //
    // Everything below this line runs once *per sub-step*. That is the whole
    // change: [`Pmove`] is Q3's `PmoveSingle`, and `PmoveSingle` is the thing
    // id's loop calls repeatedly, so a sub-step gets its own `Pmove` — its own
    // `dt`, its own ground probe, its own timer drop, its own solver. Nothing
    // inside `Pmove` knows the loop exists, exactly as nothing in
    // `PmoveSingle` knows about `Pmove`.
    //
    // The split is in integer milliseconds and the arithmetic is exact:
    // `remaining` starts at the command's duration, each pass removes the
    // sub-step it is about to integrate, and the loop ends when it reaches
    // zero. There is no float accumulator and no remainder to drift, because
    // the remainder is not a residue — it is simply the last, short sub-step,
    // which is where Q3 puts it too (`msec = finalTime - commandTime` capped
    // at the bound takes full sub-steps first, and the final pass consumes
    // whatever is left as a sub-step of its own; id has no remainder branch
    // and neither does this). A 100 ms command is 66 + 34, in that order; a
    // 1000 ms command is fifteen 66s and a 10.
    //
    // `PmoveSingle`'s own `if (pml.msec < 1) pml.msec = 1;` floor is satisfied
    // structurally rather than transcribed: a zero-duration command returned
    // above, and every sub-step below is `min(remaining, bound)` with
    // `remaining > 0`, so no sub-step of zero milliseconds can be constructed.
    // Its `else if (pml.msec > 200)` ceiling is unreachable under a 66 ms
    // bound, exactly as it is unreachable from id's own `Pmove`.
    //
    // **Q3's arrears clamp is deliberately not ported.** `Pmove` opens with
    // `if (finalTime > commandTime + 1000) commandTime = finalTime - 1000;` —
    // more than a second of backlog is thrown away rather than simulated. That
    // is a decision about *wall-clock catch-up*: `commandTime` is arrears
    // against a real clock, and a server that hitched for thirty seconds must
    // not then simulate thirty seconds at once. Straf3 has no arrears here.
    // `duration_ms` is a recorded input, not a debt against a clock, and this
    // function is not allowed to know what time it is — deciding how much wall
    // time becomes commands is the platform layer's job, above the seam, where
    // the clock lives. Silently integrating less than a command says would
    // make a replay disagree with the run it recorded. So every millisecond
    // handed in is integrated, and the cost is bounded anyway: `u16::MAX` ms
    // is 993 sub-steps.
    let mut remaining = cmd.duration_ms;
    let mut touched = TriggerSet::NONE;

    while remaining > 0 {
        let ms = remaining.min(bound);
        remaining -= ms;

        // The one permitted integer-milliseconds-to-scalar conversion in the
        // whole crate — spec rev 3, criterion 3. Mirrors Q3's
        // `pml.frametime = pml.msec * 0.001`. Do not add a second one. It is
        // reached once per sub-step rather than once per command, which is the
        // point: the truncation happens where Q3's did, at the granularity
        // Q3's did.
        let dt = num::seconds_from_millis(u32::from(ms));

        let mut pm = Pmove::new(cmd, world, profile, dt);
        let crossed = pm.run(&mut state.player, ms);

        // The clock advances by the sub-step, in whole milliseconds. The sum
        // over the loop is the command's duration exactly — integers do not
        // drift — so `time_ms` at the command boundary is unchanged from what
        // a single step produced, however many sub-steps ran.
        state.time_ms += u32::from(ms);
        touched = touched.with(crossed);

        // The clock is read at the *sub-step* boundary, in whole milliseconds,
        // from the integer sum of durations — never interpolated within the
        // step that crossed the line. A sub-tick time would be a float, and a
        // verifier would have to reproduce it bit-exactly for no benefit.
        // This is ARCHITECTURE C4's rule unchanged and its predicted shape:
        // sub-stepping makes the clock *finer* without making it float. A
        // finish crossed 66 ms into a 200 ms command is timed at 66 ms into
        // it, not at the end of it.
        //
        // The consequence, accepted deliberately (ARCHITECTURE C4): times
        // quantise to the sub-step — which for every rate this project runs at
        // is the command duration, multiples of 8 ms at 125 Hz.
        //
        // Start before finish, so a step that crosses both — a course whose
        // lines touch, or a player teleported across the map — yields zero
        // rather than a run that never started.
        if crossed.contains(TriggerSet::START) {
            state.run.start(state.time_ms);
        }
        if crossed.contains(TriggerSet::FINISH) {
            state.run.finish(state.time_ms);
        }
    }

    // Once per *command*, not once per sub-step: `tick` counts commands
    // applied, and a caller comparing it against the length of a recorded
    // command stream must keep getting the same answer. Sub-stepping is an
    // integration detail, and a recording does not know how many sub-steps its
    // commands were split into.
    state.tick += 1;

    touched
}

/// Apply a whole sequence of commands in order.
///
/// A convenience over [`step`] with no behaviour of its own — it exists so
/// that the headless runner, the tests and a future replay verifier all drive
/// the simulation through exactly the same loop rather than each writing their
/// own subtly different one.
#[must_use]
pub fn run<'a, W, I>(initial: &SimState, cmds: I, world: &W, profile: &PhysicsProfile) -> SimState
where
    W: World + ?Sized,
    I: IntoIterator<Item = &'a UserCmd>,
{
    let mut state = *initial;
    for cmd in cmds {
        step_in_place(&mut state, cmd, world, profile);
    }
    state
}

/// Which of Quake's eight `movementDir` cases the command axes fall into,
/// reduced to the three CPM actually distinguishes.
///
/// CPM's air rules key off this: air control needs pure forward or back
/// (`movementDir` 0 or 4), the strafe acceleration model needs pure left or
/// right (2 or 6). Diagonals get neither, which is why CPM air movement is
/// played with one key at a time.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum MoveDir {
    /// No movement keys held.
    None,
    /// Forward or back, with no strafe.
    ForwardBack,
    /// Left or right, with no forward or back.
    Strafe,
    /// A diagonal.
    Diagonal,
}

impl MoveDir {
    fn of(forward: i8, right: i8) -> Self {
        match (forward != 0, right != 0) {
            (false, false) => Self::None,
            (true, false) => Self::ForwardBack,
            (false, true) => Self::Strafe,
            (true, true) => Self::Diagonal,
        }
    }
}

/// Quake's `pml_t`: everything the movement functions share for the duration of
/// one **sub-step**, and nothing that outlives it.
///
/// It is a struct rather than a pile of arguments for the same reason Q3 used
/// one — `PM_WalkMove` and `PM_SlideMove` need the same eight values — but
/// unlike Q3's it is a local, not a global, so two simulations in one process
/// cannot see each other's.
///
/// # Its lifetime is a sub-step, and that is load-bearing
///
/// Q3 `memset`s `pml` at the top of every `PmoveSingle`, so each pass of the
/// `Pmove` loop starts from a clean one; [`step_in_place`] builds a fresh
/// `Pmove` per sub-step for the same reason, and that is what keeps the loop
/// from needing any state of its own. Everything that must survive a sub-step
/// boundary already lives in [`PlayerState`] — `jump_held`, the timers, the
/// ground state — which is precisely where a value that outlives one
/// integration step belongs, because that is the struct a digest folds and a
/// replay reproduces.
struct Pmove<'a, W: World + ?Sized> {
    world: &'a W,
    profile: &'a PhysicsProfile,
    /// Sub-step duration in seconds. Q3's `pml.frametime`.
    dt: Scalar,
    /// The collision box for this sub-step, standing or crouched.
    hull: Hull,

    /// View basis vectors, Q3's `pml.forward` / `pml.right`.
    forward: Vec3,
    right: Vec3,

    /// Command axes as scalars, and the `upmove` actually used by
    /// `PM_CmdScale` — which `PM_CheckJump` zeroes when a held jump is refused.
    forward_move: Scalar,
    right_move: Scalar,
    up_move: Scalar,
    move_dir: MoveDir,

    /// Whether this command asks to jump at all.
    jump_pressed: bool,
    /// Whether this command asks to crouch.
    crouch_pressed: bool,
    /// Whether this command is the *press* that began a crouch, rather than a
    /// crouch already being held.
    ///
    /// Captured in [`Self::check_duck`] before it sets
    /// [`PlayerState::crouched`], which is the only moment the previous
    /// command's value is still readable. The crouch slide is edge-triggered on
    /// this for the same reason jumping is edge-triggered on
    /// [`PlayerState::jump_held`]: a technique you can hold down is a posture,
    /// and a posture has no timing to master.
    ///
    /// Sub-stepping does not multiply the edge, and needs no help not to: the
    /// first sub-step of a crouch press sets [`PlayerState::crouched`], so
    /// every later sub-step of the same command reads it as already crouched
    /// and finds no edge. One press, one slide, whatever the command's
    /// duration — the same structural answer as the jump's.
    crouch_edge: bool,

    /// Q3's `pml.groundPlane`: there is a plane underfoot.
    ground_plane: bool,
    /// Q3's `pml.walking`: that plane is walkable.
    walking: bool,
    ground_normal: Vec3,
    ground_surface: SurfaceFlags,

    /// Timing volumes the hull has passed through so far this sub-step.
    ///
    /// Q3 has no equivalent; this is ARCHITECTURE C4's accumulator. It sits
    /// beside `ground_plane` and `walking` because it has the same lifetime —
    /// one sub-step — and is consumed by [`step_in_place`] at the end of
    /// [`Pmove::run`]. That consumption point is what makes sub-stepping make
    /// the clock finer without making it a float and without changing any rule
    /// here: a start or finish is stamped at the sub-step boundary it was
    /// crossed on, which is still an exact integer sum of durations.
    touched: TriggerSet,
}

impl<'a, W: World + ?Sized> Pmove<'a, W> {
    fn new(cmd: &UserCmd, world: &'a W, profile: &'a PhysicsProfile, dt: Scalar) -> Self {
        // Q3 spells jump as `upmove >= 10` and crouch as `upmove < 0`. This API
        // also has buttons for both. Accepting either keeps the two spellings
        // equivalent rather than making the caller pick, and a `Buttons::JUMP`
        // with no axis is given Q3's full 127 so that `PM_CmdScale` sees the
        // same thing it would have seen from a Q3 client — that is the jump
        // frame's small wish-speed dip, and dropping it would quietly change
        // ground speed.
        let jump_pressed = cmd.up_move >= JUMP_UPMOVE || cmd.buttons.contains(Buttons::JUMP);
        let crouch_pressed = cmd.up_move < 0 || cmd.buttons.contains(Buttons::CROUCH);
        let up_move = if cmd.buttons.contains(Buttons::JUMP) && cmd.up_move < JUMP_UPMOVE {
            JUMP_UPMOVE_FULL
        } else {
            cmd.up_move
        };

        // View angles arrive quantised to 16 bits (C3), so this is where the
        // physics gets its degrees back. Dequantising here rather than storing
        // degrees keeps the command stream — the thing recordings and digests
        // are taken over — the single source of truth for where the player was
        // looking: every target reads the same integer and computes the same
        // basis from it.
        let (forward, right) = angle_vectors(
            cmd.view.pitch_degrees(),
            cmd.view.yaw_degrees(),
            cmd.view.roll_degrees(),
        );

        Self {
            world,
            profile,
            dt,
            hull: profile.hull(false),
            forward,
            right,
            forward_move: s(f32::from(cmd.forward_move)),
            right_move: s(f32::from(cmd.right_move)),
            up_move: s(f32::from(up_move)),
            move_dir: MoveDir::of(cmd.forward_move, cmd.right_move),
            jump_pressed,
            crouch_pressed,
            crouch_edge: false,
            ground_plane: false,
            walking: false,
            ground_normal: num::ZERO,
            ground_surface: SurfaceFlags::NONE,
            touched: TriggerSet::NONE,
        }
    }

    /// Quake's `PmoveSingle`, in its order. Returns the timing volumes the
    /// player's hull passed through.
    ///
    /// `ms` is the **sub-step's** duration, not the command's, and every timer
    /// below counts down on it — see the `PM_DropTimers` call for why that is
    /// a movement decision rather than a detail.
    ///
    /// # Q3's `pmove->cmd.upmove = 20`, and why it is absent here
    ///
    /// After each `PmoveSingle`, id's `Pmove` loop writes:
    ///
    /// ```text
    /// if ( pmove->ps->pm_flags & PMF_JUMP_HELD ) {
    ///     pmove->cmd.upmove = 20;
    /// }
    /// ```
    ///
    /// That line is repair work for an aliasing bug, not a movement rule.
    /// `PM_CheckJump` refuses a held jump by zeroing `pm->cmd.upmove` — and
    /// `pm->cmd` is the *same* struct the loop reuses for the next sub-step,
    /// so without the write-back the next `PmoveSingle` would see `upmove 0`,
    /// read that as "the jump input was released", clear `PMF_JUMP_HELD` and
    /// hand the player a free second jump inside one command. Twenty is simply
    /// a number at or above `upmove >= 10`.
    ///
    /// Here there is nothing to repair. [`Self::up_move`] is a copy owned by
    /// one `Pmove`, so [`Self::check_jump`]'s zeroing dies with the sub-step
    /// that did it, and the next sub-step re-derives `jump_pressed` from the
    /// unmodified [`UserCmd`]. Held-ness survives in
    /// [`PlayerState::jump_held`], which is where it belongs and which the
    /// digest already folds. One press still buys one jump, however long the
    /// command — asserted by `a_held_jump_is_one_jump_however_long_the_command`
    /// in this module's tests.
    ///
    /// The one visible difference from id: in a sub-step that follows a jump
    /// within the same command, `PM_CmdScale` sees the command's own `upmove`
    /// (127 for a jump) where Q3 would have substituted 20, so the airborne
    /// wish-speed dip is the full one rather than a reduced one.
    ///
    /// **That difference goes the right way**, which is worth checking rather
    /// than assuming, because the whole purpose of chopping is to make a long
    /// command behave like the short ones it stands for. Q3 rebuilds `pm.cmd`
    /// from the client's command on every `ClientThink_real`, so the *second
    /// command* of a held jump delivered as two commands sees `upmove 127` —
    /// while the *second sub-step* of the same play delivered as one chopped
    /// command sees 20. Porting the write-back would therefore make a chopped
    /// command disagree with the stream it is meant to be equivalent to, and
    /// would break `a_command_is_exactly_the_sub_steps_it_is_chopped_into`.
    /// It is also unreachable at every rate this project runs at, since a
    /// command shorter than [`PMOVE_SUBSTEP_MAX_MS`] has no second sub-step.
    fn run(&mut self, p: &mut PlayerState, ms: u16) -> TriggerSet {
        // `PmoveSingle`: releasing the jump input re-arms the jump.
        if !self.jump_pressed {
            p.jump_held = false;
        }

        self.check_duck(p);
        self.ground_trace(p);
        // `PM_DropTimers`, and it lives inside `PmoveSingle` in id's source as
        // it does inside this function — so a timer counts down once per
        // sub-step, by the sub-step's own milliseconds. The total decrement
        // across a command is the same integer either way; what changes is
        // that the countdown is now *read* between sub-steps. That is exactly
        // what stops a long command stepping over a jump window: a 400 ms
        // double-jump window survives the first 66 ms of a 500 ms command and
        // is still open when that sub-step reaches `check_jump`, where a
        // single step would have subtracted all 500 ms before looking.
        p.timers.advance(ms);

        if self.walking {
            self.walk_move(p);
        } else {
            self.air_move(p);
        }

        // Q3 probes again after moving, so the state a caller observes — and
        // the state the next command starts from — describes where the player
        // ended up, not where they began.
        self.ground_trace(p);

        self.touched
    }

    // ── traces ─────────────────────────────────────────────────────────────

    /// A **probe**: a question about geometry the hull does not enter.
    ///
    /// Nothing swept here counts towards [`Self::touched`], and that is the
    /// whole distinction this pair of methods exists to make. The ground probe
    /// reaches `ground_trace_probe` units below the player's feet every command;
    /// `PM_CorrectAllSolid` fires 27 zero-length point tests; the step-down
    /// probe asks whether stepping is allowed before anything moves. OR-ing any
    /// of them into the accumulator credits a player with a finish line they
    /// never touched, which on a leaderboard is exactly as wrong as missing one.
    ///
    /// Use [`Self::sweep_to`] when the hull really is carried along the sweep.
    fn sweep(&self, from: Vec3, to: Vec3) -> Trace {
        self.world.trace(&Sweep {
            start: from,
            end: to,
            half_extents: self.hull.half_extents,
            center_offset: self.hull.center_offset,
        })
    }

    /// A **committed sweep**: the hull is carried along it. Returns the trace
    /// and the point it actually reached.
    ///
    /// The hull is held [`SURFACE_CLIP_EPSILON`] clear of whatever it hit,
    /// measured along the surface normal, which is what Q3's collision code
    /// does and what keeps a resting player out of the floor. See that
    /// constant's documentation.
    ///
    /// # Why the accumulator is fed here and only here
    ///
    /// These are precisely ARCHITECTURE C4's three "counts" call sites —
    /// `slide_move`'s bump loop, and `step_slide_move`'s up-lift and
    /// down-drop — and every one of C4's "does not count" sites goes through
    /// [`Self::sweep`] instead. Making that a property of *which method you
    /// call* rather than a table a future reader has to consult is the point:
    /// a new probe added with `sweep` is silently correct, and a new committed
    /// move added with `sweep_to` is silently correct.
    ///
    /// Three of the committed sites do not always commit the motion they
    /// swept — `slide_move` returns early on `all_solid` and skips the position
    /// write on a zero fraction; `step_slide_move`'s lift and drop are both
    /// conditional. None of them needs special handling here, because
    /// [`Trace::triggers`] reports the **traversed prefix** and `all_solid`
    /// implies a zero fraction: an abandoned move traverses nothing, and a
    /// zero-fraction bump reports only the volumes the player is standing in,
    /// which the previous committed sweep already reported. `TriggerSet` is
    /// OR-ed, so re-reporting is idempotent.
    ///
    /// The one case the traversed-prefix rule does *not* cover is the genuine
    /// rollback in [`Self::step_slide_move`]; see the savepoint there.
    fn sweep_to(&mut self, from: Vec3, to: Vec3) -> (Trace, Vec3) {
        let trace = self.sweep(from, to);
        self.touched = self.touched.with(trace.triggers);
        let motion = to - from;
        let mut fraction = trace.fraction;
        if trace.hit() && !trace.start_solid {
            // How fast the sweep closes on the plane, in fractions of the move.
            let closing = -motion.dot(trace.normal);
            if closing > s(0.0) {
                fraction = (fraction - SURFACE_CLIP_EPSILON / closing).max(s(0.0));
            }
        }
        (trace, from + motion * fraction)
    }

    /// Quake's `PM_GroundTrace`.
    fn ground_trace(&mut self, p: &mut PlayerState) {
        let down = p.origin - num::UP * self.profile.ground_trace_probe;
        let mut trace = self.sweep(p.origin, down);

        if trace.all_solid {
            match self.correct_all_solid(p) {
                Some(corrected) => trace = corrected,
                None => {
                    self.leave_ground(p);
                    return;
                }
            }
        }

        // Nothing underneath: free fall.
        if !trace.hit() {
            self.leave_ground(p);
            return;
        }

        // Being thrown off the ground — a jump, a jump pad, knockback — beats
        // whatever the probe found. Without this a jump would be cancelled on
        // the same command it was taken, because the player has not yet moved
        // clear of the floor.
        if p.velocity.z > s(0.0) && p.velocity.dot(trace.normal) > THROWN_OFF_GROUND_SPEED {
            self.leave_ground(p);
            return;
        }

        // Too steep to stand on: the plane is still there and velocity will be
        // clipped to it, but the player is not walking. This is ramp sliding.
        if trace.normal.z < self.profile.min_walk_normal {
            self.ground_plane = true;
            self.walking = false;
            self.ground_normal = trace.normal;
            self.ground_surface = trace.surface;
            p.ground = GroundState::Sliding {
                normal: trace.normal,
            };
            return;
        }

        let was_walking = p.ground.is_grounded();
        self.ground_plane = true;
        self.walking = true;
        self.ground_normal = trace.normal;
        self.ground_surface = trace.surface;
        p.ground = GroundState::Grounded {
            normal: trace.normal,
        };

        if !was_walking {
            // Just landed. Q3 runs `PM_CrashLand` here for fall damage, which
            // this project does not have (spec D3); what it does have is the
            // CPM double-jump window, which opens now and only if the landing
            // ended a jump.
            p.timers.since_landed_ms = 0;
            if p.left_ground_by_jumping {
                p.timers.double_jump_ms = self.profile.double_jump_window_ms;
                // The dash arms here, on the same landing, under the same
                // provenance rule, from a constant that is zero in canon. Two
                // windows opened by one event and competing for one input is
                // the point rather than an accident: on this landing the
                // player can spend the boost by jumping now, or keep the press
                // for a dash in the air, and they cannot have both from one
                // press. See `check_air_jump`.
                p.timers.dash_ms = self.profile.dash_window_ms;
            }
            p.left_ground_by_jumping = false;
        }
    }

    fn leave_ground(&mut self, p: &mut PlayerState) {
        self.ground_plane = false;
        self.walking = false;
        self.ground_normal = num::ZERO;
        self.ground_surface = SurfaceFlags::NONE;
        p.ground = GroundState::Airborne;
        // A slide is a ground technique, so leaving the ground ends it rather
        // than pausing it. Without this a player could tap crouch at speed,
        // jump immediately, and land with the remainder of a slide still owed
        // to them — a slide banked across a jump, which is a different and
        // much worse mechanic than the one being measured. Always zero under
        // canon, where nothing ever arms it.
        p.timers.slide_ms = 0;
    }

    /// Quake's `PM_CorrectAllSolid`: the player is inside geometry, so look for
    /// a nearby spot that is not and re-probe.
    ///
    /// Transcribed including the part that looks like a mistake — the jitter
    /// loop finds a free point but the second trace still starts from the
    /// unmoved origin. That is what the shipped source does, and "fixing" it
    /// would change where a stuck player ends up.
    fn correct_all_solid(&self, p: &PlayerState) -> Option<Trace> {
        for i in -1..=1 {
            for j in -1..=1 {
                for k in -1..=1 {
                    let point = p.origin + vec3(s(i as f32), s(j as f32), s(k as f32));
                    if !self.sweep(point, point).all_solid {
                        let down = p.origin - num::UP * self.profile.ground_trace_probe;
                        return Some(self.sweep(p.origin, down));
                    }
                }
            }
        }
        None
    }

    // ── input shaping ──────────────────────────────────────────────────────

    /// Quake's `PM_CheckDuck`.
    ///
    /// Crouching is not just a speed cap: it changes the hull for every trace
    /// this command makes, and standing up again is refused while something is
    /// in the way.
    fn check_duck(&mut self, p: &mut PlayerState) {
        // Before the write below, while `p.crouched` still holds the previous
        // command's value: see [`Self::crouch_edge`].
        self.crouch_edge = self.crouch_pressed && !p.crouched;
        if self.crouch_pressed {
            p.crouched = true;
        } else if p.crouched {
            // Try to stand: only allowed if the standing hull fits here.
            let standing = self.profile.hull(false);
            let probe = self.world.trace(&Sweep {
                start: p.origin,
                end: p.origin,
                half_extents: standing.half_extents,
                center_offset: standing.center_offset,
            });
            if !probe.all_solid {
                p.crouched = false;
            }
        }
        self.hull = self.profile.hull(p.crouched);
    }

    /// Quake's `PM_CmdScale`: how much of `max_speed` this command asks for.
    ///
    /// The shape matters. It is the *largest single axis* over the *length of
    /// all three*, so pressing two keys does not ask for `sqrt(2)` times the
    /// speed — it asks for the same speed, in a diagonal direction. Take this
    /// out and diagonal movement is 41% faster, which is the bug Quake 1 had.
    fn cmd_scale(&self) -> Scalar {
        let max = self
            .forward_move
            .abs()
            .max(self.right_move.abs())
            .max(self.up_move.abs());
        if max == s(0.0) {
            return s(0.0);
        }
        let total = (self.forward_move * self.forward_move
            + self.right_move * self.right_move
            + self.up_move * self.up_move)
            .sqrt();
        self.profile.max_speed * max / (CMD_AXIS_MAX * total)
    }

    // ── the movement pipeline ──────────────────────────────────────────────

    /// Quake's `PM_Friction`.
    ///
    /// Applied on every command, walking or not — but the ground-friction term
    /// is only added while walking on a non-slick surface, so in the air this
    /// reduces to "stop a player who is barely moving". That asymmetry is the
    /// reason airborne speed persists at all.
    fn friction(&self, p: &mut PlayerState) {
        let mut vec = p.velocity;
        if self.walking {
            vec.z = s(0.0); // ignore slope movement
        }
        let speed = vec.length();
        if speed < MIN_FRICTION_SPEED {
            p.velocity.x = s(0.0);
            p.velocity.y = s(0.0);
            return;
        }

        let mut drop = s(0.0);
        if self.walking
            && !self.ground_surface.contains(SurfaceFlags::SLICK)
            && p.timers.movement_locked_ms == 0
        {
            // `stop_speed` is a floor under the friction rate, not a speed
            // limit: below it a player decelerates as if moving at
            // `stop_speed`, which is what makes stopping crisp instead of
            // asymptotic.
            let control = if speed < self.profile.stop_speed {
                self.profile.stop_speed
            } else {
                speed
            };
            // The crouch slide, and the whole of its effect: the same
            // `PM_Friction`, reading a different number while the slide runs.
            // Not a second friction model and not a branch on the profile —
            // `slide_ms` can only be non-zero when a slide was armed, and a
            // slide can only be armed when `slide_duration_ms` is non-zero,
            // which is never in canon.
            //
            // The profile is consulted as well as the timer, following the
            // `strafe_wish_speed_cap` precedent below: with the mechanic off
            // the timer's value is meaningless, and a caller who hands the
            // mover a hand-built state should not be able to give a canon run
            // `slide_friction`'s zero.
            let rate = if self.profile.slide_duration_ms != 0 && p.timers.slide_ms > 0 {
                self.profile.slide_friction
            } else {
                self.profile.friction
            };
            drop += control * rate * self.dt;
        }

        let mut newspeed = speed - drop;
        if newspeed < s(0.0) {
            newspeed = s(0.0);
        }
        newspeed /= speed;
        p.velocity *= newspeed;
    }

    /// Quake's `PM_Accelerate` — the four lines the entire game is built on.
    ///
    /// The clamp is on `wishspeed - dot(velocity, wishdir)`: the *projection*
    /// of current velocity onto the direction being asked for, not the total
    /// speed. Move sideways relative to your velocity and that projection stays
    /// small, so acceleration keeps being granted however fast you are already
    /// going. Strafejumping is that sentence, and nothing else.
    fn accelerate(&self, p: &mut PlayerState, wishdir: Vec3, wishspeed: Scalar, accel: Scalar) {
        let currentspeed = p.velocity.dot(wishdir);
        let addspeed = wishspeed - currentspeed;
        if addspeed <= s(0.0) {
            return;
        }
        let mut accelspeed = accel * self.dt * wishspeed;
        if accelspeed > addspeed {
            accelspeed = addspeed;
        }
        p.velocity += wishdir * accelspeed;
    }

    /// CPM's `PM_Aircontrol`: steer the velocity vector without changing its
    /// length.
    ///
    /// Horizontal speed is taken out, the direction is nudged towards the wish
    /// direction by `k`, and the original speed is put back. Turning therefore
    /// costs nothing, which is the single largest difference in feel between
    /// CPM and VQ3. `dot²` in `k` means the effect falls away sharply as the
    /// wish direction diverges from where you are already going, so it steers
    /// rather than teleports.
    fn air_control(&self, p: &mut PlayerState, wishdir: Vec3, wishspeed: Scalar) {
        // Only available holding forward or back alone, exactly as CPM gates it
        // on `movementDir` being 0 or 4.
        if self.move_dir != MoveDir::ForwardBack || wishspeed == s(0.0) {
            return;
        }

        let zspeed = p.velocity.z;
        let flat = vec3(p.velocity.x, p.velocity.y, s(0.0));
        let (mut dir, speed) = normalize(flat);

        let dot = dir.dot(wishdir);
        let k = s(32.0) * self.profile.air_control * dot * dot * self.dt;

        if dot > s(0.0) {
            // Cannot change direction while slowing down: `air_stop_accelerate`
            // owns that case.
            dir = normalize(dir * speed + wishdir * k).0;
        }

        p.velocity.x = dir.x * speed;
        p.velocity.y = dir.y * speed;
        p.velocity.z = zspeed;
    }

    /// Quake's `PM_WalkMove`.
    fn walk_move(&mut self, p: &mut PlayerState) {
        if self.check_jump(p) {
            // Jumped away: the rest of this command is spent in the air.
            self.air_move(p);
            return;
        }

        // Before friction, so a slide armed this command is a slide *this*
        // command rather than one that starts a tick late.
        self.check_slide(p);
        self.friction(p);
        let scale = self.cmd_scale();

        // Project the view basis onto the ground plane, so that walking up a
        // ramp asks for a direction *along* the ramp rather than into it.
        let mut forward = self.forward;
        let mut right = self.right;
        forward.z = s(0.0);
        right.z = s(0.0);
        forward = clip_velocity(forward, self.ground_normal, self.profile);
        right = clip_velocity(right, self.ground_normal, self.profile);
        forward = normalize(forward).0;
        right = normalize(right).0;

        // Note the absence of `wishvel.z = 0`: on a slope the wish velocity has
        // a vertical component, and that is how walking up a ramp works.
        let wishvel = forward * self.forward_move + right * self.right_move;
        let (wishdir, len) = normalize(wishvel);
        let mut wishspeed = len * scale;

        if p.crouched {
            let ducked = self.profile.max_speed * self.profile.duck_scale;
            if wishspeed > ducked {
                wishspeed = ducked;
            }
        }

        // Slick ground and knockback both hand the player the air rules while
        // still standing on something — no friction above, no ground
        // acceleration here. That combination is the whole of ice physics.
        let slick =
            self.ground_surface.contains(SurfaceFlags::SLICK) || p.timers.movement_locked_ms != 0;
        let accel = if slick {
            self.profile.air_accelerate
        } else {
            self.profile.accelerate
        };
        self.accelerate(p, wishdir, wishspeed, accel);

        if slick {
            p.velocity.z -= self.profile.gravity * self.dt;
        }

        // Clip to the ground plane, then restore the speed that clipping took
        // away. This is Q3's "don't decrease velocity when going up or down a
        // slope", and it is *the* ramp-preservation mechanism: the direction
        // follows the surface, the magnitude does not care that the surface
        // tilted.
        let vel = p.velocity.length();
        p.velocity = clip_velocity(p.velocity, self.ground_normal, self.profile);
        p.velocity = normalize(p.velocity).0 * vel;

        if p.velocity.x == s(0.0) && p.velocity.y == s(0.0) {
            return;
        }

        self.step_slide_move(p, false);
    }

    /// Quake's `PM_AirMove`, with CPM's air rules layered on as data.
    fn air_move(&mut self, p: &mut PlayerState) {
        self.friction(p);
        let scale = self.cmd_scale();

        let (forward, _) = normalize(vec3(self.forward.x, self.forward.y, s(0.0)));
        let (right, _) = normalize(vec3(self.right.x, self.right.y, s(0.0)));

        let wishvel = vec3(
            forward.x * self.forward_move + right.x * self.right_move,
            forward.y * self.forward_move + right.y * self.right_move,
            s(0.0),
        );
        let (wishdir, len) = normalize(wishvel);
        let mut wishspeed = len * scale;

        // ── the CPM air model, expressed entirely in profile values ────────
        //
        // Each of the three below is switched off by a zero, and VQ3 sets all
        // three to zero. There is no profile test here and there must not be
        // one: these are the numbers the operator will tune.
        let wishspeed_before_cap = wishspeed;
        let mut accel = self.profile.air_accelerate;

        if self.profile.air_stop_accelerate != s(0.0) && p.velocity.dot(wishdir) < s(0.0) {
            accel = self.profile.air_stop_accelerate;
        }
        if self.profile.strafe_accelerate != s(0.0) && self.move_dir == MoveDir::Strafe {
            if wishspeed > self.profile.strafe_wish_speed_cap {
                wishspeed = self.profile.strafe_wish_speed_cap;
            }
            accel = self.profile.strafe_accelerate;
        }

        self.accelerate(p, wishdir, wishspeed, accel);

        if self.profile.air_control != s(0.0) {
            // Deliberately the *uncapped* wish speed: the cap above exists to
            // keep `PM_Accelerate` productive at speed, and reusing it here
            // would silently disable air control whenever a strafe key is held.
            self.air_control(p, wishdir, wishspeed_before_cap);
        }

        // A plane underfoot that is too steep to walk on still redirects
        // movement. This is the other half of ramp sliding: no friction was
        // applied above, and here the velocity is turned to follow the slope
        // instead of being stopped by it.
        if self.ground_plane {
            p.velocity = clip_velocity(p.velocity, self.ground_normal, self.profile);
        }

        // Last, and after the ground-plane clip, so that neither candidate's
        // impulse is immediately clipped away by a steep plane the player is
        // pushing off. Both are unreachable in canon: `wall_jump_velocity` and
        // `dash_speed` are zero there, so this call returns without reading
        // anything else.
        self.check_air_jump(p, wishdir);

        self.step_slide_move(p, true);
    }

    /// Quake's `PM_CheckJump`. Returns whether a jump was taken.
    fn check_jump(&mut self, p: &mut PlayerState) -> bool {
        if !self.jump_pressed {
            return false;
        }
        if p.jump_held {
            // The input is still down from the last jump. Q3 zeroes `upmove`
            // here so that holding jump does not quietly lower running speed
            // through `PM_CmdScale`.
            self.up_move = s(0.0);
            return false;
        }

        self.ground_plane = false;
        self.walking = false;
        p.jump_held = true;
        p.left_ground_by_jumping = true;
        p.ground = GroundState::Airborne;

        // Assignment, not addition — Q3 sets the vertical velocity outright, so
        // jumping while already rising does not accumulate.
        let mut up = self.profile.jump_velocity;
        if self.profile.double_jump_window_ms > 0 && p.timers.double_jump_ms > 0 {
            up += self.profile.double_jump_boost;
        }
        // Spent either way: a window buys exactly one boosted jump.
        p.timers.double_jump_ms = 0;
        p.velocity.z = up;
        p.timers.since_jumped_ms = 0;
        true
    }

    // ── candidate mechanics ────────────────────────────────────────────────
    //
    // # These three are candidates. None of them has been assessed.
    //
    // Crouch slide, dash and wall interaction exist here to be *measured*, not
    // because they have earned a place in the movement language. Spec rev 4
    // **defers criterion 5** — the written assessment of each against the
    // vision's five criteria, with `tools/straf3-lab`'s numbers as evidence —
    // to a later wave. Until that assessment exists, every one of them is
    // subject to being cut, and the fact that they compile, pass tests and sit
    // in the tree is **not** an argument that they should stay. This wave's
    // whole thesis is that movement decisions are argued from evidence rather
    // than inherited from whatever happened to get written first.
    //
    // They are reachable only from `PhysicsProfile::experimental`, which spec
    // D2 makes permanently incomparable to canon: its personal bests save under
    // `runs/<map>.experimental.s3d` and its recordings are refused by a canon
    // client through `PhysicsId`. Nothing below can reach the ranked game.
    //
    // # The bar they have to clear, and it is high
    //
    // Session A measured the existing vocabulary. Running at 300 ups off a
    // 508-unit drop settles permanently at 951 ups, and 600 ups entry gives
    // 1084 — the largest speed gain in the game already exists by accident, via
    // the rescale at the end of `walk_move`. Anything here justified as a
    // *source of speed* is competing with that and will lose.
    //
    // The gap that same measurement found is the opposite one: traversing a
    // ramp never gains speed, and costs `entry · cos(angle)` at the seam — 600
    // ups onto a 26° ramp arrives at 539.28. So speed-*preserving* geometry
    // interaction addresses a real hole in the vocabulary where a
    // speed-*granting* button does not. That asymmetry is the single most
    // useful thing to know about which of these three is likely to survive, and
    // it is why the wall jump pushes along a plane the player had to reach
    // rather than simply adding velocity on demand.

    /// The crouch slide: arm it if this command is a crouch press taken fast
    /// enough, on the ground.
    ///
    /// **Candidate, unassessed — see the module section above and spec rev 4
    /// criterion 5.**
    ///
    /// # Why there is no `slide_spent` flag
    ///
    /// The obvious failure mode of a slide is that it becomes a friction
    /// toggle: tap crouch, slide, tap again, slide again, and the "technique"
    /// is a key you hold down at speed. The obvious fix is a bit of state
    /// saying the slide has been used and does not recharge until you leave the
    /// ground.
    ///
    /// That bit is not here, deliberately, because
    /// [`PhysicsProfile`]'s own doctrine says a field is a number the
    /// simulation reads and not a switch — and the anti-chaining property is
    /// already *in the numbers*. [`PhysicsProfile::slide_entry_speed`] is set
    /// above [`PhysicsProfile::max_speed`] in `experimental`, so re-entry
    /// cannot be reached by ground acceleration; crouching caps wish speed at
    /// `max_speed * duck_scale`, so a sliding player cannot accelerate at all;
    /// and re-pressing crouch costs at least one command spent standing at full
    /// [`PhysicsProfile::friction`]. A chain therefore has to be *paid for* out
    /// of the speed the player is trying to keep, and how expensive that is is
    /// a number the operator can tune and the lab can measure. A flag would
    /// have made it a rule nobody can tune and a second source of truth the
    /// mover could read instead of the constants.
    ///
    /// If measurement shows the chain is cheap enough to be degenerate, the
    /// answer is a higher entry speed or a lower duration, not a bool.
    fn check_slide(&mut self, p: &mut PlayerState) {
        if self.profile.slide_duration_ms == 0 || !self.crouch_edge {
            return;
        }
        // Horizontal speed only: sliding is about the speed being carried
        // along the floor, and on a ramp the vertical component is a
        // consequence of the surface rather than something the player earned.
        let v = p.velocity;
        let speed = (v.x * v.x + v.y * v.y).sqrt();
        if speed < self.profile.slide_entry_speed {
            return;
        }
        p.timers.slide_ms = self.profile.slide_duration_ms;
    }

    /// A jump press while airborne: the wall jump, or the dash, or nothing.
    ///
    /// **Candidates, unassessed — see the module section above and spec rev 4
    /// criterion 5.**
    ///
    /// # Why both of them are a second jump press and not new buttons
    ///
    /// The vision asks for "a compact, universal input language" and says
    /// advanced behaviour should come from "timing, context, geometry, speed,
    /// and the interaction of mechanics rather than from a large ability bar or
    /// many isolated buttons". A dash key and a wall-jump key would be two
    /// isolated buttons. So neither exists: both are the jump input, pressed
    /// again in the air.
    ///
    /// This is not only cheaper, it is what gives the dash something to master.
    /// A jump press in the air is an input the player's bunnyhop rhythm is
    /// *already using* — spending it on a dash means not having it on the next
    /// landing, and `jump_held` means the press has to be released and retaken
    /// inside a window that a landing opened. A dash on its own key, available
    /// whenever the window is open, would be pressed every time it was
    /// available and would have no timing to master at all. That is the
    /// distinction spec rev 4's overbounce precedent draws between a mechanic
    /// that can survive being unreadable and one that cannot survive having
    /// nothing to master.
    ///
    /// (A dedicated bit would have cost nothing mechanically, which is worth
    /// recording since it is the kind of thing that gets re-investigated:
    /// `straf3-replay`'s codec writes `buttons` as a raw `u16` and reads it
    /// back unmasked, so a new bit needs no format version bump and
    /// invalidates no recording. The reason not to add one is the input
    /// language, not the format.)
    ///
    /// # Why geometry, not a modifier, chooses between them
    ///
    /// One press, two techniques, and which one fires depends on whether the
    /// player is against a wall. That is route choice falling out of the
    /// design rather than being claimed for it: the same input means "redirect
    /// off this surface" beside a wall and "redirect through the air"
    /// elsewhere, so *where the player is standing* is the decision. Wall
    /// contact wins when both are available, because it is the reading the
    /// player can see coming — they are touching the wall.
    ///
    /// # Why a dash cannot fire on the command that launched the jump
    ///
    /// `walk_move` calls `air_move` after a successful `check_jump`, so this
    /// runs on that command too — but `check_jump` has already set
    /// [`PlayerState::jump_held`], and the guard below refuses a held press.
    /// One press buys one thing. The two windows also live on opposite sides
    /// of the ground check: the double-jump boost is spent in `check_jump`,
    /// reachable only while walking, and both candidates here are reachable
    /// only while airborne, so they cannot both consume the same press.
    fn check_air_jump(&mut self, p: &mut PlayerState, wishdir: Vec3) {
        if !self.jump_pressed || p.jump_held {
            return;
        }

        // ── wall jump ─────────────────────────────────────────────────────
        if self.profile.wall_jump_velocity != s(0.0)
            && self.profile.wall_contact_window_ms != 0
            && p.timers.wall_contact_ms > 0
        {
            // The vertical part is the ordinary jump, so a wall jump is not a
            // better jump — `wall_jump_velocity` buys the horizontal redirect
            // and nothing else. Assignment for the same reason `check_jump`
            // assigns: jumping while already rising must not accumulate.
            p.velocity.z = self.profile.jump_velocity;
            p.velocity += p.wall_normal * self.profile.wall_jump_velocity;
            p.timers.wall_contact_ms = 0;
            p.timers.since_jumped_ms = 0;
            p.jump_held = true;
            // A wall jump is a jump: the landing it ends should arm the
            // double-jump and dash windows exactly as a floor jump's does.
            p.left_ground_by_jumping = true;
            return;
        }

        // ── dash ──────────────────────────────────────────────────────────
        if self.profile.dash_speed == s(0.0)
            || self.profile.dash_window_ms == 0
            || p.timers.dash_ms == 0
            || wishdir == num::ZERO
        {
            return;
        }

        // `PM_Accelerate`'s clamp, with no acceleration limit: bring the
        // projection of velocity onto the wish direction up to `dash_speed`,
        // and grant nothing if it is already there. This is what makes the
        // dash a redirect rather than a speed button — it is worth almost
        // nothing along a direction already travelled at 400 ups, and worth
        // its full value across one. Strafejumping is built on exactly this
        // clamp; see [`Self::accelerate`].
        let addspeed = self.profile.dash_speed - p.velocity.dot(wishdir);
        if addspeed <= s(0.0) {
            // Deliberately *not* spent. A dash that vanished because the
            // player was already moving that fast would be a punishment for
            // holding a direction, and unattributable — nothing on screen
            // would explain where the window went.
            return;
        }
        p.velocity += wishdir * addspeed;
        p.timers.dash_ms = 0;
        // Costs the input until it is released, exactly as a floor jump does.
        // Without this, holding jump would dash the instant a window opened,
        // which is "automation that replaces execution or timing" — a
        // confirmed anti-goal — rather than a technique.
        p.jump_held = true;
    }

    /// Record that the player is against a wall, if this plane is one.
    ///
    /// **Candidate, unassessed — see the module section above and spec rev 4
    /// criterion 5.**
    ///
    /// Called from the slide solver's bump loop, which is the only place the
    /// simulation learns it has touched a non-floor plane: `PM_GroundTrace`
    /// probes straight down and by construction never finds a wall. Nothing
    /// new is swept, no trace is added, and no canon number is read — this
    /// only copies a normal the solver already has into state that outlives
    /// the command, because a wall jump happens some commands after the touch
    /// and the solver's plane list dies with the command.
    ///
    /// # Why the magnitude of the normal's Z, not the signed value
    ///
    /// [`PhysicsProfile::wall_normal_max`] describes how far from horizontal a
    /// plane's normal may lean and still be a wall. A signed comparison would
    /// make every ceiling in the game a wall, since a ceiling's normal points
    /// down at −1.0 and −1.0 is comfortably "at or below" any threshold. A
    /// player bonking their head and being handed a wall jump is not the
    /// mechanic, so the test is on the magnitude.
    fn note_wall_contact(&self, p: &mut PlayerState, normal: Vec3) {
        if self.profile.wall_contact_window_ms == 0
            || self.profile.wall_jump_velocity == s(0.0)
            || normal.z.abs() > self.profile.wall_normal_max
        {
            return;
        }
        p.timers.wall_contact_ms = self.profile.wall_contact_window_ms;
        p.wall_normal = normal;
    }

    // ── the slide solver ───────────────────────────────────────────────────

    /// Quake's `PM_SlideMove`. Returns whether the move was interrupted.
    ///
    /// The player is moved by re-planning up to [`SLIDE_BUMPS`] times, each
    /// time clipping velocity to every plane touched so far. The plane set is
    /// bounded by [`PhysicsProfile::max_clip_planes`]; running out means
    /// stopping dead, which is what happens in a tight corner.
    fn slide_move(&mut self, p: &mut PlayerState, gravity: bool) -> bool {
        let mut primal_velocity = p.velocity;
        let mut end_velocity = num::ZERO;

        if gravity {
            end_velocity = p.velocity;
            end_velocity.z -= self.profile.gravity * self.dt;
            // Integrate at the *average* of the start and end vertical speeds.
            // This is what makes jump height nearly independent of the command
            // rate — nearly, not exactly, and the remainder is the rate-coupled
            // jump-height quirk the spec chose to keep.
            p.velocity.z = (p.velocity.z + end_velocity.z) * s(0.5);
            primal_velocity.z = end_velocity.z;
            if self.ground_plane {
                p.velocity = clip_velocity(p.velocity, self.ground_normal, self.profile);
            }
        }

        let mut time_left = self.dt;

        let max_planes = self.profile.max_clip_planes as usize;
        let mut planes: [Vec3; 8] = [num::ZERO; 8];
        let mut numplanes = 0usize;

        // Never turn against the ground plane, and never against the direction
        // we started in. Seeding the plane list with both is what stops the
        // solver reversing the player into a wall it has just left.
        if self.ground_plane {
            planes[0] = self.ground_normal;
            numplanes = 1;
        }
        if numplanes < planes.len() {
            planes[numplanes] = normalize(p.velocity).0;
            numplanes += 1;
        }

        let mut bumped = false;
        for _ in 0..SLIDE_BUMPS {
            let end = p.origin + p.velocity * time_left;
            let (trace, endpos) = self.sweep_to(p.origin, end);

            if trace.all_solid {
                // Completely trapped. Kill the fall so damage cannot build up,
                // but leave the horizontal velocity so the player can move out.
                p.velocity.z = s(0.0);
                return true;
            }

            if trace.fraction > s(0.0) {
                p.origin = endpos;
            }
            if !trace.hit() {
                break; // moved the whole way
            }
            bumped = true;
            self.note_wall_contact(p, trace.normal);

            time_left -= time_left * trace.fraction;

            if numplanes >= max_planes {
                // More simultaneous planes than the solver models. Q3 stops
                // dead rather than guessing, and so does this.
                p.velocity = num::ZERO;
                return true;
            }

            // Same plane as one already recorded: nudge out along it instead of
            // clipping again. Without this, non-axial planes trap the player in
            // a loop of clipping to the same surface at ever smaller fractions.
            let mut seen = false;
            for plane in &planes[..numplanes] {
                if trace.normal.dot(*plane) > SAME_PLANE_DOT {
                    p.velocity += trace.normal;
                    seen = true;
                    break;
                }
            }
            if seen {
                continue;
            }

            planes[numplanes] = trace.normal;
            numplanes += 1;

            // Find a plane the move enters, and make the velocity parallel to
            // every plane at once.
            for i in 0..numplanes {
                let into = p.velocity.dot(planes[i]);
                if into >= INTO_PLANE_EPSILON {
                    continue; // this move does not interact with that plane
                }

                let mut clip_v = clip_velocity(p.velocity, planes[i], self.profile);
                let mut end_clip_v = clip_velocity(end_velocity, planes[i], self.profile);

                let mut stopped = false;
                for j in 0..numplanes {
                    if j == i || clip_v.dot(planes[j]) >= INTO_PLANE_EPSILON {
                        continue;
                    }
                    clip_v = clip_velocity(clip_v, planes[j], self.profile);
                    end_clip_v = clip_velocity(end_clip_v, planes[j], self.profile);

                    if clip_v.dot(planes[i]) >= s(0.0) {
                        continue; // no longer heading into the first plane
                    }

                    // Two planes at once: the only direction that satisfies
                    // both is their crease. This is why sliding along an inside
                    // corner feels like being railed.
                    let dir = normalize(planes[i].cross(planes[j])).0;
                    clip_v = dir * dir.dot(p.velocity);
                    end_clip_v = dir * dir.dot(end_velocity);

                    for (k, plane) in planes.iter().enumerate().take(numplanes) {
                        if k == i || k == j || clip_v.dot(*plane) >= INTO_PLANE_EPSILON {
                            continue;
                        }
                        // Three planes: nowhere left to go.
                        p.velocity = num::ZERO;
                        stopped = true;
                        break;
                    }
                    if stopped {
                        break;
                    }
                }
                if stopped {
                    return true;
                }

                p.velocity = clip_v;
                end_velocity = end_clip_v;
                break;
            }
        }

        if gravity {
            p.velocity = end_velocity;
        }

        if p.timers.movement_locked_ms != 0 {
            // Under scripted movement — a jump pad, knockback — the solver may
            // reposition the player but must not eat the velocity that was
            // handed to them.
            p.velocity = primal_velocity;
        }

        bumped
    }

    /// Quake's `PM_StepSlideMove`: try the move again from one step higher and
    /// keep that result if it is legal.
    ///
    /// This is why Quake players walk up stairs without jumping and without the
    /// camera bobbing over each tread — the whole move is re-run from
    /// [`PhysicsProfile::step_height`] up, then dropped back down.
    fn step_slide_move(&mut self, p: &mut PlayerState, gravity: bool) {
        let start_o = p.origin;
        let start_v = p.velocity;
        // The savepoint for ARCHITECTURE C4's rule 2, captured exactly where
        // origin and velocity are. See where it is restored, below.
        let start_triggers = self.touched;

        if !self.slide_move(p, gravity) {
            return; // went exactly where we wanted on the first try
        }

        // Never step up while still rising, unless there is flat ground right
        // below — otherwise a jump into a wall would climb it.
        let down = start_o - num::UP * self.profile.step_height;
        let probe = self.sweep(start_o, down);
        if p.velocity.z > s(0.0) && (!probe.hit() || probe.normal.dot(num::UP) < STEP_MIN_NORMAL) {
            return;
        }

        let up = start_o + num::UP * self.profile.step_height;
        let (up_trace, up_pos) = self.sweep_to(start_o, up);
        if up_trace.all_solid {
            // No headroom: the first attempt's result stands, and so must its
            // triggers. Rolling back here would be a bug — the player really
            // did move along that path. The early return and the successful
            // lift look symmetric and are not, which is why the traversed-prefix
            // rule rather than a second savepoint is what makes the aborted lift
            // safe: `all_solid` implies a zero fraction, so the lift above
            // traversed nothing and contributed nothing.
            return;
        }

        let step_size = up_pos.z - start_o.z;
        p.origin = up_pos;
        p.velocity = start_v;
        // Rule 2 — the one genuine rollback. The first `slide_move` really did
        // traverse its path, and only now is it discarded: origin is overwritten
        // with the stepped-up position and velocity with the pre-attempt value.
        // Triggers accumulated during a traversal that is subsequently un-done
        // must be un-done with it, so the accumulator is restored alongside
        // exactly those two writes.
        //
        // Deliberate refinement over the code sketch in ARCHITECTURE C4, which
        // restores the bare savepoint here. That would also discard the lift's
        // own contribution — and the lift is not part of the abandoned attempt:
        // the hull really is carried from `start_o` to `up_pos` and stays there.
        // C4's governing invariant is "a trigger is touched iff the player's
        // hull overlapped it somewhere on the path the player actually
        // occupied", and the lift is such a path, so its volumes are kept while
        // the discarded attempt's are dropped. The two differ only when a volume
        // sits within `step_height` directly overhead and was not on the first
        // attempt's path; in that case the sketch under-reports.
        self.touched = start_triggers.with(up_trace.triggers);

        self.slide_move(p, gravity);

        // Put the player back down the amount we lifted them.
        let down = p.origin - num::UP * step_size;
        let (down_trace, down_pos) = self.sweep_to(p.origin, down);
        if !down_trace.all_solid {
            p.origin = down_pos;
        }
        if down_trace.hit() {
            p.velocity = clip_velocity(p.velocity, down_trace.normal, self.profile);
        }
    }
}

/// Quake's `AngleVectors`, returning the forward and right basis vectors.
///
/// Up is not returned because the movement code never uses it. Note that
/// Quake's "right" points along -Y at yaw 0, which is not a mistake to be
/// tidied — the sign is baked into every recorded `right_move`.
///
/// These are the only transcendental calls in the simulation, and they go
/// through [`num::sin_cos`] rather than `f32::sin_cos` because std's is
/// whichever libm the target links, and those disagree in the last bit — a
/// browser recording stops matching a glibc server after about 14 seconds of
/// play. `cargo xtask check-seam` fails the build if a `.sin_cos()` is written
/// here again.
fn angle_vectors(pitch: Scalar, yaw: Scalar, roll: Scalar) -> (Vec3, Vec3) {
    let (sy, cy) = num::sin_cos(yaw * DEG_TO_RAD);
    let (sp, cp) = num::sin_cos(pitch * DEG_TO_RAD);
    let (sr, cr) = num::sin_cos(roll * DEG_TO_RAD);

    let forward = vec3(cp * cy, cp * sy, -sp);
    let right = vec3(-sr * sp * cy + cr * sy, -sr * sp * sy - cr * cy, -sr * cp);
    (forward, right)
}

/// Quake's `VectorNormalize`: the unit vector and the original length.
///
/// Written out rather than delegated to `glam` because Q3 divides by the length
/// once and multiplies, and because a zero vector must come back unchanged
/// rather than as NaN — the movement code normalises the wish velocity on every
/// command, and standing still is not an error.
fn normalize(v: Vec3) -> (Vec3, Scalar) {
    let length = (v.x * v.x + v.y * v.y + v.z * v.z).sqrt();
    if length == s(0.0) {
        return (v, length);
    }
    (v * (s(1.0) / length), length)
}

/// Quake's `PM_ClipVelocity`: remove the component of `velocity` heading into
/// a plane, and then a little more.
///
/// The "little more" is [`PhysicsProfile::overclip`], and it is not rounding
/// slack to be tidied away — pushing slightly out of the plane rather than
/// exactly onto it is what produces overbounce and ramp boosts. Changing this
/// changes the game.
fn clip_velocity(velocity: Vec3, normal: Vec3, profile: &PhysicsProfile) -> Vec3 {
    let mut backoff = velocity.dot(normal);
    backoff = if backoff < s(0.0) {
        backoff * profile.overclip
    } else {
        backoff / profile.overclip
    };
    velocity - normal * backoff
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cmd::{TickRate, ViewAngles};
    use crate::world::{EmptyWorld, FlatGround};

    fn cmd(ms: u16) -> UserCmd {
        UserCmd::still(ms)
    }

    fn on_ground() -> SimState {
        let mut st = SimState::spawned_at(vec3(s(0.0), s(0.0), s(24.0)), s(0.0));
        st.player.ground = GroundState::Grounded {
            normal: vec3(s(0.0), s(0.0), s(1.0)),
        };
        st
    }

    #[test]
    fn a_zero_length_command_changes_nothing() {
        let before = SimState::spawned_at(vec3(s(0.0), s(0.0), s(64.0)), s(0.0));
        let after = step(&before, &cmd(0), &EmptyWorld, &PhysicsProfile::default());
        assert_eq!(before.checksum(), after.checksum());
    }

    #[test]
    fn time_is_the_exact_sum_of_command_durations() {
        let rate = TickRate::HZ_76; // 13 ms: the rate whose ms does not divide evenly
        let cmds = vec![UserCmd::still_at(rate); 1000];
        let end = run(
            &SimState::default(),
            &cmds,
            &EmptyWorld,
            &PhysicsProfile::default(),
        );
        assert_eq!(end.tick, 1000);
        assert_eq!(end.time_ms, 13_000); // exact, no float drift
    }

    #[test]
    fn the_tick_rate_changes_the_simulation() {
        // The point of D2: 250 commands of 4 ms and 125 of 8 ms cover the same
        // wall-clock second but are not the same simulation, because gravity
        // is applied per command.
        let spawn = SimState::spawned_at(vec3(s(0.0), s(0.0), s(1000.0)), s(0.0));
        let profile = PhysicsProfile::default();

        let at_125 = run(&spawn, &vec![cmd(8); 125], &EmptyWorld, &profile);
        let at_250 = run(&spawn, &vec![cmd(4); 250], &EmptyWorld, &profile);

        assert_eq!(at_125.time_ms, at_250.time_ms);
        assert_ne!(at_125.checksum(), at_250.checksum());
    }

    #[test]
    fn gravity_pulls_a_player_down_onto_ground_and_stops_there() {
        let ground = FlatGround::at(s(0.0));
        let profile = PhysicsProfile::default();
        let spawn = SimState::spawned_at(vec3(s(0.0), s(0.0), s(100.0)), s(0.0));

        let end = run(&spawn, &vec![cmd(8); 400], &ground, &profile);
        assert!(end.player.ground.is_grounded(), "should have landed");
        // Quake's origin is not at the feet: a standing player resting on a
        // floor at z=0 sits at z = -hull_mins.z = 24, plus the clip epsilon the
        // hull is held clear of the surface by.
        let expected = -profile.hull_mins.z + SURFACE_CLIP_EPSILON;
        assert!(
            (end.player.origin.z - expected).abs() < s(0.01),
            "came to rest at {}, expected about {expected}",
            end.player.origin.z
        );
        assert!(end.player.velocity.length() < s(1.0), "should be at rest");
    }

    #[test]
    fn the_step_function_does_not_mutate_its_input() {
        let before = SimState::spawned_at(vec3(s(0.0), s(0.0), s(64.0)), s(0.0));
        let snapshot = before;
        let _ = step(&before, &cmd(8), &EmptyWorld, &PhysicsProfile::default());
        assert_eq!(before.checksum(), snapshot.checksum());
    }

    #[test]
    fn clip_velocity_pushes_slightly_out_of_the_plane() {
        let profile = PhysicsProfile::vq3();
        let v = vec3(s(0.0), s(0.0), s(-100.0));
        let clipped = clip_velocity(v, num::UP, &profile);
        // Overclip means the result is not merely zeroed against the plane:
        // it retains a small outward component. This is the mechanism behind
        // overbounce, so it is asserted rather than assumed.
        assert!(
            clipped.z > s(0.0),
            "expected outward push, got {}",
            clipped.z
        );
        // And the size of the push is exactly the overclip excess.
        assert!((clipped.z - s(100.0) * s(0.001)).abs() < s(0.001));
    }

    #[test]
    fn the_view_basis_matches_quakes_angle_vectors() {
        let (f, r) = angle_vectors(s(0.0), s(0.0), s(0.0));
        assert!((f - vec3(s(1.0), s(0.0), s(0.0))).length() < s(1e-6));
        // Quake's right vector points along -Y at yaw 0.
        assert!((r - vec3(s(0.0), s(-1.0), s(0.0))).length() < s(1e-6));

        let (f, r) = angle_vectors(s(0.0), s(90.0), s(0.0));
        assert!((f - vec3(s(0.0), s(1.0), s(0.0))).length() < s(1e-6));
        assert!((r - vec3(s(1.0), s(0.0), s(0.0))).length() < s(1e-6));

        // Pitch is inverted, as in Quake: looking "up" is negative pitch.
        let (f, _) = angle_vectors(s(-90.0), s(0.0), s(0.0));
        assert!(f.z > s(0.99));
    }

    #[test]
    fn cmd_scale_does_not_reward_pressing_two_keys() {
        // The Quake 1 diagonal-speed bug, absent by construction.
        let profile = PhysicsProfile::vq3();
        let one_key = UserCmd {
            forward_move: 127,
            ..cmd(8)
        };
        let two_keys = UserCmd {
            forward_move: 127,
            right_move: 127,
            ..cmd(8)
        };
        let scale = |c: &UserCmd| Pmove::new(c, &EmptyWorld, &profile, s(0.008)).cmd_scale();

        // One key asks for the full 320.
        assert!((scale(&one_key) * s(127.0) - s(320.0)).abs() < s(0.01));
        // Two keys ask for 320/sqrt(2) *per axis*, so the resulting diagonal
        // wish speed is also 320 — not 452.
        assert!(
            (scale(&two_keys) * s(127.0) * s(2.0f32).sqrt() - s(320.0)).abs() < s(0.01),
            "diagonal asks for {}",
            scale(&two_keys) * s(127.0) * s(2.0f32).sqrt()
        );
    }

    #[test]
    fn friction_matches_the_quake_formula_exactly() {
        // v' = v - max(v, stop_speed) * friction * dt, applied to a walking
        // player. At 300 ups over 8 ms: 300 - 300*6*0.008 = 285.6.
        let profile = PhysicsProfile::vq3();
        let mut st = on_ground();
        st.player.velocity = vec3(s(300.0), s(0.0), s(0.0));
        let end = step(&st, &cmd(8), &FlatGround::at(s(0.0)), &profile);
        assert!(
            (end.player.velocity.x - s(285.6)).abs() < s(0.05),
            "got {}",
            end.player.velocity.x
        );

        // Below stop_speed the rate is held at stop_speed:
        // 50 - 100*6*0.008 = 45.2.
        let mut st = on_ground();
        st.player.velocity = vec3(s(50.0), s(0.0), s(0.0));
        let end = step(&st, &cmd(8), &FlatGround::at(s(0.0)), &profile);
        assert!(
            (end.player.velocity.x - s(45.2)).abs() < s(0.05),
            "got {}",
            end.player.velocity.x
        );
    }

    #[test]
    fn a_jump_sets_exactly_the_gpl_jump_velocity() {
        let profile = PhysicsProfile::vq3();
        let jump = UserCmd {
            buttons: Buttons::JUMP,
            ..cmd(8)
        };
        let end = step(&on_ground(), &jump, &FlatGround::at(s(0.0)), &profile);
        assert!(!end.player.ground.is_grounded());
        assert!(end.player.jump_held);
        // 270 up, minus one full frame of gravity: the solver moves the player
        // by the *average* of the start and end vertical speeds, but the
        // velocity it leaves behind is the end one.
        let expected = s(270.0) - s(800.0) * s(0.008);
        assert!(
            (end.player.velocity.z - expected).abs() < s(0.01),
            "got {}",
            end.player.velocity.z
        );
        // And the position moved by the average, not the end speed.
        let travelled = end.player.origin.z - s(24.0);
        let average = (s(270.0) + expected) * s(0.5) * s(0.008);
        assert!(
            (travelled - average).abs() < s(0.01),
            "rose {travelled}, expected {average}"
        );
    }

    #[test]
    fn holding_jump_does_not_re_trigger_it() {
        let profile = PhysicsProfile::vq3();
        let ground = FlatGround::at(s(0.0));
        let jump = UserCmd {
            buttons: Buttons::JUMP,
            ..cmd(8)
        };
        // Hold jump for a full second: the player must land and stay down,
        // because the input never went back up.
        let held = run(&on_ground(), &vec![jump; 125], &ground, &profile);
        assert!(held.player.ground.is_grounded(), "should have landed");

        // Releasing and pressing again does jump.
        let mut cmds = vec![jump; 125];
        cmds.extend(vec![cmd(8); 2]);
        cmds.push(jump);
        let released = run(&on_ground(), &cmds, &ground, &profile);
        assert!(!released.player.ground.is_grounded(), "should be airborne");
    }

    // ── candidate mechanics ────────────────────────────────────────────────
    //
    // These pin the *shape* of three unassessed candidates (spec rev 4
    // criterion 5), not their tuning. Each asserts what the mechanic does and,
    // more importantly, that canon cannot reach it — the second half is the
    // one that matters this wave, and it is asserted here as well as in
    // `canon_frozen.rs` because a test beside the code is the one a future
    // editor of that code actually runs.

    fn crouch(ms: u16) -> UserCmd {
        UserCmd {
            buttons: Buttons::CROUCH,
            ..cmd(ms)
        }
    }

    /// Moving along +X at `speed`, standing on flat ground.
    fn sprinting(speed: f32) -> SimState {
        let mut st = on_ground();
        st.player.velocity = vec3(s(speed), s(0.0), s(0.0));
        st
    }

    #[test]
    fn a_slide_starts_only_above_the_entry_speed_and_only_on_the_press() {
        let p = PhysicsProfile::experimental();
        let ground = FlatGround::at(s(0.0));
        assert_eq!(p.slide_entry_speed, s(400.0));

        // Below the entry speed: crouching is just crouching.
        let slow = step(&sprinting(399.0), &crouch(8), &ground, &p);
        assert_eq!(slow.player.timers.slide_ms, 0);
        assert!(slow.player.crouched, "still ducked, just not sliding");

        // Above it: armed to the full duration on the press itself.
        //
        // The full duration and not `duration - 8`: `PM_DropTimers` runs
        // before `walk_move` in `Pmove::run`, so the arming command's own
        // 8 ms is not deducted from the window it opens. The arming command
        // does slide — `check_slide` is called before `friction` — so a slide
        // is its duration *plus* the command that started it, which is one
        // tick and is the honest reading of "the press begins the slide".
        let fast = step(&sprinting(500.0), &crouch(8), &ground, &p);
        assert_eq!(fast.player.timers.slide_ms, p.slide_duration_ms);

        // Holding crouch does not re-arm. Advance past the far side of the
        // duration with the button never released, and the slide is over.
        // 600 ms is 75 commands at 8 ms, so 80 clears it.
        let held = run(&sprinting(500.0), &vec![crouch(8); 80], &ground, &p);
        assert!(
            held.player.crouched,
            "still holding crouch, so the test is about re-arming and not about standing up"
        );
        assert_eq!(
            held.player.timers.slide_ms, 0,
            "a held crouch re-armed the slide, which makes it a posture rather than a technique"
        );
    }

    #[test]
    fn a_slide_carries_speed_that_walking_crouched_would_have_lost() {
        let p = PhysicsProfile::experimental();
        let ground = FlatGround::at(s(0.0));
        let speed_after = |profile: &PhysicsProfile| {
            let end = run(&sprinting(500.0), &vec![crouch(8); 60], &ground, profile);
            end.player.velocity.x
        };

        let sliding = speed_after(&p);
        // The same commands under a profile whose only difference is that the
        // mechanic is off. Not `cpm()`: that also differs in nothing else
        // here, but going through the constant makes the claim "this is the
        // slide" rather than "this is some difference between two profiles".
        let disabled = speed_after(&PhysicsProfile {
            slide_duration_ms: 0,
            ..p
        });

        assert!(
            sliding > disabled * s(3.0),
            "slide kept {sliding} ups where ordinary friction kept {disabled}; \
             the mechanic is not doing anything worth measuring"
        );
        // And it is not frictionless: a slide that never ends is a toggle.
        assert!(sliding < s(500.0), "slide gained speed, got {sliding}");
    }

    /// The anti-chaining property, which lives in the constants rather than in
    /// a `slide_spent` flag. See `check_slide`.
    #[test]
    fn a_slide_cannot_be_re_entered_without_paying_for_it() {
        let p = PhysicsProfile::experimental();
        let ground = FlatGround::at(s(0.0));

        // Slide the whole 600 ms out (75 commands at 8 ms), release crouch for
        // one command, press again. Entry is above `max_speed` and a crouched
        // player cannot accelerate, so by the time the button comes back the
        // speed that bought the first slide is gone.
        let mut cmds = vec![crouch(8); 76];
        cmds.push(cmd(8)); // release
        cmds.push(crouch(8)); // re-press
        let end = run(&sprinting(500.0), &cmds, &ground, &p);

        let speed = end.player.velocity.x;
        assert!(
            speed < p.slide_entry_speed,
            "still at {speed} ups after a full slide, so a re-press re-slides \
             and the mechanic is a friction toggle"
        );
        assert_eq!(end.player.timers.slide_ms, 0, "the chain was not paid for");
    }

    #[test]
    fn a_dash_is_a_second_jump_press_in_the_air_and_costs_the_press() {
        let p = PhysicsProfile::experimental();
        let ground = FlatGround::at(s(0.0));
        let jump = UserCmd {
            buttons: Buttons::JUMP,
            forward_move: 127,
            ..cmd(8)
        };
        let forward = UserCmd {
            forward_move: 127,
            ..cmd(8)
        };

        // Jump, then step until the landing rather than for a fixed count:
        // the window opens *on* the landing command and starts counting down
        // immediately, so a run of arbitrary length would be asserting about
        // how long ago it landed rather than about the arming.
        let mut landed = step(&sprinting(320.0), &jump, &ground, &p);
        assert!(!landed.player.ground.is_grounded(), "premise: jumped");
        let mut commands = 0;
        while !landed.player.ground.is_grounded() {
            landed = step(&landed, &forward, &ground, &p);
            commands += 1;
            assert!(commands < 200, "never came back down");
        }
        assert_eq!(
            landed.player.timers.dash_ms, p.dash_window_ms,
            "the landing that ended a jump did not arm a dash"
        );
        assert_eq!(
            landed.player.timers.double_jump_ms, p.double_jump_window_ms,
            "the same landing arms both windows; that is what they compete over"
        );

        // Jump again out of that window, then press jump a second time in the
        // air with the input released in between.
        let airborne = run(&landed, &[jump, forward], &ground, &p);
        assert!(!airborne.player.ground.is_grounded());
        assert!(
            airborne.player.timers.dash_ms > 0,
            "the window closed before the dash could be spent"
        );
        let before = airborne.player.velocity;

        let dashed = step(&airborne, &jump, &ground, &p);
        assert_eq!(dashed.player.timers.dash_ms, 0, "the dash was not spent");
        assert!(
            dashed.player.velocity.x > before.x,
            "dash added nothing: {} -> {}",
            before.x,
            dashed.player.velocity.x
        );
        assert!(dashed.player.jump_held, "the dash did not cost the press");
    }

    /// The anti-goal check: "automation that replaces execution or timing".
    #[test]
    fn holding_jump_cannot_dash() {
        let p = PhysicsProfile::experimental();
        let ground = FlatGround::at(s(0.0));
        let jump = UserCmd {
            buttons: Buttons::JUMP,
            forward_move: 127,
            ..cmd(8)
        };

        // Jump and never let go, for long enough to land and re-arm several
        // times. If holding the button could dash, the window would be spent
        // the instant a landing opened it.
        let cmds = vec![jump; 300];
        let held = run(&sprinting(320.0), &cmds, &ground, &p);

        // The decisive comparison: the same commands under a profile whose
        // only difference is that the dash is off. Bit-identical means no dash
        // fired anywhere in 300 commands — which is a far stronger statement
        // than "the window looks unspent at the end", since a dash could have
        // fired and re-armed between two samples.
        let dashless = run(
            &sprinting(320.0),
            &cmds,
            &ground,
            &PhysicsProfile {
                dash_speed: s(0.0),
                ..p
            },
        );
        assert_eq!(
            held.checksum(),
            dashless.checksum(),
            "a held jump button dashed; that is automation replacing timing"
        );

        // And the premise is real: the run did land and did arm windows, so
        // the comparison above is not passing because nothing ever happened.
        assert!(
            held.player.timers.since_landed_ms < 2000,
            "premise: the run should have landed at some point"
        );
        assert!(held.player.jump_held, "premise: the input was held throughout");
    }

    #[test]
    fn a_dash_grants_nothing_along_a_direction_already_travelled_at_speed() {
        // The clamp is the mechanic: `dash_speed` is a wish speed, so a player
        // already moving faster than it along the wish direction gets nothing,
        // and — deliberately — is not charged the window for it.
        let p = PhysicsProfile::experimental();
        let ground = FlatGround::at(s(0.0));
        let mut st = SimState::spawned_at(vec3(s(0.0), s(0.0), s(200.0)), s(0.0));
        st.player.velocity = vec3(s(900.0), s(0.0), s(0.0));
        st.player.timers.dash_ms = p.dash_window_ms;

        let jump_forward = UserCmd {
            buttons: Buttons::JUMP,
            forward_move: 127,
            ..cmd(8)
        };
        let end = step(&st, &jump_forward, &ground, &p);
        assert_eq!(
            end.player.timers.dash_ms,
            p.dash_window_ms - 8,
            "the window was spent on a dash that did nothing"
        );
    }

    #[test]
    fn a_wall_jump_needs_a_wall_and_pushes_off_it() {
        let p = PhysicsProfile::experimental();
        let ground = FlatGround::at(s(0.0));
        let jump = UserCmd {
            buttons: Buttons::JUMP,
            ..cmd(8)
        };

        // Airborne with wall contact recorded: a normal pointing along +Y,
        // which is what the solver would have stored for a wall on the left.
        let mut st = SimState::spawned_at(vec3(s(0.0), s(0.0), s(200.0)), s(0.0));
        st.player.velocity = vec3(s(600.0), s(0.0), s(-100.0));
        st.player.timers.wall_contact_ms = p.wall_contact_window_ms;
        st.player.wall_normal = vec3(s(0.0), s(1.0), s(0.0));

        let end = step(&st, &jump, &ground, &p);
        assert!(
            end.player.velocity.y > s(150.0),
            "no push along the wall normal, y = {}",
            end.player.velocity.y
        );
        assert!(
            end.player.velocity.z > s(200.0),
            "a wall jump should also jump, z = {}",
            end.player.velocity.z
        );
        assert_eq!(end.player.timers.wall_contact_ms, 0, "contact not spent");
        assert!(
            end.player.left_ground_by_jumping,
            "a wall jump is a jump, so the landing it ends should arm the windows"
        );

        // Without the contact, the same press does nothing at all.
        let mut no_wall = st;
        no_wall.player.timers.wall_contact_ms = 0;
        let end = step(&no_wall, &jump, &ground, &p);
        assert!(end.player.velocity.y.abs() < s(1.0), "pushed off nothing");
    }

    /// A ceiling is not a wall.
    ///
    /// `wall_normal_max` bounds how far from horizontal a wall's normal may
    /// lean. Compared signed rather than by magnitude, a ceiling's normal of
    /// −1.0 satisfies "at or below 0.3" trivially, and bonking your head would
    /// hand you a wall jump. See `note_wall_contact`.
    #[test]
    fn the_wall_test_is_on_the_magnitude_of_the_normals_z() {
        let p = PhysicsProfile::experimental();
        let up = vec3(s(0.0), s(0.0), s(1.0));
        let down = vec3(s(0.0), s(0.0), s(-1.0));
        let side = vec3(s(0.0), s(1.0), s(0.0));

        assert!(down.z < p.wall_normal_max, "premise: a signed test admits it");
        assert!(down.z.abs() > p.wall_normal_max, "a ceiling is not a wall");
        assert!(up.z.abs() > p.wall_normal_max, "a floor is not a wall");
        assert!(side.z.abs() <= p.wall_normal_max, "a wall is a wall");
        // And the band between `wall_normal_max` and `min_walk_normal` is
        // deliberate: a steep ramp is neither walkable nor pushable.
        assert!(p.wall_normal_max < p.min_walk_normal);
    }

    /// The whole canon claim, asserted next to the code that could break it.
    ///
    /// `canon_frozen.rs` proves this over fourteen recorded runs in six
    /// worlds, which is the real gate. This is the cheap local version that
    /// fails in the same edit-compile cycle as the change that broke it.
    #[test]
    fn no_candidate_mechanic_is_reachable_under_canon() {
        let ground = FlatGround::at(s(0.0));
        for profile in [PhysicsProfile::vq3(), PhysicsProfile::cpm()] {
            // Every input that would invoke a candidate, in one run: crouch at
            // a speed well above any plausible entry, jump, and jump again in
            // the air with releases in between.
            let mut cmds = Vec::new();
            for i in 0..300 {
                cmds.push(UserCmd {
                    buttons: match i % 3 {
                        0 => Buttons::CROUCH,
                        1 => Buttons::JUMP,
                        _ => Buttons::NONE,
                    },
                    forward_move: 127,
                    ..cmd(8)
                });
            }
            let end = run(&sprinting(900.0), &cmds, &ground, &profile);

            assert_eq!(end.player.timers.slide_ms, 0);
            assert_eq!(end.player.timers.dash_ms, 0);
            assert_eq!(end.player.timers.wall_contact_ms, 0);
            assert_eq!(end.player.wall_normal, num::ZERO);
        }
    }

    // ── sub-stepping: Q3's `Pmove` loop ────────────────────────────────────
    //
    // These pin the split rule, its bound, and the two places the split is
    // visible in movement rather than in arithmetic: the jump window a long
    // command used to step over, and the press it must still only spend once.
    //
    // The claim they do *not* make, because a unit test cannot: that canon at
    // 125 Hz is unchanged. That is a statement about fourteen recorded runs
    // and four build targets, and it belongs to `canon_frozen.rs` and
    // `cargo xtask determinism`. What is asserted here is the mechanism those
    // two rest on — that a command at or below the bound is one sub-step.

    /// A command with the keys down and the view off-axis, so the trajectory
    /// bends. An equality between two integrations of a straight line would
    /// be nearly free; this one has friction, acceleration, a landing and a
    /// turn in it.
    fn busy(ms: u16, buttons: Buttons) -> UserCmd {
        UserCmd {
            duration_ms: ms,
            forward_move: 127,
            right_move: 127,
            up_move: 0,
            buttons,
            view: ViewAngles::from_degrees(s(0.0), s(37.5), s(0.0)),
        }
    }

    /// Everything a run is *except* `tick` — which counts commands, and so is
    /// the one field that must differ between a chopped command and the
    /// commands it was chopped into.
    ///
    /// A checksum rather than `PartialEq` so the comparison is on bits: `==`
    /// on `f32` cannot tell `0.0` from `-0.0`, and a sub-step boundary is
    /// exactly the sort of place a sign of zero changes.
    fn digest_ignoring_tick(state: &SimState) -> u64 {
        let mut state = *state;
        state.tick = 0;
        state.checksum()
    }

    /// Integrate one run of commands of the given durations, all carrying the
    /// same input, from a player running along the ground.
    ///
    /// **On the ground on purpose**, and the first draft of these tests was
    /// wrong to put the player in the air: airborne motion turns out to be
    /// step-size invariant in this mover, so every inequality below passed
    /// vacuously. See
    /// `ballistic_motion_is_the_same_however_finely_it_is_stepped`, which now
    /// pins that as the finding it is. Ground friction is the nearest regime
    /// that genuinely depends on the step size, because it decays velocity
    /// multiplicatively.
    fn over_holding(buttons: Buttons, durations: &[u16]) -> SimState {
        let mut start = sprinting(400.0);
        start.player.origin = vec3(s(0.0), s(0.0), s(24.125));
        let cmds: Vec<UserCmd> = durations.iter().map(|ms| busy(*ms, buttons)).collect();
        run(
            &start,
            &cmds,
            &FlatGround::at(s(0.0)),
            &PhysicsProfile::cpm(),
        )
    }

    fn over(durations: &[u16]) -> SimState {
        over_holding(Buttons::NONE, durations)
    }

    #[test]
    fn the_sub_step_bound_is_ids_and_no_rate_this_project_runs_at_reaches_it() {
        // id's unconditional cap in `Pmove`, verbatim.
        assert_eq!(PMOVE_SUBSTEP_MAX_MS, 66);

        // Which is why canon does not move: every rate this project names is
        // one sub-step, so the loop runs exactly once and integrates exactly
        // what a single step integrated.
        for rate in [TickRate::HZ_76, TickRate::HZ_125, TickRate::HZ_250] {
            assert!(
                rate.command_millis() <= PMOVE_SUBSTEP_MAX_MS,
                "{} Hz commands are {} ms, past the bound",
                rate.hz(),
                rate.command_millis(),
            );
        }

        // And the bound is not decorative: `TickRate` accepts rates whose
        // commands are far longer than it, and a caller may hand `step` any
        // `u16` at all.
        let slowest = TickRate::from_hz(1).expect("1 Hz is in range");
        assert!(slowest.command_millis() > PMOVE_SUBSTEP_MAX_MS);
    }

    #[test]
    fn a_command_is_exactly_the_sub_steps_it_is_chopped_into() {
        // The property the whole design turns on: chopping is not an
        // approximation of the long command, it *is* the long command. Two
        // sub-steps of the bound, and a bound plus a remainder.
        assert_eq!(
            digest_ignoring_tick(&over(&[132])),
            digest_ignoring_tick(&over(&[66, 66])),
        );
        assert_eq!(
            digest_ignoring_tick(&over(&[67])),
            digest_ignoring_tick(&over(&[66, 1])),
        );
        assert_eq!(
            digest_ignoring_tick(&over(&[100])),
            digest_ignoring_tick(&over(&[66, 34])),
        );

        // A second of wall time in one command: fifteen full sub-steps and a
        // ten. Spelled out rather than generated, because the point is the
        // exact sequence.
        let mut chopped = vec![PMOVE_SUBSTEP_MAX_MS; 15];
        chopped.push(1000 - 15 * PMOVE_SUBSTEP_MAX_MS);
        assert_eq!(chopped.iter().map(|ms| u32::from(*ms)).sum::<u32>(), 1000);
        assert_eq!(
            digest_ignoring_tick(&over(&[1000])),
            digest_ignoring_tick(&over(&chopped)),
        );

        // `tick` is the exception, and deliberately so: a recording's command
        // count must not depend on how the mover chose to integrate it.
        assert_eq!(over(&[1000]).tick, 1);
        assert_eq!(over(&chopped).tick, 16);
        assert_eq!(over(&[1000]).time_ms, over(&chopped).time_ms);
    }

    /// The same claim with the jump input held down, which is the case id
    /// patches by hand.
    ///
    /// `Pmove` writes `pmove->cmd.upmove = 20` back into the shared command
    /// after any sub-step that leaves `PMF_JUMP_HELD` set. Transcribing that
    /// literally would fail this test — see [`Pmove::run`] for why, and why
    /// failing it would be the wrong answer rather than a tolerable one.
    #[test]
    fn a_chopped_command_with_jump_held_matches_the_same_play_in_short_commands() {
        let jump = Buttons::JUMP;
        assert_eq!(
            digest_ignoring_tick(&over_holding(jump, &[132])),
            digest_ignoring_tick(&over_holding(jump, &[66, 66])),
        );
        // Long enough to jump, rise and land, all inside one command.
        let mut chopped = vec![PMOVE_SUBSTEP_MAX_MS; 12];
        chopped.push(800 - 12 * PMOVE_SUBSTEP_MAX_MS);
        assert_eq!(
            digest_ignoring_tick(&over_holding(jump, &[800])),
            digest_ignoring_tick(&over_holding(jump, &chopped)),
        );
    }

    /// The pre-sub-stepping integration, kept alive as a measuring stick.
    ///
    /// This is byte for byte what [`step_in_place`] did before the `Pmove`
    /// loop landed: one [`Pmove`] for the whole command, at the whole
    /// command's `dt`. It exists so that "what did sub-stepping change" is a
    /// measurement taken in this repository rather than a description of one,
    /// and so the claim that nothing at or below the bound moved can be
    /// *asserted* instead of argued.
    fn single_step<W: World + ?Sized>(
        state: &SimState,
        cmd: &UserCmd,
        world: &W,
        profile: &PhysicsProfile,
    ) -> SimState {
        let mut next = *state;
        // `u16::MAX` is at or above every representable `duration_ms`, so the
        // loop takes the whole command in one pass — which is precisely the
        // integration this crate shipped before the loop existed.
        step_bounded(&mut next, cmd, world, profile, u16::MAX);
        next
    }

    /// The four openings a command can be given, so a sweep over durations
    /// covers walking, falling, jumping and crouching rather than one of them.
    fn openings() -> [(&'static str, SimState, Buttons); 4] {
        let mut grounded = sprinting(400.0);
        grounded.player.origin = vec3(s(0.0), s(0.0), s(24.125));
        let mut airborne = SimState::spawned_at(vec3(s(0.0), s(0.0), s(900.0)), s(0.0));
        airborne.player.velocity = vec3(s(400.0), s(0.0), s(0.0));
        [
            ("running on flat ground", grounded, Buttons::NONE),
            ("running and jumping", grounded, Buttons::JUMP),
            ("running and crouching", grounded, Buttons::CROUCH),
            ("falling at speed", airborne, Buttons::NONE),
        ]
    }

    /// **The canon claim, asserted exhaustively rather than reasoned about.**
    ///
    /// Every command duration from 1 ms to the bound, from four different
    /// openings, integrated both ways: the sub-stepping loop and the
    /// single-step integration it replaced must produce the identical bits.
    /// This is *why* `canon_frozen.rs` stays green and why the four-target
    /// determinism digest is unchanged — the loop runs exactly once at every
    /// rate this project can be played at, and one pass of it is the old code.
    ///
    /// It also fails loudly if the bound is ever lowered without the
    /// consequences being faced: drop `PMOVE_SUBSTEP_MAX_MS` below 8 and this
    /// goes red for the rate the game ships at.
    #[test]
    fn every_duration_at_or_below_the_bound_integrates_bit_for_bit_as_before() {
        let profile = PhysicsProfile::cpm();
        let ground = FlatGround::at(s(0.0));
        for (name, start, buttons) in openings() {
            for ms in 1..=PMOVE_SUBSTEP_MAX_MS {
                let cmd = busy(ms, buttons);
                assert_eq!(
                    step(&start, &cmd, &ground, &profile).checksum(),
                    single_step(&start, &cmd, &ground, &profile).checksum(),
                    "{name}: a {ms} ms command changed under sub-stepping",
                );
            }
        }
    }

    /// And past the bound it genuinely differs — otherwise the test above
    /// would be passing because the loop does nothing.
    #[test]
    fn past_the_bound_the_two_integrations_part_company() {
        let profile = PhysicsProfile::cpm();
        let ground = FlatGround::at(s(0.0));
        let (name, start, buttons) = openings()[0];
        for ms in [PMOVE_SUBSTEP_MAX_MS + 1, 100, 200, 500, 1000] {
            let cmd = busy(ms, buttons);
            assert_ne!(
                step(&start, &cmd, &ground, &profile).checksum(),
                single_step(&start, &cmd, &ground, &profile).checksum(),
                "{name}: a {ms} ms command integrated identically either way",
            );
        }
    }

    /// **Where the delta lives**, measured rather than assumed — the thing the
    /// movement lab needs before it re-takes its numbers.
    ///
    /// Sub-stepping does not perturb the movement vocabulary evenly. In the
    /// air there is almost nothing for it to change, for two reasons:
    ///
    /// - **Gravity is integrated at the average of the start and end vertical
    ///   speeds** (`slide_move`), and the trapezoid rule is *exact* for
    ///   constant acceleration — splitting the interval changes nothing that
    ///   is not float rounding.
    /// - **`PM_Accelerate` is either linear in `dt` or saturated.** Below the
    ///   clamp it adds `accel · dt · wishspeed`, which sums the same over
    ///   sub-steps; at the clamp it adds `wishspeed − dot(v, wishdir)`, which
    ///   the first sub-step consumes and the rest find already spent.
    ///
    /// On the ground there is: `PM_Friction` decays velocity *multiplicatively*
    /// per step, so eight small steps and one big one are different numbers,
    /// not the same number computed differently.
    ///
    /// The caveat this fixture deliberately holds still: the view is diagonal,
    /// which is what switches CPM's air control off. Air control renormalises a
    /// vector per step and is the one airborne rule that is genuinely
    /// step-size dependent, so an air-control-heavy route will show a larger
    /// airborne delta than this measures.
    #[test]
    fn the_delta_is_on_the_ground_and_barely_in_the_air() {
        let profile = PhysicsProfile::cpm();
        let ground = FlatGround::at(s(0.0));
        const MS: u16 = 200;

        let gap = |start: &SimState, cmd: &UserCmd| {
            let chopped = step(start, cmd, &ground, &profile);
            let whole = single_step(start, cmd, &ground, &profile);
            (chopped.player.velocity - whole.player.velocity).length()
        };

        // Falling at 400 ups with the keys down: the two integrations are the
        // same numbers to within float rounding.
        let mut falling = SimState::spawned_at(vec3(s(0.0), s(0.0), s(900.0)), s(0.0));
        falling.player.velocity = vec3(s(400.0), s(0.0), s(0.0));
        let air = gap(&falling, &busy(MS, Buttons::NONE));
        assert!(
            air < s(0.01),
            "falling at speed, the two integrations differ by {air} ups — \
             airborne motion was supposed to be step-size invariant bar rounding",
        );

        // Coasting at 800 ups on the floor with no keys, where friction is the
        // only thing acting. One 200 ms step takes `800 · 6 · 0.2 = 960` ups
        // off a player who only has 800, and stops them dead in a single
        // command; four sub-steps decay them and leave them still running.
        let mut coasting = sprinting(800.0);
        coasting.player.origin = vec3(s(0.0), s(0.0), s(24.125));
        let floor = gap(&coasting, &cmd(MS));
        assert!(
            floor > s(100.0),
            "coasting on flat ground, the two integrations differ by only {floor} ups — \
             ground friction was supposed to be where the whole delta lives",
        );
    }

    #[test]
    fn the_split_is_at_the_bound_and_the_remainder_goes_last() {
        // At the bound: still one step. If this were split the numbers above
        // would agree for the wrong reason.
        assert_ne!(
            digest_ignoring_tick(&over(&[66])),
            digest_ignoring_tick(&over(&[33, 33])),
            "a command at the bound was split",
        );

        // id takes full sub-steps first and leaves the short one at the end
        // (`msec = finalTime - commandTime`, capped). The order matters —
        // friction and acceleration are not linear in the step size — so it
        // is asserted rather than assumed.
        assert_ne!(
            digest_ignoring_tick(&over(&[100])),
            digest_ignoring_tick(&over(&[34, 66])),
            "the remainder was integrated first",
        );
        assert_ne!(
            digest_ignoring_tick(&over(&[100])),
            digest_ignoring_tick(&over(&[50, 50])),
            "the split was even rather than bounded",
        );
    }

    #[test]
    fn a_chopped_command_still_sums_to_exact_integer_time() {
        // Ten commands of a second each, none of which is a whole number of
        // sub-steps. Integers do not drift, so this is exact — the same
        // guarantee `time_is_the_exact_sum_of_command_durations` makes for
        // short commands, made again on the far side of the loop.
        let end = over(&[1000; 10]);
        assert_eq!(end.tick, 10);
        assert_eq!(end.time_ms, 10_000);
    }

    /// **The behavioural headline.** A long command used to step over a jump
    /// window; it no longer can.
    ///
    /// `PM_DropTimers` runs inside `PmoveSingle`, so under a single step the
    /// whole command's duration came off the double-jump window *before*
    /// `check_jump` ever looked at it. A command longer than the window
    /// therefore always found it shut, however early in the command the player
    /// pressed jump. Sub-stepping reads the countdown between sub-steps, so a
    /// window that outlives the first sub-step is still open when that
    /// sub-step jumps.
    #[test]
    fn a_long_command_no_longer_steps_over_the_double_jump_window() {
        let profile = PhysicsProfile::cpm();
        let ground = FlatGround::at(s(0.0));

        const WINDOW_MS: u16 = 100;
        const COMMAND_MS: u16 = 200;
        // Both premises are known at compile time, so they are checked there:
        // the window must outlive the first sub-step, and must not outlive the
        // whole command.
        const _: () = assert!(
            WINDOW_MS > PMOVE_SUBSTEP_MAX_MS,
            "the first sub-step would close the window on its own",
        );
        const _: () = assert!(
            WINDOW_MS < COMMAND_MS,
            "a single step would not have closed it before `check_jump`",
        );

        let mut armed = on_ground();
        armed.player.timers.double_jump_ms = WINDOW_MS;
        let shut = on_ground(); // the same player with the window already gone

        let jump = UserCmd {
            buttons: Buttons::JUMP,
            ..cmd(COMMAND_MS)
        };
        let boosted = step(&armed, &jump, &ground, &profile);
        let plain = step(&shut, &jump, &ground, &profile);

        // Both jumped in the first sub-step and have fallen for the same time
        // since, so every gravity subtraction cancels and the whole remaining
        // difference between them is the boost.
        let gained = boosted.player.velocity.z - plain.player.velocity.z;
        assert!(
            (gained - profile.double_jump_boost).abs() < s(0.01),
            "the boosted jump gained {gained}, expected {}",
            profile.double_jump_boost,
        );
        assert_eq!(
            boosted.player.timers.double_jump_ms, 0,
            "a window buys exactly one boosted jump and is spent either way",
        );
    }

    /// The property id's `pmove->cmd.upmove = 20` protects, held here by
    /// structure instead. See [`Pmove::run`]'s documentation.
    ///
    /// A command long enough to contain a whole jump arc must still contain
    /// exactly one jump. If a sub-step ever read the held input as released,
    /// the player would re-launch off the landing inside the same command —
    /// a free double jump nobody pressed for.
    #[test]
    fn a_held_jump_is_one_jump_however_long_the_command() {
        let profile = PhysicsProfile::vq3();
        let ground = FlatGround::at(s(0.0));

        // 270 ups against 800 units/s² is a 675 ms round trip, so 800 ms is a
        // command with a full jump *and* its landing inside it.
        const COMMAND_MS: u16 = 800;
        let jump = UserCmd {
            buttons: Buttons::JUMP,
            ..cmd(COMMAND_MS)
        };
        let end = step(&on_ground(), &jump, &ground, &profile);

        assert!(
            end.player.ground.is_grounded(),
            "the jump and its landing should both fit inside this command",
        );
        assert!(
            end.player.jump_held,
            "the input never went up, so the press is still spent",
        );
        // The one number that says *when* the jump was: `since_jumped_ms` is
        // zeroed by `check_jump` and then counts up per sub-step. One jump, in
        // the first sub-step, and no second one on the landing.
        assert_eq!(
            end.player.timers.since_jumped_ms,
            COMMAND_MS - PMOVE_SUBSTEP_MAX_MS,
            "the player jumped more than once inside one command",
        );
    }

    /// The other edge-triggered technique, for the same reason.
    ///
    /// A slide armed once per *sub-step* would be a slide that never ends
    /// while crouch is held on a long command — the exact "friction toggle"
    /// failure mode `check_slide` is written to avoid.
    #[test]
    fn a_crouch_press_arms_one_slide_per_command_however_long() {
        let p = PhysicsProfile::experimental();
        let ground = FlatGround::at(s(0.0));

        // 200 ms is 66 + 66 + 66 + 2. The first sub-step finds the edge and
        // arms the full duration; the remaining 134 ms count *down* off it.
        const COMMAND_MS: u16 = 200;
        let end = step(&sprinting(500.0), &crouch(COMMAND_MS), &ground, &p);
        assert_eq!(
            end.player.timers.slide_ms,
            p.slide_duration_ms - (COMMAND_MS - PMOVE_SUBSTEP_MAX_MS),
            "a slide was re-armed inside one command",
        );
        assert!(end.player.crouched);
    }
}
