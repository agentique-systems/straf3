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
use crate::world::{SurfaceFlags, Sweep, Trace, World};

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
pub fn step_in_place<W>(state: &mut SimState, cmd: &UserCmd, world: &W, profile: &PhysicsProfile)
where
    W: World + ?Sized,
{
    // A zero-length command advances nothing. Returning early rather than
    // integrating by zero keeps `tick` counting commands that did something.
    if cmd.duration_ms == 0 {
        return;
    }

    // TODO(wave3): Q3's `PmoveSingle` splits a long command into several
    // sub-steps (`pmove_msec`) so that a large duration cannot tunnel through
    // geometry or skip a jump window. That split changes results, so it must
    // land before any reference replay is recorded.
    let ms = cmd.duration_ms;

    // The one permitted integer-milliseconds-to-scalar conversion in the whole
    // crate — spec rev 3, criterion 3. Mirrors Q3's
    // `pml.frametime = pml.msec * 0.001`. Do not add a second one.
    let dt = num::seconds_from_millis(u32::from(ms));

    // The view is player input, applied whole. Movement never rotates the
    // player: what you looked at is what the recording says you looked at.
    state.player.view = cmd.view;

    let mut pm = Pmove::new(cmd, world, profile, dt);
    pm.run(&mut state.player, ms);

    state.tick += 1;
    state.time_ms += u32::from(ms);
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
/// one command, and nothing that outlives it.
///
/// It is a struct rather than a pile of arguments for the same reason Q3 used
/// one — `PM_WalkMove` and `PM_SlideMove` need the same eight values — but
/// unlike Q3's it is a local, not a global, so two simulations in one process
/// cannot see each other's.
struct Pmove<'a, W: World + ?Sized> {
    world: &'a W,
    profile: &'a PhysicsProfile,
    /// Frame duration in seconds. Q3's `pml.frametime`.
    dt: Scalar,
    /// The collision box for this command, standing or crouched.
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

    /// Q3's `pml.groundPlane`: there is a plane underfoot.
    ground_plane: bool,
    /// Q3's `pml.walking`: that plane is walkable.
    walking: bool,
    ground_normal: Vec3,
    ground_surface: SurfaceFlags,
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

        let (forward, right) = angle_vectors(cmd.view.pitch, cmd.view.yaw, cmd.view.roll);

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
            ground_plane: false,
            walking: false,
            ground_normal: num::ZERO,
            ground_surface: SurfaceFlags::NONE,
        }
    }

    /// Quake's `PmoveSingle`, in its order.
    fn run(&mut self, p: &mut PlayerState, ms: u16) {
        // `PmoveSingle`: releasing the jump input re-arms the jump.
        if !self.jump_pressed {
            p.jump_held = false;
        }

        self.check_duck(p);
        self.ground_trace(p);
        p.timers.advance(ms); // PM_DropTimers

        if self.walking {
            self.walk_move(p);
        } else {
            self.air_move(p);
        }

        // Q3 probes again after moving, so the state a caller observes — and
        // the state the next command starts from — describes where the player
        // ended up, not where they began.
        self.ground_trace(p);
    }

    // ── traces ─────────────────────────────────────────────────────────────

    fn sweep(&self, from: Vec3, to: Vec3) -> Trace {
        self.world.trace(&Sweep {
            start: from,
            end: to,
            half_extents: self.hull.half_extents,
            center_offset: self.hull.center_offset,
        })
    }

    /// A sweep, plus the point it actually reached.
    ///
    /// The hull is held [`SURFACE_CLIP_EPSILON`] clear of whatever it hit,
    /// measured along the surface normal, which is what Q3's collision code
    /// does and what keeps a resting player out of the floor. See that
    /// constant's documentation.
    fn sweep_to(&self, from: Vec3, to: Vec3) -> (Trace, Vec3) {
        let trace = self.sweep(from, to);
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
            drop += control * self.profile.friction * self.dt;
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

    // ── the slide solver ───────────────────────────────────────────────────

    /// Quake's `PM_SlideMove`. Returns whether the move was interrupted.
    ///
    /// The player is moved by re-planning up to [`SLIDE_BUMPS`] times, each
    /// time clipping velocity to every plane touched so far. The plane set is
    /// bounded by [`PhysicsProfile::max_clip_planes`]; running out means
    /// stopping dead, which is what happens in a tight corner.
    fn slide_move(&self, p: &mut PlayerState, gravity: bool) -> bool {
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
    fn step_slide_move(&self, p: &mut PlayerState, gravity: bool) {
        let start_o = p.origin;
        let start_v = p.velocity;

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
            return; // no headroom to step into
        }

        let step_size = up_pos.z - start_o.z;
        p.origin = up_pos;
        p.velocity = start_v;

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
    use crate::cmd::TickRate;
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
}
