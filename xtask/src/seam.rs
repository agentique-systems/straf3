//! Enforcement of straf3's one-directional dependency rule.
//!
//! # What this checks, and why
//!
//! The architecture (spec section 4) draws a line:
//!
//! ```text
//! straf3-game / straf3-render / straf3-platform / straf3-devtools   ← above
//! ─────────────────────────────────────────────────────────────────
//! straf3-sim / straf3-collision / straf3-map / straf3-replay        ← below
//! ```
//!
//! Nothing below the line may depend on anything above it, and `straf3-sim`
//! must additionally never reach a crate that touches the filesystem, a
//! window, or a GPU. That property is what makes headless tests, replays,
//! ghosts and future RL environments possible without touching the physics —
//! and it is the kind of property that dies quietly to one convenient
//! `use straf3_platform::...` two months from now.
//!
//! So this is not a comment or a lint config. It runs `cargo tree` and reads
//! the **actually resolved** dependency graph — transitively, across all
//! targets, with all features enabled — and fails if a forbidden crate
//! appears anywhere in it. A `deny.toml` stanza nobody runs would not.
//!
//! Run it with:
//!
//! ```text
//! cargo xtask check-seam
//! ```
//!
//! It also runs as a normal test (`cargo test --workspace`) and in CI.

use std::fmt::Write as _;
use std::path::{Path, PathBuf};
use std::process::Command;

/// The crates below the line. Every one of these is checked.
const BELOW_THE_LINE: &[&str] = &[
    "straf3-sim",
    "straf3-collision",
    "straf3-map",
    "straf3-replay",
];

/// The crates above the line. None of the above may appear in the dependency
/// tree of any crate below it.
const ABOVE_THE_LINE: &[&str] = &[
    "straf3-platform",
    "straf3-render",
    "straf3-devtools",
    "straf3-game",
];

/// Third-party crates that must never appear below the line.
///
/// This is a denylist, so it is necessarily incomplete — a new windowing crate
/// invented tomorrow is not in it. It is a backstop. The primary guarantee
/// comes from [`ABOVE_THE_LINE`]: rendering and windowing live in those
/// crates, and those crates are unreachable from below.
const FORBIDDEN_THIRD_PARTY: &[(&str, &str)] = &[
    // windowing / input / display
    ("winit", "windowing"),
    ("raw-window-handle", "windowing"),
    ("raw_window_handle", "windowing"),
    ("sdl2", "windowing"),
    ("glutin", "windowing"),
    ("x11-dl", "windowing"),
    ("wayland-client", "windowing"),
    ("gilrs", "device input"),
    // GPU
    ("wgpu", "GPU"),
    ("wgpu-core", "GPU"),
    ("wgpu-hal", "GPU"),
    ("wgpu-types", "GPU"),
    ("naga", "GPU"),
    ("ash", "GPU"),
    ("metal", "GPU"),
    ("glow", "GPU"),
    // UI
    ("egui", "UI"),
    ("eframe", "UI"),
    ("egui-wgpu", "UI"),
    ("egui-winit", "UI"),
    // audio
    ("cpal", "audio"),
    ("rodio", "audio"),
    // filesystem / OS / ambient environment
    ("walkdir", "filesystem"),
    ("tempfile", "filesystem"),
    ("memmap2", "filesystem"),
    ("notify", "filesystem"),
    ("dirs", "filesystem"),
    ("directories", "filesystem"),
    ("rfd", "filesystem"),
    ("image", "filesystem/decoding"),
    // async runtimes drag in I/O and nondeterministic scheduling
    ("tokio", "async I/O"),
    ("async-std", "async I/O"),
    ("mio", "async I/O"),
    // Not an I/O concern — a determinism one. Work-stealing changes the order
    // float reductions happen in, so the same inputs stop producing the same
    // run. parry3d can enable rayon via its `parallel` feature; don't.
    ("rayon", "nondeterministic parallelism"),
];

/// Source patterns banned inside `straf3-sim` specifically.
///
/// `cargo tree` cannot see this: `std` is always present, so a crate can reach
/// the filesystem and the wall clock without any dependency edge at all. The
/// tree check alone would miss `std::fs::read("map.bsp")` sitting in the
/// middle of the simulation.
const FORBIDDEN_SIM_SOURCE: &[(&str, &str)] = &[
    ("std::fs", "filesystem access"),
    ("std::net", "network access"),
    ("std::process", "subprocess spawning"),
    ("std::env", "ambient environment"),
    (
        "std::time::Instant",
        "wall clock (the sim advances on commands, not time)",
    ),
    (
        "SystemTime",
        "wall clock (the sim advances on commands, not time)",
    ),
    ("include_str!", "compile-time filesystem access"),
    ("include_bytes!", "compile-time filesystem access"),
];

