//! Racing a saved run: re-simulating it, placing it, and saying how far ahead
//! or behind the live player is.
//!
//! # The ghost is a re-simulation. It is not a position track.
//!
//! A `.s3d` file stores **commands**. Everything in this module that looks
//! like a position was produced here, by running those commands through
//! `straf3-sim` — the same `step_in_place` the live player is running through,
//! called by `straf3_replay::Recording::replay`. There is no second physics
//! implementation and no interpolated recording of where somebody used to be.
//! If the map is recompiled, the recording is refused (contract item C6)
//! rather than replayed against geometry it never touched.
//!
//! # Why the whole run is re-simulated once, at load
//!
//! The alternative is stepping the ghost forward every frame in lockstep with
//! the live player. It produces exactly the same states — the simulation is
//! deterministic, which is the entire premise — and it costs the frame loop
//! work it does not need to do. Re-simulating a minute-long run is 7 500 steps
//! and lands well under a millisecond, once, at load. What that buys is the
//! thing lockstep cannot give: the *whole* path is known, which is what the
//! live split is computed against (see [`Ghost::split_ms`]).
//!
//! # The ghost's clock is its own
//!
//! Samples are indexed by *run-elapsed* milliseconds, not by tick and not by
//! simulation time. The player and the ghost each start their own clock when
//! they cross the start line, so a player who loitered at spawn for a minute
//! still leaves the line level with the ghost — which is what a race against a
//! personal best means.

use straf3_replay::{Mismatch, Recording, WorldId};
use straf3_sim::num::{Scalar, Vec3, s};
use straf3_sim::{PhysicsProfile, RunState, World};

/// One instant of the recorded run, as this process re-simulated it.
///
/// No rendering type appears here on purpose: the ghost is a simulation
/// question, and this module has to be usable — and testable — in a build with
/// `--no-default-features`, where there is no renderer at all.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Sample {
    /// Milliseconds since the recorded run crossed its own start line.
    pub elapsed_ms: u32,
    /// Where the recorded player was.
    pub origin: Vec3,
    /// Which way they were looking, in degrees.
    pub yaw: Scalar,
    /// Whether they were crouched — the ghost is drawn at the hull it
    /// collided with, so this changes its height.
    pub crouched: bool,
}

/// A saved run, re-simulated and ready to race.
#[derive(Debug, Clone)]
pub struct Ghost {
    /// The path, from the start line to the finish, one entry per command.
    ///
    /// Non-empty by construction: [`Ghost::from_recording`] refuses a
    /// recording that never started a run.
    track: Vec<Sample>,
    /// What the recorded run came to, in milliseconds.
    run_time_ms: u32,
    /// Where [`Ghost::split_ms`] last matched the player onto the track.
    ///
    /// Carried between frames so the search is a short scan around where the
    /// player was rather than over the whole run — and so a course that
    /// crosses itself matches the lap you are on rather than the one you are
    /// about to be on.
    cursor: usize,
}

/// Why a recording could not be raced.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum GhostError {
    /// The recording is not bound to this world or these physics — the map was
    /// recompiled, or the profile changed. Contract item C6: this is detected,
    /// never silently replayed.
    Mismatch(Mismatch),
    /// The recording is of a run that never crossed the start line, so there
    /// is nothing to race against.
    NeverStarted,
    /// The recording crossed the start but never the finish.
    NeverFinished,
}

impl core::fmt::Display for GhostError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            Self::Mismatch(m) => write!(f, "{m}"),
            Self::NeverStarted => write!(f, "the recording never crossed the start line"),
            Self::NeverFinished => write!(f, "the recording never crossed the finish line"),
        }
    }
}

impl std::error::Error for GhostError {}

/// How far back and forward of the last match [`Ghost::split_ms`] looks, in
/// milliseconds of the ghost's run.
///
/// Backwards is short and forwards is long because the two failures are not
/// symmetric: a small backward window keeps a course that doubles back on
/// itself from matching the earlier pass (which would report a wildly negative
/// split and look like a bug), while the forward window has to be long enough
/// to still find the player when they are seconds ahead of the ghost.
const SPLIT_WINDOW_BACK_MS: u32 = 1_500;
const SPLIT_WINDOW_FORWARD_MS: u32 = 15_000;

