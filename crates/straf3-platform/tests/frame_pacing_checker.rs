//! Proof that the criterion-5 checker has teeth.
//!
//! # Read this before treating any of it as criterion-5 evidence
//!
//! Criterion 5 (spec rev 6 §S) is about `straf3-platform`'s accumulator, and at
//! the time of writing that accumulator does not exist. **Nothing in this file
//! tests the product.** Every test here runs
//! [`support::pacing::violations`] against a deliberately broken accumulator
//! and passes when the checker *rejects* it.
//!
//! That is worth doing ahead of time for one reason: when platform's
//! accumulator lands, attaching it is a single line, and the question "does
//! this checker actually catch anything?" will already have been answered
//! rather than being answered hurriedly at the end of the wave by whoever is
//! holding the deadline.
//!
//! The broken accumulators below are not strawmen. Each one is the way an
//! accumulator gets written when the author is thinking about frames instead of
//! milliseconds, and each produces a symptom that is invisible on a machine
//! whose frame rate happens to match the command rate — which is exactly the
//! machine a developer tests on.

mod support;

use support::pacing::{Sequence, drive, reference_accumulator, sequences, violations};

/// 125 Hz — 8 ms commands, the spec D2 default.
const PERIOD: u32 = 8;

/// The control: a correct accumulator must pass every sequence.
///
/// Without this, every other test in the file could be satisfied by a checker
/// that rejects everything unconditionally.
#[test]
fn a_correct_accumulator_passes_every_sequence() {
    for seq in sequences() {
        let pacing = drive(&seq.frames, reference_accumulator(PERIOD));
        let found = violations(&seq, PERIOD, &pacing);
        assert!(
            found.is_empty(),
            "the reference accumulator failed {} ({}):\n  {}",
            seq.name,
            seq.intent,
            found.join("\n  "),
        );
    }
}

/// The command total must depend only on elapsed time, never on how that time
/// was divided into frames. This is criterion 5 stated as an equation.
#[test]
fn total_commands_depend_only_on_elapsed_time_not_on_frame_boundaries() {
    // Every sequence padded to the same total elapsed time, so the totals are
    // directly comparable.
    let target_ms = 1600;
    let mut results = Vec::new();

    for seq in sequences() {
        let mut frames = seq.frames.clone();
        let mut total: u32 = frames.iter().sum();
        while total < target_ms {
            let take = (target_ms - total).min(8);
            frames.push(take);
            total += take;
        }
        while total > target_ms {
            let excess = total - target_ms;
            let last = frames.pop().expect("non-empty");
            if last > excess {
                frames.push(last - excess);
                total -= excess;
            } else {
                total -= last;
            }
        }
        assert_eq!(frames.iter().sum::<u32>(), target_ms);

        let pacing = drive(&frames, reference_accumulator(PERIOD));
        results.push((seq.name, pacing.total));
    }

    let expected = target_ms / PERIOD;
    for (name, total) in &results {
        assert_eq!(
            *total, expected,
            "{name} delivered {total} commands over {target_ms} ms; every frame \
             partition of the same elapsed time must deliver {expected}",
        );
    }
}

/// An accumulator that throws away its remainder each frame runs the
/// simulation slow, and the checker must say so.
///
/// This is the single most likely wrong implementation — `dt / 8` is the
/// obvious expression, and on a machine running at exactly 125 fps it is
/// indistinguishable from correct.
#[test]
fn checker_rejects_an_accumulator_that_discards_its_remainder() {
    let broken = |dt: u32| dt / PERIOD;
    assert_rejected(
        "discards remainder",
        broken,
        &[
            "fast_1ms",
            "just_under_7ms",
            "just_over_9ms",
            "coprime_13ms",
        ],
    );
}

/// An accumulator that rounds to nearest invents time it was never given.
///
/// In play this is a simulation that drifts *ahead* of the wall clock, which
/// looks like nothing at all until a replay recorded on one machine fails to
/// reproduce on another.
#[test]
fn checker_rejects_an_accumulator_that_rounds_to_nearest() {
    let broken = move |dt: u32| (dt + PERIOD / 2) / PERIOD;
    assert_rejected(
        "rounds to nearest",
        broken,
        &["just_under_7ms", "vsync_60hz"],
    );
}

/// An accumulator that emits at most one command per frame cannot catch up
/// after a stall, so a dropped frame becomes permanent lag.
///
/// This is what "frame pacing coupled to simulation stepping" looks like in
/// code, and it is precisely what criterion 5 forbids.
#[test]
fn checker_rejects_an_accumulator_that_cannot_catch_up_after_a_stall() {
    let mut leftover = 0u32;
    let broken = move |dt: u32| {
        leftover += dt;
        if leftover >= PERIOD {
            leftover -= PERIOD;
            1
        } else {
            0
        }
    };
    assert_rejected("one command per frame", broken, &["stall_500ms"]);
}

