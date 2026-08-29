//! The whole game loop, with no window and no GPU in it.
//!
//! # Why this type exists separately from the event loop
//!
//! [`Game`] is everything that happens between "the wall clock moved" and
//! "the simulation is here now": capture input, split elapsed time into fixed
//! ticks, build one [`UserCmd`] per tick, step. It holds no winit type and no
//! wgpu type, so `harness` can run a whole session through it with synthetic
//! frame deltas and synthetic input and check the result against
//! `straf3_sim::step_in_place` called directly (criterion 4).
//!
//! `app` is then a genuinely thin shell: it turns winit events into
//! [`InputState`] changes, calls [`Game::advance`] once per frame, and hands
//! [`Game::previous`] / [`Game::state`] / [`Game::alpha`] to the renderer.
//!
//! # Playback, and why it is *inside* [`Game::advance`]
//!
//! [`Game::play`] swaps out one thing: where a tick's command comes from — the
//! recorded stream instead of [`InputState`]. Everything else about the frame
//! is the identical code, running in the identical order: the accumulator, the
//! recorder, the step function, the two-state interpolation.
//!
//! **This is the whole design, and it is worth defending against tidying.** The
//! obvious alternative is to drive playback from the event loop: pull `n`
//! commands per frame and push them through [`Game::apply`]. It looks cleaner,
//! it keeps `advance` simpler, and it is wrong — it creates a *second stepping
//! path*. "The windowed build plays a recording back exactly as the headless
//! replay does" would then be a coincidence maintained by two pieces of code
//! agreeing, rather than a fact about one piece of code. Every future change to
//! the frame loop would have to be made twice, correctly, by someone who
//! remembered there were two.
//!
//! The version of that bug this design already avoided: [`Game::apply`] does
//! not feed the recorder, and a personal best is built out of the recorder. An
//! event-loop playback would therefore have crossed the finish line and saved
//! **nothing**, and the failure would have looked like a bug in the personal-best
//! path rather than in the playback. Because playback runs through `advance`,
//! the recorder sees it for free. Nothing had to remember to make that work.
//!
//! The frame rate decides *when* commands are consumed. It never decides which
//! ones, or how many per unit of simulated time, because it never did: the tick
//! count still comes from [`crate::FixedStep`].
//!
//! # What this bought, measured
//!
//! One recorded run of `coil` (864 commands, 125 Hz, cpm) produced a single
//! identical [`SimState::checksum`] down **five** paths: the windowed client on
//! an RTX 3060 Ti twice, the Windows headless `--replay`, the same replay under
//! a deliberately hostile frame schedule (`--frame-ms 1,97,3,250,8`), and the
//! Linux headless `--replay`. The windowed sessions drew at ~165 fps against
//! 125 Hz commands, so the frame rate and the tick rate genuinely disagreed,
//! across two operating systems and a real GPU, and the answer did not move.
//!
//! That is the strongest determinism evidence this project has: `cargo xtask
//! determinism` compares four *targets* running the same headless code, which
//! is a compiler-and-architecture check. This additionally crosses the seam —
//! the same simulation with a window, a swapchain and an overlay in front of
//! it — which is the part a player would actually be affected by.
//!
//! No checksum literal is written down anywhere. The tests assert the
//! *equality* of two paths in one build, so they keep meaning this when
//! `SimState`'s encoding changes.
//!
//! # Interpolation, and why two states are kept
//!
//! The renderer draws between the last two simulation states. Keeping the
//! previous state here rather than in the renderer means the renderer cannot
//! accidentally become the thing that decides when the simulation advances —
//! it is handed two finished states and a number between them, and there is
//! no API through which it could ask for a third.

use straf3_platform::InputState;
use straf3_sim::num::{Scalar, Vec3};
use straf3_sim::world::Sweep;
use straf3_sim::{PhysicsProfile, SimState, TickRate, TriggerSet, UserCmd, World};

use crate::input_map::command_from_input;
use crate::record::Recorder;
use crate::tick::{FixedStep, advance_one};