impl Ghost {
    /// Re-simulate `recording` and keep what it produced.
    ///
    /// `world` and `world_id` must be the world this session is played in;
    /// they are what the recording is checked against before anything is
    /// simulated.
    ///
    /// # Errors
    ///
    /// [`GhostError::Mismatch`] when the recording was not made on this
    /// geometry or under these physics. [`GhostError::NeverStarted`] or
    /// [`GhostError::NeverFinished`] when the run is not a complete time —
    /// there is no such thing as racing half a ghost.
    pub fn from_recording(
        recording: &Recording,
        world: &impl World,
        world_id: &WorldId,
        profile: &PhysicsProfile,
    ) -> Result<Self, GhostError> {
        let mut track: Vec<Sample> = Vec::with_capacity(recording.command_count());
        let mut finish: Option<Sample> = None;

        recording
            .replay(world, world_id, profile, |_, state| {
                // Only the run itself. Whatever the recorded player did before
                // crossing the start line is not part of the time and must not
                // be part of the race.
                let elapsed_ms = match state.run {
                    RunState::NotStarted => return,
                    RunState::Running { started_at_ms } => state.time_ms - started_at_ms,
                    RunState::Finished {
                        started_at_ms,
                        finished_at_ms,
                    } => {
                        // The first `Finished` state is the command that
                        // crossed the line, and it is the only place the
                        // *finishing position* exists. Keeping it is not a
                        // refinement: see the note below the loop.
                        if finish.is_none() {
                            finish = Some(Sample {
                                elapsed_ms: finished_at_ms - started_at_ms,
                                origin: state.player.origin,
                                yaw: state.player.view.yaw_degrees(),
                                crouched: state.player.crouched,
                            });
                        }
                        // Commands after the finish line still exist in the
                        // file — the recorded player kept moving until they
                        // quit — but the run is over and the ghost stops.
                        return;
                    }
                };
                track.push(Sample {
                    elapsed_ms,
                    origin: state.player.origin,
                    yaw: state.player.view.yaw_degrees(),
                    crouched: state.player.crouched,
                });
            })
            .map_err(GhostError::Mismatch)?;

        if track.is_empty() {
            return Err(GhostError::NeverStarted);
        }
        let Some(finish) = finish else {
            return Err(GhostError::NeverFinished);
        };
        let run_time_ms = finish.elapsed_ms;

        // The finish itself. Without it the ghost's path stops one command
        // short of the line, and [`Ghost::split_ms`] — which matches the player
        // to the nearest *point* on that path — then has nowhere to match a
        // player standing on the line except the command before it. A run that
        // ties the personal best exactly therefore reported a whole tick of
        // split (measured: +8 ms at 125 Hz, racing a recording against itself).
        //
        // This carried a copy of the previous sample's *position* before, which
        // fixed the ghost's disappearance but not the bias: two samples at one
        // place is a zero-length segment the nearest-point search cannot
        // discriminate, so it kept picking the earlier of the two. The state
        // that actually crossed the line is the honest point, and it is the one
        // the player's own finishing state is compared against.
        if track.last().is_none_or(|last| last.elapsed_ms < run_time_ms) {
            track.push(finish);
        }

        Ok(Self {
            track,
            run_time_ms,
            cursor: 0,
        })
    }

    /// The time this ghost set, in milliseconds. The number to beat.
    #[must_use]
    pub const fn run_time_ms(&self) -> u32 {
        self.run_time_ms
    }

    /// How many re-simulated states the ghost's path is made of.
    #[must_use]
    pub fn sample_count(&self) -> usize {
        self.track.len()
    }

    /// Where the ghost is `elapsed_ms` into its run, interpolated between the
    /// two re-simulated states either side of that instant.
    ///
    /// Clamped at both ends: before the run it stands on the start line, and
    /// after it has finished it stays on the finish line rather than vanishing
    /// — a ghost that disappears the instant it beats you tells you nothing
    /// about by how much.
    #[must_use]
    pub fn sample_at(&self, elapsed_ms: u32) -> Sample {
        let next = self.track.partition_point(|s| s.elapsed_ms <= elapsed_ms);
        if next == 0 {
            return self.track[0];
        }
        let a = self.track[next - 1];
        let Some(&b) = self.track.get(next) else {
            return a;
        };

        let span = b.elapsed_ms.saturating_sub(a.elapsed_ms);
        if span == 0 {
            return a;
        }
        // Integer numerator over integer denominator, converted once: whole
        // milliseconds in, a rendering fraction out. This is a picture, not a
        // duration — nothing downstream of it is ever added to a time.
        let t = s((elapsed_ms - a.elapsed_ms) as f32) / s(span as f32);
        Sample {
            elapsed_ms,
            origin: a.origin + (b.origin - a.origin) * t,
            yaw: a.yaw + shortest_turn(a.yaw, b.yaw) * t,
            // Crouch is a state, not a slider: the hull changes on a command
            // boundary, so blending it would draw a box nobody collided with.
            crouched: a.crouched,
        }
    }

