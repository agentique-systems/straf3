//! What splitting a command into sub-steps changed, measured rather than
//! assumed.
//!
//! # The limit this section closes
//!
//! Every previous edition of `docs/movement-lab.md` opened with a limit saying
//! that `pmove_msec` sub-stepping was not implemented, that **every number in
//! the document was valid for single-step integration only**, that section 4's
//! overbounce counts were the most exposed, and that neither this seat nor the
//! sim seat had measured the difference. Sub-stepping has landed
//! (`crates/straf3-sim/src/step.rs`, `PMOVE_SUBSTEP_MAX_MS`). This section is
//! the measurement that limit was waiting for.
//!
//! # Why the answer is zero, and why that is a result rather than a let-off
//!
//! A command is split into sub-steps of at most 66 ms. Every command rate this
//! project offers is 13 ms or shorter, so every command is **one** sub-step, and
//! one sub-step is the integration that was there before. The delta is therefore
//! not "small" or "within tolerance": it is zero, for the same reason that
//! dividing by one changes nothing.
//!
//! A zero that is asserted is worth little, so what is published below is the
//! *mechanism* — how many sub-steps each rate produces, checked against the
//! rates the game actually offers — and the *shape past the bound*, where the
//! two integrations genuinely differ. A reader can then see that the zero is a
//! consequence of where the bound sits rather than of nothing having changed.
//!
//! # What this section cannot measure, and who measured it instead
//!
//! The superseded single-step integration is reachable only through
//! `step_bounded`, which is **private on purpose**: its own documentation says
//! "it is not a knob", and the one caller allowed to pass anything but the bound
//! is `step.rs`'s own test module — precisely so that "nothing at or below the
//! bound moved" is measured through the shipped code rather than through a copy
//! of it. The lab is above that seam and cannot reach it, and it must not answer
//! that by writing its own single-step integrator: a duplicated integrator that
//! drifts from the shipped one produces a comparison against a game nobody runs.
//!
//! So the single-step column in [`friction`] is **cited, not measured** — taken
//! from the sim seat's published table, in exactly the form section 7 already
//! uses for that seat's numbers. Everything else here is this crate's own
//! measurement.
//!
//! # One piece of the old text that was wrong
//!
//! The `TODO` sub-stepping replaced said a large command could tunnel through
//! geometry. It could not: `slide_move` sweeps a hull continuously, so no
//! duration ever risked tunnelling in this tree. What a large command actually
//! did was integrate friction, gravity and every timer in one lump, and spend
//! the whole bump budget on one enormous move. That is the framing below, and
//! the earlier one is not repeated.

use straf3_sim::num::{s, vec3};
use straf3_sim::world::{FlatGround, World};
use straf3_sim::{PMOVE_SUBSTEP_MAX_MS, PhysicsProfile, SimState, TickRate, UserCmd, run, step};

use crate::dataset::{Measurement, Section, Table};
use crate::harness::{Axis, holding, settle_on, yaw_for};
use crate::measure::pad;
use crate::num::{heading_degrees, horizontal_speed};

/// Command durations the tables sweep, in milliseconds.
///
/// The three below the bound are the three command rates the project offers
/// (250 Hz, 125 Hz, 76 Hz); 33 and 66 bracket the bound from underneath; 67 is
/// the first duration that splits at all; the rest are the shape of a stall.
const DURATIONS: &[u16] = &[4, 8, 13, 33, 66, 67, 100, 132, 200, 264, 500, 1000];

/// The rates the game can actually be played at.
const RATES: &[TickRate] = &[TickRate::HZ_250, TickRate::HZ_125, TickRate::HZ_76];

/// The speed the friction fixture coasts from.
const COAST_UPS: f32 = 800.0;

/// The speed the air-control fixture flies at.
const AIR_UPS: f32 = 640.0;

/// The angle the air-control fixture holds off its velocity.
///
/// Held on the **forward** axis alone, which is what gates CPM's air control on
/// (`move_dir == MoveDir::ForwardBack`). The sim seat's own sub-stepping fixture
/// held the view diagonally, which switches air control off, and that seat said
/// so and left the case here. This is that case.
const AIR_ANGLE: f32 = 30.0;

