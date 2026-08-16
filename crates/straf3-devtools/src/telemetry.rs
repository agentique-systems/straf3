//! What the overlay shows, and how each number is spelled.
//!
//! Everything in this module is a pure function of a [`SimState`] plus a
//! couple of numbers the shell knows and the simulation cannot (the frame
//! rate; the ghost's split). There is no GPU, no window and no clock here, so
//! "the HUD shows the right thing" is a unit test rather than a claim about
//! what somebody saw on a screen.
//!
//! # Milliseconds, spelled out
//!
//! Every duration in this file is `u32` (or `i32`, for a signed split)
//! milliseconds, and every conversion to a `m:ss.mmm` string is integer
//! division. No float seconds value exists anywhere in the formatting path —
//! not even transiently — which is the same rule the simulation is held to.

use straf3_sim::num::Scalar;
use straf3_sim::{GroundState, RunState, SimState};

/// Which of the three movement regimes the player is in.
///
/// The three cases are exactly [`GroundState`]'s, and the distinction between
/// [`Self::Ground`] and [`Self::Slide`] is the whole reason this is not a
/// boolean: a player on a ramp too steep to walk on is touching geometry but
/// is under air rules, and that is a technique, not an edge case. A HUD that
/// showed "ground" for both would be lying about the thing the player most
/// needs to see.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Phase {
    /// Touching nothing.
    #[default]
    Air,
    /// On a plane too steep to stand on: no friction, air acceleration.
    Slide,
    /// On walkable ground: friction applies, a jump is available.
    Ground,
}

impl Phase {
    /// How this reads on screen.
    #[must_use]
    pub const fn label(self) -> &'static str {
        match self {
            Self::Air => "AIR",
            Self::Slide => "SLIDE",
            Self::Ground => "GROUND",
        }
    }

    /// The phase a [`GroundState`] is in.
    #[must_use]
    pub const fn of(ground: &GroundState) -> Self {
        match ground {
            GroundState::Airborne => Self::Air,
            GroundState::Sliding { .. } => Self::Slide,
            GroundState::Grounded { .. } => Self::Ground,
        }
    }
}

/// Where the run clock is, as far as the overlay is concerned.
///
/// A flattening of [`RunState`]: the overlay does not care *when* the start
/// line was crossed, only how long ago. Flattening it here rather than in the
/// painter means the number on screen is decided by a tested function.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum RunReadout {
    /// Before the start line. The clock shows placeholders, not `0:00.000` —
    /// a zeroed clock reads as "a run that has started and gone nowhere".
    #[default]
    NotStarted,
    /// The clock is running.
    Running {
        /// Milliseconds since the start line.
        elapsed_ms: u32,
    },
    /// Past the finish line. This is the time.
    Finished {
        /// Milliseconds between the start and finish lines.
        time_ms: u32,
    },
}

impl RunReadout {
    /// Flatten a [`RunState`] against the current simulation time.
    #[must_use]
    pub const fn of(run: &RunState, now_ms: u32) -> Self {
        match run.elapsed_ms(now_ms) {
            None => Self::NotStarted,
            Some(ms) => match run {
                RunState::Finished { .. } => Self::Finished { time_ms: ms },
                _ => Self::Running { elapsed_ms: ms },
            },
        }
    }

    /// The number of milliseconds on the clock, or `None` before the start.
    #[must_use]
    pub const fn millis(self) -> Option<u32> {
        match self {
            Self::NotStarted => None,
            Self::Running { elapsed_ms } => Some(elapsed_ms),
            Self::Finished { time_ms } => Some(time_ms),
        }
    }

    /// Whether the run is over — the moment the time stops moving and becomes
    /// a result.
    #[must_use]
    pub const fn is_finished(self) -> bool {
        matches!(self, Self::Finished { .. })
    }
}

