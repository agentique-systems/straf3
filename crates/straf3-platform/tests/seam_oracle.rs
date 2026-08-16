//! The reference oracle for criterion 4 (spec rev 6 §S).
//!
//! Criterion 4: *a recorded input sequence replays to the same checksum
//! through the windowed build as through `straf3-headless`.* That is a claim
//! about three things agreeing:
//!
//! | Corner | What it is |
//! |---|---|
//! | **A** | `straf3-headless <fixture> --trace --csv` — the spec's named reference |
//! | **B** | `straf3_sim` driven directly in this process |
//! | **C** | the windowed build, via `straf3 --replay` |
//!
//! This file establishes **A == B**. It is not the criterion on its own, and
//! it does not pretend to be: it is the part that can be proved before the
//! windowed build exists, and it is what makes a later A-vs-C failure
//! *localisable*. Without A == B, a mismatch against the windowed build could
//! equally be a bad fixture, a bad parser or a bad comparison, and there would
//! be no way to tell which. `replay_equivalence.rs` adds corner C.
//!
//! Every assertion here is computed on both sides. There is not one golden
//! checksum literal in this crate, by policy — spec rev 6 Q1 changes every
//! checksum in the repository when the Cody–Waite trig lands.

mod support;

use support::{assert_digests_match, parse_trace_csv, runs};

/// The checked-in fixtures must be exactly what `runs()` says they are.
///
/// # Why this test is load-bearing rather than housekeeping
///
/// The fixture files are what `straf3-headless` actually reads, and `runs()`
/// is what the in-process reference actually executes. If the two drift, the
/// oracle silently starts comparing two different runs — and because both
/// sides would still be internally consistent, every other test here would go
/// on passing. This is the test that makes "the same recorded input sequence"
/// mean something.
///
/// Regenerate after an intentional change:
/// `STRAF3_BLESS_FIXTURES=1 cargo test -p straf3-platform`
#[test]
fn fixtures_match_their_definitions() {
    let bless = std::env::var_os("STRAF3_BLESS_FIXTURES").is_some();
    let mut stale = Vec::new();

    // `all_runs`, not `runs`: the course fixture is read by the windowed build's
    // tests and would otherwise drift with nothing watching it.
    for run in support::all_runs() {
        let expected = run.render();
        let path = run.fixture_path();

        if bless {
            std::fs::create_dir_all(support::fixtures_dir()).expect("create fixtures dir");
            std::fs::write(&path, &expected)
                .unwrap_or_else(|e| panic!("writing {}: {e}", path.display()));
            continue;
        }

        match std::fs::read_to_string(&path) {
            Ok(actual) if actual == expected => {}
            Ok(actual) => stale.push(format!(
                "{}: on disk differs from its definition in support::runs() \
                 (disk {} bytes, definition {} bytes)",
                path.display(),
                actual.len(),
                expected.len(),
            )),
            Err(e) => stale.push(format!("{}: {e}", path.display())),
        }
    }

    assert!(
        stale.is_empty(),
        "fixture files are out of sync with their definitions:\n  {}\n\n\
         If the change was intentional, regenerate with \
         STRAF3_BLESS_FIXTURES=1 cargo test -p straf3-platform",
        stale.join("\n  "),
    );
}

/// Rendering a scalar to fixture text and parsing it back must preserve every
/// bit.
///
/// The fixture format is decimal text, and the simulation is bit-exact. If
/// `render_scalar` lost a single ULP, every fixture would still look right and
/// every replay comparison would fail for a reason that has nothing to do with
/// the seam. Rust's `{}` for `f32` emits the shortest decimal that round-trips,
/// so this holds — but it holds as a property of the standard library, which
/// is worth pinning rather than assuming.
#[test]
fn rendered_scalars_round_trip_exactly() {
    // The exact scalars the fixtures contain, plus the awkward ones.
    let mut values: Vec<f32> = vec![0.0, -0.0, 1.0, -1.0, 0.55, -20.0, 1024.0, 0.13, 1.7, 0.05];
    for i in 0..300 {
        let t = i as f32;
        values.push(0.55 * t);
        values.push(-20.0 + 0.13 * t);
        values.push(1.7 * t);
        values.push(0.05 * t);
    }

    for v in values {
        // Must go through `render_scalar` itself. Inlining `format!("{v}")`
        // here would test the standard library rather than the function the
        // fixtures are actually written with — verified by mutation: with the
        // inlined version, changing `render_scalar` to `{v:.3}` left this test
        // green.
        let text = support::render_scalar(v);
        let back: f32 = text
            .parse()
            .unwrap_or_else(|e| panic!("{text:?} rendered from {v} does not parse back: {e}"));
        assert_eq!(
            v.to_bits(),
            back.to_bits(),
            "{v} rendered as {text:?} parsed back as {back} — bit pattern \
             {:#010x} became {:#010x}",
            v.to_bits(),
            back.to_bits(),
        );
    }
}

