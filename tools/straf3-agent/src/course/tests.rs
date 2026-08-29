//! The derivation tested against `.map` text, on maps this crate invents.
//!
//! # Why the fixtures are synthetic, and why none of them is `coil`
//!
//! r27's claim is that no map-specific constant appears in the agent's source.
//! A test that only ever ran on the one map the prior art was tuned for could
//! not tell a general rule from a lucky one. So every fixture here is a shape
//! chosen to break a specific piece of the derivation: a course that runs west
//! instead of north, a finish volume whose floor is nowhere near its `mins.z`,
//! a start line made of two brushes, a volume hanging over nothing.
//!
//! `coil.map` and `training-crouch-slide.map` are exercised too — by
//! `tests/first_party.rs`, against the files themselves, because a test that
//! pasted their numbers in here would be the very thing r27 forbids.
//!
//! # The brush fixtures
//!
//! The primitives that write `.map` text — and in particular the winding rule
//! that decides whether a brush is a floor or a hole — live in [`crate::brush`],
//! because `crate::fixture` needs the same ones to generate a committed map and
//! two copies of that rule would be one copy too many.

use super::*;

use crate::brush::{box_brush, brush_entity, point_entity, timed_trigger, timed_trigger_with};
use straf3_map::compile;

fn plan_of(source: &str) -> (CompiledMap, CoursePlan) {
    let map = compile(source).expect("the fixture must compile");
    let profile = PhysicsProfile::cpm();
    let plan = CoursePlan::derive(&map, &profile, "cpm");
    (map, plan)
}

/// A course that runs **west**, not north: start at `x = 0`, finish at
/// `x = -1584`, on a floor at `z = 0`.
///
/// Deliberately not `+y`. `probes/coil-course` maximises `y` and would run
/// backwards out of this map; nothing in the derivation should notice.
///
/// Its corridor is 384 units wide and its triggers span `-160..160`. Both
/// numbers are deliberately unlike the shipped maps', which are 448 wide with
/// triggers at `-224..224` — the two first-party maps share those dimensions,
/// so a derivation that had quietly keyed on one of them would still pass a
/// fixture that reused them. None of the fixtures below shares a dimension with
/// `assets/maps/`.
fn westward_course() -> String {
    let mut world = box_brush([-1856, -192, -80], [192, 192, 0], "straf3/floor");
    world.push_str(&box_brush(
        [-1856, -256, -80],
        [192, -192, 352],
        "straf3/wall",
    ));
    world.push_str(&box_brush(
        [-1856, 192, -80],
        [192, 256, 352],
        "straf3/wall",
    ));

    let mut map = brush_entity("worldspawn", &[("message", "westward")], &[world]);
    map.push_str(&point_entity(
        "info_player_start",
        &[("origin", "144 0 24"), ("angle", "180")],
    ));
    map.push_str(&timed_trigger(
        "target_startTimer",
        "t_start",
        &[box_brush([-40, -160, -48], [0, 160, 576], "common/trigger")],
    ));
    map.push_str(&timed_trigger(
        "target_checkpoint",
        "t_cp1",
        &[box_brush(
            [-816, -160, -48],
            [-784, 160, 576],
            "common/trigger",
        )],
    ));
    map.push_str(&timed_trigger(
        "target_stopTimer",
        "t_finish",
        &[box_brush(
            [-1600, -160, -48],
            [-1568, 160, 576],
            "common/trigger",
        )],
    ));
    map
}

#[test]
fn the_course_is_start_then_checkpoints_in_order_then_finish() {
    let (_, plan) = plan_of(&westward_course());
    let steps: Vec<Step> = plan.waypoints.iter().map(|w| w.step).collect();
    assert_eq!(
        steps,
        vec![Step::Start, Step::Checkpoint(0), Step::Finish],
        "the map hands over the order; the agent does not invent one"
    );
    assert!(plan.is_runnable());
}

