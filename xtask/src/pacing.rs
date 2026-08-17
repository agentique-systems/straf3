//! Turning frame-interval logs into publishable numbers, and running the
//! measurement that produces them.
//!
//! # What acceptance criterion 7 asks for
//!
//! Frame-time mean, p50, p99 and max, from a **release** build on the real
//! GPU, **vsynced and uncapped**, with the panel's refresh rate stated. This
//! module is the analysis half: it ingests the CSV that a client or the
//! renderer example writes (`straf3_devtools::pacing`) and reports those
//! numbers, plus the one derived statistic that settles the question the wave
//! opened — whether a flat 165 fps was vsync or a ceiling.
//!
//! # How "was that vsync?" is answered
//!
//! Not by the mean, and not by regularity alone. Both mislead:
//!
//! - A mean of 6.06 ms is equally consistent with a vsynced 165 Hz run and
//!   with a loop that simply happens to take 6 ms.
//! - Regularity is not the discriminator either. Measured on this hardware,
//!   the *uncapped* run was **more** regular than the vsynced one: a
//!   312-triangle scene at ~4000 fps holds its interval inside 40 µs, while
//!   the FIFO run's 87 % keep-rate is spoiled by real dropped and doubled
//!   frames.
//!
//! [`Stats::paced_externally`] therefore requires two things at once: the
//! intervals must be tightly grouped, **and** the interval they are grouped
//! around must be one a display could produce (24–360 Hz). A 4470 Hz beat is
//! extremely regular and cannot be a refresh, so it is reported as the loop
//! running at its own speed — which is what it is.
//!
//! # Percentiles are nearest-rank, and the method is stated
//!
//! `p99` is the value at index `ceil(0.99 * n) - 1` of the sorted samples — an
//! observed frame interval, never an interpolation between two. With a few
//! thousand samples the difference is small, but "the 99th percentile is a
//! frame that actually happened" is a more useful guarantee than a smoother
//! number.
//!
//! # No number from this machine's Linux side is publishable
//!
//! `docs/environment.md` §6: Vulkan resolves to llvmpipe under WSL2 and
//! presentation goes to a synthetic virtual output with no real vblank. This
//! module therefore refuses to *run* a measurement on Linux — it will still
//! *analyse* a file, because the file may have come from the Windows binary,
//! but the header records the build and the caller records the host.

use std::fmt::Write as _;
use std::path::{Path, PathBuf};
use std::process::Command;

/// How close to the median an interval must be to count as "the same", as a
/// percentage of the median.
///
/// Relative, not absolute, and that correction was made after the statistic
/// got a real answer wrong. An absolute ±250 µs window is 4 % of a 6.06 ms
/// vsynced frame — sensible — but it is *more than the whole frame* when the
/// loop is running uncapped at 0.22 ms, so it called a 4000 fps free-running
/// run "tightly clustered, paced by a display refresh". A tolerance that
/// widens with the thing it measures cannot make that mistake.
const LOCK_TOLERANCE_PERCENT: u64 = 4;

/// The floor under that tolerance, so a sub-100 µs median does not shrink the
/// window to the point where timer quantisation alone fails it.
const LOCK_TOLERANCE_FLOOR_NS: u64 = 50_000;

/// The range of median intervals that could plausibly be a display refresh:
/// 24 Hz to 360 Hz. Outside it, "the loop is waiting on vblank" is not an
/// available explanation, whatever the spread looks like.
const DISPLAY_HZ: std::ops::RangeInclusive<f64> = 24.0..=360.0;

// ── the file ────────────────────────────────────────────────────────────────

/// One parsed pacing log.
#[derive(Debug, Clone)]
pub struct Log {
    /// Where it came from.
    pub path: PathBuf,
    /// `key=value` pairs from the `#` header lines, in order.
    pub headers: Vec<(String, String)>,
    /// Frame intervals in nanoseconds, in recorded order.
    pub deltas_ns: Vec<u64>,
}

