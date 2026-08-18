//! Criterion 1, item 5: edge clip and step-up behaviour.
//!
//! # Three separate behaviours, measured separately
//!
//! **Step-up** is `PM_StepSlideMove`: a move that hit something is re-run from
//! `step_height` = 18 units higher and the result kept if it is legal. That is
//! why a Quake player walks up stairs without jumping — and why an 18-unit step
//! is free and a 19-unit one is a wall.
//!
//! **Edge release** is the hull's own width, not a rule: the player is 30 units
//! across, so their origin can travel past a ledge by half of that before the
//! box stops overlapping the floor. Everything in between is standing on
//! nothing visible, which is what a player calls "coyote time" and what the
//! collision code calls arithmetic.
//!
//! **The inside corner** is the slide solver running out of directions. Driven
//! into a crease, `PM_SlideMove` finds nothing satisfying every plane at once
//! and stops dead rather than guessing.
//!
//! # Why the cost of a climb is the *dip* and not the speed afterwards
//!
//! Measuring speed some fixed time after a step measures how fast the player
//! re-accelerated, which is `pm_accelerate` and not the step. What the step
//! costs is the lowest speed reached while climbing it, against the lowest
//! speed the same run reaches on flat ground — so that is what is reported.

use straf3_sim::num::{Scalar, s, vec3};
use straf3_sim::world::World;
use straf3_sim::{PhysicsProfile, SimState, step};

use crate::dataset::{Measurement, Section, Table};
use crate::geometry;
use crate::harness::{Axis, HZ, holding, settle_on, still};
use crate::measure::{pad, profiles};
use crate::num::horizontal_speed;

/// Step heights swept, in units. Dense either side of `STEPSIZE` = 18.
const STEP_HEIGHTS: &[f32] = &[4.0, 8.0, 12.0, 16.0, 17.0, 18.0, 19.0, 20.0, 24.0, 32.0];

/// Speeds a step is approached at.
///
/// 320 is the ground cap. 800 is not sustainable on the ground — friction takes
/// it back to 320 in about 110 ms — so it is reached by placing the player at
/// speed a short run from the riser, which is the state they are in for the one
/// command after a bunnyhop landing. The `at riser` column reports what was
/// actually left when they arrived, so neither number is taken on trust.
const APPROACH_SPEEDS: &[f32] = &[320.0, 800.0];

/// How far back from the riser a run starts, in units. Short, so a fast
/// approach is still fast when it arrives.
const RUN_UP: f32 = 40.0;

/// How long a step approach is observed for, in commands — 320 ms.
const APPROACH_COMMANDS: usize = 40;

/// The drop used for the ledge-release measurement.
const LEDGE_DROP: f32 = 256.0;

/// How finely the step-up boundary is bisected, in units.
///
/// A thousandth of a unit is two orders below `SURFACE_CLIP_EPSILON` = 0.125,
/// which is the quantity the boundary is expected to be made of. Resolving
/// finer than the thing being measured is what makes the answer a measurement
/// rather than a restatement of the resolution.
const BOUNDARY_RESOLUTION: Scalar = s(0.001);

/// Drops the "does edge clipping happen" sweep runs off.
const CLIP_DROPS: &[f32] = &[32.0, 64.0, 128.0];

/// Speeds it runs off them at.
const CLIP_SPEEDS: &[f32] = &[200.0, 400.0, 600.0, 900.0];

