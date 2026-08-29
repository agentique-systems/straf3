//! The fixed-step accumulator: how a variable frame rate drives a fixed
//! simulation cadence.
//!
//! # The property this module exists to hold (criterion 5)
//!
//! **Frame pacing is decoupled from simulation stepping.** The simulation
//! advances in commands of a fixed integer duration — 8 ms at the default 125
//! Hz — and it does so whether the machine drew one frame or four hundred in
//! that time. A frame is not a tick. A stuttering frame rate changes how often
//! the player *sees* the world; it must not change where the player *is*.
//!
//! Getting this wrong is the single most common way a game becomes
//! irreproducible, and it does not look like a bug: stepping the simulation
//! by "however long the last frame took" produces a perfectly smooth-looking
//! game whose every run is different, and whose replays desync on any machine
//! that renders at a different speed.
//!
//! # How the carry works
//!
//! Wall time arrives in whole milliseconds ([`straf3_platform::Clock`]) and is
//! added to a carried remainder. Whole ticks are spent, the sub-tick remainder
//! is kept for the next frame. Everything is integer arithmetic, so the
//! accumulated time is *exact*: after any sequence of frame deltas summing to
//! `T` ms, the simulation has advanced `(T - carried) / tick_ms` ticks with
//! nothing lost to rounding, however long the session runs.
//!
//! The remainder doubles as the render interpolation parameter — see
//! [`FixedStep::alpha`].

use straf3_sim::{PhysicsProfile, SimState, TickRate, TriggerSet, UserCmd, World};

/// The command rate straf3 runs at unless told otherwise: 125 Hz, 8 ms
/// commands, per spec D2.
pub const DEFAULT_RATE: TickRate = TickRate::HZ_125;

/// What a frame's worth of elapsed wall time buys.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct TickPlan {
    /// How many fixed-duration commands to run this frame.
    pub ticks: u32,
    /// Sub-tick wall time carried into the next frame.
    pub remainder_ms: u32,
    /// Wall time thrown away because [`plan_ticks`]'s cap was reached.
    ///
    /// Reported rather than swallowed. A silent cap is indistinguishable from
    /// correct pacing right up until someone wonders why a run recorded over a
    /// tab-switch is shorter than the wall clock says.
    pub dropped_ms: u64,
}

/// Split elapsed wall time into whole ticks plus a carried remainder.
///
/// Pure integer arithmetic on purpose — no clock, no state, no float. This is
/// the function criterion 5 is actually about, and it is testable with
/// synthetic deltas alone.
///
/// `max_ticks` bounds how much simulation one frame may be asked to run. It is
/// not an optimisation: without it, a frame that took two minutes (a laptop
/// lid closing, a browser tab suspended) asks for 15 000 ticks in one frame,
/// each of which takes longer than a frame to compute, so the next delta is
/// larger still. That is the classic spiral of death, and the way out is to
/// admit the lost time rather than try to catch up on it.
#[must_use]
pub const fn plan_ticks(
    elapsed_ms: u64,
    carried_ms: u32,
    tick_ms: u16,
    max_ticks: u32,
) -> TickPlan {
    if tick_ms == 0 {
        // Not reachable through `TickRate`, which cannot produce a zero-length
        // command. Handled anyway, because the alternative is a divide by zero
        // in the one function the whole loop runs through.
        return TickPlan {
            ticks: 0,
            remainder_ms: carried_ms,
            dropped_ms: elapsed_ms,
        };
    }

    let tick = tick_ms as u64;
    let available = carried_ms as u64 + elapsed_ms;
    let wanted = available / tick;
    // `available % tick < tick <= u16::MAX`, so this narrowing is lossless.
    let remainder_ms = (available % tick) as u32;

    if wanted > max_ticks as u64 {
        return TickPlan {
            ticks: max_ticks,
            remainder_ms,
            dropped_ms: (wanted - max_ticks as u64) * tick,
        };
    }

    TickPlan {
        ticks: wanted as u32,
        remainder_ms,
        dropped_ms: 0,
    }
}

/// The accumulator, carrying its remainder between frames.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FixedStep {
    rate: TickRate,
    carried_ms: u32,
    max_ticks_per_frame: u32,
    dropped_total_ms: u64,
}

impl FixedStep {
    /// Two seconds of simulation in one frame, at the default rate.
    ///
    /// Generous on purpose: it must never fire during ordinary stutter (a
    /// 200 ms hitch is 25 ticks), only when the process was genuinely not
    /// running. Choosing it too low would silently slow the game down under
    /// load, which is exactly the coupling this module exists to prevent.
    pub const DEFAULT_MAX_TICKS_PER_FRAME: u32 = 250;

