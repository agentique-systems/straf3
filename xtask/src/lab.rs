//! The movement lab: measure the movement language and publish the numbers.
//!
//! # Why this exists as an xtask and not just a binary
//!
//! Two reasons, and both are about the document being trustworthy rather than
//! about convenience.
//!
//! **It stamps the tree state.** A results document without a commit is a
//! rumour: a reader cannot tell which code produced a number, and a number they
//! cannot trace is a number they have to re-measure. `straf3-lab` deliberately
//! does *not* discover this itself — a tool that shelled out to `git rev-parse`
//! on its own would produce a different document on every commit, including
//! commits that changed nothing it measures, so `--check` could never be green.
//! Provenance therefore comes from outside the deterministic part, and this is
//! the outside.
//!
//! **It is the name a reader will look for.** `check-seam` and `determinism`
//! are how this project spells "the thing that will tell you if you broke it".
//! A movement instrument that had to be invoked as `cargo run -p straf3-lab
//! --release -- --emit …` would be run once, by its author.
//!
//! # What it does not do
//!
//! It does not decide whether a change is acceptable. `--check` reports which
//! measurements moved and exits non-zero; whether that is a regression or the
//! intended effect of a movement change is a judgement, and the fix for the
//! second case is to re-run without `--check` and commit the new numbers beside
//! the change that caused them.

use std::path::{Path, PathBuf};
use std::process::Command;

/// The package that owns the measurements.
const LAB_PACKAGE: &str = "straf3-lab";

/// Run the lab.
///
/// Returns `Ok(false)` when the lab ran and reported a difference — that is a
/// failed check, not a broken tool, and the caller prints a different message
/// for each.
pub fn run(argv: &[String]) -> Result<bool, String> {
    let root = workspace_root()?;
    let mut args: Vec<String> = Vec::new();

    // Everything the caller passed goes through, so `straf3-lab --help` is the
    // one place the options are documented and this file cannot drift from it.
    // The provenance flags are added unless the caller set them, which keeps
    // `cargo xtask lab --tree <something>` usable for a reproduction.
    let names_tree = argv.iter().any(|a| a == "--tree");
    args.extend(argv.iter().cloned());
    if !names_tree {
        let (tree, dirty) = tree_state(&root);
        args.push("--tree".to_string());
        args.push(tree);
        if dirty {
            args.push("--dirty".to_string());
        }
    }

    // Release, not debug. The measurements are tens of millions of simulation
    // commands; a debug build turns a ten-second instrument into a several-
    // minute one, and an instrument nobody waits for is not an instrument.
    let mut cmd = Command::new(cargo());
    cmd.current_dir(&root)
        .arg("run")
        .arg("--release")
        .arg("--quiet")
        .arg("-p")
        .arg(LAB_PACKAGE)
        .arg("--")
        .args(&args);

    let status = cmd
        .status()
        .map_err(|e| format!("could not run {LAB_PACKAGE}: {e}"))?;
    Ok(status.success())
}

/// The commit the working tree is on, and whether it has uncommitted changes.
///
/// A tree that is not a git repository, or a git that is not installed, is not
/// an error: the document says its provenance was not recorded, loudly, and the
/// measurements are unaffected because they do not depend on it.
fn tree_state(root: &Path) -> (String, bool) {
    let git = |args: &[&str]| -> Option<String> {
        let out = Command::new("git")
            .current_dir(root)
            .args(args)
            .output()
            .ok()?;
        if !out.status.success() {
            return None;
        }
        Some(String::from_utf8_lossy(&out.stdout).trim().to_string())
    };

    match git(&["rev-parse", "HEAD"]) {
        Some(head) if !head.is_empty() => {
            let dirty = git(&["status", "--porcelain"]).is_none_or(|s| !s.is_empty());
            (head, dirty)
        }
        _ => ("not a git checkout".to_string(), false),
    }
}

/// The cargo to invoke, preferring the one that launched this xtask.
fn cargo() -> String {
    std::env::var("CARGO").unwrap_or_else(|_| "cargo".to_string())
}

/// The workspace root, found by walking up from this crate's manifest.
fn workspace_root() -> Result<PathBuf, String> {
    let manifest = std::env::var("CARGO_MANIFEST_DIR")
        .map_err(|_| "CARGO_MANIFEST_DIR is not set; run this through `cargo xtask`".to_string())?;
    let mut dir = PathBuf::from(manifest);
    loop {
        if dir.join("Cargo.toml").exists() && dir.join("crates").is_dir() {
            return Ok(dir);
        }
        if !dir.pop() {
            return Err("could not find the workspace root above this crate".to_string());
        }
    }
}