/// A running session: a world, a profile, the player's input, and the
/// simulation state that follows from them.
#[derive(Debug, Clone)]
pub struct Game<W> {
    world: W,
    profile: PhysicsProfile,
    step: FixedStep,
    /// What the player is holding and where they are looking, right now.
    ///
    /// Public because the event loop's entire job is to keep it current; every
    /// tick reads it as it stands at that moment, exactly as a Q3 client
    /// sampled its input state when it built a command.
    pub input: InputState,
    state: SimState,
    previous: SimState,
    spawn: Vec3,
    spawn_yaw: Scalar,
    recorder: Option<Recorder>,
    /// The recorded stream driving this session, if [`Game::play`] was called.
    playback: Option<Playback>,
    /// Every timing volume this attempt has passed through, and the tick it
    /// first touched each one.
    ///
    /// Kept here rather than in `SimState` on `straf3-sim`'s explicit
    /// instruction: `step.rs` returns the per-command [`TriggerSet`] instead of
    /// storing it because `SimState` is what a recording's digest folds, and a
    /// checkpoint table in there would move every checksum ever taken to carry
    /// data the physics never reads. This field is above the seam, so it costs
    /// no digest.
    crossings: Vec<TriggerCrossing>,
    /// The union of everything in `crossings`, kept alongside it so that
    /// deciding whether a volume is newly touched is not a linear scan per
    /// command.
    crossed: TriggerSet,
}

/// The first moment a run touched one or more timing volumes.
///
/// One entry per *command that touched something new*, so the list is in
/// crossing order and a volume appears exactly once however long the player
/// stands in it. `triggers` can hold more than one bit: a command is up to a
/// whole tick of movement and can cross two adjacent volumes within it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TriggerCrossing {
    /// The simulation tick at which the crossing was observed — the tick the
    /// command produced, so it lines up with a trace line and with
    /// `SimState::tick`.
    pub tick: u32,
    /// Simulation time at that tick, in whole milliseconds.
    pub time_ms: u32,
    /// The volumes newly touched, and only the newly touched ones.
    pub triggers: TriggerSet,
}

/// A recorded command stream, and how far through it the session is.
#[derive(Debug, Clone)]
struct Playback {
    cmds: Vec<UserCmd>,
    next: usize,
}

impl Playback {
    /// The next command, or `None` once the stream has run out.
    fn take(&mut self) -> Option<UserCmd> {
        let cmd = self.cmds.get(self.next).copied();
        if cmd.is_some() {
            self.next += 1;
        }
        cmd
    }

    /// How many commands have not been applied yet.
    const fn remaining(&self) -> usize {
        self.cmds.len() - self.next
    }
}

impl<W: World> Game<W> {
    /// Start a session with the player stood at `spawn`, looking along
    /// `spawn_yaw`.
    pub fn new(
        world: W,
        profile: PhysicsProfile,
        rate: TickRate,
        spawn: Vec3,
        spawn_yaw: Scalar,
    ) -> Self {
        let state = SimState::spawned_at(spawn, spawn_yaw);
        let mut input = InputState::looking_along(spawn_yaw);
        input.look.set(straf3_sim::num::s(0.0), spawn_yaw);
        Self {
            world,
            profile,
            step: FixedStep::new(rate),
            input,
            state,
            previous: state,
            spawn,
            spawn_yaw,
            recorder: None,
            playback: None,
            crossings: Vec::new(),
            crossed: TriggerSet::NONE,
        }
    }

    /// Fold one command's worth of touched volumes into the crossing list.
    ///
    /// Only the bits not seen before this command are recorded, so standing in
    /// a volume for twenty ticks produces one entry rather than twenty, and the
    /// list reads as the order the run met them.
    fn note_crossings(&mut self, touched: TriggerSet) {
        let fresh = TriggerSet(touched.0 & !self.crossed.0);
        if fresh.is_empty() {
            return;
        }
        self.crossed = TriggerSet(self.crossed.0 | fresh.0);
        self.crossings.push(TriggerCrossing {
            tick: self.state.tick,
            time_ms: self.state.time_ms,
            triggers: fresh,
        });
    }

    /// Every timing volume this attempt has touched, in the order it met them.
    ///
    /// The point of surfacing this is that a run's route can be checked by
    /// somebody who did not produce the run: "it finished" is what the clock
    /// says, and a clock cannot tell a lap of the course from a shortcut that
    /// crossed the start and finish volumes and nothing between them.
    #[must_use]
    pub fn crossings(&self) -> &[TriggerCrossing] {
        &self.crossings
    }

    /// The union of every volume touched so far.
    #[must_use]
    pub const fn crossed(&self) -> TriggerSet {
        self.crossed
    }

