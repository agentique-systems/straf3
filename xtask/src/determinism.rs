//! Cross-target determinism: architecture item C2.
//!
//! # What this checks
//!
//! One reference command stream, compiled and run on every target the project
//! ships or verifies on, must produce the same bits everywhere:
//!
//! ```text
//! x86_64-unknown-linux-gnu     the native build, and the verification server
//! x86_64-unknown-linux-musl    the other side of the known fault line
//! x86_64-pc-windows-gnu        what players actually run
//! wasm32-unknown-unknown       the browser client, under Node's V8
//! ```
//!
//! Including **both** glibc and musl is what makes the check meaningful at
//! all. They are the two sides of the divergence the probe measured: Chrome,
//! Node and a musl native build agree with each other bit for bit, and glibc
//! is the odd one out. A check that ran only "native versus wasm" on a musl
//! box would go green on a tree that still had the bug.
//!
//! # Why the digest is folded over every command
//!
//! `probes/wasm-determinism` ran six cases of 1 200 commands across four
//! builds. Its `cpm-fine-turn` case diverged at command 635, differed on 29
//! of the 1 200 intermediate checksums — and its **final** checksum matched,
//! because the two trajectories happened to re-converge. An end-state
//! comparison would have certified that run as reproducible.
//!
//! So `straf3-det-runner` publishes a digest folded over every command's
//! state checksum, and this module compares those. The fold is sticky: once
//! two targets disagree on any command, the digest differs on every command
//! after it and can never come back. The per-command checksums travel in the
//! report too, so a failure names the *first* diverging command instead of
//! saying only that two targets disagree.
//!
//! # There is no golden value
//!
//! The check is relative — every target must agree with the others. Nothing
//! here stores an expected digest, so a legitimate physics change (C1's owned
//! `sin_cos`, C3's 16-bit angles) changes every number in the report and
//! requires no fixture to be re-recorded.
//!
//! # Usage
//!
//! ```text
//! cargo xtask determinism                       # build, run and compare all four
//! cargo xtask determinism --skip <triple>       # ...minus one, loudly
//! cargo xtask determinism --only <triple> --emit <file>   # one target, for CI
//! cargo xtask determinism --compare <file>...   # compare emitted reports
//! ```
//!
//! The split `--emit` / `--compare` form is what CI uses: a Linux runner
//! emits three reports, a Windows runner emits the fourth, and a third job
//! compares all four. That way the Windows binary is executed on Windows
//! rather than under an emulator.

use std::collections::BTreeSet;
use std::fmt::Write as _;
use std::path::{Path, PathBuf};
use std::process::Command;

/// The report format this xtask understands. Must match
/// `straf3_det_runner::REPORT_VERSION`; a mismatch means a stale binary.
const EXPECTED_REPORT_VERSION: u32 = 1;

/// The package that owns the reference stream.
const RUNNER_PACKAGE: &str = "straf3-det-runner";

/// How a target's artefact is executed once it is built.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Exec {
    /// Run the ELF binary directly.
    Native,
    /// Run the `.exe`. On this project's development host that is WSL
    /// interop, which runs the real Windows binary against the real Windows
    /// loader; on a Windows CI runner it is simply the native path. Wine is
    /// tried only as a fallback.
    Windows,
    /// Instantiate the `.wasm` under Node.
    Wasm,
}

/// One target of the check.
struct Target {
    triple: &'static str,
    exec: Exec,
}

/// The four targets, in report order. Changing this list changes what the
/// project promises; it is not a convenience list.
const TARGETS: &[Target] = &[
    Target {
        triple: "x86_64-unknown-linux-gnu",
        exec: Exec::Native,
    },
    Target {
        triple: "x86_64-unknown-linux-musl",
        exec: Exec::Native,
    },
    Target {
        triple: "x86_64-pc-windows-gnu",
        exec: Exec::Windows,
    },
    Target {
        triple: "wasm32-unknown-unknown",
        exec: Exec::Wasm,
    },
];

/// The pair whose disagreement was the whole reason C1 exists. If either side
/// is missing from a run, the run cannot see the known fault line and says so.
const FAULT_LINE: [&str; 2] = ["x86_64-unknown-linux-gnu", "x86_64-unknown-linux-musl"];

// ── re-deriving the digests, rather than believing them ─────────────────────

// The same FNV-1a constants `straf3-det-runner` folds with. Duplicated rather
// than imported because xtask depends on nothing by design; the parser below
// recomputes every digest it is handed, so a drift between these constants and
// the runner's shows up as "internally inconsistent", not as a silent pass.
const FNV_OFFSET: u64 = 0xcbf2_9ce4_8422_2325;
const FNV_PRIME: u64 = 0x0000_0100_0000_01b3;

