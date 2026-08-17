//! The straf3 executable: a window, a loop, and nothing else.
//!
//! Everything this binary does beyond parsing its arguments is in
//! `straf3_game`, the library next to it — the same split `straf3-headless`
//! has over `straf3-sim`, and for the same reason: the library is testable
//! with no window and no GPU, and this file is not testable at all.
//!
//! Per spec section 2, this binary is tuned on native Windows. Under WSLg it
//! runs on a software adapter (lavapipe) behind a software-composited RDP
//! pipeline, so a window opening here proves the plumbing works and says
//! nothing at all about feel or frame pacing.

// The bin target is built for `wasm32-unknown-unknown` too, even though the
// browser's entry point is `straf3_game::start_web` and a command line means
// nothing there. So `main` exists on both targets and the command-line half
// lives in a module that only exists on one.

#[cfg(target_arch = "wasm32")]
fn main() {}

#[cfg(not(target_arch = "wasm32"))]
fn main() -> std::process::ExitCode {
    native::main()
}

#[cfg(not(target_arch = "wasm32"))]
mod native {

    use std::process::ExitCode;

    use straf3_game::app::Options;
    use straf3_game::replay::ReplayOptions;
    use straf3_game::scene::WorldChoice;
    use straf3_sim::{RunState, TickRate};

    const USAGE: &str = "\
usage: straf3 [options]                     open a window and play
       straf3 --play <file> [options]       open a window and watch a recorded
                                            run drive it
       straf3 --replay <file> [options]     run a recorded file, no window

  --play <file>               drive the windowed, rendering session from a
                              recorded command file instead of from the
                              keyboard. The file's own rate, profile, world,
                              spawn and yaw are used, exactly as --replay does,
                              so the run on screen is the run in the file and
                              lands on the same checksum. Live movement input
                              and R (respawn) are ignored; Esc and closing the
                              window still work. When the stream runs out the
                              final state is held and the window stays open —
                              --exit-after is what ends an unattended session.
  --map <file.map>            Valve 220 map to compile and play (default
                              assets/maps/coil.map)
  --world <map|flat|empty>    geometry to play in (default map). `flat` and
                              `empty` need no map and are the two worlds
                              straf3-headless can reproduce.
  --profile <cpm|vq3|experimental>
                              movement constants (default cpm). `experimental`
                              is straf3's own vocabulary: playable and
                              recordable, but its personal bests are kept under
                              their own name (runs/<map>.experimental.s3d) and
                              are never ranked against a cpm or vq3 time.
  --rate <hz>                 command rate, 1..=1000 (default 125)
  --record <file>             write every command produced to <file>, in
                              straf3-headless's input format
  --pb-dir <dir>              where personal bests are kept (default runs/).
                              The best saved run for this map and profile is
                              raced as a ghost, and a finished run that beats
                              it is written there as <map>.<profile>.s3d
  --no-pb                     neither load a ghost nor save a personal best
  --exit-after <ms>           close the window after <ms> of wall time, so an
                              unattended run can be recorded and replayed
  --pacing-log <file>         write one high-resolution frame delta per frame to
                              <file> as CSV when the session ends. Measurement
                              only: the simulation keeps taking whole-millisecond
                              deltas from exactly the path it uses without this
                              flag. Needs a window, so not with --replay.
  -h, --help                  this

replay options (no window is opened and no GPU adapter is created):
  --replay <file|->           run a recorded command file, `-` for stdin
  --trace                     print one line per tick, not just the final state
  --csv                       print in straf3-headless's CSV form
  --frame-ms <a,b,c,...>      drive the replay on this frame schedule, in whole
                              wall milliseconds, cycled. The output must be
                              identical to the regular schedule's — that
                              equality is what criterion 5 means.

Controls: WASD move, mouse look, Space jump, Ctrl crouch, Shift walk,
          click to capture the mouse, Esc to release, R to respawn.

R starts a new attempt: the clock resets, the ghost goes back to the start
line, and the recording begins again — a respawn is not a command, so a
recording that spanned one could not be replayed.
";

