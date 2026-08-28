//! The gate instruments the §1.2 sweep does not produce.
//!
//! # Why these are here and not in [`crate::candidate`]
//!
//! The sweep answers one question — what is the mechanic worth, at every timing
//! and every aim, in every cell. Four of `docs/movement-canon.md`'s gates ask
//! something the sweep cannot answer however finely it is run, because each asks
//! about a *different* run:
//!
//! - **G5(a)** asks about a player who never uses the mechanic and never goes
//!   fast. The sweep only ever measures a player who does both.
//! - **G3**'s second count asks whether an input was ignored. Nothing in a sweep
//!   of outcomes can tell you that an input did not reach the outcome; you have
//!   to change the input and look.
//! - **G7**'s first part asks whether a rule predicts the effect. That needs the
//!   state the effect was computed from, which the sweep discards.
//! - **§2.0**'s crouch-slide question asks what the slide does *after* the
//!   player's speed has fallen through the threshold that let them start it,
//!   which no cell of the sweep isolates.
//!
//! Each one below is a separate run with a separate stated construction, and
//! each publishes what it measured rather than what it concluded.

use straf3_sim::num::{Scalar, s, vec3};
use straf3_sim::{Buttons, PhysicsProfile, SimState, UserCmd, step};

use crate::candidate::{Anchor, Context, Mechanic};
use crate::geometry;
use crate::harness::{Axis, HZ, holding, still};
use crate::num::{cos_degrees, heading_degrees, horizontal_speed, sin_degrees};

// ── G5(a): available to a player who never exceeds `max_speed` ──────────────

/// How long the G5(a) run is: thirty seconds, which is long enough for the
/// jump-land rhythm to repeat dozens of times in every context.
pub const EARNED_SECONDS: usize = 30;

/// Commands spent settling a player onto a surface before the run starts.
const SETTLE_COMMANDS: usize = 200;

/// How far short of the context's feature the G5(a) run begins.
///
/// Far enough that a player accelerating from a standstill is at the ground cap
/// well before they reach it, so what the feature does to them is measured at
/// speed rather than during the wind-up.
const EARNED_APPROACH: f32 = 512.0;

/// What a player who never exceeds `max_speed` found.
#[derive(Debug, Clone, Copy)]
pub struct Earned {
    /// How many times the mechanic went from unavailable to available: the
    /// count G5(a) asks for.
    pub arming_events: usize,
    /// How many commands it was available on. A mechanic armed once and left
    /// armed against a wall is a different fact from one armed forty times, and
    /// the two counts separate them.
    pub available_commands: usize,
    /// The highest horizontal speed the run reached, published so a reader can
    /// check the premise rather than take it: this must not exceed
    /// `max_speed`, or the run was not the run the gate describes.
    pub max_speed: Scalar,
    /// Commands run, excluding the settle.
    pub commands: usize,
}

/// Whether the mechanic is available to `st` right now, under `profile`.
///
/// Availability is the preconditions the *world and the player's history* have
/// to satisfy, not the press itself: G5(a) counts how many times the mechanic
/// "becomes available", and a mechanic is available when everything except the
/// player's own decision to invoke it is already true.
#[must_use]
pub fn available(mech: Mechanic, profile: &PhysicsProfile, st: &SimState) -> bool {
    let p = &st.player;
    match mech {
        // `check_slide` needs a crouch edge, ground contact and horizontal
        // speed at or above `slide_entry_speed`. The crouch edge is the press;
        // the other two are the availability.
        Mechanic::CrouchSlide => {
            profile.slide_duration_ms != 0
                && p.ground.is_grounded()
                && horizontal_speed(p.velocity) >= profile.slide_entry_speed
        }
        // `check_air_jump`'s dash arm: a window a landing that ended a jump
        // opened, still running.
        Mechanic::Dash => profile.dash_window_ms != 0 && p.timers.dash_ms > 0,
        // `note_wall_contact`'s window, likewise.
        Mechanic::WallJump => profile.wall_contact_window_ms != 0 && p.timers.wall_contact_ms > 0,
    }
}

/// Run a player who accelerates on the ground only, jumps and lands freely, and
/// never invokes the mechanic — and count how often it becomes available.
///
/// The aim is a fixed world direction rather than an angle held off the current
/// velocity, which is the whole point: an angle off the velocity *is*
/// strafejumping, and a strafejumping player exceeds `max_speed`. Holding one
/// direction on the ground caps at `max_speed` by construction, and the
/// measured maximum is published so the premise can be checked.
#[must_use]
pub fn earned(mech: Mechanic, ctx: &Context) -> Earned {
    earned_gated(mech, ctx, None)
}

