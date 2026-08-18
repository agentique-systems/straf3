//! `straf3-lab` — run the measurements, publish the document.
//!
//! Normally reached through `cargo xtask lab`, which fills in the tree state.
//! Runnable directly, in which case the document says its provenance was not
//! recorded rather than quietly leaving it out.

use std::path::{Path, PathBuf};
use std::process::ExitCode;

use straf3_lab::report::{self, Provenance};
use straf3_lab::{Dataset, dataset};

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    match parse(&args) {
        Ok(Options::Help) => {
            print!("{USAGE}");
            ExitCode::SUCCESS
        }
        Ok(Options::Run(run)) => match run.execute() {
            Ok(true) => ExitCode::SUCCESS,
            Ok(false) => ExitCode::FAILURE,
            Err(e) => {
                eprintln!("straf3-lab: {e}");
                ExitCode::FAILURE
            }
        },
        Err(e) => {
            eprintln!("straf3-lab: {e}\n");
            eprint!("{USAGE}");
            ExitCode::FAILURE
        }
    }
}

const USAGE: &str = "\
usage: straf3-lab [options]

  Measures the movement language and writes the results document. Deterministic
  and headless: same invocation, same bytes out, on any machine.

  --emit <file>      write the document here instead of docs/movement-lab.md
  --pinned <file>    read/write the machine-readable fixture here
  --check            recompute and compare against the pinned fixture, naming
                     which measurements moved. Writes nothing, and exits
                     non-zero if any of them did.
  --tree <state>     the commit these numbers were measured from, stamped into
                     the document's header. `cargo xtask lab` fills this in.
  --dirty            the working tree had uncommitted changes at --tree
  --stdout           print the document instead of writing it
  -h, --help         this
";

enum Options {
    Help,
    Run(Run),
}

struct Run {
    document: PathBuf,
    pinned: PathBuf,
    check: bool,
    to_stdout: bool,
    provenance: Provenance,
}

fn parse(args: &[String]) -> Result<Options, String> {
    let mut document = PathBuf::from(report::DEFAULT_PATH);
    let mut pinned = PathBuf::from(report::PINNED_PATH);
    let mut check = false;
    let mut to_stdout = false;
    let mut tree: Option<String> = None;
    let mut dirty = false;

    let mut it = args.iter();
    while let Some(arg) = it.next() {
        match arg.as_str() {
            "-h" | "--help" => return Ok(Options::Help),
            "--check" => check = true,
            "--stdout" => to_stdout = true,
            "--dirty" => dirty = true,
            "--emit" => document = PathBuf::from(next(&mut it, "--emit")?),
            "--pinned" => pinned = PathBuf::from(next(&mut it, "--pinned")?),
            "--tree" => tree = Some(next(&mut it, "--tree")?),
            other => return Err(format!("unknown option {other}")),
        }
    }

    Ok(Options::Run(Run {
        document,
        pinned,
        check,
        to_stdout,
        provenance: match tree {
            Some(t) => Provenance::at(t, dirty),
            None => Provenance::unrecorded(),
        },
    }))
}

fn next<'a>(it: &mut impl Iterator<Item = &'a String>, flag: &str) -> Result<String, String> {
    it.next()
        .cloned()
        .ok_or_else(|| format!("{flag} wants a value"))
}

impl Run {
    /// Returns whether the run should be considered a success.
    fn execute(&self) -> Result<bool, String> {
        let sections = straf3_lab::run();
        let data = Dataset::from_sections(&sections);

        if self.check {
            return self.compare(&data);
        }

        let document = report::render(
            &sections,
            &data,
            &self.provenance,
            straf3_lab::geometry::MIRRORED,
        );

        if self.to_stdout {
            print!("{document}");
            return Ok(true);
        }

        write(&self.document, &document)?;
        write(&self.pinned, &report::render_pinned(&data))?;
        println!(
            "{} measurements\n  {}\n  {}",
            data.len(),
            self.document.display(),
            self.pinned.display()
        );
        Ok(true)
    }

    /// Recompute and compare against the pinned fixture.
    fn compare(&self, now: &Dataset) -> Result<bool, String> {
        let text = std::fs::read_to_string(&self.pinned).map_err(|e| {
            format!(
                "cannot read the pinned measurements at {}: {e}\n\
                 Run `{}` once to create it.",
                self.pinned.display(),
                report::COMMAND
            )
        })?;
        let pinned = Dataset::from_tsv(&text)
            .map_err(|e| format!("{} is malformed: {e}", self.pinned.display()))?;
        let changes = dataset::diff(&pinned, now);

        if changes.is_empty() {
            println!(
                "{} measurements, all unchanged against {}",
                now.len(),
                self.pinned.display()
            );
            return Ok(true);
        }

        eprintln!(
            "{} of {} measurements moved against {}:\n",
            changes.len(),
            now.len(),
            self.pinned.display()
        );
        eprint!("{}", dataset::summarise(&changes));
        eprintln!(
            "\nIf the movement change was intended, re-run `{}` and commit the \
             new numbers alongside it.",
            report::COMMAND
        );
        Ok(false)
    }
}

fn write(path: &Path, contents: &str) -> Result<(), String> {
    if let Some(parent) = path.parent() {
        if !parent.as_os_str().is_empty() {
            std::fs::create_dir_all(parent)
                .map_err(|e| format!("cannot create {}: {e}", parent.display()))?;
        }
    }
    std::fs::write(path, contents).map_err(|e| format!("cannot write {}: {e}", path.display()))
}
