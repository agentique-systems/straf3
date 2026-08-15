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

// The web build has no `main`: `straf3_game::start_web` is the entry point and
// JS calls it. Compiling this file for wasm32 would produce a second, useless
// module alongside the cdylib.
#![cfg(not(target_arch = "wasm32"))]

use std::process::ExitCode;

use straf3_game::app::Options;
use straf3_game::replay::ReplayOptions;
use straf3_game::scene::WorldChoice;
use straf3_sim::{PhysicsProfile, TickRate};

const USAGE: &str = "\
usage: straf3 [options]                     open a window and play
       straf3 --replay <file> [options]     run a recorded file, no window

  --world <arena|flat|empty>  geometry to play in (default arena)
  --profile <cpm|vq3>         movement constants (default cpm)
  --rate <hz>                 command rate, 1..=1000 (default 125)
  --record <file>             write every command produced to <file>, in
                              straf3-headless's input format
  --exit-after <ms>           close the window after <ms> of wall time, so an
                              unattended run can be recorded and replayed
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
";

fn main() -> ExitCode {
    // `info` by default: the once-a-second speed readout is the only feedback
    // this wave has, since the telemetry overlay is out of scope.
    env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("info"))
        .format_timestamp(None)
        .init();

    let options = match parse(std::env::args().skip(1)) {
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

    if let Some(path) = &options.replay_from {
        return run_replay(path, &options.replay);
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
            eprintln!("straf3: nothing was recorded, {path} not written");
            return ExitCode::FAILURE;
        }
        _ => {}
    }

    ExitCode::SUCCESS
}

struct Parsed {
    session: Options,
    record_to: Option<String>,
    replay_from: Option<String>,
    replay: ReplayOptions,
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
    println!(
        "  origin        {:.6} {:.6} {:.6}",
        final_state.player.origin.x, final_state.player.origin.y, final_state.player.origin.z
    );
    println!(
        "  velocity      {:.6} {:.6} {:.6}",
        final_state.player.velocity.x, final_state.player.velocity.y, final_state.player.velocity.z
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
    let mut record_to = None;
    let mut replay_from = None;
    let mut replay = ReplayOptions::default();

    let mut args = args;
    while let Some(arg) = args.next() {
        let mut value = || args.next().ok_or_else(|| format!("`{arg}` needs a value"));
        match arg.as_str() {
            "-h" | "--help" => return Ok(None),
            "--world" => {
                let name = value()?;
                session.world = WorldChoice::parse(&name)
                    .ok_or_else(|| format!("unknown world `{name}` (arena|flat|empty)"))?;
            }
            "--profile" => {
                let name = value()?;
                session.profile = match name.as_str() {
                    "cpm" => PhysicsProfile::cpm(),
                    "vq3" => PhysicsProfile::vq3(),
                    other => return Err(format!("unknown profile `{other}` (cpm|vq3)")),
                };
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
            "--exit-after" => {
                let ms = value()?;
                session.exit_after_ms = Some(
                    ms.parse()
                        .map_err(|_| format!("`--exit-after {ms}` is not a number"))?,
                );
            }
            "--replay" => replay_from = Some(value()?),
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

    Ok(Some(Parsed {
        session,
        record_to,
        replay_from,
        replay,
    }))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse_args(args: &[&str]) -> Result<Option<Parsed>, String> {
        parse(args.iter().map(|s| (*s).to_owned()))
    }

    #[test]
    fn no_arguments_is_the_arena_at_125hz_under_cpm() {
        let parsed = parse_args(&[]).unwrap().unwrap();
        assert_eq!(parsed.session.world, WorldChoice::Arena);
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
    fn help_asks_for_the_usage_rather_than_a_session() {
        assert!(parse_args(&["--help"]).unwrap().is_none());
        assert!(parse_args(&["-h"]).unwrap().is_none());
    }
}
