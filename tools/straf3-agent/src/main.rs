//! `straf3-agent` — the command line.
//!
//! Three commands, and the split between them is deliberate. `plan` derives a
//! course from a map's own trigger volumes and prints it, without running
//! anything: a plan only reachable through a run is a plan nobody can check on
//! its own. `run` searches that course and writes a replay fixture. `fixture`
//! regenerates the crate-internal map the r9 demonstration is measured on.
//!
//! **`run` takes the same arguments for every map.** There is no per-map flag,
//! no aim point, no axis and no threshold — that absence is r27's claim, and it
//! is easier to check on a command line than in a source tree.

use std::path::PathBuf;
use std::process::ExitCode;

use straf3_agent::search::{Alphabet, SearchSpec};
use straf3_agent::{course::CoursePlan, fixture, profile, report, search};
use straf3_sim::num::{Scalar, s};
use straf3_sim::{SimState, TickRate};

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    match parse(&args) {
        Ok(Command::Help) => {
            print!("{USAGE}");
            ExitCode::SUCCESS
        }
        Ok(Command::Plan(p)) => match p.execute() {
            Ok(()) => ExitCode::SUCCESS,
            Err(e) => {
                eprintln!("straf3-agent: {e}");
                ExitCode::FAILURE
            }
        },
        Ok(Command::Run(r)) => match r.execute() {
            Ok(finished) => {
                if finished {
                    ExitCode::SUCCESS
                } else {
                    // A run that did not finish exits non-zero so a script
                    // cannot mistake a negative for a completion.
                    ExitCode::FAILURE
                }
            }
            Err(e) => {
                eprintln!("straf3-agent: {e}");
                ExitCode::FAILURE
            }
        },
        Ok(Command::Fixture(f)) => match f.execute() {
            Ok(()) => ExitCode::SUCCESS,
            Err(e) => {
                eprintln!("straf3-agent: {e}");
                ExitCode::FAILURE
            }
        },
        Err(e) => {
            eprintln!("straf3-agent: {e}\n");
            eprint!("{USAGE}");
            ExitCode::FAILURE
        }
    }
}

const USAGE: &str = "\
usage: straf3-agent <plan|run|fixture> [options]

plan <map.map>
  Compiles the map with the shipped compiler and prints the course it implies:
  the start, the checkpoints in order, the finish, an aim point derived from
  each volume's own geometry, and the legs between them. No number about any
  particular map appears in this program.

run <map.map>
  Searches that course and, with --fixture, writes a command stream the shipped
  `straf3 --replay` binary can replay. Exits non-zero if the run clock did not
  reach Finished, so a negative cannot be mistaken for a completion.

  Every option below is a property of the SEARCH and is the same for every map.
  There is deliberately no aim point, axis, threshold or per-map flag.

  --frontier <n>     open-list cap. 1 is exactly greedy one-step-per-window,
                     which is the negative control (default 512)
  --stride <n>       commands committed per edge (default 8)
  --patience <f>     weight on time already spent (default 0.25)
  --cells <f>        visited-set cell size in player hull widths (default 1)
  --no-crouch        drop CROUCH from the alphabet
  --max-expansions <n>   give up after this many (default 60000)
  --max-ticks <n>    give up after this many simulated commands
  --fixture <file>   write the replay stream here
  --out <file>       write the report here as well as to stdout

fixture [--out <file>]
  Regenerates the crate-internal `fixtures/wishbone.map` from
  `straf3_agent::fixture::wishbone`. A test fails if the committed copy and the
  generator disagree.

common
  --profile <name>   straf3|cpm|vq3|experimental (default cpm). The default is
                     not `straf3` only because no seat has yet replayed a
                     straf3-headed stream through the shipped binary; the two
                     are numerically equal today.
  -h, --help         this
";

enum Command {
    Help,
    Plan(Plan),
    Run(Run),
    Fixture(Fixture),
}

struct Plan {
    source: PathBuf,
    profile_name: String,
    out: Option<PathBuf>,
}

