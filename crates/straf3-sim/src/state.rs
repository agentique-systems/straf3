//! Everything the simulation remembers between commands.

use crate::cmd::ViewAngles;
use crate::num::{Scalar, Vec3, to_bits};

/// Whether the player is standing on something.
///
/// An enum rather than a `bool` plus a normal field, because "on the ground
/// but with no ground normal" is not a state that should be representable —
/// the ramp behaviour the game is about is entirely a function of that normal.
///
/// # Why there are three states and not two
///
/// Quake keeps two separate booleans, `pml.groundPlane` and `pml.walking`, and
/// the gap between them is where ramp sliding lives. A plane steeper than
/// [`crate::PhysicsProfile::min_walk_normal`] is touched — velocity is clipped
/// to it, so the player follows its surface — but it is not walkable, so no
/// ground friction is applied and the air rules govern acceleration. Collapsing
/// that into a `bool` would delete the technique.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub enum GroundState {
    /// Touching nothing: gravity applies, air acceleration rules apply.
    #[default]
    Airborne,
    /// Touching a plane too steep to stand on — Q3's `groundPlane && !walking`.
    ///
    /// Velocity is clipped to this plane, but the player is *not* walking:
    /// friction is not applied and jumping is not available. Only gravity's
    /// component along the slope bleeds speed, which is why a steep ramp
    /// preserves it.
    Sliding {
        /// Surface normal of the plane being slid along.
        normal: Vec3,
    },
    /// Standing on walkable ground.
    Grounded {
        /// Surface normal underfoot. Not assumed to be straight up: ramps are
        /// the point.
        normal: Vec3,
    },
}

impl GroundState {
    /// Whether the player is on *walkable* ground — Q3's `pml.walking`.
    ///
    /// [`Self::Sliding`] is deliberately not included: a player on a steep ramp
    /// is touching geometry but is under air rules.
    #[must_use]
    pub const fn is_grounded(&self) -> bool {
        matches!(self, Self::Grounded { .. })
    }

    /// Whether there is a plane underfoot at all — Q3's `pml.groundPlane`.
    #[must_use]
    pub const fn is_on_plane(&self) -> bool {
        !matches!(self, Self::Airborne)
    }

