//! **Criterion 4, end to end** — corner C (spec rev 6 §S).
//!
//! *"A recorded input sequence replays to the same checksum through the
//! windowed build as through `straf3-headless` — the renderer changes nothing
//! below the seam."*
//!
//! `crates/straf3-platform/tests/seam_oracle.rs` established that
//! `straf3-headless` and the in-process simulation agree per tick (corners A
//! and B). This file adds the corner the criterion is actually about: the
//! **shipped `straf3` binary**.
//!
//! # Why this drives the binary rather than calling the library
//!
//! "Through the windowed build" means the artefact the operator runs, not a
//! library function that resembles it. Calling `straf3_game::replay::replay`
//! from a test would prove that *a* function in that crate is faithful; it
//! would not prove that the binary's argument handling, its world selection or
//! its output path are. So this file uses `env!("CARGO_BIN_EXE_straf3")` and
//! compares process output.
//!
//! It has a second benefit worth stating: this file does not name a single
//! `straf3_game` item, so it cannot rot when that crate's internals move.
//!
//! # The parsers are independent, and that is the point
//!
//! Per the coordinator's ruling, `straf3-game` implements its own fixture
//! parser rather than sharing `bin/headless.rs`'s — `crates/straf3-sim` is
//! off-limits this wave. Two independent parsers reading the same file is a
//! genuine divergence risk, and it is precisely what these tests detect: the
//! same fixture through both binaries, per-tick digests diffed. A parser
//! disagreement shows up here as a mismatch at the tick where the misread
//! command first bites.
//!
//! # No golden checksums
//!
//! Nothing here asserts a literal. Spec rev 6 Q1's Cody–Waite trig replacement
//! changes every checksum in the repository; a comparison survives it, a
//! constant does not.

// The oracle lives with the platform tests. Included by path rather than
// copied so both crates compare against one mutation-proven comparator and one
// set of fixtures — a second copy would be a second thing to drift.
#[path = "../../straf3-platform/tests/support/mod.rs"]
mod support;

use std::path::PathBuf;
use std::process::Command;

use support::{Run, assert_digests_match, parse_trace_csv, runs};

/// The `straf3` binary as Cargo built it for this test.
fn straf3() -> PathBuf {
    PathBuf::from(env!("CARGO_BIN_EXE_straf3"))
}

/// Run `straf3 --replay` over a fixture and return its per-tick digests.
///
/// `frame_ms` empty means "no `--frame-ms` flag", which is the default one
/// frame per tick.
fn replay_digests(run: &Run, frame_ms: &[u64]) -> Vec<u64> {
    let bin = straf3();
    let mut cmd = Command::new(&bin);
    cmd.arg("--replay")
        .arg(run.fixture_path())
        .arg("--trace")
        .arg("--csv");

    if !frame_ms.is_empty() {
        let schedule = frame_ms
            .iter()
            .map(u64::to_string)
            .collect::<Vec<_>>()
            .join(",");
        cmd.arg("--frame-ms").arg(schedule);
    }

    let out = cmd
        .output()
        .unwrap_or_else(|e| panic!("could not execute {}: {e}", bin.display()));

    assert!(
        out.status.success(),
        "{} --replay {} --trace --csv{} exited {}\nstderr:\n{}",
        bin.display(),
        run.fixture_path().display(),
        if frame_ms.is_empty() {
            String::new()
        } else {
            format!(" --frame-ms {frame_ms:?}")
        },
        out.status,
        String::from_utf8_lossy(&out.stderr),
    );

    parse_trace_csv(&String::from_utf8_lossy(&out.stdout))
}

/// **Criterion 4.** Every fixture replays through the windowed build to the
/// identical per-tick checksum stream that `straf3-headless` produces.
///
/// Compared against *both* other corners, because they are different claims:
/// agreeing with `straf3-headless` is the criterion as written, and agreeing
/// with the in-process simulation localises a failure to the binary rather
/// than to the fixture.
///
/// Per tick, never end-state. A single command's `right_move` nudged by one
/// unit on these very fixtures changes 88 of 322 ticks and then re-converges
/// to the *identical* final checksum — measured, see
/// `a_mid_run_divergence_can_hide_behind_a_matching_final_checksum`. An
/// end-state check here would certify a build that had actually diverged.
#[test]
fn windowed_build_replays_identically_to_headless_at_every_tick() {
    for run in runs() {
        let headless = run.headless_digests();
        let in_process = run.reference_digests();
        let windowed = replay_digests(&run, &[]);

        assert_digests_match(
            &format!("{}/straf3-headless", run.name),
            &headless,
            &format!("{}/straf3 --replay", run.name),
            &windowed,
        );
        assert_digests_match(
            &format!("{}/in-process", run.name),
            &in_process,
            &format!("{}/straf3 --replay", run.name),
            &windowed,
        );
    }
}