impl Log {
    /// The value of a header key, if the writer recorded one.
    #[must_use]
    pub fn header(&self, key: &str) -> Option<&str> {
        self.headers
            .iter()
            .find(|(k, _)| k == key)
            .map(|(_, v)| v.as_str())
    }

    /// The present mode the surface was configured with, when the log records
    /// it — and `None` when it does not, which is a meaningful answer rather
    /// than a missing one.
    ///
    /// A writer only emits this key when it asked the surface what it was
    /// configured with. A writer that knows only what it *wanted* emits
    /// [`Log::requested_mode`] instead, under a different name, so the two
    /// claims cannot be confused by a reader skimming for a number.
    #[must_use]
    pub fn configured_mode(&self) -> Option<&str> {
        self.header("present_mode")
    }

    /// The mode the writer asked for, when that is all it knew.
    ///
    /// An unsupported request falls back to FIFO silently as far as the
    /// requester is concerned, so this is never a measurement.
    #[must_use]
    pub fn requested_mode(&self) -> Option<&str> {
        self.header("present_mode_requested")
    }

    /// Whether this log records the mode the surface was actually configured
    /// with.
    ///
    /// Absence is treated as "not known to be measured", which is the safe
    /// direction: a report that assumed the header was authoritative would
    /// publish a fiction the first time a fallback happened.
    #[must_use]
    pub fn mode_is_measured(&self) -> bool {
        self.configured_mode().is_some()
    }

    /// `debug` or `release`, as stamped by the writer's compiler.
    #[must_use]
    pub fn build(&self) -> &str {
        self.header("build").unwrap_or("unknown")
    }
}

/// Parse a pacing log.
///
/// Unrecognised `#` lines are kept as headers when they look like
/// `key=value` and ignored otherwise, so a writer can add context without
/// breaking this reader.
///
/// # Errors
///
/// If the file cannot be read, does not carry the v1 magic line, or has no
/// `frame,delta_ns` column header.
pub fn parse(path: &Path) -> Result<Log, String> {
    let text = std::fs::read_to_string(path)
        .map_err(|e| format!("could not read {}: {e}", path.display()))?;
    parse_str(path, &text)
}

/// Parse pacing-log text. Split out so the format is testable without a file.
///
/// # Errors
///
/// As [`parse`].
pub fn parse_str(path: &Path, text: &str) -> Result<Log, String> {
    let mut lines = text.lines();
    let magic = lines.next().unwrap_or("").trim_end();
    if magic != straf3_pacing_magic() {
        return Err(format!(
            "{} does not start with {:?} — this is not a straf3 pacing log v1 \
             (first line was {magic:?})",
            path.display(),
            straf3_pacing_magic()
        ));
    }

    let mut headers = Vec::new();
    let mut saw_columns = false;
    let mut deltas_ns = Vec::new();

    for (number, line) in lines.enumerate() {
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        if let Some(rest) = line.strip_prefix('#') {
            // `# present_mode=fifo  build=release` — several pairs per line.
            for token in rest.split_whitespace() {
                if let Some((key, value)) = token.split_once('=') {
                    headers.push((key.to_owned(), value.to_owned()));
                }
            }
            continue;
        }
        if !saw_columns {
            if line != "frame,delta_ns" {
                return Err(format!(
                    "{}: expected the column header `frame,delta_ns`, found {line:?}",
                    path.display()
                ));
            }
            saw_columns = true;
            continue;
        }
        let Some((_frame, ns)) = line.split_once(',') else {
            return Err(format!(
                "{} line {}: {line:?} is not `frame,delta_ns`",
                path.display(),
                number + 2
            ));
        };
        let ns = ns.trim().parse::<u64>().map_err(|_| {
            format!(
                "{} line {}: {ns:?} is not a whole number of nanoseconds",
                path.display(),
                number + 2
            )
        })?;
        deltas_ns.push(ns);
    }

    if !saw_columns {
        return Err(format!(
            "{}: no `frame,delta_ns` column header — nothing to read",
            path.display()
        ));
    }

    Ok(Log {
        path: path.to_path_buf(),
        headers,
        deltas_ns,
    })
}

