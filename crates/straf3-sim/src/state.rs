//! Everything the simulation remembers between commands.

use crate::cmd::ViewAngles;
use crate::num::{Scalar, Vec3, s, to_bits};

/// Whether the player is standing on something.
///
/// An enum rather than a `bool` plus a normal field, because "on the ground
/// but with no ground normal" is not a state that should be representable —
/// the ramp behaviour the game is about is entirely a function of that normal.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub enum GroundState {
    /// Not touching walkable ground: gravity applies, air acceleration rules
    /// apply.
    #[default]
    Airborne,
    /// Standing on walkable ground.
    Grounded {
        /// Surface normal underfoot. Not assumed to be straight up: ramps are
        /// the point.
        normal: Vec3,
    },
}

impl GroundState {
    /// Whether the player is on walkable ground.
    #[must_use]
    pub const fn is_grounded(&self) -> bool {
        matches!(self, Self::Grounded { .. })
    }

    /// The ground normal, or `None` when airborne.
    #[must_use]
    pub const fn normal(&self) -> Option<Vec3> {
        match self {
            Self::Grounded { normal } => Some(*normal),
            Self::Airborne => None,
        }
    }
}

/// Countdowns the movement code keeps, all in **whole milliseconds**.
///
/// Integers, not floats, for the same reason [`crate::UserCmd::duration_ms`]
/// is: a float timer accumulates rounding error over a long run, so two runs
/// of the same input would eventually disagree about whether a window was
/// still open. Integers cannot drift, so a technique that works on tick 40,000
/// works identically on the replay.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Timers {
    /// Time left in which movement is under scripted control rather than
    /// player control — Q3's `pm_time`, used by jump pads and knockback.
    pub movement_locked_ms: u16,
    /// Time since the player last landed. Drives the CPM double-jump window
    /// (see [`crate::PhysicsProfile::double_jump_window_ms`]).
    pub since_landed_ms: u16,
    /// Time since the player last jumped.
    pub since_jumped_ms: u16,
}

impl Timers {
    /// Advance every timer by `ms`.
    ///
    /// Countdowns saturate at zero and elapsed counters saturate at the
    /// maximum, so a long run cannot wrap a timer around into a state that
    /// briefly re-enables a technique.
    pub fn advance(&mut self, ms: u16) {
        self.movement_locked_ms = self.movement_locked_ms.saturating_sub(ms);
        self.since_landed_ms = self.since_landed_ms.saturating_add(ms);
        self.since_jumped_ms = self.since_jumped_ms.saturating_add(ms);
    }
}

/// The player.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct PlayerState {
    /// Position of the player origin. Quake units, Z up.
    ///
    /// Note the Quake convention: the origin is not at the feet. The hull
    /// spans `origin + hull_mins` to `origin + hull_maxs`, so a standing
    /// player resting on a floor at z=0 has an origin at z=24.
    pub origin: Vec3,
    /// Velocity in units per second.
    pub velocity: Vec3,
    /// Where the player is looking. Mirrors the last command's view angles;
    /// held in state so a replay can be rendered without re-reading commands.
    pub view: ViewAngles,
    /// Whether the player is standing on something, and on what.
    pub ground: GroundState,
    /// Movement timers. See [`Timers`].
    pub timers: Timers,
    /// Whether the crouch button was held at the end of the last command.
    pub crouched: bool,
    /// Whether the jump button was held at the end of the last command.
    ///
    /// Needed because jumping is edge-triggered: holding jump must not
    /// re-trigger, which is what makes bunny-hop timing a skill.
    pub jump_held: bool,
}

/// The timer: where the run is, between the start line and the finish.
///
/// Times are in whole milliseconds accumulated from command durations, never
/// read from a clock. That is what makes a time reproducible — the same input
/// yields the same time on a 60 fps laptop and a 240 fps desktop.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum RunState {
    /// Before the start line.
    #[default]
    NotStarted,
    /// The clock is running. `started_at_ms` is simulation time at the start
    /// line.
    Running {
        /// Simulation time when the start line was crossed.
        started_at_ms: u32,
    },
    /// Past the finish line.
    Finished {
        /// Simulation time when the start line was crossed.
        started_at_ms: u32,
        /// Simulation time when the finish line was crossed.
        finished_at_ms: u32,
    },
}

impl RunState {
    /// The run's elapsed time at simulation time `now_ms`, or `None` before
    /// the start line.
    #[must_use]
    pub const fn elapsed_ms(&self, now_ms: u32) -> Option<u32> {
        match self {
            Self::NotStarted => None,
            Self::Running { started_at_ms } => Some(now_ms.saturating_sub(*started_at_ms)),
            Self::Finished {
                started_at_ms,
                finished_at_ms,
            } => Some(finished_at_ms.saturating_sub(*started_at_ms)),
        }
    }

    /// Start the clock. Crossing the start line again mid-run does not restart
    /// it; a restart is a new [`SimState`].
    pub fn start(&mut self, now_ms: u32) {
        if matches!(self, Self::NotStarted) {
            *self = Self::Running {
                started_at_ms: now_ms,
            };
        }
    }

    /// Stop the clock. Does nothing unless the run is in progress.
    pub fn finish(&mut self, now_ms: u32) {
        if let Self::Running { started_at_ms } = *self {
            *self = Self::Finished {
                started_at_ms,
                finished_at_ms: now_ms,
            };
        }
    }
}