    /// How far ahead of, or behind, the ghost the player is — in milliseconds,
    /// negative when the player is winning.
    ///
    /// # What "at the same point of the run" means here, and what it does not
    ///
    /// A true split needs the two runs compared where they are at the same
    /// *place*, and the simulation does not record checkpoint times:
    /// `straf3_sim::TriggerSet` can express checkpoints, but `RunState` carries
    /// only the start and the finish, so there are no per-checkpoint times to
    /// difference above the seam. What this does instead is match the player
    /// to the nearest point on the ghost's re-simulated path — within a window
    /// around the last match, so the match walks the route forward with the
    /// player rather than teleporting to the far side of a course that crosses
    /// itself — and difference the two clocks there.
    ///
    /// That is an approximation, and it is stated as one. It is exact where it
    /// matters most (the finish line, and anywhere the two runs take the same
    /// line) and it degrades where the player leaves the ghost's route
    /// entirely, which is a situation in which "how far behind am I" has no
    /// well-defined answer anyway.
    pub fn split_ms(&mut self, player_origin: Vec3, player_elapsed_ms: u32) -> i32 {
        let anchor = self.track[self.cursor].elapsed_ms;
        let lo = self
            .track
            .partition_point(|s| s.elapsed_ms < anchor.saturating_sub(SPLIT_WINDOW_BACK_MS));
        let hi = self
            .track
            .partition_point(|s| s.elapsed_ms <= anchor.saturating_add(SPLIT_WINDOW_FORWARD_MS));

        let mut best = self.cursor;
        let mut best_distance = Scalar::INFINITY;
        for (index, sample) in self.track[lo..hi].iter().enumerate() {
            let d = (sample.origin - player_origin).length_squared();
            if d < best_distance {
                best_distance = d;
                best = lo + index;
            }
        }
        self.cursor = best;

        // Signed like a motorsport split: negative is the player ahead.
        i64::from(player_elapsed_ms) as i32 - self.track[best].elapsed_ms as i32
    }

    /// Put the match back at the start line.
    ///
    /// Called when the player respawns: their next attempt begins at the
    /// beginning, and a cursor left three quarters of the way round the course
    /// would report a split against the wrong part of it.
    pub fn rewind(&mut self) {
        self.cursor = 0;
    }
}

/// The signed turn from `from` to `to`, in degrees, never the long way round.
///
/// Interpolating 359° towards 1° through 180 spins the ghost a full turn in one
/// frame, which is the single most visible artefact this whole file could have.
fn shortest_turn(from: Scalar, to: Scalar) -> Scalar {
    let mut delta = (to - from) % s(360.0);
    if delta > s(180.0) {
        delta -= s(360.0);
    } else if delta < s(-180.0) {
        delta += s(360.0);
    }
    delta
}

#[cfg(test)]
mod tests {
    use super::*;
    use straf3_replay::RunStart;
    use straf3_sim::num::vec3;
    use straf3_sim::world::FlatGround;
    use straf3_sim::{Buttons, TickRate, UserCmd};

    /// A world with a start line at the origin and a finish 512 units along
    /// +Y, so a recording made in it is a real timed run rather than a
    /// contrivance around one.
    ///
    /// `FlatGround` reports no triggers, so the timing comes from this wrapper
    /// — the simulation is untouched, which is the point: it reads
    /// `Trace::triggers` and does not care who filled them in.
    struct TimedFlat {
        ground: FlatGround,
        start_y: Scalar,
        finish_y: Scalar,
    }

    impl straf3_sim::World for TimedFlat {
        fn trace(&self, sweep: &straf3_sim::world::Sweep) -> straf3_sim::Trace {
            let mut trace = self.ground.trace(sweep);
            let y = sweep.end.y;
            if y >= self.start_y && y < self.start_y + s(64.0) {
                trace.triggers = trace.triggers.with(straf3_sim::TriggerSet::START);
            }
            if y >= self.finish_y {
                trace.triggers = trace.triggers.with(straf3_sim::TriggerSet::FINISH);
            }
            trace
        }
    }

    fn world() -> TimedFlat {
        TimedFlat {
            ground: FlatGround::at(s(0.0)),
            start_y: s(0.0),
            finish_y: s(512.0),
        }
    }

    /// Run forward for `count` commands, looking along +Y.
    fn forward(count: usize) -> Vec<UserCmd> {
        let mut cmd = UserCmd::still_at(TickRate::HZ_125);
        cmd.view = straf3_sim::ViewAngles::looking_along(s(90.0));
        cmd.forward_move = 127;
        cmd.buttons = Buttons::NONE;
        vec![cmd; count]
    }

