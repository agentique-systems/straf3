//! A recorded run.

use straf3_sim::num::{Scalar, Vec3};
use straf3_sim::world::World;
use straf3_sim::{PhysicsProfile, RunState, SimState, TickRate, UserCmd, step_in_place};

use crate::codec;
use crate::digest::{FNV_OFFSET, fold};
use crate::error::{LoadError, Mismatch, VerifyError};
use crate::identity::{PhysicsId, WorldId, physics_digest};

/// The state a run began from.
///
/// Carried explicitly rather than looked up from the map, because a map's
/// spawn point is *not* part of [`WorldId`] — it is in
/// `CompiledMap::full_digest`, not in `collision_digest`, precisely so that
/// moving a spawn marker does not invalidate a leaderboard. That is the right
/// call for the leaderboard and it means the recording has to carry the spawn
/// itself, or a replay could start somewhere the run did not.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct RunStart {
    /// The command rate. Part of the physics, not of the frame loop — see
    /// [`TickRate`].
    pub rate: TickRate,
    /// Where the player stood on the first command.
    pub spawn: Vec3,
    /// Which way they faced, in degrees. Quantised to a 16-bit view angle by
    /// [`SimState::spawned_at`], the same way a command's angle is.
    pub yaw: Scalar,
}

impl RunStart {
    /// The simulation state this run began from.
    #[must_use]
    pub fn state(&self) -> SimState {
        SimState::spawned_at(self.spawn, self.yaw)
    }
}

/// What a run came to.
///
/// Three numbers, and each of them is a different kind of claim. The time is
/// what a player cares about; the digest is what makes the time believable;
/// `sim_time_ms` is what makes a run that never crossed the finish line still
/// comparable to itself.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Outcome {
    /// Simulation time after the last command, in whole milliseconds — the
    /// exact integer sum of every command's duration. Never a float, never a
    /// wall clock.
    pub sim_time_ms: u32,
    /// The run's time in whole milliseconds, when it crossed both the start
    /// and the finish volume.
    ///
    /// `None` for a run that never finished, which includes every recording
    /// made in a world with no timing triggers at all. A recording of an
    /// unfinished run is still a valid recording — it is just not a time.
    pub run_time_ms: Option<u32>,
    /// The rolling digest, folded over every command's
    /// [`SimState::checksum`] in order.
    ///
    /// The same scheme, seeded the same way, that criterion 2's cross-target
    /// check folds over its reference stream. Sticky: two machines that
    /// disagree about any single command cannot agree about this.
    pub digest: u64,
}

/// A recorded run: the inputs, and what they are bound to.
///
/// Storing inputs rather than positions is what makes a replay a regression
/// test — replaying it *re-runs the simulation*, and must reproduce the same
/// result. It is also what makes the binding necessary: a position is
/// self-describing and an input is not, so a recording has to say what world
/// and what physics its inputs meant. See [`crate::identity`].
#[derive(Debug, Clone, PartialEq)]
pub struct Recording {
    start: RunStart,
    commands: Vec<UserCmd>,
    world: WorldId,
    physics: PhysicsId,
    claimed: Outcome,
    trace: Option<Vec<u64>>,
}

impl Recording {
    /// Record a run: re-simulate `commands` against `world` and capture what
    /// they produced.
    ///
    /// # Why this re-simulates instead of being told the answer
    ///
    /// The alternative is a live recorder that folds each checksum as the game
    /// steps, and it has one failure this does not: the digest and the
    /// commands come from two different places, so a bug that drops a command
    /// on the way into the recording produces a file whose digest belongs to a
    /// run its command stream does not describe. Here the digest is by
    /// construction the digest of *these* commands on *this* world, because
    /// that is the only thing that was measured. Saving a minute-long personal
    /// best re-runs 7 500 commands, which is well under a millisecond.
    ///
    /// `world_id` must describe `world`. That is the one assertion this crate
    /// cannot check for itself — [`World`] is a trait with a single `trace`
    /// method and no identity — so it is the caller's obligation, discharged
    /// at the one place a recording is created by passing
    /// `WorldId::map(name, compiled.collision_digest())`. The physics identity
    /// is *derived* from `profile` and cannot be misdeclared.
    #[must_use]
    pub fn record(
        start: RunStart,
        commands: Vec<UserCmd>,
        world: &impl World,
        world_id: WorldId,
        profile: &PhysicsProfile,
        physics_name: impl Into<String>,
    ) -> Self {
        let (claimed, trace) = simulate(&start, &commands, world, profile);
        Self {
            start,
            commands,
            world: world_id,
            physics: PhysicsId {
                name: physics_name.into(),
                digest: physics_digest(profile),
            },
            claimed,
            trace: Some(trace),
        }
    }