    /// The ground normal, or `None` when touching nothing.
    #[must_use]
    pub const fn normal(&self) -> Option<Vec3> {
        match self {
            Self::Grounded { normal } | Self::Sliding { normal } => Some(*normal),
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
    /// Time since the player last landed.
    pub since_landed_ms: u16,
    /// Time since the player last jumped.
    pub since_jumped_ms: u16,
    /// Time left in which a jump would count as a CPM double jump.
    ///
    /// Armed to [`crate::PhysicsProfile::double_jump_window_ms`] on landing
    /// from a jump, counted down, and consumed by the jump that uses it. A
    /// countdown rather than a "time since landed" comparison because the
    /// window has to be *spent*: two jumps in one window must not both be
    /// boosted, and a comparison against an elapsed counter would allow that.
    pub double_jump_ms: u16,

    // ── candidate mechanics (spec rev 3, criterion 4) ───────────────────
    //
    // Three countdowns for the three mechanics in
    // [`crate::PhysicsProfile::experimental`]. All zero under `vq3` and `cpm`,
    // because the constants that arm them are zero there.
    //
    // These are also the *whole* legibility budget for the candidates: a
    // mechanic is only assessable if a player can see it happening, and what
    // the overlay can see is `SimState`. Each of the three below is readable
    // above the seam and folded into [`SimState::checksum`], which is what
    // makes "the overlay can print it" and "a replay cannot diverge silently"
    // the same statement.
    /// Time left in the current crouch slide.
    ///
    /// Non-zero means the player is sliding: `PM_Friction` reads
    /// [`crate::PhysicsProfile::slide_friction`] instead of
    /// [`crate::PhysicsProfile::friction`]. Armed on the crouch press that
    /// starts a slide, cleared on leaving the ground.
    pub slide_ms: u16,
    /// Time left in which an air jump press would spend a dash.
    ///
    /// Armed on the same landing that arms [`Self::double_jump_ms`] and under
    /// the same provenance rule, so the two windows open together and compete
    /// for the same input. Zeroed by the dash that uses it.
    pub dash_ms: u16,
    /// Time left in which a jump press would be a wall jump.
    ///
    /// Armed by the slide solver whenever the player is clipped against a
    /// plane at or below [`crate::PhysicsProfile::wall_normal_max`], alongside
    /// [`PlayerState::wall_normal`]. Zeroed by the wall jump that uses it.
    pub wall_contact_ms: u16,
}

impl Timers {
    /// Advance every timer by `ms`.
    ///
    /// Countdowns saturate at zero and elapsed counters saturate at the
    /// maximum, so a long run cannot wrap a timer around into a state that
    /// briefly re-enables a technique.
    pub fn advance(&mut self, ms: u16) {
        self.movement_locked_ms = self.movement_locked_ms.saturating_sub(ms);
        self.double_jump_ms = self.double_jump_ms.saturating_sub(ms);
        self.slide_ms = self.slide_ms.saturating_sub(ms);
        self.dash_ms = self.dash_ms.saturating_sub(ms);
        self.wall_contact_ms = self.wall_contact_ms.saturating_sub(ms);
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
    /// Whether a jump has been taken and the jump input not yet released —
    /// Q3's `PMF_JUMP_HELD`.
    ///
    /// Needed because jumping is edge-triggered: holding jump must not
    /// re-trigger, which is what makes bunny-hop timing a skill. Set by the
    /// jump itself, cleared the moment the input goes away, exactly as
    /// `PmoveSingle` does.
    pub jump_held: bool,
    /// Whether the player left the ground by jumping rather than by walking
    /// off an edge.
    ///
    /// The CPM double-jump window opens on landing, but only for a landing
    /// that ended a jump — stepping off a crate and jumping on contact is not a
    /// double jump. This is that one bit of provenance, and it lives in state
    /// because [`crate::step`] may not consult anything else.
    pub left_ground_by_jumping: bool,
    /// Surface normal of the last wall the player was clipped against, while
    /// [`Timers::wall_contact_ms`] is still running.
    ///
    /// A wall jump pushes along this, so the direction has to survive the
    /// commands between touching the wall and pressing jump — the slide
    /// solver's plane list does not, it lives for one command.
    ///
    /// Zero whenever [`crate::PhysicsProfile::wall_contact_window_ms`] is
    /// zero, which is both canon profiles: with the mechanic off the solver
    /// never writes here at all, so a canon run's state is bit-identical to
    /// its pre-wave self and not merely behaviourally identical.
    ///
    /// Meaningless when `wall_contact_ms` is zero, and deliberately *not*
    /// cleared when the timer expires: clearing it would be a second write for
    /// no reader to benefit from, and the timer is the thing every reader
    /// already has to check. It is folded into [`SimState::checksum`] anyway,
    /// because a stale value the mover could read on a later command is
    /// exactly the kind of state a digest exists to cover.
    pub wall_normal: Vec3,
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
    /// `yaw` degrees, clock not started.
    ///
    /// `yaw` is in degrees and is quantised to a 16-bit view angle on the way
    /// in (contract item C3) — a spawn yaw is an input like any other, and a
    /// spawn the simulation could not have been *commanded* into would make
    /// the first tick of a replay unreproducible.
    #[must_use]
    pub fn spawned_at(spawn: Vec3, yaw: Scalar) -> Self {
        Self {
            player: PlayerState {
                origin: spawn,
                view: ViewAngles::looking_along(yaw),
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
        // The view folds as the 16-bit angles it is, not as the degrees they
        // stand for: the shorts are the recorded value, and folding a derived
        // float would put a conversion between the recording and the digest
        // that verifies it.
        fold_u32(u32::from(self.player.view.pitch), &mut h);
        fold_u32(u32::from(self.player.view.yaw), &mut h);
        fold_u32(u32::from(self.player.view.roll), &mut h);
        match self.player.ground {
            GroundState::Airborne => fold_u32(0, &mut h),
            GroundState::Grounded { normal } => {
                fold_u32(1, &mut h);
                fold_vec(normal, &mut h);
            }
            GroundState::Sliding { normal } => {
                fold_u32(2, &mut h);
                fold_vec(normal, &mut h);
            }
        }
        fold_u32(u32::from(self.player.timers.movement_locked_ms), &mut h);
        fold_u32(u32::from(self.player.timers.since_landed_ms), &mut h);
        fold_u32(u32::from(self.player.timers.since_jumped_ms), &mut h);
        fold_u32(u32::from(self.player.timers.double_jump_ms), &mut h);
        // The candidate mechanics' state. Folded for the same reason
        // `double_jump_ms` is: the mover branches on all four, so a replay
        // could diverge with a matching checksum if they were left out — which
        // is worse than diverging visibly. See
        // `the_checksum_covers_the_state_a_technique_depends_on`.
        fold_u32(u32::from(self.player.timers.slide_ms), &mut h);
        fold_u32(u32::from(self.player.timers.dash_ms), &mut h);
        fold_u32(u32::from(self.player.timers.wall_contact_ms), &mut h);
        fold_vec(self.player.wall_normal, &mut h);
        fold_u32(u32::from(self.player.crouched), &mut h);
        fold_u32(u32::from(self.player.jump_held), &mut h);
        fold_u32(u32::from(self.player.left_ground_by_jumping), &mut h);
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
    use crate::num::s;
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
            double_jump_ms: 5,
            slide_ms: 5,
            dash_ms: 5,
            wall_contact_ms: 5,
        };
        t.advance(10);
        assert_eq!(t.movement_locked_ms, 0);
        assert_eq!(t.double_jump_ms, 0);
        assert_eq!(t.since_landed_ms, u16::MAX);
        // The candidate windows are countdowns like the double-jump one, not
        // elapsed counters: a window that wrapped past zero would briefly
        // re-enable a technique thousands of commands later.
        assert_eq!(t.slide_ms, 0);
        assert_eq!(t.dash_ms, 0);
        assert_eq!(t.wall_contact_ms, 0);
    }

    /// Every candidate timer is actually advanced.
    ///
    /// Separate from the saturation test above because that one only proves
    /// they reach zero, which a timer that is never decremented *from* a small
    /// value also does. A window the mover reads but `advance` forgets would
    /// stay open forever — the exact bug that turns a technique into a
    /// permanent state, and it is one missing line away at all times.
    #[test]
    fn every_candidate_window_counts_down() {
        let mut t = Timers {
            slide_ms: 600,
            dash_ms: 400,
            wall_contact_ms: 200,
            ..Timers::default()
        };
        t.advance(8);
        assert_eq!(t.slide_ms, 592);
        assert_eq!(t.dash_ms, 392);
        assert_eq!(t.wall_contact_ms, 192);
    }

    #[test]
    fn the_checksum_covers_the_state_a_technique_depends_on() {
        // Every field the movement code branches on has to be in the digest,
        // or a replay could diverge with a matching checksum — which is worse
        // than diverging visibly.
        let base = SimState::default();

        let mut armed = base;
        armed.player.timers.double_jump_ms = 400;
        assert_ne!(base.checksum(), armed.checksum());

        let mut jumped = base;
        jumped.player.left_ground_by_jumping = true;
        assert_ne!(base.checksum(), jumped.checksum());

        // The three candidate windows and the wall normal. Each is checked on
        // its own rather than all at once, because a fold that missed exactly
        // one of them would still pass a combined assertion.
        for arm in [
            (|t: &mut Timers| t.slide_ms = 600) as fn(&mut Timers),
            |t: &mut Timers| t.dash_ms = 400,
            |t: &mut Timers| t.wall_contact_ms = 200,
        ] {
            let mut armed = base;
            arm(&mut armed.player.timers);
            assert_ne!(
                base.checksum(),
                armed.checksum(),
                "a candidate window is not folded into the checksum"
            );
        }
        for axis in 0..3 {
            let mut against_wall = base;
            let v = &mut against_wall.player.wall_normal;
            *match axis {
                0 => &mut v.x,
                1 => &mut v.y,
                _ => &mut v.z,
            } = s(1.0);
            assert_ne!(
                base.checksum(),
                against_wall.checksum(),
                "wall_normal component {axis} is not folded into the checksum"
            );
        }

        // Sliding and Grounded on the same plane are different states.
        let n = vec3(s(0.0), s(0.6), s(0.8));
        let mut sliding = base;
        sliding.player.ground = GroundState::Sliding { normal: n };
        let mut grounded = base;
        grounded.player.ground = GroundState::Grounded { normal: n };
        assert_ne!(sliding.checksum(), grounded.checksum());
        assert!(!sliding.player.ground.is_grounded());
        assert!(sliding.player.ground.is_on_plane());
        assert_eq!(sliding.player.ground.normal(), Some(n));
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
