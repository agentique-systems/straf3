//! What the fixture has to be true of *itself* before any run on it means
//! anything.
//!
//! These tests do not run the search. They check the map is the shape the module
//! docs claim — that the pits have no floor, that the seals seal, that the two
//! forks really are mirrored — because a demonstration on geometry that is not
//! what it says it is proves nothing, and the b5 predecessor of this file caught
//! four brushes compiling to `DegenerateBrush` from inverted windings that
//! reading the source had not.

use super::*;

use crate::course::{CoursePlan, Step};
use straf3_map::compile;
use straf3_sim::PhysicsProfile;
use straf3_sim::num::{Scalar, s, vec3};
use straf3_sim::world::{Sweep, World};

fn compiled() -> straf3_map::CompiledMap {
    compile(&wishbone()).expect("the generated fixture must compile")
}

/// The surface a player standing at `(x, y)` would come to rest on, or `None`
/// over a void.
fn surface_under(x: i32, y: i32) -> Option<Scalar> {
    let map = compiled();
    let world = map.collider();
    let profile = PhysicsProfile::cpm();
    // Below the ceiling by more than the hull's own height: starting a sweep
    // flush against a brush reports `start_solid` and finds nothing, which looks
    // exactly like a void.
    let from = vec3(x as Scalar, y as Scalar, s((CEILING - 96) as Scalar));
    let to = vec3(x as Scalar, y as Scalar, s(-512.0));
    let t = world.trace(&Sweep {
        start: from,
        end: to,
        half_extents: profile.hull_half_extents(),
        center_offset: profile.hull_center_offset(),
    });
    if t.start_solid || t.fraction >= s(1.0) {
        return None;
    }
    let origin = from + (to - from) * t.fraction;
    Some(origin.z + profile.hull_center_offset().z - profile.hull_half_extents().z)
}

#[test]
fn the_fixture_compiles_without_warnings() {
    let map = compiled();
    assert!(
        map.warnings.is_empty(),
        "a generated map with warnings is a generator defect: {:?}",
        map.warnings
    );
}

#[test]
fn it_is_a_timed_course_with_two_checkpoints() {
    let map = compiled();
    assert!(map.has_timing());
    let plan = CoursePlan::derive(&map, &PhysicsProfile::cpm(), "cpm");
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
}

#[test]
fn the_spawn_is_in_open_air_with_ground_under_it() {
    let map = compiled();
    let plan = CoursePlan::derive(&map, &PhysicsProfile::cpm(), "cpm");
    assert!(!plan.spawn.start_solid, "the spawn is inside a brush");
    assert_eq!(plan.spawn.ground_z, Some(s(FLOOR_TOP as Scalar)));
}

#[test]
fn every_aim_point_lands_inside_its_own_volume() {
    let map = compiled();
    let plan = CoursePlan::derive(&map, &PhysicsProfile::cpm(), "cpm");
    for w in &plan.waypoints {
        for t in &w.targets {
            assert!(t.aim_inside, "{:?}: aim outside its volume", w.step);
        }
    }
}

#[test]
fn the_two_forks_are_mirrored() {
    // b7's S3 in one assertion: a search that always turns the same way gets
    // exactly one of these right.
    let [f1, f2] = forks();
    assert_eq!(f1.trap, f2.trap.other());
    assert_ne!(f1.clear_x(), f2.clear_x());
    // And the mirroring is real in the geometry, not only in the description.
    assert_eq!(f1.divider_x().0, -f2.divider_x().1);
}

#[test]
fn the_divider_is_offset_onto_the_clear_side() {
    // The lane a player reaches by running straight ahead from the spawn is the
    // trap. If this ever inverts, the fixture stops posing a decision and starts
    // rewarding doing nothing.
    for f in forks() {
        let (lo, hi) = f.divider_x();
        let (tx0, tx1) = f.trap_x();
        let (cx0, cx1) = f.clear_x();
        assert!(
            (tx1 - tx0) > (cx1 - cx0),
            "{}: the trap lane must be the wider one",
            f.trap.label()
        );
        // The centre line falls in the trap lane.
        assert!(lo.signum() == hi.signum(), "the divider straddles no centre");
        assert!(
            (tx0..tx1).contains(&0),
            "{}: x = 0 must lie in the trap lane",
            f.trap.label()
        );
    }
}