fn fold(mut digest: u64, value: u64) -> u64 {
    for b in value.to_le_bytes() {
        digest ^= u64::from(b);
        digest = digest.wrapping_mul(FNV_PRIME);
    }
    digest
}

fn fold_all(values: impl IntoIterator<Item = u64>) -> u64 {
    values.into_iter().fold(FNV_OFFSET, fold)
}

// ── the parsed report ───────────────────────────────────────────────────────

/// One case's numbers, as reported by one target.
#[derive(Debug)]
pub struct CaseReport {
    pub name: String,
    pub hz: u32,
    /// The digest folded over every command. This is what decides pass/fail.
    pub rolling: u64,
    /// The state checksum after the last command — what an end-state
    /// comparison would have looked at, kept so a failure can say when such a
    /// comparison would have missed the divergence.
    pub final_checksum: u64,
    /// The state checksum after each command, in order.
    pub checksums: Vec<u64>,
}

/// A whole report from one target.
#[derive(Debug)]
pub struct Report {
    /// Where it came from, for messages: a triple, or a file path.
    pub source: String,
    pub target: String,
    pub platform: String,
    pub steps: u32,
    pub grand: u64,
    pub cases: Vec<CaseReport>,
}

impl Report {
    /// Parse the text `straf3-det-runner` prints.
    pub fn parse(text: &str, source: &str) -> Result<Self, String> {
        let mut version = None;
        let mut target = None;
        let mut platform = None;
        let mut steps = None;
        let mut grand = None;
        let mut cases: Vec<CaseReport> = Vec::new();
        let mut checksums: Vec<(usize, Vec<u64>)> = Vec::new();

        for (n, line) in text.lines().enumerate() {
            let where_ = || format!("{source}:{}", n + 1);
            let mut f = line.split_ascii_whitespace();
            match f.next() {
                None => continue,
                Some("straf3-determinism-report") => {
                    version = Some(num(f.next(), &where_())?);
                }
                Some("target") => target = Some(word(f.next(), &where_())?),
                Some("platform") => platform = Some(word(f.next(), &where_())?),
                Some("cases") => {
                    // Only checked against the number of `case` lines below.
                    let _: u32 = num(f.next(), &where_())?;
                }
                Some("steps") => steps = Some(num(f.next(), &where_())?),
                Some("grand") => grand = Some(hex(f.next(), &where_())?),
                Some("case") => {
                    let index: usize = num::<u32>(f.next(), &where_())? as usize;
                    if index != cases.len() {
                        return Err(format!(
                            "{}: case lines are out of order (expected {}, got {index})",
                            where_(),
                            cases.len()
                        ));
                    }
                    let name = word(f.next(), &where_())?;
                    expect(f.next(), "hz", &where_())?;
                    let hz = num(f.next(), &where_())?;
                    expect(f.next(), "rolling", &where_())?;
                    let rolling = hex(f.next(), &where_())?;
                    expect(f.next(), "final", &where_())?;
                    let final_checksum = hex(f.next(), &where_())?;
                    cases.push(CaseReport {
                        name,
                        hz,
                        rolling,
                        final_checksum,
                        checksums: Vec::new(),
                    });
                }
                Some("checksums") => {
                    let index: usize = num::<u32>(f.next(), &where_())? as usize;
                    let list = f
                        .next()
                        .ok_or_else(|| format!("{}: checksum list is missing", where_()))?;
                    let mut parsed = Vec::new();
                    for value in list.split(',') {
                        parsed.push(u64::from_str_radix(value, 16).map_err(|_| {
                            format!("{}: {value:?} is not a 64-bit hex checksum", where_())
                        })?);
                    }
                    checksums.push((index, parsed));
                }
                // Unknown lines are ignored on purpose: a future report may
                // add fields, and an old xtask should still be able to
                // compare the fields it knows.
                Some(_) => {}
            }
        }

        let version: u32 = version
            .ok_or_else(|| format!("{source}: not a determinism report (no header line)"))?;
        if version != EXPECTED_REPORT_VERSION {
            return Err(format!(
                "{source}: report format v{version}, but this xtask speaks v{EXPECTED_REPORT_VERSION} \
                 — rebuild {RUNNER_PACKAGE}, or the artefact is stale"
            ));
        }
        for (index, list) in checksums {
            let case = cases
                .get_mut(index)
                .ok_or_else(|| format!("{source}: checksums for unknown case {index}"))?;
            case.checksums = list;
        }
        if cases.is_empty() {
            return Err(format!("{source}: report contains no cases"));
        }
        let steps: u32 = steps.ok_or_else(|| format!("{source}: no `steps` line"))?;
        for case in &cases {
            if case.checksums.len() != steps as usize {
                return Err(format!(
                    "{source}: case {} has {} per-command checksums, expected {steps} \
                     — the report is truncated, and a truncated report must never be \
                     compared as if it agreed",
                    case.name,
                    case.checksums.len()
                ));
            }
        }

        // `compare` decides pass/fail on `rolling`, but every diagnostic it
        // prints — the first diverging command, how many differ, whether the
        // finals agree — is read off `checksums`. Nothing so far has checked
        // that the two describe the same run. A report that is truncated
        // mid-write, merged from a stale artefact, or hand-edited can carry a
        // `rolling` that agrees with another target while its per-command
        // checksums do not, and the check would call that "bit for bit". So
        // recompute every digest from the per-command checksums and refuse the
        // report if it does not add up. The number compared is then provably
        // the number the evidence supports.
        let grand = grand.ok_or_else(|| format!("{source}: no `grand` line"))?;
        for case in &cases {
            let recomputed = fold_all(case.checksums.iter().copied());
            if recomputed != case.rolling {
                return Err(format!(
                    "{source}: case {} reports rolling digest {:016x}, but folding its own \
                     {} per-command checksums gives {recomputed:016x} — the report contradicts \
                     itself and must not be compared as if it agreed",
                    case.name,
                    case.rolling,
                    case.checksums.len()
                ));
            }
            let last = *case
                .checksums
                .last()
                .expect("the steps check above rejects an empty case");
            if last != case.final_checksum {
                return Err(format!(
                    "{source}: case {} reports final checksum {:016x}, but its last per-command \
                     checksum is {last:016x} — the report contradicts itself",
                    case.name, case.final_checksum
                ));
            }
        }
        let recomputed_grand = fold_all(cases.iter().map(|c| c.rolling));
        if recomputed_grand != grand {
            return Err(format!(
                "{source}: reports grand digest {grand:016x}, but folding its {} case digests \
                 gives {recomputed_grand:016x} — the report contradicts itself",
                cases.len()
            ));
        }

        Ok(Self {
            source: source.to_string(),
            target: target.ok_or_else(|| format!("{source}: no `target` line"))?,
            platform: platform.unwrap_or_else(|| "unknown".to_string()),
            steps,
            grand,
            cases,
        })
    }
}