#[test]
fn every_aim_point_lands_inside_its_own_volume() {
    let (map, plan) = plan_of(&westward_course());
    let profile = PhysicsProfile::cpm();
    for w in &plan.waypoints {
        for t in &w.targets {
            assert!(t.aim_inside, "{:?} aim {:?} is outside it", w.step, t.aim);
            // Not merely the flag the derivation set: asked again, of the
            // volume, the way the run clock asks it.
            assert!(
                map.triggers[t.trigger].intersects_box(
                    t.aim + profile.hull_center_offset(),
                    profile.hull_half_extents()
                ),
                "{:?} disagrees with its own aim_inside",
                w.step
            );
        }
    }
    assert!(
        !plan
            .notes
            .iter()
            .any(|n| matches!(n, Note::AimOutsideVolume(_)))
    );
}

#[test]
fn a_westward_course_is_derived_without_anything_noticing() {
    // The whole point of r27 in one test: a course that runs the opposite way to
    // coil produces a plan with the same shape, and its legs say so.
    let (_, plan) = plan_of(&westward_course());
    for leg in &plan.legs {
        assert!(
            (leg.bearing_deg.abs() - 180.0).abs() < 0.5,
            "every leg runs west, not north: {leg:?}"
        );
    }
    // Spawn -> start -> cp0 -> finish.
    assert_eq!(plan.legs.len(), 3);
    assert!(plan.legs.iter().all(|l| l.distance > 0.0));
}

#[test]
fn the_aim_is_a_player_standing_on_the_ground_under_the_volume() {
    let (_, plan) = plan_of(&westward_course());
    let profile = PhysicsProfile::cpm();
    for w in &plan.waypoints {
        let t = w.primary();
        assert_eq!(t.horizontal, Horizontal::BoundsCentre);
        match t.vertical {
            Vertical::Standing(z) => assert!(
                (z - 0.0).abs() < 0.01,
                "the floor is at z=0, not {z} — and note the volumes reach down to -32, \
                 so a rule reading `mins.z` would have said -32"
            ),
            other => panic!("expected the general rule, got {other:?}"),
        }
        assert!(
            (t.aim.z - (0.0 + profile.hull_half_extents().z - profile.hull_center_offset().z))
                .abs()
                < 0.2,
            "the aim is a player origin, so it sits a hull half-height above the floor"
        );
    }
}

/// The finish sits on a platform, so its floor is nowhere near its `mins.z`.
///
/// This is the case the prior art's `mins.z + 48` gets wrong by construction —
/// it is coil's geometry written down, and here it would aim 352 units below
/// the surface a player crosses on.
fn raised_finish_course() -> String {
    let mut world = box_brush([-192, -192, -80], [192, 1088, 0], "straf3/floor");
    // A platform 352 units up, at the far end.
    world.push_str(&box_brush(
        [-192, 1088, -80],
        [192, 1600, 352],
        "straf3/floor",
    ));

    let mut map = brush_entity("worldspawn", &[("message", "raised")], &[world]);
    map.push_str(&point_entity(
        "info_player_start",
        &[("origin", "0 -96 24"), ("angle", "90")],
    ));
    map.push_str(&timed_trigger(
        "target_startTimer",
        "t_start",
        &[box_brush([-160, 0, -48], [160, 32, 576], "common/trigger")],
    ));
    // Spans from far below the platform to far above it: `mins.z` is -48 and
    // the surface a player stands on inside it is 352.
    map.push_str(&timed_trigger(
        "target_stopTimer",
        "t_finish",
        &[box_brush(
            [-160, 1280, -48],
            [160, 1344, 1152],
            "common/trigger",
        )],
    ));
    map
}

#[test]
fn the_vertical_rule_finds_the_real_floor_and_not_the_volumes_bottom() {
    let (_, plan) = plan_of(&raised_finish_course());
    let finish = plan
        .waypoints
        .iter()
        .find(|w| w.step == Step::Finish)
        .expect("the fixture has a finish")
        .primary();
    match finish.vertical {
        Vertical::Standing(z) => assert!(
            (z - 352.0).abs() < 0.01,
            "the platform is at 352; got {z}. `mins.z + 48` would have said 0"
        ),
        other => panic!("expected the general rule, got {other:?}"),
    }
    assert!(finish.aim_inside);
    assert!(
        finish.aim.z > 352.0,
        "a player standing on the platform is above it, not in it"
    );
}

