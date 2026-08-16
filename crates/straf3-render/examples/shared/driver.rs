//! An autopilot that drives the real simulation through the real course.
//!
//! Shared by the `window` and `web-demo` examples via `#[path]`, so both
//! targets show the same thing and neither carries its own idea of how the
//! loop works.
//!
//! # Why an autopilot and not keyboard input
//!
//! Input is `straf3-platform`'s to own — spec rev 6 §S gives it the window,
//! the mouse and the keyboard, and two crates capturing input would be two
//! things to keep in agreement for no benefit. What the renderer needs to
//! demonstrate on its own is narrower: that the camera follows the simulation,
//! that the drawn course is the course the player collides with, and that frames
//! are decoupled from ticks. A scripted player proves all three and needs
//! nobody to hold the mouse.
//!
//! The commands are exactly the [`UserCmd`]s the real input path will produce:
//! integer milliseconds, absolute view angles, signed-byte axes. Nothing here
//! reaches under the seam.

use straf3_render::InterpolationAlpha;

#[path = "course.rs"]
pub mod course;
use straf3_sim::num::{Scalar, s};
use straf3_sim::{
    Buttons, PhysicsProfile, PlayerState, SimState, TickRate, UserCmd, ViewAngles, step_in_place,
};

/// How fast the autopilot sweeps its view, in degrees per second.
///
/// Strafejumping is the relationship between view angle and velocity, so a
/// stationary view would accelerate to the ground speed cap and sit there. A
/// steady sweep with the strafe key held is the crudest possible strafejump,
/// and it is enough to see the course go past at speed.
const TURN_RATE: Scalar = s(75.0);

/// The simulation, driven by a scripted player.
pub struct Autopilot {
    /// The current simulation state.
    pub state: SimState,
    /// The state one command ago — the other end of the interpolation.
    pub prev: PlayerState,
    rate: TickRate,
    profile: PhysicsProfile,
    /// The yaw the course spawns the player at — the base the view sweeps from,
    /// so the autopilot sets off down the course rather than into a wall.
    spawn_yaw: Scalar,
}

impl Autopilot {
    /// Spawn at the course's own `info_player_start`, facing the way it says.
    pub fn new() -> Self {
        let (spawn, spawn_yaw) = course::spawn();
        let state = SimState::spawned_at(spawn, spawn_yaw);
        Self {
            prev: state.player,
            state,
            rate: TickRate::DEFAULT,
            profile: PhysicsProfile::cpm(),
            spawn_yaw,
        }
    }

    /// Catch the simulation up to `wall_ms` of elapsed wall-clock time, and
    /// return where between the last two states the frame sits.
    ///
    /// This is the fixed-step accumulator in miniature — criterion 5's shape.
    /// The simulation advances in whole commands of
    /// [`TickRate::command_millis`] and never in fractions of one, however
    /// long or short the frame was; the leftover becomes the interpolation
    /// factor rather than being simulated. `straf3-game` owns the real one;
    /// this is the renderer proving it can be driven by one.
    pub fn advance_to(&mut self, wall_ms: u64) -> InterpolationAlpha {
        let step_ms = u64::from(self.rate.command_millis());
        // A tab left in the background for a minute must not come back and
        // run 7500 commands in one frame. Q3 clamps the same way.
        let target = wall_ms.min(u64::from(self.state.time_ms) + 250);

        while u64::from(self.state.time_ms) + step_ms <= target {
            self.prev = self.state.player;
            let cmd = self.command();
            step_in_place(&mut self.state, &cmd, &course::get().world, &self.profile);
        }

        let leftover = target.saturating_sub(u64::from(self.state.time_ms));
        InterpolationAlpha(leftover as f32 / step_ms as f32)
    }

    /// The command the scripted player issues for the next tick.
    fn command(&self) -> UserCmd {
        let seconds = self.state.time_ms as Scalar / s(1000.0);
        let grounded = self.state.player.ground.is_grounded();

        UserCmd {
            duration_ms: self.rate.command_millis(),
            // Forward only while there is ground to push against; in the air
            // it is the strafe axis alone that accelerates, which is the whole
            // technique.
            forward_move: if grounded { 127 } else { 0 },
            right_move: 127,
            up_move: 0,
            // Jump is edge-triggered (`PMF_JUMP_HELD`), so holding it would
            // produce exactly one jump. Pressing it only when there is ground
            // underfoot is auto-hop, and it is also what a player does.
            buttons: if grounded {
                Buttons::JUMP
            } else {
                Buttons::NONE
            },
            view: ViewAngles::from_degrees(s(0.0), self.spawn_yaw + TURN_RATE * seconds, s(0.0)),
        }
    }

    /// Horizontal speed in units per second — the number that says whether the
    /// strafejump is working.
    pub fn speed(&self) -> Scalar {
        self.state.player.velocity.truncate().length()
    }
}