/// How a command of `ms` is split, as the sub-step lengths themselves.
///
/// The rule from `step.rs`: take the bound while more than the bound remains,
/// and the remainder is the last, short sub-step rather than a residue spread
/// across the others.
fn split(ms: u16) -> Vec<u16> {
    let mut out = Vec::new();
    let mut remaining = ms;
    while remaining > 0 {
        let piece = remaining.min(PMOVE_SUBSTEP_MAX_MS);
        out.push(piece);
        remaining -= piece;
    }
    out
}

/// A player coasting on flat ground with no keys held, so `PM_Friction` is the
/// only thing acting on them.
fn coasting(profile: &PhysicsProfile) -> SimState {
    let world = FlatGround::at(s(0.0));
    let mut st = settle_on(&world, profile, vec3(s(0.0), s(0.0), s(64.0)));
    st.player.velocity = vec3(s(COAST_UPS), s(0.0), s(0.0));
    st
}

/// A player airborne at speed, in empty space, holding one angle off their
/// velocity on the forward axis.
fn flying(profile: &PhysicsProfile) -> (SimState, UserCmd) {
    let mut st = SimState::spawned_at(vec3(s(0.0), s(0.0), s(16_384.0)), s(0.0));
    st.player.velocity = vec3(s(AIR_UPS), s(0.0), s(0.0));
    st.player.ground = straf3_sim::GroundState::Airborne;
    let want = heading_degrees(st.player.velocity) + s(AIR_ANGLE);
    let cmd = holding(Axis::Forward, yaw_for(Axis::Forward, want));
    let _ = profile;
    (st, cmd)
}

/// The same command, at a different duration.
fn lasting(cmd: &UserCmd, ms: u16) -> UserCmd {
    UserCmd {
        duration_ms: ms,
        ..*cmd
    }
}

pub(crate) fn measure() -> Section {
    let mut section = Section::new("8. Sub-stepping");
    let profile = PhysicsProfile::cpm();

    section.say(format!(
        "`crates/straf3-sim/src/step.rs` now runs Quake 3's outer `Pmove` loop: \
         a command longer than **{PMOVE_SUBSTEP_MAX_MS} ms** \
         (`straf3_sim::PMOVE_SUBSTEP_MAX_MS`) is split into sub-steps of at most \
         that length, and each sub-step gets its own `dt`, its own ground probe, \
         its own timer drop and its own solver. Before it, one command of any \
         length was integrated once — which did not risk tunnelling, because \
         `slide_move` sweeps a hull continuously, but did integrate friction, \
         gravity and every timer in a single lump and spend the whole bump \
         budget on one enormous move."
    ));
    section.say(
        "**This section closes the limit this document has carried since it was \
         written.** Every previous edition said that every number in it was \
         valid for single-step integration only, that section 4's overbounce \
         counts were the most exposed, and that neither this seat nor the sim \
         seat had measured the difference. It is measured now, and the delta is \
         **zero at every rate the game offers**. The three tables below are why: \
         the first shows that no rate produces more than one sub-step, the \
         second checks the split rule from outside the simulation, and the third \
         shows what the split is worth where it does apply — because a zero \
         whose mechanism is not published is indistinguishable from a \
         measurement nobody took.",
    );

    let rates = rates(&mut section);
    section.table(rates);
    let split_rule = split_rule(&mut section, &profile);
    section.table(split_rule);
    let friction = friction(&mut section, &profile);
    section.table(friction);
    let air_control = air_control(&mut section, &profile);
    section.table(air_control);

    section.say(format!(
        "**Where the delta lives is ground friction, and the 200 ms row of the \
         friction table is the whole argument.** `PM_Friction` removes \
         `speed · friction · dt` per step. One 200 ms step takes \
         800 · 6 · 0.2 = 960 ups from a player who has {COAST_UPS:.0}, so it \
         stops them dead; four sub-steps decay them and leave them running. That \
         is the \"framerate dependent behavior\" id's own comment names, and it \
         is a stall being converted from a full stop into a slowdown."
    ));
    section.say(
        "**Air control is the one airborne rule that is genuinely step-size \
         dependent**, because `PM_Aircontrol` renormalises a direction vector \
         once per step: chop the same elapsed time more finely and the steering \
         is applied more often. The sim seat flagged this and could not measure \
         it — its fixture holds the view diagonally, which is exactly what gates \
         air control off (`move_dir` must be pure forward/back) — so the fourth \
         table is that case, held on the forward axis alone. The gap is large: \
         the same held input turns a player noticeably further when the time is \
         chopped finely. **It is still zero at every rate the game offers**, for \
         the same reason everything else here is — an 8 ms command is one step \
         either way — and what sub-stepping buys is the row a *long* command \
         lands on, which is now the 66 ms answer rather than whatever a single \
         step of that length would have produced.",
    );
    section.say(
        "**What is measured here and what is cited.** The single-step column of \
         the friction table is the sim seat's published number, restated in the \
         same way section 7 restates that seat's other figures. The lab cannot \
         re-measure it: the superseded integration is reachable only through \
         `step_bounded`, which is private on purpose so that the comparison runs \
         through the shipped code rather than through a copy of it, and writing \
         a second integrator here to get at it would produce a comparison \
         against a game nobody runs. Every other number in this section is this \
         crate's own.",
    );

    section
}

