//! What the search has to be true of itself, independently of any map.
//!
//! The claims r9 rests on are behavioural — "it prefers an action that scores
//! worse at the horizon" — and behaviour is measured in `results/`, on the
//! fixture, by running it. What is testable here is the property that makes
//! those measurements admissible: that `frontier = 1` really is greedy
//! one-step-per-window, so b7's G1 control is this search with one number
//! changed rather than a second program.

use super::*;

use crate::course::CoursePlan;
use crate::fixture;
use straf3_map::compile;

fn plan_and_world() -> (Vec<Goal>, straf3_map::CompiledMap) {
    let map = compile(&fixture::wishbone()).expect("the fixture must compile");
    let plan = CoursePlan::derive(&map, &PhysicsProfile::cpm(), "cpm");
    (goals_of(&plan), map)
}

fn spec(frontier: usize) -> SearchSpec {
    SearchSpec {
        frontier,
        // Small budgets: these tests are about the search's shape, not about
        // whether it can finish a course.
        max_expansions: 400,
        max_ticks: 4_000_000,
        ..SearchSpec::default()
    }
}

fn run_with(frontier: usize) -> SearchResult {
    let (goals, map) = plan_and_world();
    let profile = PhysicsProfile::cpm();
    let world = map.collider();
    let start = SimState::spawned_at(map.spawn, map.spawn_yaw);
    run(
        &goals,
        &spec(frontier),
        TickRate::HZ_125,
        start,
        &world,
        &profile,
    )
}

#[test]
fn a_frontier_of_one_is_greedy_one_step_per_window() {
    // The load-bearing property of b7's G1. A greedy search commits to the
    // horizon-argmax at every decision, so its winning path contains no node
    // that was not the argmax — and therefore no deferrals at all.
    let r = run_with(1);
    assert_eq!(
        r.non_argmax_decisions, 0,
        "at frontier 1 every committed action must be the horizon-argmax; \
         found {} that were not",
        r.non_argmax_decisions
    );
    assert!(r.deferrals.is_empty());
}

#[test]
fn a_wider_frontier_actually_departs_from_the_argmax() {
    // The other half: the knob has to do something. If a wide frontier also
    // never left the argmax, the mechanism would be documented and absent.
    let r = run_with(256);
    assert!(
        r.non_argmax_decisions > 0,
        "a frontier of 256 committed to the horizon-argmax at every one of its \
         {} decisions, so the retention is not changing any outcome",
        r.path_len
    );
}

#[test]
fn the_reconstructed_stream_reproduces_the_search_s_own_state() {
    // The command stream is re-simulated from the spawn along the winning
    // controls rather than stitched from cached fragments, so this equality is
    // what says the published stream *is* the run the search found.
    for frontier in [1, 64] {
        let r = run_with(frontier);
        assert!(
            r.reconstruction_agrees,
            "frontier {frontier}: replaying the winning controls did not \
             reproduce the node's checksum"
        );
    }
}

#[test]
fn the_same_search_twice_gives_the_same_commands() {
    // Ties are broken by generation order and scores are compared with
    // `total_cmp`, so two runs of one configuration must agree exactly. Without
    // this, a published checksum would be a coincidence.
    let a = run_with(64);
    let b = run_with(64);
    assert_eq!(a.cmds, b.cmds);
    assert_eq!(a.checksum, b.checksum);
    assert_eq!(a.expansions, b.expansions);
}

#[test]
fn the_alphabet_decides_the_control_count_and_nothing_else_does() {
    let jump_only = controls(Alphabet { crouch: false });
    let with_crouch = controls(Alphabet { crouch: true });
    assert_eq!(with_crouch.len(), jump_only.len() * 2);
    // Assumption a6 made checkable: an agent without CROUCH in its alphabet
    // cannot press it however good its search is.
    assert!(jump_only.iter().all(|c| !c.crouch));
    assert!(with_crouch.iter().any(|c| c.crouch));
}

#[test]
fn goals_come_out_of_the_plan_in_the_plan_s_order() {
    let (goals, _) = plan_and_world();
    let steps: Vec<Step> = goals.iter().map(|g| g.step).collect();
    assert_eq!(
        steps,
        vec![
            Step::Start,
            Step::Checkpoint(0),
            Step::Checkpoint(1),
            Step::Finish
        ]
    );
    assert_eq!(goals[0].bits, TriggerSet::START);
    assert_eq!(goals[3].bits, TriggerSet::FINISH);
}

#[test]
fn a_budget_that_runs_out_says_so_rather_than_claiming_a_finish() {
    let (goals, map) = plan_and_world();
    let profile = PhysicsProfile::cpm();
    let world = map.collider();
    let start = SimState::spawned_at(map.spawn, map.spawn_yaw);
    let tiny = SearchSpec {
        frontier: 16,
        max_expansions: 3,
        ..SearchSpec::default()
    };
    let r = run(&goals, &tiny, TickRate::HZ_125, start, &world, &profile);
    assert_eq!(r.stop, Stop::ExpansionsExhausted);
    assert!(r.run_ms().is_none());
}
