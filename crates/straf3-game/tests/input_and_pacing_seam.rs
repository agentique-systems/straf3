//! **Criteria 3 and 5, at the library seam** (spec rev 6 §S).
//!
//! - Criterion 3: *"Input is captured as the same integer-millisecond
//!   `UserCmd` the simulation already takes — the renderer feeds the seam, it
//!   does not bypass it."*
//! - Criterion 5: *"Frame pacing is decoupled from simulation stepping, so the
//!   fixed 8 ms command cadence survives a variable frame rate."*
//!
//! `replay_equivalence.rs` proves both end-to-end through the shipped binary,
//! which is the stronger claim. This file proves them at the seam, which is
//! what makes a failure *diagnosable*: an end-to-end mismatch says the build
//! diverged somewhere, and these say where.
//!
//! Written independently of platform, against the published signatures. Every
//! assertion is computed on both sides; there is no golden checksum literal
//! here, because spec rev 6 Q1's Cody–Waite trig replacement changes every
//! checksum in the repository.

#[path = "../../straf3-platform/tests/support/mod.rs"]
mod support;

use straf3_game::input_map::command_from_input;
use straf3_game::tick::{FixedStep, TickPlan, advance_one, plan_ticks};
use straf3_platform::input::{Action, InputState, MouseLook};
use straf3_sim::num::{s, vec3};
use straf3_sim::world::FlatGround;
use straf3_sim::{Buttons, PhysicsProfile, SimState, TickRate, UserCmd, step_in_place};

use support::pacing::{drive_reporting_drops, sequences, violations};

const PERIOD_MS: u16 = 8;

// ---------------------------------------------------------------------------
// Criterion 3 — the command that reaches the simulation
// ---------------------------------------------------------------------------

/// The command carries exactly the integer milliseconds it was handed.
///
/// This is criterion 3's literal text. `duration_ms` is a `u16`, so the type
/// system already forbids a float — what this pins is that the value is not
/// rescaled, rounded or derived from anything else on the way through.
#[test]
fn the_command_carries_exactly_the_milliseconds_it_was_given() {
    let input = InputState::default();
    for ms in [1u16, 4, 8, 13, 33, 1000] {
        let cmd = command_from_input(&input, ms);
        assert_eq!(
            cmd.duration_ms, ms,
            "command_from_input was handed {ms} ms and produced a command \
             lasting {} ms",
            cmd.duration_ms,
        );
    }
}

/// Movement axes are Quake's signed-byte axes, and opposed keys cancel.
#[test]
fn movement_axes_are_signed_bytes_and_opposed_keys_cancel() {
    let mut input = InputState::default();

    input.set(Action::MoveForward, true);
    assert_eq!(command_from_input(&input, PERIOD_MS).forward_move, 127);

    input.set(Action::MoveBack, true);
    assert_eq!(
        command_from_input(&input, PERIOD_MS).forward_move,
        0,
        "holding forward and back at once must cancel to 0, not saturate",
    );

    input.set(Action::MoveForward, false);
    assert_eq!(command_from_input(&input, PERIOD_MS).forward_move, -127);
}

/// **`right_move` is +127 for `MoveRight`.**
///
/// This sign is not arbitrary and flipping it produces no compile error and no
/// test failure anywhere else — the game simply strafe-jumps mirrored. Q3's
/// "right" basis vector points along −Y at yaw 0 (`angle_vectors`,
/// `crates/straf3-sim/src/step.rs`), and that sign is baked into every recorded
/// `right_move` in every fixture. Pinning it here is the difference between a
/// replay reproducing and a replay drifting sideways.
#[test]
fn strafe_right_is_positive_right_move() {
    let mut input = InputState::default();
    input.set(Action::MoveRight, true);
    assert_eq!(command_from_input(&input, PERIOD_MS).right_move, 127);

    let mut input = InputState::default();
    input.set(Action::MoveLeft, true);
    assert_eq!(command_from_input(&input, PERIOD_MS).right_move, -127);
}

/// Jump and crouch travel both ways Quake 3 spells them: the button bit *and*
/// the `up_move` axis.
///
/// The simulation substitutes 127 when only the bit is set, so sending the bit
/// alone would "work" — but it would not reproduce the crouch case's
/// `PM_CmdScale` input, and a recording made that way replays differently.
#[test]
fn jump_and_crouch_travel_as_both_a_button_and_an_axis() {
    let mut input = InputState::default();
    input.set(Action::Jump, true);
    let cmd = command_from_input(&input, PERIOD_MS);
    assert!(cmd.buttons.contains(Buttons::JUMP), "jump bit not set");
    assert_eq!(cmd.up_move, 127, "jump did not drive up_move");

    let mut input = InputState::default();
    input.set(Action::Crouch, true);
    let cmd = command_from_input(&input, PERIOD_MS);
    assert!(cmd.buttons.contains(Buttons::CROUCH), "crouch bit not set");
    assert_eq!(cmd.up_move, -127, "crouch did not drive up_move");

    // Both held: jump wins the shared axis, and both bits stay set.
    let mut input = InputState::default();
    input.set(Action::Jump, true);
    input.set(Action::Crouch, true);
    let cmd = command_from_input(&input, PERIOD_MS);
    assert_eq!(cmd.up_move, 127, "jump must win the shared up/down axis");
    assert!(cmd.buttons.contains(Buttons::JUMP));
    assert!(cmd.buttons.contains(Buttons::CROUCH));
}

