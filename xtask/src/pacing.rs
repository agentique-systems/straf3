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
///
/// Hand-copied from `straf3-game`, like [`straf3_pacing_magic`], and pinned by
/// `the_accumulator_replay_still_describes_the_client`.
const MAX_TICKS_PER_FRAME: u64 = 250;

/// How finely arrival times are sampled when building the wait distribution.
/// 50 µs is far below any interval being measured and costs a few hundred
/// thousand comparisons over a twelve-second run.
///
/// **This constant is the one modelled element of the entire input-to-
/// simulation accounting.** Sweeping arrivals uniformly in time is a prior
/// about when a player presses a key; it is not a measurement of one, and no
/// input event is timestamped anywhere in straf3. Everything else below is
/// either an interval this run recorded or arithmetic the client itself
/// performs. [`latency_report`] therefore prints this number rather than
/// leaving it in a source comment, because a reader who cannot see the
/// assumption cannot weigh the result.
const ARRIVAL_STEP_NS: u64 = 50_000;

/// Four numbers about one wait.
///
/// A component of the chain is published as a distribution rather than as an
/// average, so each one carries its own tail. The mean and the p99 of an input
/// wait answer different questions and the second is the one a player notices.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Spread {
    pub mean_ns: u64,
    pub p50_ns: u64,
    pub p99_ns: u64,
    pub max_ns: u64,
}

impl Spread {
    /// Reduce `samples`, sorting them in place.
    fn of(samples: &mut [u64]) -> Self {
        if samples.is_empty() {
            return Self::default();
        }
        samples.sort_unstable();
        // `u128` because a stalled run's samples are hundreds of milliseconds
        // each and there are a few hundred thousand of them.
        let total: u128 = samples.iter().copied().map(u128::from).sum();
        Self {
            mean_ns: u64::try_from(total / samples.len() as u128).unwrap_or(u64::MAX),
            p50_ns: percentile(samples, 50),
            p99_ns: percentile(samples, 99),
            max_ns: samples[samples.len() - 1],
        }
    }

    /// `mean … p50 … p99 … max …`, in milliseconds.
    fn row(self) -> String {
        format!(
            "mean {}  p50 {}  p99 {}  max {}",
            ms(self.mean_ns),
            ms(self.p50_ns),
            ms(self.p99_ns),
            ms(self.max_ns)
        )
    }
}

/// How long an input waits between the raw event becoming available to the
/// process and the simulated command that carries it running.
///
/// This is a *derivation from measured frame times*, not a model with invented
/// numbers in it: the client's own integer accumulator is replayed over the
/// frame intervals that were actually recorded, and the answer is the wait
/// until the next frame that ran a tick. The single assumption is
/// [`ARRIVAL_STEP_NS`]'s uniform arrival prior.
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
    /// The prior's sampling interval, carried in the data so the report can
    /// print the assumption instead of burying it.
    pub arrival_step_ns: u64,
    /// **Stage B.** From the event being available to the process, to the frame
    /// boundary at which winit dispatches it into `InputState`.
    pub queue: Spread,
    /// **Stage C.** From that boundary, to the first boundary that runs a
    /// command. Zero whenever the dispatching frame runs one itself, which at
    /// 165 fps and 8 ms commands is most of them.
    pub carry: Spread,
    /// **B + C, arrival by arrival** — not the sum of the two rows above.
    ///
    /// Because it is accumulated per arrival, its p99 is the p99 of the sum
    /// rather than the sum of two p99s, and no independence has to be assumed
    /// between the two stages (they are strongly anti-correlated: the longer an
    /// input waits in the queue, the closer it lands to a boundary that runs a
    /// command).
    pub total: Spread,
}