    /// Assemble a recording from already-decoded parts. Used by the decoder;
    /// not a way to mint a recording with a digest nobody computed.
    pub(crate) const fn from_parts(
        start: RunStart,
        commands: Vec<UserCmd>,
        world: WorldId,
        physics: PhysicsId,
        claimed: Outcome,
        trace: Option<Vec<u64>>,
    ) -> Self {
        Self {
            start,
            commands,
            world,
            physics,
            claimed,
            trace,
        }
    }

    // ── reading the metadata, which is always free ──────────────────────────

    /// Where and how the run began.
    #[must_use]
    pub const fn start(&self) -> &RunStart {
        &self.start
    }

    /// The world this run was made in.
    #[must_use]
    pub const fn world(&self) -> &WorldId {
        &self.world
    }

    /// The physics this run was made under.
    #[must_use]
    pub const fn physics(&self) -> &PhysicsId {
        &self.physics
    }

    /// What the recording says the run came to. A *claim* until
    /// [`Self::verify`] has re-simulated it.
    #[must_use]
    pub const fn claimed(&self) -> Outcome {
        self.claimed
    }

    /// How many commands the run took.
    #[must_use]
    pub fn command_count(&self) -> usize {
        self.commands.len()
    }

    /// The per-command state checksums, if this recording carries them.
    ///
    /// Present on anything built by [`Self::record`], and on anything loaded
    /// from a file that stored them. See [`Self::to_bytes_with_checksums`].
    #[must_use]
    pub fn trace(&self) -> Option<&[u64]> {
        self.trace.as_deref()
    }

    // ── reading the commands, which is not free ─────────────────────────────

    /// The commands, once you have said what you intend to run them against.
    ///
    /// This is the shape contract item C6 takes in the API: there is no way to
    /// obtain a runnable command stream without presenting the world and the
    /// physics it will run under, and a stale binding comes back as a
    /// [`Mismatch`] instead of as a ghost that lands somewhere the player did
    /// not.
    ///
    /// # Errors
    ///
    /// [`Mismatch::World`] when `world_id` is not the world the run was made
    /// in — including the case that matters most, the same map recompiled
    /// since. [`Mismatch::Physics`] when `profile`'s constants are not the
    /// ones the run was made under.
    pub fn commands_for(
        &self,
        world_id: &WorldId,
        profile: &PhysicsProfile,
    ) -> Result<&[UserCmd], Mismatch> {
        self.check(world_id, profile)?;
        Ok(&self.commands)
    }

    /// The commands with no binding check.
    ///
    /// For inspecting or re-encoding a recording — listing what a file
    /// contains, converting it, counting it. **Not** for driving a
    /// simulation: every guarantee in [`crate::identity`] is discharged by
    /// [`Self::commands_for`], and this is the function that skips it. It is
    /// named the way it is so that a review can grep for it.
    #[must_use]
    pub fn commands_unchecked(&self) -> &[UserCmd] {
        &self.commands
    }

    /// Whether this recording may be replayed against this world and physics.
    ///
    /// # Errors
    ///
    /// As [`Self::commands_for`].
    pub fn check(&self, world_id: &WorldId, profile: &PhysicsProfile) -> Result<(), Mismatch> {
        if !self.world.is_same_world(world_id) {
            return Err(Mismatch::World {
                recorded: self.world.clone(),
                actual: world_id.clone(),
            });
        }
        let actual = physics_digest(profile);
        if actual != self.physics.digest {
            return Err(Mismatch::Physics {
                recorded: self.physics.clone(),
                actual,
            });
        }
        Ok(())
    }

    // ── running it again ────────────────────────────────────────────────────

    /// Re-simulate the run and report what it produced *this* time.
    ///
    /// Does not compare against the claim; that is [`Self::verify`]. Useful on
    /// its own for a ghost, which needs the states rather than the verdict.
    ///
    /// # Errors
    ///
    /// As [`Self::commands_for`] — the binding is checked before anything is
    /// simulated.
    pub fn resimulate(
        &self,
        world: &impl World,
        world_id: &WorldId,
        profile: &PhysicsProfile,
    ) -> Result<Outcome, Mismatch> {
        let commands = self.commands_for(world_id, profile)?;
        Ok(simulate(&self.start, commands, world, profile).0)
    }

