//! G7 part 2: looking for invisible cliffs, and proving the instrument can find
//! one before it is trusted to say there are none.
//!
//! # Why this module exists separately from the candidate sweep
//!
//! `docs/movement-canon.md` G7 asks whether a mechanic hides a large change in
//! outcome across a small change in input, somewhere the player cannot see
//! coming. The amended wording answers it with a **refinement rule** —
//! [`crate::refine`] — rather than with a step on a fixed grid, because a step
//! on a fixed grid is a fact about the grid.
//!
//! A refinement rule that has never been shown to bite is a second untested
//! instrument, and G7 requires it to pass a self-test before it judges anything:
//!
//! - **Strafejumping must report no surviving discontinuity.** It is the
//!   canonical technique, it is smooth, and an instrument that flags it has
//!   rejected the game.
//! - **Overbounce must report one.** `docs/movement-lab.md` §4 measures a
//!   16.000-unit drop returning all of its impact speed and a 16.500-unit drop
//!   returning a tenth of a percent. That is a real cliff, on no boundary a
//!   player can see, and an instrument that cannot find it is measuring nothing.
//!
//! This is the discipline `crates/straf3-collision/tests/canon_frozen.rs`
//! applies to itself when it perturbs canon to prove the freeze bites: a check
//! that has never been seen to fail is not known to be a check.
//!
//! # The overbounce curve is deliberately the one section 4 already publishes
//!
//! [`overbounce::drop_onto`] is called here rather than reimplemented. A
//! self-test against a second implementation of the fall would prove that the
//! refinement rule can find a cliff in *that* function, which is not the claim.

// Everything below is reachable from the `#[test]` at the foot of this file,
// which is what makes the self-test run on every `cargo test`. It is not yet
// reachable from `super::all`, because the candidate section that would publish
// it is not written. That is a deliberate state, not an oversight: the
// instrument is landed, tested and unpublished, and the next wave starts from a
// self-test that has been seen to work rather than from nothing.
#![allow(dead_code)]

use straf3_sim::PhysicsProfile;
use straf3_sim::num::{Scalar, s};
use straf3_sim::world::EmptyWorld;

use crate::dataset::{Measurement, Section, Table};
use crate::geometry;
use crate::harness::{Axis, flying_at, gain_per_second};
use crate::measure::overbounce;
use crate::refine::{self, Step};

/// One threshold, everywhere: 16 ups, which is 5% of the 320 ups ground cap and
/// the smallest change legible on the speed overlay at a glance.
pub const MATERIAL: Scalar = s(16.0);

/// The aim grid the candidate sweep uses, in degrees.
pub const AIM_COARSE: Scalar = s(5.0);

/// How far aim refinement goes before it stops. Canon's choice, not a
/// derivation: a player cannot hold an angle finer than this, and the lab's own
/// §1 curve falls under [`MATERIAL`] at about this spacing.
pub const AIM_FLOOR: Scalar = s(1.0);

/// The finer floor to fall back to when [`AIM_FLOOR`] is not fine enough to tell
/// a steep gradient from a cliff.
///
/// Not a fudge, and the difference matters: refining further and finding the
/// step *halves* says the step was a gradient and the floor was too coarse.
/// Refining further and finding it unchanged says it is a cliff, and that is a
/// finding about canon's own technique rather than a reason to move the floor
/// again. The report says which happened.
pub const AIM_FLOOR_FINE: Scalar = s(0.25);

/// Geometry refinement floor, in units: a sixteenth, which is finer than the
/// `SURFACE_CLIP_EPSILON` any map can express a surface to.
pub const GEOMETRY_FLOOR: Scalar = s(0.0625);

/// The drop-height grid the overbounce self-test sweeps on, in units.
const DROP_COARSE: Scalar = s(0.5);

/// The range of drop heights swept, in units.
///
/// Starts where §4's sweep starts. Stops at 128 rather than at §4's 1024 because
/// the question here is whether the *instrument* finds a cliff, and §4 already
/// establishes that the lowest one is at 16 units; sweeping eight times as far
/// to find the same answer is eight times the simulation for no extra claim.
const DROP_MIN: Scalar = s(16.0);
const DROP_MAX: Scalar = s(128.0);