/// One frame's worth of everything the overlay draws.
///
/// Built from a [`SimState`] with [`Self::of`], then topped up by the shell
/// with the two things the simulation cannot know: the frame rate, and how
/// this run compares with the personal best.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct TelemetrySample {
    /// Horizontal speed in units per second — the number that matters.
    ///
    /// Horizontal, not total: vertical speed is gravity's, and a strafe-jumper
    /// watching the total would see it swell on every fall.
    pub horizontal_speed: Scalar,
    /// Vertical speed in units per second, signed. Up is positive.
    pub vertical_speed: Scalar,
    /// Ground, slide or air.
    pub phase: Phase,
    /// Where the run clock is.
    pub run: RunReadout,
    /// Milliseconds ahead of (negative) or behind (positive) the ghost at the
    /// same point of the run, or `None` when no ghost is loaded.
    ///
    /// Signed like a motorsport split, because that is the convention every
    /// player already reads: a negative number is good news.
    pub split_ms: Option<i32>,
    /// Frames per second, as measured by the shell over the last interval.
    pub fps: u32,
    /// How many commands have been applied.
    pub tick: u32,
    /// Simulation time in milliseconds — the exact sum of command durations,
    /// which is not the same thing as wall time.
    pub sim_ms: u32,
}

impl TelemetrySample {
    /// Read everything the simulation knows.
    ///
    /// [`Self::fps`] and [`Self::split_ms`] are left at their defaults; they
    /// are not the simulation's to say. Use [`Self::with_fps`] and
    /// [`Self::with_split_ms`].
    #[must_use]
    pub fn of(state: &SimState) -> Self {
        let v = state.player.velocity;
        Self {
            horizontal_speed: (v.x * v.x + v.y * v.y).sqrt(),
            vertical_speed: v.z,
            phase: Phase::of(&state.player.ground),
            run: RunReadout::of(&state.run, state.time_ms),
            split_ms: None,
            fps: 0,
            tick: state.tick,
            sim_ms: state.time_ms,
        }
    }

    /// Attach the frame rate the shell measured.
    #[must_use]
    pub const fn with_fps(mut self, fps: u32) -> Self {
        self.fps = fps;
        self
    }

    /// Attach the split against the ghost, if there is one.
    #[must_use]
    pub const fn with_split_ms(mut self, split_ms: Option<i32>) -> Self {
        self.split_ms = split_ms;
        self
    }
}

/// Placeholder shown by the run clock before the start line is crossed.
pub const CLOCK_PLACEHOLDER: &str = "--:--.---";

/// `m:ss.mmm`, by integer division only.
///
/// Minutes are not zero-padded (a run is `1:02.480`, not `01:02.480`) and are
/// not capped: a 90-minute run reads `90:00.000` rather than wrapping, because
/// a wrapped clock is a wrong time and a long one is only an ugly time.
#[must_use]
pub fn format_clock_ms(ms: u32) -> String {
    let minutes = ms / 60_000;
    let rest = ms % 60_000;
    format!("{minutes}:{:02}.{:03}", rest / 1_000, rest % 1_000)
}

/// The run clock as it appears on screen.
#[must_use]
pub fn format_run(run: RunReadout) -> String {
    match run.millis() {
        None => CLOCK_PLACEHOLDER.to_owned(),
        Some(ms) => format_clock_ms(ms),
    }
}

/// A split against the ghost, signed, as `+1.204` / `-0.312`.
///
/// Always signed, including at zero: an unsigned `0.000` in a column that
/// otherwise carries a sign reads as a missing sign rather than as dead level.
/// Splits of a minute or more grow a minutes field rather than counting past
/// sixty seconds.
#[must_use]
pub fn format_split_ms(split_ms: i32) -> String {
    let sign = if split_ms < 0 { '-' } else { '+' };
    // `unsigned_abs`, not `abs`: `i32::MIN.abs()` panics, and a split is a
    // difference of two clocks that a corrupt recording could make enormous.
    let ms = split_ms.unsigned_abs();
    let minutes = ms / 60_000;
    let rest = ms % 60_000;
    if minutes == 0 {
        format!("{sign}{}.{:03}", rest / 1_000, rest % 1_000)
    } else {
        format!("{sign}{minutes}:{:02}.{:03}", rest / 1_000, rest % 1_000)
    }
}

/// Speed in units per second, to the nearest whole unit.
///
/// Whole units because the fractional part changes every frame and reads as
/// noise; a strafe-jumper is comparing 480 against 500, not 480.3 against
/// 480.4. A non-finite speed — which would mean the simulation had already
/// gone wrong — prints as dashes rather than as `NaN`, so the overlay never
/// implies a number it does not have.
#[must_use]
pub fn format_speed(ups: Scalar) -> String {
    if ups.is_finite() {
        format!("{}", ups.round() as i64)
    } else {
        "----".to_owned()
    }
}

