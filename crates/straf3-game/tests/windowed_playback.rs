//! **Windowed playback on the geometry that matters, and the loop that hangs
//! off it** — coordinator decisions D-B1 and D-B2.
//!
//! `--play` opens a window and drives the session from a recorded command
//! stream. Two things have to be true of it, and neither is checkable by
//! looking at the window:
//!
//! 1. **It is a replay.** The state the played session reaches must be the
//!    state `straf3 --replay` reaches from the same file — at every tick, under
//!    any frame schedule. If those can differ, the run on screen is not the run
//!    in the file, and everything measured downstream of it is measuring
//!    something nobody recorded.
//! 2. **A played run that finishes saves a personal best that is really that
//!    run.** `Game::apply` never fed the recorder, so before this wave a played
//!    run would have crossed the finish line and written nothing. The `.s3d`
//!    the run produces has to re-simulate to the same time the source file
//!    does, or the saved record is a claim about a run that did not happen.
//!
//! # Why this is a separate test binary
//!
//! `scene::install` is a process-lifetime `OnceLock`: a map installed by one
//! test is installed for every test in that process. `straf3-game`'s unit tests
//! deliberately assert the *no map* behaviour, so installing `coil` alongside
//! them would make their result depend on thread scheduling. A `tests/` file is
//! its own process, and every test in this one wants the course loaded.
//!
//! # Why this runs the library rather than the binary
//!
//! `--play` opens a window, so the shipped binary cannot be driven by a test on
//! a machine with no display. What this file drives instead is
//! [`straf3_game::App::simulate_frame`] — the *same* function the winit callback
//! calls, with the drawing left off. There is no second stepping path.

use std::sync::OnceLock;

use straf3_game::app::{App, Options, Playback};
use straf3_game::replay::{Fixture, ReplayOptions, parse, replay};

/// The bot's run of the course: 125 Hz, cpm, `world map`, and it finishes.
///
/// `include_str!` rather than a path read at runtime, so a test cannot pass
/// against a file that is not actually committed.
const COIL_RUN: &str = include_str!("../../../probes/coil-course/results/coil-run.txt");
const COIL_MAP: &str = include_str!("../../../assets/maps/coil.map");

/// What that run comes to, as `probes/coil-course` measured it. Asserted rather
/// than merely reported: this is the number the whole personal-best
/// demonstration is about, and a silent change to it would make every claim
/// downstream quietly wrong.
const COIL_RUN_MS: u32 = 5_096;

/// Install the course once for this process.
fn course() -> &'static str {
    static INSTALLED: OnceLock<()> = OnceLock::new();
    INSTALLED.get_or_init(|| {
        straf3_game::scene::install("coil", COIL_MAP).expect("assets/maps/coil.map must compile");
    });
    "coil"
}

fn fixture() -> Fixture {
    parse(COIL_RUN).expect("the bot run must parse")
}

fn options(fixture: &Fixture, pb_dir: Option<String>) -> Options {
    Options {
        world: fixture.world,
        profile: fixture.profile,
        profile_name: fixture.profile_name.clone(),
        rate: fixture.rate,
        pb_dir,
        playback: Some(Playback {
            cmds: fixture.cmds.clone(),
            spawn: fixture.spawn,
            yaw: fixture.yaw,
            source: "probes/coil-course/results/coil-run.txt".to_owned(),
        }),
        ..Options::default()
    }
}

/// Drive a session to the end of its stream on `schedule`, cycled.
fn play_out(app: &mut App, schedule: &[u64]) {
    let mut elapsed_ms = 0u64;
    for frame in 0..1_000_000 {
        let delta = schedule[frame % schedule.len()];
        elapsed_ms += delta;
        app.simulate_frame(delta, elapsed_ms);
        if app.game().playback_remaining() == Some(0) {
            return;
        }
    }
    panic!("the stream never ran out on schedule {schedule:?}");
}