    fn recording() -> Recording {
        Recording::record(
            RunStart {
                rate: TickRate::HZ_125,
                spawn: vec3(s(0.0), s(0.0), s(24.0)),
                yaw: s(90.0),
            },
            forward(400),
            &world(),
            WorldId::flat(s(0.0)),
            &PhysicsProfile::cpm(),
            "cpm",
        )
    }

    fn ghost() -> Ghost {
        Ghost::from_recording(
            &recording(),
            &world(),
            &WorldId::flat(s(0.0)),
            &PhysicsProfile::cpm(),
        )
        .expect("the fixture run starts and finishes")
    }

    #[test]
    fn the_track_is_the_run_and_not_a_millisecond_more() {
        let ghost = ghost();
        assert!(ghost.run_time_ms() > 0);
        // First sample is the start line, last is the finish.
        assert_eq!(ghost.track[0].elapsed_ms, 0);
        assert_eq!(
            ghost.track.last().unwrap().elapsed_ms,
            ghost.run_time_ms(),
            "the ghost must reach the finish line, not stop a command short"
        );
        for pair in ghost.track.windows(2) {
            assert!(
                pair[0].elapsed_ms <= pair[1].elapsed_ms,
                "the track must be monotonic in run time or every search in \
                 this file is wrong"
            );
        }
    }

    /// The finish sample must be *where the run finished*, not a copy of the
    /// command before it.
    ///
    /// Two samples at one position is a zero-length segment, and
    /// [`Ghost::split_ms`]'s nearest-point search cannot discriminate between
    /// them — it kept the earlier, so a player standing on the line was matched
    /// one command back and a tie read as a whole tick of deficit.
    #[test]
    fn the_finish_sample_is_the_state_that_crossed_the_line() {
        let ghost = ghost();
        let track = &ghost.track;
        let finish = *track.last().unwrap();
        let before = track[track.len() - 2];
        assert_eq!(finish.elapsed_ms, ghost.run_time_ms());
        assert!(
            finish.elapsed_ms > before.elapsed_ms,
            "the finish must be its own instant"
        );
        assert_ne!(
            finish.origin, before.origin,
            "the finish sample is a duplicate of the command before it, so the \
             track has no point at the line"
        );
        // Moving along +Y, so finishing is strictly further along the course.
        assert!(finish.origin.y > before.origin.y);
    }

    /// A *live* run of the recorded commands is level with its own ghost.
    ///
    /// Not a duplicate of
    /// `racing_the_ghost_against_itself_is_level_the_whole_way_round`, which
    /// feeds the ghost points taken out of its own track: that can only detect
    /// a track that disagrees with itself, and it samples `step_by(7)`, so it
    /// never asked about the finish sample at all. This drives the player from
    /// an independent re-simulation and asks at every command, which is the
    /// arrangement a real race is.
    #[test]
    fn a_live_run_of_the_recorded_commands_is_level_with_its_own_ghost() {
        let recording = recording();
        let mut ghost = ghost();
        let mut worst = 0;
        recording
            .replay(
                &world(),
                &WorldId::flat(s(0.0)),
                &PhysicsProfile::cpm(),
                |_, state| {
                    if let Some(elapsed_ms) = state.run.elapsed_ms(state.time_ms)
                        && matches!(state.run, RunState::Running { .. })
                    {
                        let split = ghost.split_ms(state.player.origin, elapsed_ms);
                        worst = worst.max(split.abs());
                    }
                },
            )
            .unwrap();
        assert_eq!(worst, 0, "a run is not ahead of or behind itself");
    }

    #[test]
    fn a_recording_of_an_unfinished_run_is_refused_rather_than_half_raced() {
        // Ten commands is nowhere near the finish line.
        let short = Recording::record(
            RunStart {
                rate: TickRate::HZ_125,
                spawn: vec3(s(0.0), s(0.0), s(24.0)),
                yaw: s(90.0),
            },
            forward(10),
            &world(),
            WorldId::flat(s(0.0)),
            &PhysicsProfile::cpm(),
            "cpm",
        );
        let refused = Ghost::from_recording(
            &short,
            &world(),
            &WorldId::flat(s(0.0)),
            &PhysicsProfile::cpm(),
        );
        assert!(matches!(refused, Err(GhostError::NeverFinished)));
    }

    #[test]
    fn a_recording_from_different_geometry_is_refused_not_replayed() {
        // C6 restated where a player would meet it: the ghost is the thing
        // that would visibly land where a ramp used to be.
        let err = Ghost::from_recording(
            &recording(),
            &world(),
            &WorldId::map("coil", 0x1234_5678_9abc_def0),
            &PhysicsProfile::cpm(),
        )
        .unwrap_err();
        assert!(matches!(err, GhostError::Mismatch(_)));
    }

