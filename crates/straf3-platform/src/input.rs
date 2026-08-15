//! Raw keyboard and mouse state, and the mouse-look accumulator.
//!
//! # What this module deliberately does not do
//!
//! It does not build [`straf3_sim::UserCmd`]s. It holds *what is held right
//! now* and *where the player is looking right now*; turning that into a
//! command of a particular duration is `straf3-game`'s job, because the tick
//! rate is game policy and the conversion has to be unit-testable with no
//! window in the process.
//!
//! # Absolute angles, not deltas
//!
//! [`straf3_sim::ViewAngles`] is absolute, and says why: a recorded delta
//! depends on the sensitivity, acceleration curve and polling rate of the
//! mouse that produced it, so a replay would only reproduce on the machine
//! that recorded it. [`MouseLook`] is therefore the *last* place a delta
//! exists. It accumulates deltas into an absolute orientation, and only the
//! orientation leaves this crate.
//!
//! Getting this backwards is the failure mode that compiles cleanly and
//! breaks replay equivalence silently, which is why it has its own test.

use straf3_sim::ViewAngles;
use straf3_sim::num::{Scalar, s};

/// Something the player can hold down.
///
/// Bound to *physical* keys, not layout-translated characters: WASD is a
/// shape on the keyboard, and a player who moved their keycaps around still
/// wants the shape.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum Action {
    /// Forward, `W`.
    MoveForward,
    /// Back, `S`.
    MoveBack,
    /// Strafe left, `A`.
    MoveLeft,
    /// Strafe right, `D`.
    MoveRight,
    /// Jump, `Space`.
    Jump,
    /// Crouch, `Ctrl`.
    Crouch,
    /// Walk (slow, quiet), `Shift`.
    Walk,
    /// Fire, left mouse button. Movement-only knockback (spec D3).
    Attack,
}

impl Action {
    /// Every action, in declaration order. Handy for tests and for rebinding
    /// UI that does not exist yet.
    pub const ALL: [Self; 8] = [
        Self::MoveForward,
        Self::MoveBack,
        Self::MoveLeft,
        Self::MoveRight,
        Self::Jump,
        Self::Crouch,
        Self::Walk,
        Self::Attack,
    ];

    const fn bit(self) -> u16 {
        1 << (self as u16)
    }
}

/// Absolute view orientation, accumulated from mouse motion.
///
/// Quake's sign conventions, because the simulation is Quake's: **negative
/// pitch is up**, and yaw *increases* to the left (`angle_vectors` builds
/// forward as `(cos yaw · cos pitch, sin yaw · cos pitch, −sin pitch)`, so a
/// larger yaw rotates from +X toward +Y).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct MouseLook {
    /// Degrees of rotation per unit of mouse motion, before [`Self::sensitivity`].
    ///
    /// Quake's `m_yaw` / `m_pitch`, both 0.022. Kept as one number because no
    /// part of straf3 wants a different horizontal and vertical scale, and a
    /// second field would just be a way to get them out of step.
    pub degrees_per_count: Scalar,
    /// Quake's `cl_sensitivity`, default 5 — so the default works out to
    /// 0.11°/count, the number every Q3 config in the world is written in.
    pub sensitivity: Scalar,
    /// How far up or down the player may look, in degrees from level.
    ///
    /// Q3 clamps to ±90; we stop a degree short so `cos(pitch)` never reaches
    /// zero and the forward vector never degenerates to straight down, which
    /// would leave the strafe axis undefined at exactly the wrong moment.
    pub pitch_limit: Scalar,
    pitch: Scalar,
    yaw: Scalar,
}

impl MouseLook {
    /// Quake's `m_yaw` and `m_pitch`.
    pub const DEGREES_PER_COUNT: Scalar = s(0.022);
    /// Quake's `cl_sensitivity` default.
    pub const DEFAULT_SENSITIVITY: Scalar = s(5.0);
    /// One degree short of straight up/down. See [`Self::pitch_limit`].
    pub const DEFAULT_PITCH_LIMIT: Scalar = s(89.0);