    /// An accumulator at `rate`, starting with nothing carried.
    #[must_use]
    pub const fn new(rate: TickRate) -> Self {
        Self {
            rate,
            carried_ms: 0,
            max_ticks_per_frame: Self::DEFAULT_MAX_TICKS_PER_FRAME,
            dropped_total_ms: 0,
        }
    }

    /// Change the per-frame cap. See [`plan_ticks`].
    #[must_use]
    pub const fn with_max_ticks_per_frame(mut self, max_ticks: u32) -> Self {
        self.max_ticks_per_frame = max_ticks;
        self
    }

    /// The command rate this accumulator paces.
    #[must_use]
    pub const fn rate(&self) -> TickRate {
        self.rate
    }

    /// How long one command lasts, in whole milliseconds.
    #[must_use]
    pub const fn tick_ms(&self) -> u16 {
        self.rate.command_millis()
    }

    /// Sub-tick wall time not yet spent.
    #[must_use]
    pub const fn carried_ms(&self) -> u32 {
        self.carried_ms
    }

    /// Total wall time dropped to the per-frame cap over this session.
    #[must_use]
    pub const fn dropped_total_ms(&self) -> u64 {
        self.dropped_total_ms
    }

    /// Spend `elapsed_ms` of wall time and report how many ticks to run.
    pub fn advance(&mut self, elapsed_ms: u64) -> u32 {
        let plan = plan_ticks(
            elapsed_ms,
            self.carried_ms,
            self.tick_ms(),
            self.max_ticks_per_frame,
        );
        self.carried_ms = plan.remainder_ms;
        if plan.dropped_ms > 0 {
            self.dropped_total_ms += plan.dropped_ms;
            log::warn!(
                "dropped {} ms of simulation time: {} ticks wanted, {} allowed in one frame",
                plan.dropped_ms,
                plan.ticks as u64 + plan.dropped_ms / self.tick_ms() as u64,
                self.max_ticks_per_frame
            );
        }
        plan.ticks
    }

    /// How far between the last two simulation states the next frame sits, in
    /// `0.0..1.0`.
    ///
    /// This is the whole reason the remainder is kept rather than rounded
    /// away: it lets the renderer draw the player *between* two 8 ms states,
    /// so a 144 Hz display shows smooth motion from a 125 Hz simulation
    /// without the simulation knowing the display exists.
    #[must_use]
    pub fn alpha(&self) -> f32 {
        f32::from(u16::try_from(self.carried_ms).unwrap_or(u16::MAX)) / f32::from(self.tick_ms())
    }
}

impl Default for FixedStep {
    fn default() -> Self {
        Self::new(DEFAULT_RATE)
    }
}

/// Advance the simulation by exactly one command, reporting the timing volumes
/// the player passed through during it.
///
/// A one-line wrapper, and that is the point: it is the *only* path from the
/// windowed build into the simulation, so "the renderer changes nothing below
/// the seam" (criterion 4) is a claim about this function and nothing else.
/// `harness` diffs it against calling [`straf3_sim::step_in_place`] directly,
/// which is exactly what `straf3-headless` does.
///
/// The [`TriggerSet`] is *forwarded*, not interpreted. `step_in_place` has
/// always returned it and this wrapper used to drop it on the floor, which is
/// why a run's checkpoint crossings could not be read out of the shipped
/// binary at all. Accumulating it is [`crate::game::Game`]'s job and
/// deliberately not the simulation's: `step.rs` records that a checkpoint table
/// in `SimState` would change every digest ever taken to carry data the physics
/// never reads. Returning it here keeps the bookkeeping above the seam without
/// moving a single byte of what the checksum folds.
pub fn advance_one<W>(
    state: &mut SimState,
    cmd: &UserCmd,
    world: &W,
    profile: &PhysicsProfile,
) -> TriggerSet
where
    W: World + ?Sized,
{
    straf3_sim::step_in_place(state, cmd, world, profile)
}

#[cfg(test)]
mod tests {
    use super::*;

    const TICK: u16 = 8;
    const CAP: u32 = FixedStep::DEFAULT_MAX_TICKS_PER_FRAME;

    #[test]
    fn a_frame_shorter_than_a_tick_runs_nothing_and_carries_everything() {
        let plan = plan_ticks(5, 0, TICK, CAP);
        assert_eq!(plan.ticks, 0);
        assert_eq!(plan.remainder_ms, 5);
        assert_eq!(plan.dropped_ms, 0);
    }

