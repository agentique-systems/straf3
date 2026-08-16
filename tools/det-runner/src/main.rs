//! Native side of the cross-target determinism check: run the reference
//! stream and print the report on stdout.
//!
//! ```text
//! straf3-det-runner            # print the report
//! straf3-det-runner --summary  # everything except the per-command lists
//! ```
//!
//! Invoked by `cargo xtask determinism`, once per target. On
//! `wasm32-unknown-unknown` this binary is not built at all: the library's
//! `d_report_ptr` / `d_report_len` exports render the identical text and
//! `tools/det-runner/web/run-node.mjs` copies it out of linear memory.
//!
//! This file, and not the library, is where the process boundary is: the
//! library never reads the environment or prints anything, so it compiles for
//! wasm with no imports at all.

use std::io::Write as _;

fn main() {
    let summary_only = std::env::args().any(|a| a == "--summary");

    let platform = format!("native-{}-{}", std::env::consts::ARCH, std::env::consts::OS);
    let report = straf3_det_runner::render(&platform);

    let text = if summary_only {
        report
            .lines()
            .filter(|l| !l.starts_with("checksums "))
            .map(|l| format!("{l}\n"))
            .collect()
    } else {
        report
    };

    // Written as one block through a locked handle: the report is ~200 KiB and
    // `println!` per line would lock stdout thousands of times for nothing.
    let stdout = std::io::stdout();
    let mut out = stdout.lock();
    out.write_all(text.as_bytes()).expect("write report");
    out.flush().expect("flush report");
}
