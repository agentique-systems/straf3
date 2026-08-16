//! Keeping `probes/` from rotting silently.
//!
//! # The failure this exists to stop
//!
//! Every crate under `probes/` is standalone: each carries its own
//! `[workspace]` table and its own `Cargo.lock`, deliberately, so a probe
//! measures what it says it measures and cannot be perturbed by the root
//! workspace's feature unification. The cost of that isolation is that
//! `cargo build --workspace`, `cargo test --workspace` and
//! `cargo clippy --workspace` never compile a single line of them.
//!
//! That is not hypothetical. C3 changed `ViewAngles` from three `Scalar`
//! fields to three `u16` at the command boundary. Every producer of a view
//! angle inside the workspace was updated and the workspace stayed green —
//! while two probes that path-depend on `straf3-sim` stopped compiling and
//! nobody found out, because nothing in the repository ever built them. One
//! of those two was `probes/wasm-determinism`, which is the only path that
//! exercises wasm under a real browser. While it was broken, browser parity
//! could not be checked at all, and the tree still looked green.
//!
//! So: `cargo xtask check-probes`.
//!
//! # Discovery is by directory listing, not by a list
//!
//! A hardcoded list of probe names is the same bug one level up — it goes
//! stale the first time somebody adds a probe and forgets to register it,
//! and it goes stale invisibly. This module reads `probes/`, treats every
//! subdirectory holding a `Cargo.toml` as a probe, and checks it. A probe
//! added tomorrow is covered tomorrow, with no edit here.
//!
//! # Coupling, and why it is reported rather than used to skip
//!
//! A probe can only be broken by a change to this tree if it reaches into
//! this tree, which it does through a `path = "..."` dependency on a crate
//! under `crates/` or `tools/` — possibly indirectly, via another probe.
//! This module works that relation out from the manifests and prints it, so
//! a reader can see *why* each probe is in the list.
//!
//! It is reported, not used as a filter. Compiling a probe that only depends
//! on registry crates still costs little and still catches toolchain drift,
//! and a filter is exactly the kind of thing that quietly stops covering
//! something. The one thing that is genuinely not covered by default —
//! `optional` path dependencies, which `cargo check` does not compile unless
//! the feature is on — is named explicitly in the report, every run, rather
//! than left for a reader to infer. `--all-features` turns it into coverage.

use std::collections::{BTreeMap, BTreeSet};
use std::fmt::Write as _;
use std::path::{Path, PathBuf};
use std::process::Command;

/// Directories under the repository root that constitute "this tree" for the
/// purpose of coupling. A probe with a path dependency reaching one of these
/// is a probe that a change to this tree can break.
const WORKSPACE_CODE_DIRS: [&str; 2] = ["crates", "tools"];

// ── what we found on disk ───────────────────────────────────────────────────

/// One crate under `probes/`.
#[derive(Debug, Clone)]
pub struct Probe {
    /// Directory name under `probes/`, which is what the user names on the
    /// command line. Not the package name — they differ (`wasm-determinism`
    /// is package `wasm-determinism-probe`).
    pub dir: String,
    pub manifest: PathBuf,
    /// Path dependencies declared by this probe's own manifest.
    pub path_deps: Vec<PathDep>,
}

/// A single `path = "..."` dependency edge out of a probe's manifest.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PathDep {
    /// Where it points, resolved against the probe directory and normalised.
    /// Not canonicalised: a dangling path should be reported, not silently
    /// dropped, and `canonicalize` fails on those.
    pub target: PathBuf,
    /// `optional = true`, i.e. only compiled when some feature enables it.
    /// This is the one hole in default-feature coverage, so it is tracked.
    pub optional: bool,
}

/// How a probe is attached to the tree, once indirect edges are followed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Coupling {
    /// Reaches `crates/` or `tools/` through non-optional path dependencies.
    /// A change to the workspace can break this probe's build.
    Direct,
    /// Reaches the tree only through `optional` path dependencies, so a
    /// default-feature `cargo check` does not compile the coupling at all.
    FeatureGated,
    /// No path dependency reaches the tree. Registry crates only.
    Independent,
}

impl Coupling {
    fn label(self) -> &'static str {
        match self {
            Coupling::Direct => "couples to the tree",
            Coupling::FeatureGated => "couples only behind a feature",
            Coupling::Independent => "independent of the tree",
        }
    }
}

// ── manifest scanning ───────────────────────────────────────────────────────