    #[test]
    fn the_carry_is_what_makes_a_60hz_frame_rate_average_out() {
        let mut step = FixedStep::default();
        // 16, 17, 16, 17 … is what a 60 Hz display actually delivers in whole
        // milliseconds. Two ticks then three, alternating, averaging 2.08.
        let counts: Vec<u32> = [16, 17, 16, 17, 16, 17]
            .iter()
            .map(|d| step.advance(*d))
            .collect();
        assert_eq!(counts.iter().sum::<u32>(), 12);
        assert_eq!(99 - 12 * 8, step.carried_ms() as i32);
    }

    #[test]
    fn tick_count_tracks_elapsed_time_exactly_over_a_long_session() {
        // Deliberately awful pacing: nothing here divides into 8.
        let deltas = [1u64, 7, 3, 33, 5, 0, 11, 2, 25, 9];
        let mut step = FixedStep::default();
        let mut ticks = 0u64;
        let mut wall = 0u64;
        for round in 0..1_000 {
            let delta = deltas[round % deltas.len()];
            wall += delta;
            ticks += u64::from(step.advance(delta));
        }
        // The invariant: simulated time plus what is still carried is exactly
        // the wall time that went in. No drift, at any length of run.
        assert_eq!(ticks * u64::from(TICK) + u64::from(step.carried_ms()), wall);
        assert_eq!(step.dropped_total_ms(), 0);
    }

    #[test]
    fn a_zero_length_frame_is_not_an_error() {
        let mut step = FixedStep::default();
        assert_eq!(step.advance(0), 0);
        assert_eq!(step.carried_ms(), 0);
        // …and it does not disturb the carry.
        step.advance(5);
        assert_eq!(step.advance(0), 0);
        assert_eq!(step.carried_ms(), 5);
    }

    #[test]
    fn a_backgrounded_tab_catches_up_rather_than_being_dropped() {
        // 200 ms is 25 ticks, well inside the cap: this must catch up, not
        // discard. Dropping here would be the simulation quietly running slow.
        let mut step = FixedStep::default();
        assert_eq!(step.advance(200), 25);
        assert_eq!(step.carried_ms(), 0);
        assert_eq!(step.dropped_total_ms(), 0);
    }

    #[test]
    fn an_absurd_frame_is_capped_and_says_so() {
        let mut step = FixedStep::default();
        // Ten minutes with the lid shut. 75 000 ticks wanted, 250 allowed.
        assert_eq!(step.advance(600_000), CAP);
        assert_eq!(step.dropped_total_ms(), 600_000 - u64::from(CAP) * 8);
        // The sub-tick remainder still survives the cap.
        assert_eq!(step.carried_ms(), 0);
    }

    #[test]
    fn the_cap_keeps_the_sub_tick_remainder() {
        let plan = plan_ticks(1_003, 0, TICK, 10);
        assert_eq!(plan.ticks, 10);
        assert_eq!(plan.remainder_ms, 3);
        assert_eq!(plan.dropped_ms, 1_000 - 80);
    }

    #[test]
    fn alpha_walks_from_zero_to_just_under_one() {
        let mut step = FixedStep::default();
        assert!((step.alpha() - 0.0).abs() < 1e-6);
        step.advance(4);
        assert!((step.alpha() - 0.5).abs() < 1e-6);
        step.advance(3);
        assert!((step.alpha() - 0.875).abs() < 1e-6);
        // Spending the tick resets it — alpha is never >= 1.
        step.advance(1);
        assert!((step.alpha() - 0.0).abs() < 1e-6);
    }

    #[test]
    fn a_different_rate_is_a_different_cadence_and_nothing_else() {
        let mut step = FixedStep::new(TickRate::HZ_76);
        assert_eq!(step.tick_ms(), 13);
        assert_eq!(step.advance(26), 2);
        assert_eq!(step.advance(12), 0);
        assert_eq!(step.advance(1), 1);
    }

    #[test]
    fn the_simulation_never_sees_wall_time_directly() {
        // The proof that pacing is decoupled: two wildly different frame
        // schedules delivering the same total wall time must produce the same
        // number of ticks, hence the same simulation.
        let smooth: Vec<u64> = std::iter::repeat_n(8, 125).collect();
        let awful = {
            let mut v = vec![0u64; 120];
            v.push(1_000); // one frame that took the whole second
            v
        };

        let mut a = FixedStep::default();
        let ticks_a: u32 = smooth.iter().map(|d| a.advance(*d)).sum();
        let mut b = FixedStep::default();
        let ticks_b: u32 = awful.iter().map(|d| b.advance(*d)).sum();

        assert_eq!(ticks_a, 125);
        assert_eq!(ticks_a, ticks_b);
    }
}
