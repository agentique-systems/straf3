//! Turning held keys and an absolute view angle into a [`UserCmd`].
//!
//! # The property this module exists to hold (criterion 3)
//!
//! **Input becomes the same integer-millisecond [`UserCmd`] the simulation
//! already consumes.** There is no second path into the physics: the windowed
//! build produces exactly the commands `straf3-headless` reads from a file,
//! carrying exactly the same fields, and hands them to exactly the same
//! [`straf3_sim::step_in_place`]. The renderer feeds the seam; it never
//! reaches past it.
//!
//! Concretely that means this function must not be tempted into any of the
//! shortcuts a windowed game usually takes:
//!
//! - no float `dt` — `duration_ms` is the tick's whole-millisecond duration,
//!   handed in by the caller and never derived from a frame time;
//! - no mouse deltas — `view` is absolute, accumulated in
//!   [`straf3_platform::MouseLook`] and already whole when it arrives here;
//! - no analogue axes — the command axes are Quake's signed bytes, because
//!   keys are digital and an integer cannot smuggle a NaN into the physics.
//!
//! # Why the two spellings of jump are both sent
//!
//! Q3's client puts jump on the `upmove` axis (`+127`) and crouch on the same
//! axis (`-127`); this API additionally has [`Buttons::JUMP`] and
//! [`Buttons::CROUCH`] bits. `step` accepts either and documents them as
//! equivalent. We send both, because sending the axis is what a Q3 client
//! actually did — `PM_CmdScale` sees the same `upmove` it would have seen, so
//! the jump frame's small wish-speed dip is reproduced rather than
//! reconstructed.

use straf3_platform::{Action, InputState};
use straf3_sim::{Buttons, UserCmd};

/// The magnitude Q3's client sends on a fully-deflected command axis.
const AXIS_FULL: i8 = 127;