/// Pull the `path = "..."` dependency edges out of a Cargo manifest.
///
/// Hand-written rather than `cargo metadata` + serde_json on purpose: xtask
/// declares zero dependencies so it can run on a cold registry, and
/// `cargo metadata` on a probe would resolve and download that probe's whole
/// dependency graph just to tell us where its path edges point.
///
/// Both spellings Cargo accepts are handled:
///
/// ```toml
/// [dependencies]
/// straf3-sim = { path = "../../crates/straf3-sim" }
///
/// [dependencies.straf3-render]
/// path = "../../crates/straf3-render"
/// optional = true
/// ```
pub fn scan_path_deps(manifest_text: &str, probe_dir: &Path) -> Vec<PathDep> {
    let mut found = Vec::new();
    // Set while inside `[dependencies.NAME]`-style tables, where `path` and
    // `optional` arrive on separate lines and must be paired up.
    let mut pending: Option<PathDep> = None;
    let mut in_dep_table = false;

    let flush = |pending: &mut Option<PathDep>, found: &mut Vec<PathDep>| {
        if let Some(d) = pending.take() {
            found.push(d);
        }
    };

    for raw in manifest_text.lines() {
        let line = strip_comment(raw).trim();

        if line.starts_with('[') {
            flush(&mut pending, &mut found);
            in_dep_table = is_dependency_section(line);
            // `[dependencies.NAME]` opens a table whose `path`/`optional`
            // lines follow; `[dependencies]` opens inline-table entries.
            continue;
        }
        if !in_dep_table {
            continue;
        }

        if let Some(p) = value_of(line, "path") {
            let dep = PathDep {
                target: normalise(&probe_dir.join(p)),
                optional: matches!(value_of(line, "optional").as_deref(), Some("true")),
            };
            // A line that *starts* with `path` is the table form: it belongs
            // to an open `[dependencies.NAME]`, and its `optional = true` may
            // still be on a later line, so it has to stay open. Anything else
            // is the inline form and is complete as it stands.
            if line.starts_with("path") {
                flush(&mut pending, &mut found);
                pending = Some(dep);
            } else {
                found.push(dep);
            }
            continue;
        }

        // Table form: `optional = true` on its own line, after `path`.
        if let Some(v) = value_of(line, "optional")
            && v == "true"
            && let Some(d) = pending.as_mut()
        {
            d.optional = true;
        }
    }
    flush(&mut pending, &mut found);
    found
}

/// True for the manifest sections in which a `path` key means a dependency.
///
/// Covers `[target.'cfg(...)'.dependencies]` and the `dev`/`build` variants,
/// because a probe that only breaks in its dev-dependencies is still broken.
fn is_dependency_section(header: &str) -> bool {
    let h = header.trim_start_matches('[').trim_end_matches(']');
    // The last component is what names the kind: `target.'cfg(..)'.dependencies`
    // and `dependencies.straf3-render` both end in something we care about,
    // but only the former ends in a dependency *kind*.
    h.split('.').any(|seg| {
        matches!(
            seg.trim().trim_matches('"'),
            "dependencies" | "dev-dependencies" | "build-dependencies"
        )
    })
}

/// `key = "value"` → `Some("value")`, for the one line given. Returns the
/// value unquoted. Only matches the key as a whole word.
fn value_of(line: &str, key: &str) -> Option<String> {
    let mut rest = line;
    loop {
        let at = rest.find(key)?;
        let before_ok = at == 0
            || !rest[..at]
                .chars()
                .next_back()
                .is_some_and(|c| c.is_alphanumeric() || c == '-' || c == '_');
        let after = &rest[at + key.len()..];
        let after_trimmed = after.trim_start();
        if before_ok && after_trimmed.starts_with('=') {
            let v = after_trimmed[1..].trim_start();
            return Some(if let Some(q) = v.strip_prefix('"') {
                q.split('"').next().unwrap_or("").to_string()
            } else {
                v.split([',', '}', ' '])
                    .next()
                    .unwrap_or("")
                    .trim()
                    .to_string()
            });
        }
        rest = &rest[at + key.len()..];
    }
}

/// Drop a trailing `#` comment, respecting quotes so a `#` inside a string
/// (`cfg(target_os = "...")`, paths) does not truncate the line.
fn strip_comment(line: &str) -> &str {
    let mut in_quotes = false;
    for (i, c) in line.char_indices() {
        match c {
            '"' => in_quotes = !in_quotes,
            '#' if !in_quotes => return &line[..i],
            _ => {}
        }
    }
    line
}