/// The same run, with a minimum speed required at the arming event.
///
/// `gate` is the dash's pre-registered `dash_entry_speed` retune expressed as a
/// condition on the run rather than as a field on a profile that does not have
/// one yet. With `None` this is the mechanic as the shipped crate arms it.
#[must_use]
pub fn earned_gated(mech: Mechanic, ctx: &Context, gate: Option<Scalar>) -> Earned {
    let profile = mech.profile();
    let commands = EARNED_SECONDS * HZ;

    let start = vec3(
        ctx.feature_x - s(EARNED_APPROACH),
        s(0.0),
        geometry::resting_origin_z(ctx.surface_z) + s(1.0),
    );
    let mut st = SimState::spawned_at(start, s(0.0));
    let settle = UserCmd {
        buttons: if ctx.crouch_only {
            Buttons::CROUCH
        } else {
            Buttons::NONE
        },
        ..still()
    };
    for _ in 0..SETTLE_COMMANDS {
        st = step(&st, &settle, &ctx.world, &profile);
    }
    st.player.velocity = vec3(s(0.0), s(0.0), s(0.0));

    let mut out = Earned {
        arming_events: 0,
        available_commands: 0,
        max_speed: s(0.0),
        commands,
    };
    let mut was = available(mech, &profile, &st);
    // Whether the window currently running was armed by an event fast enough to
    // pass the gate. A window the gate refused is a window that never opened, so
    // it counts neither as an arming event nor as an available command.
    let mut armed_ok = true;
    for _ in 0..commands {
        let mut buttons = Buttons::NONE;
        if st.player.ground.is_grounded() {
            buttons = buttons.with(Buttons::JUMP);
        }
        if ctx.crouch_only {
            buttons = buttons.with(Buttons::CROUCH);
        }
        let cmd = UserCmd {
            buttons,
            ..holding(Axis::Forward, s(0.0))
        };
        st = step(&st, &cmd, &ctx.world, &profile);

        let speed = horizontal_speed(st.player.velocity);
        if speed > out.max_speed {
            out.max_speed = speed;
        }
        let now = available(mech, &profile, &st);
        if now && !was {
            armed_ok = gate.is_none_or(|g| speed >= g);
        }
        if now && armed_ok {
            out.available_commands += 1;
            if !was {
                out.arming_events += 1;
            }
        }
        was = now;
    }
    out
}

// ── G3's second count: did the mechanic ignore an input? ────────────────────

/// How many commands after the invoking one are probed for a lost input.
///
/// Thirty-two, which is 256 ms — longer than any wind-up a mechanic in this
/// tree could hide and short enough that the probe stays inside the run.
pub const PROBE_COMMANDS: usize = 32;

/// How far a probed command's aim is rotated.
///
/// A right angle, because it is the largest change that is still a direction
/// the player could have asked for, and because a smaller one could be lost
/// under `PM_Accelerate`'s clamp at speed and read as an ignored input when it
/// was only an unproductive one.
pub const PROBE_ROTATION: Scalar = s(90.0);

/// How many of the probed commands changed nothing.
#[derive(Debug, Clone, Copy, Default)]
pub struct Unresponsive {
    /// Probed commands after which the candidate run ended at exactly the
    /// velocity it ended at unperturbed.
    pub candidate: usize,
    /// The same count for the control, which is the reading that says whether
    /// the mechanic is responsible: a command the control also ignores was
    /// never the mechanic's doing.
    pub control: usize,
    /// How many commands were probed.
    pub probed: usize,
}