/// Duplicated rather than depended on: `xtask` deliberately has zero
/// dependencies so it runs on a cold registry, which means it cannot import
/// `straf3_devtools::pacing::MAGIC`. The two are pinned together by
/// `the_magic_line_matches_the_writer`, which fails if they ever drift.
fn straf3_pacing_magic() -> &'static str {
    "# straf3 pacing log v1"
}

// ── the numbers ─────────────────────────────────────────────────────────────

/// What criterion 7 asks to be published, plus what is needed to interpret it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Stats {
    /// Intervals considered, after any warm-up frames were dropped.
    pub n: usize,
    /// Intervals dropped from the front as warm-up.
    pub dropped: usize,
    pub mean_ns: u64,
    pub p50_ns: u64,
    pub p99_ns: u64,
    pub min_ns: u64,
    pub max_ns: u64,
    /// Share of intervals within [`LOCK_TOLERANCE_PERCENT`] of the median, in
    /// tenths of a percent. High means regular; on its own it does NOT mean
    /// vsync — see [`Stats::paced_externally`].
    pub lock_permille: u32,
    /// The tolerance that was actually applied, so the number above can be
    /// checked rather than taken on trust.
    pub lock_tolerance_ns: u64,
    /// Total wall time the intervals span.
    pub total_ns: u64,
}

impl Stats {
    /// Frames per second implied by the mean interval.
    #[must_use]
    pub fn mean_fps(&self) -> f64 {
        if self.mean_ns == 0 {
            return 0.0;
        }
        1e9 / self.mean_ns as f64
    }

    /// The refresh rate this run would imply if it were vsync-locked.
    ///
    /// Reported next to `lock_permille` so a reader can check the claim
    /// "this was FIFO on a 165 Hz panel" against the two facts that support
    /// it, rather than against a fps counter.
    #[must_use]
    pub fn implied_hz(&self) -> f64 {
        if self.p50_ns == 0 {
            return 0.0;
        }
        1e9 / self.p50_ns as f64
    }

    /// Whether the evidence supports "this loop was waiting on a display
    /// refresh".
    ///
    /// Two conditions, both necessary. The intervals must be tightly grouped
    /// — a loop waiting on vblank cannot be irregular — *and* the interval
    /// they are grouped around has to be one a display could actually produce.
    /// Regularity alone proves nothing: a trivial scene rendered uncapped is
    /// extremely regular too, at 4000 fps.
    #[must_use]
    pub fn paced_externally(&self) -> bool {
        self.lock_permille >= 850 && DISPLAY_HZ.contains(&self.implied_hz())
    }
}

/// Compute the statistics, dropping `warmup` intervals from the front.
///
/// Frame 0 is the interval that contains device acquisition and the first
/// present. Dropping it is normal and is *reported* in [`Stats::dropped`]
/// rather than done silently.
#[must_use]
pub fn stats(deltas_ns: &[u64], warmup: usize) -> Option<Stats> {
    let dropped = warmup.min(deltas_ns.len());
    let kept = &deltas_ns[dropped..];
    if kept.is_empty() {
        return None;
    }

    let mut sorted = kept.to_vec();
    sorted.sort_unstable();

    let n = sorted.len();
    let total_ns: u64 = sorted.iter().sum();
    let p50_ns = percentile(&sorted, 50);
    let tolerance = (p50_ns * LOCK_TOLERANCE_PERCENT / 100).max(LOCK_TOLERANCE_FLOOR_NS);
    let within = sorted
        .iter()
        .filter(|ns| ns.abs_diff(p50_ns) <= tolerance)
        .count();

    Some(Stats {
        n,
        dropped,
        mean_ns: total_ns / n as u64,
        p50_ns,
        p99_ns: percentile(&sorted, 99),
        min_ns: sorted[0],
        max_ns: sorted[n - 1],
        lock_permille: u32::try_from(within as u64 * 1000 / n as u64).unwrap_or(0),
        lock_tolerance_ns: tolerance,
        total_ns,
    })
}