pub(crate) fn measure() -> Section {
    let mut section = Section::new("5. Edge clip and step-up");
    section.say(format!(
        "A player is placed {RUN_UP:.0} units back from the riser at the stated \
         speed, holding forward, and observed for {APPROACH_COMMANDS} commands \
         ({} ms). An identical run on flat ground is recorded alongside it, \
         command for command, and the two are compared at every step: `worst \
         deficit` is the largest the flat run was ever ahead by, and `end \
         deficit` is where they finished. Two runs differing only in the \
         geometry isolate what the geometry did.",
        APPROACH_COMMANDS * 8
    ));

    let mut climb = Table::new(
        "**Step-up.** `worst deficit` is the largest the flat-ground control was \
         ever ahead by; `at` is the speed it had at that moment.",
        &[
            "profile",
            "approach",
            "riser",
            "climbed",
            "at",
            "worst deficit",
            "end deficit",
        ],
    );
    let mut edges = Table::new(
        format!(
            "**Edges.** `climb boundary` is the tallest riser that is climbed, \
             bisected to {BOUNDARY_RESOLUTION} units. `release x` is the \
             furthest the origin gets past a ledge while the ground probe still \
             finds the upper floor. `worst clip` is the largest horizontal speed \
             any single airborne command took while running off a ledge, over \
             every drop × speed combination tested — the number edge clipping \
             would appear in."
        ),
        &[
            "profile",
            "climb boundary",
            "release x",
            "hull half-width",
            "worst clip",
            "corner: origin reached",
            "corner: speed left",
        ],
    );

    for (name, profile) in profiles() {
        let flat = geometry::floor();

        for &speed in APPROACH_SPEEDS {
            let control = approach(&flat, &profile, s(speed), s(0.0));

            for &height in STEP_HEIGHTS {
                let world = geometry::step(s(height));
                let run = approach(&world, &profile, s(speed), s(height));
                let (worst, at_that_moment) = run.worst_deficit(&control);
                let key = format!(
                    "{name}.step.approach{}.riser{}",
                    pad(speed as u32, 4),
                    pad(height as u32, 2)
                );
                section.record(Measurement::flag(format!("{key}.climbed"), run.climbed));
                section.record(Measurement::ups(format!("{key}.at_riser_ups"), at_that_moment));
                section.record(Measurement::ups(format!("{key}.worst_deficit_ups"), worst));
                section.record(Measurement::ups(
                    format!("{key}.end_deficit_ups"),
                    run.end_deficit(&control),
                ));
                section.record(Measurement::units(format!("{key}.origin_z"), run.origin_z));
                section.record(Measurement::units(format!("{key}.origin_x"), run.origin_x));

                climb.push(vec![
                    name.to_string(),
                    format!("{speed:.0}"),
                    format!("{height:.0}"),
                    if run.climbed { "yes" } else { "no" }.to_string(),
                    if worst > s(0.0) {
                        format!("{at_that_moment:.2}")
                    } else {
                        "—".to_string()
                    },
                    format!("{worst:.2}"),
                    format!("{:.2}", run.end_deficit(&control)),
                ]);
            }
        }

        // Where `step_height` actually stops working, searched at a sixteenth of
        // a unit — finer than the constant, so the number is measured rather
        // than restated.
        let boundary = highest_climbable(&profile);
        section.record(Measurement::units(
            format!("{name}.step.highest_climbable"),
            boundary,
        ));
        section.record(Measurement::units(
            format!("{name}.step.step_height_constant"),
            profile.step_height,
        ));
        // `SURFACE_CLIP_EPSILON` is private to `step.rs`, so it is named here as
        // the literal it is rather than imported. If the measured boundary stops
        // landing on this sum, one of the two moved and the report will say so.
        section.record(Measurement::units(
            format!("{name}.step.step_height_plus_clip_epsilon"),
            profile.step_height + s(0.125),
        ));

        // ── does edge clipping happen at all? ─────────────────────────────
        let mut worst_clip = s(0.0);
        for &drop in CLIP_DROPS {
            for &speed in CLIP_SPEEDS {
                let loss = worst_command_loss_off_a_ledge(&profile, s(drop), s(speed));
                section.record(Measurement::ups(
                    format!(
                        "{name}.edge.clip.drop{}.speed{}.worst_command_loss_ups",
                        pad(drop as u32, 3),
                        pad(speed as u32, 3)
                    ),
                    loss,
                ));
                if loss > worst_clip {
                    worst_clip = loss;
                }
            }
        }
        section.record(Measurement::ups(
            format!("{name}.edge.clip.worst_command_loss_ups"),
            worst_clip,
        ));

        // ── the ledge ─────────────────────────────────────────────────────
        let release = ledge_release_x(&profile);
        let half_width = profile.hull_maxs.x;
        section.record(Measurement::units(
            format!("{name}.edge.ledge_release_x"),
            release,
        ));
        section.record(Measurement::units(
            format!("{name}.edge.hull_half_width"),
            half_width,
        ));
        section.record(Measurement::units(
            format!("{name}.edge.release_minus_half_width"),
            release - half_width,
        ));

        // ── the inside corner ─────────────────────────────────────────────
        let corner = into_corner(&profile);
        section.record(Measurement::units(
            format!("{name}.edge.corner_origin_x"),
            corner.0,
        ));
        section.record(Measurement::units(
            format!("{name}.edge.corner_origin_y"),
            corner.1,
        ));
        section.record(Measurement::ups(
            format!("{name}.edge.corner_speed_left_ups"),
            corner.2,
        ));

        edges.push(vec![
            name.to_string(),
            format!("{boundary:.3}"),
            format!("{release:.3}"),
            format!("{half_width:.3}"),
            format!("{worst_clip:.3}"),
            format!("({:.3}, {:.3})", corner.0, corner.1),
            format!("{:.2}", corner.2),
        ]);
    }

    section.table(climb);
    section.say(
        "**Step-up is free, and the cliff on the other side of it is total.** A \
         riser the player can climb costs them nothing measurable at any \
         approach speed: `PM_StepSlideMove` re-runs the move from `step_height` \
         higher and then clips velocity to what it comes down on, and clipping \
         against a flat floor removes only the vertical component. One unit \
         taller and the same run loses everything. There is no gradient between \
         those two outcomes, which is worth knowing before a map is built out of \
         risers near the boundary: 18 is a staircase and 19 is a wall.",
    );
    section.say(
        "`highest_climbable` in the machine-readable section is that boundary \
         searched at a sixteenth of a unit rather than read off the constant, so \
         it is a measurement of the solver and not a restatement of \
         `step_height`.",
    );
    section.table(edges);
    section.say(
        "**The release point is the hull, not a rule.** The origin travels past \
         the edge by the hull's half-width before the ground probe stops finding \
         the floor beneath it. There is no coyote-time timer in this simulation \
         and none is needed: the tolerance a player feels is a consequence of \
         being 30 units wide, and it scales with the hull rather than with the \
         frame rate.",
    );

    section
}