/// Replay the client's fixed-step accumulator over measured frame intervals and
/// report how long an input waits for the command that carries it.
///
/// # The loop this replays
///
/// `straf3-game` reads the clock once per frame, converts it to whole
/// milliseconds, and spends whole ticks out of a carried remainder
/// (`tick::plan_ticks`). Every command produced in one frame is built from the
/// *same* `InputState` snapshot, so an input that has reached `InputState` is
/// carried by the first command of the next frame that runs any — and a frame
/// that runs none does not carry it at all.
///
/// The truncation matters and is reproduced: `Clock` truncates the *absolute*
/// reading and subtracts, so a 6.0606 ms frame alternates between 6 ms and
/// 7 ms of simulated credit rather than losing 0.0606 ms every frame.
///
/// # Why stages B and C come out of one sweep
///
/// An earlier version of this function reported a single number and
/// [`latency_report`] presented it as stage C alone — the wait *after*
/// `InputState` — then added a whole frame interval on top of it for the queue
/// wait. That double-counted the queue, and the two rows it added were never
/// independent.
///
/// The order of events in one iteration of the loop is what makes it one wait
/// rather than two. `about_to_wait` requests a redraw, so the next iteration
/// has both queued input and a pending paint — and Win32 resolves that in
/// input's favour, by documented design:
///
/// > With the exception of the `WM_PAINT` message, the `WM_TIMER` message, and
/// > the `WM_QUIT` message, the system always posts messages at the end of a
/// > message queue. […] The `WM_PAINT` message, the `WM_TIMER` message, and the
/// > `WM_QUIT` message, however, are kept in the queue and are forwarded to the
/// > window procedure **only when the queue contains no other messages**.
///
/// — Microsoft, *About Messages and Message Queues*
/// (`learn.microsoft.com/en-us/windows/win32/winmsg/about-messages-and-message-queues`).
///
/// Keyboard and mouse messages are ordinary queued messages posted at the tail,
/// so winit dispatches every pending input event into `InputState` and *then*
/// delivers `RedrawRequested`, which is where the clock is read and the
/// commands run. The boundary that dispatches an input is therefore the first
/// frame boundary after that input arrived, and the first command-running
/// boundary at or after the dispatch is the same boundary you reach by counting
/// from the arrival itself.
///
/// So: sweep arrivals, and for each one record where it was dispatched
/// (stage B), how long from there to the command (stage C), and the whole wait.
/// The three are consistent by construction — `queue + carry == total` for
/// every sampled arrival — which is what makes the total's p99 the p99 of the
/// sum rather than an upper bound on it.
///
/// The B/C boundary is placed at the frame timestamp, and the real dispatch
/// happens a few microseconds *before* it — the input messages are drained,
/// then `WM_PAINT` is retrieved, then the timestamp is taken. So B is
/// overstated and C understated by that same microsecond-scale amount, on a
/// scale where the numbers being reported are milliseconds. **The total is
/// unaffected**, because the error cancels between the two.
///
/// If the ordering were the other way round — the paint delivered before the
/// pending input — this function would *under*-count by one frame interval
/// rather than the old report over-counting by one. Either way the two rows
/// were never addable; the documentation quoted above is what settles which of
/// the two it is.
///
/// # What is NOT derived here
///
/// Everything before the event is available to the process: the device's own
/// polling interval and the driver. Neither is observable from inside the
/// process, and neither is recorded anywhere in this repository. And everything
/// after the simulation — see [`display_report`].
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
    let mut boundaries: Vec<u64> = Vec::with_capacity(kept.len());
    let mut ran_command: Vec<bool> = Vec::with_capacity(kept.len());
    let mut tick_frames = 0usize;
    let mut last_command_boundary: Option<u64> = None;

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
        ticks_total += ticks;
        boundaries.push(boundary_ns);
        ran_command.push(ticks > 0);
        if ticks > 0 {
            tick_frames += 1;
            last_command_boundary = Some(boundary_ns);
        }
    }

    // A run in which no frame ever accumulated a whole command has no wait to
    // report, which is a different answer from a wait of zero.
    let last = last_command_boundary?;

    // For each boundary, the first boundary at or after it that ran a command.
    // One backwards pass, so the sweep below stays linear.
    let mut command_after: Vec<u64> = vec![0; boundaries.len()];
    let mut seen = u64::MAX;
    for i in (0..boundaries.len()).rev() {
        if ran_command[i] {
            seen = boundaries[i];
        }
        command_after[i] = seen;
    }

    // Arrivals are swept uniformly — the prior for "a player pressed a key at
    // some moment", and the one assumption in here (see `ARRIVAL_STEP_NS`).
    let capacity = (last / ARRIVAL_STEP_NS) as usize + 1;
    let mut queue: Vec<u64> = Vec::with_capacity(capacity);
    let mut carry: Vec<u64> = Vec::with_capacity(capacity);
    let mut total: Vec<u64> = Vec::with_capacity(capacity);
    let mut next = 0usize;
    let mut arrival = 0u64;
    while arrival <= last {
        while boundaries[next] < arrival {
            next += 1;
        }
        // `arrival <= last` guarantees a command-running boundary at or after
        // `boundaries[next]`, so `command_after` is a real time and not the
        // sentinel.
        let dispatch = boundaries[next];
        let command = command_after[next];
        queue.push(dispatch - arrival);
        carry.push(command - dispatch);
        total.push(command - arrival);
        arrival += ARRIVAL_STEP_NS;
    }

    Some(LatencyStats {
        tick_ms,
        frames: kept.len(),
        tick_frames,
        ticks: ticks_total,
        arrivals: total.len(),
        arrival_step_ns: ARRIVAL_STEP_NS,
        queue: Spread::of(&mut queue),
        carry: Spread::of(&mut carry),
        total: Spread::of(&mut total),
    })
}