/// The view angle is **absolute**, never a delta.
///
/// Spec: a recorded delta depends on the sensitivity, acceleration curve and
/// polling rate of the mouse that produced it, so a replay would only reproduce
/// on the machine that recorded it. Two motions in the same direction must
/// accumulate; if `angles()` returned the most recent motion instead, this
/// fails.
#[test]
fn the_view_is_an_absolute_angle_not_the_last_mouse_delta() {
    let mut look = MouseLook::looking_along(s(0.0));
    look.apply_motion(s(100.0), s(0.0));
    let after_one = look.yaw();
    look.apply_motion(s(100.0), s(0.0));
    let after_two = look.yaw();

    assert_ne!(
        after_one, after_two,
        "two identical mouse motions produced the same yaw — the view is \
         reporting the last delta rather than the accumulated angle",
    );

    let step = after_one - s(0.0);
    let second_step = after_two - after_one;
    assert!(
        (step - second_step).abs() < s(1e-3),
        "identical motions moved the view by {step} then {second_step} degrees; \
         accumulation is not linear",
    );
}

/// Yaw stays wrapped to (−180, 180].
///
/// Not cosmetic. An unwrapped yaw loses angular resolution as its exponent
/// grows — around 1e−5 degrees at ±180, but 0.008 degrees at ±100 000 — so a
/// long spin would quietly coarsen the input the whole game turns on.
#[test]
fn yaw_stays_wrapped_and_keeps_its_resolution() {
    let mut look = MouseLook::looking_along(s(0.0));
    for _ in 0..2_000 {
        look.apply_motion(s(500.0), s(0.0));
        let yaw = look.yaw();
        assert!(
            yaw > s(-180.0) && yaw <= s(180.0),
            "yaw escaped its range: {yaw}",
        );
    }
}

/// Roll is never set from input. The simulation may set it; the player cannot.
#[test]
fn input_never_produces_roll() {
    let mut look = MouseLook::looking_along(s(30.0));
    look.apply_motion(s(-250.0), s(140.0));
    assert_eq!(look.angles().roll, 0);
}

/// **The renderer feeds the seam and does not bypass it.**
///
/// `advance_one` is documented as being `step_in_place` and nothing else. This
/// proves it per-tick over a real run rather than by reading the source: the
/// same commands driven through the game's step-driver and through the
/// simulation directly must produce the identical checksum stream.
///
/// If a wrapper ever appears above the seam — smoothing, clamping, an extra
/// sub-step — this is the test that goes red.
#[test]
fn the_games_step_driver_is_the_simulation_and_nothing_else() {
    let run = support::run_named("strafe_jump_cpm");
    let world = FlatGround::at(s(0.0));
    let profile = PhysicsProfile::cpm();

    let mut through_game = SimState::spawned_at(run.spawn, run.yaw);
    let mut through_sim = SimState::spawned_at(run.spawn, run.yaw);
    let mut game_digests = vec![through_game.checksum()];
    let mut sim_digests = vec![through_sim.checksum()];

    for cmd in &run.cmds {
        advance_one(&mut through_game, cmd, &world, &profile);
        step_in_place(&mut through_sim, cmd, &world, &profile);
        game_digests.push(through_game.checksum());
        sim_digests.push(through_sim.checksum());
    }

    support::assert_digests_match(
        "straf3_game::tick::advance_one",
        &game_digests,
        "straf3_sim::step_in_place",
        &sim_digests,
    );
}

/// A command built from input drives the simulation identically to the same
/// command written by hand.
///
/// This closes the loop criterion 3 is about: the thing the input layer
/// produces is not merely *shaped* like a `UserCmd`, it *is* the value the
/// simulation would have been given anyway.
#[test]
fn a_command_built_from_input_is_indistinguishable_from_a_handwritten_one() {
    let mut input = InputState::default();
    input.set(Action::MoveForward, true);
    input.set(Action::MoveRight, true);
    let from_input = command_from_input(&input, PERIOD_MS);

    let handwritten = UserCmd {
        duration_ms: PERIOD_MS,
        forward_move: 127,
        right_move: 127,
        up_move: 0,
        buttons: Buttons::NONE,
        view: from_input.view, // the view is the input layer's to decide
    };
    assert_eq!(from_input, handwritten);

    let world = FlatGround::at(s(0.0));
    let profile = PhysicsProfile::cpm();
    let spawn = SimState::spawned_at(vec3(s(0.0), s(0.0), s(64.0)), s(0.0));

    let mut a = spawn;
    let mut b = spawn;
    for _ in 0..200 {
        step_in_place(&mut a, &from_input, &world, &profile);
        step_in_place(&mut b, &handwritten, &world, &profile);
        assert_eq!(a.checksum(), b.checksum());
    }
}