/// How many sub-steps each playable command rate produces.
fn rates(section: &mut Section) -> Table {
    let mut table = Table::new(
        format!(
            "**Sub-steps per command, at every rate the game offers.** A command \
             at or below {PMOVE_SUBSTEP_MAX_MS} ms is one sub-step, and one \
             sub-step is the integration that was there before — so a column of \
             ones is the reason every number in this document is unchanged."
        ),
        &["rate", "command", "sub-steps", "same as single-step"],
    );
    for rate in RATES {
        let ms = rate.command_millis();
        let steps = split(ms).len() as u32;
        let key = format!("substep.rate.hz{}", pad(rate.hz(), 3));
        section.record(Measurement::ms(format!("{key}.command_ms"), u32::from(ms)));
        section.record(Measurement::count(format!("{key}.substeps"), steps));
        section.record(Measurement::flag(format!("{key}.unchanged"), steps == 1));
        table.push(vec![
            format!("{} Hz", rate.hz()),
            format!("{ms} ms"),
            steps.to_string(),
            if steps == 1 { "yes" } else { "no" }.to_string(),
        ]);
    }
    section.record(Measurement::ms(
        "substep.bound_ms",
        u32::from(PMOVE_SUBSTEP_MAX_MS),
    ));
    section.record(Measurement::ms(
        "substep.slowest_rate_command_ms",
        u32::from(TickRate::HZ_76.command_millis()),
    ));
    table
}

/// The split rule, checked from outside the simulation.
///
/// If a command of `d` ms really is integrated as the sub-step sequence
/// `split(d)`, then handing the mover one command of `d` ms and handing it that
/// sequence as separate commands must leave the player in the same place. That
/// is checkable without reaching into the private function, and it is the whole
/// claim.
fn split_rule(section: &mut Section, profile: &PhysicsProfile) -> Table {
    let mut table = Table::new(
        "**The split rule, checked from outside.** One command of `d` ms against \
         the sub-step lengths it claims to split into, delivered as separate \
         commands. The player state must be identical — not close — or the \
         sub-step sequence published is not the one being run. Two openings, \
         because the ground path and the air path split the same way but \
         integrate different rules.",
        &[
            "command",
            "sub-steps",
            "lengths",
            "coasting: identical",
            "airborne: identical",
        ],
    );
    let world = FlatGround::at(s(0.0));
    let (air_start, air_cmd) = flying(profile);

    for &ms in DURATIONS {
        let pieces = split(ms);
        let ground_same = {
            let start = coasting(profile);
            let one = step(&start, &lasting(&UserCmd::still(ms), ms), &world, profile);
            let many: Vec<UserCmd> = pieces.iter().map(|p| UserCmd::still(*p)).collect();
            let seq = run(&start, &many, &world, profile);
            one.player == seq.player
        };
        let air_same = {
            let one = step(
                &air_start,
                &lasting(&air_cmd, ms),
                &straf3_sim::world::EmptyWorld,
                profile,
            );
            let many: Vec<UserCmd> = pieces.iter().map(|p| lasting(&air_cmd, *p)).collect();
            let seq = run(&air_start, &many, &straf3_sim::world::EmptyWorld, profile);
            one.player == seq.player
        };
        let key = format!("substep.split.ms{}", pad(u32::from(ms), 4));
        section.record(Measurement::count(
            format!("{key}.substeps"),
            pieces.len() as u32,
        ));
        section.record(Measurement::flag(
            format!("{key}.ground_matches"),
            ground_same,
        ));
        section.record(Measurement::flag(format!("{key}.air_matches"), air_same));
        table.push(vec![
            format!("{ms} ms"),
            pieces.len().to_string(),
            pieces
                .iter()
                .map(std::string::ToString::to_string)
                .collect::<Vec<_>>()
                .join(" + "),
            if ground_same { "yes" } else { "**no**" }.to_string(),
            if air_same { "yes" } else { "**no**" }.to_string(),
        ]);
    }
    table
}