    /// Looking along `yaw`, level, at the default sensitivity.
    #[must_use]
    pub const fn looking_along(yaw: Scalar) -> Self {
        Self {
            degrees_per_count: Self::DEGREES_PER_COUNT,
            sensitivity: Self::DEFAULT_SENSITIVITY,
            pitch_limit: Self::DEFAULT_PITCH_LIMIT,
            pitch: s(0.0),
            yaw,
        }
    }

    /// Fold one mouse motion event in.
    ///
    /// `dx` grows to the right and `dy` grows *downwards*, which is winit's
    /// convention (and every mouse's). Turning right lowers yaw; looking down
    /// raises pitch. Both of those signs are Quake's, not a preference.
    pub fn apply_motion(&mut self, dx: Scalar, dy: Scalar) {
        let scale = self.degrees_per_count * self.sensitivity;
        self.yaw = wrap_degrees(self.yaw - dx * scale);
        self.pitch = (self.pitch + dy * scale).clamp(-self.pitch_limit, self.pitch_limit);
    }

    /// Point the view somewhere outright — spawning, teleporting, a replay
    /// seeking to a frame.
    pub fn set(&mut self, pitch: Scalar, yaw: Scalar) {
        self.pitch = pitch.clamp(-self.pitch_limit, self.pitch_limit);
        self.yaw = wrap_degrees(yaw);
    }

    /// The current absolute orientation, ready to go into a command.
    ///
    /// Roll is always zero: player input never rolls the view
    /// ([`ViewAngles::roll`] exists for the simulation's benefit, not the
    /// mouse's).
    #[must_use]
    pub const fn angles(&self) -> ViewAngles {
        ViewAngles {
            pitch: self.pitch,
            yaw: self.yaw,
            roll: s(0.0),
        }
    }

    /// Look up/down, in degrees. Negative is up.
    #[must_use]
    pub const fn pitch(&self) -> Scalar {
        self.pitch
    }

    /// Look left/right, in degrees, wrapped to `(-180, 180]`.
    #[must_use]
    pub const fn yaw(&self) -> Scalar {
        self.yaw
    }
}

impl Default for MouseLook {
    fn default() -> Self {
        Self::looking_along(s(0.0))
    }
}

/// Yaw folded into `(-180, 180]`.
///
/// Not cosmetic: an un-wrapped yaw grows without bound during a long session,
/// and `f32` loses a bit of angular resolution every time the exponent steps
/// up. At ±180 the resolution is ~1e-5°; at ±100 000° it is ~0.008°, which is
/// enough to feel in a strafe. Wrapping keeps every session's precision the
/// same as the first minute's.
fn wrap_degrees(degrees: Scalar) -> Scalar {
    let wrapped = degrees.rem_euclid(s(360.0));
    if wrapped > s(180.0) {
        wrapped - s(360.0)
    } else {
        wrapped
    }
}

/// Everything the player is doing at this instant.
///
/// Cheap to copy, and constructible without a window — `harness` drives it
/// from synthetic input, and so do the tests below.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct InputState {
    held: u16,
    /// Where the player is looking.
    pub look: MouseLook,
}

impl InputState {
    /// Nothing held, looking along `yaw`.
    #[must_use]
    pub const fn looking_along(yaw: Scalar) -> Self {
        Self {
            held: 0,
            look: MouseLook::looking_along(yaw),
        }
    }

    /// Press or release one action.
    pub fn set(&mut self, action: Action, pressed: bool) {
        if pressed {
            self.held |= action.bit();
        } else {
            self.held &= !action.bit();
        }
    }

    /// Whether an action is currently held.
    #[must_use]
    pub const fn is_held(&self, action: Action) -> bool {
        self.held & action.bit() != 0
    }

