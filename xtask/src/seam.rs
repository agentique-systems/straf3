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

/// Source paths banned inside `straf3-sim` specifically.
///
/// `cargo tree` cannot see this: `std` is always present, so a crate can reach
/// the filesystem and the wall clock without any dependency edge at all. The
/// tree check alone would miss `std::fs::read("map.bsp")` sitting in the
/// middle of the simulation.
///
/// These are matched against *normalised* source (see [`strip_comments_and_literals`])
/// and against *expanded* `use` declarations (see [`expanded_use_paths`]), not
/// against raw lines. That distinction is the whole point: an earlier version
/// of this check compared raw lines with `contains`, and
/// `use std::{fs as sneaky, io};` slipped straight through it because the text
/// `std::fs` never appears on that line.
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
    // Renaming the root defeats every other entry in this table: after
    // `use std as sneaky;` the call site reads `sneaky::fs::read(..)`, so the
    // raw-line scan never sees `std::fs`, and the use-tree scan emits only
    // `std` (the ` as ` alias is truncated in `expand_use_tree`). Both passes
    // miss it. Nothing legitimate needs to rename the standard library, so the
    // import itself is the violation.
    (
        "std as ",
        "aliasing the `std` root, which hides every path in this table",
    ),
];

/// Transcendental functions the simulation may not call, anywhere in
/// `crates/straf3-sim/src/` except `num.rs`.
///
/// # Why this is a rule and not a review note
///
/// `f32::sin_cos` is whichever libm the target links, and libm `sinf`/`cosf`
/// are not IEEE-754-specified: glibc and the statically-linked `libm` that
/// wasm and musl builds use disagree by 1 ulp on roughly 1.3% of angles. A
/// probe measured that gap diverging a recorded run after about 14 seconds of
/// play, which is enough to make a browser recording unverifiable on a glibc
/// server. `num::sin_cos` exists to replace them, using only IEEE-specified
/// arithmetic (contract item C1).
///
/// Fixing the three call sites once is worth nothing without this: the next
/// `.sin()` written anywhere in the physics silently reintroduces the whole
/// problem, and it would look like ordinary code while doing it.
///
/// `num.rs` is exempt because it is where a deterministic replacement has to
/// be written and tested — its tests compare against libm on purpose.
/// `sqrt` is absent deliberately: it *is* IEEE-specified and correctly
/// rounded, so `normalize` may keep calling it.
const TRANSCENDENTALS: &[&str] = &[
    "sin", "cos", "sin_cos", "tan", "asin", "acos", "atan2", "exp", "ln", "powf",
];

/// The one file under `straf3-sim/src/` allowed to hold the arithmetic these
/// functions would otherwise be reached for.
const TRANSCENDENTAL_HOME: &str = "num.rs";

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
const FORBIDDEN_FEATURES: &[(&str, &str)] = &[
    (
        "fast-math",
        "permits float reassociation, which breaks bit-identical replay",
    ),
    (
        "scalar-math",
        "replaces glam's SIMD paths with scalar ones, changing float results",
    ),
];

/// glam's libm-family features, which swap its float math for `libm`'s.
///
/// These are handled separately from [`FORBIDDEN_FEATURES`] because they are
/// **conditional**, and getting that wrong would either break the build or
/// pretend to guard something it does not:
///
/// `libm` and `nostd-libm` both resolve to `dep:libm`, but glam only *uses*
/// libm when its `std` feature is off — with `std` on, std's math wins and the
/// libm dependency is inert. Right now `nostd-libm` **is** enabled in this
/// workspace, pulled in transitively via
/// `straf3-collision → parry3d → glamx → glam` feature unification, and it is
/// harmless purely because `glam/std` is also on.
///
/// So flagging it unconditionally would fail a clean tree. Instead this fires
/// exactly when the hazard is real: libm-family enabled *and* `glam/std` gone.
/// If someone makes the workspace `no_std`, this becomes a violation the same
/// day, which is the point.
const GLAM_LIBM_FEATURES: &[&str] = &["libm", "nostd-libm"];