/// A directory of this test's own, so nothing here can read or write the
/// repository's `runs/`.
fn pb_dir(tag: &str) -> String {
    let dir = std::env::temp_dir().join(format!("straf3-play-{}-{tag}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    dir.to_string_lossy().into_owned()
}

/// **The acceptance test for `--play` on the course.**
///
/// Every tick, under six frame schedules, against the headless replay of the
/// same file. Per-tick rather than end-state because a divergence can
/// reconverge — the repository has a measured case where the final checksum
/// matched while 29 of 1200 intermediate ones did not.
#[test]
fn a_played_course_run_matches_the_headless_replay_at_every_tick() {
    course();
    let fixture = fixture();
    let reference = replay(&fixture, &ReplayOptions::default()).unwrap();
    assert_eq!(reference.len(), fixture.cmds.len() + 1);

    for schedule in [
        &[8u64][..],
        &[1, 97, 3, 250, 8][..],
        &[0, 0, 0, 33][..],
        &[250][..],
        &[1][..],
        &[16, 17][..],
    ] {
        let mut app = App::new(options(&fixture, None));
        assert_eq!(
            app.game().state().checksum(),
            reference[0].checksum(),
            "schedule {schedule:?}: the session did not start where the file did"
        );

        let mut elapsed_ms = 0u64;
        let mut applied;
        for frame in 0..1_000_000 {
            let delta = schedule[frame % schedule.len()];
            elapsed_ms += delta;
            app.simulate_frame(delta, elapsed_ms);
            applied = app.game().state().tick as usize;
            assert_eq!(
                app.game().state().checksum(),
                reference[applied].checksum(),
                "schedule {schedule:?}: diverged after {applied} commands"
            );
            if app.game().playback_remaining() == Some(0) {
                break;
            }
        }

        assert_eq!(
            app.game().state().tick as usize,
            fixture.cmds.len(),
            "schedule {schedule:?}: the stream did not run to the end"
        );
        assert_eq!(
            app.game().state().checksum(),
            reference.last().unwrap().checksum(),
            "schedule {schedule:?}"
        );
    }
}

/// The run actually finishes, and finishes at the time the probe measured.
///
/// Without this the rest of the file could pass on a session that never crossed
/// the finish line — every "no personal best was saved" assertion would hold
/// for the wrong reason.
#[test]
fn the_bot_run_crosses_the_finish_line_in_the_time_the_probe_measured() {
    course();
    let mut app = App::new(options(&fixture(), None));
    play_out(&mut app, &[8]);

    let state = app.game().state();
    match state.run {
        straf3_sim::RunState::Finished {
            started_at_ms,
            finished_at_ms,
        } => assert_eq!(finished_at_ms - started_at_ms, COIL_RUN_MS),
        other => panic!("the bot run did not finish: {other:?}"),
    }
}

/// **Criterion 6, in process**: a played run that finishes saves a personal
/// best, a second session loads it, races it, and has a split to show.
///
/// The GPU demonstration is the same sequence with a window and a screenshot on
/// the end of it. Doing it here first means a failure on the real hardware is a
/// *hardware or windowing* failure, and not this.
#[test]
fn a_played_run_saves_a_personal_best_that_a_second_session_races() {
    course();
    let fixture = fixture();
    let dir = pb_dir("pb");
    let path = format!("{dir}/coil.cpm.s3d");

    // First session: nothing on disk, so this run sets the record.
    let mut first = App::new(options(&fixture, Some(dir.clone())));
    assert!(first.ghost().is_none(), "nothing should be raced yet");
    play_out(&mut first, &[1, 97, 3, 250, 8]);

    let bytes = std::fs::read(&path).expect("a finished run must write its personal best");
    assert!(!bytes.is_empty());

    // The saved file is really the run that was played: it re-simulates, on
    // this build's own geometry, to the same time.
    let saved = straf3_replay::Recording::from_bytes(&bytes).expect("what was written reads back");
    assert_eq!(saved.claimed().run_time_ms, Some(COIL_RUN_MS));
    assert_eq!(saved.physics().name, "cpm");
    let world_id = straf3_game::WorldChoice::Map
        .world_id()
        .expect("the course is installed");
    assert_eq!(saved.world(), &world_id);
    let verified = saved
        .verify(
            &straf3_game::WorldChoice::Map.world(),
            &world_id,
            &fixture.profile,
        )
        .expect("the saved recording must re-simulate to what it claims");
    assert_eq!(verified.run_time_ms, Some(COIL_RUN_MS));

    // Second session: the same binary, the same file, a personal best on disk.
    let mut second = App::new(options(&fixture, Some(dir.clone())));
    let ghost = second
        .ghost()
        .expect("the saved personal best must load and re-simulate into a ghost");
    assert_eq!(ghost.run_time_ms(), COIL_RUN_MS);
    assert!(ghost.sample_count() > 0);
    assert!(second.split_ms().is_none(), "no split before the start line");

    // Race it. The split exists from the moment the start line is crossed —
    // that is the number the overlay draws.
    let mut split_seen = false;
    let mut split_at_finish = None;
    let mut elapsed_ms = 0u64;
    for frame in 0..1_000_000u64 {
        let delta = [1u64, 97, 3, 250, 8][(frame % 5) as usize];
        elapsed_ms += delta;
        second.simulate_frame(delta, elapsed_ms);
        split_seen |= second.split_ms().is_some();
        if split_at_finish.is_none()
            && matches!(
                second.game().state().run,
                straf3_sim::RunState::Finished { .. }
            )
        {
            split_at_finish = second.split_ms();
        }
        if second.game().playback_remaining() == Some(0) {
            break;
        }
    }
    assert!(split_seen, "the ghost was loaded but never raced");

    // Racing itself, the split at the line is zero. A tie must read as a tie:
    // a systematic offset here would be an untrue number on screen for every
    // run, and it is the number criterion 6's evidence is a photograph of.
    assert_eq!(split_at_finish, Some(0));
    // And it stays the result: the player carries on past the line for the rest
    // of the file, but a finished run's split does not drift with them.
    assert_eq!(second.split_ms(), Some(0));

    // And the record stands: an equal time does not rewrite the file.
    let after = std::fs::read(&path).unwrap();
    assert_eq!(after, bytes, "an equal run must not replace the record");

    let _ = std::fs::remove_dir_all(&dir);
}

/// **A replay that ran in the wrong world must not exit 0.**
///
/// `WorldChoice::or_fallback` drops to the flat plane when no map is installed,
/// which is right for an interactive session — a window you can move in beats
/// refusing to start over a missing file. For a replay it is the opposite: the
/// output *is* the claim, and running a `world map` recording against an
/// infinite plane produces a full trace, a checksum, a run time and a zero exit
/// status for a run that happened somewhere else. A determinism check or a
/// criterion-4 diff would compare against a world nobody played in and see
/// nothing wrong.
///
/// Driven as a subprocess because the exit status is the thing under test, and
/// because the map is a process-lifetime singleton — this file's own process has
/// `coil` installed, and a child does not inherit it.
#[test]
fn a_replay_that_cannot_reach_its_own_world_fails_instead_of_answering() {
    let fixture = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../probes/coil-course/results/coil-run.txt");
    let straf3 = std::path::PathBuf::from(env!("CARGO_BIN_EXE_straf3"));

    // `--world flat` stops the default map being installed, so the `world map`
    // in the file has nothing to resolve to. This is the silent-wrong-answer
    // case: before, it printed a complete replay of the flat world and exited 0.
    let refused = std::process::Command::new(&straf3)
        .args([
            "--replay",
            &fixture.to_string_lossy(),
            "--world",
            "flat",
            "--csv",
        ])
        .output()
        .expect("the straf3 binary must run");
    assert!(
        !refused.status.success(),
        "a `world map` recording replayed with no map exited {:?} and printed:\n{}",
        refused.status.code(),
        String::from_utf8_lossy(&refused.stdout),
    );
    let complaint = String::from_utf8_lossy(&refused.stderr);
    assert!(
        complaint.contains("--map"),
        "the refusal must say how to fix it, got: {complaint}"
    );

    // The control: with the map it does resolve, the same file replays and the
    // command succeeds. Without this the assertion above would also pass on a
    // binary that refused everything.
    let map = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../assets/maps/coil.map");
    let accepted = std::process::Command::new(&straf3)
        .args([
            "--replay",
            &fixture.to_string_lossy(),
            "--map",
            &map.to_string_lossy(),
        ])
        .output()
        .expect("the straf3 binary must run");
    assert!(
        accepted.status.success(),
        "the same file with its map must replay: {}",
        String::from_utf8_lossy(&accepted.stderr)
    );
    let report = String::from_utf8_lossy(&accepted.stdout);
    assert!(
        report.contains(&format!("{COIL_RUN_MS} ms")),
        "the replay must report the run time the probe measured:\n{report}"
    );
}

/// **Spec D2**: an experimental record lives in its own namespace and is never
/// ranked against a canonical one.
///
/// Both halves of the rule, because the file name alone is not enough. A
/// `coil.cpm.s3d` *copied* to `coil.experimental.s3d` is in the right place
/// with the wrong provenance, and while `experimental` is still CPM's
/// constants the physics digest cannot tell them apart — so the recording's own
/// profile name has to.
#[test]
fn an_experimental_record_is_never_ranked_against_a_canon_one() {
    course();
    let fixture = fixture();
    let dir = pb_dir("profiles");

    assert_eq!(
        straf3_game::pb::path_in(&dir, "coil", "cpm"),
        format!("{dir}/coil.cpm.s3d")
    );
    assert_eq!(
        straf3_game::pb::path_in(&dir, "coil", "experimental"),
        format!("{dir}/coil.experimental.s3d")
    );

    // Set a canon record, the ordinary way.
    let mut canon = App::new(options(&fixture, Some(dir.clone())));
    play_out(&mut canon, &[8]);
    let canon_bytes = std::fs::read(format!("{dir}/coil.cpm.s3d"))
        .expect("the canon run must have saved its personal best");

    // A canon session races it back. This is the control: it establishes that
    // the refusal below is about the profile and not about the file.
    let again = App::new(options(&fixture, Some(dir.clone())));
    assert!(
        again.ghost().is_some(),
        "a cpm session must race a cpm record"
    );

    // Now put that same record where an experimental session would look for it.
    std::fs::write(format!("{dir}/coil.experimental.s3d"), &canon_bytes).unwrap();
    let mut experimental = options(&fixture, Some(dir.clone()));
    experimental.profile_name = "experimental".to_owned();
    experimental.profile = straf3_game::profile::experimental();
    let session = App::new(experimental);
    assert!(
        session.ghost().is_none(),
        "a cpm time was raced as an experimental one — spec D2 says the two are \
         never compared, and the file name is not the only thing that decides it"
    );
    assert!(session.split_ms().is_none());

    let _ = std::fs::remove_dir_all(&dir);
}

/// The experimental profile is selectable everywhere a canonical one is, and it
/// writes its record under its own name.
#[test]
fn an_experimental_session_is_playable_recordable_and_separately_filed() {
    course();
    let dir = pb_dir("experimental");
    let fixture = fixture();

    let mut options = options(&fixture, Some(dir.clone()));
    options.profile_name = "experimental".to_owned();
    options.profile = straf3_game::profile::experimental();
    let mut app = App::new(options);
    play_out(&mut app, &[1, 97, 3, 250, 8]);

    let saved = std::fs::read(format!("{dir}/coil.experimental.s3d"))
        .expect("an experimental run that finishes must save under its own name");
    let recording = straf3_replay::Recording::from_bytes(&saved).unwrap();
    assert_eq!(recording.physics().name, "experimental");
    assert!(
        !std::path::Path::new(&format!("{dir}/coil.cpm.s3d")).exists(),
        "an experimental run must not touch the canon record"
    );

    // The fixture format understands the name too, so an experimental run can
    // be recorded and played back like any other.
    assert!(straf3_game::replay::parse("rate 125\nprofile experimental\nworld flat 0\n").is_ok());

    let _ = std::fs::remove_dir_all(&dir);
}
