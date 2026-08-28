//! Records the commit this verifier was built from, for `sim_builds.git_sha`.
//!
//! Taken from `git` when it is there and from `STRAF3_GIT_SHA` when a build
//! environment has no repository. `unknown` when neither answers — the column
//! says "unknown" rather than carrying a plausible-looking value nobody
//! derived (wave contracts §E3).

use std::process::Command;

fn main() {
    println!("cargo:rerun-if-env-changed=STRAF3_GIT_SHA");

    let sha = std::env::var("STRAF3_GIT_SHA")
        .ok()
        .filter(|s| !s.trim().is_empty())
        .or_else(git_head)
        .unwrap_or_else(|| "unknown".to_string());

    println!("cargo:rustc-env=STRAF3_GIT_SHA={sha}");
}

fn git_head() -> Option<String> {
    let output = Command::new("git")
        .args(["rev-parse", "HEAD"])
        .current_dir(env!("CARGO_MANIFEST_DIR"))
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    let sha = String::from_utf8(output.stdout).ok()?.trim().to_string();
    (!sha.is_empty()).then_some(sha)
}