/// What glam's libm-family features mean for a given resolved tree.
#[derive(Debug, PartialEq, Eq)]
enum LibmVerdict {
    /// No libm-family feature on glam at all.
    NotEnabled,
    /// Enabled, but `glam/std` is on, so std's math is used regardless.
    Inert(Vec<String>),
    /// Enabled with no `glam/std` — glam's float results actually change.
    Live(Vec<String>),
}

/// Decide whether glam's libm features are actually changing float behaviour.
fn libm_verdict(edges: &[FeatureEdge]) -> LibmVerdict {
    let on: Vec<String> = edges
        .iter()
        .filter(|e| e.krate == "glam" && GLAM_LIBM_FEATURES.contains(&e.feature.as_str()))
        .map(|e| e.feature.clone())
        .collect();
    if on.is_empty() {
        return LibmVerdict::NotEnabled;
    }
    if edges
        .iter()
        .any(|e| e.krate == "glam" && e.feature == "std")
    {
        LibmVerdict::Inert(on)
    } else {
        LibmVerdict::Live(on)
    }
}

/// One `crate feature "name"` edge from `cargo tree --edges features`.
#[derive(Debug, PartialEq, Eq, PartialOrd, Ord)]
struct FeatureEdge {
    krate: String,
    feature: String,
}

