//! What the player did, and for how long.
//!
//! This module carries spec decision D2. Read [`UserCmd::duration_ms`] and
//! [`TickRate`] before changing anything here.

use crate::num::{Scalar, s};

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
///
/// # Why 16 bits per axis (contract item C3)
///
/// Each axis is one of 65 536 evenly spaced directions — Q3's `ANGLE2SHORT` —
/// giving [`ANGLE_RESOLUTION`], about 0.0055°, which is roughly a twentieth of
/// the smallest motion a default-sensitivity mouse can report.
///
/// The point is not the wire size, though the format gets it for free. Mouse
/// input is a stream of deltas accumulated into an angle, and if the recorded
/// value is whatever `f32` that accumulator happens to hold, its value depends
/// on the order and magnitude of every delta since the run began. Two clients
/// with the same *intent* have no reason to produce the same number, and a
/// recording is only exactly reproducible if the number it carries is exactly
/// the number that was simulated. Quantising here — at the command boundary,
/// where the input becomes physics — makes the input space finite and removes
/// the whole question. There is no representation in which a recording can
/// round-trip imperfectly, because the recorded value *is* the simulated one.
///
/// Quantising here and not at the input sample is deliberate: the platform
/// layer's accumulator stays a full-precision angle, so mouse motion still
/// feels continuous and repeated small deltas still add up. Only the command
/// it hands over is snapped.
///
/// The second reason is range. [`crate::num::sin_cos`] reduces its argument in
/// `f64`, which caps cancellation at 53 bits and so loses accuracy for
/// arguments past a few thousand degrees. A `u16` cannot express an angle
/// outside one turn, so a session that turns for an hour cannot walk the view
/// angle out into that region — the degraded range is unreachable by
/// construction rather than by a convention someone has to remember.
///
/// # Reading them
///
/// The physics wants degrees: [`Self::pitch_degrees`], [`Self::yaw_degrees`]
/// and [`Self::roll_degrees`] are `SHORT2ANGLE`, and return a value in
/// `[0, 360)`. Everything that consumes them goes through trigonometry, for
/// which 315° and −45° are the same direction.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub struct ViewAngles {
    /// Look up/down, as a 16-bit angle. Negative is up, as in Quake — which
    /// after quantisation means looking up is just under 65 536, not below
    /// zero. Use [`Self::pitch_degrees`] to read it.
    pub pitch: u16,
    /// Look left/right, as a 16-bit angle. See [`Self::yaw_degrees`].
    pub yaw: u16,
    /// Roll, as a 16-bit angle. Player input never sets this; it exists
    /// because the simulation may (tilt effects, and it keeps the type a
    /// complete orientation).
    pub roll: u16,
}

/// Degrees per unit of a 16-bit angle: 360/65536, about 0.0055°.
///
/// Exact in binary — 360/65536 is 45·2⁻¹³ — which is what makes
/// [`short_to_angle`] an exact operation rather than an approximate one.
pub const ANGLE_RESOLUTION: Scalar = s(360.0 / 65536.0);

/// 16-bit angle units per degree: 65536/360. The reciprocal of
/// [`ANGLE_RESOLUTION`], and unlike it *not* exactly representable — which is
/// why [`angle_to_short`] rounds to nearest rather than truncating.
const SHORTS_PER_DEGREE: Scalar = s(65536.0 / 360.0);

/// Quantise an angle in degrees to Q3's 16-bit form (`ANGLE2SHORT`).
///
/// Angles outside one turn wrap into it, so this is total: every `Scalar`,
/// including a negative one, has an answer. (`NaN` and the infinities are not
/// meaningful view angles; they land on a value rather than trapping, because
/// float-to-integer casts in Rust saturate.)
///
/// # Why round-to-nearest and not Q3's truncation
///
/// Q3 wrote `(int)(x * 65536 / 360) & 65535` — a truncation. Truncating is not
/// idempotent here: `65536/360` is not exactly representable, so
/// `short_to_angle` followed by a truncating `angle_to_short` lands a hair
/// below the original short about half the time and truncation then throws the
/// whole unit away. That would make a recording that is *written* as degrees
/// and *read back* as shorts lose a step per round trip, which is exactly the
/// property C3 exists to guarantee. Rounding to nearest recovers the original
/// short for all 65 536 of them — asserted exhaustively in this module's
/// tests.
///
/// The rounding adds a half and truncates rather than calling `round`, which
/// is the trick [`crate::num`] uses for the same reason: it reaches no libm
/// entry point, so the result is fixed by IEEE arithmetic alone and cannot
/// differ between a glibc build and a wasm one.
#[inline]
#[must_use]
pub fn angle_to_short(degrees: Scalar) -> u16 {
    let scaled = degrees * SHORTS_PER_DEGREE;
    let half = if scaled < s(0.0) { s(-0.5) } else { s(0.5) };
    // `as i64` saturates rather than wrapping or trapping, which is what keeps
    // this total for absurd inputs; the mask is Q3's `& 65535`, and it is what
    // folds an angle outside one turn back into it.
    ((scaled + half) as i64 & 0xffff) as u16
}

/// The angle a 16-bit view angle stands for, in degrees `[0, 360)`
/// (`SHORT2ANGLE`).
///
/// Exact: [`ANGLE_RESOLUTION`] is a power-of-two fraction and the product
/// needs at most 22 bits of mantissa, so no rounding happens at all.
#[inline]
#[must_use]
pub fn short_to_angle(short: u16) -> Scalar {
    s(f32::from(short)) * ANGLE_RESOLUTION
}

