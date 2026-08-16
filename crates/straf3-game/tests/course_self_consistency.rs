//! **Criterion 4 on the geometry that matters**: the course half.
//!
//! # The gap this closes
//!
//! Criterion 4 names `straf3-headless` as its reference, and headless's fixture
//! format can spell `empty` and `flat <z>` and nothing else. The playable build
//! runs the compiled course with ramps (`assets/maps/coil.map`), and
//! `crates/straf3-sim` is off-limits this wave — so the named reference
//! *structurally cannot* run the geometry the game is actually played on.
//!
//! That is not a small omission. The spec's own words: ramps are *"where CPM
//! and VQ3 diverge most, and they are the discrete branches the 1-ULP finding
//! warns about."* A criterion-4 proof that only ever ran on an infinite flat
//! plane would be silent about exactly the geometry most likely to break.
//!
//! So criterion 4's evidence splits in two, as approved by the coordinator:
//!
//! 1. `replay_equivalence.rs` — flat ground, three-way: the windowed build vs
//!    `straf3-headless` vs the in-process simulation, per tick. That pins the
//!    *absolute* answer against the reference the spec names.
//! 2. **This file** — the course, self-consistency: the same recorded input
//!    replayed through the windowed build under wildly different frame
//!    schedules, required to produce byte-identical per-tick digests.
//!
//! Neither half alone is criterion 4. Together they say: the simulation's
//! answer is correct against the reference where the reference can reach, and
//! frame pacing perturbs nothing where it cannot.
//!
//! # Why frame schedules are the right lever here
//!
//! With no external reference to compare against, the question becomes "does
//! anything above the seam leak into the simulation?" Frame pacing is the only
//! thing that varies between two runs of the same recorded input in the same
//! world — so if the course's digests are invariant under a schedule of 1 ms
//! frames, 200 ms frames and zero-length frames, nothing about *when* frames
//! happened reached the physics. That is the seam claim, stated in the only
//! terms available on this geometry.

#[path = "../../straf3-platform/tests/support/mod.rs"]
mod support;

use std::path::PathBuf;
use std::process::Command;

use support::{Run, assert_digests_match, map_runs, parse_trace_csv};

fn straf3() -> PathBuf {
    PathBuf::from(env!("CARGO_BIN_EXE_straf3"))
}

/// The committed course, as an absolute path.
///
/// `--map` is resolved relative to the binary's working directory, and a test
/// harness's working directory is not something to depend on. This is the same
/// file `straf3-render`'s `course_is_playable` embeds.
fn course_map() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../assets/maps/coil.map")
}

fn replay_digests(run: &Run, frame_ms: &[u64]) -> Vec<u64> {
    let bin = straf3();
    let mut cmd = Command::new(&bin);
    cmd.arg("--map")
        .arg(course_map())
        .arg("--replay")
        .arg(run.fixture_path())
        .arg("--trace")
        .arg("--csv");

    if !frame_ms.is_empty() {
        cmd.arg("--frame-ms").arg(
            frame_ms
                .iter()
                .map(u64::to_string)
                .collect::<Vec<_>>()
                .join(","),
        );
    }

    let out = cmd
        .output()
        .unwrap_or_else(|e| panic!("could not execute {}: {e}", bin.display()));
    assert!(
        out.status.success(),
        "{} --replay {} exited {}\nstderr:\n{}",
        bin.display(),
        run.fixture_path().display(),
        out.status,
        String::from_utf8_lossy(&out.stderr),
    );
    parse_trace_csv(&String::from_utf8_lossy(&out.stdout))
}

/// The course replays identically under every frame schedule.
///
/// Per tick, never end-state — on these very fixtures a single command's
/// `right_move` nudged by one unit changes 88 of 322 ticks and then
/// re-converges to the *identical* final checksum. An end-state check here
/// would certify a build whose ramp behaviour had genuinely diverged.
#[test]
fn the_course_replays_identically_under_every_frame_schedule() {
    let schedules: [&[u64]; 6] = [
        &[1],
        &[7],
        &[9],
        &[13, 17, 19],
        &[16, 16, 17],
        &[1, 0, 200, 3, 37, 0, 0, 61, 8, 5, 17, 16],
    ];

    for run in map_runs() {
        let reference = replay_digests(&run, &[]);

        for schedule in schedules {
            let paced = replay_digests(&run, schedule);
            assert_digests_match(
                &format!("{}/one frame per tick", run.name),
                &reference,
                &format!("{}/--frame-ms {schedule:?}", run.name),
                &paced,
            );
        }
    }
}

/// Replaying the same course input twice must be bit-identical.
///
/// The weakest possible property, and worth pinning precisely because it is
/// weak: if it ever fails, nothing else in this file means anything, and the
/// cause is above the seam — an uninitialised buffer, a hash iteration order,
/// a clock that leaked into the physics.
#[test]
fn replaying_the_same_course_input_twice_is_bit_identical() {
    for run in map_runs() {
        let first = replay_digests(&run, &[]);
        let second = replay_digests(&run, &[]);
        assert_digests_match(
            &format!("{}/first replay", run.name),
            &first,
            &format!("{}/second replay", run.name),
            &second,
        );
    }
}

/// The course run must actually *meet the geometry*.
///
/// Without this the whole file is satisfiable by a run that falls through an
/// empty void: perfectly reproducible, perfectly invariant under frame
/// schedules, and evidence of nothing at all. Landing on a surface is the
/// cheapest observable proof that the course's collision world was consulted,
/// and it is asserted here rather than assumed because the fixture's spawn
/// point is chosen by hand and the map it is aimed at can be edited.
#[test]
fn the_course_run_actually_touches_the_courses_geometry() {
    for run in map_runs() {
        let bin = straf3();
        let out = Command::new(&bin)
            .arg("--map")
            .arg(course_map())
            .arg("--replay")
            .arg(run.fixture_path())
            .arg("--trace")
            .arg("--csv")
            .output()
            .unwrap_or_else(|e| panic!("could not execute {}: {e}", bin.display()));
        assert!(out.status.success(), "--replay failed for {}", run.name);

        let stdout = String::from_utf8_lossy(&out.stdout);
        let rows: Vec<&str> = stdout
            .lines()
            .skip(1)
            .filter(|l| !l.trim().is_empty())
            .collect();

        // Columns of the trace CSV: tick,time_ms,x,y,z,vx,vy,vz,speed,grounded,checksum
        let field =
            |line: &str, n: usize| -> Option<f32> { line.split(',').nth(n)?.trim().parse().ok() };
        let grounded_ticks = rows
            .iter()
            .filter(|line| {
                line.split(',')
                    .nth(9)
                    .is_some_and(|field| field.trim() == "1")
            })
            .count();

        assert!(
            grounded_ticks > 0,
            "{}: the run was never grounded across the whole replay — it never \
             touched the course's geometry, so this fixture proves nothing about \
             ramps. Retune its spawn point and heading in `map_runs()` against \
             the course as built.",
            run.name,
        );

        // Grounded is not enough on its own: the start room's floor is flat, and
        // a fixture that never left it would satisfy the check above while
        // proving nothing about ramps — which is the only reason this half of
        // criterion 4 exists. The ramp wave crests 112 units above the corridor
        // floor, so a run that climbed is a run that was on it.
        let climbed = rows
            .iter()
            .filter_map(|l| field(l, 4))
            .fold(f32::MIN, f32::max);
        assert!(
            climbed > 100.0,
            "{}: the run never got above z={climbed:.1}, so it stayed on flat \
             floor and never reached the ramp wave. Retune its spawn point and \
             heading in `map_runs()` against the course as built.",
            run.name,
        );
    }
}
