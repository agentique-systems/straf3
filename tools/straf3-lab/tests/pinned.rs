//! Spec criterion 2: the lab's numbers are pinned, and a failure **names which
//! measurement moved**.
//!
//! # Why a digest would not do
//!
//! The tempting shape is one checksum over the whole dataset. It is a line of
//! code, it never goes stale, and it is useless: "the movement digest changed"
//! tells a reader that something moved and nothing about what, so the only way
//! to act on it is to re-derive the whole comparison by hand — which is to say,
//! to write this file anyway, under pressure, after the failure.
//!
//! The point of the wave is that a movement change should be *legible*. So the
//! fixture is the machine-readable dataset itself, keyed measurement by
//! measurement, and a failure prints the name of every number that moved along
//! with what it read before and after. `cpm.window.double_jump.usable_delay_ms
//! 384 -> 376` is a sentence somebody can act on. A changed digest is not.
//!
//! # Where the tolerance is, and why it is not an epsilon here
//!
//! The obvious design is `(mine - pinned).abs() < tolerance`, with a tolerance
//! chosen per measurement family. This does something equivalent and does it one
//! layer earlier: **the tolerance is the printed precision**, chosen per
//! measurement kind at the point the number is produced (see
//! `dataset.rs`'s table — 2 decimals on a speed, 2 on an angle, 3 on a distance,
//! 0 on a millisecond, 4 on a ratio), and the comparison is then exact.
//!
//! Two reasons that is the better place for it.
//!
//! **One threshold instead of two.** With a separate epsilon, the document shows
//! `519.74` and the test quietly accepts anything in `519.74 ± ε`. A reader
//! comparing two published documents by eye applies a different standard than
//! the gate does, and the two drift. Rounding once means the number in the
//! document *is* the number under test: if it reads the same, it passed.
//!
//! **There is no float wobble here to absorb.** An epsilon earns its place where
//! a legitimate refactor moves the last bits. This simulation is bit-identical
//! by contract — `cargo xtask determinism` holds it to the same bits across
//! glibc, musl, Windows and wasm — and the lab adds no libm on any path that
//! feeds a number. A change in the last bits of a movement result is not noise
//! to be tolerated; it is a movement change, and rounding to the published
//! precision is exactly the filter that decides whether it is one a player could
//! ever notice.
//!
//! The one place that argument does not hold is a value hovering either side of
//! zero, where the sign flips for reasons far below the printed precision.
//! `dataset.rs` normalises `-0.00` to `0.00` for that reason and says so.
//!
//! # What this test is not
//!
//! It is not a judgement. Movement is *supposed* to change: that is what the
//! wave after this one is for. A red run here means "you changed how the game
//! moves — was that what you meant?", and the answer being yes is fine. The fix
//! is `cargo xtask lab`, which rewrites the fixture and the document together,
//! committed alongside the change that caused it so the two travel as one
//! reviewable thing. That instruction is in the failure message, in the
//! fixture's own header, and in `cargo xtask lab --check`'s output, because a
//! gate nobody can legitimately update gets deleted the first time it is
//! inconvenient.
//!
//! # A new profile is not a regression
//!
//! Keys are `<profile>.<family>.…`, so `experimental` landing adds keys and
//! moves none. The gate still fails — a fixture that silently accepted new
//! measurements would stop guarding them — but the failure counts *moved*
//! separately from *appeared*, and says which of the two happened. See
//! `dataset::summarise`.
//!
//! # Cost
//!
//! Recomputing every measurement is tens of millions of simulation commands —
//! about ten seconds in release, a few minutes in a debug build. That is the
//! price of pinning the real numbers rather than a summary of them, and it is
//! paid once per test run rather than once per assertion because every
//! measurement comes out of one pass.

use std::path::{Path, PathBuf};

use straf3_lab::{Dataset, dataset, report};

/// The fixture, resolved from this crate rather than from the working
/// directory: `cargo test` runs with the crate root as its cwd today, and a
/// test that quietly depended on that would break the first time somebody ran
/// it from elsewhere.
fn pinned_path() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("measurements.pinned.tsv")
}

/// The published document, for the agreement check below.
fn document_path() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join("..")
        .join(report::DEFAULT_PATH)
}

/// Everything the lab measures, recomputed once and shared by the tests here.
fn measured() -> Dataset {
    straf3_lab::measurements()
}