struct Run {
    source: PathBuf,
    profile_name: String,
    out: Option<PathBuf>,
    fixture_out: Option<PathBuf>,
    spec: SearchSpec,
}

struct Fixture {
    out: PathBuf,
}

fn parse(args: &[String]) -> Result<Command, String> {
    let mut it = args.iter();
    let Some(first) = it.next() else {
        return Ok(Command::Help);
    };
    let verb = match first.as_str() {
        "-h" | "--help" => return Ok(Command::Help),
        v @ ("plan" | "run" | "fixture") => v.to_owned(),
        other => return Err(format!("unknown command `{other}`")),
    };

    let mut source: Option<PathBuf> = None;
    let mut profile_name = profile::DEFAULT.to_owned();
    let mut out: Option<PathBuf> = None;
    let mut fixture_out: Option<PathBuf> = None;
    let mut spec = SearchSpec::default();
    while let Some(arg) = it.next() {
        match arg.as_str() {
            "-h" | "--help" => return Ok(Command::Help),
            "--profile" => profile_name = next(&mut it, "--profile")?,
            "--out" => out = Some(PathBuf::from(next(&mut it, "--out")?)),
            "--fixture" => fixture_out = Some(PathBuf::from(next(&mut it, "--fixture")?)),
            "--frontier" => spec.frontier = number(&mut it, "--frontier")?,
            "--stride" => spec.stride = number(&mut it, "--stride")?,
            "--patience" => spec.patience = scalar(&mut it, "--patience")?,
            "--cells" => spec.cells_per_hull = scalar(&mut it, "--cells")?,
            "--no-crouch" => spec.alphabet = Alphabet { crouch: false },
            "--max-expansions" => spec.max_expansions = number(&mut it, "--max-expansions")?,
            "--max-ticks" => spec.max_ticks = number(&mut it, "--max-ticks")?,
            "--max-depth" => spec.max_depth = number(&mut it, "--max-depth")?,
            other if other.starts_with('-') => return Err(format!("unknown option {other}")),
            path if source.is_none() => source = Some(PathBuf::from(path)),
            extra => return Err(format!("unexpected argument `{extra}`")),
        }
    }

    if profile::by_name(&profile_name).is_none() {
        return Err(format!(
            "unknown profile `{profile_name}`; this build has {}",
            profile::NAMES
        ));
    }
    if spec.frontier == 0 {
        return Err("--frontier must be at least 1".to_owned());
    }
    if spec.stride == 0 {
        return Err("--stride must be at least 1".to_owned());
    }

    match verb.as_str() {
        "fixture" => Ok(Command::Fixture(Fixture {
            out: out.unwrap_or_else(|| {
                PathBuf::from(env!("CARGO_MANIFEST_DIR")).join(fixture::PATH)
            }),
        })),
        "plan" => Ok(Command::Plan(Plan {
            source: source.ok_or_else(|| "plan wants a .map file".to_owned())?,
            profile_name,
            out,
        })),
        _ => Ok(Command::Run(Run {
            source: source.ok_or_else(|| "run wants a .map file".to_owned())?,
            profile_name,
            out,
            fixture_out,
            spec,
        })),
    }
}

fn number<'a, T: std::str::FromStr>(
    it: &mut impl Iterator<Item = &'a String>,
    flag: &str,
) -> Result<T, String> {
    next(it, flag)?
        .parse()
        .map_err(|_| format!("{flag} wants a number"))
}

fn scalar<'a>(it: &mut impl Iterator<Item = &'a String>, flag: &str) -> Result<Scalar, String> {
    number::<f32>(it, flag).map(s)
}

fn next<'a>(it: &mut impl Iterator<Item = &'a String>, flag: &str) -> Result<String, String> {
    it.next()
        .cloned()
        .ok_or_else(|| format!("{flag} wants a value"))
}