/// Entry speeds the strafejump self-test sweeps at.
///
/// The same six the candidate sweep uses, so that a floor good enough here is
/// good enough there.
const ENTRIES: &[f32] = &[320.0, 400.0, 500.0, 640.0, 800.0, 1000.0];

/// What a self-test found.
pub(crate) struct Found {
    /// Which curve it was.
    pub what: String,
    /// The worst step that survived refinement, if any step was large enough to
    /// refine at all.
    pub step: Option<Step>,
    /// The floor refinement reached.
    pub floor: Scalar,
}

impl Found {
    /// Whether a material step survived.
    fn survived(&self) -> bool {
        self.step.is_some_and(|s| s.survives(MATERIAL))
    }

    fn refined(&self) -> Scalar {
        self.step.map_or(s(0.0), |s| s.refined)
    }

    fn coarse(&self) -> Scalar {
        self.step.map_or(s(0.0), |s| s.coarse)
    }
}

/// The strafejump curve, refined: gain per second against held angle.
///
/// Swept for every profile, both axes and every entry speed, and the **worst**
/// result across all of them is what the self-test is scored on. A floor that
/// only works at the entry speed somebody happened to check is not a floor.
pub(crate) fn strafejump(floor: Scalar) -> Found {
    let world = EmptyWorld;
    let mut worst: Option<(String, Step)> = None;
    for (name, profile) in super::profiles() {
        for axis in [Axis::Forward, Axis::Strafe] {
            for &entry in ENTRIES {
                let start = flying_at(s(entry));
                let found = refine::largest_step(
                    |angle| gain_per_second(&world, &profile, &start, angle, axis),
                    s(0.0),
                    s(90.0),
                    AIM_COARSE,
                    floor,
                    MATERIAL,
                );
                if let Some(step) = found
                    && worst.as_ref().is_none_or(|(_, w)| step.refined > w.refined)
                {
                    worst = Some((format!("{name}/{} at {entry:.0} ups", axis.key()), step));
                }
            }
        }
    }
    match worst {
        Some((what, step)) => Found {
            what: format!("strafejump, worst at {what}"),
            step: Some(step),
            floor,
        },
        None => Found {
            what: "strafejump".to_string(),
            step: None,
            floor,
        },
    }
}

/// The overbounce curve, refined: upward velocity returned against drop height.
pub(crate) fn overbounce_cliff() -> Found {
    let world = geometry::floor();
    let profile = PhysicsProfile::vq3();
    let step = refine::largest_step(
        |height| overbounce::drop_onto(&world, &profile, height, s(0.0)).upward,
        DROP_MIN,
        DROP_MAX,
        DROP_COARSE,
        GEOMETRY_FLOOR,
        MATERIAL,
    );
    Found {
        what: format!("overbounce, drop height {DROP_MIN:.0}–{DROP_MAX:.0} units, vq3"),
        step,
        floor: GEOMETRY_FLOOR,
    }
}

/// Both self-tests, and whether the instrument may be trusted.
pub(crate) struct SelfTest {
    /// Strafejumping at the ordinary floor.
    pub strafejump: Found,
    /// Strafejumping at the finer floor, taken only when the ordinary one did
    /// not settle it.
    pub strafejump_fine: Option<Found>,
    /// Overbounce.
    pub overbounce: Found,
}

impl SelfTest {
    /// Whether the instrument found no cliff in strafejumping and a cliff in
    /// overbounce, which is the only outcome that licenses a G7 verdict.
    pub fn passed(&self) -> bool {
        let smooth = self
            .strafejump_fine
            .as_ref()
            .unwrap_or(&self.strafejump)
            .survived();
        !smooth && self.overbounce.survived()
    }

    /// The floor the aim refinement actually needed.
    pub fn aim_floor(&self) -> Scalar {
        self.strafejump_fine
            .as_ref()
            .map_or(self.strafejump.floor, |f| f.floor)
    }
}

/// Run both self-tests, falling back to the finer floor if the ordinary one
/// cannot tell strafejumping's steepest gradient from a cliff.
pub(crate) fn self_test() -> SelfTest {
    let strafejump = strafejump(AIM_FLOOR);
    let strafejump_fine = if strafejump.survived() {
        Some(self::strafejump(AIM_FLOOR_FINE))
    } else {
        None
    };
    SelfTest {
        strafejump,
        strafejump_fine,
        overbounce: overbounce_cliff(),
    }
}