/// **Criterion 5, end to end.** The fixed command cadence survives a variable
/// frame rate.
///
/// The same fixture replayed under hostile frame schedules must produce the
/// identical per-tick digest stream as the regular one — and as
/// `straf3-headless`, which has no frame loop at all. If frame pacing were
/// coupled to simulation stepping in any way, a schedule of 1 ms and 200 ms
/// frames would change the commands the simulation sees, and every tick after
/// that point would differ.
///
/// The schedules are chosen to break specific implementations: durations below
/// the command period, durations coprime to it, a stall long enough to demand
/// catch-up, and zero-length frames.
#[test]
fn hostile_frame_schedules_do_not_change_the_command_stream() {
    let schedules: [&[u64]; 6] = [
        &[1],                                          // 1000 fps: 7 of 8 frames emit nothing
        &[7],                                          // just under one command
        &[9],                                          // just over one command
        &[13, 17, 19],                                 // coprime to the 8 ms period
        &[16, 16, 17],                                 // what a 60 Hz display really delivers
        &[1, 0, 200, 3, 37, 0, 0, 61, 8, 5, 17, 16],   // stalls and zero-length frames
    ];

    for run in runs() {
        let reference = run.headless_digests();

        for schedule in schedules {
            let paced = replay_digests(&run, schedule);
            assert_digests_match(
                &format!("{}/straf3-headless (no frame loop)", run.name),
                &reference,
                &format!("{}/straf3 --replay --frame-ms {schedule:?}", run.name),
                &paced,
            );
        }
    }
}

/// The replay path must not be a no-op that agrees with everything.
///
/// If `--replay` ignored the fixture and emitted a fixed stream, or emitted the
/// spawn state and stopped, the comparisons above could still pass for two
/// fixtures that happened to be short. This pins that the windowed build
/// produces a distinct, evolving stream per fixture — the same property
/// `every_run_produces_a_distinct_and_evolving_digest_stream` pins for the
/// in-process reference, asserted here against the binary.
#[test]
fn the_windowed_build_produces_a_distinct_evolving_stream_per_fixture() {
    let mut finals = Vec::new();

    for run in runs() {
        let digests = replay_digests(&run, &[]);

        assert_eq!(
            digests.len(),
            run.cmds.len() + 1,
            "{}: --replay emitted {} ticks for a {}-command fixture (expected \
             the spawn state plus one per command)",
            run.name,
            digests.len(),
            run.cmds.len(),
        );
        assert_ne!(
            digests.first(),
            digests.last(),
            "{}: --replay ended in the state it started in",
            run.name,
        );
        finals.push((run.name, *digests.last().expect("non-empty")));
    }

    for (i, (name_a, a)) in finals.iter().enumerate() {
        for (name_b, b) in &finals[i + 1..] {
            assert_ne!(
                a, b,
                "{name_a} and {name_b} replay to the same final state through \
                 the windowed build — they are not independent evidence",
            );
        }
    }
}

/// A frame schedule of nothing but zeros must be refused, not hung on.
///
/// Platform documents this refusal. It is worth a test because the failure mode
/// it prevents is an infinite loop in CI — a test suite that never returns is
/// worse than one that fails, and "it would obviously hang" is exactly the kind
/// of obviousness that stops being true after a refactor.
#[test]
fn an_all_zero_frame_schedule_is_refused_rather_than_hanging() {
    let run = support::run_named("still_on_ground");
    let out = Command::new(straf3())
        .arg("--replay")
        .arg(run.fixture_path())
        .arg("--trace")
        .arg("--csv")
        .arg("--frame-ms")
        .arg("0,0,0")
        .output()
        .expect("could not execute straf3");

    assert!(
        !out.status.success(),
        "--frame-ms 0,0,0 was accepted. Either it hung until the test harness \
         killed it, or it silently advanced time it was never given.",
    );
}

/// `--trace`, `--csv` and `--frame-ms` must be errors without `--replay`.
///
/// Otherwise someone measures a frame schedule, sees a window open, and
/// believes the numbers. Platform documents this refusal; this is the
/// independent check of it.
#[test]
fn pacing_flags_are_rejected_without_replay() {
    for args in [
        vec!["--trace"],
        vec!["--csv"],
        vec!["--frame-ms", "8,8,8"],
    ] {
        let out = Command::new(straf3())
            .args(&args)
            .output()
            .expect("could not execute straf3");
        assert!(
            !out.status.success(),
            "straf3 {args:?} without --replay exited successfully; a caller \
             could believe it measured a frame schedule when it did not",
        );
    }
}