/// An L-shaped start line: two brushes meeting at a corner, so the centre of
/// their union is in the wall between them.
fn elbow_start_course() -> String {
    let mut world = box_brush([-1088, -192, -80], [192, 192, 0], "straf3/floor");
    world.push_str(&box_brush([-192, 192, -80], [192, 1600, 0], "straf3/floor"));

    let mut map = brush_entity("worldspawn", &[("message", "elbow")], &[world]);
    map.push_str(&point_entity(
        "info_player_start",
        &[("origin", "-960 0 24"), ("angle", "0")],
    ));
    // Two pieces: a long arm along -x and a short one up +y. The union's bounds
    // centre is out over the missing corner, where no piece is.
    map.push_str(&timed_trigger(
        "target_startTimer",
        "t_start",
        &[
            box_brush([-816, -160, -48], [-784, 160, 576], "common/trigger"),
            box_brush([-40, 1024, -48], [192, 1056, 576], "common/trigger"),
        ],
    ));
    map.push_str(&timed_trigger(
        "target_stopTimer",
        "t_finish",
        &[box_brush(
            [-160, 1344, -48],
            [160, 1408, 576],
            "common/trigger",
        )],
    ));
    map
}

#[test]
fn a_two_piece_volume_falls_back_to_its_largest_piece_and_says_so() {
    let (_, plan) = plan_of(&elbow_start_course());
    assert_eq!(plan.waypoints[0].step, Step::Start);
    let start = plan.waypoints[0].primary();
    assert_eq!(start.pieces, 2);
    match start.horizontal {
        Horizontal::LargestPiece(_) => {}
        Horizontal::BoundsCentre => {
            panic!("the bounds centre of an L is not inside it; the derivation should have noticed")
        }
    }
    assert!(
        start.aim_inside,
        "the fallback exists to put the aim back inside the volume"
    );
}

/// A finish volume hanging over a hole, with no floor under it at all.
fn void_finish_course() -> String {
    // Two floor slabs with a gap, and the finish over the gap.
    let mut world = box_brush([-192, -192, -80], [192, 448, 0], "straf3/floor");
    world.push_str(&box_brush(
        [-192, 1088, -80],
        [192, 1600, 0],
        "straf3/floor",
    ));

    let mut map = brush_entity("worldspawn", &[("message", "void")], &[world]);
    map.push_str(&point_entity(
        "info_player_start",
        &[("origin", "0 -96 24"), ("angle", "90")],
    ));
    map.push_str(&timed_trigger(
        "target_startTimer",
        "t_start",
        &[box_brush([-160, 0, -48], [160, 32, 576], "common/trigger")],
    ));
    map.push_str(&timed_trigger(
        "target_stopTimer",
        "t_finish",
        &[box_brush(
            [-160, 672, 48],
            [160, 736, 576],
            "common/trigger",
        )],
    ));
    map
}

#[test]
fn a_volume_over_nothing_falls_back_to_its_centre_rather_than_inventing_a_floor() {
    let (_, plan) = plan_of(&void_finish_course());
    let finish = plan
        .waypoints
        .iter()
        .find(|w| w.step == Step::Finish)
        .expect("the fixture has a finish")
        .primary();
    assert_eq!(finish.vertical, Vertical::VolumeCentre);
    assert!(
        finish.aim_inside,
        "the fallback still has to be a point in the box"
    );
}

#[test]
fn a_map_with_no_timing_is_reported_rather_than_planned() {
    let world = box_brush([-192, -192, -80], [192, 192, 0], "straf3/floor");
    let mut map = brush_entity("worldspawn", &[("message", "no course")], &[world]);
    map.push_str(&point_entity(
        "info_player_start",
        &[("origin", "0 0 24"), ("angle", "0")],
    ));
    let (_, plan) = plan_of(&map);

    assert!(plan.waypoints.is_empty());
    assert!(!plan.is_runnable());
    assert!(plan.notes.contains(&Note::NoStart));
    assert!(plan.notes.contains(&Note::NoFinish));
}

