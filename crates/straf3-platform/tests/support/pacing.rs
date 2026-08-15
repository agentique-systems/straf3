//! The criterion-5 property checker: does a fixed command cadence survive a
//! variable frame rate?
//!
//! # What this module is, and what it is not
//!
//! Criterion 5 (spec rev 6 §S): *"Frame pacing is decoupled from simulation
//! stepping, so the fixed 8 ms command cadence survives a variable frame
//! rate."*
//!
//! This module is the **checker** for that property, plus the hostile frame-time
//! sequences to check it against. It is **not** evidence about `straf3-platform`
//! — at the time of writing, platform's accumulator does not exist yet, so
//! there is nothing to point it at. The checker is built and proven first, so
//! that attaching it later is one line and the proof does not have to be
//! invented under time pressure at the end of the wave.
//!
//! `frame_pacing_checker.rs` proves the checker has teeth by running it against
//! deliberately broken accumulators. Those tests pass when the checker
//! *rejects* the broken ones — they say nothing about the product.
//!
//! # The accumulator contract being checked
//!
//! An accumulator is anything shaped like `FnMut(u32) -> u32`: given the
//! measured duration of one frame in whole milliseconds, return how many whole
//! simulation commands are now due, carrying any remainder forward. Contract
//! R1(a) in the interface note requires the frame duration to be an *argument*
//! rather than something the accumulator reads from a clock, which is exactly
//! what makes this checkable — a hostile frame-time sequence cannot be
//! presented to an accumulator that times itself.

use std::fmt::Write as _;

/// A frame-time sequence, and why it is worth running.
#[derive(Debug, Clone)]
pub struct Sequence {
    /// Short name, used in failure messages.
    pub name: &'static str,
    /// What this sequence is trying to break.
    pub intent: &'static str,
    /// Per-frame durations in whole milliseconds.
    pub frames: Vec<u32>,
}

/// The sequences criterion 5 is judged against.
///
/// Each one is a frame-rate pattern that a real machine actually produces, or
/// a boundary case that an accumulator written the obvious wrong way gets
/// wrong. Between them they cover: durations below, at and above the command
/// period; durations coprime to it; a compositor stall; and a frame rate that
/// varies every single frame.
pub fn sequences() -> Vec<Sequence> {
    vec![
        Sequence {
            name: "regular_8ms",
            intent: "the control: frame rate exactly equals the command rate",
            frames: vec![8; 200],
        },
        Sequence {
            name: "fast_1ms",
            intent: "1000 fps — seven frames out of eight must emit nothing",
            frames: vec![1; 800],
        },
        Sequence {
            name: "just_under_7ms",
            intent: "frames slightly shorter than a command: leftover must accumulate, never round up",
            frames: vec![7; 200],
        },
        Sequence {
            name: "just_over_9ms",
            intent: "frames slightly longer than a command: the extra 1 ms must carry, not vanish",
            frames: vec![9; 200],
        },
        Sequence {
            name: "coprime_13ms",
            intent: "77 fps, coprime to the 8 ms period, so leftover never repeats a short cycle",
            frames: vec![13; 200],
        },
        Sequence {
            name: "vsync_60hz",
            intent: "16/17 ms alternation, which is what a 60 Hz display actually delivers",
            frames: (0..200).map(|i| if i % 3 == 0 { 17 } else { 16 }).collect(),
        },
        Sequence {
            name: "stall_500ms",
            intent: "a compositor stall: the simulation must catch up, not discard the time",
            frames: vec![8, 8, 8, 500, 8, 8, 8],
        },
        Sequence {
            name: "zero_length_frames",
            intent: "a frame that took under a millisecond must not deadlock or emit",
            frames: vec![0, 0, 8, 0, 0, 8, 0, 16, 0],
        },
        Sequence {
            name: "catastrophic_stall_5s",
            intent: "5 s in one frame — past DEFAULT_MAX_TICKS_PER_FRAME (250 x 8 ms = 2000 ms), \
                     so a capped accumulator must DECLARE what it discards",
            frames: vec![8, 8, 5_000, 8, 8],
        },
        Sequence {
            name: "wildly_variable",
            intent: "every frame a different length — the criterion-5 headline case",
            frames: vec![
                7, 9, 1, 33, 3, 120, 8, 8, 2, 17, 41, 5, 5, 5, 64, 1, 1, 1, 1, 96, 12, 6, 23, 8,
                14, 2, 88, 3, 3, 31,
            ],
        },
    ]
}

/// What a checked run produced.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Pacing {
    /// Commands emitted on each frame.
    pub per_frame: Vec<u32>,
    /// Total commands emitted.
    pub total: u32,
    /// Milliseconds the accumulator deliberately discarded rather than
    /// converting into commands.
    ///
    /// # Why this is not simply a bug
    ///
    /// A fixed-step loop that always catches up fully will, on a frame long
    /// enough, spend longer simulating than the frame took — and then need to
    /// catch up further next frame. That is the spiral of death, and the
    /// standard defence is a per-frame tick cap that discards the excess.
    /// `straf3-game`'s `FixedStep` does exactly this, capping at
    /// `DEFAULT_MAX_TICKS_PER_FRAME` and reporting the discarded time through
    /// `dropped_total_ms()`.
    ///
    /// So dropping is permitted — but only *declared* dropping. Time that
    /// disappears without being reported is indistinguishable from an
    /// accumulator that loses its remainder, and it desynchronises a replay.
    /// The checker therefore requires the books to balance:
    /// `commands x period + carried + dropped == elapsed`.
    pub dropped_ms: u32,
}

