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
//! # Interpolation, and why two states are kept
//!
//! The renderer draws between the last two simulation states. Keeping the
//! previous state here rather than in the renderer means the renderer cannot
//! accidentally become the thing that decides when the simulation advances —
//! it is handed two finished states and a number between them, and there is
//! no API through which it could ask for a third.

use straf3_platform::InputState;
use straf3_sim::num::{Scalar, Vec3};
use straf3_sim::{PhysicsProfile, SimState, TickRate, UserCmd, World};

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
        }
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
    pub fn respawn(&mut self) {
        self.state = SimState::spawned_at(self.spawn, self.input.look.yaw());
        self.state.player.view = self.input.look.angles();
        self.previous = self.state;
    }

    /// Advance the session by `elapsed_ms` of wall time.
    ///
    /// Returns how many simulation ticks ran, which may be zero (a frame
    /// shorter than a tick) or many (a frame longer than one). **This is the
    /// decoupling** — the caller reports real time and gets told what happened
    /// to the simulation, never the other way round.
    pub fn advance(&mut self, elapsed_ms: u64) -> u32 {
        let ticks = self.step.advance(elapsed_ms);
        let duration_ms = self.step.tick_ms();
        for _ in 0..ticks {
            self.previous = self.state;
            let cmd = command_from_input(&self.input, duration_ms);
            if let Some(recorder) = &mut self.recorder {
                recorder.push(cmd);
            }
            advance_one(&mut self.state, &cmd, &self.world, &self.profile);
        }
        ticks
    }

    /// Apply one command directly, bypassing the wall clock entirely.
    ///
    /// This is how a recorded sequence is replayed through the *windowed*
    /// build's own code path, and how a test drives the session with no clock
    /// at all.
    pub fn apply(&mut self, cmd: &UserCmd) {
        self.previous = self.state;
        advance_one(&mut self.state, cmd, &self.world, &self.profile);
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
}

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
        assert_eq!(game.state().player.view.yaw, looking);
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