    /// Release everything.
    ///
    /// Called on focus loss: a key released while the window was not focused
    /// never produces a release event, and a player who alt-tabs mid-strafe
    /// should not come back still strafing.
    pub fn release_all(&mut self) {
        self.held = 0;
    }

    /// A signed axis from two opposed actions, `-1`, `0` or `1`.
    ///
    /// Both held is zero, exactly as Quake's `+forward`/`+back` cancel.
    #[must_use]
    pub const fn axis(&self, positive: Action, negative: Action) -> i8 {
        match (self.is_held(positive), self.is_held(negative)) {
            (true, false) => 1,
            (false, true) => -1,
            _ => 0,
        }
    }
}

impl Default for InputState {
    fn default() -> Self {
        Self::looking_along(s(0.0))
    }
}

// ── winit ──────────────────────────────────────────────────────────────────
//
// The only part of this module that names a windowing type, kept together so
// the boundary is one place rather than sprinkled through the state above.

/// The default binding for a physical key, or `None` if it is not bound.
#[must_use]
pub fn action_for_key(key: winit::keyboard::KeyCode) -> Option<Action> {
    use winit::keyboard::KeyCode as K;
    Some(match key {
        K::KeyW | K::ArrowUp => Action::MoveForward,
        K::KeyS | K::ArrowDown => Action::MoveBack,
        K::KeyA => Action::MoveLeft,
        K::KeyD => Action::MoveRight,
        K::Space => Action::Jump,
        K::ControlLeft | K::ControlRight | K::KeyC => Action::Crouch,
        K::ShiftLeft | K::ShiftRight => Action::Walk,
        _ => return None,
    })
}

impl InputState {
    /// Fold a window event in. Returns whether anything changed.
    ///
    /// Key *repeats* are ignored: the state is level-triggered, a repeat
    /// carries no new information, and passing them through would make the
    /// held-set depend on the OS keyboard repeat rate.
    pub fn apply_window_event(&mut self, event: &winit::event::WindowEvent) -> bool {
        use winit::event::{ElementState, MouseButton, WindowEvent};
        use winit::keyboard::PhysicalKey;

        match event {
            WindowEvent::KeyboardInput { event, .. } => {
                if event.repeat {
                    return false;
                }
                let PhysicalKey::Code(code) = event.physical_key else {
                    return false;
                };
                let Some(action) = action_for_key(code) else {
                    return false;
                };
                let pressed = event.state == ElementState::Pressed;
                if self.is_held(action) == pressed {
                    return false;
                }
                self.set(action, pressed);
                true
            }
            WindowEvent::MouseInput {
                state,
                button: MouseButton::Left,
                ..
            } => {
                let pressed = *state == ElementState::Pressed;
                if self.is_held(Action::Attack) == pressed {
                    return false;
                }
                self.set(Action::Attack, pressed);
                true
            }
            WindowEvent::Focused(false) => {
                let had = self.held != 0;
                self.release_all();
                had
            }
            _ => false,
        }
    }