/// One claim from the sim seat's sub-stepping table: speed remaining after a
/// single command of this length under the *superseded* single-step
/// integration.
///
/// Restated verbatim, with the same discipline section 7 applies to that seat's
/// numbers: it is their measurement, it is not recomputed here, and if it is
/// wrong this table faithfully reproduces the error.
struct SingleStep {
    ms: u16,
    ups: f32,
}

/// The sim seat's published single-step figures for the coasting fixture.
const SINGLE_STEP: &[SingleStep] = &[
    SingleStep {
        ms: 66,
        ups: 483.20,
    },
    SingleStep {
        ms: 67,
        ups: 478.40,
    },
    SingleStep {
        ms: 100,
        ups: 320.00,
    },
    SingleStep { ms: 200, ups: 0.00 },
    SingleStep { ms: 500, ups: 0.00 },
];

/// Speed remaining after one command of each length, coasting under friction
/// alone — the case where the two integrations differ most.
fn friction(section: &mut Section, profile: &PhysicsProfile) -> Table {
    let mut table = Table::new(
        format!(
            "**Ground friction: horizontal speed remaining after one command.** \
             Coasting from {COAST_UPS:.0} ups on flat ground with no keys held, \
             so `PM_Friction` is the only rule acting. `sub-stepped` is measured \
             here; `single-step` is the sim seat's published figure for the \
             integration this replaced, restated rather than recomputed (see the \
             note below the tables). The delta is **exactly zero at and below \
             the bound**, which is the row that matters for every other number \
             in this document."
        ),
        &[
            "command",
            "sub-steps",
            "sub-stepped",
            "single-step (sim seat)",
            "delta",
        ],
    );
    let world = FlatGround::at(s(0.0));
    for &ms in DURATIONS {
        let start = coasting(profile);
        let after = step(&start, &UserCmd::still(ms), &world, profile);
        let ups = horizontal_speed(after.player.velocity);
        let key = format!("substep.friction.ms{}", pad(u32::from(ms), 4));
        section.record(Measurement::ups(format!("{key}.remaining_ups"), ups));

        let cited = SINGLE_STEP.iter().find(|c| c.ms == ms);
        let (theirs, delta) = match cited {
            Some(c) => (format!("{:.2}", c.ups), format!("{:+.2}", ups - s(c.ups))),
            None => ("—".to_string(), "—".to_string()),
        };
        if let Some(c) = cited {
            section.record(Measurement::ups(
                format!("{key}.single_step_delta_ups"),
                ups - s(c.ups),
            ));
        }
        table.push(vec![
            format!("{ms} ms"),
            split(ms).len().to_string(),
            format!("{ups:.2}"),
            theirs,
            delta,
        ]);
    }
    table
}