/// **A == B.** The `straf3-headless` binary and the in-process simulation
/// produce the identical per-tick checksum stream for every fixture.
///
/// Per tick, not end-state: spec rev 6 §R records a probe case whose final
/// checksum matched across builds while 29 of its 1200 intermediate checksums
/// did not.
#[test]
fn headless_binary_and_in_process_simulation_agree_per_tick() {
    for run in runs() {
        let reference = run.reference_digests();
        let headless = run.headless_digests();

        assert_eq!(
            reference.len(),
            run.cmds.len() + 1,
            "{}: the in-process reference should emit the spawn state plus one \
             checksum per command",
            run.name,
        );

        assert_digests_match(
            &format!("{}/in-process", run.name),
            &reference,
            &format!("{}/straf3-headless", run.name),
            &headless,
        );
    }
}

/// The runs must actually be distinguishable from each other and from a run
/// that does nothing.
///
/// # Why
///
/// Every equivalence test in this crate compares two digest streams. If a
/// fixture's stream were constant — every tick the same checksum, because the
/// player never moves — then a comparison against it would pass for reasons
/// unrelated to the seam, and a platform bug that froze the simulation would
/// be invisible. This asserts each run's stream genuinely evolves, and that
/// the four runs are genuinely different runs.
#[test]
fn every_run_produces_a_distinct_and_evolving_digest_stream() {
    let mut finals = Vec::new();

    for run in runs() {
        let digests = run.reference_digests();
        let distinct: std::collections::HashSet<_> = digests.iter().collect();

        assert!(
            distinct.len() > digests.len() / 2,
            "{}: only {} distinct checksums across {} ticks — this run barely \
             changes state, so comparing against it proves little",
            run.name,
            distinct.len(),
            digests.len(),
        );
        assert_ne!(
            digests.first(),
            digests.last(),
            "{}: ends in the same state it started in",
            run.name,
        );
        finals.push((run.name, *digests.last().expect("non-empty")));
    }

    for (i, (name_a, a)) in finals.iter().enumerate() {
        for (name_b, b) in &finals[i + 1..] {
            assert_ne!(
                a, b,
                "{name_a} and {name_b} end in the identical state — they are \
                 not independent evidence",
            );
        }
    }
}

/// The comparison helper must catch a mid-run divergence that a final-state
/// check would miss.
///
/// This is a test of the harness itself, and it is here because the failure it
/// guards against is documented as having actually happened: spec rev 6 §R,
/// 29 of 1200 intermediate checksums differing under a matching final
/// checksum. A comparison that only looked at the last element would certify
/// that run as equivalent.
#[test]
fn divergence_detector_catches_what_a_final_state_check_would_hide() {
    let left = vec![0x11, 0x22, 0x33, 0x44, 0x55];
    let mut right = left.clone();
    right[2] = 0xdead; // differs mid-run…
    assert_eq!(
        left.last(),
        right.last(),
        "the fixture for this test must have matching final checksums, \
         otherwise it is not testing what it claims to",
    );

    let result = std::panic::catch_unwind(|| {
        assert_digests_match("left", &left, "right", &right);
    });
    assert!(
        result.is_err(),
        "assert_digests_match accepted two streams that differ at tick 2 \
         merely because their final checksums agree — this is precisely the \
         probe failure in spec rev 6 §R",
    );

    // …and it must accept genuinely identical streams, or the test above
    // would pass simply by rejecting everything.
    assert_digests_match("left", &left, "left again", &left);
}