    #[test]
    fn a_recording_from_different_physics_is_refused_too() {
        let err = Ghost::from_recording(
            &recording(),
            &world(),
            &WorldId::flat(s(0.0)),
            &PhysicsProfile::vq3(),
        )
        .unwrap_err();
        assert!(matches!(err, GhostError::Mismatch(_)));
    }

    #[test]
    fn the_sample_is_clamped_at_both_ends_rather_than_wrapping_or_vanishing() {
        let ghost = ghost();
        assert_eq!(ghost.sample_at(0).origin, ghost.track[0].origin);

        let finish = ghost.sample_at(ghost.run_time_ms());
        let after = ghost.sample_at(ghost.run_time_ms() + 60_000);
        assert_eq!(finish.origin, after.origin);
    }

    #[test]
    fn the_ghost_moves_forward_through_the_run() {
        let ghost = ghost();
        let a = ghost.sample_at(0);
        let b = ghost.sample_at(ghost.run_time_ms() / 2);
        let c = ghost.sample_at(ghost.run_time_ms());
        assert!(b.origin.y > a.origin.y);
        assert!(c.origin.y > b.origin.y);
    }

    #[test]
    fn a_sample_between_two_commands_lands_between_them() {
        // Interpolation is what stops the ghost stepping at the command rate
        // while the live player moves smoothly beside it.
        let ghost = ghost();
        let a = ghost.track[10];
        let b = ghost.track[11];
        let between = ghost.sample_at((a.elapsed_ms + b.elapsed_ms) / 2);
        assert!(between.origin.y > a.origin.y && between.origin.y < b.origin.y);
    }

    #[test]
    fn racing_the_ghost_against_itself_is_level_the_whole_way_round() {
        // The property that makes the split trustworthy: re-simulating the
        // recording produces the run it recorded, so a player standing exactly
        // where the ghost was, at exactly its elapsed time, is neither ahead
        // nor behind.
        //
        // # What this covers, and what it used to miss
        //
        // It feeds the ghost points taken out of its own track, so it can only
        // detect a track that disagrees with *itself*. `step_by(7)` then walks
        // over the finish sample — and the finish sample is exactly where a real
        // bug lived: it carried a copy of the previous command's position, so a
        // run that tied its own personal best was reported 8 ms behind itself.
        // This test passed throughout.
        //
        // **Do not tidy the stride away to `step_by(1)` and call that the fix.**
        // The stride is not the problem; feeding the ghost its own data is. The
        // two tests that actually cover this are
        // `the_finish_sample_is_the_state_that_crossed_the_line` and
        // `a_live_run_of_the_recorded_commands_is_level_with_its_own_ghost`,
        // which drives the player from an independent re-simulation.
        let mut ghost = ghost();
        let track = ghost.track.clone();
        for sample in track.iter().step_by(7) {
            let split = ghost.split_ms(sample.origin, sample.elapsed_ms);
            assert_eq!(
                split, 0,
                "racing itself should be dead level at {} ms",
                sample.elapsed_ms
            );
        }
    }

    #[test]
    fn a_player_at_the_same_place_but_later_is_reported_behind() {
        let mut ghost = ghost();
        let midpoint = ghost.track[ghost.track.len() / 2];
        let split = ghost.split_ms(midpoint.origin, midpoint.elapsed_ms + 250);
        assert_eq!(split, 250, "positive means behind");

        ghost.rewind();
        let earlier = ghost.split_ms(midpoint.origin, midpoint.elapsed_ms.saturating_sub(250));
        assert_eq!(earlier, -250, "negative means ahead");
    }

    #[test]
    fn the_match_does_not_run_away_down_the_course_ahead_of_the_player() {
        // The forward window is bounded, so a player who stops dead does not
        // get matched to wherever the ghost happens to be much later.
        let mut ghost = ghost();
        let first = ghost.track[0];
        for _ in 0..20 {
            let split = ghost.split_ms(first.origin, 0);
            assert_eq!(split, 0);
        }
        assert_eq!(ghost.cursor, 0);
    }

    #[test]
    fn a_turn_is_interpolated_the_short_way_round() {
        assert_eq!(shortest_turn(s(359.0), s(1.0)), s(2.0));
        assert_eq!(shortest_turn(s(1.0), s(359.0)), s(-2.0));
        assert_eq!(shortest_turn(s(0.0), s(90.0)), s(90.0));
        assert_eq!(shortest_turn(s(90.0), s(0.0)), s(-90.0));
    }
}
