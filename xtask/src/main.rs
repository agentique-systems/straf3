//! `cargo xtask <command>`

use std::process::ExitCode;

fn main() -> ExitCode {
    let argv: Vec<String> = std::env::args().skip(2).collect();
    match std::env::args().nth(1).as_deref() {
        Some("check-seam") => check_seam(),
        Some("check-probes") => check_probes(&argv),
        Some("determinism") => determinism(&argv),
        Some("capture") => capture(&argv),
        Some("pacing") => pacing(&argv),
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

  check-probes Compile every crate under probes/. They are standalone by
               design — each has its own [workspace] and Cargo.lock — so no
               --workspace command builds them, and two of them stopped
               compiling when C3 changed ViewAngles to 16-bit without the
               tree ever going red. Probes are discovered by listing the
               directory, not from a list here, so a new one is covered
               without editing xtask.

               Options:
                 --list              show what was discovered, compile nothing
                 --all-features      also compile `optional` path deps
                 --only <dir>        check only this probe (repeatable)
                 --skip <dir>        skip this probe, loudly (repeatable)

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

  capture      Screenshot the running straf3 window on the real GPU
               (criterion 8). Cross-builds tools/straf3-capture for
               x86_64-pc-windows-gnu and runs it through WSL interop; every
               other argument is forwarded to it.

               It captures a WINDOW, never the screen. A missing window exits
               4 and an occluded one exits 5; neither writes a file. The
               whole-screen diagnostic (--desktop) may only be written under
               target/, which .gitignore excludes.

                 cargo xtask capture --out shots/coil.png --wait-ms 8000
                 cargo xtask capture --list
                 cargo xtask capture -- --help

  pacing       Measure frame times on the real GPU and report mean, p50, p99
               and max (criterion 7). Runs the release Windows build once per
               present mode with --pacing-log, then ingests the CSVs.

               Options:
                 --analyse <file>    report on an existing log, run nothing
                                     (repeatable)
                 --mode <mode>       fifo|immediate|mailbox, repeatable;
                                     default is fifo then immediate
                 --exit-after <ms>   how long each run lasts (default 8000)
                 --warmup <n>        intervals dropped from the front
                                     (default 1: it holds device acquisition)
                 --renderer-example  measure straf3-render's `window` example
                                     instead of the client
                 --out <dir>         where the CSVs go (default target/pacing)
                 --no-build          use the binaries already built

               Never run on the Linux side: Vulkan is llvmpipe there and
               presentation has no real vblank, so the numbers are fiction
               (docs/environment.md §6).
"
    );
}

fn capture(argv: &[String]) -> ExitCode {
    match xtask::capture::run(argv) {
        Ok(0) => ExitCode::SUCCESS,
        Ok(code) => ExitCode::from(u8::try_from(code).unwrap_or(1)),
        Err(e) => {
            eprintln!("capture could not run: {e}");
            ExitCode::FAILURE
        }
    }
}

fn pacing(argv: &[String]) -> ExitCode {
    match xtask::pacing::run(argv) {
        Ok(true) => ExitCode::SUCCESS,
        Ok(false) => {
            eprintln!(
                "\nA pacing run did not produce usable numbers. Nothing above should be\n\
                 published as a measurement of this machine."
            );
            ExitCode::FAILURE
        }
        Err(e) => {
            eprintln!("pacing could not run: {e}");
            ExitCode::FAILURE
        }
    }
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

fn check_probes(argv: &[String]) -> ExitCode {
    println!(
        "checking that every crate under probes/ still compiles against this tree...\n\
         nothing in `cargo check --workspace` builds them, so this is the only\n\
         thing standing between a probe and silent rot.\n"
    );
    match xtask::probes::run(argv) {
        Ok(true) => ExitCode::SUCCESS,
        Ok(false) => {
            eprintln!(
                "\nA probe no longer compiles. That probe's evidence cannot be\n\
                 reproduced until it does — and because probes are what settle\n\
                 empirical questions here, a broken one silently removes the\n\
                 ability to check the thing it was written to check."
            );
            ExitCode::FAILURE
        }
        Err(e) => {
            eprintln!("probe check could not run: {e}");
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