/// Air control's step-size dependence: the case the sim seat could not measure.
///
/// # The confound this is written to avoid
///
/// The obvious version of this measurement re-aims off the current velocity
/// every command, exactly as [`crate::harness::strafe_for`] does. It is wrong
/// here, and wrong in a way that reads as a result: a run delivered as 33
/// commands re-aims 33 times and one delivered as a single command re-aims once,
/// so the two differ in **input** and the difference gets attributed to the
/// integration. The first version of this table reported the 66 ms and 264 ms
/// rows as 31.98° and 22.81°, which the split-rule table above says outright is
/// impossible: a 264 ms command *is* four 66 ms sub-steps.
///
/// So the view is fixed for the whole 264 ms — one absolute yaw, held — and the
/// only thing that varies is how the time is chopped. The 66, 132 and 264 ms
/// rows then agree exactly, which is not a coincidence to be explained away but
/// the table's own consistency check: all three are four sub-steps of 66 ms.
fn air_control(section: &mut Section, profile: &PhysicsProfile) -> Table {
    let mut table = Table::new(
        format!(
            "**Air control: the same {ELAPSED_MS} ms, chopped differently.** \
             Airborne at {AIR_UPS:.0} ups in empty space with the view fixed \
             {AIR_ANGLE:.0}° off the entry heading, on the forward axis alone — \
             which is what switches CPM's `air_control` on. `PM_Aircontrol` \
             renormalises a direction once per step, so it is the one airborne \
             rule whose answer depends on how the time was divided. **The last \
             three rows are identical, and that is the point**: sub-stepping \
             pins a long command to the {PMOVE_SUBSTEP_MAX_MS} ms answer instead \
             of letting it slide further down this curve, which is what \
             integrating it in one step did."
        ),
        &[
            "command",
            "commands",
            "steps of",
            "total steps",
            "speed",
            "heading turned",
        ],
    );
    let world = straf3_sim::world::EmptyWorld;
    for &ms in &[8u16, 33, 66, 132, 264] {
        let (start, cmd) = flying(profile);
        let commands = ELAPSED_MS / ms;
        let end = integrate(&start, &cmd, commands, ms, &world, profile);
        let ups = horizontal_speed(end.player.velocity);
        let turned = heading_degrees(end.player.velocity) - heading_degrees(start.player.velocity);
        let pieces = split(ms);
        let key = format!("substep.air_control.ms{}", pad(u32::from(ms), 4));
        section.record(Measurement::ups(format!("{key}.speed_ups"), ups));
        section.record(Measurement::degrees(format!("{key}.turned_deg"), turned));
        table.push(vec![
            format!("{ms} ms"),
            commands.to_string(),
            format!("{} ms", pieces[0]),
            (pieces.len() * commands as usize).to_string(),
            format!("{ups:.2} ups"),
            format!("{turned:.2}°"),
        ]);
    }
    table
}

/// How long the air-control comparison runs for, in milliseconds.
///
/// 264 = 4 × 66, and it is divisible by every step size in the table, so every
/// row covers exactly the same elapsed time with no remainder to explain.
const ELAPSED_MS: u16 = 264;

/// Deliver the same fixed-view command `count` times at `ms` each.
fn integrate<W: World>(
    start: &SimState,
    cmd: &UserCmd,
    count: u16,
    ms: u16,
    world: &W,
    profile: &PhysicsProfile,
) -> SimState {
    let mut st = *start;
    let held = lasting(cmd, ms);
    for _ in 0..count {
        st = step(&st, &held, world, profile);
    }
    st
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The split rule, as arithmetic rather than as a table: the bound while
    /// more than the bound remains, then whatever is left.
    #[test]
    fn a_command_splits_into_the_bound_and_a_remainder() {
        assert_eq!(split(8), vec![8]);
        assert_eq!(split(66), vec![66]);
        assert_eq!(split(67), vec![66, 1]);
        assert_eq!(split(132), vec![66, 66]);
        assert_eq!(split(200), vec![66, 66, 66, 2]);
        assert_eq!(split(1000).len(), 16);
        assert_eq!(split(1000).iter().map(|p| u32::from(*p)).sum::<u32>(), 1000);
    }

    /// The claim the whole section rests on, asserted as well as tabulated: no
    /// rate the game offers splits at all. If a rate is ever added that does,
    /// this fails and the document stops saying the delta is zero.
    #[test]
    fn no_playable_rate_produces_more_than_one_substep() {
        for rate in RATES {
            assert_eq!(
                split(rate.command_millis()).len(),
                1,
                "{} Hz commands are {} ms, which splits",
                rate.hz(),
                rate.command_millis()
            );
        }
        assert!(TickRate::HZ_76.command_millis() <= PMOVE_SUBSTEP_MAX_MS);
    }
}