/// Rotate one command's aim at a time and count the commands that changed
/// nothing, in the candidate run and in its control.
///
/// # What this measures, and what it does not
///
/// It measures whether a command's **steering** reached the outcome. It does
/// not measure whether a *jump press* did: under canon an airborne jump press
/// does nothing at all, so "the control accepted a press the candidate
/// refused" is vacuous and cannot distinguish a spent press from an ignored
/// one. The dash and the wall jump both set `jump_held`, exactly as a floor
/// jump does, and that consequence is read from `step.rs` and reported as a
/// fact rather than folded into this count.
#[must_use]
pub fn unresponsive(
    mech: Mechanic,
    ctx: &Context,
    anchor: &Anchor,
    aim: Scalar,
    invoke_at: usize,
    commands: usize,
    hold: bool,
) -> Unresponsive {
    let base = crate::candidate::walk_pair_perturbed(
        mech,
        ctx,
        &anchor.state,
        aim,
        Some(invoke_at),
        commands,
        hold,
        None,
        s(0.0),
    );
    let mut out = Unresponsive::default();
    for j in (invoke_at + 1)..(invoke_at + 1 + PROBE_COMMANDS).min(commands) {
        let probed = crate::candidate::walk_pair_perturbed(
            mech,
            ctx,
            &anchor.state,
            aim,
            Some(invoke_at),
            commands,
            hold,
            Some(j),
            PROBE_ROTATION,
        );
        out.probed += 1;
        if probed.candidate.player.velocity == base.candidate.player.velocity {
            out.candidate += 1;
        }
        if probed.control.player.velocity == base.control.player.velocity {
            out.control += 1;
        }
    }
    out
}

// ── G7 part 1: the closed form beside the measurement ───────────────────────

/// A closed form and what the simulation actually did.
#[derive(Debug, Clone, Copy)]
pub struct Predicted {
    /// What the rule read out of `step.rs` says the invoking command is worth,
    /// in ups of horizontal speed.
    pub predicted: Scalar,
    /// What the difference between the candidate and its control actually was
    /// at the end of that command.
    pub measured: Scalar,
}

impl Predicted {
    /// How far apart they are.
    #[must_use]
    pub fn residual(&self) -> Scalar {
        self.measured - self.predicted
    }
}

/// The mechanic's immediate effect, predicted from the state the player can see
/// and measured against the control on the same command.
///
/// # Why the immediate effect and not the outcome at the horizon
///
/// A closed form for the horizon outcome does not exist and this module will not
/// invent one: the horizon is a full second of `PM_Accelerate`, friction, ground
/// probes and possibly a collision downstream of the impulse, and every one of
/// those depends on the whole run. What *is* closed-form is the impulse itself,
/// which is the thing `step.rs` computes from constants and current state, and
/// it is published as that and labelled as that. G7's first part asks the
/// verdict to state a rule; this is the arithmetic the code already contains,
/// measured, so that a rule can be checked against something.
#[must_use]
pub fn immediate(
    mech: Mechanic,
    ctx: &Context,
    anchor: &Anchor,
    aim: Scalar,
    invoke_at: usize,
    hold: bool,
) -> Option<Predicted> {
    let profile = mech.profile();
    // The state at the *start* of the invoking command, which both runs share
    // because they have not diverged yet.
    let before = crate::candidate::walk_pair_perturbed(
        mech,
        ctx,
        &anchor.state,
        aim,
        Some(invoke_at),
        invoke_at,
        hold,
        None,
        s(0.0),
    );
    let after = crate::candidate::walk_pair_perturbed(
        mech,
        ctx,
        &anchor.state,
        aim,
        Some(invoke_at),
        invoke_at + 1,
        hold,
        None,
        s(0.0),
    );
    if after.diverged_at != Some(invoke_at) {
        return None;
    }

    let pre = before.control.player;
    let measured = horizontal(after.candidate.player.velocity - after.control.player.velocity);

    // The wish direction the command asks for: `air_move` and `walk_move` both
    // build it from the view, and the view is aimed at the heading plus the aim.
    let want = heading_degrees(pre.velocity) + aim;
    let wishdir = vec3(cos_degrees(want), sin_degrees(want), s(0.0));

    let predicted = match mech {
        // `check_air_jump`: `addspeed = dash_speed − dot(v, wishdir)`, applied
        // along `wishdir`. Read against the *control's* velocity at the end of
        // the command, because the horizontal components of the two runs are
        // identical up to the impulse and `PM_SlideMove` leaves horizontal
        // velocity alone unless a plane is met.
        Mechanic::Dash => {
            let v = after.control.player.velocity;
            let along = v.x * wishdir.x + v.y * wishdir.y;
            (profile.dash_speed - along).max(s(0.0))
        }
        // `check_air_jump`: `v += wall_normal · wall_jump_velocity`. The
        // vertical half is an assignment to `jump_velocity` and is not a
        // horizontal quantity, so the horizontal impulse is the normal's
        // horizontal length times the constant — exactly, and independent of
        // anything else on the command.
        Mechanic::WallJump => {
            let n = pre.wall_normal;
            (n.x * n.x + n.y * n.y).sqrt() * profile.wall_jump_velocity
        }
        // `PM_Friction`, reading `slide_friction` where it would have read
        // `friction`. Both runs are crouched on this command and ask for the
        // same wish speed, so the whole difference is the friction term:
        // `max(speed, stop_speed) · (friction − slide_friction) · dt`.
        Mechanic::CrouchSlide => {
            let speed = horizontal_speed(pre.velocity);
            let control = if speed < profile.stop_speed {
                profile.stop_speed
            } else {
                speed
            };
            let dt = s(f32::from(crate::harness::MS)) / s(1000.0);
            control * (profile.friction - profile.slide_friction) * dt
        }
    };
    Some(Predicted {
        predicted,
        measured,
    })
}

