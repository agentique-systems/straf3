//! `cargo xtask <command>`

use std::process::ExitCode;

fn main() -> ExitCode {
    let argv: Vec<String> = std::env::args().skip(2).collect();
    match std::env::args().nth(1).as_deref() {
        Some("check-seam") => check_seam(),
        Some("determinism") => determinism(&argv),
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
                   dependency edge or by std call. Sources are scanned with
                   comments and string literals blanked and `use` groups
                   expanded, so `use std::{{fs as sneaky, io}};` is caught.
                 - no determinism-breaking cargo feature is enabled anywhere
                   in the workspace: glam's fast-math and scalar-math are
                   forbidden outright, and its libm-family features are
                   forbidden when `glam/std` is absent (only then do they
                   change float results).

  determinism  Run one reference command stream through straf3-sim on every
               target the project ships or verifies on — glibc, musl,
               windows-gnu and wasm32 under Node — and fail if they do not
               all agree. The comparison is a digest folded over EVERY
               command, not the final state: a measured probe case diverged
               mid-run and re-converged, and an end-state comparison would
               have certified it as reproducible.

               Options:
                 --only <triple>     check only this target (repeatable)
                 --skip <triple>     skip this target, loudly (repeatable)
                 --emit <file>       write one target's report to a file
                 --compare <files>   compare reports emitted elsewhere

               CI uses --emit on a Linux runner and a Windows runner, then
               --compare, so the Windows binary is executed on Windows.
"
    );
}

fn determinism(argv: &[String]) -> ExitCode {
    println!(
        "checking cross-target determinism (architecture item C2)...\n\
         one reference stream, every target, compared by a digest folded over\n\
         every command rather than by the final state.\n"
    );
    match xtask::determinism::run(argv) {
        Ok(true) => ExitCode::SUCCESS,
        Ok(false) => {
            eprintln!(
                "\nThe simulation does not compute the same bits on every target.\n\
                 A recorded run cannot be verified and a ghost cannot be trusted\n\
                 while this is true. See docs/web/ARCHITECTURE.md items C1 and C2."
            );
            ExitCode::FAILURE
        }
        Err(e) => {
            eprintln!("determinism check could not run: {e}");
            ExitCode::FAILURE
        }
    }
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
