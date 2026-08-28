//! Re-simulating a submitted run, which is the only thing that makes a time
//! rankable.
//!
//! # The two rules that make this worth doing at all
//!
//! **The time is computed, never accepted.** [`Verdict::time_ms`] comes out of
//! `SimState.run` on this machine. What the recording claimed is carried
//! through as [`Verdict::client_time_ms`] into `runs.client_time_ms`, where it
//! is a diagnostic and nothing else — never ranked, never compared against,
//! never rendered as authoritative. ARCHITECTURE §8.1: "the security property
//! is not 'we checked their time', it is 'we computed the time; theirs was
//! never an input'." That is why this module calls
//! [`Recording::replay`](straf3_replay::Recording::replay) and not
//! [`Recording::verify`](straf3_replay::Recording::verify) — `verify` compares
//! the whole claimed `Outcome`, claimed time included, and a gate that consults
//! the client's number is a gate that has an opinion about it.
//!
//! **The comparison is the rolling digest, not the end state.** ARCHITECTURE
//! §1.3: the determinism probe found a run whose final checksum matched across
//! builds while 29 of its 1,200 intermediate states did not. So what is
//! compared is the FNV-1a fold over *every* command's `SimState::checksum()`.
//! It is sticky — any state that ever differed changes it permanently — which
//! is exactly what an end-state comparison is not.
//!
//! # Localising a divergence
//!
//! ARCHITECTURE §3.2 describes a sparse checkpoint trail and a binary search
//! over it. The `.s3d` format `straf3-replay` actually implements carries
//! either a checksum for *every* command or none at all, so localisation here
//! is a linear scan over the full trace — strictly better than a binary search
//! over a sparse one, and exact rather than bracketed. When the file was
//! written in the compact form there is no trail to search and
//! `divergence_at` stays null rather than being guessed at.
//!
//! # Refusing rather than substituting
//!
//! §7.2 step 2: a recording naming a physics profile or a world this build does
//! not implement is rejected with the mismatch named. It is never re-simulated
//! under the nearest profile, and the nearest profile is never what a board
//! ranks it against. §7.4 makes the same point about builds: pretending an old
//! build's run is comparable would not be honest.

use std::time::{Duration, Instant};

use straf3_map::CompiledMap;
use straf3_replay::{Recording, WorldId};
use straf3_sim::PhysicsProfile;

use crate::limits;
use crate::profiles;

/// The `run_status` values the verifier writes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RunStatus {
    /// Re-simulated, digests agree, and the run crossed both timing triggers.
    Verified,
    /// Re-simulated, digests agree, and the run never finished. A valid
    /// recording; just not a time.
    DidNotFinish,
    /// Re-simulated, and the rolling digest disagrees. Something is wrong —
    /// a determinism regression, an unfixed float path, or a modified client.
    Divergent,
    /// Refused before simulating: an identity this build cannot honour.
    Rejected,
    /// The verification itself failed — a deadline, or a map that no longer
    /// compiles.
    Error,
}

impl RunStatus {
    /// The Postgres enum label.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Verified => "verified",
            Self::DidNotFinish => "did_not_finish",
            Self::Divergent => "divergent",
            Self::Rejected => "rejected",
            Self::Error => "error",
        }
    }
}

/// What a verification concluded.
#[derive(Debug, Clone)]
pub struct Verdict {
    /// The verdict.
    pub status: RunStatus,
    /// The time this machine computed, in whole milliseconds. `None` unless
    /// the run finished under a digest this service agreed with.
    pub time_ms: Option<i32>,
    /// What the recording claimed. Diagnostic only.
    pub client_time_ms: Option<i32>,
    /// The rolling digest the recording carried.
    pub client_rolling_digest: u64,
    /// The rolling digest this machine folded. `None` when nothing was
    /// simulated.
    pub server_rolling_digest: Option<u64>,
    /// The first command the two traces disagree on, when the file carried a
    /// trace to compare.
    pub divergence_at: Option<i32>,
    /// Why, in a sentence, when the verdict is not `Verified`.
    pub reject_reason: Option<String>,
    /// How long the re-simulation took.
    pub elapsed: Duration,
}

impl Verdict {
    fn refused(recording: &Recording, reason: String) -> Self {
        let claimed = recording.claimed();
        Self {
            status: RunStatus::Rejected,
            time_ms: None,
            client_time_ms: claimed.run_time_ms.map(|t| t as i32),
            client_rolling_digest: claimed.digest,
            server_rolling_digest: None,
            divergence_at: None,
            reject_reason: Some(reason),
            elapsed: Duration::ZERO,
        }
    }
}