/// Build the command for one tick from the input held at this instant.
///
/// Pure: no clock, no window, no winit type in the signature. `harness` calls
/// it directly with a synthetic [`InputState`], which is the whole reason it
/// lives here rather than inside the event loop.
///
/// `duration_ms` is the tick's duration — [`straf3_sim::TickRate::command_millis`]
/// for the run's rate. It is a parameter and not a constant because the rate
/// is part of the physics (spec D2) and a recording has to be able to state
/// the one it ran at.
#[must_use]
pub fn command_from_input(input: &InputState, duration_ms: u16) -> UserCmd {
    let jump = input.is_held(Action::Jump);
    let crouch = input.is_held(Action::Crouch);

    let mut buttons = Buttons::NONE;
    if jump {
        buttons = buttons.with(Buttons::JUMP);
    }
    if crouch {
        buttons = buttons.with(Buttons::CROUCH);
    }
    if input.is_held(Action::Attack) {
        buttons = buttons.with(Buttons::ATTACK);
    }
    if input.is_held(Action::Walk) {
        buttons = buttons.with(Buttons::WALK);
    }

    // Jump wins over crouch on the shared axis, as it does in Q3: the axis has
    // one value and `PM_CheckJump` runs before the duck check.
    let up_move = if jump {
        AXIS_FULL
    } else if crouch {
        -AXIS_FULL
    } else {
        0
    };

    UserCmd {
        duration_ms,
        forward_move: input.axis(Action::MoveForward, Action::MoveBack) * AXIS_FULL,
        // Quake's "right" basis vector points along -Y at yaw 0, and the sign
        // is baked into every recorded `right_move` (see `angle_vectors`), so
        // `D` is positive here and must stay that way.
        right_move: input.axis(Action::MoveRight, Action::MoveLeft) * AXIS_FULL,
        up_move,
        buttons,
        view: input.look.angles(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use straf3_platform::MouseLook;
    use straf3_sim::TickRate;
    use straf3_sim::num::s;

    const TICK: u16 = 8;

    fn holding(actions: &[Action]) -> InputState {
        let mut input = InputState::default();
        for action in actions {
            input.set(*action, true);
        }
        input
    }

    #[test]
    fn an_idle_player_produces_a_still_command_of_the_right_length() {
        let cmd = command_from_input(&InputState::default(), TICK);
        assert_eq!(cmd, UserCmd::still_at(TickRate::HZ_125));
        assert_eq!(cmd.duration_ms, 8);
    }

    #[test]
    fn the_duration_is_whatever_the_caller_says_and_nothing_else() {
        for rate in [TickRate::HZ_76, TickRate::HZ_125, TickRate::HZ_250] {
            let cmd = command_from_input(&InputState::default(), rate.command_millis());
            assert_eq!(cmd.duration_ms, rate.command_millis());
        }
    }

    #[test]
    fn wasd_lands_on_quakes_axes_with_quakes_signs() {
        assert_eq!(
            command_from_input(&holding(&[Action::MoveForward]), TICK).forward_move,
            127
        );
        assert_eq!(
            command_from_input(&holding(&[Action::MoveBack]), TICK).forward_move,
            -127
        );
        assert_eq!(
            command_from_input(&holding(&[Action::MoveRight]), TICK).right_move,
            127
        );
        assert_eq!(
            command_from_input(&holding(&[Action::MoveLeft]), TICK).right_move,
            -127
        );
    }

    #[test]
    fn a_strafe_jump_is_one_key_and_one_strafe_key() {
        // The input that actually matters: forward + right + jump, looking
        // 45° off the direction of travel.
        let mut input = holding(&[Action::MoveForward, Action::MoveRight, Action::Jump]);
        input.look.set(s(0.0), s(45.0));
        let cmd = command_from_input(&input, TICK);

        assert_eq!(cmd.forward_move, 127);
        assert_eq!(cmd.right_move, 127);
        assert!(cmd.buttons.contains(Buttons::JUMP));
        assert_eq!(cmd.up_move, 127);
        assert_eq!(cmd.view.yaw, s(45.0));
    }

    #[test]
    fn jump_and_crouch_are_sent_both_ways_q3_spells_them() {
        let jump = command_from_input(&holding(&[Action::Jump]), TICK);
        assert!(jump.buttons.contains(Buttons::JUMP));
        assert_eq!(jump.up_move, 127);

        let crouch = command_from_input(&holding(&[Action::Crouch]), TICK);
        assert!(crouch.buttons.contains(Buttons::CROUCH));
        assert_eq!(crouch.up_move, -127);
    }

    #[test]
    fn jump_wins_the_shared_axis_when_both_are_held() {
        let both = command_from_input(&holding(&[Action::Jump, Action::Crouch]), TICK);
        assert_eq!(both.up_move, 127);
        // Both bits are still reported: the axis has one value, the buttons
        // do not, and `step` reads both.
        assert!(both.buttons.contains(Buttons::JUMP));
        assert!(both.buttons.contains(Buttons::CROUCH));
    }

    #[test]
    fn opposed_keys_cancel_rather_than_fighting() {
        let cmd = command_from_input(&holding(&[Action::MoveForward, Action::MoveBack]), TICK);
        assert_eq!(cmd.forward_move, 0);
    }

    #[test]
    fn the_view_travels_as_an_absolute_angle_not_a_delta() {
        // This is the one that catches the silent replay-breaking mistake:
        // accumulate two mouse motions, and the command must carry where the
        // player is *now looking*, not how far the mouse moved this tick.
        let mut input = InputState::default();
        input.look = MouseLook::looking_along(s(90.0));

        input.look.apply_motion(s(100.0), s(0.0)); // 11° right
        let first = command_from_input(&input, TICK);
        input.look.apply_motion(s(100.0), s(0.0)); // 11° more
        let second = command_from_input(&input, TICK);

        assert!((first.view.yaw - s(79.0)).abs() < s(1e-3));
        assert!((second.view.yaw - s(68.0)).abs() < s(1e-3));
        // A delta would have made these two equal. They must not be.
        assert_ne!(first.view.yaw, second.view.yaw);
        assert_eq!(second.view.roll, s(0.0));
    }

    #[test]
    fn a_command_holding_nothing_carries_no_buttons() {
        let cmd = command_from_input(&InputState::default(), TICK);
        assert_eq!(cmd.buttons, Buttons::NONE);
        assert_eq!(cmd.up_move, 0);
    }
}