/// Nearest-rank percentile over already-sorted samples: the value at index
/// `ceil(p/100 * n) - 1`. Always an observed sample.
fn percentile(sorted: &[u64], p: u64) -> u64 {
    if sorted.is_empty() {
        return 0;
    }
    let n = sorted.len() as u64;
    let rank = (p * n).div_ceil(100).max(1);
    sorted[(rank - 1).min(n - 1) as usize]
}

/// Format nanoseconds as milliseconds to three decimals.
#[must_use]
pub fn ms(ns: u64) -> String {
    format!("{:.3}", ns as f64 / 1e6)
}

/// A human-readable report for one log.
#[must_use]
pub fn report(log: &Log, stats: &Stats) -> String {
    let mut out = String::new();
    let _ = writeln!(out, "{}", log.path.display());
    match log.configured_mode() {
        Some(mode) => {
            let _ = writeln!(
                out,
                "  build={}  present_mode={mode} (as configured on the surface){}",
                log.build(),
                log.header("fell_back")
                    .map_or(String::new(), |_| "  (FELL BACK from the requested mode)"
                        .to_owned())
            );
        }
        None => {
            // The writer could not see what the surface granted, so this file
            // says only what was asked for. An unsupported request falls back
            // silently, and a number published against a mode that never
            // happened is worse than no number: it looks like evidence.
            let _ = writeln!(
                out,
                "  build={}  present_mode_requested={}",
                log.build(),
                log.requested_mode().unwrap_or("not recorded"),
            );
            let _ = writeln!(
                out,
                "  !! THE CONFIGURED PRESENT MODE IS NOT RECORDED IN THIS LOG — the line\n\
                 !! above is what was asked for, which an unsupported mode silently is\n\
                 !! not. Take the real mode from the renderer's own\n\
                 !! `straf3-render: present_mode=…` line for this run, and say in the\n\
                 !! published document which source each number came from."
            );
        }
    }
    for (key, value) in &log.headers {
        if !matches!(
            key.as_str(),
            "build" | "present_mode" | "present_mode_requested" | "fell_back"
        ) {
            let _ = writeln!(out, "  {key}={value}");
        }
    }
    let _ = writeln!(
        out,
        "  frames={} (dropped {} as warm-up)  span={:.2} s",
        stats.n,
        stats.dropped,
        stats.total_ns as f64 / 1e9
    );
    let _ = writeln!(
        out,
        "  frame time ms:  mean {}  p50 {}  p99 {}  max {}  min {}",
        ms(stats.mean_ns),
        ms(stats.p50_ns),
        ms(stats.p99_ns),
        ms(stats.max_ns),
        ms(stats.min_ns),
    );
    let _ = writeln!(
        out,
        "  mean {:.1} fps;  median interval implies {:.2} Hz;  \
         {}.{} % of frames within ±{} ms of the median",
        stats.mean_fps(),
        stats.implied_hz(),
        stats.lock_permille / 10,
        stats.lock_permille % 10,
        ms(stats.lock_tolerance_ns),
    );
    let verdict = if stats.paced_externally() {
        format!(
            "  → PACED EXTERNALLY: {}.{} % of frames sit on a {:.2} Hz beat, which is \
             a rate a display can produce. The loop is waiting on something, and \
             vblank is what it is waiting on.",
            stats.lock_permille / 10,
            stats.lock_permille % 10,
            stats.implied_hz()
        )
    } else if !DISPLAY_HZ.contains(&stats.implied_hz()) {
        format!(
            "  → NOT DISPLAY-PACED: the median interval implies {:.0} Hz, which no \
             display produces. However regular the frames look, nothing is waiting \
             for a refresh — this is the loop running at its own speed.",
            stats.implied_hz()
        )
    } else {
        format!(
            "  → INCONCLUSIVE: the beat ({:.2} Hz) is in display range but only \
             {}.{} % of frames keep to it. Neither cleanly locked nor cleanly free.",
            stats.implied_hz(),
            stats.lock_permille / 10,
            stats.lock_permille % 10,
        )
    };
    let _ = writeln!(out, "{verdict}");
    out
}