fn word(v: Option<&str>, where_: &str) -> Result<String, String> {
    v.map(str::to_string)
        .ok_or_else(|| format!("{where_}: expected a value"))
}

fn num<T: std::str::FromStr>(v: Option<&str>, where_: &str) -> Result<T, String> {
    word(v, where_)?
        .parse()
        .map_err(|_| format!("{where_}: expected a number"))
}

fn hex(v: Option<&str>, where_: &str) -> Result<u64, String> {
    let raw = word(v, where_)?;
    u64::from_str_radix(&raw, 16).map_err(|_| format!("{where_}: {raw:?} is not 64-bit hex"))
}

fn expect(v: Option<&str>, want: &str, where_: &str) -> Result<(), String> {
    match v {
        Some(got) if got == want => Ok(()),
        other => Err(format!("{where_}: expected {want:?}, got {other:?}")),
    }
}

// ── comparison ──────────────────────────────────────────────────────────────

/// Two targets disagreeing about one case.
pub struct Divergence {
    pub case: String,
    pub left: String,
    pub right: String,
    pub left_rolling: u64,
    pub right_rolling: u64,
    /// Zero-based index of the first command whose state checksum differs.
    /// `None` only if the rolling digests differ while every per-command
    /// checksum agrees, which would mean the runner itself is broken.
    pub first_command: Option<usize>,
    pub differing_commands: usize,
    pub total_commands: usize,
    pub command_millis: u32,
    /// True when the *final* state checksums agree despite the divergence —
    /// the reconvergence case, and the reason this check exists.
    pub finals_agree: bool,
}

impl Divergence {
    fn render(&self, o: &mut String) {
        let _ = writeln!(o, "  DIVERGENCE  case {}", self.case);
        let _ = writeln!(o, "    {} vs {}", self.left, self.right);
        match self.first_command {
            Some(i) => {
                let t = (i as u64 + 1) * u64::from(self.command_millis);
                let _ = writeln!(
                    o,
                    "    first differing command #{i} of {} (t = {t} ms)",
                    self.total_commands
                );
            }
            None => {
                let _ = writeln!(
                    o,
                    "    rolling digests differ but no per-command checksum does — \
                     the report or the digest is broken, not the simulation"
                );
            }
        }
        let _ = writeln!(
            o,
            "    {} of {} commands differ",
            self.differing_commands, self.total_commands
        );
        let _ = writeln!(
            o,
            "    rolling digest {:016x} vs {:016x}",
            self.left_rolling, self.right_rolling
        );
        if self.finals_agree {
            let _ = writeln!(
                o,
                "    NOTE: the FINAL state checksums are identical. An end-state\n\
                 \x20         comparison would have called this run reproducible. This is\n\
                 \x20         exactly the reconvergence the probe measured, and exactly\n\
                 \x20         why the check folds a digest over every command."
            );
        }
    }
}