    /// Drive this session from `cmds` instead of from [`Game::input`].
    ///
    /// Every tick that comes due takes the next command from the stream rather
    /// than building one from what the player is holding. When the stream runs
    /// out the session holds its final state: no further tick runs, and
    /// [`Game::previous`] is collapsed onto [`Game::state`] so the render
    /// interpolation stops swinging between the last two states forever.
    ///
    /// The caller is responsible for having spawned this session where the
    /// recording began — a stream applied from a different origin re-simulates
    /// to a different run, which is the one way this can silently lie.
    pub fn play(&mut self, cmds: Vec<UserCmd>) {
        self.playback = Some(Playback { cmds, next: 0 });
    }

    /// Whether this session is being driven by a recording.
    #[must_use]
    pub const fn is_playing(&self) -> bool {
        self.playback.is_some()
    }

    /// How many recorded commands are still to be applied, or `None` when this
    /// session is not playing one back.
    #[must_use]
    pub fn playback_remaining(&self) -> Option<usize> {
        self.playback.as_ref().map(Playback::remaining)
    }

    /// Record every command this session produces, for later replay.
    ///
    /// Recording is off by default: it is the input sequence criterion 4
    /// replays, not something an ordinary session needs.
    pub fn record(&mut self) {
        self.recorder = Some(Recorder::new(self.step.rate(), self.spawn, self.spawn_yaw));
    }

    /// The recorded commands so far, if recording was turned on.
    #[must_use]
    pub fn recorder(&self) -> Option<&Recorder> {
        self.recorder.as_ref()
    }

    /// Put the player back at the spawn, standing still.
    ///
    /// The view is left where the player is looking — respawning should not
    /// also spin the camera.
    ///
    /// # The recording starts again with it
    ///
    /// A respawn is **not** a command: it moves the player without anything in
    /// the command stream saying so. A recording that spanned one would
    /// therefore not re-simulate — replaying it would run the pre-respawn
    /// commands from the *new* state and land somewhere nobody went. So the
    /// recorder is reset here, and every attempt is its own recording, which is
    /// also exactly what a personal best wants to be.
    ///
    /// # A played session refuses to respawn
    ///
    /// Under [`Game::play`] this does nothing. The remaining commands are
    /// anchored to the state the recording began from, so restarting mid-stream
    /// would run them from somewhere nobody recorded them from — and the
    /// recorder, which is what the saved personal best is built out of, would
    /// then hold half a run while claiming a whole one. The caller says so out
    /// loud (see `app`); refusing here is what makes it impossible rather than
    /// merely discouraged.
    pub fn respawn(&mut self) {
        if self.playback.is_some() {
            return;
        }
        self.state = SimState::spawned_at(self.spawn, self.input.look.yaw());
        self.state.player.view = self.input.look.angles();
        self.previous = self.state;
        // R starts a new attempt, and the crossing list describes an attempt.
        // Carrying the old one over would report a run as having passed a
        // checkpoint it only reached before the respawn that discarded it —
        // the same reasoning that restarts the recorder below.
        self.crossings.clear();
        self.crossed = TriggerSet::NONE;
        if self.recorder.is_some() {
            self.recorder = Some(Recorder::new(
                self.step.rate(),
                self.spawn,
                self.input.look.yaw(),
            ));
        }
    }

    /// Advance the session by `elapsed_ms` of wall time.
    ///
    /// Returns how many simulation ticks ran, which may be zero (a frame
    /// shorter than a tick) or many (a frame longer than one). **This is the
    /// decoupling** — the caller reports real time and gets told what happened
    /// to the simulation, never the other way round.
    ///
    /// Under [`Game::play`] the returned count is how many *recorded* commands
    /// were applied, which is fewer than the ticks the clock bought once the
    /// stream has run out. The accumulator is still asked first, so a played
    /// session consumes its file on exactly the cadence a live one would
    /// produce it.
    pub fn advance(&mut self, elapsed_ms: u64) -> u32 {
        let ticks = self.step.advance(elapsed_ms);
        let duration_ms = self.step.tick_ms();
        let mut ran = 0;
        for _ in 0..ticks {
            let cmd = match &mut self.playback {
                // A stream that has run out ends the frame rather than
                // falling back to live input: half a played run followed by
                // half a keyboard run is not a replay of anything.
                Some(playback) => match playback.take() {
                    Some(cmd) => cmd,
                    None => break,
                },
                None => command_from_input(&self.input, duration_ms),
            };
            self.previous = self.state;
            if let Some(recorder) = &mut self.recorder {
                recorder.push(cmd);
            }
            let touched = advance_one(&mut self.state, &cmd, &self.world, &self.profile);
            self.note_crossings(touched);
            ran += 1;
        }
        // A finished stream holds still. Without this the renderer would keep
        // interpolating between the last two states as alpha cycled, and the
        // held player would visibly vibrate on the spot for as long as the
        // window stayed open.
        if self.playback.as_ref().is_some_and(|p| p.remaining() == 0) {
            self.previous = self.state;
        }
        ran
    }