// ── running it ──────────────────────────────────────────────────────────────

/// `cargo xtask pacing` — see `main.rs` for the usage text.
///
/// # Errors
///
/// If a subprocess cannot be started, or a log cannot be read.
pub fn run(argv: &[String]) -> Result<bool, String> {
    let mut analyse: Vec<PathBuf> = Vec::new();
    let mut warmup = 1usize;
    let mut exit_after_ms = 8_000u64;
    let mut modes: Vec<String> = Vec::new();
    let mut out_dir = PathBuf::from("target/pacing");
    let mut target_binary = Binary::Client;
    let mut build = true;

    let mut args = argv.iter();
    while let Some(arg) = args.next() {
        let mut value = || {
            args.next()
                .cloned()
                .ok_or_else(|| format!("`{arg}` needs a value"))
        };
        match arg.as_str() {
            "--analyse" | "--analyze" => analyse.push(PathBuf::from(value()?)),
            "--warmup" => {
                let raw = value()?;
                warmup = raw
                    .parse()
                    .map_err(|_| format!("`--warmup {raw}` is not a number"))?;
            }
            "--exit-after" => {
                let raw = value()?;
                exit_after_ms = raw
                    .parse()
                    .map_err(|_| format!("`--exit-after {raw}` is not a number"))?;
            }
            "--mode" => modes.push(value()?),
            "--out" => out_dir = PathBuf::from(value()?),
            "--renderer-example" => target_binary = Binary::RendererExample,
            "--no-build" => build = false,
            other => return Err(format!("unknown argument: {other}")),
        }
    }

    if !analyse.is_empty() {
        let mut ok = true;
        for path in &analyse {
            let log = parse(path)?;
            match stats(&log.deltas_ns, warmup) {
                Some(s) => print!("{}", report(&log, &s)),
                None => {
                    eprintln!("{}: no frame intervals to report", path.display());
                    ok = false;
                }
            }
            println!();
        }
        return Ok(ok);
    }

    if !cfg!(windows) && std::env::consts::OS == "linux" && !wsl_interop_available() {
        return Err(
            "a pacing measurement has to run the Windows binary on the real GPU.\n\
             WSL interop is not available here, so the .exe cannot be launched \
             from this shell.\n\
             See docs/environment.md §3 and §6: a number taken on the Linux side \
             is llvmpipe with no real vblank, and is not publishable."
                .to_owned(),
        );
    }

    if modes.is_empty() {
        modes = vec!["fifo".to_owned(), "immediate".to_owned()];
    }

    if build {
        println!("building the release Windows binary...");
        let status = Command::new("cargo")
            .args(target_binary.build_args())
            .status()
            .map_err(|e| format!("could not start cargo: {e}"))?;
        if !status.success() {
            return Err("the release cross-build failed".to_owned());
        }
    }

    std::fs::create_dir_all(&out_dir)
        .map_err(|e| format!("could not create {}: {e}", out_dir.display()))?;

    let mut all_ok = true;
    for mode in &modes {
        let log_path = out_dir.join(format!("{}-{mode}.csv", target_binary.slug()));
        println!(
            "\n=== {} in {mode} for {exit_after_ms} ms — a window will open ===",
            target_binary.slug()
        );
        let status = Command::new(target_binary.exe())
            .args(target_binary.run_args(exit_after_ms, &log_path))
            .env("STRAF3_PRESENT_MODE", mode)
            // Without this the variable above does not reach the process.
            // WSL interop does not pass the Linux environment to a Windows
            // child; only variables named in WSLENV cross. Measured, not
            // assumed: a run launched as `STRAF3_PRESENT_MODE=immediate
            // ./straf3.exe` came up in FIFO and the renderer's own log line
            // was the only thing that said so. Exactly the failure the
            // requested-versus-configured distinction exists to catch, hit for
            // real within an hour of introducing it.
            .env("WSLENV", wslenv_including("STRAF3_PRESENT_MODE"))
            .status()
            .map_err(|e| {
                format!(
                    "could not run {}: {e}\n(has it been built for \
                     x86_64-pc-windows-gnu?)",
                    target_binary.exe()
                )
            })?;
        if !status.success() {
            eprintln!("the run exited with {status}");
            all_ok = false;
        }

        match parse(&log_path) {
            Ok(log) => match stats(&log.deltas_ns, warmup) {
                Some(s) => print!("\n{}", report(&log, &s)),
                None => {
                    eprintln!("{}: no frame intervals recorded", log_path.display());
                    all_ok = false;
                }
            },
            Err(e) => {
                eprintln!("{e}");
                all_ok = false;
            }
        }
    }

    println!(
        "\nThe panel's refresh rate is NOT measured here — it is a property of \
         the display, and the only thing these numbers can show is whether the \
         loop is locked to it. State the panel separately."
    );
    Ok(all_ok)
}

