//! The derivation, run against the maps this project actually ships.
//!
//! The unit tests in `src/course/tests.rs` invent their fixtures so that a rule
//! can be told from a coincidence. This file is the other half: it runs the
//! same derivation over `assets/maps/`, because a plan that only works on maps
//! written to suit it is not a plan.
//!
//! # What is asserted here, and what deliberately is not
//!
//! Only properties that must hold for *any* first-party map: it compiles, it
//! can be timed, every step of its course has an aim point inside its own
//! volume, and the general rule — not a fallback — produced every one of them.
//! No coordinate, distance or bearing from either map appears below. Those are
//! published in `results/`, where a number that moves is a diff rather than a
//! failing assertion nobody can interpret.
//!
//! Adding a map to `assets/maps/` adds it here: the list is read from the
//! directory, so a new first-party map is covered the day it lands rather than
//! the day someone remembers this file.

use std::path::PathBuf;

use straf3_agent::course::{CoursePlan, Note, Step, Vertical};
use straf3_sim::PhysicsProfile;

fn maps_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../assets/maps")
}

/// Every `.map` under `assets/maps/`, sorted, so the test order does not depend
/// on how the filesystem feels about enumerating a directory.
fn first_party_maps() -> Vec<PathBuf> {
    let mut out: Vec<PathBuf> = std::fs::read_dir(maps_dir())
        .expect("assets/maps must exist")
        .filter_map(Result::ok)
        .map(|e| e.path())
        .filter(|p| p.extension().is_some_and(|e| e == "map"))
        .collect();
    out.sort();
    assert!(!out.is_empty(), "no first-party maps found");
    out
}

fn plan_for(path: &PathBuf) -> CoursePlan {
    let source = std::fs::read_to_string(path).expect("read the map");
    let map = straf3_map::compile(&source)
        .unwrap_or_else(|e| panic!("{} does not compile: {e}", path.display()));
    assert!(
        map.has_timing(),
        "{} has no start/finish pair, so no course can be derived from it",
        path.display()
    );
    CoursePlan::derive(&map, &PhysicsProfile::cpm(), "cpm")
}

#[test]
fn every_first_party_map_yields_a_runnable_course() {
    for path in first_party_maps() {
        let plan = plan_for(&path);
        let name = path.display();
        assert!(plan.is_runnable(), "{name}: the plan reaches no finish");
        assert_eq!(
            plan.waypoints.first().map(|w| w.step),
            Some(Step::Start),
            "{name}: a course begins at the start line"
        );
        assert_eq!(
            plan.waypoints.last().map(|w| w.step),
            Some(Step::Finish),
            "{name}: and ends at the finish"
        );
        // Start, every checkpoint, finish — and nothing else.
        let checkpoints = plan
            .waypoints
            .iter()
            .filter(|w| matches!(w.step, Step::Checkpoint(_)))
            .count();
        assert_eq!(plan.waypoints.len(), checkpoints + 2, "{name}");
        assert_eq!(
            plan.legs.len(),
            plan.waypoints.len(),
            "{name}: spawn included"
        );
    }
}

#[test]
fn the_general_rule_alone_suffices_on_every_map_we_ship() {
    // The fallbacks exist and are tested, but a first-party map needing one is
    // worth knowing about: it means the volume is over a void, or is several
    // brushes whose union has a hollow middle. Neither is true today.
    for path in first_party_maps() {
        let plan = plan_for(&path);
        let name = path.display();
        for w in &plan.waypoints {
            for t in &w.targets {
                assert!(
                    t.aim_inside,
                    "{name}: {:?}'s aim point is outside its own volume",
                    w.step
                );
                assert!(
                    matches!(t.vertical, Vertical::Standing(_)),
                    "{name}: {:?} fell back to {:?}",
                    w.step,
                    t.vertical
                );
            }
        }
        assert!(
            !plan.notes.iter().any(|n| matches!(
                n,
                Note::AimOutsideVolume(_) | Note::NoStart | Note::NoFinish
            )),
            "{name}: {:?}",
            plan.notes
        );
    }
}

#[test]
fn no_first_party_map_spawns_the_player_in_solid() {
    for path in first_party_maps() {
        let plan = plan_for(&path);
        let name = path.display();
        assert!(
            !plan.spawn.start_solid,
            "{name}: the spawn is inside a brush"
        );
        assert!(
            plan.spawn.ground_z.is_some(),
            "{name}: nothing under the spawn"
        );
    }
}

#[test]
fn the_same_map_derives_the_same_plan_twice() {
    // What makes the committed printouts in `results/` checkable by re-running
    // the command instead of by trusting them.
    for path in first_party_maps() {
        assert_eq!(plan_for(&path), plan_for(&path), "{}", path.display());
    }
}
