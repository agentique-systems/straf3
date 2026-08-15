//! `cargo xtask <command>`

use std::process::ExitCode;

fn main() -> ExitCode {
    match std::env::args().nth(1).as_deref() {
        Some("check-seam") => check_seam(),
        Some(other) => {
            eprintln!("unknown command: {other}");
            usage();
            ExitCode::FAILURE
        }
        None => {
            usage();
            ExitCode::FAILURE
        }
    }
}

fn usage() {
    eprintln!(
        "\
usage: cargo xtask <command>

  check-seam   Verify, against the real resolved dependency graph:
                 - the one-directional dependency rule. No crate below the
                   line (straf3-sim, -collision, -map, -replay) may reach a
                   crate above it (straf3-platform, -render, -devtools,
                   -game), at any depth.
                 - straf3-sim reaches no window, GPU or filesystem, by
                   dependency edge or by std call.
                 - no determinism-breaking cargo feature (glam/fast-math) is
                   enabled anywhere in the workspace.
"
    );
}

fn check_seam() -> ExitCode {
    println!("checking the straf3 dependency seam (spec section 4)...\n");
    match xtask::seam::check() {
        Ok(report) => {
            print!("{}", report.render());
            if report.is_clean() {
                ExitCode::SUCCESS
            } else {
                ExitCode::FAILURE
            }
        }
        Err(e) => {
            eprintln!("could not read the dependency graph: {e}");
            eprintln!(
                "\nThis is not a seam violation — the graph could not be resolved at all.\n\
                 If a workspace member is missing, create it or remove it from the\n\
                 workspace `members` list."
            );
            ExitCode::FAILURE
        }
    }
}