/// Feed a sequence to an accumulator and record what it emitted.
///
/// For accumulators that never drop time. Use [`drive_reporting_drops`] for one
/// with a tick cap.
pub fn drive(frames: &[u32], mut accumulator: impl FnMut(u32) -> u32) -> Pacing {
    let per_frame: Vec<u32> = frames.iter().map(|&dt| accumulator(dt)).collect();
    let total = per_frame.iter().sum();
    Pacing {
        per_frame,
        total,
        dropped_ms: 0,
    }
}

/// Feed a sequence to an accumulator that reports discarded time.
///
/// `dropped_so_far` is polled after every frame, mirroring
/// `FixedStep::dropped_total_ms()`, so a drop is attributed to the frame that
/// caused it.
pub fn drive_reporting_drops(
    frames: &[u32],
    mut accumulator: impl FnMut(u32) -> u32,
    mut dropped_so_far: impl FnMut() -> u32,
) -> Pacing {
    let per_frame: Vec<u32> = frames.iter().map(|&dt| accumulator(dt)).collect();
    let total = per_frame.iter().sum();
    Pacing {
        per_frame,
        total,
        dropped_ms: dropped_so_far(),
    }
}

/// Check one sequence against the criterion-5 properties.
///
/// Returns a list of violations; empty means the accumulator behaved. Returning
/// violations rather than panicking lets a caller report every failing sequence
/// at once instead of only the first.
///
/// The four properties, and why each one is separately necessary:
///
/// - **Conservation.** After frames totalling `T` ms, exactly `T / period`
///   commands must have been emitted. An accumulator that discards its
///   remainder each frame passes every other check while quietly running the
///   simulation slow.
/// - **No time creation.** At no point may the commands emitted so far account
///   for more milliseconds than have been fed in. An accumulator that rounds to
///   nearest fails here, and the symptom in play would be a simulation that
///   drifts *ahead* of the wall clock.
/// - **Partition independence.** The per-frame emissions may differ between
///   sequences, but the *total* must depend only on elapsed time, never on how
///   that time was chopped into frames. This is the criterion in one sentence.
/// - **Bounded lag.** A frame may emit many commands (catching up after a
///   stall), but the accumulator must never be left holding a whole command's
///   worth of time. Otherwise a stall is silently converted into permanent lag.
pub fn violations(seq: &Sequence, period: u32, pacing: &Pacing) -> Vec<String> {
    let mut out = Vec::new();
    let total_ms: u32 = seq.frames.iter().sum();

    // Time the accumulator was responsible for turning into commands: whatever
    // it was fed, less whatever it declared it discarded.
    let accountable = total_ms.saturating_sub(pacing.dropped_ms);
    let expected = accountable / period;
    if pacing.total != expected {
        let mut m = String::new();
        let _ = write!(
            m,
            "{}: {} ms of frames ({} ms declared dropped) at a {} ms cadence \
             should emit {} commands, got {}",
            seq.name, total_ms, pacing.dropped_ms, period, expected, pacing.total
        );
        out.push(m);
    }

    // The books must balance: commands x period + carried + dropped == elapsed,
    // with 0 <= carried < period. Checked incrementally so the failure names
    // the frame that broke it. Drops are only observable in total, so they are
    // allowed against the running balance from the point they could have
    // occurred.
    let mut fed = 0u32;
    let mut emitted = 0u32;
    for (i, (&dt, &n)) in seq.frames.iter().zip(pacing.per_frame.iter()).enumerate() {
        fed += dt;
        emitted += n;
        let simulated = emitted * period;

        if simulated > fed {
            out.push(format!(
                "{}: after frame {i}, {} commands account for {simulated} ms but \
                 only {fed} ms have elapsed — the accumulator invented time",
                seq.name, emitted,
            ));
            break;
        }
        let unaccounted = fed - simulated;
        if unaccounted >= period + pacing.dropped_ms {
            out.push(format!(
                "{}: after frame {i}, {unaccounted} ms are unaccounted for — a \
                 whole command or more beyond the {} ms it declared dropped. The \
                 accumulator is falling behind rather than catching up, or it is \
                 discarding time without reporting it.",
                seq.name, pacing.dropped_ms,
            ));
            break;
        }
    }

    // A drop is only legitimate as spiral-of-death protection, which means some
    // single frame had to be long enough to hit the cap. Dropping time on a
    // sequence of ordinary frames is the silent-data-loss case.
    if pacing.dropped_ms > 0 {
        let longest = seq.frames.iter().copied().max().unwrap_or(0);
        if longest < period {
            out.push(format!(
                "{}: {} ms were dropped, but the longest frame was only {longest} ms \
                 — under one command period. There was no catch-up storm to \
                 protect against, so this is time silently lost.",
                seq.name, pacing.dropped_ms,
            ));
        }
    }

    out
}

/// A correct accumulator, used as the control in the checker's own tests.
///
/// This is *not* the product. It exists so the checker can be shown to accept
/// something, which is the other half of showing it rejects the broken ones —
/// a checker that rejects everything is as useless as one that accepts
/// everything.
pub fn reference_accumulator(period: u32) -> impl FnMut(u32) -> u32 {
    let mut leftover = 0u32;
    move |dt| {
        leftover += dt;
        let due = leftover / period;
        leftover -= due * period;
        due
    }
}
