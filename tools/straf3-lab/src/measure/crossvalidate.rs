//! The lab's numbers against the sim seat's, measured independently.
//!
//! # Why this section exists
//!
//! Two seats measured the same five behaviours this wave, from opposite sides:
//! the sim seat from inside `cargo test`, with hand-built fixtures and
//! assertions; this crate from outside, with sweeps and a published document.
//! Two implementations agreeing is evidence. One implementation agreeing with
//! itself is a tautology, and it is what a single-seat measurement gives you.
//!
//! So every number below is a claim the sim seat published, restated verbatim
//! with its provenance, put beside the lab's own measurement of the same
//! quantity, and marked. **A disagreement is not smoothed over.** Where the two
//! differ, the row says so and the prose says why — one such row survives, and
//! it turned out to be the difference between measuring what the rule does and
//! measuring what a player can get, which is worth more than the agreement
//! would have been.
//!
//! # What a `sim` value is, and is not
//!
//! It is a literal, copied from that seat's reported findings. It is **not**
//! recomputed here and cannot be: their fixtures live in
//! `crates/straf3-collision/tests/` and `crates/straf3-sim/tests/`, which this
//! crate does not and must not link. If one of their numbers is wrong, this
//! section faithfully reproduces the error and reports agreement with it — the
//! protection against that is that the two measurements were taken by different
//! code against different fixtures, not that either checks the other.

use crate::dataset::{Measurement, Section, Table};

/// One published claim, and where the lab's answer to it lives.
struct Claim {
    /// What is being compared, in a reader's words.
    what: &'static str,
    /// The measurement key carrying the lab's answer.
    key: &'static str,
    /// The sim seat's published value.
    theirs: f64,
    /// How close counts. Chosen per claim from what the two seats' methods can
    /// be expected to share — an exact count is exact, a speed that both sides
    /// derive from a closed form is a hundredth, a speed one side arrives at
    /// through a whole simulated fall is looser and says so in the prose.
    tolerance: f64,
    /// Units, for the table.
    unit: &'static str,
}

/// The claims, in the order the report's sections raise them.
///
/// Every one of these was published by the sim seat in their A3 findings. The
/// keys beside them are this crate's own, and the values are looked up rather
/// than restated, so a claim cannot silently stop being checked when a
/// measurement is renamed — the lookup fails and the row says `not measured`.
///
/// `0.70711` is `cos 45°`, which is also `FRAC_1_SQRT_2`. It is written as the
/// decimal the other seat published rather than as the constant it happens to
/// equal: this list is a transcription of somebody else's report, and
/// substituting a more precise value for a quoted one would make the comparison
/// against a number nobody actually published.
#[allow(clippy::approx_constant)]
const CLAIMS: &[Claim] = &[
    Claim {
        what: "Grounded, handed (0,0,−100): returned upward velocity",
        key: "vq3.overbounce.handed0100.returned_vz",
        theirs: 100.0,
        tolerance: 0.01,
        unit: "ups",
    },
    Claim {
        what: "Grounded, handed (0,0,−1000): returned upward velocity",
        key: "vq3.overbounce.handed1000.returned_vz",
        theirs: 1000.0,
        tolerance: 0.01,
        unit: "ups",
    },
    Claim {
        what: "Overbounces among 1920 drops over 16–256 units, 0.125 sampling",
        key: "vq3.overbounce.brush_floor.full_in_subrange",
        theirs: 151.0,
        tolerance: 0.0,
        unit: "of 1920",
    },
    Claim {
        what: "Overbounces among 8064 drops over 16–1024 units, 0.125 sampling",
        key: "vq3.overbounce.brush_floor.full",
        theirs: 350.0,
        tolerance: 0.0,
        unit: "of 8064",
    },
    Claim {
        what: "A 16.000-unit drop overbounces (fraction of impact returned)",
        key: "vq3.overbounce.drop0016000.return_ratio",
        theirs: 1.0,
        tolerance: 0.001,
        unit: "",
    },
    Claim {
        what: "A 16.500-unit drop does not (fraction of impact returned)",
        key: "vq3.overbounce.drop0016500.return_ratio",
        theirs: 0.001,
        tolerance: 0.001,
        unit: "",
    },
    Claim {
        what: "Seam loss crossing onto a 10° ramp",
        key: "vq3.ramp.deg10.uphill_seam_ratio_less_friction",
        theirs: 0.984_81,
        tolerance: 0.000_05,
        unit: "",
    },
    Claim {
        what: "Seam loss crossing onto a 26° ramp",
        key: "vq3.ramp.deg26.uphill_seam_ratio_less_friction",
        theirs: 0.898_79,
        tolerance: 0.000_05,
        unit: "",
    },
    Claim {
        what: "Seam loss crossing onto a 45° ramp",
        key: "vq3.ramp.deg45.uphill_seam_ratio_less_friction",
        theirs: 0.707_11,
        tolerance: 0.000_05,
        unit: "",
    },
    Claim {
        what: "Steepest walkable ramp",
        key: "vq3.ramp.flip_angle_observed",
        theirs: 45.57,
        tolerance: 0.01,
        unit: "deg",
    },
    Claim {
        what: "Worst single-command speed loss running off a ledge",
        key: "vq3.edge.clip.worst_command_loss_ups",
        theirs: 0.0,
        tolerance: 0.001,
        unit: "ups",
    },
    Claim {
        what: "Ledge release offset (one hull half-width)",
        key: "vq3.edge.ledge_release_x",
        theirs: 15.0,
        tolerance: 0.1,
        unit: "units",
    },
    Claim {
        what: "Tallest riser climbed (STEPSIZE + SURFACE_CLIP_EPSILON = 18.125)",
        key: "vq3.step.highest_climbable",
        theirs: 18.125,
        tolerance: 0.002,
        unit: "units",
    },
    Claim {
        what: "Drop launch, 300 ups off 16 units",
        key: "vq3.overbounce.launch0300from0016000.peak_ups",
        theirs: 340.0,
        tolerance: 0.5,
        unit: "ups",
    },
    Claim {
        what: "Drop launch, 300 ups off 508.875 units",
        key: "vq3.overbounce.launch0300from0508875.peak_ups",
        theirs: 950.96,
        tolerance: 0.5,
        unit: "ups",
    },
    Claim {
        what: "Drop launch, 600 ups off 508.875 units",
        key: "vq3.overbounce.launch0600from0508875.peak_ups",
        theirs: 1083.66,
        tolerance: 0.5,
        unit: "ups",
    },
];