/// The outcome of comparing two or more reports.
pub struct Comparison {
    pub divergences: Vec<Divergence>,
    /// Problems that are not simulation divergences: a report with different
    /// cases, a different command count, a different report version.
    pub structural: Vec<String>,
    pub targets: Vec<String>,
    pub cases: usize,
    pub steps: u32,
    pub skipped: Vec<String>,
}

impl Comparison {
    #[must_use]
    pub fn is_clean(&self) -> bool {
        self.divergences.is_empty() && self.structural.is_empty()
    }

    #[must_use]
    pub fn render(&self) -> String {
        let mut o = String::new();
        for problem in &self.structural {
            let _ = writeln!(o, "  REPORTS DISAGREE STRUCTURALLY: {problem}");
        }
        for d in &self.divergences {
            d.render(&mut o);
            o.push('\n');
        }
        if self.is_clean() {
            let _ = writeln!(
                o,
                "  {} targets agree, bit for bit, over {} cases x {} commands:",
                self.targets.len(),
                self.cases,
                self.steps
            );
            for t in &self.targets {
                let _ = writeln!(o, "    {t}");
            }
        }
        if !self.skipped.is_empty() {
            let _ = writeln!(o, "\n  NOT A FULL CHECK — these targets were skipped:");
            for t in &self.skipped {
                let _ = writeln!(o, "    {t}");
            }
            if FAULT_LINE
                .iter()
                .any(|f| self.skipped.iter().any(|s| s == f))
            {
                let _ = writeln!(
                    o,
                    "  glibc and musl are the two sides of the known fault line. With one\n  \
                     of them missing, this run cannot see the divergence C1 exists to fix."
                );
            }
        }
        o
    }
}

/// Compare every report against the first one.
///
/// Comparing against a reference rather than every pair is deliberate: with
/// four targets, all-pairs reporting turns one real divergence into three
/// near-identical failures and buries the first diverging command.
#[must_use]
pub fn compare(reports: &[Report], skipped: Vec<String>) -> Comparison {
    let mut divergences = Vec::new();
    let mut structural = Vec::new();
    let reference = &reports[0];

    for other in &reports[1..] {
        if other.steps != reference.steps {
            structural.push(format!(
                "{} ran {} commands per case, {} ran {}",
                reference.source, reference.steps, other.source, other.steps
            ));
            continue;
        }
        if other.cases.len() != reference.cases.len() {
            structural.push(format!(
                "{} has {} cases, {} has {}",
                reference.source,
                reference.cases.len(),
                other.source,
                other.cases.len()
            ));
            continue;
        }
        for (a, b) in reference.cases.iter().zip(&other.cases) {
            if a.name != b.name || a.hz != b.hz {
                structural.push(format!(
                    "case order differs: {} has {} at {} Hz where {} has {} at {} Hz",
                    reference.source, a.name, a.hz, other.source, b.name, b.hz
                ));
                continue;
            }
            if a.rolling == b.rolling {
                continue;
            }
            let first = a
                .checksums
                .iter()
                .zip(&b.checksums)
                .position(|(x, y)| x != y);
            let differing = a
                .checksums
                .iter()
                .zip(&b.checksums)
                .filter(|(x, y)| x != y)
                .count();
            divergences.push(Divergence {
                case: a.name.clone(),
                left: reference.target.clone(),
                right: other.target.clone(),
                left_rolling: a.rolling,
                right_rolling: b.rolling,
                first_command: first,
                differing_commands: differing,
                total_commands: a.checksums.len(),
                command_millis: 1000 / a.hz.max(1),
                finals_agree: a.final_checksum == b.final_checksum,
            });
        }
    }

    Comparison {
        divergences,
        structural,
        targets: reports.iter().map(|r| r.target.clone()).collect(),
        cases: reference.cases.len(),
        steps: reference.steps,
        skipped,
    }
}

// ── building and running ────────────────────────────────────────────────────

fn workspace_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("xtask/ always has a parent")
        .to_path_buf()
}

fn cargo() -> String {
    std::env::var("CARGO").unwrap_or_else(|_| "cargo".to_string())
}

/// Targets rustup has installed, or `None` if rustup could not be consulted.
fn installed_targets() -> Option<BTreeSet<String>> {
    let out = Command::new("rustup")
        .args(["target", "list", "--installed"])
        .output()
        .ok()?;
    if !out.status.success() {
        return None;
    }
    Some(
        String::from_utf8_lossy(&out.stdout)
            .lines()
            .map(|l| l.trim().to_string())
            .filter(|l| !l.is_empty())
            .collect(),
    )
}