/// A mid-run divergence really can hide behind a matching final checksum —
/// **in this repository, on the criterion-4 fixtures, not just in the probe**.
///
/// # Why this test exists
///
/// Spec rev 6 §R records a probe case whose final checksum matched across
/// builds while 29 of its 1200 intermediate checksums did not, and requires
/// that any run-submission format carry a rolling or per-command digest.
/// That was measured on a different codebase in a different probe, so it is
/// easy to file away as somebody else's anecdote.
///
/// It is not. Perturbing `right_move` by one unit on a single 8 ms command of
/// `strafe_jump_cpm` produces a run that differs from the original across 88
/// of its 322 ticks — and then re-converges, so that the two runs finish with
/// the identical checksum. An end-state-only criterion-4 check would certify
/// those two different runs as equivalent.
///
/// # Why it is a search rather than a hardcoded index
///
/// The specific command that reproduces this is a numerical accident, and
/// spec rev 6 Q1's Cody–Waite trig replacement will change every checksum in
/// the repository. Pinning one index would make this test a
/// maintenance-burden-shaped time bomb. Searching for *any* single-command
/// perturbation with the hiding property states the claim that actually
/// matters — the phenomenon is reachable here — and survives the trig swap.
/// If it ever stops reproducing, the failure message says so plainly, and
/// that is worth knowing rather than worth deleting.
#[test]
fn a_mid_run_divergence_can_hide_behind_a_matching_final_checksum() {
    let base = support::run_named("strafe_jump_cpm");
    let base_digests = base.reference_digests();

    let mut hidden: Option<(usize, usize)> = None;
    for idx in 0..base.cmds.len() {
        if base.cmds[idx].right_move == 0 {
            continue; // Nudging an axis that is already idle changes nothing.
        }
        let mut variant = base.clone();
        variant.cmds[idx].right_move -= 1;
        let digests = variant.reference_digests();

        if digests != base_digests && digests.last() == base_digests.last() {
            let differing = digests
                .iter()
                .zip(base_digests.iter())
                .filter(|(a, b)| a != b)
                .count();
            hidden = Some((idx, differing));
            break;
        }
    }

    let (idx, differing) = hidden.expect(
        "no single-command perturbation of strafe_jump_cpm produced a run that \
         differs mid-stream yet finishes on the same checksum. The hiding \
         phenomenon of spec rev 6 §R no longer reproduces on this fixture — \
         which does NOT mean end-state comparison became safe. Re-derive a \
         case (a different run, a different axis) rather than deleting this.",
    );

    assert!(
        differing > 0 && differing < base_digests.len(),
        "internal contradiction: {differing} of {} ticks differ",
        base_digests.len(),
    );

    // The point of the whole exercise: per-tick comparison catches this, and
    // it is the only thing that does.
    let mut variant = base.clone();
    variant.cmds[idx].right_move -= 1;
    let digests = variant.reference_digests();

    assert_eq!(
        digests.last(),
        base_digests.last(),
        "the perturbed run must finish on the same checksum for this test to \
         mean anything",
    );
    let caught = std::panic::catch_unwind(|| {
        assert_digests_match("original", &base_digests, "perturbed", &digests);
    });
    assert!(
        caught.is_err(),
        "command {idx} nudged by one unit changed {differing} of {} ticks, the \
         run still finished on checksum {:#018x}, and assert_digests_match did \
         not notice",
        base_digests.len(),
        base_digests.last().expect("non-empty"),
    );

    eprintln!(
        "note: nudging right_move by 1 on command {idx} changes {differing} of \
         {} ticks yet finishes on the identical checksum",
        base_digests.len(),
    );
}

/// Two empty streams must not count as agreement.
///
/// The cheapest way for a replay test to pass without proving anything is for
/// both sides to produce nothing — a binary that exits early, a parser that
/// finds no commands, a trace flag that stopped being honoured. This pins the
/// refusal.
#[test]
fn empty_streams_are_not_evidence_of_equivalence() {
    let result = std::panic::catch_unwind(|| {
        assert_digests_match("nothing", &[], "nothing either", &[]);
    });
    assert!(
        result.is_err(),
        "assert_digests_match treated two empty digest streams as equal",
    );
}

/// The CSV parser must reject output it does not understand rather than
/// returning a short stream.
///
/// A lenient parser that skipped unparseable lines would turn a broken
/// `--trace-csv` implementation into a *passing* test with a two-element
/// stream. Every rejection below is a case that could plausibly arise from the
/// windowed build's independent implementation of the format.
#[test]
fn trace_csv_parser_rejects_malformed_output_instead_of_shortening() {
    const HEADER: &str = "tick,time_ms,x,y,z,vx,vy,vz,speed,grounded,checksum";

    let good = format!("{HEADER}\n0,0,0,0,64,0,0,0,0,0,0x1999a4edbb5b8711\n");
    assert_eq!(parse_trace_csv(&good).len(), 1, "the good case must parse");

    let cases: [(&str, String); 5] = [
        ("empty output", String::new()),
        ("header only", format!("{HEADER}\n")),
        (
            "a column was added, so the last field is no longer the checksum",
            format!("{HEADER},extra\n0,0,0,0,64,0,0,0,0,0,0x11,7\n"),
        ),
        (
            "checksum written in decimal instead of hex",
            format!("{HEADER}\n0,0,0,0,64,0,0,0,0,0,1234567\n"),
        ),
        (
            "checksum is not a number",
            format!("{HEADER}\n0,0,0,0,64,0,0,0,0,0,0xzzzz\n"),
        ),
    ];

    for (why, text) in cases {
        let result = std::panic::catch_unwind(|| parse_trace_csv(&text));
        assert!(
            result.is_err(),
            "parse_trace_csv accepted output it should have rejected ({why}): {text:?}",
        );
    }
}