/// A course that **doubles back**: the second checkpoint sits behind the first
/// along the corridor, so the route is start → far → back → finish.
///
/// This is the shape a sort-the-triggers-by-position heuristic cannot survive.
/// Both first-party maps are monotone `+y` corridors, so on either of them
/// "declared order" and "sorted by y" are the same list and a derivation that
/// silently sorted would be indistinguishable from one that did not. Here they
/// differ, and the plan has to follow the map.
fn doubling_back_course() -> String {
    let mut source = box_brush([-192, -192, -80], [192, 1728, 0], "straf3/floor");
    source = brush_entity("worldspawn", &[("message", "doubling back")], &[source]);
    source.push_str(&point_entity(
        "info_player_start",
        &[("origin", "0 -96 24"), ("angle", "90")],
    ));
    source.push_str(&timed_trigger(
        "target_startTimer",
        "t_start",
        &[box_brush([-160, 0, -48], [160, 32, 576], "common/trigger")],
    ));
    // Declared first, and it is the FURTHEST of the two.
    source.push_str(&timed_trigger(
        "target_checkpoint",
        "t_cp1",
        &[box_brush(
            [-160, 1024, -48],
            [160, 1056, 576],
            "common/trigger",
        )],
    ));
    // Declared second, and it is nearer the start than the one before it.
    source.push_str(&timed_trigger(
        "target_checkpoint",
        "t_cp2",
        &[box_brush(
            [-160, 512, -48],
            [160, 544, 576],
            "common/trigger",
        )],
    ));
    source.push_str(&timed_trigger(
        "target_stopTimer",
        "t_finish",
        &[box_brush(
            [-160, 1536, -48],
            [160, 1568, 576],
            "common/trigger",
        )],
    ));
    source
}

#[test]
fn a_course_that_doubles_back_is_planned_in_the_declared_order_not_a_sorted_one() {
    let (map, plan) = plan_of(&doubling_back_course());

    let names: Vec<&str> = plan
        .waypoints
        .iter()
        .map(|w| w.primary().name.as_deref().unwrap_or("-"))
        .collect();
    assert_eq!(
        names,
        vec!["t_start", "t_cp1", "t_cp2", "t_finish"],
        "the map declares this order; sorting by any coordinate would give another"
    );

    // The plan is genuinely non-monotone: one leg turns back on the one before
    // it, so no unit direction can make every leg's projection increase. That
    // is the property a sort-by-axis heuristic cannot produce, and it is the
    // same `turn` column the printout shows a reader.
    assert!(
        plan.legs
            .iter()
            .any(|l| l.turn_deg.is_some_and(|t| t.abs() > 90.0)),
        "the fixture is supposed to double back: {:?}",
        plan.legs
    );

    // And it is the map's own geometry that says so, not this test's arithmetic.
    let cp0 = map
        .triggers
        .iter()
        .find(|t| t.kind == TriggerKind::Checkpoint(0))
        .expect("cp0");
    let cp1 = map
        .triggers
        .iter()
        .find(|t| t.kind == TriggerKind::Checkpoint(1))
        .expect("cp1");
    assert!(
        cp1.bounds.mins.y < cp0.bounds.mins.y,
        "checkpoint 1 is behind checkpoint 0, which is the whole point"
    );
}

/// The same doubling-back course, with `count` keys that imply the other order.
///
/// Both shipped maps declare `count` on their checkpoints and nothing reads it —
/// `straf3-map` takes six keys out of a `.map` and that is not one of them. On
/// those maps `count` and source order agree, so the disagreement has never
/// surfaced. A reversal is where they come apart, which makes this the fixture
/// that has to exist before anyone authors one.
fn contested_order_course() -> String {
    let mut source = box_brush([-192, -192, -80], [192, 1728, 0], "straf3/floor");
    source = brush_entity("worldspawn", &[("message", "contested")], &[source]);
    source.push_str(&point_entity(
        "info_player_start",
        &[("origin", "0 -96 24"), ("angle", "90")],
    ));
    source.push_str(&timed_trigger(
        "target_startTimer",
        "t_start",
        &[box_brush([-160, 0, -48], [160, 32, 576], "common/trigger")],
    ));
    // First in the file, so the compiler calls it checkpoint 0 — but its `count`
    // says it is the second one visited.
    source.push_str(&timed_trigger_with(
        "target_checkpoint",
        "t_cp_far",
        &[("count", "2")],
        &[box_brush(
            [-160, 1024, -48],
            [160, 1056, 576],
            "common/trigger",
        )],
    ));
    source.push_str(&timed_trigger_with(
        "target_checkpoint",
        "t_cp_near",
        &[("count", "1")],
        &[box_brush(
            [-160, 512, -48],
            [160, 544, 576],
            "common/trigger",
        )],
    ));
    source.push_str(&timed_trigger(
        "target_stopTimer",
        "t_finish",
        &[box_brush(
            [-160, 1536, -48],
            [160, 1568, 576],
            "common/trigger",
        )],
    ));
    source
}