fn build(target: &Target) -> Result<(), String> {
    let root = workspace_root();
    let mut cmd = Command::new(cargo());
    cmd.current_dir(&root)
        .args(["build", "--release", "-p", RUNNER_PACKAGE])
        .args(["--target", target.triple]);
    // On wasm the binary would compile but has nothing to run in; the library
    // is what carries the exports.
    if target.exec == Exec::Wasm {
        cmd.arg("--lib");
    } else {
        cmd.args(["--bin", RUNNER_PACKAGE]);
    }

    let status = cmd
        .status()
        .map_err(|e| format!("could not run {}: {e}", cargo()))?;
    if !status.success() {
        return Err(format!(
            "building {RUNNER_PACKAGE} for {} failed. If the target itself is missing:\n    \
             rustup target add {}",
            target.triple, target.triple
        ));
    }
    Ok(())
}

fn artefact(target: &Target) -> PathBuf {
    let dir = workspace_root()
        .join("target")
        .join(target.triple)
        .join("release");
    match target.exec {
        Exec::Native => dir.join(RUNNER_PACKAGE),
        Exec::Windows => dir.join(format!("{RUNNER_PACKAGE}.exe")),
        // Cargo replaces `-` with `_` in library file names.
        Exec::Wasm => dir.join(format!("{}.wasm", RUNNER_PACKAGE.replace('-', "_"))),
    }
}

/// Run the built artefact and return the report text it printed.
fn execute(target: &Target) -> Result<String, String> {
    let path = artefact(target);
    if !path.exists() {
        return Err(format!("{} was not produced", path.display()));
    }

    let attempts: Vec<(String, Vec<String>)> = match target.exec {
        Exec::Native => vec![(path.display().to_string(), vec![])],
        // The .exe first: on the development host WSL interop runs it against
        // the real Windows loader, and on a Windows runner it is simply
        // native. Wine only if that fails, because wine is an emulator and a
        // determinism result from an emulator is worth less.
        Exec::Windows => vec![
            (path.display().to_string(), vec![]),
            ("wine64".to_string(), vec![path.display().to_string()]),
            ("wine".to_string(), vec![path.display().to_string()]),
        ],
        Exec::Wasm => vec![(
            "node".to_string(),
            vec![
                workspace_root()
                    .join("tools/det-runner/web/run-node.mjs")
                    .display()
                    .to_string(),
                path.display().to_string(),
            ],
        )],
    };

    let mut failures = Vec::new();
    for (program, args) in &attempts {
        match Command::new(program).args(args).output() {
            Ok(out) => {
                let stderr = String::from_utf8_lossy(&out.stderr);
                if !stderr.trim().is_empty() {
                    for line in stderr.lines() {
                        eprintln!("    [{}] {line}", target.triple);
                    }
                }
                if out.status.success() {
                    return String::from_utf8(out.stdout)
                        .map_err(|_| format!("{program}: report was not UTF-8"));
                }
                failures.push(format!("{program} exited with {}", out.status));
            }
            Err(e) => failures.push(format!("{program}: {e}")),
        }
    }

    Err(format!(
        "could not run the {} artefact. Tried:\n      {}\n    {}",
        target.triple,
        failures.join("\n      "),
        hint(target)
    ))
}

fn hint(target: &Target) -> &'static str {
    match target.exec {
        Exec::Windows => {
            "The Windows build runs directly under WSL interop on this project's host; \n    \
             elsewhere install wine, or emit the report on a Windows machine with\n    \
             `cargo xtask determinism --only x86_64-pc-windows-gnu --emit <file>`\n    \
             and compare the files with `--compare`."
        }
        Exec::Wasm => {
            "The wasm target is executed under Node (V8 — the same engine family the\n    \
             browser runs). Install node, or emit the report elsewhere and use\n    \
             `--compare`."
        }
        Exec::Native => "",
    }
}

// ── the command ─────────────────────────────────────────────────────────────

/// What `cargo xtask determinism` was asked to do.
struct Args {
    only: Vec<String>,
    skip: Vec<String>,
    emit: Option<PathBuf>,
    compare: Vec<PathBuf>,
}

