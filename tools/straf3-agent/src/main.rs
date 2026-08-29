//! `straf3-agent` — the command line.
//!
//! One command so far: `plan`, which derives a course from a map's own trigger
//! volumes and prints it. Running the course is the next unit of work and lands
//! as a second command rather than as a flag on this one — a plan that is only
//! reachable through a run is a plan nobody can check on its own.

use std::path::PathBuf;
use std::process::ExitCode;

use straf3_agent::{course::CoursePlan, profile, report};

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
        Err(e) => {
            eprintln!("straf3-agent: {e}\n");
            eprint!("{USAGE}");
            ExitCode::FAILURE
        }
    }
}

const USAGE: &str = "\
usage: straf3-agent plan <map.map> [options]

  Compiles the map with the shipped compiler and prints the course it implies:
  the start, the checkpoints in order, the finish, an aim point derived from
  each volume's own geometry, and the legs between them. No number about any
  particular map appears in this program.

  --profile <name>   cpm|vq3|experimental (default cpm). `straf3` is absent on
                     purpose: the shipped --replay reader does not accept it.
  --out <file>       write the printout here as well as to stdout
  -h, --help         this
";

enum Command {
    Help,
    Plan(Plan),
}

struct Plan {
    source: PathBuf,
    profile_name: String,
    out: Option<PathBuf>,
}

fn parse(args: &[String]) -> Result<Command, String> {
    let mut it = args.iter();
    let Some(first) = it.next() else {
        return Ok(Command::Help);
    };
    match first.as_str() {
        "-h" | "--help" => return Ok(Command::Help),
        "plan" => {}
        other => return Err(format!("unknown command `{other}`")),
    }

    let mut source: Option<PathBuf> = None;
    let mut profile_name = profile::DEFAULT.to_owned();
    let mut out: Option<PathBuf> = None;
    while let Some(arg) = it.next() {
        match arg.as_str() {
            "-h" | "--help" => return Ok(Command::Help),
            "--profile" => profile_name = next(&mut it, "--profile")?,
            "--out" => out = Some(PathBuf::from(next(&mut it, "--out")?)),
            other if other.starts_with('-') => return Err(format!("unknown option {other}")),
            path if source.is_none() => source = Some(PathBuf::from(path)),
            extra => return Err(format!("unexpected argument `{extra}`")),
        }
    }

    let source = source.ok_or_else(|| "plan wants a .map file".to_owned())?;
    if profile::by_name(&profile_name).is_none() {
        return Err(format!(
            "unknown profile `{profile_name}`; this build has {}",
            profile::NAMES
        ));
    }
    Ok(Command::Plan(Plan {
        source,
        profile_name,
        out,
    }))
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
        let Err(e) = parse(&args(&["plan", "a.map", "--profile", "straf3"])) else {
            panic!("a name the shipped --replay reader refuses must be refused here too");
        };
        assert!(e.contains("unknown profile"), "{e}");
    }

    #[test]
    fn no_arguments_is_the_usage_text() {
        assert!(matches!(parse(&[]), Ok(Command::Help)));
    }
}