/// Resolve `..` and `.` textually. Not `canonicalize`: a path dependency
/// pointing at something that does not exist is a finding to report, and
/// `canonicalize` would turn it into an I/O error instead.
fn normalise(p: &Path) -> PathBuf {
    let mut out = PathBuf::new();
    for c in p.components() {
        match c {
            std::path::Component::ParentDir => {
                out.pop();
            }
            std::path::Component::CurDir => {}
            other => out.push(other.as_os_str()),
        }
    }
    out
}

// ── discovery ───────────────────────────────────────────────────────────────

/// Every crate under `probes/`, found by listing the directory.
pub fn discover(probes_root: &Path) -> Result<Vec<Probe>, String> {
    let entries = std::fs::read_dir(probes_root)
        .map_err(|e| format!("could not list {}: {e}", probes_root.display()))?;

    // BTreeMap so the report is in a stable, alphabetical order run to run —
    // readdir order is not stable and a shuffling report is hard to diff.
    let mut probes = BTreeMap::new();
    for entry in entries {
        let entry = entry.map_err(|e| format!("could not read an entry in probes/: {e}"))?;
        if !entry.file_type().is_ok_and(|t| t.is_dir()) {
            continue;
        }
        let manifest = entry.path().join("Cargo.toml");
        if !manifest.is_file() {
            continue;
        }
        let dir = entry.file_name().to_string_lossy().into_owned();
        let text = std::fs::read_to_string(&manifest)
            .map_err(|e| format!("could not read {}: {e}", manifest.display()))?;
        let path_deps = scan_path_deps(&text, &entry.path());
        probes.insert(
            dir.clone(),
            Probe {
                dir,
                manifest,
                path_deps,
            },
        );
    }

    if probes.is_empty() {
        return Err(format!(
            "no crates found under {} — this check discovers probes by listing that \
             directory, so an empty result means the directory moved, not that there \
             is nothing to check",
            probes_root.display()
        ));
    }
    Ok(probes.into_values().collect())
}

/// Work out how each probe attaches to the tree, following path edges that
/// point at *other probes* so an indirect coupling is not missed.
///
/// `probes/dettrig-accuracy` is exactly this case: it depends on
/// `probes/wasm-determinism`, which depends on `crates/straf3-sim`. Reading
/// only its own manifest would call it independent of the tree, and it is not.
pub fn classify(probes: &[Probe], repo_root: &Path) -> BTreeMap<String, Coupling> {
    let by_dir: BTreeMap<&Path, &Probe> = probes
        .iter()
        .map(|p| (p.manifest.parent().expect("a manifest has a directory"), p))
        .collect();

    let mut out = BTreeMap::new();
    for probe in probes {
        // Depth-first over path edges. `seen` also terminates the walk if two
        // probes ever depend on each other.
        let mut best = Coupling::Independent;
        let mut seen: BTreeSet<PathBuf> = BTreeSet::new();
        let mut stack = vec![(probe, false)];

        while let Some((current, gated_so_far)) = stack.pop() {
            for dep in &current.path_deps {
                let gated = gated_so_far || dep.optional;
                if reaches_tree(&dep.target, repo_root) {
                    best = match (best, gated) {
                        (Coupling::Direct, _) => Coupling::Direct,
                        (_, false) => Coupling::Direct,
                        (_, true) => Coupling::FeatureGated,
                    };
                    continue;
                }
                if let Some(next) = by_dir.get(dep.target.as_path())
                    && seen.insert(dep.target.clone())
                {
                    stack.push((next, gated));
                }
            }
        }
        out.insert(probe.dir.clone(), best);
    }
    out
}

/// True if `target` points inside one of this repository's code directories.
fn reaches_tree(target: &Path, repo_root: &Path) -> bool {
    WORKSPACE_CODE_DIRS
        .iter()
        .any(|d| target.starts_with(repo_root.join(d)))
}

// ── running ─────────────────────────────────────────────────────────────────

struct Args {
    only: Vec<String>,
    skip: Vec<String>,
    all_features: bool,
    list_only: bool,
}