/// The latency accounting, written out with its stages, how each was obtained,
/// and its limits.
///
/// Every row says how it was arrived at, in three words that are used
/// consistently across this whole report and are not interchangeable:
///
/// - **MEASURED** — an interval this run actually recorded.
/// - **DERIVED** — computed from those intervals by arithmetic the client
///   itself performs, with no constant invented on the way.
/// - **MODELLED** — an assumption. There is no measurement behind it anywhere
///   in this repository, and it is somebody's judgement about typical hardware
///   or typical players.
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
        "    {} of {} frames ran a command ({} commands over the run), \
         {} arrivals sampled",
        latency.tick_frames, latency.frames, latency.ticks, latency.arrivals
    );
    let _ = writeln!(
        out,
        "    MEASURED = an interval this run recorded.  DERIVED = the client's own\n\
         \x20   arithmetic over those intervals.  MODELLED = an assumption, with no\n\
         \x20   measurement behind it anywhere in this repository. Figures are ms.",
    );
    let _ = writeln!(
        out,
        "    A    device -> the event reaches this process   MODELLED  see below",
    );
    let _ = writeln!(
        out,
        "    B    available -> winit dispatches it           DERIVED   {}",
        latency.queue.row(),
    );
    let _ = writeln!(
        out,
        "    C    dispatched -> the command runs             DERIVED   {}",
        latency.carry.row(),
    );
    let _ = writeln!(
        out,
        "    B+C  available -> the command runs              DERIVED   {}",
        latency.total.row(),
    );
    let _ = writeln!(
        out,
        "    B+C is accumulated arrival by arrival, so B + C = B+C holds for every\n\
         \x20   sample and its p99 is the p99 of the sum — not the sum of two p99s.\n\
         \x20   Do NOT add rows B and C: the B+C row already IS their sum. Adding the\n\
         \x20   percentile columns instead gives a slightly larger number, because the\n\
         \x20   two stages are anti-correlated (the longer an input sits in the queue,\n\
         \x20   the closer it lands to a boundary that runs a command). And adding a\n\
         \x20   whole frame interval on top of the B+C row — which is what this report\n\
         \x20   did until this wave — counts the queue twice outright.\n\
         \x20   That B and C are one wait rests on Win32 delivering WM_PAINT only when\n\
         \x20   the queue holds no other message, so pending input reaches InputState\n\
         \x20   before the frame that then runs the commands (Microsoft, \"About\n\
         \x20   Messages and Message Queues\").",
    );
    let _ = writeln!(
        out,
        "    METHOD (B, C, B+C): straf3_game::tick::plan_ticks replayed over the frame\n\
         \x20   intervals THIS run measured, reproducing straf3_platform::Clock's\n\
         \x20   truncation of the absolute reading and the {MAX_TICKS_PER_FRAME}-tick\n\
         \x20   per-frame cap. Nothing in those three rows is a guess about hardware.",
    );
    let _ = writeln!(
        out,
        "    MODELLED WITHIN THEM: the arrival prior. Arrivals are swept uniformly in\n\
         \x20   time, every {} ms, as the prior for \"a player pressed a key at some\n\
         \x20   moment\". No input event is timestamped anywhere in straf3, so this\n\
         \x20   prior — not a measurement — is what turns frame intervals into a\n\
         \x20   latency distribution. A player whose keypresses correlate with what is\n\
         \x20   on screen would not be uniform, and nothing here would notice.",
        ms(latency.arrival_step_ns),
    );
    let _ = writeln!(
        out,
        "    STAGE A is not measured and is not measurable in-process: it is the USB\n\
         \x20   polling interval a device's rate means (~1 ms at 1000 Hz, up to 8 ms at\n\
         \x20   125 Hz) plus the driver's own path. This repository does not record\n\
         \x20   which devices were attached, so even those figures describe a class of\n\
         \x20   hardware rather than this machine. Measuring it needs an instrumented\n\
         \x20   mouse or external capture hardware.",
    );
    // A consistency check a reader can apply without re-running anything: the
    // queue wait is bounded by the frame it lands in, so its maximum cannot
    // exceed the longest frame. If it does, the two halves of this report were
    // computed from different data.
    let _ = writeln!(
        out,
        "    cross-check: B max {} ms vs longest frame {} ms — B cannot exceed the\n\
         \x20   frame it lands in, and both come from the same intervals.",
        ms(latency.queue.max_ns),
        ms(frame.max_ns),
    );
    let _ = writeln!(
        out,
        "    THE FLOOR IS THE COMMAND PERIOD, not the frame rate. At {} ms commands an\n\
         \x20   input waits about half a command on average however fast the frames\n\
         \x20   are; a higher frame rate buys the tail, not the median.",
        latency.tick_ms,
    );
    out
}

