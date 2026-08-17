//! `cargo xtask capture` — the repeatable screenshot command.
//!
//! Acceptance criterion 8 asks for a repeatable command that captures the
//! running client, and rules out hand-written PowerShell. This is the command;
//! `tools/straf3-capture` is what it runs.
//!
//! All this does is build the tool for `x86_64-pc-windows-gnu` and execute the
//! `.exe` through WSL interop, forwarding every argument. It exists so the
//! documented way to take a screenshot is one line with no target triple,
//! no path into `target/`, and no shell quoting in it — and so that line keeps
//! working when the tool's own flags change.
//!
//! The tool's rule travels with it: it captures a window, never the screen.
//! See `tools/straf3-capture/src/lib.rs`.

use std::path::Path;
use std::process::Command;

/// Where the cross-built tool lands.
const EXE: &str = "target/x86_64-pc-windows-gnu/release/straf3-capture.exe";

/// Run the capture tool, building it first unless `--no-build` is passed.
///
/// Returns the tool's own exit code, so a caller can tell "blank" (3) from
/// "no window" (4) from "occluded" (5).
///
/// # Errors
///
/// If cargo or the tool cannot be started at all.
pub fn run(argv: &[String]) -> Result<i32, String> {
    let mut build = true;
    let mut forwarded: Vec<String> = Vec::new();
    for arg in argv {
        if arg == "--no-build" {
            build = false;
        } else {
            forwarded.push(arg.clone());
        }
    }

    if !windows_exe_runnable() {
        return Err(
            "the capture tool is a Windows binary and this shell cannot launch one.\n\
             WSL interop (/proc/sys/fs/binfmt_misc/WSLInterop) is not enabled.\n\
             A capture taken on the Linux side would be of WSLg's software renderer, \
             not of the client on the real GPU — see docs/environment.md §6."
                .to_owned(),
        );
    }

    if build {
        let status = Command::new("cargo")
            .args([
                "build",
                "--release",
                "--target",
                "x86_64-pc-windows-gnu",
                "-p",
                "straf3-capture",
            ])
            .status()
            .map_err(|e| format!("could not start cargo: {e}"))?;
        if !status.success() {
            return Err("building straf3-capture for x86_64-pc-windows-gnu failed".to_owned());
        }
    }

    if !Path::new(EXE).exists() {
        return Err(format!(
            "{EXE} does not exist. Drop --no-build, or run:\n  \
             cargo build --release --target x86_64-pc-windows-gnu -p straf3-capture"
        ));
    }

    let status = Command::new(EXE)
        .args(&forwarded)
        .status()
        .map_err(|e| format!("could not run {EXE}: {e}"))?;

    // 130 stands in for a signal death, which has no exit code of its own.
    Ok(status.code().unwrap_or(130))
}

/// Whether a Windows `.exe` can be executed from this shell.
fn windows_exe_runnable() -> bool {
    if cfg!(windows) {
        return true;
    }
    std::fs::read_to_string("/proc/sys/fs/binfmt_misc/WSLInterop")
        .is_ok_and(|s| s.contains("enabled"))
}