/// Which program to measure.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Binary {
    /// `straf3-game`: the client. The number criterion 7 is about.
    Client,
    /// `straf3-render`'s `window` example: the renderer's own slice. Same
    /// surface, same present mode, same loop shape, no game logic.
    RendererExample,
}

impl Binary {
    fn slug(self) -> &'static str {
        match self {
            Self::Client => "straf3",
            Self::RendererExample => "render-window",
        }
    }

    fn exe(self) -> &'static str {
        match self {
            Self::Client => "./target/x86_64-pc-windows-gnu/release/straf3.exe",
            Self::RendererExample => {
                "./target/x86_64-pc-windows-gnu/release/examples/window.exe"
            }
        }
    }

    fn build_args(self) -> Vec<&'static str> {
        let mut args = vec!["build", "--release", "--target", "x86_64-pc-windows-gnu"];
        match self {
            Self::Client => args.extend(["-p", "straf3-game"]),
            Self::RendererExample => {
                args.extend(["-p", "straf3-render", "--example", "window"]);
            }
        }
        args
    }

    fn run_args(self, exit_after_ms: u64, log: &Path) -> Vec<String> {
        vec![
            "--exit-after".to_owned(),
            exit_after_ms.to_string(),
            "--pacing-log".to_owned(),
            log.display().to_string(),
        ]
    }
}

/// `WSLENV` with `name` added, preserving whatever was already there.
///
/// `WSLENV` is the allow-list of Linux environment variables that WSL interop
/// copies into a Windows child process. A variable absent from it simply does
/// not exist on the other side — silently, with no error — so anything that
/// configures the Windows binary from the environment has to be named here.
fn wslenv_including(name: &str) -> String {
    let existing = std::env::var("WSLENV").unwrap_or_default();
    if existing.split(':').any(|v| v.split('/').next() == Some(name)) {
        return existing;
    }
    if existing.is_empty() {
        name.to_owned()
    } else {
        format!("{existing}:{name}")
    }
}

/// Whether a Windows `.exe` can be launched from this shell.
fn wsl_interop_available() -> bool {
    std::fs::read_to_string("/proc/sys/fs/binfmt_misc/WSLInterop")
        .map(|s| s.contains("enabled"))
        .unwrap_or(false)
}

#[cfg(test)]
mod tests {
    use super::*;

    const SAMPLE: &str = "\
# straf3 pacing log v1
# present_mode=fifo  build=release
# source=test
frame,delta_ns
0,50000000
1,6060606
2,6048112
3,6070000
4,6055000
";

    #[test]
    fn a_log_parses_into_headers_and_intervals() {
        let log = parse_str(Path::new("sample.csv"), SAMPLE).unwrap();
        assert_eq!(log.configured_mode(), Some("fifo"));
        assert_eq!(log.build(), "release");
        assert_eq!(log.header("source"), Some("test"));
        assert_eq!(log.deltas_ns.len(), 5);
    }

    #[test]
    fn a_file_without_the_magic_line_is_refused() {
        // The alternative is reading somebody's spreadsheet as frame times and
        // publishing the result.
        let err = parse_str(Path::new("x.csv"), "frame,delta_ns\n0,1\n").unwrap_err();
        assert!(err.contains("pacing log v1"), "{err}");
    }