/// The complete simulation state.
///
/// # The contract
///
/// This struct is the *entire* state of the game world that the simulation
/// cares about. Given a `SimState`, a sequence of [`crate::UserCmd`]s, a
/// [`crate::World`] and a [`crate::PhysicsProfile`], the result is fixed —
/// there is nothing else consulted, no hidden accumulator, no clock, no file.
/// That is what makes replays, ghosts, regression tests, headless servers and
/// future RL environments possible without touching the physics.
///
/// The practical rule: **if the physics reads it, it lives here.** A cached
/// value stashed anywhere else is a determinism bug waiting for the day the
/// cache and the state disagree.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct SimState {
    /// The player.
    pub player: PlayerState,
    /// The run timer.
    pub run: RunState,
    /// How many commands have been applied. Not a time — commands may differ
    /// in duration.
    pub tick: u32,
    /// Simulation time, in whole milliseconds, being the exact sum of every
    /// command duration applied so far.
    ///
    /// Exact because it is an integer sum: it cannot drift from the sum of the
    /// commands in a recording, however long the run.
    pub time_ms: u32,
}

impl SimState {
    /// A state with the player standing still at `spawn`, looking along
    /// `yaw`, clock not started.
    #[must_use]
    pub fn spawned_at(spawn: Vec3, yaw: Scalar) -> Self {
        Self {
            player: PlayerState {
                origin: spawn,
                view: ViewAngles {
                    pitch: s(0.0),
                    yaw,
                    roll: s(0.0),
                },
                ..PlayerState::default()
            },
            ..Self::default()
        }
    }

    /// A 64-bit digest of the exact bits of this state.
    ///
    /// # Why a checksum
    ///
    /// Determinism here means *bit-identical*, and comparing two runs by
    /// eyeballing printed positions would hide a last-bit divergence — which
    /// is precisely the kind that grows into a visibly different run 30
    /// seconds later. This folds the exact bit patterns, so it changes when
    /// anything changes, including `0.0` versus `-0.0`.
    ///
    /// It is FNV-1a: order-dependent, no allocation, no dependency, identical
    /// on every platform for the same input bits. It is an equality check, not
    /// a security primitive.
    #[must_use]
    pub fn checksum(&self) -> u64 {
        const OFFSET: u64 = 0xcbf2_9ce4_8422_2325;
        const PRIME: u64 = 0x0000_0100_0000_01b3;

        let mut h = OFFSET;
        let byte = |b: u8, h: &mut u64| {
            *h ^= u64::from(b);
            *h = h.wrapping_mul(PRIME);
        };
        let fold_u32 = |v: u32, h: &mut u64| {
            for b in v.to_le_bytes() {
                byte(b, h);
            }
        };
        let fold_scalar = |v: Scalar, h: &mut u64| fold_u32(to_bits(v), h);
        let fold_vec = |v: Vec3, h: &mut u64| {
            fold_scalar(v.x, h);
            fold_scalar(v.y, h);
            fold_scalar(v.z, h);
        };

        fold_vec(self.player.origin, &mut h);
        fold_vec(self.player.velocity, &mut h);
        fold_scalar(self.player.view.pitch, &mut h);
        fold_scalar(self.player.view.yaw, &mut h);
        fold_scalar(self.player.view.roll, &mut h);
        match self.player.ground {
            GroundState::Airborne => fold_u32(0, &mut h),
            GroundState::Grounded { normal } => {
                fold_u32(1, &mut h);
                fold_vec(normal, &mut h);
            }
        }
        fold_u32(u32::from(self.player.timers.movement_locked_ms), &mut h);
        fold_u32(u32::from(self.player.timers.since_landed_ms), &mut h);
        fold_u32(u32::from(self.player.timers.since_jumped_ms), &mut h);
        fold_u32(u32::from(self.player.crouched), &mut h);
        fold_u32(u32::from(self.player.jump_held), &mut h);
        match self.run {
            RunState::NotStarted => fold_u32(0, &mut h),
            RunState::Running { started_at_ms } => {
                fold_u32(1, &mut h);
                fold_u32(started_at_ms, &mut h);
            }
            RunState::Finished {
                started_at_ms,
                finished_at_ms,
            } => {
                fold_u32(2, &mut h);
                fold_u32(started_at_ms, &mut h);
                fold_u32(finished_at_ms, &mut h);
            }
        }
        fold_u32(self.tick, &mut h);
        fold_u32(self.time_ms, &mut h);
        h
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::num::vec3;

    #[test]
    fn checksum_notices_a_single_bit() {
        let a = SimState::spawned_at(vec3(s(0.0), s(0.0), s(64.0)), s(0.0));
        let mut b = a;
        b.player.origin.x = Scalar::from_bits(to_bits(a.player.origin.x) + 1);
        assert_ne!(a.checksum(), b.checksum());
    }

    #[test]
    fn checksum_notices_signed_zero() {
        let a = SimState::default();
        let mut b = a;
        b.player.velocity.x = s(-0.0);
        assert_eq!(a.player.velocity.x, b.player.velocity.x); // == says equal
        assert_ne!(a.checksum(), b.checksum()); // bits say otherwise
    }

    #[test]
    fn timers_saturate_rather_than_wrapping() {
        let mut t = Timers {
            movement_locked_ms: 5,
            since_landed_ms: u16::MAX - 1,
            since_jumped_ms: 0,
        };
        t.advance(10);
        assert_eq!(t.movement_locked_ms, 0);
        assert_eq!(t.since_landed_ms, u16::MAX);
    }

    #[test]
    fn the_clock_runs_between_the_lines() {
        let mut run = RunState::NotStarted;
        assert_eq!(run.elapsed_ms(1000), None);
        run.start(1000);
        assert_eq!(run.elapsed_ms(1500), Some(500));
        run.start(1200); // crossing the start line again does not restart
        assert_eq!(run.elapsed_ms(1500), Some(500));
        run.finish(2000);
        assert_eq!(run.elapsed_ms(9999), Some(1000));
    }
}