    /// Apply one command directly, bypassing the wall clock entirely.
    ///
    /// This is how a recorded sequence is replayed through the *windowed*
    /// build's own code path, and how a test drives the session with no clock
    /// at all.
    pub fn apply(&mut self, cmd: &UserCmd) {
        self.previous = self.state;
        let touched = advance_one(&mut self.state, cmd, &self.world, &self.profile);
        self.note_crossings(touched);
    }

    /// The current simulation state.
    #[must_use]
    pub const fn state(&self) -> &SimState {
        &self.state
    }

    /// The state one tick ago — the other end of the render interpolation.
    #[must_use]
    pub const fn previous(&self) -> &SimState {
        &self.previous
    }

    /// How far past [`Game::previous`] towards [`Game::state`] the next frame
    /// sits, in `0.0..1.0`.
    #[must_use]
    pub fn alpha(&self) -> f32 {
        self.step.alpha()
    }

    /// The fixed-step accumulator, for anyone who wants to inspect the pacing.
    #[must_use]
    pub const fn step(&self) -> &FixedStep {
        &self.step
    }

    /// The physics constants this session runs under.
    #[must_use]
    pub const fn profile(&self) -> &PhysicsProfile {
        &self.profile
    }

    /// The world this session collides against.
    #[must_use]
    pub const fn world(&self) -> &W {
        &self.world
    }

    /// Horizontal speed in units per second — the number a strafe-jumper is
    /// actually watching.
    #[must_use]
    pub fn horizontal_speed(&self) -> Scalar {
        let v = self.state.player.velocity;
        (v.x * v.x + v.y * v.y).sqrt()
    }

    /// How far the player's feet are above the surface below them, in units.
    /// `None` when there is nothing underneath within [`CLEARANCE_FAR_PROBE`].
    ///
    /// # Why this exists
    ///
    /// Ramp boost and overbounce are one mechanism, and it fires on whether a
    /// command happened to end with the feet inside a
    /// [`PhysicsProfile::ground_trace_probe`]-wide band above the surface —
    /// a quarter of a unit. Nothing on screen distinguishes a drop that will do
    /// it from one half a unit taller that will not, so to a player the largest
    /// speed gain in the game reads as luck. The operator's ruling was to make
    /// it visible. This is the number that makes the band observable.
    ///
    /// # What this number cannot tell you
    ///
    /// **It does not predict whether a landing will overbounce**, and an
    /// overlay built on it must not imply that it does. What decides the
    /// outcome is where a *command boundary* happens to fall relative to the
    /// surface as the player descends — a sub-tick question about when the
    /// simulation next samples, and the signal that would answer it does not
    /// exist above the seam. Two descents through identical clearances at
    /// different velocities resolve differently.
    ///
    /// What it does do is make the *band* legible: a player can watch the
    /// number pass through a quarter of a unit and learn that the effect lives
    /// in a fraction of a unit, rather than experiencing it as randomness. That
    /// is a smaller claim than prediction and it is the true one.
    ///
    /// # Why it is measured the way the simulation measures it
    ///
    /// The same downward hull sweep `PM_GroundTrace` issues, from the same
    /// origin with the same hull — only longer, and reported rather than acted
    /// on. A readout computed a second way could disagree with the rule it
    /// claims to illustrate, which would be worse than no readout.
    ///
    /// It is a *probe*: nothing here is carried along the sweep, no trigger is
    /// gathered, and no state is written. The simulation cannot observe that
    /// this was called.
    ///
    /// # Resolution
    ///
    /// A tracer's `fraction` carries relative error, so recovering a distance
    /// from a long sweep loses precision exactly where it is needed — over
    /// 4096 units, an `f32` ulp is a substantial slice of a quarter-unit band.
    /// So the near probe is tried first and is short enough that its answer is
    /// precise around the band; the far probe only runs when the near one found
    /// nothing, where the question is "how high am I" and a unit either way
    /// does not matter.
    #[must_use]
    pub fn foot_clearance(&self) -> Option<Scalar> {
        let hull = self.profile.hull(self.state.player.crouched);
        let origin = self.state.player.origin;
        let probe = |distance: Scalar| {
            let trace = self.world.trace(&Sweep {
                start: origin,
                end: origin - straf3_sim::num::UP * distance,
                half_extents: hull.half_extents,
                center_offset: hull.center_offset,
            });
            if trace.start_solid || trace.all_solid {
                // Feet already inside geometry. Zero rather than `None`: there
                // is a surface, and the distance to it is nothing.
                return Some(straf3_sim::num::s(0.0));
            }
            (trace.fraction < straf3_sim::num::s(1.0)).then_some(trace.fraction * distance)
        };
        probe(CLEARANCE_NEAR_PROBE).or_else(|| probe(CLEARANCE_FAR_PROBE))
    }
}