fn parse_args(argv: &[String]) -> Result<Args, String> {
    let mut args = Args {
        only: Vec::new(),
        skip: Vec::new(),
        emit: None,
        compare: Vec::new(),
    };
    let mut it = argv.iter();
    while let Some(arg) = it.next() {
        let mut value = |flag: &str| {
            it.next()
                .cloned()
                .ok_or_else(|| format!("{flag} needs a value"))
        };
        match arg.as_str() {
            "--only" => args.only.push(value("--only")?),
            "--skip" => args.skip.push(value("--skip")?),
            "--emit" => args.emit = Some(PathBuf::from(value("--emit")?)),
            "--compare" => args.compare.push(PathBuf::from(value("--compare")?)),
            // Bare paths after --compare, so `--compare a.txt b.txt` works as
            // it reads.
            other if !other.starts_with('-') && !args.compare.is_empty() => {
                args.compare.push(PathBuf::from(other));
            }
            other => return Err(format!("unknown argument {other:?}")),
        }
    }
    for triple in args.only.iter().chain(&args.skip) {
        if !TARGETS.iter().any(|t| t.triple == triple) {
            return Err(format!(
                "{triple} is not one of the checked targets:\n    {}",
                TARGETS
                    .iter()
                    .map(|t| t.triple)
                    .collect::<Vec<_>>()
                    .join("\n    ")
            ));
        }
    }
    if args.emit.is_some() && args.only.len() != 1 {
        return Err(
            "--emit writes one target's report, so it needs exactly one --only".to_string(),
        );
    }
    if !args.compare.is_empty() && (!args.only.is_empty() || args.emit.is_some()) {
        return Err("--compare reads reports from files; it does not build anything".to_string());
    }
    Ok(args)
}

/// Run the check. `Ok(true)` means every report agreed.
pub fn run(argv: &[String]) -> Result<bool, String> {
    let args = parse_args(argv)?;

    if !args.compare.is_empty() {
        return compare_files(&args.compare);
    }

    let selected: Vec<&Target> = TARGETS
        .iter()
        .filter(|t| args.only.is_empty() || args.only.iter().any(|o| o == t.triple))
        .filter(|t| !args.skip.iter().any(|s| s == t.triple))
        .collect();
    if selected.is_empty() {
        return Err("every target was filtered out; nothing to check".to_string());
    }
    let skipped: Vec<String> = TARGETS
        .iter()
        .filter(|t| !selected.iter().any(|s| s.triple == t.triple))
        .map(|t| t.triple.to_string())
        .collect();

    if let Some(installed) = installed_targets() {
        let missing: Vec<&str> = selected
            .iter()
            .map(|t| t.triple)
            .filter(|t| !installed.contains(*t))
            .collect();
        if !missing.is_empty() {
            return Err(format!(
                "these targets are not installed:\n    {}\n\n  Install them:\n    rustup target add {}",
                missing.join("\n    "),
                missing.join(" ")
            ));
        }
    }

    let mut reports = Vec::new();
    for target in &selected {
        println!("  building {} ...", target.triple);
        build(target)?;
        println!("  running  {} ...", target.triple);
        let text = execute(target)?;
        let report = Report::parse(&text, target.triple)?;
        if report.target != target.triple {
            return Err(format!(
                "the {} artefact reports itself as {} — a stale binary in target/{}/release?",
                target.triple, report.target, target.triple
            ));
        }
        println!(
            "    {} ({}), grand digest {:016x}",
            report.target, report.platform, report.grand
        );
        if let Some(path) = &args.emit {
            std::fs::write(path, &text)
                .map_err(|e| format!("could not write {}: {e}", path.display()))?;
            println!("    report written to {}", path.display());
        }
        reports.push(report);
    }

    if reports.len() < 2 {
        println!(
            "\n  Only {} target was run, so nothing was compared. This is the --emit\n  \
             form: compare the files afterwards with `cargo xtask determinism\n  \
             --compare <file> <file> ...`.",
            reports.len()
        );
        return Ok(true);
    }

    finish(compare(&reports, skipped))
}

fn compare_files(paths: &[PathBuf]) -> Result<bool, String> {
    if paths.len() < 2 {
        return Err("--compare needs at least two reports to compare".to_string());
    }
    let mut reports = Vec::new();
    for path in paths {
        let text = std::fs::read_to_string(path)
            .map_err(|e| format!("could not read {}: {e}", path.display()))?;
        let report = Report::parse(&text, &path.display().to_string())?;
        println!(
            "  {} — {} ({}), grand digest {:016x}",
            path.display(),
            report.target,
            report.platform,
            report.grand
        );
        reports.push(report);
    }

    // Duplicate targets are a CI wiring accident — two jobs uploading the
    // same artefact — and would otherwise look like a passing check.
    let mut seen = BTreeSet::new();
    for report in &reports {
        if !seen.insert(report.target.clone()) {
            return Err(format!(
                "{} appears twice. Comparing a report against itself proves nothing.",
                report.target
            ));
        }
    }
    let skipped: Vec<String> = TARGETS
        .iter()
        .map(|t| t.triple.to_string())
        .filter(|t| !seen.contains(t))
        .collect();

    finish(compare(&reports, skipped))
}