/// A single seam violation, phrased so the fix is obvious.
#[derive(Debug, Clone)]
pub struct Violation {
    /// The crate below the line whose rule was broken.
    pub crate_name: String,
    /// What it must not have reached.
    pub offender: String,
    /// Why that thing is forbidden.
    pub reason: String,
    /// How it got there: the dependency chain, or the source location.
    pub how: String,
    /// `true` for source-scan hits, `false` for dependency-graph hits. Source
    /// hits are per-line and must not be collapsed together.
    pub is_source: bool,
}

impl std::fmt::Display for Violation {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "{} must not reach {} ({}).\n      via {}",
            self.crate_name, self.offender, self.reason, self.how
        )
    }
}

/// Everything the check learned, violations or not.
#[derive(Debug, Default)]
pub struct Report {
    /// Rules that were broken. Empty means the seam holds.
    pub violations: Vec<Violation>,
    /// Human-readable notes about what was and was not inspected.
    pub notes: Vec<String>,
}

impl Report {
    /// Whether the seam holds.
    pub fn is_clean(&self) -> bool {
        self.violations.is_empty()
    }

    /// A printable summary, suitable for CI logs and test failure output.
    pub fn render(&self) -> String {
        let mut out = String::new();
        for note in &self.notes {
            let _ = writeln!(out, "  {note}");
        }
        if self.is_clean() {
            let _ = writeln!(
                out,
                "\n  seam holds: no crate below the line reaches above it."
            );
        } else {
            let _ = writeln!(out, "\n  {} SEAM VIOLATION(S):", self.violations.len());
            for v in &self.violations {
                let _ = writeln!(out, "\n    ✗ {v}");
            }
            if self.violations.iter().any(|v| v.crate_name != "workspace") {
                let _ = writeln!(
                    out,
                    "\n  The dependency rule is one-directional (spec section 4). Move the\n  \
                     offending code above the line, or invert the dependency so the crate\n  \
                     above calls down into the crate below."
                );
            }
            if self.violations.iter().any(|v| v.crate_name == "workspace") {
                let _ = writeln!(
                    out,
                    "\n  A determinism-breaking cargo feature is enabled. Remove it: replays,\n  \
                     ghosts and regression tests all assume bit-identical float results\n  \
                     (spec section 4)."
                );
            }
        }
        out
    }
}

/// The workspace root, derived from this crate's manifest directory.
pub fn workspace_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("xtask/ always has a parent")
        .to_path_buf()
}

/// Run the whole seam check against the real, resolved dependency graph.
///
/// Returns `Err` only when the graph could not be read at all (for example
/// because a workspace member is missing) — that is a different failure from
/// a violation, and is reported differently.
pub fn check() -> Result<Report, String> {
    let root = workspace_root();
    let mut report = Report::default();

    for &crate_name in BELOW_THE_LINE {
        let tree = cargo_tree(&root, crate_name)?;
        let entries = parse_tree(&tree);
        report.notes.push(format!(
            "{crate_name}: {} package(s) in the resolved tree (all targets, all features)",
            entries.len()
        ));

        for entry in &entries {
            if entry.name == crate_name {
                continue;
            }
            if ABOVE_THE_LINE.contains(&entry.name.as_str()) {
                report.violations.push(Violation {
                    crate_name: crate_name.to_string(),
                    offender: entry.name.clone(),
                    reason: "it is above the line".to_string(),
                    how: entry.chain.join(" → "),
                    is_source: false,
                });
            } else if let Some((_, reason)) = FORBIDDEN_THIRD_PARTY
                .iter()
                .find(|(name, _)| *name == entry.name)
            {
                report.violations.push(Violation {
                    crate_name: crate_name.to_string(),
                    offender: entry.name.clone(),
                    reason: (*reason).to_string(),
                    how: entry.chain.join(" → "),
                    is_source: false,
                });
            }
        }
    }

    check_sim_source(&root, &mut report);
    check_float_determinism(&root, &mut report)?;
    dedupe_keeping_shortest_chain(&mut report.violations);
    Ok(report)
}