    pub fn main() -> ExitCode {
        // `info` by default: the once-a-second speed readout is what an
        // unattended run leaves in a log file, where the on-screen overlay
        // cannot follow it.
        env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("info"))
            .format_timestamp(None)
            .init();

        let mut options = match parse(std::env::args().skip(1)) {
            Ok(Some(options)) => options,
            Ok(None) => {
                print!("{USAGE}");
                return ExitCode::SUCCESS;
            }
            Err(e) => {
                eprintln!("straf3: {e}\n\n{USAGE}");
                return ExitCode::FAILURE;
            }
        };

        // Before the window opens, and before a single command is simulated.
        if let Some(path) = &options.record_to
            && let Err(e) = check_recording_destination(path)
        {
            eprintln!("straf3: {e}");
            return ExitCode::FAILURE;
        }

        // Before the map is installed, because the fixture decides which world
        // the session runs in and the map is only compiled for `world map`.
        if let Some(path) = options.play_from.clone() {
            if let Err(e) = load_playback(&path, &mut options.session) {
                eprintln!("straf3: {e}");
                return ExitCode::FAILURE;
            }
        }

        // Reading the file is deliberately here and not in `straf3-map`:
        // `compile` takes text, not a path, because that is what lets the
        // browser fetch a `.map` over HTTP and compile it through the identical
        // code path. This is the only place in the native build that turns a
        // path into map source.
        //
        // A map that cannot be read or compiled is a warning, not a failure:
        // `WorldChoice::or_fallback` drops to the flat world, which still opens
        // a window you can move in. Refusing to start would make a missing file
        // indistinguishable from a broken build.
        if options.session.world == WorldChoice::Map {
            match std::fs::read_to_string(&options.map_path) {
                Ok(source) => {
                    // The map's name is its file stem, and it is what a
                    // personal best is filed under and what a `.s3d` says it
                    // was set on. Only the name: the identity that decides
                    // whether a saved run may be raced is the compiled
                    // collision digest, not this.
                    let name = map_name(&options.map_path);
                    if let Err(e) = straf3_game::scene::install(&name, &source) {
                        eprintln!("straf3: cannot compile {}: {e}", options.map_path);
                    }
                }
                Err(e) => eprintln!("straf3: cannot read {}: {e}", options.map_path),
            }
        }

        if let Some(path) = &options.replay_from {
            return run_replay(path, &options.replay);
        }

        // Same rule as `replay::replay`'s, at the other entry point: a played
        // session that quietly fell back to the flat world would open a window,
        // draw a run, print a checksum and save a personal best for a run that
        // happened on geometry this process does not have. The interactive
        // session's fallback stays — there, a window you can move in beats
        // refusing to start.
        if let Some(playback) = &options.session.playback
            && !options.session.world.is_available()
        {
            eprintln!(
                "straf3: {} was recorded in the `{}` world and no map is installed, so \
                 it cannot be played here. Pass `--map <file.map>` naming the map it was \
                 recorded on.",
                playback.source,
                options.session.world.spec()
            );
            return ExitCode::FAILURE;
        }

        let record_to = options.record_to.clone();
        let fixture = straf3_game::run(options.session);

        match (record_to, fixture) {
            (Some(path), Some(text)) => {
                if let Err(e) = std::fs::write(&path, text) {
                    eprintln!("straf3: cannot write {path}: {e}");
                    return ExitCode::FAILURE;
                }
                eprintln!("straf3: recording written to {path}");
            }
            (Some(path), None) => {
                // The startup check opened this path to prove it writable, so
                // if it did not exist before there is now an empty file where
                // the message says nothing was written. An empty file is never
                // a valid recording — it has no `rate` — so it would fail
                // loudly if replayed, but it would still sit in a directory
                // looking like a run somebody made. Clear it up, and *only*
                // when it is empty: a session that recorded nothing must not
                // destroy the recording that was already there.
                if std::fs::metadata(&path).is_ok_and(|m| m.len() == 0) {
                    let _ = std::fs::remove_file(&path);
                }
                eprintln!("straf3: nothing was recorded, {path} not written");
                return ExitCode::FAILURE;
            }
            _ => {}
        }