impl Plan {
    fn execute(&self) -> Result<(), String> {
        let path = self.source.display().to_string();
        let source = std::fs::read_to_string(&self.source)
            .map_err(|e| format!("could not read {path}: {e}"))?;
        // The shipped compiler, deliberately. The course has to be derived from
        // the world the game builds, not from a second reading of the same file.
        let map = straf3_map::compile(&source).map_err(|e| format!("{path}: {e}"))?;
        let profile = profile::by_name(&self.profile_name)
            .expect("the name was checked when the arguments were parsed");

        let plan = CoursePlan::derive(&map, &profile, &self.profile_name);
        let text = report::plan(&path, &map, &profile, &plan);
        print!("{text}");

        if let Some(dest) = &self.out {
            std::fs::write(dest, &text)
                .map_err(|e| format!("could not write {}: {e}", dest.display()))?;
        }
        Ok(())
    }
}

impl Fixture {
    fn execute(&self) -> Result<(), String> {
        let text = fixture::wishbone();
        if let Some(parent) = self.out.parent() {
            std::fs::create_dir_all(parent)
                .map_err(|e| format!("could not create {}: {e}", parent.display()))?;
        }
        std::fs::write(&self.out, &text)
            .map_err(|e| format!("could not write {}: {e}", self.out.display()))?;
        // Compiled here as well as in the tests, so a generator that emits text
        // the shipped compiler rejects fails at the moment it is run rather than
        // at the moment someone tries to use the file.
        let map = straf3_map::compile(&text)
            .map_err(|e| format!("the generated fixture does not compile: {e}"))?;
        println!(
            "wrote {} ({} bytes, {} solid hulls, {} trigger volumes, collision digest {:#018x})",
            self.out.display(),
            text.len(),
            map.hulls.len(),
            map.triggers.len(),
            map.collision_digest()
        );
        Ok(())
    }
}

impl Run {
    /// Returns whether the run clock reached `Finished`.
    fn execute(&self) -> Result<bool, String> {
        let path = self.source.display().to_string();
        let source = std::fs::read_to_string(&self.source)
            .map_err(|e| format!("could not read {path}: {e}"))?;
        let map = straf3_map::compile(&source).map_err(|e| format!("{path}: {e}"))?;
        let profile = profile::by_name(&self.profile_name)
            .expect("the name was checked when the arguments were parsed");

        let plan = CoursePlan::derive(&map, &profile, &self.profile_name);
        if !plan.is_runnable() {
            return Err(format!(
                "{path} has no start/finish pair, so there is no course to run"
            ));
        }
        let goals = search::goals_of(&plan);
        let world = map.collider();
        let rate = TickRate::HZ_125;
        let start = SimState::spawned_at(map.spawn, map.spawn_yaw);
        let result = search::run(&goals, &self.spec, rate, start, &world, &profile);

        let text = report::run(&path, &map, &profile, &plan, &self.spec, &result);
        print!("{text}");
        if let Some(dest) = &self.out {
            std::fs::write(dest, &text)
                .map_err(|e| format!("could not write {}: {e}", dest.display()))?;
        }

        if let Some(dest) = &self.fixture_out {
            let note = format!(
                "{} at frontier {}: {} — reported {} ms, checksum {:#018x}",
                path,
                self.spec.frontier,
                match result.run_ms() {
                    Some(ms) => format!("FINISHED in {ms} ms"),
                    None => format!("did not finish ({:?})", result.stop),
                },
                result.run_ms().unwrap_or(0),
                result.checksum,
            );
            let stream = replay_stream(&result.cmds, rate, &map, &path, &note);
            if let Some(parent) = dest.parent() {
                let _ = std::fs::create_dir_all(parent);
            }
            std::fs::write(dest, &stream)
                .map_err(|e| format!("could not write {}: {e}", dest.display()))?;
            println!(
                "wrote {} ({} bytes, {} commands)",
                dest.display(),
                stream.len(),
                result.cmds.len()
            );
        }
        Ok(result.run_ms().is_some())
    }
}