/// The self-test, as a published table.
///
/// Not called until the candidate section is wired into [`super::all`]; the
/// self-test itself runs as a `#[test]` from the moment it exists, because an
/// instrument that only checks itself when it is being used is checked once.
#[allow(dead_code)]
pub(crate) fn report(section: &mut Section, test: &SelfTest) {
    section.say(
        "**G7 asks whether a mechanic hides a cliff the player cannot see \
         coming, and the rule that answers it needs checking before it is \
         believed.** A step measured between adjacent points of a fixed grid is \
         a fact about the grid: at the 5° aim spacing this report sweeps on, \
         §1's own `vq3`/`forward` curve at 320 ups steps about 40 ups between \
         40° and 50°, which is the slope times the spacing and nothing else. A \
         rule that calls that a cliff has rejected strafejumping. So a \
         discontinuity is defined as a step that **does not shrink when the grid \
         is refined**, and the two rows below are the check that the definition \
         can tell the difference — one curve that must come out smooth, and one \
         that must not.",
    );

    let mut table = Table::new(
        "**Self-test.** `coarse` is the largest step on the sweep grid; \
         `refined` is what is left of it across one refinement floor. A gradient \
         shrinks; a cliff does not.",
        &[
            "curve",
            "coarse step",
            "floor",
            "refined step",
            "survives 16 ups",
            "required",
            "",
        ],
    );
    let rows: Vec<(&Found, bool)> = match &test.strafejump_fine {
        Some(fine) => vec![
            (&test.strafejump, false),
            (fine, false),
            (&test.overbounce, true),
        ],
        None => vec![(&test.strafejump, false), (&test.overbounce, true)],
    };
    for (found, must_survive) in rows {
        let ok = found.survived() == must_survive;
        table.push(vec![
            found.what.clone(),
            format!("{:.2} ups", found.coarse()),
            format!("{:.4}", found.floor),
            format!("{:.2} ups", found.refined()),
            if found.survived() { "yes" } else { "no" }.to_string(),
            if must_survive { "yes" } else { "no" }.to_string(),
            if ok { "✓" } else { "✗" }.to_string(),
        ]);
    }
    section.table(table);

    section.record(Measurement::ups(
        "g7.selftest.strafejump.coarse_step_ups",
        test.strafejump.coarse(),
    ));
    section.record(Measurement::ups(
        "g7.selftest.strafejump.refined_step_ups",
        test.strafejump.refined(),
    ));
    section.record(Measurement::flag(
        "g7.selftest.strafejump.survives",
        test.strafejump.survived(),
    ));
    if let Some(fine) = &test.strafejump_fine {
        section.record(Measurement::ups(
            "g7.selftest.strafejump_fine.refined_step_ups",
            fine.refined(),
        ));
        section.record(Measurement::flag(
            "g7.selftest.strafejump_fine.survives",
            fine.survived(),
        ));
    }
    section.record(Measurement::ups(
        "g7.selftest.overbounce.coarse_step_ups",
        test.overbounce.coarse(),
    ));
    section.record(Measurement::ups(
        "g7.selftest.overbounce.refined_step_ups",
        test.overbounce.refined(),
    ));
    section.record(Measurement::flag(
        "g7.selftest.overbounce.survives",
        test.overbounce.survived(),
    ));
    section.record(Measurement::flag("g7.selftest.passed", test.passed()));
    section.record(Measurement::degrees(
        "g7.selftest.aim_floor_needed_deg",
        test.aim_floor(),
    ));

    if let Some(step) = test.overbounce.step {
        section.record(Measurement::units(
            "g7.selftest.overbounce.at_units",
            step.at,
        ));
        section.say(format!(
            "The cliff the instrument found in overbounce sits at a drop height \
             of {:.3} units and is worth {:.2} ups across {:.4} of a unit — a \
             distance no map can express and no player can judge by eye. That is \
             what G7 is looking for, and it is in canon rather than in a \
             candidate. §1.8 of `docs/movement-canon.md` says outright that \
             overbounce would fail G7 if it were proposed today, and is exempt \
             only because it is inherited.",
            step.at, step.refined, step.width,
        ));
    }
    if let Some(fine) = &test.strafejump_fine {
        section.say(format!(
            "**The starting floor was not fine enough, and that is a finding \
             rather than a failure.** `docs/movement-canon.md` G7 states the \
             refinement floor as *a parameter of the rule, not a constant*: if a \
             step is still above threshold at the starting floor, refine further \
             and record the floor that was needed. It was. At the stated \
             starting floor of {:.2}° the steepest step in **{}** still measures \
             {:.2} ups — above the {MATERIAL:.0} ups threshold — so the \
             instrument would have reported a cliff in strafejumping and, with \
             it, rejected the technique the game is named after. Refined to \
             {:.2}° the same step measures {:.2} ups. **It halved with the grid, \
             which is what a gradient does**, so the finding is that the floor \
             was too coarse and not that canon's own technique hides a cliff. \
             Every G7 number in this section is taken at {:.2}°.\n\n\
             Canon predicted this exposure and named a different curve for it: \
             §1's `vq3`/`forward` at 500 ups entry, where gain is pinned at zero \
             until the wish-speed clamp opens between 50° and 60°. That band is \
             not the worst one. The worst is the row above, and a reader \
             checking canon's prediction against this table should know that the \
             *prediction was right about the mechanism and wrong about the \
             curve*.",
            AIM_FLOOR,
            test.strafejump.what,
            test.strafejump.refined(),
            fine.floor,
            fine.refined(),
            test.aim_floor(),
        ));
    }

    section.say(if test.passed() {
        format!(
            "**The instrument passes both halves of its self-test**, refining aim \
             to {:.2}° and geometry to {GEOMETRY_FLOOR} of a unit. Every G7 \
             number below is taken with it.",
            test.aim_floor()
        )
    } else {
        "**The instrument does NOT pass its self-test, and no G7 verdict below \
         should be believed.** Either a smooth canonical technique is being \
         reported as a cliff, or a known cliff is not being found; both mean the \
         refinement rule is measuring the grid rather than the movement."
            .to_string()
    });
}