/// The short downward probe [`Game::foot_clearance`] tries first.
///
/// Comfortably larger than a step (18 units is the profile's step height, and
/// this is deliberately smaller — a clearance reading that large is not about
/// the band any more) while short enough that the recovered distance stays
/// precise across the quarter-unit band the readout exists for.
pub const CLEARANCE_NEAR_PROBE: Scalar = straf3_sim::num::s(16.0);

/// The long downward probe, used only when the short one found nothing.
///
/// The same distance `scene`'s "is there a floor under the spawn" check uses,
/// so "no ground under the player" means the same thing in both places.
pub const CLEARANCE_FAR_PROBE: Scalar = straf3_sim::num::s(4096.0);

#[cfg(test)]
mod tests {
    use super::*;
    use straf3_platform::Action;
    use straf3_sim::num::{s, vec3};
    use straf3_sim::world::FlatGround;

    fn session() -> Game<FlatGround> {
        Game::new(
            FlatGround::at(s(0.0)),
            PhysicsProfile::cpm(),
            TickRate::HZ_125,
            vec3(s(0.0), s(0.0), s(24.0)),
            s(90.0),
        )
    }

    #[test]
    fn a_new_session_is_stood_still_at_the_spawn() {
        let game = session();
        assert_eq!(game.state().tick, 0);
        assert_eq!(game.state().time_ms, 0);
        assert_eq!(game.state().player.origin, vec3(s(0.0), s(0.0), s(24.0)));
        assert_eq!(game.input.look.yaw(), s(90.0));
        assert_eq!(game.state().checksum(), game.previous().checksum());
    }

    #[test]
    fn simulation_time_is_the_sum_of_the_commands_not_the_wall_clock() {
        let mut game = session();
        // 1000 ms of wall time in unhelpfully-sized pieces.
        for delta in [3u64, 17, 1, 40, 0, 939] {
            game.advance(delta);
        }
        assert_eq!(game.state().tick, 125);
        assert_eq!(game.state().time_ms, 1_000);
    }

    #[test]
    fn the_same_input_under_two_frame_rates_reaches_the_same_state() {
        // Criterion 5, stated as an equality rather than a property: a
        // stuttering frame rate must not change the simulation's trajectory.
        let mut smooth = session();
        let mut awful = session();
        for game in [&mut smooth, &mut awful] {
            game.input.set(Action::MoveForward, true);
            game.input.set(Action::MoveRight, true);
        }

        for _ in 0..125 {
            smooth.advance(8);
        }
        // Same total wall time, delivered as 119 empty frames and one enormous
        // one.
        for _ in 0..119 {
            awful.advance(0);
        }
        awful.advance(1_000);

        assert_eq!(smooth.state().tick, awful.state().tick);
        assert_eq!(smooth.state().checksum(), awful.state().checksum());
    }

    #[test]
    fn holding_forward_actually_moves_the_player() {
        let mut game = session();
        game.input.set(Action::MoveForward, true);
        for _ in 0..125 {
            game.advance(8);
        }
        // Looking along yaw 90 is +Y in Quake's convention.
        assert!(game.state().player.origin.y > s(100.0));
        assert!(game.horizontal_speed() > s(200.0));
    }

    #[test]
    fn the_previous_state_trails_the_current_one_by_exactly_one_tick() {
        let mut game = session();
        game.input.set(Action::MoveForward, true);
        game.advance(80); // ten ticks
        assert_eq!(game.state().tick, 10);
        assert_eq!(game.previous().tick, 9);
        assert_ne!(game.state().checksum(), game.previous().checksum());
    }