fn finish(comparison: Comparison) -> Result<bool, String> {
    println!();
    print!("{}", comparison.render());
    Ok(comparison.is_clean())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn report_text(target: &str, checksums: &[&[u64]]) -> String {
        let fold = |xs: &[u64]| fold_all(xs.iter().copied());
        let mut o = String::new();
        let _ = writeln!(o, "straf3-determinism-report {EXPECTED_REPORT_VERSION}");
        let _ = writeln!(o, "target {target}");
        let _ = writeln!(o, "platform test");
        let _ = writeln!(o, "cases {}", checksums.len());
        let _ = writeln!(o, "steps {}", checksums[0].len());
        // A well-formed report: the parser now recomputes these and rejects a
        // report whose digests do not follow from its own checksums.
        let _ = writeln!(
            o,
            "grand {:016x}",
            fold_all(checksums.iter().map(|c| fold(c)))
        );
        for (i, case) in checksums.iter().enumerate() {
            let _ = writeln!(
                o,
                "case {i} case-{i} hz 125 rolling {:016x} final {:016x}",
                fold(case),
                case.last().unwrap()
            );
        }
        for (i, case) in checksums.iter().enumerate() {
            let list: Vec<String> = case.iter().map(|c| format!("{c:016x}")).collect();
            let _ = writeln!(o, "checksums {i} {}", list.join(","));
        }
        o
    }

    #[test]
    fn identical_reports_agree() {
        let text = report_text("x86_64-unknown-linux-gnu", &[&[1, 2, 3, 4]]);
        let a = Report::parse(&text, "a").unwrap();
        let b = Report::parse(
            &report_text("x86_64-unknown-linux-musl", &[&[1, 2, 3, 4]]),
            "b",
        )
        .unwrap();
        let c = compare(&[a, b], vec![]);
        assert!(c.is_clean(), "{}", c.render());
    }

    /// `compare` decides on `rolling`; every diagnostic it prints comes from
    /// `checksums`. If a report can carry the two disagreeing, then a
    /// truncated or stale artefact can pass the check while the evidence
    /// underneath it says otherwise. Measured against a real emitted report
    /// before this was added: tampering with one of 1,200 per-command
    /// checksums and leaving `rolling` alone produced "2 targets agree, bit
    /// for bit" and exit 0.
    #[test]
    fn a_report_whose_rolling_digest_contradicts_its_checksums_is_refused() {
        let honest = report_text("x86_64-unknown-linux-gnu", &[&[1, 2, 3, 4, 5]]);
        // Change one mid-stream checksum, leave every digest line as it was.
        let tampered = honest.replacen(
            &format!("{:016x},{:016x}", 3u64, 4u64),
            &format!("{:016x},{:016x}", 99u64, 4u64),
            1,
        );
        assert_ne!(tampered, honest, "the tamper did not apply");
        let err = Report::parse(&tampered, "tampered").unwrap_err();
        assert!(
            err.contains("contradicts itself") && err.contains("rolling digest"),
            "wrong rejection: {err}"
        );
    }

    #[test]
    fn a_report_whose_final_checksum_is_not_its_last_checksum_is_refused() {
        let honest = report_text("x86_64-unknown-linux-gnu", &[&[1, 2, 3, 4, 5]]);
        let tampered = honest.replace(
            &format!("final {:016x}", 5u64),
            &format!("final {:016x}", 7u64),
        );
        assert_ne!(tampered, honest, "the tamper did not apply");
        let err = Report::parse(&tampered, "tampered").unwrap_err();
        assert!(err.contains("final checksum"), "wrong rejection: {err}");
    }

    #[test]
    fn a_report_whose_grand_digest_does_not_follow_from_its_cases_is_refused() {
        let honest = report_text("x86_64-unknown-linux-gnu", &[&[1, 2, 3], &[4, 5, 6]]);
        let grand = honest
            .lines()
            .find(|l| l.starts_with("grand "))
            .unwrap()
            .to_string();
        let tampered = honest.replace(&grand, "grand 0000000000000001");
        let err = Report::parse(&tampered, "tampered").unwrap_err();
        assert!(err.contains("grand digest"), "wrong rejection: {err}");
    }

    /// The guard must not be so eager that it rejects honest reports — the
    /// four this tool actually emits parse, and the constants above therefore
    /// match the runner's.
    #[test]
    fn an_honest_report_still_parses() {
        let text = report_text("x86_64-unknown-linux-gnu", &[&[1, 2, 3], &[4, 5, 6]]);
        let report = Report::parse(&text, "honest").unwrap();
        assert_eq!(report.cases.len(), 2);
        assert_eq!(
            report.grand,
            fold_all(report.cases.iter().map(|c| c.rolling))
        );
    }

    /// The case the whole design exists for: the trajectories differ in the
    /// middle and land on the same final state.
    #[test]
    fn a_divergence_that_reconverges_is_still_caught() {
        let a = Report::parse(
            &report_text("x86_64-unknown-linux-gnu", &[&[1, 2, 3, 4, 5]]),
            "a",
        )
        .unwrap();
        let b = Report::parse(
            &report_text("x86_64-unknown-linux-musl", &[&[1, 2, 99, 4, 5]]),
            "b",
        )
        .unwrap();

        // An end-state comparison would have passed.
        assert_eq!(a.cases[0].final_checksum, b.cases[0].final_checksum);

        let c = compare(&[a, b], vec![]);
        assert!(!c.is_clean());
        let d = &c.divergences[0];
        assert_eq!(d.first_command, Some(2));
        assert_eq!(d.differing_commands, 1);
        assert!(d.finals_agree);
        assert!(c.render().contains("end-state"), "{}", c.render());
    }

    #[test]
    fn the_first_diverging_command_is_named_not_just_the_fact() {
        let mut left = vec![7u64; 1200];
        let mut right = left.clone();
        for (i, v) in right.iter_mut().enumerate().skip(677) {
            *v = 9 + i as u64;
        }
        left[0] = 1;
        right[0] = 1;
        let a = Report::parse(&report_text("x86_64-unknown-linux-gnu", &[&left]), "a").unwrap();
        let b = Report::parse(&report_text("wasm32-unknown-unknown", &[&right]), "b").unwrap();
        let c = compare(&[a, b], vec![]);
        let rendered = c.render();
        assert!(!c.is_clean());
        assert!(
            rendered.contains("first differing command #677") && rendered.contains("t = 5424 ms"),
            "{rendered}"
        );
    }

    #[test]
    fn a_truncated_report_is_rejected_rather_than_compared() {
        let text =
            report_text("x86_64-unknown-linux-gnu", &[&[1, 2, 3, 4]]).replace("steps 4", "steps 5");
        let err = Report::parse(&text, "a").unwrap_err();
        assert!(err.contains("truncated"), "{err}");
    }

    #[test]
    fn a_report_from_another_format_version_is_rejected() {
        let text = report_text("x86_64-unknown-linux-gnu", &[&[1, 2]]).replace(
            "straf3-determinism-report 1",
            "straf3-determinism-report 99",
        );
        let err = Report::parse(&text, "a").unwrap_err();
        assert!(err.contains("stale"), "{err}");
    }

    #[test]
    fn reports_with_different_cases_are_a_structural_failure_not_a_pass() {
        let a = Report::parse(
            &report_text("x86_64-unknown-linux-gnu", &[&[1, 2], &[3, 4]]),
            "a",
        )
        .unwrap();
        let b = Report::parse(&report_text("x86_64-unknown-linux-musl", &[&[1, 2]]), "b").unwrap();
        let c = compare(&[a, b], vec![]);
        assert!(!c.is_clean());
        assert!(c.render().contains("STRUCTURALLY"), "{}", c.render());
    }

    #[test]
    fn skipping_a_side_of_the_fault_line_is_said_out_loud() {
        let a = Report::parse(&report_text("x86_64-unknown-linux-gnu", &[&[1, 2]]), "a").unwrap();
        let b = Report::parse(&report_text("wasm32-unknown-unknown", &[&[1, 2]]), "b").unwrap();
        let c = compare(&[a, b], vec!["x86_64-unknown-linux-musl".to_string()]);
        assert!(c.is_clean());
        let rendered = c.render();
        assert!(rendered.contains("NOT A FULL CHECK"), "{rendered}");
        assert!(rendered.contains("fault line"), "{rendered}");
    }

    #[test]
    fn arguments_that_would_weaken_the_check_are_refused() {
        assert!(parse_args(&["--only".into(), "not-a-target".into()]).is_err());
        assert!(parse_args(&["--emit".into(), "x.txt".into()]).is_err());
        assert!(
            parse_args(&[
                "--compare".into(),
                "a.txt".into(),
                "--only".into(),
                "x86_64-unknown-linux-gnu".into()
            ])
            .is_err()
        );
        let ok = parse_args(&["--compare".into(), "a.txt".into(), "b.txt".into()]).unwrap();
        assert_eq!(ok.compare.len(), 2);
    }

    #[test]
    fn every_declared_target_has_a_way_to_be_run() {
        assert_eq!(TARGETS.len(), 4);
        for t in TARGETS {
            assert!(!artefact(t).as_os_str().is_empty());
        }
        for side in FAULT_LINE {
            assert!(TARGETS.iter().any(|t| t.triple == side));
        }
    }
}