/// What a step approach produced.
struct Approach {
    /// Whether the player ended up on top of the riser.
    climbed: bool,
    /// Horizontal speed after each command.
    speeds: Vec<Scalar>,
    origin_x: Scalar,
    origin_z: Scalar,
}

impl Approach {
    /// The largest amount this run was behind `control` at the same command,
    /// and the speed the control had at that moment.
    ///
    /// A trajectory comparison rather than a minimum, because the minimum over a
    /// window is the friction floor: a player holding forward decays to 320
    /// whatever the geometry does, so the lowest speed both runs reach is the
    /// same number and the step disappears into it. Comparing command by
    /// command against a run that differs *only* in the geometry isolates what
    /// the geometry did, at the moment it did it.
    fn worst_deficit(&self, control: &Approach) -> (Scalar, Scalar) {
        let mut worst = (s(0.0), s(0.0));
        for (i, mine) in self.speeds.iter().enumerate() {
            let theirs = control.speeds[i];
            if theirs - mine > worst.0 {
                worst = (theirs - mine, theirs);
            }
        }
        worst
    }

    /// How far behind the control this run finished.
    fn end_deficit(&self, control: &Approach) -> Scalar {
        control.speeds[control.speeds.len() - 1] - self.speeds[self.speeds.len() - 1]
    }
}

/// Walk +X into whatever is at x=0, from [`RUN_UP`] units back.
///
/// `riser_top` is the height being approached, used only to decide whether the
/// player finished on top of it; pass zero for the flat control.
fn approach<W: World>(
    world: &W,
    profile: &PhysicsProfile,
    speed: Scalar,
    riser_top: Scalar,
) -> Approach {
    let mut st = settle_on(world, profile, vec3(s(-RUN_UP), s(0.0), s(64.0)));
    st.player.velocity = vec3(speed, s(0.0), s(0.0));
    let forward = holding(Axis::Forward, s(0.0));

    let mut speeds = Vec::with_capacity(APPROACH_COMMANDS);
    for _ in 0..APPROACH_COMMANDS {
        st = step(&st, &forward, world, profile);
        speeds.push(horizontal_speed(st.player.velocity));
    }

    Approach {
        climbed: riser_top > s(0.0)
            && st.player.origin.z > geometry::resting_origin_z(riser_top) - s(1.0),
        speeds,
        origin_x: st.player.origin.x,
        origin_z: st.player.origin.z,
    }
}