    #[test]
    fn respawning_puts_the_player_back_without_moving_the_camera() {
        let mut game = session();
        game.input.set(Action::MoveForward, true);
        game.advance(800);
        game.input.look.apply_motion(s(500.0), s(0.0));
        let looking = game.input.look.yaw();

        game.respawn();
        assert_eq!(game.state().player.origin, vec3(s(0.0), s(0.0), s(24.0)));
        assert_eq!(game.state().player.velocity, straf3_sim::num::ZERO);
        assert_eq!(
            game.state().player.view.yaw,
            straf3_sim::angle_to_short(looking)
        );
    }

    /// The readout measures what the simulation measures: standing on the
    /// plane is zero clearance, and lifting the player lifts the number by the
    /// same amount.
    #[test]
    fn foot_clearance_is_the_distance_from_the_feet_to_the_surface() {
        let mut game = session();
        // The flat spawn is stood on the floor: origin z=24, feet at z=0.
        assert_eq!(game.foot_clearance(), Some(s(0.0)));

        for height in [s(0.25), s(1.0), s(12.5), s(100.0), s(1000.0)] {
            game.state.player.origin = vec3(s(0.0), s(0.0), s(24.0) + height);
            let clearance = game.foot_clearance().expect("the plane is underneath");
            // Not exact equality: the far probe recovers a distance from a
            // fraction over 4096 units, and that is the precision this readout
            // documents. A tenth of a unit at 1000 is far finer than anything
            // the band needs.
            assert!(
                (clearance - height).abs() < s(0.1),
                "clearance {clearance} for a player {height} above the floor"
            );
        }
    }

    /// The quarter-unit band the whole readout exists to make legible is
    /// resolved, not rounded away.
    #[test]
    fn the_quarter_unit_band_is_resolved_rather_than_rounded_to_nothing() {
        let mut game = session();
        let band = PhysicsProfile::cpm().ground_trace_probe;
        assert_eq!(band, s(0.25), "the band this readout is about");

        // Either side of the band, and the readout must tell them apart — that
        // is the entire point. A number that could not would leave the largest
        // speed gain in the game looking like luck, which is what the operator
        // ruled against.
        let mut clearance_at = |height: Scalar| {
            game.state.player.origin = vec3(s(0.0), s(0.0), s(24.0) + height);
            game.foot_clearance().expect("the plane is underneath")
        };
        let inside = clearance_at(band * s(0.5));
        let outside = clearance_at(band * s(2.0));
        assert!(inside < band, "{inside} should read as inside the band");
        assert!(outside > band, "{outside} should read as outside it");
        // And the resolution is far finer than the band, not merely finer.
        let a = clearance_at(s(0.01));
        let b = clearance_at(s(0.02));
        assert!(
            a < b,
            "0.01 and 0.02 units must be distinguishable: {a} vs {b}"
        );
    }

    #[test]
    fn a_world_with_no_floor_reports_no_clearance_rather_than_a_number() {
        // `None`, so the overlay can say nothing rather than draw a distance to
        // a surface that is not there.
        let game = Game::new(
            straf3_sim::world::EmptyWorld,
            PhysicsProfile::cpm(),
            TickRate::HZ_125,
            vec3(s(0.0), s(0.0), s(24.0)),
            s(90.0),
        );
        assert_eq!(game.foot_clearance(), None);
    }

    #[test]
    fn measuring_clearance_does_not_move_the_simulation() {
        // It is a probe. If reading the overlay could change the run, the
        // overlay would be a mechanic.
        let mut game = session();
        game.input.set(Action::MoveForward, true);
        game.advance(400);
        let before = game.state().checksum();
        for _ in 0..100 {
            let _ = game.foot_clearance();
        }
        assert_eq!(game.state().checksum(), before);
        assert_eq!(game.previous().checksum(), game.previous().checksum());
    }

    #[test]
    fn a_recorded_session_replays_through_apply_to_the_same_checksum() {
        // The in-crate half of criterion 4: what was recorded is exactly what
        // was simulated, so replaying it lands in the same place.
        let mut played = session();
        played.record();
        played.input.set(Action::MoveForward, true);
        played.input.set(Action::MoveRight, true);
        for frame in 0..200u64 {
            played
                .input
                .look
                .apply_motion(s(frame as f32 % 7.0), s(0.0));
            played.input.set(Action::Jump, frame % 40 < 3);
            played.advance(frame % 23);
        }

        let cmds = played.recorder().unwrap().commands().to_vec();
        assert_eq!(cmds.len() as u32, played.state().tick);

        let mut replayed = session();
        for cmd in &cmds {
            replayed.apply(cmd);
        }
        assert_eq!(played.state().checksum(), replayed.state().checksum());
    }
}