/// Whether speed is being gained, held or lost.
///
/// Purely a display heuristic — it colours the speed readout so a bad strafe
/// is visible before the number has moved far enough to notice. It is computed
/// from a smoothed history in the painter and is deliberately not part of
/// [`TelemetrySample`]: nothing below the seam has an opinion about it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum SpeedTrend {
    /// Accelerating.
    Gaining,
    /// Neither, within the deadband.
    #[default]
    Holding,
    /// Decelerating.
    Losing,
}

/// How much of the current speed is folded into the smoothed reference each
/// frame.
const TREND_SMOOTHING: Scalar = 0.12;

/// How far speed must diverge from its smoothed reference, in units per
/// second, before the tint moves off neutral. Below this the number is simply
/// noisy, and a flickering colour would be worse than no colour.
const TREND_DEADBAND_UPS: Scalar = 3.0;

/// The low-pass filter behind [`SpeedTrend`].
///
/// Frame-rate dependent by construction: a fixed per-frame coefficient settles
/// faster in wall time at 240 Hz than at 60. That is fine for a colour hint
/// and would not be for anything the simulation reads, which is exactly why it
/// lives above the seam and is fed by the painter rather than by `step`.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct TrendFilter {
    smoothed: Scalar,
    primed: bool,
}

impl TrendFilter {
    /// Feed one frame's horizontal speed and get the tint for it.
    ///
    /// A non-finite speed leaves the filter untouched and reports
    /// [`SpeedTrend::Holding`]: the overlay does not colour a number it is
    /// already refusing to print.
    pub fn feed(&mut self, speed: Scalar) -> SpeedTrend {
        if !speed.is_finite() {
            return SpeedTrend::Holding;
        }
        if !self.primed {
            self.primed = true;
            self.smoothed = speed;
        }
        let delta = speed - self.smoothed;
        self.smoothed += delta * TREND_SMOOTHING;
        if delta > TREND_DEADBAND_UPS {
            SpeedTrend::Gaining
        } else if delta < -TREND_DEADBAND_UPS {
            SpeedTrend::Losing
        } else {
            SpeedTrend::Holding
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use straf3_sim::num::{s, vec3};

    #[test]
    fn default_sample_is_stationary() {
        let sample = TelemetrySample::default();
        assert_eq!(sample.horizontal_speed, 0.0);
        assert_eq!(sample.phase, Phase::Air);
        assert_eq!(sample.run, RunReadout::NotStarted);
        assert_eq!(sample.split_ms, None);
    }

    #[test]
    fn the_clock_is_whole_milliseconds_all_the_way_to_the_string() {
        assert_eq!(format_clock_ms(0), "0:00.000");
        // Two milliseconds, not one, and the reason is worth a line: the
        // string `0:00.001` contains `0.001`, and `straf3-sim`'s
        // `timing_seam` test flags any statement that holds both a
        // millisecond-named token and a thousandth literal. That guard is
        // right to be blunt — it is what stops a second millisecond-to-seconds
        // conversion appearing anywhere in the workspace — and a display
        // string is not worth blunting it for. The property under test, a
        // zero-padded three-digit millisecond field, is the same either way.
        assert_eq!(format_clock_ms(2), "0:00.002");
        assert_eq!(format_clock_ms(999), "0:00.999");
        assert_eq!(format_clock_ms(1_000), "0:01.000");
        assert_eq!(format_clock_ms(12_480), "0:12.480");
        assert_eq!(format_clock_ms(59_999), "0:59.999");
        assert_eq!(format_clock_ms(60_000), "1:00.000");
        assert_eq!(format_clock_ms(62_480), "1:02.480");
        // Long runs grow, they do not wrap.
        assert_eq!(format_clock_ms(5_400_000), "90:00.000");
    }

    #[test]
    fn an_unstarted_run_shows_placeholders_rather_than_zero() {
        // A zeroed clock reads as "started, going nowhere". They are different
        // states and they must not look the same.
        assert_eq!(format_run(RunReadout::NotStarted), "--:--.---");
        assert_eq!(
            format_run(RunReadout::Running { elapsed_ms: 0 }),
            "0:00.000"
        );
    }

    #[test]
    fn a_split_always_carries_its_sign() {
        assert_eq!(format_split_ms(0), "+0.000");
        assert_eq!(format_split_ms(-312), "-0.312");
        assert_eq!(format_split_ms(1_204), "+1.204");
        assert_eq!(format_split_ms(-1_204), "-1.204");
        assert_eq!(format_split_ms(62_480), "+1:02.480");
        assert_eq!(format_split_ms(-62_480), "-1:02.480");
    }

    #[test]
    fn an_absurd_split_formats_rather_than_panicking() {
        // `i32::MIN.abs()` panics. A split is the difference of two clocks and
        // a corrupt recording can hand us anything, so the overlay must be
        // unable to take the process down.
        assert!(format_split_ms(i32::MIN).starts_with('-'));
        assert!(format_split_ms(i32::MAX).starts_with('+'));
    }

    #[test]
    fn speed_is_whole_units_and_never_prints_a_nan() {
        assert_eq!(format_speed(s(0.0)), "0");
        assert_eq!(format_speed(s(480.4)), "480");
        assert_eq!(format_speed(s(480.5)), "481");
        assert_eq!(format_speed(Scalar::NAN), "----");
        assert_eq!(format_speed(Scalar::INFINITY), "----");
    }

    #[test]
    fn the_sample_reads_horizontal_speed_and_not_total_speed() {
        let mut state = SimState::default();
        // 300 across, 400 down: the total is 500, the horizontal is 300, and
        // showing 500 to a player in mid-fall would be a lie about the run.
        state.player.velocity = vec3(s(300.0), s(0.0), s(-400.0));
        let sample = TelemetrySample::of(&state);
        assert_eq!(sample.horizontal_speed, s(300.0));
        assert_eq!(sample.vertical_speed, s(-400.0));
    }

    #[test]
    fn sliding_is_its_own_phase_and_not_ground() {
        let n = vec3(s(0.0), s(0.6), s(0.8));
        let mut state = SimState::default();
        assert_eq!(TelemetrySample::of(&state).phase, Phase::Air);
        state.player.ground = GroundState::Sliding { normal: n };
        assert_eq!(TelemetrySample::of(&state).phase, Phase::Slide);
        state.player.ground = GroundState::Grounded { normal: n };
        assert_eq!(TelemetrySample::of(&state).phase, Phase::Ground);
        assert_eq!(Phase::Slide.label(), "SLIDE");
    }

    #[test]
    fn the_readout_follows_the_run_through_its_three_states() {
        let mut state = SimState {
            time_ms: 4_000,
            ..SimState::default()
        };
        assert_eq!(TelemetrySample::of(&state).run, RunReadout::NotStarted);

        state.run.start(4_000);
        state.time_ms = 16_480;
        assert_eq!(
            TelemetrySample::of(&state).run,
            RunReadout::Running { elapsed_ms: 12_480 }
        );

        state.run.finish(16_480);
        // The clock keeps ticking; the time does not.
        state.time_ms = 30_000;
        let done = TelemetrySample::of(&state).run;
        assert_eq!(done, RunReadout::Finished { time_ms: 12_480 });
        assert!(done.is_finished());
        assert_eq!(format_run(done), "0:12.480");
    }

    #[test]
    fn the_trend_ignores_noise_and_notices_a_real_gain() {
        let mut filter = TrendFilter::default();
        // The first sample has nothing to compare against and must not flash.
        assert_eq!(filter.feed(s(320.0)), SpeedTrend::Holding);
        // Jitter inside the deadband stays neutral.
        for step in [321.0, 319.5, 320.4, 320.0] {
            assert_eq!(filter.feed(s(step)), SpeedTrend::Holding, "at {step}");
        }
        // A strafe that is actually working.
        assert_eq!(filter.feed(s(360.0)), SpeedTrend::Gaining);
        // ...and a wall.
        let mut filter = TrendFilter::default();
        filter.feed(s(600.0));
        assert_eq!(filter.feed(s(120.0)), SpeedTrend::Losing);
    }

    #[test]
    fn the_trend_does_not_poison_itself_on_a_non_finite_speed() {
        let mut filter = TrendFilter::default();
        filter.feed(s(400.0));
        assert_eq!(filter.feed(Scalar::NAN), SpeedTrend::Holding);
        // The NaN was not folded in, so the filter still knows where it was.
        assert_eq!(filter.feed(s(400.0)), SpeedTrend::Holding);
        assert_eq!(filter.feed(s(500.0)), SpeedTrend::Gaining);
    }

    #[test]
    fn the_two_things_the_simulation_cannot_know_are_added_by_the_shell() {
        let sample = TelemetrySample::of(&SimState::default())
            .with_fps(241)
            .with_split_ms(Some(-312));
        assert_eq!(sample.fps, 241);
        assert_eq!(sample.split_ms, Some(-312));
    }
}