// ── simulation to display ───────────────────────────────────────────────────

/// `desired_maximum_frame_latency` as the client configured it, from the run's
/// own header.
///
/// The only lever in the whole input-to-photon chain that belongs to this
/// project rather than to the hardware, and the only **configured** number in
/// the simulation-to-display accounting.
///
/// A free function reading [`Log::header`] rather than a method on [`Log`]:
/// this file is being edited by two seats at once this wave, and the header
/// accessors belong to whoever owns the log *format*. Adding a method here
/// would collide on merge with an identical one; reading the header the way any
/// other consumer would cannot.
fn configured_frame_latency(log: &Log) -> Option<u32> {
    log.header("frame_latency").and_then(|v| v.parse().ok())
}

/// The panel's refresh rate in millihertz, as the window's monitor reported it.
///
/// Measured, not stated: it comes from winit's monitor handle at the time of
/// the run. A log without this header was written before the client recorded
/// it, and the scanout accounting then has no basis at all — which is reported
/// as a gap rather than filled in with 165.
fn measured_refresh_mhz(log: &Log) -> Option<u64> {
    log.header("refresh_mhz").and_then(|v| v.parse().ok())
}

/// The simulation-to-display half of the chain, component by component.
///
/// # Why this is a much weaker document than the input half
///
/// The input half is derived end to end from intervals this run recorded. This
/// half is not, and pretending otherwise would be the exact failure r17 exists
/// to prevent. Of its four components, one is configured, one is arithmetic
/// over a measured refresh rate, one is bounded but not measured, and one
/// cannot be obtained on this machine at all. Each says which it is.
///
/// Nothing here is added into a single "input-to-photon" number, because a
/// total whose largest term is unmeasurable is a number that would be quoted
/// without its caveat within a week.
#[must_use]
pub fn display_report(log: &Log, frame: &Stats) -> String {
    let mut out = String::new();
    let _ = writeln!(out, "  simulation-to-display:");

    let mode = log.configured_mode();
    let refresh_hz = measured_refresh_mhz(log).map(|mhz| mhz as f64 / 1000.0);
    let present_interval_ns = refresh_hz
        .filter(|hz| *hz > 0.0)
        .map(|hz| (1e9 / hz) as u64);

    // D — the frame the command belongs to is submitted. Same frame, so it is
    // bounded by that frame's interval, but nothing times it.
    let _ = writeln!(
        out,
        "    D  the command runs -> the frame is submitted   NOT MEASURED\n\
         \x20      No timestamp exists between the last command of a frame and the\n\
         \x20      present call; the pacing log records frame boundaries, not the work\n\
         \x20      between them. It is bounded above by the frame it belongs to:\n\
         \x20      p50 {} ms, p99 {} ms, max {} ms — a bound, not a value.",
        ms(frame.p50_ns),
        ms(frame.p99_ns),
        ms(frame.max_ns),
    );

    // E — the queue depth. Ours, configured, and reported by the client.
    match configured_frame_latency(log) {
        Some(n) => {
            let _ = writeln!(
                out,
                "    E  submitted -> the GPU presents it             CONFIGURED depth {n}\n\
                 \x20      `desired_maximum_frame_latency={n}`, as this run configured it and\n\
                 \x20      wrote into its own header. wgpu defines it as the desired maximum\n\
                 \x20      number of MONITOR REFRESHES between acquiring a texture and that\n\
                 \x20      texture being presented — \"frames in flight\". On the Vulkan backend\n\
                 \x20      this client runs on, it becomes a swapchain image count of {}\n\
                 \x20      (wgpu-hal vulkan/swapchain/native.rs: min_image_count = latency + 1).\n\
                 \x20      This is the one component of the chain that is ours rather than the\n\
                 \x20      hardware's.\n\
                 \x20      CAVEAT, and it is wgpu's own word: the value is a HINT, \"always\n\
                 \x20      clamped to the supported range\". The client does not read back what\n\
                 \x20      the driver granted, so {n} is what was asked for, in a way the\n\
                 \x20      present MODE is not — that one is read back and reported.",
                n + 1,
            );
            match (mode, present_interval_ns) {
                (Some(m @ ("fifo" | "fifo_relaxed")), Some(interval)) => {
                    let _ = writeln!(
                        out,
                        "\x20      In {m} on this panel, a queued frame waits one present interval\n\
                         \x20      for every frame already ahead of it, so the depth costs up to\n\
                         \x20      {} ms — on top of the {} ms a frame then takes to scan out.\n\
                         \x20      DERIVED from the depth above and the measured refresh below.",
                        ms(interval.saturating_mul(u64::from(n))),
                        ms(interval),
                    );
                }
                (Some(m @ ("fifo" | "fifo_relaxed")), None) => {
                    let _ = writeln!(
                        out,
                        "\x20      In {m} the queue is paced by the display, so the depth costs up\n\
                         \x20      to {n} present intervals — but this log does not record the\n\
                         \x20      refresh, so that is a count of intervals and not a duration.",
                    );
                }
                (Some(m), _) => {
                    let _ = writeln!(
                        out,
                        "\x20      In {m} the queue is not paced by the display, so the depth\n\
                         \x20      bounds how far ahead the CPU may run rather than adding a fixed\n\
                         \x20      wait. Its cost here is NOT DERIVED — it depends on where the\n\
                         \x20      GPU's own work sits relative to the panel's scanout, which this\n\
                         \x20      run does not observe.",
                    );
                }
                (None, _) => {}
            }
        }
        None => {
            let _ = writeln!(
                out,
                "    E  submitted -> the GPU presents it             NOT RECORDED\n\
                 \x20      This log carries no `frame_latency=` header, so the swapchain queue\n\
                 \x20      depth this run used is unknown. It is not assumed to be the\n\
                 \x20      renderer's default: a run whose depth is unknown cannot have this\n\
                 \x20      component of its latency accounted for at all.",
            );
        }
    }

    // F — scanout, arithmetic over the measured refresh.
    match (refresh_hz, present_interval_ns) {
        (Some(hz), Some(interval)) => {
            let _ = writeln!(
                out,
                "    F  presented -> the pixel is scanned out        DERIVED\n\
                 \x20      The panel reported {hz:.3} Hz through winit's monitor handle for this\n\
                 \x20      run — measured, not stated — so a whole frame takes {} ms to scan\n\
                 \x20      out. A given pixel is lit between 0 and that, depending only on\n\
                 \x20      where it sits on the screen: mean {} ms, worst {} ms. This is\n\
                 \x20      arithmetic over a measured refresh and nothing else.",
                ms(interval),
                ms(interval / 2),
                ms(interval),
            );
        }
        _ => {
            let _ = writeln!(
                out,
                "    F  presented -> the pixel is scanned out        NOT RECORDED\n\
                 \x20      This log carries no `refresh_mhz=` header. The panel's refresh is a\n\
                 \x20      property of the display, not of the loop, and no number in this\n\
                 \x20      report can supply it — a scanout figure computed from an assumed\n\
                 \x20      refresh would be a model wearing a measurement's clothes.",
            );
        }
    }

    // G — the panel. Nobody in this process can see it.
    let _ = writeln!(
        out,
        "    G  scanned out -> the pixel has changed         NOT MEASURABLE HERE\n\
         \x20      Panel pixel response and any panel-internal processing. No number is\n\
         \x20      offered: this repository does not record the monitor's model, and\n\
         \x20      even if it did, a spec-sheet response time is a vendor's claim about\n\
         \x20      a transition, not this transition. Measuring it needs a high-speed\n\
         \x20      camera or an LDAT-class instrument photographing the panel, which is\n\
         \x20      external capture hardware nobody in this session has.",
    );

    // The finding that makes E awkward, and it is worth stating in the report
    // rather than only in a review: the one knob we own is invisible to the one
    // instrument we have.
    let _ = writeln!(
        out,
        "    E IS NOT VISIBLE TO THIS INSTRUMENT. Queue depth changes when a frame is\n\
         \x20   displayed, not how often frames are displayed: in FIFO every frame still\n\
         \x20   lands on a vblank whatever the depth, so a run at depth 1 and a run at\n\
         \x20   depth 2 can produce the same frame-time distribution while differing by\n\
         \x20   a refresh interval of latency. Nothing in a pacing log can separate\n\
         \x20   them. Settling what the depth costs needs external capture hardware, or\n\
         \x20   a player who can feel the difference — it is a playtest question, not a\n\
         \x20   measurement this tooling can answer.",
    );
    let _ = writeln!(
        out,
        "    NO INPUT-TO-PHOTON TOTAL IS PRINTED. A, D, G and E's cost are not measured\n\
         \x20   on this machine, and a sum whose largest term is a guess would be quoted\n\
         \x20   without its caveat within a week. What can be stated end to end is\n\
         \x20   B+C above, plus F, plus the gaps named by letter.",
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
                    print!("{}", display_report(&log, &s));
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
                    print!("{}", display_report(&log, &s));
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
        assert!((3_500_000..5_000_000).contains(&v.total.mean_ns), "{v:?}");
        assert!((3_500_000..4_500_000).contains(&u.total.mean_ns), "{u:?}");
        assert!(
            v.total.mean_ns > u.total.mean_ns,
            "vsync cannot be the faster one"
        );
        assert!(
            v.total.mean_ns - u.total.mean_ns < 1_000_000,
            "{v:?} vs {u:?}"
        );

        // The worst case does not: a vsynced frame that runs no command makes
        // the next one wait two frames.
        assert!(
            v.total.max_ns > u.total.max_ns + 3_000_000,
            "{v:?} vs {u:?}"
        );
    }

    #[test]
    fn the_two_stages_are_one_wait_and_add_up_to_it() {
        // The property that makes B+C publishable as a p99 rather than as an
        // upper bound: it is accumulated per arrival, so the decomposition is
        // exact at the sample level. Which statistic shows that, and which does
        // not, is the substance of this test — the mean is additive, the
        // percentiles are not, and the maximum is additive only because there
        // is an arrival that suffers both worst cases at once.
        let frames: Vec<u64> = (0..2000).map(|_| 6_060_606).collect();
        let l = input_to_sim(&frames, 8, 0).unwrap();

        // The queue wait cannot exceed the frame it lands in.
        assert!(l.queue.max_ns <= 6_100_000, "{l:?}");
        // The carry is zero for the median input, because most 6.06 ms frames
        // at 8 ms commands do run one.
        assert_eq!(l.carry.p50_ns, 0, "{l:?}");

        // The decomposition is exact per arrival, and the *mean* is the
        // statistic that demonstrates it: means are linear, so B + C = B+C
        // holds to within integer division. Percentiles are not linear, which
        // is the whole reason the two rows must not be added.
        assert!(
            l.total.mean_ns.abs_diff(l.queue.mean_ns + l.carry.mean_ns) <= 2,
            "{l:?}"
        );
        // Adding the two tails instead overstates, because the stages are
        // anti-correlated: the p99 of the sum sits below the sum of the p99s.
        assert!(l.total.p99_ns < l.queue.p99_ns + l.carry.p99_ns, "{l:?}");
        // The maximum is where that relation is tight rather than loose — one
        // arrival really does wait a whole frame to be dispatched and then a
        // whole frame to be carried — so it is subadditive, not strict. Worth
        // pinning: an earlier draft of this test asserted strict inequality
        // here and was simply wrong about the arithmetic.
        assert!(l.total.max_ns <= l.queue.max_ns + l.carry.max_ns, "{l:?}");
    }

    #[test]
    fn a_frame_rate_at_the_command_rate_puts_the_whole_wait_in_the_queue() {
        // Every frame runs a command, so stage C is identically zero and the
        // entire wait is the queue. This is the case that shows the old report
        // was mislabelling: it printed this 4 ms as "stage C, InputState -> the
        // command runs", when stage C here is zero and the 4 ms is stage B.
        let frames: Vec<u64> = (0..1000).map(|_| 8_000_000).collect();
        let l = input_to_sim(&frames, 8, 0).unwrap();
        assert_eq!(l.carry.max_ns, 0, "{l:?}");
        assert_eq!(l.queue.mean_ns, l.total.mean_ns, "{l:?}");
        assert!((3_900_000..4_100_000).contains(&l.total.mean_ns), "{l:?}");
    }

    #[test]
    fn the_command_period_bounds_the_wait_however_fast_the_frames_are() {
        // 0.1 ms frames: the loop is 80x faster than the command rate, and the
        // wait still cannot exceed one command.
        let frames: Vec<u64> = (0..100_000).map(|_| 100_000).collect();
        let l = input_to_sim(&frames, 8, 0).unwrap();
        assert!(l.total.max_ns <= 8_200_000, "{l:?}");
        assert!((3_800_000..4_200_000).contains(&l.total.mean_ns), "{l:?}");
        // At 0.1 ms frames the queue is negligible and the wait is all carry —
        // the mirror image of the 8 ms case above, and the reason the two
        // stages have to be reported separately.
        assert!(l.queue.mean_ns < 100_000, "{l:?}");
        assert!((3_800_000..4_200_000).contains(&l.carry.mean_ns), "{l:?}");
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
        assert!(l.total.max_ns <= 8_100_000, "{l:?}");
    }

    #[test]
    fn a_stalled_frame_makes_an_input_wait_for_it() {
        // A 250 ms hitch is 31 commands' worth of credit, all of which run in
        // the frame that ends the stall — so an input that arrived during it
        // waits the whole stall, and that is what the player feels.
        let mut frames: Vec<u64> = (0..200).map(|_| 6_060_606).collect();
        frames.insert(100, 250_000_000);
        let l = input_to_sim(&frames, 8, 0).unwrap();
        assert!(l.total.max_ns > 240_000_000, "{l:?}");
        // …while the median is untouched, which is exactly why a mean or a
        // median alone is not an honest latency report.
        assert!(l.total.p50_ns < 6_000_000, "{l:?}");
        // The stall belongs to the QUEUE, not to the carry: an input arriving
        // during the hitch is not dispatched until the frame that ends it, and
        // that frame then spends all 31 commands' worth of credit at once. The
        // old report added a whole frame on top of a number that already
        // contained the stall, so a 250 ms hitch was published as ~500 ms.
        assert!(l.queue.max_ns > 240_000_000, "{l:?}");
        assert!(l.carry.max_ns < 10_000_000, "{l:?}");
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
    fn the_display_chain_reads_its_configured_and_measured_facts_from_the_log() {
        // 165 Hz exactly, and the renderer's default queue depth.
        let text = SAMPLE.replace(
            "# source=test",
            "# frame_latency=2  refresh_mhz=165000  source=test",
        );
        let log = parse_str(Path::new("sample.csv"), &text).unwrap();
        assert_eq!(configured_frame_latency(&log), Some(2));
        assert_eq!(measured_refresh_mhz(&log), Some(165_000));

        let out = display_report(&log, &stats(&log.deltas_ns, 0).unwrap());
        // The depth is labelled configured, and its cost is derived from the
        // measured refresh: 2 frames of 6.061 ms.
        assert!(out.contains("CONFIGURED depth 2"), "{out}");
        assert!(out.contains("12.121"), "{out}");
        // Scanout is arithmetic over the refresh the panel reported.
        assert!(out.contains("165.000 Hz"), "{out}");
        assert!(out.contains("6.060") || out.contains("6.061"), "{out}");
        // And the two components nobody can see are named as such rather than
        // filled in.
        assert!(out.contains("NOT MEASURABLE HERE"), "{out}");
        assert!(out.contains("NOT MEASURED"), "{out}");
        // The Vulkan mapping, so a reader knows what the depth became.
        assert!(out.contains("swapchain image count of 3"), "{out}");
        // And no end-to-end number is offered, because four of the terms in it
        // are not measured on this machine.
        assert!(out.contains("NO INPUT-TO-PHOTON TOTAL IS PRINTED"), "{out}");
    }

    #[test]
    fn a_log_without_the_display_headers_reports_gaps_rather_than_defaults() {
        // The failure this guards: assuming `frame_latency=2` because that is
        // the renderer's default, or 165 Hz because that is this machine's
        // panel. Both would be a model presented as a measurement, which is the
        // one outcome r17 forbids.
        let log = parse_str(Path::new("sample.csv"), SAMPLE).unwrap();
        assert_eq!(configured_frame_latency(&log), None);
        assert_eq!(measured_refresh_mhz(&log), None);

        let out = display_report(&log, &stats(&log.deltas_ns, 0).unwrap());
        assert!(out.contains("NOT RECORDED"), "{out}");
        // No configured depth is claimed for this run, and no refresh is
        // invented. (The prose below the table names depth 1 and depth 2 as an
        // illustration, which is why this looks for the row's own marker rather
        // than for the digits.)
        assert!(!out.contains("CONFIGURED depth"), "{out}");
        assert!(!out.contains("swapchain image count"), "{out}");
        assert!(!out.contains("Hz through winit"), "{out}");
    }

    /// The workspace root, from the manifest rather than the process cwd —
    /// `cargo test -p xtask` runs with its cwd at `xtask/`.
    fn workspace_root() -> PathBuf {
        Path::new(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .expect("xtask/ has a parent")
            .to_path_buf()
    }

    #[test]
    fn the_accumulator_replay_still_describes_the_client() {
        // `input_to_sim` is a hand copy of the client's pacing arithmetic:
        // `straf3_game::tick::plan_ticks`, its per-frame cap, and
        // `straf3_platform::Clock`'s truncation of the *absolute* reading.
        // `xtask` has no dependencies by design, so it cannot import the real
        // thing and call it — the same situation `MAGIC` was in.
        //
        // Without this test the drift is silent and expensive: if the client's
        // accumulator changes, the replay keeps reporting the old client's
        // behaviour, and every published latency number describes a game that
        // no longer exists while every test in this file stays green.
        //
        // Whitespace is collapsed before matching, so a reformat is not a false
        // alarm. What this pins is the arithmetic, not the layout.
        //
        // KNOWN LIMIT, stated rather than glossed: this reads source text. It
        // catches the arithmetic being edited, which is how such a change would
        // actually arrive. It does NOT catch the arithmetic staying put while
        // something else changes around it — `advance` ceasing to call
        // `plan_ticks`, or the loop stepping the simulation somewhere else. A
        // behavioural pin would need a crate that can import both sides, and no
        // such crate exists today.
        fn squeeze(text: &str) -> String {
            text.split_whitespace().collect::<Vec<_>>().join(" ")
        }

        let root = workspace_root();
        let required: [(&str, &[&str]); 3] = [
            (
                "crates/straf3-game/src/tick.rs",
                &[
                    // The three lines `input_to_sim`'s inner loop mirrors.
                    "let available = carried_ms as u64 + elapsed_ms;",
                    "let wanted = available / tick;",
                    "let remainder_ms = (available % tick) as u32;",
                    // The cap, and that it truncates rather than dropping the
                    // remainder — `MAX_TICKS_PER_FRAME` here is its copy.
                    "pub const DEFAULT_MAX_TICKS_PER_FRAME: u32 = 250;",
                    "if wanted > max_ticks as u64 { return TickPlan { ticks: max_ticks,",
                    // The rate `DEFAULT_TICK_MS` is a copy of.
                    "pub const DEFAULT_RATE: TickRate = TickRate::HZ_125;",
                ],
            ),
            (
                "crates/straf3-sim/src/cmd.rs",
                &[
                    "pub const HZ_125: Self = Self { hz: 125 };",
                    "pub const fn command_millis(self) -> u16 { (1000 / self.hz) as u16 }",
                ],
            ),
            (
                "crates/straf3-platform/src/clock.rs",
                &[
                    // The truncation of the absolute reading, and the delta as
                    // a difference of two truncated readings. Reproducing this
                    // is what makes a 6.0606 ms frame alternate 6/7 ms of
                    // credit instead of losing 0.0606 ms every frame.
                    "u64::try_from(self.start.elapsed().as_millis()).unwrap_or(u64::MAX)",
                    "let delta_ms = elapsed_ms.saturating_sub(self.last_ms);",
                ],
            ),
        ];

        for (relative, fragments) in required {
            let source = squeeze(
                &std::fs::read_to_string(root.join(relative))
                    .unwrap_or_else(|e| panic!("read {relative}: {e}")),
            );
            for fragment in fragments {
                assert!(
                    source.contains(&squeeze(fragment)),
                    "{relative} no longer contains:\n  {fragment}\n\n\
                     `xtask::pacing::input_to_sim` replays the client's accumulator by \
                     hand and can no longer be assumed to describe it. Re-derive the \
                     replay against the client's current arithmetic before publishing \
                     another latency number, then update this pin.",
                );
            }
        }

        // And the constants themselves, so a reader does not have to trust the
        // text match to believe the two numbers agree.
        assert_eq!(DEFAULT_TICK_MS, 1000 / 125);
        assert_eq!(MAX_TICKS_PER_FRAME, 250);
    }

    #[test]
    fn the_replay_places_commands_where_the_client_does() {
        // A known-answer companion to the source pin above, transcribed from
        // `straf3-game`'s own `the_carry_is_what_makes_a_60hz_frame_rate_average_out`:
        // 16, 17, 16, 17 … is what a 60 Hz display delivers in whole
        // milliseconds, and the client spends two ticks then three, alternating.
        //
        // Fed here as exact nanosecond boundaries so the absolute-reading
        // truncation lands on the same milliseconds the client sees.
        let deltas: Vec<u64> = [16, 17, 16, 17, 16, 17]
            .iter()
            .map(|ms| ms * 1_000_000)
            .collect();
        let l = input_to_sim(&deltas, 8, 0).unwrap();
        // 99 ms of wall time at 8 ms a command is 12 commands with 3 ms carried.
        assert_eq!(l.ticks, 12, "{l:?}");
        assert_eq!(l.frames, 6, "{l:?}");
        // Every one of those frames buys at least one command at 60 Hz.
        assert_eq!(l.tick_frames, 6, "{l:?}");
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