/// The shipped `--replay` reader's own input format.
///
/// Byte-compatible with what `straf3-game`'s recorder writes, because the whole
/// point of emitting one is that the shipped binary reads it and lands on the
/// checksum this program printed. Identical runs of commands are folded into one
/// `cmd` line with a repeat count, exactly as the recorder does.
fn replay_stream(
    cmds: &[straf3_sim::UserCmd],
    rate: TickRate,
    map: &straf3_map::CompiledMap,
    map_path: &str,
    note: &str,
) -> String {
    use core::fmt::Write as _;
    use straf3_sim::Buttons;

    let mut out = String::with_capacity(48 * cmds.len() + 512);
    let _ = write!(
        out,
        "# Generated by tools/straf3-agent. Not played by a person.\n# {note}\n\
         # Replay it with `straf3 --replay <this file> --map {map_path}`; the final\n\
         # checksum must equal the one straf3-agent printed beside it.\n\
         #\n# The fixture format carries no map identity, so this line is the only\n\
         # record of which world it was made in: collision digest {:#018x}.\n\n",
        map.collision_digest()
    );
    let _ = writeln!(out, "rate {}", rate.hz());
    out.push_str("profile cpm\n");
    out.push_str("world map\n");
    let _ = writeln!(
        out,
        "spawn {:?} {:?} {:?}",
        map.spawn.x, map.spawn.y, map.spawn.z
    );
    let _ = writeln!(out, "yaw {:?}\n", map.spawn_yaw);
    out.push_str("# cmd <repeat> <fwd> <right> <up> <buttons> <pitch> <yaw> <roll>\n");

    let mut i = 0;
    while i < cmds.len() {
        let cmd = cmds[i];
        let mut repeat = 1;
        while i + repeat < cmds.len() && cmds[i + repeat] == cmd {
            repeat += 1;
        }
        let mut names: Vec<&str> = Vec::new();
        if cmd.buttons.contains(Buttons::JUMP) {
            names.push("jump");
        }
        if cmd.buttons.contains(Buttons::CROUCH) {
            names.push("crouch");
        }
        let buttons = if names.is_empty() {
            "-".to_string()
        } else {
            names.join("+")
        };
        let _ = writeln!(
            out,
            "cmd {} {} {} {} {} {:?} {:?} {:?}",
            repeat,
            cmd.forward_move,
            cmd.right_move,
            cmd.up_move,
            buttons,
            cmd.view.pitch_degrees(),
            cmd.view.yaw_degrees(),
            cmd.view.roll_degrees(),
        );
        i += repeat;
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn args(list: &[&str]) -> Vec<String> {
        list.iter().map(|s| (*s).to_owned()).collect()
    }

    #[test]
    fn a_plan_needs_a_map() {
        assert!(parse(&args(&["plan"])).is_err());
    }

    #[test]
    fn the_map_is_positional_and_the_options_are_not() {
        let parsed = parse(&args(&[
            "plan",
            "a.map",
            "--profile",
            "vq3",
            "--out",
            "b.txt",
        ]));
        match parsed {
            Ok(Command::Plan(p)) => {
                assert_eq!(p.source, PathBuf::from("a.map"));
                assert_eq!(p.profile_name, "vq3");
                assert_eq!(p.out, Some(PathBuf::from("b.txt")));
            }
            _ => panic!("expected a plan"),
        }
    }

    #[test]
    fn an_unknown_profile_is_refused_before_anything_is_compiled() {
        // Refused rather than defaulted: a plan that silently used other
        // constants would have aim points derived for a different-sized player.
        let Err(e) = parse(&args(&["plan", "a.map", "--profile", "quake3"])) else {
            panic!("a name the shipped --replay reader refuses must be refused here too");
        };
        assert!(e.contains("unknown profile"), "{e}");
    }

    #[test]
    fn straf3_is_accepted_now_that_the_client_half_has_landed() {
        // This test used to assert `--profile straf3` was refused, on the
        // premise that `straf3-game` did not know the name. r1 landed it —
        // crates/straf3-game/src/profile.rs:102 — so the premise is dead and
        // the assertion inverts with it.
        assert!(parse(&args(&["plan", "a.map", "--profile", "straf3"])).is_ok());
    }

    #[test]
    fn no_arguments_is_the_usage_text() {
        assert!(matches!(parse(&[]), Ok(Command::Help)));
    }
}