// ---------------------------------------------------------------------------
// Criterion 5 — the cadence survives a variable frame rate
// ---------------------------------------------------------------------------

/// `FixedStep` satisfies the criterion-5 properties on every hostile frame
/// schedule.
///
/// The checker and its sequences are in `support::pacing`, and
/// `frame_pacing_checker.rs` proves the checker rejects five distinct broken
/// accumulators. This is the same checker pointed at the real one.
#[test]
fn fixed_step_survives_every_hostile_frame_schedule() {
    for seq in sequences() {
        let mut step = FixedStep::new(TickRate::HZ_125);
        let dropped = std::cell::Cell::new(0u32);
        let pacing = drive_reporting_drops(
            &seq.frames,
            |dt| {
                let n = step.advance(u64::from(dt));
                dropped.set(
                    u32::try_from(step.dropped_total_ms())
                        .expect("dropped time should not exceed u32 in a test"),
                );
                n
            },
            || dropped.get(),
        );

        let found = violations(&seq, u32::from(PERIOD_MS), &pacing);
        assert!(
            found.is_empty(),
            "FixedStep failed {} ({}):\n  {}",
            seq.name,
            seq.intent,
            found.join("\n  "),
        );
    }
}

/// The accumulator's books balance exactly, every frame.
///
/// `ticks x tick_ms + carried_ms + dropped_ms == every millisecond ever fed
/// in`. Platform names this as the invariant to mutate against: break the carry
/// (`remainder_ms` to 0, or `%` to `/`) and it fails immediately.
#[test]
fn every_millisecond_is_accounted_for() {
    let mut step = FixedStep::new(TickRate::HZ_125);
    let tick_ms = u64::from(step.tick_ms());
    let mut fed = 0u64;
    let mut ticks = 0u64;

    for seq in sequences() {
        for &dt in &seq.frames {
            let dt = u64::from(dt);
            fed += dt;
            ticks += u64::from(step.advance(dt));

            assert_eq!(
                ticks * tick_ms + u64::from(step.carried_ms()) + step.dropped_total_ms(),
                fed,
                "after {fed} ms: {ticks} ticks, {} carried, {} dropped",
                step.carried_ms(),
                step.dropped_total_ms(),
            );
            assert!(
                u64::from(step.carried_ms()) < tick_ms,
                "carried {} ms is a whole command or more",
                step.carried_ms(),
            );
        }
    }
}

/// `plan_ticks` is pure: the same arguments always give the same plan, and it
/// never depends on how many times it has been called.
#[test]
fn plan_ticks_is_pure() {
    for elapsed in [0u64, 1, 7, 8, 9, 100, 5_000] {
        for carried in [0u32, 1, 7] {
            let first = plan_ticks(elapsed, carried, PERIOD_MS, 250);
            for _ in 0..5 {
                let again: TickPlan = plan_ticks(elapsed, carried, PERIOD_MS, 250);
                assert_eq!(again.ticks, first.ticks);
                assert_eq!(again.remainder_ms, first.remainder_ms);
                assert_eq!(again.dropped_ms, first.dropped_ms);
            }
            assert_eq!(
                u64::from(first.ticks) * u64::from(PERIOD_MS)
                    + u64::from(first.remainder_ms)
                    + first.dropped_ms,
                elapsed + u64::from(carried),
                "plan_ticks({elapsed}, {carried}) does not account for its input",
            );
        }
    }
}

/// The per-frame cap bounds catch-up, and reports what it discarded.
///
/// Without a cap, a frame that took two minutes (lid closed, tab suspended)
/// demands 15 000 ticks at once and the next frame is longer still — the spiral
/// of death. With a cap that discards silently, the simulation clock falls
/// behind the wall clock and a replay recorded through it cannot reproduce.
/// Both halves matter.
#[test]
fn the_catch_up_cap_bounds_work_and_declares_what_it_drops() {
    let mut step = FixedStep::new(TickRate::HZ_125).with_max_ticks_per_frame(10);

    let ticks = step.advance(10_000);
    assert_eq!(ticks, 10, "the cap did not bound catch-up");
    assert!(
        step.dropped_total_ms() > 0,
        "time was discarded to the cap without being declared",
    );
    assert_eq!(
        u64::from(ticks) * u64::from(step.tick_ms())
            + u64::from(step.carried_ms())
            + step.dropped_total_ms(),
        10_000,
        "the cap lost time that it did not declare",
    );
}

/// An uncapped accumulator drops nothing, however hostile the schedule.
///
/// The cap is a safety valve, not part of the cadence. This distinguishes "the
/// implementation drops time under stress" from "the implementation drops time
/// as a matter of course", which the conservation test alone cannot.
#[test]
fn an_uncapped_accumulator_never_drops_a_millisecond() {
    let mut step = FixedStep::new(TickRate::HZ_125).with_max_ticks_per_frame(u32::MAX);
    let mut fed = 0u64;

    for seq in sequences() {
        for &dt in &seq.frames {
            fed += u64::from(dt);
            step.advance(u64::from(dt));
        }
    }
    assert_eq!(
        step.dropped_total_ms(),
        0,
        "an uncapped accumulator discarded time across {fed} ms of frames",
    );
}