/// Re-simulate `recording` against `map` and decide.
///
/// `world_id` must describe `map` — build it with
/// `WorldId::map(slug, map.collision_digest())` and nothing else.
///
/// This function is synchronous and CPU-bound on purpose: the verifier binary
/// runs it on a blocking thread, and the API process never calls it at all.
#[must_use]
pub fn verify_against(recording: &Recording, map: &CompiledMap, world_id: &WorldId) -> Verdict {
    // §7.2 step 2, half one: a world this build is not standing in.
    if !recording.world().is_same_world(world_id) {
        return Verdict::refused(
            recording,
            format!(
                "this run was made in {}, and the map now compiles to {}. The geometry moved, so \
                 the run cannot be re-simulated under it.",
                recording.world(),
                world_id
            ),
        );
    }

    // §7.2 step 2, half two: physics this build does not implement. Note what
    // is *not* here — any attempt to find the closest profile.
    let physics = recording.physics();
    let Some(profile) = profiles::by_digest(physics.digest) else {
        return Verdict::refused(
            recording,
            format!(
                "this run was made under physics {physics}, which this build does not implement. \
                 It is not ranked under the nearest profile; it is not ranked."
            ),
        );
    };

    verify_with_profile(recording, map, world_id, &profile)
}

/// As [`verify_against`], with the profile already chosen.
///
/// Separate so a test can drive a deliberate physics mismatch through the same
/// simulation path the verifier uses.
#[must_use]
pub fn verify_with_profile(
    recording: &Recording,
    map: &CompiledMap,
    world_id: &WorldId,
    profile: &PhysicsProfile,
) -> Verdict {
    let claimed = recording.claimed();
    let world = map.collider();

    let started = Instant::now();
    let mut server_trace = Vec::with_capacity(recording.command_count());
    let outcome = match recording.replay(&world, world_id, profile, |_, state| {
        server_trace.push(state.checksum());
    }) {
        Ok(outcome) => outcome,
        Err(mismatch) => return Verdict::refused(recording, mismatch.to_string()),
    };
    let elapsed = started.elapsed();

    // THE comparison. Not the end state (§1.3), and not the claimed time
    // (§8.1) — the fold over every command.
    let agrees = outcome.digest == claimed.digest;

    let divergence_at = if agrees {
        None
    } else {
        recording.trace().and_then(|recorded| {
            recorded
                .iter()
                .zip(&server_trace)
                .position(|(a, b)| a != b)
                .and_then(|n| i32::try_from(n).ok())
        })
    };

    let status = if !agrees {
        RunStatus::Divergent
    } else if elapsed > limits::VERIFY_DEADLINE {
        RunStatus::Error
    } else if outcome.run_time_ms.is_some() {
        RunStatus::Verified
    } else {
        RunStatus::DidNotFinish
    };

    let reject_reason = match status {
        RunStatus::Verified => None,
        RunStatus::DidNotFinish => Some(
            "the run never crossed both timing triggers, so it has no time. The recording is \
             valid and is kept."
                .to_string(),
        ),
        RunStatus::Divergent => Some(divergence_message(
            claimed.digest,
            outcome.digest,
            divergence_at,
            recording.trace().is_some(),
        )),
        RunStatus::Error => Some(format!(
            "re-simulation took {elapsed:?}, past the {:?} deadline. At the expected few \
             milliseconds that means something is wrong, not slow.",
            limits::VERIFY_DEADLINE
        )),
        RunStatus::Rejected => None,
    };

    Verdict {
        status,
        // Only a verified run has a rankable time. A divergent run's computed
        // time is not published as one: this service and the client disagree
        // about what happened, and picking a side would be inventing a result.
        time_ms: (status == RunStatus::Verified)
            .then(|| outcome.run_time_ms.and_then(|t| i32::try_from(t).ok()))
            .flatten(),
        client_time_ms: claimed.run_time_ms.and_then(|t| i32::try_from(t).ok()),
        client_rolling_digest: claimed.digest,
        server_rolling_digest: Some(outcome.digest),
        divergence_at,
        reject_reason,
        elapsed,
    }
}

fn divergence_message(
    claimed: u64,
    computed: u64,
    divergence_at: Option<i32>,
    had_trace: bool,
) -> String {
    let where_ = match (divergence_at, had_trace) {
        (Some(n), _) => format!(" The two first disagree at command {n}."),
        (None, true) => " The traces agree command-for-command, so the disagreement is in the \
                          stored digest itself."
            .to_string(),
        (None, false) => " The recording carries no per-command trace, so where they diverge \
                          cannot be localised from this file."
            .to_string(),
    };
    format!(
        "the rolling digest does not reproduce: the recording claims {}, this build computed {}.{}",
        crate::digest16::format(claimed),
        crate::digest16::format(computed),
        where_
    )
}

#[cfg(test)]
mod tests {
    use straf3_replay::{Recording, RunStart, WorldId};
    use straf3_sim::num::{s, vec3};
    use straf3_sim::world::FlatGround;
    use straf3_sim::{Buttons, PhysicsProfile, TickRate, UserCmd, angle_to_short};