        ExitCode::SUCCESS
    }

    struct Parsed {
        session: Options,
        map_path: String,
        record_to: Option<String>,
        replay_from: Option<String>,
        play_from: Option<String>,
        replay: ReplayOptions,
    }

    /// Prove `--record`'s destination is writable *before* anything is played.
    ///
    /// # Why this is not left to the write at the end
    ///
    /// The recording is written when the session ends, which is the only moment
    /// it is complete. So a destination that cannot be written — a directory
    /// that does not exist, a path with no permission, a name that is itself a
    /// directory — was discovered *after* the run, and the run was already gone:
    /// the commands live only in the process that just exited. For a human
    /// playtest that is somebody's best attempt of the evening; for an
    /// unattended `--exit-after` session it is the whole point of the session.
    ///
    /// Creating the parent directory here rather than complaining about it is
    /// the same choice [`straf3_game::pb::store`] makes for personal bests, and
    /// for the same reason: `--record runs/tonight/attempt.txt` is a clear
    /// instruction, not an error.
    ///
    /// The file is opened, not written: an existing recording keeps its
    /// contents until there is a new one to replace them, so a session that
    /// crashes does not also destroy the last good file.
    ///
    /// # Errors
    ///
    /// The parent directory could not be created, or the file could not be
    /// opened for writing.
    fn check_recording_destination(path: &str) -> Result<(), String> {
        if let Some(dir) = std::path::Path::new(path).parent()
            && !dir.as_os_str().is_empty()
            && let Err(e) = std::fs::create_dir_all(dir)
        {
            return Err(format!(
                "cannot record to {path}: its directory {} cannot be created: {e}",
                dir.display()
            ));
        }
        std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(path)
            .map_err(|e| {
                format!(
                    "cannot record to {path}: {e}. Refusing to start rather than \
                     discovering this after the session, when the commands only exist \
                     in a process that has exited."
                )
            })?;
        Ok(())
    }

    /// Read a recorded command file and make it the session's driver.
    ///
    /// # The file wins, and that is the whole rule
    ///
    /// A playback session is only a replay if it runs the file's commands
    /// under the file's own parameters. `--profile`, `--rate` and `--world` are
    /// therefore overwritten here rather than merged: running a `cpm` recording
    /// under `vq3` because a flag said so would produce a different run and
    /// present it as a playback of this file. `--replay` has always behaved
    /// this way; this makes the windowed path behave identically.
    ///
    /// `--map` still selects which `.map` is compiled, because the fixture says
    /// `world map` and cannot say *which* map — binding a recording to specific
    /// geometry is the `.s3d` format's job, not this one's.
    ///
    /// # Errors
    ///
    /// The file could not be read, could not be parsed, or holds no commands.
    fn load_playback(path: &str, session: &mut Options) -> Result<(), String> {
        let text = read_input(path).map_err(|e| format!("cannot read {path}: {e}"))?;
        let fixture = straf3_game::replay::parse(&text).map_err(|e| format!("{path}: {e}"))?;
        if fixture.cmds.is_empty() {
            return Err(format!(
                "{path}: no commands, so there is nothing to play back"
            ));
        }

        session.rate = fixture.rate;
        session.profile = fixture.profile;
        session.profile_name = fixture.profile_name;
        session.world = fixture.world;
        session.playback = Some(straf3_game::Playback {
            cmds: fixture.cmds,
            spawn: fixture.spawn,
            yaw: fixture.yaw,
            source: path.to_owned(),
        });
        Ok(())
    }

    /// Where the course lives when `--map` is not given.
    const DEFAULT_MAP: &str = "assets/maps/coil.map";

    /// The map's name: its file stem, with the directory and the extension
    /// taken off.
    fn map_name(path: &str) -> String {
        std::path::Path::new(path)
            .file_stem()
            .map_or_else(|| "map".to_owned(), |s| s.to_string_lossy().into_owned())
    }

    /// Replay a recorded file with no window and no adapter.
    ///
    /// Nothing in this path touches winit or wgpu, deliberately: it has to run in
    /// CI and on this software-rendered box, where opening a window proves nothing
    /// and enumerating an adapter is a slow way to reach lavapipe.
    fn run_replay(path: &str, options: &ReplayOptions) -> ExitCode {
        let text = match read_input(path) {
            Ok(text) => text,
            Err(e) => {
                eprintln!("straf3: cannot read {path}: {e}");
                return ExitCode::FAILURE;
            }
        };
        let fixture = match straf3_game::replay::parse(&text) {
            Ok(fixture) => fixture,
            Err(e) => {
                eprintln!("straf3: {path}: {e}");
                return ExitCode::FAILURE;
            }
        };
        let trace = match straf3_game::replay::replay(&fixture, options) {
            Ok(trace) => trace,
            Err(e) => {
                eprintln!("straf3: {path}: {e}");
                return ExitCode::FAILURE;
            }
        };

        let Some(final_state) = trace.last() else {
            eprintln!("straf3: {path}: nothing to replay");
            return ExitCode::FAILURE;
        };

        if options.trace {
            if options.csv {
                println!("{}", straf3_game::TRACE_HEADER);
            }
            for state in &trace {
                println!("{}", straf3_game::trace_line(state, options.csv));
            }
            if !options.csv {
                println!("  checksum      {:#018x}", final_state.checksum());
            }
            return ExitCode::SUCCESS;
        }

        if options.csv {
            println!("{}", straf3_game::TRACE_HEADER);
            println!("{}", straf3_game::trace_line(final_state, true));
            return ExitCode::SUCCESS;
        }

        println!("straf3 replay ({path})");
        println!("  commands      {}", fixture.cmds.len());
        println!(
            "  rate          {} Hz ({} ms per command)",
            fixture.rate.hz(),
            fixture.rate.command_millis()
        );
        println!("  profile       {}", fixture.profile_name);
        println!("  world         {:?}", fixture.world.or_fallback());
        println!(
            "  frames        {}",
            if options.frame_ms.is_empty() {
                "one per tick".to_owned()
            } else {
                format!("{:?} ms, cycled", options.frame_ms)
            }
        );
        println!("final state");
        println!("  tick          {}", final_state.tick);
        println!("  time          {} ms", final_state.time_ms);
        // The run clock, which is the point of the exercise and is not the same
        // number as `time`: `time` is how long the session has been simulated,
        // this is how long the *run* took. Nothing here prints a time for an
        // unfinished attempt — that matches `straf3_replay::Outcome::run_time_ms`,
        // which is `Some` only for a finished run, and two places reporting the
        // same fact should agree about when the fact exists.
        match final_state.run {
            RunState::NotStarted => {
                println!("  run           not started (no start volume was crossed)");
            }
            RunState::Running { started_at_ms } => {
                println!(
                    "  run           started at {started_at_ms} ms, unfinished ({} ms so far)",
                    final_state.time_ms.saturating_sub(started_at_ms)
                );
            }
            RunState::Finished {
                started_at_ms,
                finished_at_ms,
            } => {
                let ms = finished_at_ms - started_at_ms;
                // Whole milliseconds first, because that is what a script
                // should read; the seconds in brackets are for a person, and
                // they are integer division, not a float (spec: no float
                // seconds, anywhere).
                println!(
                    "  run           {ms} ms  ({}.{:03} s, start {started_at_ms} ms, \
                     finish {finished_at_ms} ms)",
                    ms / 1000,
                    ms % 1000
                );
            }
        }
        println!(
            "  origin        {:.6} {:.6} {:.6}",
            final_state.player.origin.x, final_state.player.origin.y, final_state.player.origin.z
        );
        println!(
            "  velocity      {:.6} {:.6} {:.6}",
            final_state.player.velocity.x,
            final_state.player.velocity.y,
            final_state.player.velocity.z
        );
        // Printed last so a determinism check can pull it out with `tail -1`,
        // exactly as `straf3-headless` does.
        println!("  checksum      {:#018x}", final_state.checksum());
        ExitCode::SUCCESS
    }

    /// Read from a file, or from stdin when the path is `-`.
    fn read_input(path: &str) -> std::io::Result<String> {
        if path == "-" {
            std::io::read_to_string(std::io::stdin())
        } else {
            std::fs::read_to_string(path)
        }
    }

    /// Parse the command line. `Ok(None)` means "print the usage and stop".
    fn parse<I: Iterator<Item = String>>(args: I) -> Result<Option<Parsed>, String> {
        let mut session = Options::default();
        let mut map_path = DEFAULT_MAP.to_owned();
        let mut record_to = None;
        let mut replay_from = None;
        let mut play_from = None;
        let mut replay = ReplayOptions::default();

        let mut args = args;
        while let Some(arg) = args.next() {
            let mut value = || args.next().ok_or_else(|| format!("`{arg}` needs a value"));
            match arg.as_str() {
                "-h" | "--help" => return Ok(None),
                "--world" => {
                    let name = value()?;
                    session.world = WorldChoice::parse(&name)
                        .ok_or_else(|| format!("unknown world `{name}` (map|flat|empty)"))?;
                }
                "--map" => map_path = value()?,
                "--profile" => {
                    let name = value()?;
                    session.profile = straf3_game::profile::by_name(&name).ok_or_else(|| {
                        format!(
                            "unknown profile `{name}` ({})",
                            straf3_game::profile::NAMES
                        )
                    })?;
                    session.profile_name = name;
                }
                "--rate" => {
                    let hz = value()?;
                    let hz: u32 = hz
                        .parse()
                        .map_err(|_| format!("`--rate {hz}` is not a number"))?;
                    session.rate = TickRate::from_hz(hz).ok_or_else(|| {
                        format!("`--rate {hz}` is outside 1..=1000; below 1 Hz there is no sensible command, above 1000 Hz one would round to zero milliseconds")
                    })?;
                }
                "--record" => {
                    record_to = Some(value()?);
                    session.record = true;
                }
                "--pb-dir" => session.pb_dir = Some(value()?),
                "--no-pb" => session.pb_dir = None,
                "--exit-after" => {
                    let ms = value()?;
                    session.exit_after_ms = Some(
                        ms.parse()
                            .map_err(|_| format!("`--exit-after {ms}` is not a number"))?,
                    );
                }
                "--replay" => replay_from = Some(value()?),
                "--play" => play_from = Some(value()?),
                "--pacing-log" => session.pacing_log = Some(value()?),
                "--trace" => replay.trace = true,
                "--csv" => replay.csv = true,
                "--frame-ms" => {
                    let list = value()?;
                    replay.frame_ms = list
                        .split(',')
                        .map(str::trim)
                        .filter(|s| !s.is_empty())
                        .map(|s| {
                            s.parse::<u64>().map_err(|_| {
                                format!("`--frame-ms`: `{s}` is not a whole number of milliseconds")
                            })
                        })
                        .collect::<Result<Vec<_>, _>>()?;
                    if replay.frame_ms.is_empty() {
                        return Err("`--frame-ms` needs at least one frame duration".to_owned());
                    }
                }
                other => return Err(format!("unknown option `{other}`")),
            }
        }

        if replay_from.is_none() {
            for (flag, set) in [
                ("--trace", replay.trace),
                ("--csv", replay.csv),
                ("--frame-ms", !replay.frame_ms.is_empty()),
            ] {
                if set {
                    return Err(format!("`{flag}` only means something with `--replay`"));
                }
            }
        }

        // `--replay` opens no window and draws no frame, so there would be
        // nothing to time. Accepting the flag and writing an empty file would
        // look like a measurement of a perfectly smooth session.
        if replay_from.is_some() && session.pacing_log.is_some() {
            return Err(
                "`--pacing-log` needs frames to time, and `--replay` draws none. Run the \
                 windowed build (with `--play` for a repeatable session)."
                    .to_owned(),
            );
        }

        // They are two different answers to "what runs this file": one opens a
        // window and one refuses to. Silently preferring either would run
        // something the command line did not unambiguously ask for.
        if replay_from.is_some() && play_from.is_some() {
            return Err(
                "`--replay` and `--play` are alternatives: `--replay` runs the file with \
                 no window, `--play` runs it with one. Pick one."
                    .to_owned(),
            );
        }

        Ok(Some(Parsed {
            session,
            map_path,
            record_to,
            replay_from,
            play_from,
            replay,
        }))
    }

    #[cfg(test)]
    mod tests {
        use super::*;
        use straf3_sim::PhysicsProfile;

        fn parse_args(args: &[&str]) -> Result<Option<Parsed>, String> {
            parse(args.iter().map(|s| (*s).to_owned()))
        }

        #[test]
        fn no_arguments_is_the_default_map_at_125hz_under_cpm() {
            let parsed = parse_args(&[]).unwrap().unwrap();
            assert_eq!(parsed.session.world, WorldChoice::Map);
            assert_eq!(parsed.map_path, DEFAULT_MAP);
            assert_eq!(parsed.session.rate, TickRate::HZ_125);
            assert_eq!(parsed.session.profile_name, "cpm");
            assert!(parsed.record_to.is_none());
        }

        #[test]
        fn a_rate_outside_what_whole_milliseconds_can_express_is_refused() {
            assert!(parse_args(&["--rate", "0"]).is_err());
            assert!(parse_args(&["--rate", "1001"]).is_err());
            assert!(parse_args(&["--rate", "many"]).is_err());
            assert_eq!(
                parse_args(&["--rate", "76"]).unwrap().unwrap().session.rate,
                TickRate::HZ_76
            );
        }

        #[test]
        fn recording_sets_both_the_flag_and_the_destination() {
            let parsed = parse_args(&["--record", "run.txt"]).unwrap().unwrap();
            assert!(parsed.session.record);
            assert_eq!(parsed.record_to.as_deref(), Some("run.txt"));
        }

        #[test]
        fn unknown_options_and_missing_values_are_errors_not_defaults() {
            assert!(parse_args(&["--nonsense"]).is_err());
            assert!(parse_args(&["--world"]).is_err());
            assert!(parse_args(&["--world", "moon"]).is_err());
            assert!(parse_args(&["--profile", "quake1"]).is_err());
        }

        #[test]
        fn a_frame_schedule_is_a_list_of_whole_milliseconds() {
            let parsed = parse_args(&["--replay", "r.txt", "--frame-ms", "1,0,200, 37"])
                .unwrap()
                .unwrap();
            assert_eq!(parsed.replay.frame_ms, vec![1, 0, 200, 37]);
            assert_eq!(parsed.replay_from.as_deref(), Some("r.txt"));
            assert!(parse_args(&["--replay", "r.txt", "--frame-ms", "16.7"]).is_err());
            assert!(parse_args(&["--replay", "r.txt", "--frame-ms", ""]).is_err());
        }

        #[test]
        fn replay_only_flags_are_refused_without_a_replay() {
            // Silently ignoring them would let someone believe they had measured
            // a frame schedule when they had opened a window instead.
            assert!(parse_args(&["--trace"]).is_err());
            assert!(parse_args(&["--csv"]).is_err());
            assert!(parse_args(&["--frame-ms", "8"]).is_err());
            assert!(parse_args(&["--replay", "r.txt", "--trace", "--csv"]).is_ok());
        }

        #[test]
        fn exiting_after_a_deadline_is_a_whole_number_of_milliseconds() {
            let parsed = parse_args(&["--exit-after", "2000"]).unwrap().unwrap();
            assert_eq!(parsed.session.exit_after_ms, Some(2_000));
            assert!(parse_args(&["--exit-after", "soon"]).is_err());
        }

        #[test]
        fn playing_and_replaying_are_alternatives_not_a_combination() {
            assert_eq!(
                parse_args(&["--play", "run.txt"])
                    .unwrap()
                    .unwrap()
                    .play_from
                    .as_deref(),
                Some("run.txt")
            );
            // Silently preferring one would run something the command line did
            // not unambiguously ask for — with a window or without one.
            assert!(parse_args(&["--play", "a.txt", "--replay", "b.txt"]).is_err());
            assert!(parse_args(&["--play"]).is_err());
        }

        #[test]
        fn a_pacing_log_needs_frames_so_it_is_refused_with_replay() {
            assert_eq!(
                parse_args(&["--pacing-log", "frames.csv"])
                    .unwrap()
                    .unwrap()
                    .session
                    .pacing_log
                    .as_deref(),
                Some("frames.csv")
            );
            // An empty CSV from a session that drew nothing would read as a
            // perfectly smooth one.
            assert!(parse_args(&["--replay", "r.txt", "--pacing-log", "f.csv"]).is_err());
            assert!(parse_args(&["--play", "r.txt", "--pacing-log", "f.csv"]).is_ok());
        }

        /// The file's own parameters win, so a played session cannot be run
        /// under physics the recording was not made under.
        #[test]
        fn a_played_fixture_overrides_the_flags_that_would_change_its_physics() {
            const FIXTURE: &str = "\
rate 76
profile vq3
world flat 0
spawn 1 2 24
yaw 45

cmd 3 127 0 0 - 0 45
";
            let dir = std::env::temp_dir().join(format!("straf3-play-cli-{}", std::process::id()));
            std::fs::create_dir_all(&dir).unwrap();
            let path = dir.join("run.txt");
            std::fs::write(&path, FIXTURE).unwrap();

            // Deliberately contradicted on the command line.
            let mut parsed = parse_args(&["--profile", "cpm", "--rate", "125", "--world", "map"])
                .unwrap()
                .unwrap();
            load_playback(&path.to_string_lossy(), &mut parsed.session).unwrap();

            assert_eq!(parsed.session.rate, TickRate::from_hz(76).unwrap());
            assert_eq!(parsed.session.profile, PhysicsProfile::vq3());
            assert_eq!(parsed.session.profile_name, "vq3");
            assert_eq!(parsed.session.world, WorldChoice::Flat);
            let playback = parsed.session.playback.expect("the stream was loaded");
            assert_eq!(playback.cmds.len(), 3);
            assert_eq!(playback.yaw, straf3_sim::num::s(45.0));

            let _ = std::fs::remove_dir_all(&dir);
        }

        #[test]
        fn a_playback_file_that_cannot_be_read_or_holds_nothing_is_refused() {
            let mut session = Options::default();
            assert!(load_playback("no/such/file.txt", &mut session).is_err());

            let dir = std::env::temp_dir().join(format!("straf3-play-empty-{}", std::process::id()));
            std::fs::create_dir_all(&dir).unwrap();
            let path = dir.join("empty.txt");
            // Parses, but has no commands: playing it would open a window on a
            // player standing still and call it a replay.
            std::fs::write(&path, "rate 125\nprofile cpm\nworld flat 0\n").unwrap();
            let err = load_playback(&path.to_string_lossy(), &mut session).unwrap_err();
            assert!(err.contains("nothing to play back"), "{err}");

            let _ = std::fs::remove_dir_all(&dir);
        }

        /// A `--record` destination is proved writable at startup, because
        /// discovering it afterwards means the session is already lost.
        #[test]
        fn an_unwritable_recording_destination_is_refused_before_anything_is_played() {
            let dir = std::env::temp_dir().join(format!("straf3-rec-{}", std::process::id()));
            let _ = std::fs::remove_dir_all(&dir);

            // A directory that does not exist yet is created, not complained
            // about: `--record runs/tonight/attempt.txt` is an instruction.
            let nested = dir.join("tonight").join("attempt.txt");
            check_recording_destination(&nested.to_string_lossy())
                .expect("a missing directory is created");
            assert!(nested.exists());

            // An existing recording keeps its contents until there is a new one
            // to replace them — a crashed session must not also destroy the
            // last good file.
            std::fs::write(&nested, "rate 125\n").unwrap();
            check_recording_destination(&nested.to_string_lossy()).unwrap();
            assert_eq!(std::fs::read_to_string(&nested).unwrap(), "rate 125\n");

            // A path that is a directory can never be written as a file, and
            // that is knowable now rather than after the run.
            let occupied = dir.join("tonight");
            assert!(check_recording_destination(&occupied.to_string_lossy()).is_err());

            let _ = std::fs::remove_dir_all(&dir);
        }

        #[test]
        fn help_asks_for_the_usage_rather_than_a_session() {
            assert!(parse_args(&["--help"]).unwrap().is_none());
            assert!(parse_args(&["-h"]).unwrap().is_none());
        }
    }
}