/// Cargo features that must never be enabled anywhere in the workspace,
/// because they change float results.
///
/// `glam`'s `fast-math` permits the compiler to reassociate float operations.
/// That is a fine trade for a renderer and a fatal one here: it silently
/// breaks bit-identical replay, which is the property ghosts, regression tests
/// and future RL environments all rest on (spec section 4). It is opt-in, not
/// a default — this check exists to keep it that way, because the day someone
/// enables it for a frame-time win, nothing will fail loudly.
const FORBIDDEN_FEATURES: &[(&str, &str)] = &[(
    "fast-math",
    "permits float reassociation, which breaks bit-identical replay",
)];

/// Assert no determinism-breaking cargo feature is enabled in the workspace.
///
/// Checked twice: against the default feature resolution (what actually gets
/// built) and against `--all-features` (which would catch an opt-in workspace
/// feature that forwards to `glam/fast-math`).
fn check_float_determinism(root: &Path, report: &mut Report) -> Result<(), String> {
    for all_features in [false, true] {
        let tree = cargo_tree_features(root, all_features)?;
        let scope = if all_features {
            "--all-features"
        } else {
            "default features"
        };

        for (feature, reason) in FORBIDDEN_FEATURES {
            let hits: Vec<&str> = tree
                .lines()
                .map(str::trim)
                .filter(|line| line.contains(feature))
                .collect();

            if hits.is_empty() {
                report
                    .notes
                    .push(format!("workspace: `{feature}` not enabled ({scope})"));
            } else {
                for hit in hits {
                    report.violations.push(Violation {
                        crate_name: "workspace".to_string(),
                        offender: (*feature).to_string(),
                        reason: (*reason).to_string(),
                        how: format!(
                            "{} ({scope})",
                            hit.trim_start_matches(['│', '├', '└', '─', ' '])
                        ),
                        is_source: true, // per-occurrence; do not collapse
                    });
                }
            }
        }
    }
    Ok(())
}

/// The workspace tree with feature edges shown, so enabled cargo features are
/// visible as nodes.
fn cargo_tree_features(root: &Path, all_features: bool) -> Result<String, String> {
    let cargo = std::env::var("CARGO").unwrap_or_else(|_| "cargo".to_string());
    let mut cmd = Command::new(&cargo);
    cmd.current_dir(root)
        .args([
            "tree",
            "--workspace",
            "--edges",
            "features",
            "--target",
            "all",
        ])
        .env("CARGO_TARGET_DIR", root.join("target").join("xtask-seam"));
    if all_features {
        cmd.arg("--all-features");
    }

    let output = cmd
        .output()
        .map_err(|e| format!("could not run `{cargo} tree --edges features`: {e}"))?;
    if !output.status.success() {
        return Err(format!(
            "`cargo tree --edges features` failed:\n{}",
            String::from_utf8_lossy(&output.stderr).trim()
        ));
    }
    Ok(String::from_utf8_lossy(&output.stdout).into_owned())
}

/// One offender reached by six different paths is one problem, not six.
/// Keep the shortest chain for each `(crate, offender)` pair — that is the one
/// closest to the edge somebody actually has to delete.
/// Source-scan hits are per-line and are never collapsed: two `std::fs` calls
/// in different functions are two things to fix.
fn dedupe_keeping_shortest_chain(violations: &mut Vec<Violation>) {
    violations.sort_by(|a, b| {
        (&a.crate_name, &a.offender, a.how.len(), &a.how).cmp(&(
            &b.crate_name,
            &b.offender,
            b.how.len(),
            &b.how,
        ))
    });
    violations.dedup_by(|a, b| {
        !a.is_source && !b.is_source && a.crate_name == b.crate_name && a.offender == b.offender
    });
}

/// One package as it appeared in the tree, with the chain that reached it.
struct Entry {
    name: String,
    chain: Vec<String>,
}

/// Ask cargo for the fully resolved tree of `package`.
///
/// `--all-features` so an optional `wgpu` feature cannot hide; `--target all`
/// so a Windows-only windowing dependency cannot hide on Linux CI;
/// `--edges normal,build,dev` so a build script or a test dependency cannot
/// hide either.
fn cargo_tree(root: &Path, package: &str) -> Result<String, String> {
    let cargo = std::env::var("CARGO").unwrap_or_else(|_| "cargo".to_string());
    let output = Command::new(&cargo)
        .current_dir(root)
        .args([
            "tree",
            "--package",
            package,
            "--edges",
            "normal,build,dev",
            "--target",
            "all",
            "--all-features",
            "--prefix",
            "depth",
            "--format",
            "{p}",
        ])
        // Keep a nested invocation from contending with an outer `cargo test`
        // for the build directory lock.
        .env("CARGO_TARGET_DIR", root.join("target").join("xtask-seam"))
        .output()
        .map_err(|e| format!("could not run `{cargo} tree`: {e}"))?;

    if !output.status.success() {
        return Err(format!(
            "`cargo tree --package {package}` failed:\n{}",
            String::from_utf8_lossy(&output.stderr).trim()
        ));
    }
    Ok(String::from_utf8_lossy(&output.stdout).into_owned())
}