fn parse_args(argv: &[String]) -> Result<Args, String> {
    let mut args = Args {
        only: Vec::new(),
        skip: Vec::new(),
        all_features: false,
        list_only: false,
    };
    let mut it = argv.iter();
    while let Some(a) = it.next() {
        match a.as_str() {
            "--only" => args.only.push(
                it.next()
                    .ok_or("--only needs a probe directory name")?
                    .clone(),
            ),
            "--skip" => args.skip.push(
                it.next()
                    .ok_or("--skip needs a probe directory name")?
                    .clone(),
            ),
            "--all-features" => args.all_features = true,
            "--list" => args.list_only = true,
            other => {
                return Err(format!(
                    "unknown argument {other}\n\
                     usage: cargo xtask check-probes [--list] [--all-features] \
                     [--only <dir>] [--skip <dir>]"
                ));
            }
        }
    }
    if !args.only.is_empty() && !args.skip.is_empty() {
        return Err("--only and --skip cannot be combined".into());
    }
    Ok(args)
}

fn workspace_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("xtask/ always has a parent")
        .to_path_buf()
}

fn cargo() -> String {
    std::env::var("CARGO").unwrap_or_else(|_| "cargo".to_string())
}

/// What one probe's compile attempt told us.
struct Checked {
    result: Result<(), String>,
    /// The probe's `Cargo.lock` did not match its manifests and cargo wanted
    /// to rewrite it. Worth saying — a probe whose lock is stale cannot be
    /// built reproducibly with `--locked`, which is what CI would do — but
    /// deliberately not a failure, and deliberately not conflated with one.
    lock_was_stale: bool,
}

/// `cargo check` one probe. `--all-targets` so a probe whose *tests* stopped
/// compiling counts as broken too — that is still a probe nobody can run.
///
/// Not `--locked`. The question this command exists to answer is "has this
/// probe's *code* rotted", and `--locked` answers a different one: with a
/// stale lock, cargo refuses before compiling anything, so a lockfile drift
/// masquerades as a compile failure and the real answer never arrives. That
/// is not hypothetical either — it is what the first run of this check did,
/// reporting `coil-course` as broken for the wrong reason while its six
/// genuine type errors went unmentioned.
///
/// Dropping `--locked` means cargo may rewrite the lock, and `probes/*/
/// Cargo.lock` are tracked files. So the lock is snapshotted and put back:
/// a check that mutates the tree it is checking cannot be re-run to confirm
/// its own result, and would show up as spurious uncommitted changes.
fn check_one(probe: &Probe, all_features: bool) -> Checked {
    let dir = probe.manifest.parent().expect("a manifest has a directory");
    let lock_path = dir.join("Cargo.lock");
    let lock_before = std::fs::read(&lock_path).ok();

    let mut cmd = Command::new(cargo());
    cmd.arg("check")
        .arg("--manifest-path")
        .arg(&probe.manifest)
        .arg("--all-targets");
    if all_features {
        cmd.arg("--all-features");
    }

    let out = cmd.output();

    // Restore before interpreting anything, so an early return cannot leave
    // the tree modified.
    let mut lock_was_stale = false;
    if let Some(before) = &lock_before
        && std::fs::read(&lock_path).is_ok_and(|after| &after != before)
    {
        lock_was_stale = true;
        let _ = std::fs::write(&lock_path, before);
    }

    let out = match out {
        Ok(o) => o,
        Err(e) => {
            return Checked {
                result: Err(format!("could not run cargo check for {}: {e}", probe.dir)),
                lock_was_stale,
            };
        }
    };
    Checked {
        result: interpret(&out),
        lock_was_stale,
    }
}

fn interpret(out: &std::process::Output) -> Result<(), String> {
    if out.status.success() {
        return Ok(());
    }
    let stderr = String::from_utf8_lossy(&out.stderr);
    // The first error and its context is what a reader needs; the full log is
    // reproducible with the command we print.
    let excerpt: Vec<&str> = stderr
        .lines()
        .filter(|l| {
            l.starts_with("error")
                || l.trim_start().starts_with("--> ")
                || l.contains("expected")
                || l.contains("no method named")
                || l.contains("cannot find")
        })
        .take(12)
        .collect();
    Err(if excerpt.is_empty() {
        stderr.lines().rev().take(8).collect::<Vec<_>>().join("\n")
    } else {
        excerpt.join("\n")
    })
}