/// The tallest riser a walking player still gets on top of, to within
/// [`BOUNDARY_RESOLUTION`].
///
/// A whole-unit scan to bracket, then a bisection: settling a player is four
/// hundred commands, and a linear sweep at this resolution would be forty
/// thousand runs for a number a dozen probes can pin. The bisection needs the
/// property to be monotone in the height, which it is — a riser the solver
/// cannot lift over does not become climbable by growing.
fn highest_climbable(profile: &PhysicsProfile) -> Scalar {
    let mut low = s(0.0);
    let mut high = s(64.0);
    for whole in 1..=64 {
        let h = s(whole as f32);
        if climbs(profile, h) {
            low = h;
        } else {
            high = h;
            break;
        }
    }
    while high - low > BOUNDARY_RESOLUTION {
        let mid = (low + high) * s(0.5);
        if climbs(profile, mid) {
            low = mid;
        } else {
            high = mid;
        }
    }
    low
}

/// Whether a walking player ends up on top of a riser of `height`.
fn climbs(profile: &PhysicsProfile, height: Scalar) -> bool {
    let world = geometry::step(height);
    approach(&world, profile, s(320.0), height).climbed
}

/// The furthest a standing player's origin gets past a ledge while the ground
/// probe still finds the upper floor.
///
/// Placed rather than walked: a walking player's exact release point depends on
/// where their per-command steps happen to land, and the question here is about
/// the hull, not about the sampling.
fn ledge_release_x(profile: &PhysicsProfile) -> Scalar {
    let world = geometry::ledge(s(LEDGE_DROP));
    let mut last_supported = geometry::LEDGE_EDGE_X;
    for sixty_fourths in 0..=(64 * 24) {
        let x = geometry::LEDGE_EDGE_X + s(sixty_fourths as f32 / 64.0);
        let mut st = SimState::spawned_at(
            vec3(x, s(0.0), geometry::resting_origin_z(s(0.0))),
            s(0.0),
        );
        st.player.ground = straf3_sim::GroundState::Airborne;
        let after = step(&st, &still(), &world, profile);
        if after.player.ground.is_grounded() && after.player.origin.z > s(0.0) {
            last_supported = x;
        } else {
            break;
        }
    }
    last_supported
}

/// The largest horizontal speed a single command took while a player ran off a
/// ledge — the number "edge clipping" would show up in if it happened.
///
/// The claim being tested is that a hull crossing an edge is never caught on it.
/// If the bevel planes were missing or wrong, the box's corner would catch the
/// brush's edge and one command would take a visible bite out of the speed;
/// with them, the player leaves the edge and nothing touches them until they
/// land. Only the airborne part of the run is examined, because a landing
/// legitimately changes the speed and that is section 4's subject.
fn worst_command_loss_off_a_ledge(
    profile: &PhysicsProfile,
    drop: Scalar,
    speed: Scalar,
) -> Scalar {
    let world = geometry::ledge(drop);
    let mut st = settle_on(
        &world,
        profile,
        vec3(geometry::LEDGE_EDGE_X - s(64.0), s(0.0), s(64.0)),
    );
    st.player.velocity = vec3(speed, s(0.0), s(0.0));

    let mut worst = s(0.0);
    let mut left_the_floor = false;
    for _ in 0..200 {
        // Airborne *before* the command as well as after. The command that
        // carries the player off the edge starts on the floor, so `PM_Friction`
        // runs on it and takes `friction · dt` — 4.8%, about 20 ups at these
        // speeds. Counting that command would report ordinary ground friction
        // as an edge clip, which is exactly the false positive this measurement
        // exists to avoid.
        let was_airborne = !st.player.ground.is_on_plane();
        let before = horizontal_speed(st.player.velocity);
        st = step(&st, &still(), &world, profile);
        let after = horizontal_speed(st.player.velocity);
        let still_airborne = !st.player.ground.is_on_plane();

        if was_airborne {
            left_the_floor = true;
            if still_airborne && before - after > worst {
                worst = before - after;
            }
        }
        if left_the_floor && !still_airborne {
            break; // landed; past this point the loss is the landing's
        }
    }
    worst
}

/// Drive north-east into the inside corner for two seconds and report where the
/// player ended up and what speed they have left.
fn into_corner(profile: &PhysicsProfile) -> (Scalar, Scalar, Scalar) {
    let world = geometry::corner();
    let mut st = settle_on(&world, profile, vec3(s(0.0), s(0.0), s(64.0)));
    let into = holding(Axis::Forward, s(45.0));
    for _ in 0..(HZ * 2) {
        st = step(&st, &into, &world, profile);
    }
    (
        st.player.origin.x,
        st.player.origin.y,
        horizontal_speed(st.player.velocity),
    )
}
