//! What the player did, and for how long.
//!
//! This module carries spec decision D2. Read [`UserCmd::duration_ms`] and
//! [`TickRate`] before changing anything here.

use crate::num::Scalar;

/// The simulation's command rate — an explicit, recorded parameter.
///
/// # Why this is a parameter and not a constant
///
/// Spec D2 chose option (b): a fixed tick that models Quake 3's
/// integer-millisecond command timing explicitly. The consequence is that the
/// command rate is **part of the physics**, not part of the frame loop. At
/// 125 Hz a command lasts 8 ms; at 250 Hz it lasts 4 ms; at 76 Hz it lasts
/// 13 ms (1000/76 = 13.15…, truncated — see [`TickRate::command_millis`]).
/// Those are different simulations, and Q3 players chose between them
/// deliberately with `com_maxfps`.
///
/// Because the rate is a recorded value rather than a hidden constant, a
/// replay reproduces exactly: it carries the rate it was recorded at, so a
/// run made at 125 Hz replays at 125 Hz on a machine rendering at 240 fps.
/// If this were a `const`, changing it would silently invalidate every
/// existing replay and ghost with nothing to detect it.
///
/// Rendering rate has no relationship to this value. The renderer interpolates
/// between simulation states.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct TickRate {
    hz: u32,
}

impl TickRate {
    /// 76 Hz — 13 ms commands. One of the rates Q3 players specifically chose
    /// for its jump behaviour.
    pub const HZ_76: Self = Self { hz: 76 };

    /// 125 Hz — 8 ms commands. The classic sweet spot and the default (D2).
    pub const HZ_125: Self = Self { hz: 125 };

    /// 250 Hz — 4 ms commands.
    pub const HZ_250: Self = Self { hz: 250 };

    /// The default command rate: 125 Hz, per spec D2.
    pub const DEFAULT: Self = Self::HZ_125;

    /// Build a rate from a frequency in hertz.
    ///
    /// Returns `None` outside `1..=1000`: below 1 Hz there is no sensible
    /// command, and above 1000 Hz a command would round to zero milliseconds,
    /// which integer-millisecond timing cannot represent.
    ///
    /// Any rate in range is accepted, not just the three named ones — the
    /// named rates are conveniences, and refusing arbitrary rates would make
    /// this a policy decision hidden in a constructor.
    #[must_use]
    pub const fn from_hz(hz: u32) -> Option<Self> {
        if hz >= 1 && hz <= 1000 {
            Some(Self { hz })
        } else {
            None
        }
    }

    /// The rate in hertz.
    #[must_use]
    pub const fn hz(self) -> u32 {
        self.hz
    }

    /// How long one command lasts, in whole milliseconds.
    ///
    /// **Truncating division, on purpose.** 1000/76 is 13.157…, and this
    /// returns 13, exactly as Q3's integer millisecond timing did. The lost
    /// fraction is not an error to be corrected; it is the mechanism that
    /// makes different `com_maxfps` values behave differently, which D2 chose
    /// to reproduce.
    #[must_use]
    pub const fn command_millis(self) -> u16 {
        (1000 / self.hz) as u16
    }
}

impl Default for TickRate {
    fn default() -> Self {
        Self::DEFAULT
    }
}

/// Buttons held during a command.
///
/// A newtype over a bitfield rather than a set of `bool`s so a command stays
/// small and cheap to record — a replay is a long list of these.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub struct Buttons(pub u16);

impl Buttons {
    /// Nothing held.
    pub const NONE: Self = Self(0);
    /// Jump.
    pub const JUMP: Self = Self(1 << 0);
    /// Crouch / duck.
    pub const CROUCH: Self = Self(1 << 1);
    /// Fire the active weapon. Movement-only: the impulse is knockback, there
    /// is no damage model (spec D3).
    pub const ATTACK: Self = Self(1 << 2);
    /// Walk (move slowly and silently).
    pub const WALK: Self = Self(1 << 3);

    /// Whether every button in `other` is held.
    #[must_use]
    pub const fn contains(self, other: Self) -> bool {
        self.0 & other.0 == other.0
    }

    /// This set plus `other`.
    #[must_use]
    pub const fn with(self, other: Self) -> Self {
        Self(self.0 | other.0)
    }

    /// This set without `other`.
    #[must_use]
    pub const fn without(self, other: Self) -> Self {
        Self(self.0 & !other.0)
    }
}