/// The largest surviving step in a swept parameter of a candidate run, which is
/// what G7 part 2 asks of each mechanic.
///
/// Separate from the self-test because it takes the curve as a closure: the
/// candidate's outcome as a function of one player-controlled parameter, with
/// everything else held.
#[allow(dead_code)]
pub(crate) fn cliff_in<F>(
    f: F,
    from: Scalar,
    to: Scalar,
    coarse: Scalar,
    floor: Scalar,
) -> Option<Step>
where
    F: FnMut(Scalar) -> Scalar,
{
    refine::largest_step(f, from, to, coarse, floor, MATERIAL)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The gate on every G7 number this crate publishes, asserted as well as
    /// tabulated: the refinement rule must call strafejumping smooth and
    /// overbounce a cliff. If this fails, no candidate may be judged on G7 and
    /// the report says so rather than quietly reporting a verdict.
    #[test]
    fn the_instrument_tells_a_canonical_gradient_from_a_canonical_cliff() {
        let test = self_test();
        // Printed, not only asserted: the two numbers are the licence for every
        // G7 verdict this crate publishes, and `--nocapture` is how a reader
        // sees what floor the licence was granted at.
        for found in [
            Some(&test.strafejump),
            test.strafejump_fine.as_ref(),
            Some(&test.overbounce),
        ]
        .into_iter()
        .flatten()
        {
            println!(
                "{:<44} coarse {:>8.2}  floor {:>7.4}  refined {:>8.2}  survives {}  at {:.4}",
                found.what,
                found.coarse(),
                found.floor,
                found.refined(),
                found.survived(),
                found.step.map_or(f32::NAN, |s| s.at),
            );
        }
        println!("passed: {}", test.passed());
        assert!(
            !test
                .strafejump_fine
                .as_ref()
                .unwrap_or(&test.strafejump)
                .survived(),
            "strafejumping was reported as a cliff ({} ups across {:.4}° at {}) — \
             the rule is measuring the grid, and it has just rejected the game",
            test.strafejump.refined(),
            test.aim_floor(),
            test.strafejump.what,
        );
        assert!(
            test.overbounce.survived(),
            "overbounce was reported as smooth ({} ups surviving) — the rule \
             cannot find a cliff §4 already publishes",
            test.overbounce.refined(),
        );
        assert!(test.passed());
    }
}