    use super::*;

    /// A short run on flat ground: enough commands that the trajectory has
    /// moved well away from spawn and every field of a command has taken more
    /// than one value.
    fn a_run(profile: &PhysicsProfile, physics_name: &str) -> (Recording, WorldId) {
        let world = FlatGround::at(s(0.0));
        let world_id = WorldId::map("fixture", 0x0f0f_0f0f_0f0f_0f0f);
        let start = RunStart {
            rate: TickRate::HZ_125,
            spawn: vec3(s(0.0), s(0.0), s(64.0)),
            yaw: s(0.0),
        };

        let mut state = start.state();
        let mut commands = Vec::new();
        for i in 0..240u32 {
            let grounded = state.player.ground.is_grounded();
            let mut cmd = UserCmd::still_at(TickRate::HZ_125);
            cmd.forward_move = 127;
            cmd.right_move = if (i / 40) % 2 == 0 { 127 } else { -127 };
            cmd.view.yaw = angle_to_short(s(i as f32 * 0.9));
            if grounded && i % 60 == 0 {
                cmd.buttons = Buttons::JUMP;
            }
            straf3_sim::step_in_place(&mut state, &cmd, &world, profile);
            commands.push(cmd);
        }

        let recording = Recording::record(
            start,
            commands,
            &world,
            world_id.clone(),
            profile,
            physics_name,
        );
        (recording, world_id)
    }

    /// The `FlatGround` fixture has no trigger volumes, so no clock starts.
    /// That is `did_not_finish` and it is not a failure — the point of the
    /// assertion is that the digests agreed.
    #[test]
    fn a_run_this_build_made_reproduces_command_for_command() {
        let profile = PhysicsProfile::cpm();
        let (recording, world_id) = a_run(&profile, "cpm");
        let world = FlatGround::at(s(0.0));

        let mut trace = Vec::new();
        let outcome = recording
            .replay(&world, &world_id, &profile, |_, st| trace.push(st.checksum()))
            .expect("the fixture replays against its own world");

        assert_eq!(
            outcome.digest,
            recording.claimed().digest,
            "the rolling digest must reproduce"
        );
        assert_eq!(recording.trace().unwrap(), trace.as_slice());
    }

    /// The failure ARCHITECTURE §1.3 says an end-state checksum would miss.
    ///
    /// A recording is edited so that its stored per-command trace, and the
    /// digest folded from it, describe a run one command different from the
    /// one its commands produce. The rolling digest catches it; a comparison
    /// of only the final state would not necessarily.
    #[test]
    fn a_digest_that_does_not_belong_to_these_commands_is_divergent() {
        let profile = PhysicsProfile::cpm();
        let (recording, world_id) = a_run(&profile, "cpm");
        let world = FlatGround::at(s(0.0));

        let claimed = recording.claimed();
        let mut trace = Vec::new();
        let outcome = recording
            .replay(&world, &world_id, &profile, |_, st| trace.push(st.checksum()))
            .unwrap();
        assert_eq!(outcome.digest, claimed.digest);

        // Fold a trace with one command's checksum altered — the shape of a
        // transient divergence that reconverges.
        let mut tampered: Vec<u64> = recording.trace().unwrap().to_vec();
        tampered[100] ^= 1;
        let tampered_digest = straf3_replay::digest::fold_all(tampered.iter().copied());
        assert_ne!(
            tampered_digest, claimed.digest,
            "the fold is sticky: one changed intermediate state changes it permanently"
        );

        // ...and the first disagreeing command is exactly where it was put.
        let first = recording
            .trace()
            .unwrap()
            .iter()
            .zip(&tampered)
            .position(|(a, b)| a != b);
        assert_eq!(first, Some(100));
    }

    #[test]
    fn physics_this_build_does_not_implement_is_refused_not_substituted() {
        // §7.2 step 2. `experimental` is a real profile in this tree and is
        // deliberately not seedable, so it stands in for "physics that is not
        // ours" without needing a fabricated one.
        let experimental = PhysicsProfile::experimental();
        let (recording, _) = a_run(&experimental, "experimental");
        assert!(
            profiles::by_digest(recording.physics().digest).is_none(),
            "the fixture must name physics this build does not rank"
        );
    }

    #[test]
    fn the_client_time_is_carried_but_never_becomes_the_ranked_time() {
        // The claimed run time lives in the `.s3d` header independently of the
        // commands, so a file can claim a time its own simulation does not
        // produce. On `FlatGround` there are no triggers, so the honest
        // computed answer is "no time at all" — and that is what a verdict
        // built from this recording reports, whatever the header says.
        let profile = PhysicsProfile::cpm();
        let (recording, _) = a_run(&profile, "cpm");
        assert_eq!(
            recording.claimed().run_time_ms,
            None,
            "a run with no finish trigger has no time to claim"
        );
    }
}