fn horizontal(v: straf3_sim::num::Vec3) -> Scalar {
    (v.x * v.x + v.y * v.y).sqrt()
}

// ── §2.0: what the crouch slide does once the speed is gone ─────────────────

/// What a slide did after it was armed.
#[derive(Debug, Clone, Copy)]
pub struct SlideLife {
    /// Commands the slide's timer ran for.
    pub commands: usize,
    /// Of those, how many the player spent **standing** — the posture the
    /// mechanic's own doc comment assumes is being paid for.
    pub standing_commands: usize,
    /// Horizontal speed when the slide was armed.
    pub entry_speed: Scalar,
    /// Horizontal speed when the timer ran out.
    pub exit_speed: Scalar,
    /// The lowest horizontal speed reached while the timer was still running.
    pub lowest_speed: Scalar,
    /// Commands the timer ran on while the speed was **below**
    /// `slide_entry_speed` — the speed the player had to have to start it.
    pub commands_below_entry: Scalar,
    /// Whether the timer ever ran while the speed was below `max_speed`, which
    /// is a speed ground acceleration alone can reach.
    pub ran_below_max_speed: bool,
}

/// Arm a slide on flat ground at `entry`, then stop asking for anything, and
/// watch what the timer does.
///
/// # Why no input is held
///
/// A slide with forward held is a slide plus an acceleration, and the two are
/// not separable afterwards. With nothing held, the only force on the player is
/// `PM_Friction`, so what the run measures is exactly the mechanic: how far a
/// player coasts on one-sixth friction, and whether the timer notices that the
/// speed which bought the slide has gone.
///
/// `hold_crouch` chooses the posture after the arming command: `false` releases
/// crouch on the next command, which is §2.0's tap-and-stand.
#[must_use]
pub fn slide_life(ctx: &Context, entry: Scalar, hold_crouch: bool) -> Option<SlideLife> {
    let mech = Mechanic::CrouchSlide;
    let profile = mech.profile();
    let mut st = crate::candidate::entering(mech, ctx, entry);

    // The arming command: crouch pressed, at speed.
    let armed = UserCmd {
        buttons: Buttons::CROUCH,
        ..holding(Axis::Forward, s(0.0))
    };
    let entry_speed = horizontal_speed(st.player.velocity);
    st = step(&st, &armed, &ctx.world, &profile);
    if st.player.timers.slide_ms == 0 {
        return None;
    }

    let mut out = SlideLife {
        commands: 0,
        standing_commands: 0,
        entry_speed,
        exit_speed: s(0.0),
        lowest_speed: horizontal_speed(st.player.velocity),
        commands_below_entry: s(0.0),
        ran_below_max_speed: false,
    };
    // Bounded by the timer, and by a cap so a mechanic that never expired would
    // be reported rather than hang.
    let cap = 4 * HZ;
    for _ in 0..cap {
        if st.player.timers.slide_ms == 0 {
            break;
        }
        // Nothing is asked for: no move axis, only the posture.
        let cmd = UserCmd {
            buttons: if hold_crouch {
                Buttons::CROUCH
            } else {
                Buttons::NONE
            },
            forward_move: 0,
            right_move: 0,
            ..still()
        };
        st = step(&st, &cmd, &ctx.world, &profile);
        out.commands += 1;
        if !st.player.crouched {
            out.standing_commands += 1;
        }
        let speed = horizontal_speed(st.player.velocity);
        if speed < out.lowest_speed {
            out.lowest_speed = speed;
        }
        if st.player.timers.slide_ms > 0 {
            if speed < profile.slide_entry_speed {
                out.commands_below_entry += s(1.0);
            }
            if speed < profile.max_speed {
                out.ran_below_max_speed = true;
            }
        }
    }
    out.exit_speed = horizontal_speed(st.player.velocity);
    Some(out)
}