/// Pull `crate feature "name"` nodes out of `cargo tree --edges features`.
///
/// Parsed structurally rather than by substring, because the feature name and
/// a package name routinely collide: `libm v0.2.16` (a package) and
/// `glam feature "nostd-libm"` (a feature) both contain "libm", and a
/// `contains("libm")` guard would report a dozen phantom violations on a tree
/// that is perfectly fine.
fn parse_feature_edges(tree: &str) -> Vec<FeatureEdge> {
    let mut edges = Vec::new();
    for line in tree.lines() {
        let line = line.trim_start_matches(['│', '├', '└', '─', ' ', '\t']);
        let Some((krate, rest)) = line.split_once(" feature \"") else {
            continue;
        };
        let Some((feature, _)) = rest.split_once('"') else {
            continue;
        };
        if krate.is_empty() || krate.contains(' ') {
            continue;
        }
        edges.push(FeatureEdge {
            krate: krate.to_string(),
            feature: feature.to_string(),
        });
    }
    edges.sort();
    edges.dedup();
    edges
}

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

        let edges = parse_feature_edges(&tree);

        for (feature, reason) in FORBIDDEN_FEATURES {
            let hits: Vec<&FeatureEdge> = edges.iter().filter(|e| e.feature == *feature).collect();

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
                        how: format!("{} feature \"{}\" ({scope})", hit.krate, hit.feature),
                        is_source: true, // per-occurrence; do not collapse
                    });
                }
            }
        }

        // The conditional libm rule. See GLAM_LIBM_FEATURES for why this is not
        // just another entry in FORBIDDEN_FEATURES.
        match libm_verdict(&edges) {
            LibmVerdict::NotEnabled => report.notes.push(format!(
                "workspace: glam libm-family features not enabled ({scope})"
            )),
            LibmVerdict::Inert(on) => report.notes.push(format!(
                "workspace: glam `{}` enabled but inert — `glam/std` is on, so std math wins ({scope})",
                on.join("`, `")
            )),
            LibmVerdict::Live(on) => {
                for feature in on {
                    report.violations.push(Violation {
                        crate_name: "workspace".to_string(),
                        offender: format!("glam/{feature}"),
                        reason:
                            "routes glam's float math through libm, changing results, because \
                             `glam/std` is not enabled"
                                .to_string(),
                        how: format!("glam feature \"{feature}\" with no `glam/std` ({scope})"),
                        is_source: true,
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
        "straf3-sim: scanned {} source file(s) for std escapes and \
         transcendental calls outside {TRANSCENDENTAL_HOME}",
        files.len()
    ));

    for file in files {
        let Ok(text) = std::fs::read_to_string(&file) else {
            continue;
        };
        let rel = file
            .strip_prefix(root)
            .unwrap_or(&file)
            .display()
            .to_string();

        // Comments and string literals are blanked first, so prose about the
        // rule cannot trip the rule, and a `"std::fs"` inside a string cannot
        // either. Line structure is preserved, so line numbers stay true.
        let code = strip_comments_and_literals(&text);

        // (line number, pattern) — one finding per line per pattern, so an
        // inline path and its expanded `use` do not report twice.
        let mut hits: Vec<(usize, &str, &str, String)> = Vec::new();

        // 1. Paths written inline: `std::fs::read(..)`, `std :: fs` included.
        for (idx, line) in code.lines().enumerate() {
            let normalised = collapse_path_spaces(line);
            for (pattern, reason) in FORBIDDEN_SIM_SOURCE {
                if normalised.contains(pattern) {
                    hits.push((idx + 1, pattern, reason, format!("{rel}:{}", idx + 1)));
                }
            }
        }

        // 2. `use` declarations with their brace groups expanded and aliases
        //    resolved, which is what raw-line matching could not see.
        for (path, lineno) in expanded_use_paths(&code) {
            for (pattern, reason) in FORBIDDEN_SIM_SOURCE {
                if path.contains(pattern) {
                    hits.push((
                        lineno,
                        pattern,
                        reason,
                        format!("{rel}:{lineno} (`use` resolves to `{path}`)"),
                    ));
                }
            }
        }

        hits.sort_by(|a, b| (a.0, a.1, a.3.len()).cmp(&(b.0, b.1, b.3.len())));
        hits.dedup_by(|a, b| a.0 == b.0 && a.1 == b.1);

        for (_, pattern, reason, how) in hits {
            report.violations.push(Violation {
                crate_name: "straf3-sim".to_string(),
                offender: pattern.to_string(),
                reason: reason.to_string(),
                how,
                is_source: true,
            });
        }

        // 3. Transcendental calls (contract item C1). Skipped for the one file
        //    that is allowed to hold the replacement and test it against libm.
        let is_home = file.file_name().is_some_and(|n| n == TRANSCENDENTAL_HOME);
        if !is_home {
            for (lineno, pattern) in transcendental_hits(&code) {
                report.violations.push(Violation {
                    crate_name: "straf3-sim".to_string(),
                    offender: pattern,
                    reason: format!(
                        "libm is not IEEE-specified and differs between targets \
                         (glibc vs wasm/musl) — use `num::sin_cos`, or add the \
                         arithmetic to {TRANSCENDENTAL_HOME}"
                    ),
                    how: format!("{rel}:{lineno}"),
                    is_source: true,
                });
            }
        }
    }
}

/// Every transcendental call in already-normalised source, as
/// `(line number, the text that matched)`.
///
/// Matching happens on the file with **all whitespace removed**, carrying a
/// parallel index back to line numbers. That is what makes the rule hold up:
/// a raw-line `contains(".sin(")` misses `x . sin ( )`, and — far more likely
/// — misses the rustfmt-wrapped method chain
///
/// ```text
/// let (s, c) = (yaw * DEG_TO_RAD)
///     .sin_cos();
/// ```
///
/// which is not a bypass anyone has to intend. The `use`-expansion pass above
/// is no help here either, since these are method calls with no import to see.
///
/// Both the method form (`.sin(`) and the fully-qualified form (`f32::sin(`,
/// and the `Scalar` alias for it) are matched, because UFCS is the one-line
/// way around a method-only rule. `num::sin_cos(..)` is deliberately *not*
/// matched: calling the owned implementation is the entire point.
fn transcendental_hits(code: &str) -> Vec<(usize, String)> {
    let mut dense = String::with_capacity(code.len());
    let mut line_of: Vec<usize> = Vec::with_capacity(code.len());
    let mut line = 1usize;
    for ch in code.chars() {
        if ch == '\n' {
            line += 1;
        }
        if ch.is_whitespace() {
            continue;
        }
        dense.push(ch);
        line_of.push(line);
    }

    let mut hits: Vec<(usize, String)> = Vec::new();
    for name in TRANSCENDENTALS {
        for form in [
            format!(".{name}("),
            format!("f32::{name}("),
            format!("f64::{name}("),
            format!("Scalar::{name}("),
        ] {
            for (byte_idx, _) in dense.match_indices(&form) {
                // Rare enough (usually zero) that counting chars per hit is
                // cheaper than carrying a byte-indexed map for every file.
                let char_idx = dense[..byte_idx].chars().count();
                let lineno = line_of.get(char_idx).copied().unwrap_or(1);
                hits.push((lineno, form.clone()));
            }
        }
    }

    // One finding per line per matched form.
    hits.sort();
    hits.dedup();
    hits
}

/// Blank out comments and the contents of string/char literals, preserving
/// every newline so line numbers survive.
///
/// Handles line comments, nested block comments, raw strings (`r#"..."#`),
/// byte strings, and escapes. Lifetimes (`'a`) are distinguished from char
/// literals (`'a'`) by lookahead, so a lifetime does not swallow the rest of
/// the file.
fn strip_comments_and_literals(text: &str) -> String {
    let c: Vec<char> = text.chars().collect();
    let mut out = String::with_capacity(text.len());
    let mut i = 0;

    // Keep newlines, blank everything else, so offsets stay line-accurate.
    let blank = |out: &mut String, ch: char| out.push(if ch == '\n' { '\n' } else { ' ' });

    while i < c.len() {
        // line comment
        if c[i] == '/' && i + 1 < c.len() && c[i + 1] == '/' {
            while i < c.len() && c[i] != '\n' {
                blank(&mut out, c[i]);
                i += 1;
            }
            continue;
        }
        // block comment, nested
        if c[i] == '/' && i + 1 < c.len() && c[i + 1] == '*' {
            let mut depth = 0usize;
            loop {
                if i >= c.len() {
                    break;
                }
                if c[i] == '/' && i + 1 < c.len() && c[i + 1] == '*' {
                    depth += 1;
                    blank(&mut out, c[i]);
                    blank(&mut out, c[i + 1]);
                    i += 2;
                    continue;
                }
                if c[i] == '*' && i + 1 < c.len() && c[i + 1] == '/' {
                    depth -= 1;
                    blank(&mut out, c[i]);
                    blank(&mut out, c[i + 1]);
                    i += 2;
                    if depth == 0 {
                        break;
                    }
                    continue;
                }
                blank(&mut out, c[i]);
                i += 1;
            }
            continue;
        }
        // raw string: optional `b`, then `r`, then `#`*, then `"`
        let prev_is_ident = i > 0 && (c[i - 1].is_alphanumeric() || c[i - 1] == '_');
        if !prev_is_ident && (c[i] == 'r' || c[i] == 'b') {
            let mut j = i;
            if c[j] == 'b' {
                j += 1;
            }
            if j < c.len() && c[j] == 'r' {
                j += 1;
                let mut hashes = 0;
                while j < c.len() && c[j] == '#' {
                    hashes += 1;
                    j += 1;
                }
                if j < c.len() && c[j] == '"' {
                    // blank the prefix and opening quote
                    while i <= j {
                        blank(&mut out, c[i]);
                        i += 1;
                    }
                    // scan for the closing `"` followed by `hashes` `#`
                    while i < c.len() {
                        if c[i] == '"' {
                            let closed = (1..=hashes).all(|k| c.get(i + k) == Some(&'#'));
                            if closed {
                                for _ in 0..=hashes {
                                    if i < c.len() {
                                        blank(&mut out, c[i]);
                                        i += 1;
                                    }
                                }
                                break;
                            }
                        }
                        blank(&mut out, c[i]);
                        i += 1;
                    }
                    continue;
                }
            }
        }
        // normal string
        if c[i] == '"' {
            blank(&mut out, c[i]);
            i += 1;
            while i < c.len() {
                if c[i] == '\\' {
                    blank(&mut out, c[i]);
                    if i + 1 < c.len() {
                        blank(&mut out, c[i + 1]);
                    }
                    i += 2;
                    continue;
                }
                let end = c[i] == '"';
                blank(&mut out, c[i]);
                i += 1;
                if end {
                    break;
                }
            }
            continue;
        }
        // char literal vs lifetime: `'x'` and `'\n'` are literals, `'a` is not
        if c[i] == '\'' {
            let escaped = c.get(i + 1) == Some(&'\\');
            let short = c.get(i + 2) == Some(&'\'');
            if escaped || short {
                blank(&mut out, c[i]);
                i += 1;
                while i < c.len() {
                    if c[i] == '\\' {
                        blank(&mut out, c[i]);
                        if i + 1 < c.len() {
                            blank(&mut out, c[i + 1]);
                        }
                        i += 2;
                        continue;
                    }
                    let end = c[i] == '\'';
                    blank(&mut out, c[i]);
                    i += 1;
                    if end {
                        break;
                    }
                }
                continue;
            }
        }
        out.push(c[i]);
        i += 1;
    }
    out
}

/// Collapse whitespace around `::`, so `std :: fs` reads as `std::fs`.
fn collapse_path_spaces(line: &str) -> String {
    let c: Vec<char> = line.chars().collect();
    let mut out = String::with_capacity(line.len());
    let mut i = 0;
    while i < c.len() {
        if c[i].is_whitespace() {
            let mut j = i;
            while j < c.len() && c[j].is_whitespace() {
                j += 1;
            }
            let next_is_sep = j + 1 < c.len() && c[j] == ':' && c[j + 1] == ':';
            if next_is_sep || out.ends_with("::") {
                i = j;
                continue;
            }
            out.push(' ');
            i = j;
            continue;
        }
        out.push(c[i]);
        i += 1;
    }
    out
}

/// Expand every `use` declaration into the full paths it actually imports.
///
/// `use std::{fs as sneaky, io};` yields `std::fs` and `std::io`. That is the
/// bypass this check exists to close: the text `std::fs` never appears on that
/// line, so line-wise `contains` reported the file clean while the sim read
/// the filesystem through `sneaky::read_to_string`.
///
/// Returns `(path, line number)` pairs.
fn expanded_use_paths(code: &str) -> Vec<(String, usize)> {
    let lines: Vec<&str> = code.lines().collect();
    let mut out = Vec::new();
    let mut i = 0;

    while i < lines.len() {
        let mut head = lines[i].trim_start();
        head = head.strip_prefix("pub").map_or(head, str::trim_start);
        if head.starts_with('(') {
            head = head.split_once(')').map_or(head, |(_, r)| r.trim_start());
        }
        let Some(rest) = head
            .strip_prefix("use ")
            .or_else(|| head.strip_prefix("use\t"))
        else {
            i += 1;
            continue;
        };

        // A `use` may wrap across lines; gather to the terminating `;`.
        let start = i + 1;
        let mut stmt = rest.to_string();
        while !stmt.contains(';') && i + 1 < lines.len() {
            i += 1;
            stmt.push(' ');
            stmt.push_str(lines[i]);
        }
        let stmt = stmt.split(';').next().unwrap_or_default().to_string();
        let normalised = collapse_path_spaces(&stmt);
        expand_use_tree("", normalised.trim(), &mut |p| out.push((p, start)));
        i += 1;
    }
    out
}

/// Recursively flatten one use-tree, resolving `{}` groups, `as` aliases and
/// trailing `self`.
fn expand_use_tree(prefix: &str, tree: &str, emit: &mut impl FnMut(String)) {
    let tree = tree.trim();
    if tree.is_empty() {
        return;
    }
    let Some(open) = tree.find('{') else {
        let mut path = format!("{prefix}{tree}");
        if let Some(at) = path.find(" as ") {
            path.truncate(at);
        }
        let path = path.trim().trim_end_matches("::self").trim().to_string();
        if !path.is_empty() {
            emit(path);
        }
        return;
    };

    let new_prefix = format!("{prefix}{}", &tree[..open]);
    let mut depth = 0i32;
    let mut close = None;
    for (idx, ch) in tree.char_indices().skip(open) {
        match ch {
            '{' => depth += 1,
            '}' => {
                depth -= 1;
                if depth == 0 {
                    close = Some(idx);
                    break;
                }
            }
            _ => {}
        }
    }
    let Some(close) = close else { return };

    for part in split_top_level_commas(&tree[open + 1..close]) {
        expand_use_tree(&new_prefix, &part, emit);
    }
}

/// Split on commas that are not inside a nested `{}` group.
fn split_top_level_commas(s: &str) -> Vec<String> {
    let mut parts = Vec::new();
    let mut depth = 0i32;
    let mut current = String::new();
    for ch in s.chars() {
        match ch {
            '{' => {
                depth += 1;
                current.push(ch);
            }
            '}' => {
                depth -= 1;
                current.push(ch);
            }
            ',' if depth == 0 => parts.push(std::mem::take(&mut current)),
            _ => current.push(ch),
        }
    }
    if !current.trim().is_empty() {
        parts.push(current);
    }
    parts
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

    /// Expand a use-tree the way `check_sim_source` does, for assertions.
    fn uses(src: &str) -> Vec<String> {
        let code = strip_comments_and_literals(src);
        expanded_use_paths(&code)
            .into_iter()
            .map(|(p, _)| p)
            .collect()
    }

    /// Does the scanner flag this source at all?
    fn flags(src: &str) -> bool {
        let code = strip_comments_and_literals(src);
        let inline = code.lines().any(|l| {
            let n = collapse_path_spaces(l);
            FORBIDDEN_SIM_SOURCE.iter().any(|(p, _)| n.contains(p))
        });
        let imported = expanded_use_paths(&code)
            .iter()
            .any(|(path, _)| FORBIDDEN_SIM_SOURCE.iter().any(|(p, _)| path.contains(p)));
        inline || imported
    }

    /// The exact bypass that reached main: an aliased brace-group import.
    /// Raw-line `contains("std::fs")` reported this clean while the sim read
    /// the filesystem through the alias.
    #[test]
    fn the_aliased_brace_group_bypass_is_caught() {
        assert!(flags("use std::{fs as sneaky, io};"));
        assert!(flags("use std::{io, fs as sneaky};"));
        assert!(flags("use std::{env as e};"));
        assert!(flags("use std::{collections::HashMap, process::Command};"));
        assert!(flags("use std :: fs :: read_to_string;"));
    }

    /// The remaining bypass after the brace-group one was closed: rename the
    /// root itself. Both scanner passes are blind to it — the raw-line scan
    /// sees `sneaky::fs`, and the use-tree scan emits only `std` because
    /// `expand_use_tree` truncates at ` as `. Caught on the import line, which
    /// is the one place the string `std` still appears.
    #[test]
    fn aliasing_the_std_root_is_caught() {
        assert!(flags(
            "use std as sneaky;\npub fn load() { sneaky::fs::read(\"m\"); }"
        ));
        assert!(flags("use std as s;"));
        assert!(flags("use std as _;"));

        // The pass that used to miss it still does; the table entry is what
        // catches it. This documents *why* the entry exists, so nobody
        // "simplifies" it away later.
        assert_eq!(uses("use std as sneaky;"), vec!["std"]);
    }

    /// The entry above is a substring match on `std as `, so these must not
    /// trip it. `use std::fmt::Write as _;` is in this very file.
    #[test]
    fn aliasing_a_std_item_is_not_confused_with_aliasing_the_root() {
        assert!(!flags("use std::fmt::Write as _;"));
        assert!(!flags("use std::io::Read as _;"));
        assert!(!flags("use crate::thing as std_as_name;"));
    }

    #[test]
    fn brace_groups_expand_to_real_paths() {
        assert_eq!(
            uses("use std::{fs as sneaky, io};"),
            vec!["std::fs", "std::io"]
        );
        assert_eq!(
            uses("use std::collections::{HashMap, BTreeMap};"),
            vec!["std::collections::HashMap", "std::collections::BTreeMap"]
        );
        // nested groups
        assert_eq!(
            uses("use std::{fs::{self, File}, io};"),
            vec!["std::fs", "std::fs::File", "std::io"]
        );
        // a `use` split across lines
        assert_eq!(
            uses("use std::{\n    fs as sneaky,\n    io,\n};"),
            vec!["std::fs", "std::io"]
        );
    }

    /// The scanner must not fire on prose or strings, or nobody will keep it.
    #[test]
    fn comments_and_strings_do_not_trip_the_scanner() {
        assert!(!flags("//! no `std::fs` allowed here"));
        assert!(!flags("// std::env::var is banned"));
        assert!(!flags("/* std::process::Command */"));
        assert!(!flags("/* nested /* std::net */ still a comment */"));
        assert!(!flags(r#"let s = "std::fs::read";"#));
        assert!(!flags("let s = r#\"std::env\"#;"));
        assert!(flags("let x = 1; // ok\nuse std::fs;"));
    }

    /// A lifetime must not be mistaken for a char literal and swallow the file.
    #[test]
    fn lifetimes_do_not_swallow_following_code() {
        assert!(flags("struct S<'a>(&'a str);\nuse std::fs;"));
        assert!(!flags("let c = '\\'';"));
    }

    #[test]
    fn ordinary_sim_code_stays_clean() {
        assert!(!flags(
            "use core::f32;\nuse crate::profile::PhysicsProfile;\npub fn step(dt_ms: u32) {}"
        ));
    }

    /// Run the C1 transcendental scan the way `check_sim_source` does.
    fn trig(src: &str) -> Vec<(usize, String)> {
        transcendental_hits(&strip_comments_and_literals(src))
    }

    /// The regression this rule exists for: the exact line C1 removed from
    /// `angle_vectors`, written again.
    #[test]
    fn a_reintroduced_sin_cos_is_caught() {
        let hits = trig("let (sy, cy) = (yaw * DEG_TO_RAD).sin_cos();");
        assert_eq!(hits, vec![(1, ".sin_cos(".to_string())]);
    }

    /// Every function in the table, and the line number each is reported on.
    #[test]
    fn every_transcendental_in_the_table_is_caught() {
        for name in TRANSCENDENTALS {
            let src = format!("fn f(x: Scalar) -> Scalar {{ x.{name}(1.0) }}");
            assert!(!trig(&src).is_empty(), "`.{name}(` was not caught: {src}",);
        }
        let hits = trig("let a = 1;\nlet b = x.sin();\nlet c = y.cos();");
        assert_eq!(
            hits,
            vec![(2, ".sin(".to_string()), (3, ".cos(".to_string())]
        );
    }

    /// A method-only rule is one `rustfmt` away from silence: a wrapped chain
    /// puts `.sin_cos()` on its own line with the receiver on the one before.
    /// UFCS is the deliberate version of the same bypass.
    #[test]
    fn formatting_and_ufcs_do_not_hide_a_transcendental() {
        assert!(!trig("let v = (yaw * DEG_TO_RAD)\n    .sin_cos();").is_empty());
        assert!(!trig("let v = x . sin ( ) ;").is_empty());
        assert!(!trig("let v = f32::sin_cos(x);").is_empty());
        assert!(!trig("let v = f64::atan2(y, x);").is_empty());
        assert!(!trig("let v = Scalar::powf(x, 2.0);").is_empty());
    }

    /// The rule must not fire on the code around it, or it gets deleted.
    #[test]
    fn arithmetic_and_the_owned_implementation_stay_clean() {
        // sqrt is IEEE-specified and correctly rounded, so `normalize` keeps it.
        assert!(trig("let (v, l) = ((x * x + y * y).sqrt(), 1.0);").is_empty());
        // Calling the replacement is the point of the rule, not a breach of it.
        assert!(trig("let (sy, cy) = num::sin_cos(yaw * DEG_TO_RAD);").is_empty());
        assert!(trig("let (sy, cy) = crate::num::sin_cos(yaw);").is_empty());
        // Ordinary integer and float arithmetic, and unrelated method names.
        assert!(trig("let d = v.dot(n);\nlet m = a.min(b).max(c);").is_empty());
        assert!(trig("let e = state.expected(4);\nlet t = p.tangent();").is_empty());
        // Prose about the rule must not trip the rule.
        assert!(trig("// never call .sin_cos() here\nlet a = 1;").is_empty());
        assert!(trig("let s = \".sin_cos(\";").is_empty());
    }

    /// `num.rs` is exempt, and the exemption is by file name — so it is the
    /// scan caller that has to apply it, not the scanner.
    #[test]
    fn the_exempt_file_is_named_and_the_scanner_itself_is_not_selective() {
        assert_eq!(TRANSCENDENTAL_HOME, "num.rs");
        // The scanner flags this text; `check_sim_source` is what declines to
        // ask it about num.rs. Pinned so the exemption cannot silently widen
        // into "the scanner ignores sin_cos everywhere".
        assert!(!trig("let (s, c) = x.sin_cos();").is_empty());
    }

    /// `libm v0.2.16` (a package) and `glam feature \"nostd-libm\"` (a feature)
    /// both contain \"libm\". Only the second is a feature edge.
    #[test]
    fn feature_edges_are_not_confused_with_package_names() {
        let tree = "\
straf3-collision v0.1.0
├── libm v0.2.16
├── glam feature \"nostd-libm\"
│   └── glam feature \"std\"
└── num-traits feature \"libm\"
";
        let edges = parse_feature_edges(tree);
        assert!(
            !edges
                .iter()
                .any(|e| e.krate == "libm" && e.feature.is_empty())
        );
        assert!(
            edges
                .iter()
                .any(|e| e.krate == "glam" && e.feature == "nostd-libm")
        );
        assert!(
            edges
                .iter()
                .any(|e| e.krate == "glam" && e.feature == "std")
        );
        assert!(
            edges
                .iter()
                .any(|e| e.krate == "num-traits" && e.feature == "libm")
        );
        // The package line contributed no edge at all.
        assert_eq!(edges.len(), 3);
    }

    fn edges(pairs: &[(&str, &str)]) -> Vec<FeatureEdge> {
        pairs
            .iter()
            .map(|(k, f)| FeatureEdge {
                krate: k.to_string(),
                feature: f.to_string(),
            })
            .collect()
    }

    /// The conditional guard must fire when the hazard is real and stay quiet
    /// when it is not — otherwise it is either a false alarm on every clean
    /// tree, or a guard that never fires at all.
    #[test]
    fn libm_is_a_violation_only_when_glam_std_is_absent() {
        // Today's tree: nostd-libm on, std on -> inert, must not fail.
        assert_eq!(
            libm_verdict(&edges(&[("glam", "nostd-libm"), ("glam", "std")])),
            LibmVerdict::Inert(vec!["nostd-libm".into()])
        );
        // Someone drops std (a no_std push) -> the hazard is now real.
        assert_eq!(
            libm_verdict(&edges(&[("glam", "nostd-libm")])),
            LibmVerdict::Live(vec!["nostd-libm".into()])
        );
        assert_eq!(
            libm_verdict(&edges(&[("glam", "libm")])),
            LibmVerdict::Live(vec!["libm".into()])
        );
        // num-traits' own libm feature is not glam's and must not fire.
        assert_eq!(
            libm_verdict(&edges(&[("num-traits", "libm"), ("simba", "libm")])),
            LibmVerdict::NotEnabled
        );
    }

    #[test]
    fn fast_math_and_scalar_math_are_both_forbidden() {
        for f in ["fast-math", "scalar-math"] {
            assert!(
                FORBIDDEN_FEATURES.iter().any(|(n, _)| *n == f),
                "{f} must be forbidden outright"
            );
        }
        // libm is deliberately NOT unconditional: it is inert while glam/std
        // is on, and it IS on today via parry3d → glamx → glam.
        for f in GLAM_LIBM_FEATURES {
            assert!(
                !FORBIDDEN_FEATURES.iter().any(|(n, _)| n == f),
                "{f} is conditional, not unconditional"
            );
        }
    }

    #[test]
    fn the_two_sides_of_the_line_do_not_overlap() {
        for c in BELOW_THE_LINE {
            assert!(!ABOVE_THE_LINE.contains(c), "{c} cannot be on both sides");
        }
    }
}