/// An accumulator that lets unspent time pile up beyond a whole command is
/// falling behind, even though it never invents time.
///
/// Conservation alone does not catch this within a run — only the bounded-lag
/// property does.
#[test]
fn checker_rejects_an_accumulator_that_lags_by_a_whole_command() {
    let mut leftover = 0u32;
    let broken = move |dt: u32| {
        leftover += dt;
        // Emits correctly, but always one command later than it should.
        let due = leftover / PERIOD;
        let held = due.min(1);
        leftover -= (due - held) * PERIOD;
        due - held
    };
    assert_rejected("always one command behind", broken, &["regular_8ms"]);
}

/// The checker must not be satisfiable by emitting nothing at all.
#[test]
fn checker_rejects_an_accumulator_that_never_emits() {
    assert_rejected(
        "never emits",
        |_dt: u32| 0,
        &["regular_8ms", "fast_1ms", "wildly_variable"],
    );
}

/// A capped accumulator that *declares* what it discards must be accepted.
///
/// This models `FixedStep` as platform actually built it: catch-up is bounded
/// by `DEFAULT_MAX_TICKS_PER_FRAME` (250) so a long stall cannot start a spiral
/// of death, and the discarded milliseconds are reported through
/// `dropped_total_ms()`. Without this test the checker could satisfy every
/// other test in the file by rejecting all capped accumulators, which would
/// make it useless against the real one.
#[test]
fn a_capped_accumulator_that_declares_its_drops_is_accepted() {
    const MAX_TICKS: u32 = 250;

    for seq in sequences() {
        let mut leftover = 0u32;
        let mut dropped = 0u32;
        let pacing = {
            let mut per_frame = Vec::new();
            for &dt in &seq.frames {
                leftover += dt;
                let due = leftover / PERIOD;
                let ticks = due.min(MAX_TICKS);
                leftover -= due * PERIOD;
                dropped += (due - ticks) * PERIOD;
                per_frame.push(ticks);
            }
            let total = per_frame.iter().sum();
            support::pacing::Pacing {
                per_frame,
                total,
                dropped_ms: dropped,
            }
        };

        let found = violations(&seq, PERIOD, &pacing);
        assert!(
            found.is_empty(),
            "the checker rejected a correct capped accumulator on {} ({}):\n  {}",
            seq.name,
            seq.intent,
            found.join("\n  "),
        );
    }

    // And the cap must actually have engaged somewhere, or this test proves
    // nothing about capping.
    let stall = sequences()
        .into_iter()
        .find(|s| s.name == "catastrophic_stall_5s")
        .expect("catastrophic_stall_5s");
    assert!(
        stall.frames.iter().any(|&f| f / PERIOD > MAX_TICKS),
        "no frame in catastrophic_stall_5s exceeds the {MAX_TICKS}-tick cap, so \
         the drop path was never exercised",
    );
}

/// An accumulator that discards time *without reporting it* must be rejected.
///
/// This is the dangerous sibling of the legitimate cap above: identical
/// behaviour on screen, but the simulation clock silently falls behind the wall
/// clock, and a replay recorded through it cannot reproduce.
#[test]
fn checker_rejects_an_accumulator_that_drops_time_silently() {
    const MAX_TICKS: u32 = 250;
    let mut leftover = 0u32;
    let broken = move |dt: u32| {
        leftover += dt;
        let due = leftover / PERIOD;
        let ticks = due.min(MAX_TICKS);
        leftover -= due * PERIOD; // excess discarded…
        ticks // …and never declared.
    };
    assert_rejected("drops time silently", broken, &["catastrophic_stall_5s"]);
}

/// Dropping time when nothing stalled is silent data loss, not protection.
#[test]
fn checker_rejects_drops_on_a_sequence_with_no_long_frame() {
    let seq = sequences()
        .into_iter()
        .find(|s| s.name == "fast_1ms")
        .expect("fast_1ms");
    let pacing = support::pacing::Pacing {
        per_frame: vec![0; seq.frames.len()],
        total: 0,
        dropped_ms: seq.frames.iter().sum(),
    };
    let found = violations(&seq, PERIOD, &pacing);
    assert!(
        !found.is_empty(),
        "the checker accepted an accumulator that dropped every millisecond of \
         a sequence whose longest frame was 1 ms",
    );
}

/// Run a broken accumulator and require the checker to reject it on every
/// sequence named — and to produce a non-empty explanation, since a violation
/// with no message is not actionable.
fn assert_rejected(what: &str, mut accumulator: impl FnMut(u32) -> u32, must_fail_on: &[&str]) {
    let all = sequences();
    for name in must_fail_on {
        let seq: &Sequence = all
            .iter()
            .find(|s| s.name == *name)
            .unwrap_or_else(|| panic!("no sequence named {name:?}"));

        let pacing = drive(&seq.frames, &mut accumulator);
        let found = violations(seq, PERIOD, &pacing);

        assert!(
            !found.is_empty(),
            "the checker ACCEPTED an accumulator that {what}, on sequence {} \
             ({}). The checker is not catching what it claims to.",
            seq.name,
            seq.intent,
        );
        for message in &found {
            assert!(
                message.contains(seq.name) && message.len() > 20,
                "violation message is not actionable: {message:?}",
            );
        }
    }
}
