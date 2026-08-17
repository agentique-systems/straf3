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
//! [`Stats::consistent_with_display_pacing`] therefore requires two things at once: the
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
/// Relative, not absolute. That correction was made after the statistic got a
/// real answer wrong: an absolute ±250 µs window is 4 % of a 6.06 ms vsynced
/// frame — sensible — but it is *more than the whole frame* when the loop is
/// running uncapped at 0.22 ms, so it called a 4000 fps free-running run
/// "tightly clustered, paced by a display refresh".
///
/// Ten percent, not four, and that is the second correction. Measured on this
/// host, a vsynced run's intervals spread ±13 % at the tails — jitter around
/// one 164.9 Hz beat, not missed vblanks; a histogram against integer
/// multiples of the median found 83.6 % at 1x and essentially nothing at 2x.
/// A ±4 % window called that "inconclusive" on one run and "locked" on the
/// next, which is a threshold reporting its own arbitrariness rather than the
/// display.
const LOCK_TOLERANCE_PERCENT: u64 = 10;

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

    /// The swapchain warm-up interval the writer held out of the data rows.
    ///
    /// Reported rather than ignored: it is a real number about the session,
    /// and seeing it is how a reader knows the warm-up was excluded rather
    /// than never happened.
    #[must_use]
    pub fn warmup_ns(&self) -> Option<u64> {
        self.header("warmup_ns").and_then(|v| v.parse().ok())
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
        // v1 is refused rather than read leniently, and that is the point of
        // the version tag. A v1 file's first data row is a swapchain warm-up
        // interval — 421 ms against a steady 6 ms on one real run — which a v2
        // reader treats as a measurement. Reading it "just to be helpful"
        // would publish that as a worst-case frame time.
        let hint = if magic.starts_with("# straf3 pacing log v1") {
            "\nThis is a v1 log. v1's first data row is the swapchain warm-up \
             interval, which v2 moved into `warmup_ns=` because reading it as a \
             frame time publishes a hundredfold-wrong maximum. Re-record it."
        } else {
            ""
        };
        return Err(format!(
            "{} does not start with {:?} — this is not a straf3 pacing log v2 \
             (first line was {magic:?}){hint}",
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

    // The writer records how many rows it meant to write. Checking it turns a
    // truncated file — a killed process, a full disk — into an error instead
    // of a shorter run that looks perfectly valid.
    if let Some(declared) = headers
        .iter()
        .find(|(k, _)| k == "frames")
        .and_then(|(_, v)| v.parse::<usize>().ok())
        && declared != deltas_ns.len()
    {
        return Err(format!(
            "{}: the header says frames={declared} but {} data rows are present. \
             The file is truncated or was written by something else; its statistics \
             would be of a run that did not happen.",
            path.display(),
            deltas_ns.len()
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
    "# straf3 pacing log v2"
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
    /// Which frame the longest interval was, counting from the first kept one.
    ///
    /// Reported because "max 29.7 ms" means two very different things at frame
    /// 3 and at frame 1200. The first is start-up still settling and argues
    /// for a longer warm-up; the second is a hitch a player felt. Without the
    /// index a reader cannot tell them apart, and the temptation is to raise
    /// the warm-up until the number looks better.
    pub max_at_frame: usize,
    /// Share of intervals within [`LOCK_TOLERANCE_PERCENT`] of the median, in
    /// tenths of a percent. High means regular; on its own it does NOT mean
    /// vsync — see [`Stats::consistent_with_display_pacing`].
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

    /// Whether this run's numbers are *consistent with* the loop waiting on a
    /// display refresh.
    ///
    /// Deliberately not called "was vsynced", because one run cannot establish
    /// that. Two conditions are necessary — the intervals grouped around one
    /// beat, and that beat being a rate a display could produce — but they are
    /// not sufficient: a loop that simply takes 6 ms of work per frame looks
    /// identical from inside one log.
    ///
    /// What settles it is [`compare`]: the same binary and scene, one run per
    /// present mode. If turning the mode off multiplies the frame rate, the
    /// cap was the display. Nothing short of that comparison is evidence, and
    /// this method is named to keep a single number from being quoted as if it
    /// were.
    #[must_use]
    pub fn consistent_with_display_pacing(&self) -> bool {
        self.lock_permille >= 700 && DISPLAY_HZ.contains(&self.implied_hz())
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

    let max_at_frame = kept
        .iter()
        .enumerate()
        .max_by_key(|(_, ns)| **ns)
        .map_or(0, |(i, _)| i);

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
        max_at_frame,
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
            "build"
                | "present_mode"
                | "present_mode_requested"
                | "fell_back"
                | "warmup_ns"
                | "frames"
        ) {
            let _ = writeln!(out, "  {key}={value}");
        }
    }
    let _ = writeln!(
        out,
        "  frames={}{}  span={:.2} s{}",
        stats.n,
        if stats.dropped > 0 {
            format!(" (dropped {} more at the reader's request)", stats.dropped)
        } else {
            String::new()
        },
        stats.total_ns as f64 / 1e9,
        log.warmup_ns().map_or(String::new(), |ns| format!(
            "  (swapchain warm-up of {} ms excluded by the writer)",
            ms(ns)
        )),
    );
    let _ = writeln!(
        out,
        "  frame time ms:  mean {}  p50 {}  p99 {}  max {} (at frame {} of {})  min {}",
        ms(stats.mean_ns),
        ms(stats.p50_ns),
        ms(stats.p99_ns),
        ms(stats.max_ns),
        stats.max_at_frame,
        stats.n,
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
    let verdict = if !DISPLAY_HZ.contains(&stats.implied_hz()) {
        format!(
            "  → CANNOT be display-paced: the beat implies {:.0} Hz, which no display \
             produces. However regular the frames look, nothing here is waiting for a \
             refresh.",
            stats.implied_hz()
        )
    } else if stats.consistent_with_display_pacing() {
        format!(
            "  → consistent with display pacing at {:.2} Hz — but one run cannot prove \
             it. A loop that simply takes {} ms of work per frame looks the same from \
             inside one log. Compare two present modes to settle it.",
            stats.implied_hz(),
            ms(stats.p50_ns),
        )
    } else {
        format!(
            "  → beat {:.2} Hz is in display range, but only {}.{} % of frames keep to \
             it. Too scattered to say anything from this run alone.",
            stats.implied_hz(),
            stats.lock_permille / 10,
            stats.lock_permille % 10,
        )
    };
    let _ = writeln!(out, "{verdict}");
    out
}

/// The cross-run conclusion, which is the only thing that actually settles
/// whether a frame rate was a display cap.
///
/// Takes the per-mode results and states what the *pair* supports. A single
/// log can only ever be consistent with vsync; two logs from the same binary
/// and scene, differing only in present mode, can distinguish a cap imposed by
/// the display from a limit of the renderer — because turning vsync off cannot
/// make a GPU-bound loop faster.
#[must_use]
pub fn compare(runs: &[(String, Stats)]) -> String {
    let mut out = String::new();
    if runs.len() < 2 {
        return out;
    }
    let _ = writeln!(out, "comparison across present modes");
    for (mode, s) in runs {
        let _ = writeln!(
            out,
            "  {mode:<12} p50 {} ms  ({:.1} fps mean)",
            ms(s.p50_ns),
            s.mean_fps()
        );
    }

    let slowest = runs.iter().max_by_key(|(_, s)| s.p50_ns);
    let fastest = runs.iter().min_by_key(|(_, s)| s.p50_ns);
    let (Some((slow_mode, slow)), Some((fast_mode, fast))) = (slowest, fastest) else {
        return out;
    };
    if fast.p50_ns == 0 || slow.p50_ns == fast.p50_ns {
        return out;
    }
    let ratio = slow.p50_ns as f64 / fast.p50_ns as f64;

    if ratio >= 2.0 && DISPLAY_HZ.contains(&slow.implied_hz()) {
        let _ = writeln!(
            out,
            "  → SETTLED: {slow_mode} is {ratio:.1}x slower than {fast_mode} on the same \
             binary and the same scene. The renderer can draw this frame in {} ms, so \
             the {:.1} fps of {slow_mode} is not a limit of the renderer — it is the \
             display, at {:.2} Hz.",
            ms(fast.p50_ns),
            slow.mean_fps(),
            slow.implied_hz(),
        );
    } else {
        let _ = writeln!(
            out,
            "  → the two modes differ by only {ratio:.2}x, so this pair does not \
             establish that {slow_mode} was capped by anything external.",
        );
    }
    out
}

// ── input to simulation ─────────────────────────────────────────────────────

/// The default command duration: 125 Hz, 8 ms, `straf3_game::tick::DEFAULT_RATE`.
pub const DEFAULT_TICK_MS: u16 = 8;

/// `FixedStep::DEFAULT_MAX_TICKS_PER_FRAME`, so a stalled frame is modelled the
/// way the client actually behaves rather than as an unbounded catch-up.
const MAX_TICKS_PER_FRAME: u64 = 250;

/// How finely arrival times are sampled when building the wait distribution.
/// 50 µs is far below any interval being measured and costs a few hundred
/// thousand comparisons over a twelve-second run.
const ARRIVAL_STEP_NS: u64 = 50_000;

/// How long an input waits between reaching `InputState` and being carried by a
/// simulated command.
///
/// This is a *derivation from measured frame times*, not a model with invented
/// numbers in it: the client's own integer accumulator is replayed over the
/// frame intervals that were actually recorded, and the answer is the wait
/// until the next frame that ran a tick.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LatencyStats {
    pub tick_ms: u16,
    /// Frame boundaries considered.
    pub frames: usize,
    /// How many of them executed at least one command.
    pub tick_frames: usize,
    /// Commands simulated across the run.
    pub ticks: u64,
    /// Arrival times sampled.
    pub arrivals: usize,
    pub mean_ns: u64,
    pub p50_ns: u64,
    pub p99_ns: u64,
    pub max_ns: u64,
}

/// Replay the client's fixed-step accumulator over measured frame intervals and
/// report how long an input waits for the command that carries it.
///
/// # What is modelled, exactly
///
/// `straf3-game` reads the clock once per frame, converts it to whole
/// milliseconds, and spends whole ticks out of a carried remainder
/// (`tick::plan_ticks`). Every command produced in one frame is built from the
/// *same* `InputState` snapshot, so an input that has reached `InputState` is
/// carried by the first command of the next frame that runs any — and a frame
/// that runs none does not carry it at all.
///
/// So the wait is: from the input landing in `InputState`, to the next frame
/// boundary at which the accumulator had a whole tick to spend.
///
/// The truncation matters and is reproduced: `Clock` truncates the *absolute*
/// reading and subtracts, so a 6.0606 ms frame alternates between 6 ms and
/// 7 ms of simulated credit rather than losing 0.0606 ms every frame.
///
/// # What is NOT modelled
///
/// Everything before `InputState`: the device's own polling interval, the
/// driver, and the wait in the thread's message queue until winit dispatches.
/// The first two are not observable from inside the process at all. The third
/// is bounded by one frame interval, since the loop pumps events once per
/// frame — so the measured frame-time distribution is its bound, and the two
/// have to be added to get an end-to-end figure.
#[must_use]
pub fn input_to_sim(deltas_ns: &[u64], tick_ms: u16, warmup: usize) -> Option<LatencyStats> {
    if tick_ms == 0 {
        return None;
    }
    let kept = deltas_ns.get(warmup.min(deltas_ns.len())..)?;
    if kept.len() < 2 {
        return None;
    }

    // Absolute frame-boundary times, and which of them ran a command.
    let mut boundary_ns = 0u64;
    let mut previous_ms = 0u64;
    let mut carried = 0u64;
    let mut ticks_total = 0u64;
    let mut tick_boundaries: Vec<u64> = Vec::with_capacity(kept.len());

    for delta in kept {
        boundary_ns += delta;
        // `Clock::elapsed_ms` truncates the absolute reading; the delta is the
        // difference of two truncated readings, never a truncated difference.
        let elapsed_ms = boundary_ns / 1_000_000;
        let delta_ms = elapsed_ms.saturating_sub(previous_ms);
        previous_ms = elapsed_ms;

        let available = carried + delta_ms;
        let wanted = available / u64::from(tick_ms);
        carried = available % u64::from(tick_ms);
        let ticks = wanted.min(MAX_TICKS_PER_FRAME);
        if ticks > 0 {
            ticks_total += ticks;
            tick_boundaries.push(boundary_ns);
        }
    }

    if tick_boundaries.is_empty() {
        return None;
    }

    // An input arriving at `a` waits for the first tick-executing boundary at
    // or after `a`. Arrivals are swept uniformly, which is the right prior for
    // "a player pressed a key at some moment".
    let last = *tick_boundaries.last()?;
    let mut waits: Vec<u64> = Vec::with_capacity((last / ARRIVAL_STEP_NS) as usize + 1);
    let mut next = 0usize;
    let mut arrival = 0u64;
    while arrival <= last {
        while tick_boundaries[next] < arrival {
            next += 1;
        }
        waits.push(tick_boundaries[next] - arrival);
        arrival += ARRIVAL_STEP_NS;
    }

    let mut sorted = waits;
    sorted.sort_unstable();
    let n = sorted.len() as u64;
    let total: u64 = sorted.iter().sum();

    Some(LatencyStats {
        tick_ms,
        frames: kept.len(),
        tick_frames: tick_boundaries.len(),
        ticks: ticks_total,
        arrivals: sorted.len(),
        mean_ns: total / n,
        p50_ns: percentile(&sorted, 50),
        p99_ns: percentile(&sorted, 99),
        max_ns: sorted[sorted.len() - 1],
    })
}

/// The latency accounting, written out with its stages and its limits.
#[must_use]
pub fn latency_report(frame: &Stats, latency: &LatencyStats) -> String {
    let mut out = String::new();
    let _ = writeln!(
        out,
        "  input-to-simulation, at {} Hz commands ({} ms):",
        1000 / u64::from(latency.tick_ms).max(1),
        latency.tick_ms
    );
    let _ = writeln!(
        out,
        "    {} of {} frames ran a command ({} commands over the run)",
        latency.tick_frames, latency.frames, latency.ticks
    );
    let _ = writeln!(
        out,
        "    stage A  device -> Windows raw input        NOT MEASURABLE in-process",
    );
    let _ = writeln!(
        out,
        "    stage B  queued until winit dispatches      <= one frame: p50 {} ms, p99 {} ms, max {} ms",
        ms(frame.p50_ns),
        ms(frame.p99_ns),
        ms(frame.max_ns),
    );
    let _ = writeln!(
        out,
        "    stage C  InputState -> the command runs     mean {} ms, p50 {} ms, p99 {} ms, max {} ms",
        ms(latency.mean_ns),
        ms(latency.p50_ns),
        ms(latency.p99_ns),
        ms(latency.max_ns),
    );
    let _ = writeln!(
        out,
        "    B+C upper bound                            p99 {} ms, worst {} ms",
        ms(frame.p99_ns + latency.p99_ns),
        ms(frame.max_ns + latency.max_ns),
    );
    let _ = writeln!(
        out,
        "    (B and C are not independent, so adding their percentiles is an upper\n\
         \x20    bound, not the p99 of the sum. Stage A is a hardware property — a\n\
         \x20    1000 Hz mouse adds ~1 ms, a 125 Hz keyboard up to 8 ms — and\n\
         \x20    input-to-PHOTON adds display scanout on top of all of it and cannot\n\
         \x20    be measured here at all without external capture hardware.)"
    );
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
    // Zero, because in v2 every data row is a measurement: the writer already
    // held the swapchain warm-up out and put it in `warmup_ns=`. Dropping a
    // row here as well would silently discard a real frame.
    let mut warmup = 0usize;
    let mut tick_ms = DEFAULT_TICK_MS;
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
            "--tick-ms" => {
                let raw = value()?;
                tick_ms = raw
                    .parse()
                    .map_err(|_| format!("`--tick-ms {raw}` is not a number"))?;
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
        let mut runs: Vec<(String, Stats)> = Vec::new();
        for path in &analyse {
            let log = parse(path)?;
            match stats(&log.deltas_ns, warmup) {
                Some(s) => {
                    print!("{}", report(&log, &s));
                    if let Some(l) = input_to_sim(&log.deltas_ns, tick_ms, warmup) {
                        print!("{}", latency_report(&s, &l));
                    }
                    runs.push((
                        log.configured_mode()
                            .map(str::to_owned)
                            .unwrap_or_else(|| format!("{}?", log.requested_mode().unwrap_or("?"))),
                        s,
                    ));
                }
                None => {
                    eprintln!("{}: no frame intervals to report", path.display());
                    ok = false;
                }
            }
            println!();
        }
        print!("{}", compare(&runs));
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
    let mut runs: Vec<(String, Stats)> = Vec::new();
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

        if !log_path.exists() {
            // The writer declines to produce a file when the session drew too
            // few frames to have a steady-state interval. That is a result,
            // not a missing output — and it is a better one than a file whose
            // p99 is taken over nothing.
            eprintln!(
                "{}: no pacing log was written. The session had no measurable \
                 interval — fewer than {} frames drawn. Give it a longer \
                 --exit-after.",
                log_path.display(),
                3
            );
            all_ok = false;
            continue;
        }
        match parse(&log_path) {
            Ok(log) => match stats(&log.deltas_ns, warmup) {
                Some(s) => {
                    print!("\n{}", report(&log, &s));
                    if let Some(l) = input_to_sim(&log.deltas_ns, tick_ms, warmup) {
                        print!("{}", latency_report(&s, &l));
                    }
                    runs.push((log.configured_mode().unwrap_or(mode.as_str()).to_owned(), s));
                }
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

    print!("\n{}", compare(&runs));

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
            Self::RendererExample => "./target/x86_64-pc-windows-gnu/release/examples/window.exe",
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
    if existing
        .split(':')
        .any(|v| v.split('/').next() == Some(name))
    {
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
# straf3 pacing log v2
# present_mode=fifo  build=release
# warmup_ns=50000000  frames=4
# source=test
frame,delta_ns
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
        assert_eq!(log.deltas_ns.len(), 4);
        assert_eq!(log.warmup_ns(), Some(50_000_000));
    }

    #[test]
    fn a_v1_log_is_refused_with_the_reason() {
        // Not read leniently. v1's row 0 is the swapchain warm-up, and a v2
        // reader would take it as a frame time — 421 ms against a steady 6 ms
        // on a real run here.
        let v1 =
            "# straf3 pacing log v1\n# present_mode=fifo\nframe,delta_ns\n0,421000000\n1,6060606\n";
        let err = parse_str(Path::new("old.csv"), v1).unwrap_err();
        assert!(err.contains("v1 log"), "{err}");
        assert!(err.contains("warmup_ns"), "{err}");
    }

    #[test]
    fn a_truncated_file_is_caught_by_the_declared_row_count() {
        let truncated = SAMPLE.replace("4,6055000\n", "");
        let err = parse_str(Path::new("cut.csv"), &truncated).unwrap_err();
        assert!(err.contains("frames=4"), "{err}");
        assert!(err.contains("3 data rows"), "{err}");
    }

    #[test]
    fn a_file_without_the_magic_line_is_refused() {
        // The alternative is reading somebody's spreadsheet as frame times and
        // publishing the result.
        let err = parse_str(Path::new("x.csv"), "frame,delta_ns\n0,1\n").unwrap_err();
        assert!(err.contains("pacing log v2"), "{err}");
    }

    #[test]
    fn a_non_numeric_interval_is_an_error_rather_than_a_zero() {
        let text = "# straf3 pacing log v2\nframe,delta_ns\n0,fast\n";
        assert!(parse_str(Path::new("x.csv"), text).is_err());
    }

    #[test]
    fn every_v2_data_row_is_a_measurement() {
        // The 50 ms warm-up is in the header, not the rows, so taking every
        // row at face value is correct — that is what v2 bought. On v1 the
        // same naive read reported a 421 ms worst-case frame on a 165 Hz
        // display.
        let log = parse_str(Path::new("sample.csv"), SAMPLE).unwrap();
        let s = stats(&log.deltas_ns, 0).unwrap();
        assert_eq!(s.dropped, 0);
        assert_eq!(s.n, 4);
        assert_eq!(s.max_ns, 6_070_000);
        assert!(report(&log, &s).contains("warm-up of 50.000 ms excluded"));
    }

    #[test]
    fn the_worst_frame_is_located_as_well_as_measured() {
        // A 30 ms frame at index 2 is start-up; the same frame at index 1200
        // is a hitch. The report has to let a reader tell which, or the only
        // way to make the number look better is to raise the warm-up until the
        // evidence is gone.
        let mut deltas = vec![6_060_000u64; 500];
        deltas[301] = 30_000_000;
        let s = stats(&deltas, 1).unwrap();
        assert_eq!(s.max_ns, 30_000_000);
        // Indices are relative to the first kept frame, so the dropped warm-up
        // does not silently shift them.
        assert_eq!(s.max_at_frame, 300);
        assert!(
            report(
                &Log {
                    path: PathBuf::from("x.csv"),
                    headers: vec![],
                    deltas_ns: deltas,
                },
                &s
            )
            .contains("at frame 300")
        );
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
        assert!(locked.consistent_with_display_pacing(), "{locked:?}");

        let free: Vec<u64> = (0..1000).map(|i| 1_000_000 + (i % 500) * 20_000).collect();
        let free = stats(&free, 0).unwrap();
        assert!(free.lock_permille < 300, "{free:?}");
        assert!(!free.consistent_with_display_pacing(), "{free:?}");
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
        assert!(
            !s.consistent_with_display_pacing(),
            "but not display-paced: {s:?}"
        );
        assert!(s.implied_hz() > 4_000.0, "{s:?}");

        let log = Log {
            path: PathBuf::from("uncapped.csv"),
            headers: vec![("present_mode".into(), "immediate".into())],
            deltas_ns: uncapped,
        };
        let text = report(&log, &s);
        assert!(text.contains("CANNOT be display-paced"), "{text}");
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
        assert!(
            text.contains("THE CONFIGURED PRESENT MODE IS NOT RECORDED"),
            "{text}"
        );
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
    fn a_faster_frame_rate_does_not_buy_much_input_latency() {
        // The finding this whole accounting exists to make legible: at a fixed
        // 125 Hz command rate, the command period sets the floor. Going from
        // 165 fps to 4000 fps moves the mean wait by well under a millisecond,
        // because the input still has to wait for the next 8 ms command either
        // way. It is the tail that improves.
        let vsynced: Vec<u64> = (0..2000).map(|_| 6_060_606).collect();
        let uncapped: Vec<u64> = (0..48_000).map(|_| 250_000).collect();

        let v = input_to_sim(&vsynced, 8, 0).unwrap();
        let u = input_to_sim(&uncapped, 8, 0).unwrap();

        // Both means sit near half a command period.
        assert!((3_500_000..5_000_000).contains(&v.mean_ns), "{v:?}");
        assert!((3_500_000..4_500_000).contains(&u.mean_ns), "{u:?}");
        assert!(v.mean_ns > u.mean_ns, "vsync cannot be the faster one");
        assert!(v.mean_ns - u.mean_ns < 1_000_000, "{v:?} vs {u:?}");

        // The worst case does not: a vsynced frame that runs no command makes
        // the next one wait two frames.
        assert!(v.max_ns > u.max_ns + 3_000_000, "{v:?} vs {u:?}");
    }

    #[test]
    fn the_command_period_bounds_the_wait_however_fast_the_frames_are() {
        // 0.1 ms frames: the loop is 80x faster than the command rate, and the
        // wait still cannot exceed one command.
        let frames: Vec<u64> = (0..100_000).map(|_| 100_000).collect();
        let l = input_to_sim(&frames, 8, 0).unwrap();
        assert!(l.max_ns <= 8_200_000, "{l:?}");
        assert!((3_800_000..4_200_000).contains(&l.mean_ns), "{l:?}");
        // One command per 8 ms of the ten seconds of frames, not one per frame.
        assert_eq!(l.frames, 100_000);
        assert!(l.tick_frames < 1_300, "{l:?}");
    }

    #[test]
    fn every_frame_runs_a_command_when_the_frame_rate_is_the_command_rate() {
        let frames: Vec<u64> = (0..1000).map(|_| 8_000_000).collect();
        let l = input_to_sim(&frames, 8, 0).unwrap();
        assert_eq!(l.tick_frames, l.frames);
        assert_eq!(l.ticks, l.frames as u64);
        assert!(l.max_ns <= 8_100_000, "{l:?}");
    }

    #[test]
    fn a_stalled_frame_makes_an_input_wait_for_it() {
        // A 250 ms hitch is 31 commands' worth of credit, all of which run in
        // the frame that ends the stall — so an input that arrived during it
        // waits the whole stall, and that is what the player feels.
        let mut frames: Vec<u64> = (0..200).map(|_| 6_060_606).collect();
        frames.insert(100, 250_000_000);
        let l = input_to_sim(&frames, 8, 0).unwrap();
        assert!(l.max_ns > 240_000_000, "{l:?}");
        // …while the median is untouched, which is exactly why a mean or a
        // median alone is not an honest latency report.
        assert!(l.p50_ns < 6_000_000, "{l:?}");
    }

    #[test]
    fn there_is_no_answer_from_a_run_too_short_to_have_one() {
        assert!(input_to_sim(&[], 8, 0).is_none());
        assert!(input_to_sim(&[8_000_000], 8, 0).is_none());
        // Frames that never accumulate a whole millisecond never run a
        // command, so there is no wait to report rather than a wait of zero.
        let sub_ms: Vec<u64> = (0..10).map(|_| 10_000).collect();
        assert!(input_to_sim(&sub_ms, 8, 0).is_none());
        assert!(input_to_sim(&[8_000_000; 10], 0, 0).is_none());
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
        assert_eq!(
            wslenv_including("STRAF3_PRESENT_MODE"),
            "STRAF3_PRESENT_MODE"
        );
        // A path-translation flag on the name still counts as the name.
        unsafe { std::env::set_var("WSLENV", "STRAF3_PRESENT_MODE/p") };
        assert_eq!(
            wslenv_including("STRAF3_PRESENT_MODE"),
            "STRAF3_PRESENT_MODE/p"
        );

        unsafe { std::env::remove_var("WSLENV") };
        assert_eq!(
            wslenv_including("STRAF3_PRESENT_MODE"),
            "STRAF3_PRESENT_MODE"
        );
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