impl ViewAngles {
    /// Looking along +X, level, no roll.
    pub const LEVEL: Self = Self {
        pitch: 0,
        yaw: 0,
        roll: 0,
    };

    /// Quantise angles in degrees. See [`angle_to_short`].
    #[must_use]
    pub fn from_degrees(pitch: Scalar, yaw: Scalar, roll: Scalar) -> Self {
        Self {
            pitch: angle_to_short(pitch),
            yaw: angle_to_short(yaw),
            roll: angle_to_short(roll),
        }
    }

    /// Level, no roll, looking along `yaw` degrees.
    #[must_use]
    pub fn looking_along(yaw: Scalar) -> Self {
        Self::from_degrees(s(0.0), yaw, s(0.0))
    }

    /// Look up/down in degrees, `[0, 360)`. Up is just under 360.
    #[must_use]
    pub fn pitch_degrees(self) -> Scalar {
        short_to_angle(self.pitch)
    }

    /// Look left/right in degrees, `[0, 360)`.
    #[must_use]
    pub fn yaw_degrees(self) -> Scalar {
        short_to_angle(self.yaw)
    }

    /// Roll in degrees, `[0, 360)`.
    #[must_use]
    pub fn roll_degrees(self) -> Scalar {
        short_to_angle(self.roll)
    }
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
            view: ViewAngles::LEVEL,
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

    /// The property every recording rests on: a 16-bit angle, written out as
    /// degrees and quantised again, is the *same* 16-bit angle. Exhaustive,
    /// because "almost always" is not a property a replay format can be built
    /// on — and because the obvious implementation (Q3's truncating
    /// `ANGLE2SHORT`) fails this for roughly half of the 65 536.
    #[test]
    fn every_short_survives_a_round_trip_through_degrees() {
        for short in 0..=u16::MAX {
            assert_eq!(
                angle_to_short(short_to_angle(short)),
                short,
                "short {short} did not come back"
            );
        }
    }

    #[test]
    fn the_scale_is_quakes_angle2short() {
        // The four cardinal directions, at the exact shorts Q3's macro gives.
        assert_eq!(angle_to_short(s(0.0)), 0);
        assert_eq!(angle_to_short(s(90.0)), 16_384);
        assert_eq!(angle_to_short(s(180.0)), 32_768);
        assert_eq!(angle_to_short(s(270.0)), 49_152);
        assert_eq!(short_to_angle(16_384), s(90.0));
        assert_eq!(short_to_angle(49_152), s(270.0));
    }

    #[test]
    fn angles_outside_one_turn_wrap_into_it() {
        // What makes the sin_cos accuracy argument hold by construction: no
        // view angle can escape one turn, however long the session.
        assert_eq!(angle_to_short(s(-90.0)), angle_to_short(s(270.0)));
        assert_eq!(angle_to_short(s(360.0)), 0);
        assert_eq!(angle_to_short(s(45.0) + s(720.0)), angle_to_short(s(45.0)));
        assert_eq!(angle_to_short(s(-45.0)), 57_344); // 65 536 − 8 192
    }

    #[test]
    fn the_resolution_is_finer_than_a_mouse_can_ask_for() {
        // Q3's m_yaw · cl_sensitivity default is 0.11°/count. One quantum is
        // ~0.0055°, so the smallest motion a mouse can report is still 20
        // distinct angles wide: quantising cannot be felt.
        assert!(ANGLE_RESOLUTION < s(0.11) / s(20.0));
        assert_eq!(short_to_angle(1), ANGLE_RESOLUTION);
        // Adjacent shorts are adjacent angles — no gaps, no repeats.
        assert_ne!(angle_to_short(s(0.0)), angle_to_short(ANGLE_RESOLUTION));
    }

    #[test]
    fn quantising_lands_on_the_nearest_representable_angle() {
        // Half a quantum below a short rounds down to it, half above rounds
        // to the next: the error is never more than half a quantum, which is
        // the strongest statement a quantiser can make.
        for short in [0_u16, 1, 4_095, 16_384, 32_768, 65_535] {
            let exact = short_to_angle(short);
            let nudge = ANGLE_RESOLUTION * s(0.4);
            assert_eq!(angle_to_short(exact + nudge), short);
            if short > 0 {
                assert_eq!(angle_to_short(exact - nudge), short);
            }
        }
    }

    #[test]
    fn a_still_command_is_looking_along_x() {
        let cmd = UserCmd::still(8);
        assert_eq!(cmd.view, ViewAngles::LEVEL);
        assert_eq!(cmd.view.yaw_degrees(), s(0.0));
        assert_eq!(cmd.view.pitch_degrees(), s(0.0));
    }

    #[test]
    fn from_degrees_and_back_agrees_with_the_free_functions() {
        let v = ViewAngles::from_degrees(s(-12.5), s(137.25), s(3.0));
        assert_eq!(v.pitch, angle_to_short(s(-12.5)));
        assert_eq!(v.yaw_degrees(), short_to_angle(angle_to_short(s(137.25))));
        assert_eq!(ViewAngles::looking_along(s(90.0)).yaw, 16_384);
        assert_eq!(ViewAngles::looking_along(s(90.0)).roll, 0);
    }

    #[test]
    fn buttons_compose() {
        let b = Buttons::NONE.with(Buttons::JUMP).with(Buttons::CROUCH);
        assert!(b.contains(Buttons::JUMP));
        assert!(!b.without(Buttons::JUMP).contains(Buttons::JUMP));
        assert!(!Buttons::NONE.contains(Buttons::JUMP));
    }
}