    /// Fold a device event in. Returns whether anything changed.
    ///
    /// Mouse-look reads [`winit::event::DeviceEvent::MouseMotion`] and not
    /// `CursorMoved`, because only the former is un-accelerated raw motion
    /// that keeps working once the pointer is locked to the window. Reading
    /// cursor positions instead would stop producing motion at the screen
    /// edge — the classic bug where you cannot turn past 180°.
    pub fn apply_device_event(&mut self, event: &winit::event::DeviceEvent) -> bool {
        match event {
            winit::event::DeviceEvent::MouseMotion { delta: (dx, dy) } => {
                if *dx == 0.0 && *dy == 0.0 {
                    return false;
                }
                self.look.apply_motion(s(*dx as f32), s(*dy as f32));
                true
            }
            _ => false,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn actions_get_distinct_bits() {
        let mut seen = 0u16;
        for action in Action::ALL {
            assert_eq!(seen & action.bit(), 0, "{action:?} collides");
            seen |= action.bit();
        }
    }

    #[test]
    fn opposed_keys_cancel() {
        let mut input = InputState::default();
        assert_eq!(input.axis(Action::MoveForward, Action::MoveBack), 0);
        input.set(Action::MoveForward, true);
        assert_eq!(input.axis(Action::MoveForward, Action::MoveBack), 1);
        input.set(Action::MoveBack, true);
        assert_eq!(input.axis(Action::MoveForward, Action::MoveBack), 0);
        input.set(Action::MoveForward, false);
        assert_eq!(input.axis(Action::MoveForward, Action::MoveBack), -1);
    }

    #[test]
    fn focus_loss_releases_everything() {
        let mut input = InputState::default();
        input.set(Action::MoveForward, true);
        input.set(Action::Jump, true);
        input.release_all();
        assert!(Action::ALL.iter().all(|a| !input.is_held(*a)));
    }

    #[test]
    fn mouse_motion_accumulates_into_an_absolute_angle() {
        let mut look = MouseLook::looking_along(s(90.0));
        // 100 counts right at the default 0.11°/count is 11° of turn.
        look.apply_motion(s(100.0), s(0.0));
        assert!((look.yaw() - s(79.0)).abs() < s(1e-3), "{}", look.yaw());
        // And it is cumulative — the second delta does not replace the first.
        look.apply_motion(s(100.0), s(0.0));
        assert!((look.yaw() - s(68.0)).abs() < s(1e-3), "{}", look.yaw());
    }

    #[test]
    fn moving_the_mouse_up_looks_up_in_quakes_sign_convention() {
        let mut look = MouseLook::default();
        // winit's dy grows downwards, so "up" is negative.
        look.apply_motion(s(0.0), s(-100.0));
        assert!(look.pitch() < s(0.0), "up must be negative pitch");
        assert!((look.pitch() + s(11.0)).abs() < s(1e-3), "{}", look.pitch());
    }

    #[test]
    fn pitch_cannot_pass_straight_up_or_down() {
        let mut look = MouseLook::default();
        for _ in 0..100 {
            look.apply_motion(s(0.0), s(-1000.0));
        }
        assert_eq!(look.pitch(), -MouseLook::DEFAULT_PITCH_LIMIT);
        for _ in 0..200 {
            look.apply_motion(s(0.0), s(1000.0));
        }
        assert_eq!(look.pitch(), MouseLook::DEFAULT_PITCH_LIMIT);
    }

    #[test]
    fn yaw_wraps_instead_of_growing_without_bound() {
        let mut look = MouseLook::looking_along(s(0.0));
        for _ in 0..1_000 {
            // ~11° per call, so this is 30-odd full turns.
            look.apply_motion(s(100.0), s(0.0));
            assert!(
                look.yaw() > s(-180.0) && look.yaw() <= s(180.0),
                "yaw escaped its range: {}",
                look.yaw()
            );
        }
    }

    #[test]
    fn wrapping_lands_on_the_boundaries_the_way_it_says_it_does() {
        assert_eq!(wrap_degrees(s(0.0)), s(0.0));
        assert_eq!(wrap_degrees(s(180.0)), s(180.0));
        assert_eq!(wrap_degrees(s(-180.0)), s(180.0));
        assert_eq!(wrap_degrees(s(360.0)), s(0.0));
        assert_eq!(wrap_degrees(s(450.0)), s(90.0));
        assert_eq!(wrap_degrees(s(-90.0)), s(-90.0));
    }

    #[test]
    fn angles_leave_with_no_roll() {
        let mut look = MouseLook::looking_along(s(45.0));
        look.apply_motion(s(10.0), s(10.0));
        assert_eq!(look.angles().roll, s(0.0));
        assert_eq!(look.angles().yaw, look.yaw());
        assert_eq!(look.angles().pitch, look.pitch());
    }
}