/// Build the cross-validation section from the measurements already taken.
///
/// Takes the finished sections rather than re-measuring, because a
/// cross-validation that computed its own numbers would be checking a second
/// implementation of the lab against the sim seat rather than checking *this*
/// report's numbers — and it is this report's numbers a reader is being asked
/// to trust.
pub(crate) fn measure(sections: &[Section]) -> Section {
    let mut section = Section::new("7. Cross-validation against the sim seat");
    section.say(
        "The sim seat measured the same five behaviours this wave from the test \
         side, with their own fixtures and their own code. Their published \
         numbers are restated below verbatim and put beside this report's. Two \
         implementations agreeing is evidence; one implementation agreeing with \
         itself is not, and a single-seat measurement is the second thing.",
    );

    let lookup = |key: &str| -> Option<f64> {
        sections
            .iter()
            .flat_map(|s| s.data.iter())
            .find(|m| m.key == key)
            .and_then(|m| m.value.parse::<f64>().ok())
    };

    let mut table = Table::new(
        "**Claim by claim.** `sim` is their published value; `lab` is this \
         report's measurement of the same quantity, looked up from the \
         machine-readable section rather than recomputed.",
        &["claim", "sim", "lab", "difference", ""],
    );

    let mut agreed = 0u32;
    let mut disagreed = 0u32;
    for claim in CLAIMS {
        let Some(mine) = lookup(claim.key) else {
            table.push(vec![
                claim.what.to_string(),
                format!("{:.5}", claim.theirs),
                "not measured".to_string(),
                "—".to_string(),
                "✗".to_string(),
            ]);
            disagreed += 1;
            continue;
        };
        let delta = mine - claim.theirs;
        let ok = delta.abs() <= claim.tolerance;
        if ok {
            agreed += 1;
        } else {
            disagreed += 1;
        }
        section.record(Measurement::ratio(
            format!("crossvalidation.{}.agrees", claim.key),
            if ok { 1.0 } else { 0.0 },
        ));
        table.push(vec![
            claim.what.to_string(),
            trim(claim.theirs, claim.unit),
            trim(mine, claim.unit),
            format!("{delta:+.5}"),
            if ok { "✓" } else { "✗" }.to_string(),
        ]);
    }

    section.record(Measurement::count("crossvalidation.claims", CLAIMS.len() as u32));
    section.record(Measurement::count("crossvalidation.agreed", agreed));
    section.record(Measurement::count("crossvalidation.disagreed", disagreed));

    section.table(table);
    section.say(format!(
        "**{agreed} of {} claims agree within tolerance.**",
        CLAIMS.len()
    ));
    section.say(
        "**The three drop-launch rows are the disagreement, and it is a real \
         one.** The sim seat's figures are the closed form \
         `sqrt(entry² + 2·g·h)`, and this report reproduces that closed form \
         exactly — it is the `closed form` column of section 4's launch table, \
         and the `constructed` column beside it confirms that handing the \
         mechanism the ideal fall speed returns all of it. What the `peak` \
         column measures is different: an actual fall from that height, which \
         launches on whichever 8 ms command happens to end with the feet inside \
         the 0.25-unit ground probe, and by then the player has not fallen the \
         whole way. The measured peak is 4–5% below the closed form for that \
         reason and no other.",
    );
    section.say(
        "Neither number is wrong; they answer different questions. The closed \
         form is the ceiling of the mechanism, and it is what a doctrine \
         argument about whether this belongs in the game should use. The peak is \
         what a player gets, and it is what a route planner should use. This \
         report publishes both, and the two seats' figures are reconciled by \
         which of the two each was measuring.",
    );
    section.say(
        "One thing **neither seat measured**, stated so it is not mistaken for a \
         settled question: whether any of section 4 survives `pmove_msec` \
         sub-stepping. Overbounce's precondition is a per-command artefact — a \
         command that ends with the feet inside the ground probe — and \
         sub-stepping moves the command boundary. Both seats expect the counts \
         to change and neither has run it, because the sub-stepping does not \
         exist yet (`crates/straf3-sim/src/step.rs`, `TODO(wave3)`).",
    );

    section
}

/// Format a value the way its unit wants, without a trailing unit string in the
/// cell: counts as integers, everything else at five decimals trimmed of the
/// zeros a reader does not need.
fn trim(v: f64, unit: &str) -> String {
    if unit.starts_with("of ") {
        format!("{v:.0}")
    } else if v.abs() >= 100.0 {
        format!("{v:.2}")
    } else {
        format!("{v:.5}")
    }
}