#[test]
fn a_count_key_that_contradicts_source_order_is_reported_and_not_obeyed() {
    let (_, plan) = plan_of(&contested_order_course());

    assert!(
        plan.notes.contains(&Note::CheckpointCountIsNotRead(2)),
        "a `count` nothing reads is worth saying out loud: {:?}",
        plan.notes
    );
    assert!(
        plan.notes
            .contains(&Note::CheckpointCountContradictsSourceOrder(vec![1, 0])),
        "the counts put checkpoint 1 before checkpoint 0: {:?}",
        plan.notes
    );

    // Reported, not resolved. The compiled index is what the game reads, so the
    // plan follows it — preferring `count` here would make this crate the one
    // component in the tree that disagrees with the shipped compiler.
    let steps: Vec<Step> = plan.waypoints.iter().map(|w| w.step).collect();
    assert_eq!(
        steps,
        vec![
            Step::Start,
            Step::Checkpoint(0),
            Step::Checkpoint(1),
            Step::Finish
        ]
    );
    assert_eq!(
        plan.waypoints[1].primary().name.as_deref(),
        Some("t_cp_far")
    );
}

#[test]
fn a_map_whose_count_agrees_with_source_order_is_noted_but_not_contested() {
    // Which is both shipped maps' shape, and the reason the trap has never
    // sprung: the note fires, the contradiction does not.
    let source = contested_order_course()
        .replace("\"count\" \"2\"", "\"count\" \"9\"")
        .replace("\"count\" \"1\"", "\"count\" \"11\"");
    let (_, plan) = plan_of(&source);
    assert!(plan.notes.contains(&Note::CheckpointCountIsNotRead(2)));
    assert!(
        !plan
            .notes
            .iter()
            .any(|n| matches!(n, Note::CheckpointCountContradictsSourceOrder(_))),
        "9 then 11 is the same order the compiler assigned: {:?}",
        plan.notes
    );
}

#[test]
fn the_checkpoint_order_assumption_is_stated_on_every_map_that_depends_on_it() {
    let (_, plan) = plan_of(&doubling_back_course());
    assert!(
        plan.notes.contains(&Note::CheckpointOrderIsSourceOrder(2)),
        "two checkpoints means the route depends on an order nothing here checked"
    );
    assert!(
        plan.notes.contains(&Note::CheckpointsDoNotGateTheClock(2)),
        "crossing start then finish produces a time whatever the checkpoints say"
    );
}

#[test]
fn the_spawn_is_checked_rather_than_trusted() {
    let (_, plan) = plan_of(&westward_course());
    assert!(!plan.spawn.start_solid, "the fixture spawns in open air");
    let ground = plan.spawn.ground_z.expect("there is a floor under it");
    assert!(ground.abs() < 0.01, "the floor is at z=0, not {ground}");
    assert!(!plan.notes.contains(&Note::SpawnInSolid));
    assert!(!plan.notes.contains(&Note::NothingUnderSpawn));
}

#[test]
fn the_derivation_is_a_pure_function_of_the_map_and_the_profile() {
    // Same inputs, same plan — which is what lets a committed printout be
    // checked by re-running the command rather than trusted.
    let source = westward_course();
    let (_, a) = plan_of(&source);
    let (_, b) = plan_of(&source);
    assert_eq!(a, b);
}

#[test]
fn a_bearing_difference_wraps_the_short_way_round() {
    assert!((wrap180(s(190.0)) - s(-170.0)).abs() < 1e-4);
    assert!((wrap180(s(-190.0)) - s(170.0)).abs() < 1e-4);
    assert!((wrap180(s(0.0))).abs() < 1e-4);
}