pub fn run(argv: &[String]) -> Result<bool, String> {
    let args = parse_args(argv)?;
    let root = workspace_root();
    let probes_root = root.join("probes");

    let all = discover(&probes_root)?;
    let coupling = classify(&all, &root);

    for name in args.only.iter().chain(args.skip.iter()) {
        if !all.iter().any(|p| &p.dir == name) {
            return Err(format!(
                "no probe directory named {name}. Found: {}",
                all.iter()
                    .map(|p| p.dir.as_str())
                    .collect::<Vec<_>>()
                    .join(", ")
            ));
        }
    }

    let selected: Vec<&Probe> = all
        .iter()
        .filter(|p| args.only.is_empty() || args.only.contains(&p.dir))
        .filter(|p| !args.skip.contains(&p.dir))
        .collect();

    let mut report = String::new();
    let _ = writeln!(
        report,
        "  discovered {} crate(s) under probes/ by listing the directory:",
        all.len()
    );
    for p in &all {
        let c = coupling[&p.dir];
        let mark = if selected.iter().any(|s| s.dir == p.dir) {
            ' '
        } else {
            '-'
        };
        let _ = writeln!(report, "    {mark} {:<20} {}", p.dir, c.label());
    }
    print!("{report}");

    if args.list_only {
        println!("\n  --list given: nothing was compiled.");
        return Ok(true);
    }

    // Never let a narrowed run read as a full one.
    let skipped: Vec<&str> = all
        .iter()
        .filter(|p| !selected.iter().any(|s| s.dir == p.dir))
        .map(|p| p.dir.as_str())
        .collect();
    if !skipped.is_empty() {
        println!(
            "\n  NOT A FULL CHECK — these probes were not compiled: {}",
            skipped.join(", ")
        );
    }

    println!();
    let mut failures: Vec<(String, String)> = Vec::new();
    let mut stale_locks: Vec<&str> = Vec::new();
    for probe in &selected {
        println!("  checking {} ...", probe.dir);
        let checked = check_one(probe, args.all_features);
        if checked.lock_was_stale {
            stale_locks.push(probe.dir.as_str());
        }
        match checked.result {
            Ok(()) => println!("    ok"),
            Err(detail) => {
                println!("    FAILED");
                failures.push((probe.dir.clone(), detail));
            }
        }
    }

    if !stale_locks.is_empty() {
        println!(
            "\n  lock note: {} carr(y) a Cargo.lock that no longer matches the\n\
             manifests. Not a failure and not why anything above failed — but\n\
             `cargo check --locked`, which is what a CI runner would do, refuses\n\
             outright on these. This check left them as it found them.",
            stale_locks.join(", ")
        );
    }

    // State the residual hole every run, whether or not anything failed. A
    // feature-gated coupling that default-feature `cargo check` never
    // compiles is precisely the shape of the bug this command exists to
    // prevent, so it does not get to be a footnote a reader has to deduce.
    if !args.all_features {
        let gated: Vec<&str> = selected
            .iter()
            .filter(|p| coupling[&p.dir] == Coupling::FeatureGated)
            .map(|p| p.dir.as_str())
            .collect();
        if !gated.is_empty() {
            println!(
                "\n  coverage note: {} reach(es) this tree only through `optional`\n\
                 path dependencies, which default features do not compile. That\n\
                 coupling was NOT checked by this run. Use --all-features to cover it.",
                gated.join(", ")
            );
        }
    }

    if failures.is_empty() {
        println!("\n  {} probe(s) compile against this tree.", selected.len());
        return Ok(true);
    }

    println!(
        "\n  {} probe(s) do not compile against this tree:\n",
        failures.len()
    );
    for (dir, detail) in &failures {
        println!("  ── {dir} ──");
        for line in detail.lines() {
            println!("     {line}");
        }
        println!(
            "     reproduce: cargo check --manifest-path probes/{dir}/Cargo.toml --all-targets\n"
        );
    }
    Ok(false)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn dir() -> PathBuf {
        PathBuf::from("/repo/probes/example")
    }

    #[test]
    fn an_inline_path_dependency_is_found() {
        let deps = scan_path_deps(
            "[dependencies]\nstraf3-sim = { path = \"../../crates/straf3-sim\" }\n",
            &dir(),
        );
        assert_eq!(
            deps,
            vec![PathDep {
                target: PathBuf::from("/repo/crates/straf3-sim"),
                optional: false
            }]
        );
    }

    #[test]
    fn an_optional_dependency_is_flagged_in_both_spellings() {
        // Inline: everything on one line.
        let inline = scan_path_deps(
            "[dependencies]\nr = { path = \"../../crates/straf3-render\", optional = true }\n",
            &dir(),
        );
        assert!(inline[0].optional, "inline `optional = true` must be seen");

        // Table: `optional` arrives on a later line and must be paired with
        // the `path` above it, or a feature-gated edge is misreported as a
        // hard one — which would claim coverage this check does not have.
        let table = scan_path_deps(
            "[dependencies.straf3-render]\npath = \"../../crates/straf3-render\"\noptional = true\n",
            &dir(),
        );
        assert_eq!(table.len(), 1);
        assert!(
            table[0].optional,
            "table-form `optional = true` must be seen"
        );
    }

    #[test]
    fn a_commented_out_dependency_is_not_a_dependency() {
        // The real manifests carry long prose comments that mention paths;
        // reading one as an edge would invent a coupling that is not there.
        let deps = scan_path_deps(
            "[dependencies]\n# straf3-collision = { path = \"../../crates/straf3-collision\" }\n\
             straf3-sim = { path = \"../../crates/straf3-sim\" }\n",
            &dir(),
        );
        assert_eq!(deps.len(), 1);
        assert_eq!(deps[0].target, PathBuf::from("/repo/crates/straf3-sim"));
    }

    #[test]
    fn keys_are_matched_as_whole_words() {
        // `optional` must not be found inside another key, and a key that
        // merely ends in `path` is not `path`.
        assert_eq!(value_of("build-path = \"x\"", "path"), None);
        assert_eq!(value_of("path = \"x\"", "path").as_deref(), Some("x"));
        assert_eq!(
            value_of("p = { path = \"a\", optional = true }", "optional").as_deref(),
            Some("true")
        );
    }

    #[test]
    fn only_dependency_sections_contribute_edges() {
        // `[patch]`/`[profile]` and friends can carry a `path`; treating one
        // as a dependency edge would misclassify the probe.
        assert!(is_dependency_section("[dependencies]"));
        assert!(is_dependency_section("[dev-dependencies]"));
        assert!(is_dependency_section(
            "[target.'cfg(target_arch = \"wasm32\")'.dependencies]"
        ));
        assert!(is_dependency_section("[dependencies.straf3-render]"));
        assert!(!is_dependency_section("[package]"));
        assert!(!is_dependency_section("[profile.release]"));
    }

    #[test]
    fn an_indirect_coupling_through_another_probe_is_found() {
        // dettrig-accuracy -> wasm-determinism -> crates/straf3-sim. Reading
        // only its own manifest would call it independent of the tree.
        let root = PathBuf::from("/repo");
        let leaf = Probe {
            dir: "wasm-determinism".into(),
            manifest: root.join("probes/wasm-determinism/Cargo.toml"),
            path_deps: vec![PathDep {
                target: root.join("crates/straf3-sim"),
                optional: false,
            }],
        };
        let outer = Probe {
            dir: "dettrig-accuracy".into(),
            manifest: root.join("probes/dettrig-accuracy/Cargo.toml"),
            path_deps: vec![PathDep {
                target: root.join("probes/wasm-determinism"),
                optional: false,
            }],
        };
        let c = classify(&[leaf, outer], &root);
        assert_eq!(c["dettrig-accuracy"], Coupling::Direct);
        assert_eq!(c["wasm-determinism"], Coupling::Direct);
    }

    #[test]
    fn an_optional_edge_downgrades_to_feature_gated_but_a_hard_one_wins() {
        let root = PathBuf::from("/repo");
        let gated_only = Probe {
            dir: "wasm-render".into(),
            manifest: root.join("probes/wasm-render/Cargo.toml"),
            path_deps: vec![PathDep {
                target: root.join("crates/straf3-render"),
                optional: true,
            }],
        };
        let mixed = Probe {
            dir: "mixed".into(),
            manifest: root.join("probes/mixed/Cargo.toml"),
            path_deps: vec![
                PathDep {
                    target: root.join("crates/straf3-render"),
                    optional: true,
                },
                PathDep {
                    target: root.join("crates/straf3-sim"),
                    optional: false,
                },
            ],
        };
        let c = classify(&[gated_only, mixed], &root);
        assert_eq!(c["wasm-render"], Coupling::FeatureGated);
        assert_eq!(
            c["mixed"],
            Coupling::Direct,
            "a hard edge outranks a gated one"
        );
    }

    #[test]
    fn a_registry_only_probe_is_independent() {
        let root = PathBuf::from("/repo");
        let p = Probe {
            dir: "standalone".into(),
            manifest: root.join("probes/standalone/Cargo.toml"),
            path_deps: vec![],
        };
        assert_eq!(classify(&[p], &root)["standalone"], Coupling::Independent);
    }
}