#[test]
fn the_measurements_are_the_ones_that_were_pinned() {
    let path = pinned_path();
    let text = std::fs::read_to_string(&path).unwrap_or_else(|e| {
        panic!(
            "cannot read the pinned measurements at {}: {e}\n\
             Run `{}` once to create it.",
            path.display(),
            report::COMMAND
        )
    });
    let pinned =
        Dataset::from_tsv(&text).unwrap_or_else(|e| panic!("{} is malformed: {e}", path.display()));
    let now = measured();

    let changes = dataset::diff(&pinned, &now);
    assert!(
        changes.is_empty(),
        "\n{} of {} movement measurements moved.\n\n{}\n\
         Each line above names a measurement, what it read, and what it reads \
         now.\nIf that was the intended effect of a movement change, run `{}` \
         and commit\nthe new numbers in the same commit as the change, with the \
         reason in the message.\n",
        changes.len(),
        now.len(),
        dataset::summarise(&changes),
        report::COMMAND
    );
}

/// The document and the fixture are generated from one dataset, so they cannot
/// disagree — unless somebody regenerated one of them and not the other, which
/// is exactly what this catches.
#[test]
fn the_published_document_carries_the_pinned_numbers() {
    let doc = document_path();
    let Ok(text) = std::fs::read_to_string(&doc) else {
        panic!(
            "the results document is missing from {}.\n\
             Run `{}` to publish it.",
            doc.display(),
            report::COMMAND
        );
    };

    let block = text
        .split("```tsv\n")
        .nth(1)
        .and_then(|rest| rest.split("```").next())
        .unwrap_or_else(|| {
            panic!(
                "{} has no machine-readable section. It is generated by `{}`; \
                 regenerate it rather than editing it.",
                doc.display(),
                report::COMMAND
            )
        });

    let embedded = Dataset::from_tsv(block)
        .unwrap_or_else(|e| panic!("the document's machine-readable section is malformed: {e}"));
    let pinned = Dataset::from_tsv(
        &std::fs::read_to_string(pinned_path()).expect("the pinned fixture must exist"),
    )
    .expect("the pinned fixture must parse");

    let changes = dataset::diff(&pinned, &embedded);
    assert!(
        changes.is_empty(),
        "\nthe published document and the pinned fixture disagree about {} \
         measurements.\n\n{}\n\
         They are generated together by `{}`; one of them was regenerated \
         without the other.\n",
        changes.len(),
        dataset::summarise(&changes),
        report::COMMAND
    );
}

/// The instrument's own contract. Two runs in one process must be identical, or
/// nothing above is a measurement of anything.
///
/// This is cheap next to the pass above and catches the one class of bug the
/// fixture cannot: a measurement that depends on something other than its
/// inputs would drift between the two datasets the tests here compute, and the
/// fixture comparison would then fail intermittently and blame the tree.
#[test]
fn the_lab_is_deterministic_within_a_process() {
    let a = measured();
    let b = measured();
    let changes = dataset::diff(&a, &b);
    assert!(
        changes.is_empty(),
        "\nthe lab is not deterministic: {} measurements differed between two \
         runs in one process.\n\n{}\n\
         A measurement that depends on anything but its inputs is not a \
         measurement.\n",
        changes.len(),
        dataset::summarise(&changes),
    );
}

/// Every key is unique and sorted, which is what makes the diff above a merge
/// over two ordered lists rather than a hash lookup — and therefore what makes
/// its output order stable.
///
/// `Dataset::from_sections` asserts uniqueness at construction; this asserts the
/// property survives the round trip through the file, where a hand-edit could
/// have introduced a duplicate that the parser would happily accept.
#[test]
fn the_pinned_fixture_is_sorted_and_has_no_duplicate_keys() {
    let text = std::fs::read_to_string(pinned_path()).expect("the pinned fixture must exist");
    let pinned = Dataset::from_tsv(&text).expect("the pinned fixture must parse");
    assert!(!pinned.is_empty(), "the pinned fixture is empty");
    for pair in pinned.entries().windows(2) {
        assert!(
            pair[0].key < pair[1].key,
            "keys out of order or duplicated: {:?} then {:?}",
            pair[0].key,
            pair[1].key
        );
    }
}