    /// Re-simulate the run and require it to reproduce the recording.
    ///
    /// This is the whole promise of the format in one call: the same file, on
    /// any of the four targets this project verifies on, must come back with
    /// the same final time and the same digest.
    ///
    /// # Errors
    ///
    /// [`VerifyError::Mismatch`] when the binding is wrong — nothing is
    /// simulated. [`VerifyError::Diverged`] when it ran and produced a
    /// different run; if the recording carries a checksum trace the error
    /// names the first command the two disagree on.
    pub fn verify(
        &self,
        world: &impl World,
        world_id: &WorldId,
        profile: &PhysicsProfile,
    ) -> Result<Outcome, VerifyError> {
        let commands = self.commands_for(world_id, profile)?;
        let (actual, trace) = simulate(&self.start, commands, world, profile);
        if actual == self.claimed {
            return Ok(actual);
        }
        let first_diverging_command = self.trace.as_ref().and_then(|recorded| {
            recorded
                .iter()
                .zip(&trace)
                .position(|(a, b)| a != b)
                .map(|n| n as u32)
        });
        Err(VerifyError::Diverged {
            claimed: Box::new(self.claimed),
            actual: Box::new(actual),
            first_diverging_command,
        })
    }

    /// Replay the run, handing each intermediate state to `observe`.
    ///
    /// What a ghost is made of: the caller keeps the positions it wants to
    /// draw. The state passed to `observe` is the state *after* the command at
    /// that index, which is the same instant the digest folds.
    ///
    /// # Errors
    ///
    /// As [`Self::commands_for`].
    pub fn replay(
        &self,
        world: &impl World,
        world_id: &WorldId,
        profile: &PhysicsProfile,
        mut observe: impl FnMut(u32, &SimState),
    ) -> Result<Outcome, Mismatch> {
        let commands = self.commands_for(world_id, profile)?;
        let mut state = self.start.state();
        let mut digest = FNV_OFFSET;
        for (i, cmd) in commands.iter().enumerate() {
            step_in_place(&mut state, cmd, world, profile);
            digest = fold(digest, state.checksum());
            observe(i as u32, &state);
        }
        Ok(outcome_of(&state, digest))
    }

    // ── bytes ───────────────────────────────────────────────────────────────

    /// Serialise to `.s3d` bytes, without the per-command checksum trace.
    ///
    /// What a saved personal best uses. The run digest alone already proves
    /// the run: the fold is sticky, so a machine that disagrees about any
    /// command cannot agree about the digest. The trace only buys a better
    /// *diagnosis* of a disagreement, and a ghost file does not need one.
    #[must_use]
    pub fn to_bytes(&self) -> Vec<u8> {
        codec::encode(self, false)
    }

    /// Serialise to `.s3d` bytes including one `u64` per command — eight bytes
    /// a command, about 60 KiB a minute.
    ///
    /// Worth it for a recording used as evidence rather than as a ghost. With
    /// the trace present, a failed [`Self::verify`] names the *first*
    /// diverging command instead of only reporting that two machines
    /// disagree — the diagnostic criterion 2's report publishes, for the same
    /// reason — and the loader can check that the stored run digest was
    /// actually derived from this file's own checksums.
    ///
    /// `None` when this recording carries no trace, which happens only when it
    /// was itself loaded from a file written without one. Returning `None`
    /// rather than quietly writing the compact form is the point: a caller
    /// asking for evidence should be told it is not available, not handed
    /// something that looks like it.
    #[must_use]
    pub fn to_bytes_with_checksums(&self) -> Option<Vec<u8>> {
        self.trace.is_some().then(|| codec::encode(self, true))
    }

    /// Parse `.s3d` bytes.
    ///
    /// # Errors
    ///
    /// See [`LoadError`]. Every one of them is a refusal; there is no lenient
    /// mode.
    pub fn from_bytes(bytes: &[u8]) -> Result<Self, LoadError> {
        codec::decode(bytes)
    }
}

/// Run a command stream and fold as it goes.
///
/// The single definition of what re-simulating a recording means: everything
/// in this crate that produces an [`Outcome`] goes through here, so a
/// recording's digest and the digest it is checked against cannot be computed
/// two different ways.
fn simulate(
    start: &RunStart,
    commands: &[UserCmd],
    world: &impl World,
    profile: &PhysicsProfile,
) -> (Outcome, Vec<u64>) {
    let mut state = start.state();
    let mut digest = FNV_OFFSET;
    let mut trace = Vec::with_capacity(commands.len());
    for cmd in commands {
        step_in_place(&mut state, cmd, world, profile);
        let checksum = state.checksum();
        digest = fold(digest, checksum);
        trace.push(checksum);
    }
    (outcome_of(&state, digest), trace)
}

fn outcome_of(state: &SimState, digest: u64) -> Outcome {
    Outcome {
        sim_time_ms: state.time_ms,
        // Deliberately not `RunState::elapsed_ms`, which also answers for a
        // run still in progress. An unfinished run has no time — reporting the
        // split it had reached as though it were a result is how a leaderboard
        // ends up with a personal best nobody set.
        run_time_ms: match state.run {
            RunState::Finished {
                started_at_ms,
                finished_at_ms,
            } => Some(finished_at_ms.saturating_sub(started_at_ms)),
            RunState::NotStarted | RunState::Running { .. } => None,
        },
        digest,
    }
}