    #[test]
    fn a_non_numeric_interval_is_an_error_rather_than_a_zero() {
        let text = "# straf3 pacing log v1\nframe,delta_ns\n0,fast\n";
        assert!(parse_str(Path::new("x.csv"), text).is_err());
    }

    #[test]
    fn the_warm_up_frame_is_dropped_and_the_drop_is_reported() {
        let log = parse_str(Path::new("sample.csv"), SAMPLE).unwrap();
        // Frame 0 is 50 ms — device acquisition. Left in, it would be the max
        // and would drag the mean by 20 %.
        let s = stats(&log.deltas_ns, 1).unwrap();
        assert_eq!(s.dropped, 1);
        assert_eq!(s.n, 4);
        assert_eq!(s.max_ns, 6_070_000);

        let kept = stats(&log.deltas_ns, 0).unwrap();
        assert_eq!(kept.max_ns, 50_000_000);
    }

    #[test]
    fn percentiles_are_observed_samples() {
        let sorted = [1u64, 2, 3, 4, 5, 6, 7, 8, 9, 10];
        assert_eq!(percentile(&sorted, 50), 5);
        assert_eq!(percentile(&sorted, 99), 10);
        assert_eq!(percentile(&sorted, 100), 10);
        // Every answer is a member of the input — never an interpolation.
        for p in [1, 25, 50, 75, 90, 99] {
            assert!(sorted.contains(&percentile(&sorted, p)));
        }
    }

    #[test]
    fn a_vsync_locked_run_reads_as_locked_and_a_free_one_does_not() {
        // This is the statistic that settles the wave's open question, so it
        // is pinned in both directions.
        let locked: Vec<u64> = (0..1000).map(|i| 6_060_000 + (i % 7) * 1_000).collect();
        let locked = stats(&locked, 0).unwrap();
        assert!(locked.lock_permille >= 990, "{locked:?}");
        assert!((locked.implied_hz() - 165.0).abs() < 1.0, "{locked:?}");
        assert!(locked.paced_externally(), "{locked:?}");

        let free: Vec<u64> = (0..1000).map(|i| 1_000_000 + (i % 500) * 20_000).collect();
        let free = stats(&free, 0).unwrap();
        assert!(free.lock_permille < 100, "{free:?}");
        assert!(!free.paced_externally(), "{free:?}");
    }

    #[test]
    fn a_very_regular_uncapped_run_is_not_mistaken_for_vsync() {
        // Regression, from a real measurement. Uncapped on this hardware the
        // renderer produced ~4000 fps with intervals clustered inside 40 µs —
        // more regular than the vsynced run. With an absolute ±250 µs window
        // that read as 98 % locked and the report announced a display refresh.
        // Two things fix it: a tolerance proportional to the median, and the
        // requirement that the beat be a rate a display can actually produce.
        let uncapped: Vec<u64> = (0..2000).map(|i| 224_000 + (i % 40) * 1_000).collect();
        let s = stats(&uncapped, 0).unwrap();
        assert!(s.lock_permille > 900, "still regular: {s:?}");
        assert!(!s.paced_externally(), "but not display-paced: {s:?}");
        assert!(s.implied_hz() > 4_000.0, "{s:?}");

        let log = Log {
            path: PathBuf::from("uncapped.csv"),
            headers: vec![("present_mode".into(), "immediate".into())],
            deltas_ns: uncapped,
        };
        let text = report(&log, &s);
        assert!(text.contains("NOT DISPLAY-PACED"), "{text}");
    }

    #[test]
    fn an_empty_log_yields_no_statistics_rather_than_zeros() {
        assert!(stats(&[], 0).is_none());
        assert!(stats(&[1, 2], 5).is_none());
    }

    #[test]
    fn the_report_names_the_mode_and_the_build_it_measured() {
        let log = parse_str(Path::new("sample.csv"), SAMPLE).unwrap();
        let s = stats(&log.deltas_ns, 1).unwrap();
        let text = report(&log, &s);
        assert!(text.contains("present_mode=fifo"), "{text}");
        assert!(text.contains("build=release"), "{text}");
        assert!(text.contains("p99"), "{text}");
    }