/// Turn `--prefix depth` output into packages plus the chain that reached them.
///
/// Lines look like `2glam v0.30.8`. The leading integer is the depth, which is
/// enough to rebuild the ancestry as a stack.
fn parse_tree(tree: &str) -> Vec<Entry> {
    let mut entries = Vec::new();
    let mut stack: Vec<String> = Vec::new();

    for line in tree.lines() {
        let digits: String = line.chars().take_while(char::is_ascii_digit).collect();
        let Ok(depth) = digits.parse::<usize>() else {
            continue; // blank lines, and the "(*)" dedupe markers cargo emits
        };
        let rest = &line[digits.len()..];
        let Some(name) = rest.split_whitespace().next() else {
            continue;
        };
        let name = name.trim_start_matches('(').to_string();

        stack.truncate(depth);
        stack.push(name.clone());
        entries.push(Entry {
            name,
            chain: stack.clone(),
        });
    }
    entries
}

/// Scan `straf3-sim`'s sources for `std` facilities that reach outside the
/// simulation. See [`FORBIDDEN_SIM_SOURCE`] for why the tree check is not
/// enough on its own.
fn check_sim_source(root: &Path, report: &mut Report) {
    let src = root.join("crates").join("straf3-sim").join("src");
    let mut files = Vec::new();
    collect_rs(&src, &mut files);

    if files.is_empty() {
        report.notes.push(format!(
            "straf3-sim: no sources found at {} — source scan skipped",
            src.display()
        ));
        return;
    }
    report.notes.push(format!(
        "straf3-sim: scanned {} source file(s) for std escapes",
        files.len()
    ));

    for file in files {
        let Ok(text) = std::fs::read_to_string(&file) else {
            continue;
        };
        for (lineno, line) in text.lines().enumerate() {
            // Crude but predictable: whole-line comments are exempt so the
            // rule can be discussed in prose without tripping itself.
            if line.trim_start().starts_with("//") {
                continue;
            }
            for (pattern, reason) in FORBIDDEN_SIM_SOURCE {
                if line.contains(pattern) {
                    let rel = file.strip_prefix(root).unwrap_or(&file);
                    report.violations.push(Violation {
                        crate_name: "straf3-sim".to_string(),
                        offender: (*pattern).to_string(),
                        reason: (*reason).to_string(),
                        how: format!("{}:{}", rel.display(), lineno + 1),
                        is_source: true,
                    });
                }
            }
        }
    }
}

/// Collect `.rs` files under `dir`, recursively.
fn collect_rs(dir: &Path, out: &mut Vec<PathBuf>) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            collect_rs(&path, out);
        } else if path.extension().is_some_and(|e| e == "rs") {
            out.push(path);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn depth_prefixed_tree_reconstructs_the_chain() {
        let tree = "\
0straf3-sim v0.1.0 (/w/crates/straf3-sim)
1straf3-collision v0.1.0 (/w/crates/straf3-collision)
2glam v0.30.8
1straf3-map v0.1.0 (/w/crates/straf3-map)
";
        let entries = parse_tree(tree);
        assert_eq!(entries.len(), 4);
        assert_eq!(entries[2].name, "glam");
        assert_eq!(
            entries[2].chain,
            vec!["straf3-sim", "straf3-collision", "glam"]
        );
        assert_eq!(entries[3].chain, vec!["straf3-sim", "straf3-map"]);
    }

    #[test]
    fn a_forbidden_crate_is_recognised_wherever_it_sits() {
        let tree = "\
0straf3-sim v0.1.0 (/w/crates/straf3-sim)
1straf3-collision v0.1.0 (/w/crates/straf3-collision)
2wgpu v27.0.0
";
        let entries = parse_tree(tree);
        let offender = &entries[2];
        assert!(
            FORBIDDEN_THIRD_PARTY
                .iter()
                .any(|(n, _)| *n == offender.name),
            "wgpu must be on the denylist"
        );
        assert_eq!(
            offender.chain.join(" → "),
            "straf3-sim → straf3-collision → wgpu"
        );
    }

    #[test]
    fn the_two_sides_of_the_line_do_not_overlap() {
        for c in BELOW_THE_LINE {
            assert!(!ABOVE_THE_LINE.contains(c), "{c} cannot be on both sides");
        }
    }
}