#[test]
fn each_trap_lane_has_no_floor_and_each_clear_lane_does() {
    for f in forks() {
        let mid_y = (f.y.0 + f.y.1) / 2;
        let trap_x = (f.trap_x().0 + f.trap_x().1) / 2;
        let clear_x = (f.clear_x().0 + f.clear_x().1) / 2;
        assert_eq!(
            surface_under(trap_x, mid_y),
            Some(s(PIT_TOP as Scalar)),
            "{} trap lane should drop to the pit floor",
            f.trap.label()
        );
        assert_eq!(
            surface_under(clear_x, mid_y),
            Some(s(FLOOR_TOP as Scalar)),
            "{} clear lane should keep the running floor",
            f.trap.label()
        );
    }
}

#[test]
fn the_pit_is_deeper_than_a_jump_can_climb() {
    // b7's S6: a recoverable trap is not a trap. A standing jump's apex under
    // cpm is far short of this, and there is no ramp back out — the fall is a
    // drop, and the seals are vertical.
    let profile = PhysicsProfile::cpm();
    let apex = profile.jump_velocity * profile.jump_velocity / (s(2.0) * profile.gravity);
    let depth = s((FLOOR_TOP - PIT_TOP) as Scalar);
    assert!(
        depth > apex + profile.step_height,
        "pit depth {depth} must exceed a jump apex {apex} plus a step {}",
        profile.step_height
    );
}

#[test]
fn the_goal_after_each_fork_sits_over_the_lane_with_no_floor() {
    // The property that makes the trap the horizon-argmax rather than merely
    // present: the checkpoint is on the trap's side, so the trap lane is closer
    // to it throughout the approach.
    let map = compiled();
    let plan = CoursePlan::derive(&map, &PhysicsProfile::cpm(), "cpm");
    for (f, step) in forks().iter().zip([Step::Checkpoint(0), Step::Finish]) {
        let w = plan
            .waypoints
            .iter()
            .find(|w| w.step == step)
            .expect("the step must exist");
        let centre = (w.primary().bounds.mins.x + w.primary().bounds.maxs.x) * s(0.5);
        let trap_sign = f.trap.sign() as Scalar;
        assert!(
            centre * trap_sign > s(0.0),
            "{step:?} at x {centre} should be on the {} side",
            f.trap.label()
        );
    }
}

#[test]
fn the_blind_run_is_a_thousand_units_of_nothing_happening() {
    // Both branches of every claim about this map share it, so a difference
    // between two runs is a difference at a fork.
    assert_eq!(BLIND_RUN_END - START_Y.1, 1024);
    let mid = (START_Y.1 + BLIND_RUN_END) / 2;
    for x in [-HALF_WIDTH + 32, 0, HALF_WIDTH - 32] {
        assert_eq!(
            surface_under(x, mid),
            Some(s(FLOOR_TOP as Scalar)),
            "the blind run must be flat and open at x {x}"
        );
    }
}

#[test]
fn generating_it_twice_gives_the_same_bytes() {
    // What makes the committed copy checkable by re-running the generator.
    assert_eq!(wishbone(), wishbone());
}

#[test]
fn the_committed_copy_matches_the_generator() {
    // The fixture is evidence, so it is committed rather than generated at test
    // time — and that is only safe if drift is caught.
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join(PATH);
    let committed = std::fs::read_to_string(&path).unwrap_or_else(|e| {
        panic!(
            "{} is missing ({e}). Regenerate it with \
             `cargo run -p straf3-agent -- fixture`.",
            path.display()
        )
    });
    assert_eq!(
        committed,
        wishbone(),
        "{} has drifted from straf3_agent::fixture::wishbone. Regenerate it \
         with `cargo run -p straf3-agent -- fixture`.",
        path.display()
    );
}