/// Where the player is looking, in degrees.
///
/// # Why absolute angles and not mouse deltas
///
/// Quake 3 sent absolute view angles in its user commands, and so do we. A
/// recorded delta would depend on the sensitivity, acceleration curve and
/// polling rate of the mouse that produced it, so a replay would only
/// reproduce on the machine and config that recorded it. Absolute angles make
/// a replay a property of the run, not of the hardware.
///
/// Strafejumping is *entirely* about the relationship between view angle and
/// velocity, so this is the input field the whole game turns on.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct ViewAngles {
    /// Look up/down. Negative is up, as in Quake.
    pub pitch: Scalar,
    /// Look left/right.
    pub yaw: Scalar,
    /// Roll. Player input never sets this; it exists because the simulation
    /// may (tilt effects, and it keeps the type a complete orientation).
    pub roll: Scalar,
}

/// One command: everything the player did during one tick, and how long that
/// tick lasted.
///
/// The simulation advances by consuming these and nothing else. It never asks
/// what time it is — see [`crate::step`].
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct UserCmd {
    /// How long this command lasts, **in whole milliseconds**.
    ///
    /// # Do not make this a float
    ///
    /// This is spec decision D2 in one field. Quake 3 truncated frame
    /// durations to integer milliseconds and integrated per command; the
    /// famous rate-coupled behaviour — the `com_maxfps 125` jump-height boost,
    /// framerate-dependent strafe efficiency — is a direct consequence of that
    /// truncation, not of the movement maths. An `f32` duration in seconds
    /// reproduces none of it, and cannot be retrofitted later without changing
    /// every call site and re-recording every replay.
    ///
    /// It is also what makes the simulation exactly reproducible: integers add
    /// without rounding, so accumulated simulation time is exact no matter how
    /// long the run is, and two runs of the same input are at the same instant
    /// on every tick.
    ///
    /// Normally this equals [`TickRate::command_millis`] for the run's
    /// recorded rate. It is stored per command rather than derived so a
    /// recording remains self-describing even if it were ever produced at a
    /// varying rate.
    pub duration_ms: u16,

    /// Forward/back axis, `-127..=127`, as Quake's signed-byte command axes.
    ///
    /// A small integer rather than a float because it comes from digital keys,
    /// and because an integer cannot smuggle a denormal or a NaN into the
    /// physics from outside.
    pub forward_move: i8,
    /// Strafe axis, `-127..=127`. The axis strafejumping is played on.
    pub right_move: i8,
    /// Up/down axis, `-127..=127`. Ladders and water; jump is a button.
    pub up_move: i8,

    /// Buttons held for this command.
    pub buttons: Buttons,

    /// Absolute view angles at the end of this command. See [`ViewAngles`].
    pub view: ViewAngles,
}

impl UserCmd {
    /// An empty command of the given duration: no keys, no buttons, looking
    /// straight ahead.
    #[must_use]
    pub const fn still(duration_ms: u16) -> Self {
        Self {
            duration_ms,
            forward_move: 0,
            right_move: 0,
            up_move: 0,
            buttons: Buttons::NONE,
            view: ViewAngles {
                pitch: 0.0,
                yaw: 0.0,
                roll: 0.0,
            },
        }
    }

    /// An empty command lasting one tick at `rate`.
    #[must_use]
    pub const fn still_at(rate: TickRate) -> Self {
        Self::still(rate.command_millis())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn named_rates_have_the_durations_quake_players_know() {
        assert_eq!(TickRate::HZ_125.command_millis(), 8);
        assert_eq!(TickRate::HZ_250.command_millis(), 4);
        // Truncated, not rounded: 1000/76 = 13.157…
        assert_eq!(TickRate::HZ_76.command_millis(), 13);
    }

    #[test]
    fn default_rate_is_125hz_per_d2() {
        assert_eq!(TickRate::default(), TickRate::HZ_125);
        assert_eq!(TickRate::default().hz(), 125);
    }

    #[test]
    fn rates_that_cannot_be_expressed_in_whole_milliseconds_are_rejected() {
        assert!(TickRate::from_hz(0).is_none());
        assert!(TickRate::from_hz(1001).is_none());
        assert_eq!(TickRate::from_hz(1000).unwrap().command_millis(), 1);
    }

    #[test]
    fn buttons_compose() {
        let b = Buttons::NONE.with(Buttons::JUMP).with(Buttons::CROUCH);
        assert!(b.contains(Buttons::JUMP));
        assert!(!b.without(Buttons::JUMP).contains(Buttons::JUMP));
        assert!(!Buttons::NONE.contains(Buttons::JUMP));
    }
}