    #[test]
    fn a_log_that_only_knows_what_it_asked_for_is_reported_as_unconfirmed() {
        // A client can emit the mode it put in STRAF3_PRESENT_MODE without
        // knowing whether the surface honoured it. The report must not launder
        // that into "measured under immediate".
        let requested = SAMPLE.replace("# present_mode=fifo", "# present_mode_requested=immediate");
        let log = parse_str(Path::new("sample.csv"), &requested).unwrap();
        assert!(!log.mode_is_measured());
        assert_eq!(log.requested_mode(), Some("immediate"));
        let text = report(&log, &stats(&log.deltas_ns, 1).unwrap());
        assert!(text.contains("present_mode_requested=immediate"), "{text}");
        assert!(text.contains("THE CONFIGURED PRESENT MODE IS NOT RECORDED"), "{text}");
        assert!(text.contains("straf3-render"), "{text}");

        // …and a log that says nothing at all about the mode is treated the
        // same way, rather than as a default.
        let silent = SAMPLE.replace("# present_mode=fifo  build=release", "# build=release");
        let log = parse_str(Path::new("sample.csv"), &silent).unwrap();
        assert!(!log.mode_is_measured());
        let text = report(&log, &stats(&log.deltas_ns, 1).unwrap());
        assert!(text.contains("not recorded"), "{text}");

        // The measured case says so plainly and carries no caveat.
        let log = parse_str(Path::new("sample.csv"), SAMPLE).unwrap();
        assert!(log.mode_is_measured());
        let text = report(&log, &stats(&log.deltas_ns, 1).unwrap());
        assert!(text.contains("as configured on the surface"), "{text}");
        assert!(!text.contains("NOT RECORDED"), "{text}");
    }

    #[test]
    fn wslenv_gains_the_variable_without_losing_what_was_there() {
        // Overwriting WSLENV would break whatever else the shell was passing
        // through (Windows Terminal sets WT_SESSION here), and forgetting it
        // entirely means the present mode silently never arrives.
        unsafe { std::env::set_var("WSLENV", "WT_SESSION:WT_PROFILE_ID:") };
        let v = wslenv_including("STRAF3_PRESENT_MODE");
        assert!(v.starts_with("WT_SESSION:WT_PROFILE_ID:"), "{v}");
        assert!(v.contains("STRAF3_PRESENT_MODE"), "{v}");

        unsafe { std::env::set_var("WSLENV", "STRAF3_PRESENT_MODE") };
        assert_eq!(wslenv_including("STRAF3_PRESENT_MODE"), "STRAF3_PRESENT_MODE");
        // A path-translation flag on the name still counts as the name.
        unsafe { std::env::set_var("WSLENV", "STRAF3_PRESENT_MODE/p") };
        assert_eq!(
            wslenv_including("STRAF3_PRESENT_MODE"),
            "STRAF3_PRESENT_MODE/p"
        );

        unsafe { std::env::remove_var("WSLENV") };
        assert_eq!(wslenv_including("STRAF3_PRESENT_MODE"), "STRAF3_PRESENT_MODE");
    }

    #[test]
    fn the_magic_line_matches_the_writer() {
        // `xtask` cannot depend on `straf3-devtools` — it has no dependencies
        // by design — so the constant is duplicated. This is the thing that
        // notices if the two ever drift apart.
        //
        // The path is derived from CARGO_MANIFEST_DIR rather than the process
        // working directory, because `cargo test -p xtask` runs the test with
        // its cwd at `xtask/`, not at the workspace root.
        let root = Path::new(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .expect("xtask/ has a parent");
        let writer = std::fs::read_to_string(root.join("crates/straf3-devtools/src/pacing.rs"))
            .expect("read the writer's source");
        let declared = format!("pub const MAGIC: &str = {:?};", straf3_pacing_magic());
        assert!(
            writer.contains(&declared),
            "straf3-devtools' MAGIC no longer matches xtask's copy:\n  expected to find {declared}"
        );
    }
}
