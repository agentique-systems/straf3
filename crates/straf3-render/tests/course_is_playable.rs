//! Does the simulation actually survive the course we ship?
//!
//! This is the descendant of `arena_is_playable.rs`, retargeted when the
//! hardcoded arena was retired. The question it asks is unchanged and is the
//! one that matters to a player: run the real `step_in_place` against the real
//! compiled hulls for thousands of commands and see whether the player stays in
//! the world, keeps their feet, and can build speed. A collision implementation
//! can pass every isolated trace and still drop the player through the floor on
//! the tick where two of them interact.
//!
//! What changed is where the geometry comes from. It is no longer a Rust array
//! in this crate; it is `assets/maps/coil.map`, compiled by `straf3-map` — so
//! this file is now also the check that the *committed course* is playable,
//! which is the thing criterion 4 actually asks for. If somebody edits the
//! `.map` and breaks the route, this is what says so.
//!
//! No GPU here, on purpose: a compiled map is usable without one, and this file
//! is the proof of that as much as it is a physics test.
//!
//! # What this cannot tell you
//!
//! Whether the course is any *good*. These assertions are survival and
//! reachability bounds, not judgements about feel. The numbers they check
//! against were measured by `probes/coil-course`, and a bot that clears a gap
//! is not a player who enjoys clearing it.

use straf3_map::{CompiledMap, HullWorld, TriggerKind};
use straf3_sim::num::{Scalar, s, vec3};
use straf3_sim::{
    Buttons, PhysicsProfile, SimState, TickRate, UserCmd, ViewAngles, World, step_in_place,
};

/// The committed course, embedded so this test cannot be run against anything
/// else, and compiled once per test binary.
fn course() -> &'static (CompiledMap, HullWorld) {
    use std::sync::OnceLock;
    static C: OnceLock<(CompiledMap, HullWorld)> = OnceLock::new();
    C.get_or_init(|| {
        let map = straf3_map::compile(include_str!("../../../assets/maps/coil.map"))
            .expect("the committed course must compile");
        let world = map.collider();
        (map, world)
    })
}

fn map() -> &'static CompiledMap {
    &course().0
}
fn world() -> &'static HullWorld {
    &course().1
}

/// One greedy strafe-jumping decision: try a handful of (yaw rate, strafe,
/// forward) controls and keep the one whose simulated future gets furthest down
/// the course, with speed breaking ties.
///
/// # Why not the examples' constant-turn autopilot
///
/// Because it cannot strafejump down a corridor. Sweeping the view at a fixed
/// rate with one strafe key held is a circle, and a circle works in an open
/// square — which is exactly what the retired arena was. Run the same autopilot
/// on `coil` and it spirals into the start room's wall at 208 ups, below the
/// 320 ground cap, having demonstrated nothing about the course at all.
///
/// A player picks their yaw to keep the velocity vector just outside the
/// acceleration cone, and re-picks it every few ticks. This is that, done
/// greedily and badly: one-window hill climbing, no plan. Every speed it reaches
/// is therefore a LOWER bound on what a human can do, which is the direction a
/// course-validation test wants to be wrong in. `probes/coil-course` runs a
/// wider version of the same search and peaks at 783 ups.
#[derive(Clone, Copy)]
struct Control {
    yaw_rate: Scalar,
    right: i8,
    forward: i8,
}

const YAW_RATES: [f32; 9] = [-9.0, -6.0, -4.0, -2.0, 0.0, 2.0, 4.0, 6.0, 9.0];

fn controls() -> Vec<Control> {
    let mut out = Vec::new();
    for &yaw_rate in &YAW_RATES {
        for right in [-127i8, 0, 127] {
            for forward in [0i8, 127] {
                out.push(Control {
                    yaw_rate: s(yaw_rate),
                    right,
                    forward,
                });
            }
        }
    }
    out
}

/// Hold one control for `ticks` and return the resulting state.
fn hold(state: &SimState, c: Control, ticks: u32, profile: &PhysicsProfile) -> SimState {
    let rate = TickRate::DEFAULT;
    let mut st = *state;
    for _ in 0..ticks {
        // Bunny hopping: Q3 edge-triggers jump, so the button is pressed only
        // when there is ground to leave.
        let grounded = st.player.ground.is_grounded();
        let cmd = UserCmd {
            duration_ms: rate.command_millis(),
            forward_move: c.forward,
            right_move: c.right,
            up_move: 0,
            buttons: if grounded {
                Buttons::JUMP
            } else {
                Buttons::NONE
            },
            // `view.yaw` is a 16-bit angle since C3, so the turn is applied in
            // degrees and re-quantised — which is exactly what a real command
            // stream does to it.
            view: ViewAngles::from_degrees(
                s(0.0),
                st.player.view.yaw_degrees() + c.yaw_rate,
                s(0.0),
            ),
        };
        step_in_place(&mut st, &cmd, world(), profile);
    }
    st
}

/// Drop a still player from `from` and report the [`GroundState`] of whatever
/// they land on *first*.
///
/// It has to be first contact: a player who lands on a sliding surface leaves it
/// within a second and ends up on the floor, and the floor would answer
/// "Grounded" for a surface that is nothing of the kind.
fn first_contact(x: Scalar, y: Scalar, from: Scalar) -> straf3_sim::GroundState {
    let profile = PhysicsProfile::cpm();
    let rate = TickRate::DEFAULT;
    let mut state = SimState::spawned_at(vec3(x, y, from), s(0.0));
    for _ in 0..600 {
        step_in_place(
            &mut state,
            &UserCmd::still(rate.command_millis()),
            world(),
            &profile,
        );
        if state.player.ground.is_on_plane() {
            return state.player.ground;
        }
    }
    panic!("dropped at ({x}, {y}) from z={from} and never touched anything");
}

#[test]
fn the_course_compiles_to_a_timeable_run() {
    let m = map();
    // The three things that make a `.map` a *course* rather than a room.
    assert!(
        !m.hulls.is_empty(),
        "the course compiled to no solids at all"
    );
    let kinds: Vec<TriggerKind> = m.triggers.iter().map(|t| t.kind).collect();
    assert!(
        kinds.contains(&TriggerKind::Start),
        "no target_startTimer — this map cannot start a clock: {kinds:?}"
    );
    assert!(
        kinds.contains(&TriggerKind::Finish),
        "no target_stopTimer — this map cannot stop a clock: {kinds:?}"
    );
    assert!(
        kinds
            .iter()
            .any(|k| matches!(k, TriggerKind::Checkpoint(_))),
        "no checkpoints, so a run has no splits: {kinds:?}"
    );

    // A dropped patch is missing *collision*, so a route over a curved surface
    // would have a hole in it exactly where the surface was. Our own course is
    // all brushes, so this must be zero — unlike a third-party map, where the
    // counts run to four figures.
    let dropped: usize = m
        .warnings
        .iter()
        .filter_map(|w| match w {
            straf3_map::Warning::PatchDropped { count, .. } => Some(*count),
            _ => None,
        })
        .sum();
    assert_eq!(
        dropped, 0,
        "the committed course lost {dropped} curved surfaces — it should contain none"
    );

    eprintln!(
        "coil: {} hulls, {} triggers, {} triangles, collision digest {:#018x}",
        m.hulls.len(),
        m.triggers.len(),
        m.mesh.triangle_count(),
        m.collision_digest(),
    );
}

#[test]
fn the_spawn_point_is_not_inside_anything() {
    let m = map();
    let hull = PhysicsProfile::cpm().hull(false);
    let t = world().trace(&straf3_sim::world::Sweep {
        start: m.spawn,
        end: m.spawn,
        half_extents: hull.half_extents,
        center_offset: hull.center_offset,
    });
    assert!(
        !t.start_solid && !t.all_solid,
        "the spawn at {:?} is inside a solid — start_solid={} all_solid={}. \
         The compiler lifts a spawn clear of the floor by SPAWN_CLEARANCE; if \
         this fires, either that did not happen or the spawn is buried in a wall.",
        m.spawn,
        t.start_solid,
        t.all_solid
    );
}

#[test]
fn holding_forward_from_the_spawn_goes_somewhere() {
    // Criterion 1 in miniature: the operator opens the window and presses W.
    // No jumping, no strafing, no view movement — the plainest input there is,
    // and if it does not work nothing else about the course matters. The
    // failure this guards against is a spawn that faces into geometry, which a
    // trace-level test cannot see because it never asks which way the player
    // is pointing.
    let m = map();
    let rate = TickRate::DEFAULT;
    let profile = PhysicsProfile::cpm();
    let mut state = SimState::spawned_at(m.spawn, m.spawn_yaw);

    for _ in 0..125 {
        let cmd = UserCmd {
            duration_ms: rate.command_millis(),
            forward_move: 127,
            right_move: 0,
            up_move: 0,
            buttons: Buttons::NONE,
            view: ViewAngles::from_degrees(s(0.0), m.spawn_yaw, s(0.0)),
        };
        step_in_place(&mut state, &cmd, world(), &profile);
    }

    let travelled = (state.player.origin - m.spawn).truncate().length();
    let speed = state.player.velocity.truncate().length();
    eprintln!(
        "one second of forward: {travelled:.1} units, {speed:.1} ups, ending at {:?}",
        state.player.origin
    );

    // A second of ground running against the 320 ups cap covers ~300 units
    // after the acceleration ramp. Anything much under that means the player
    // met something.
    assert!(
        travelled > s(280.0),
        "a second of holding forward moved the player {travelled} units — \
         the spawn is facing into geometry"
    );
    // Travelled-but-stopped is the other failure shape: sliding along a wall
    // covers ground while going nowhere the player asked to go.
    assert!(
        speed > s(280.0),
        "the player travelled {travelled} units but is down to {speed} ups — \
         they are scraping along something"
    );
}

#[test]
fn a_strafejumping_player_can_run_the_course_end_to_end() {
    // The question the retired `arena_is_playable` asked, on the geometry we
    // actually ship: does a player who can strafejump survive this course, keep
    // their feet, build speed, and get from the spawn to the finish? A collision
    // implementation can pass every isolated trace and still drop the player
    // through the floor on the tick where two of them interact.
    let m = map();
    let profile = PhysicsProfile::cpm();
    let mut state = SimState::spawned_at(m.spawn, m.spawn_yaw);

    let finish_y = m
        .triggers
        .iter()
        .find(|t| t.kind == TriggerKind::Finish)
        .expect("a finish volume")
        .bounds
        .mins
        .y;

    // Generous margins on the compiled bounds: the player may legitimately be
    // above the geometry or just outside a wall, but not in the next postcode.
    let lo = m.bounds.mins - vec3(s(512.0), s(512.0), s(512.0));
    let hi = m.bounds.maxs + vec3(s(512.0), s(512.0), s(1024.0));

    let all = controls();
    const LOOKAHEAD: u32 = 8;
    const DECIDE_EVERY: u32 = 4;
    const MAX_TICKS: u32 = 6_000; // 48 s at 125 Hz.

    let mut top_speed = s(0.0);
    let mut landings = 0u32;
    let mut was_grounded = false;
    let mut ticks = 0u32;

    while ticks < MAX_TICKS && state.player.origin.y < finish_y {
        let mut best = None;
        let mut best_score = Scalar::NEG_INFINITY;
        for &c in &all {
            let f = hold(&state, c, LOOKAHEAD, &profile);
            if !f.player.origin.is_finite() {
                continue;
            }
            let speed = f.player.velocity.truncate().length();
            // Progress along the course dominates; speed breaks ties, which is
            // what stops the bot trading its whole run for one long slide.
            let score = f.player.origin.y + s(0.25) * speed;
            if score > best_score {
                best_score = score;
                best = Some(c);
            }
        }
        let Some(c) = best else {
            panic!(
                "every control led somewhere non-finite at t={} ms, origin {:?}",
                state.time_ms, state.player.origin
            )
        };

        // Step the chosen control one tick at a time so the invariants are
        // checked on every tick, not once per decision window.
        for _ in 0..DECIDE_EVERY {
            state = hold(&state, c, 1, &profile);
            let o = state.player.origin;
            assert!(
                o.is_finite(),
                "the player left the number line at t={} ms",
                state.time_ms
            );
            assert!(
                o.x > lo.x && o.x < hi.x && o.y > lo.y && o.y < hi.y && o.z > lo.z && o.z < hi.z,
                "the player escaped the course at t={} ms, origin {o:?}, bounds {:?}..{:?}",
                state.time_ms,
                m.bounds.mins,
                m.bounds.maxs
            );
            top_speed = top_speed.max(state.player.velocity.truncate().length());
            let grounded = state.player.ground.is_grounded();
            if grounded && !was_grounded {
                landings += 1;
            }
            was_grounded = grounded;
        }
        ticks += DECIDE_EVERY;
    }

    eprintln!(
        "greedy bot: reached y={:.0} (finish at {finish_y:.0}) in {} ms, \
         top speed {top_speed:.1} ups, {landings} landings, ending at {:?}",
        state.player.origin.y, state.time_ms, state.player.origin
    );

    // It has to actually get there. A course whose route is blocked fails here
    // and nowhere else — every trace-level test would still pass.
    assert!(
        state.player.origin.y >= finish_y,
        "the bot only reached y={:.0} of {finish_y:.0} in {MAX_TICKS} ticks — the \
         route is blocked, or a gap is not clearable",
        state.player.origin.y
    );
    // The ground speed cap is 320; anything above it came from strafejumping,
    // which is the entire point of the course existing. The probe's wider search
    // peaks at 783 ups, so this bar is deliberately well under what is possible.
    assert!(
        top_speed > s(450.0),
        "the course never let the player build speed — top speed was {top_speed} ups"
    );
    // Counting *landings*, not grounded ticks: a bunny-hopping player spends
    // almost the whole run in the air by design, so a tick count would say
    // nothing. What must not happen is the player landing once and never
    // touching anything again.
    assert!(
        landings > 10,
        "the player only landed {landings} times crossing the whole course — the \
         floor is not catching them"
    );
}

#[test]
fn the_centre_line_of_the_route_is_solid_all_the_way() {
    // The failure this catches is a hole in the route: a y at which a player
    // running down the middle of the course meets nothing and falls out of the
    // world. The probe surveyed exactly this and found zero voids; this is that
    // survey as an assertion, so an edit to the `.map` cannot reintroduce one
    // silently.
    let profile = PhysicsProfile::cpm();
    let hull = profile.hull(false);
    let mut voids = Vec::new();

    let mut y = -800.0f32;
    while y <= 3968.0 {
        // Straight down from well above anything, at the course's centre line.
        let t = world().trace(&straf3_sim::world::Sweep {
            start: vec3(s(0.0), s(y), s(1024.0)),
            end: vec3(s(0.0), s(y), s(-512.0)),
            half_extents: hull.half_extents,
            center_offset: hull.center_offset,
        });
        if t.fraction >= s(1.0) {
            voids.push(y);
        }
        y += 32.0;
    }

    assert!(
        voids.is_empty(),
        "there is nothing under the centre line at y={voids:?} — a player running \
         the route falls out of the world there"
    );
}

#[test]
fn the_course_carries_surfaces_on_both_sides_of_min_walk_normal() {
    // The `GroundState` the simulation reports is the whole reason the course
    // carries ramps at several angles: a course made only of walkable floor
    // exercises one branch of the ground check and calls it tested. These are
    // the surfaces `probes/coil-course` measured, with the normals it measured.
    let profile = PhysicsProfile::cpm();
    let min_walk = profile.min_walk_normal;

    // The gentle ramp: comfortably walkable.
    let gentle = first_contact(s(0.0), s(1600.0), s(512.0));
    assert!(
        gentle.is_grounded(),
        "the gentle ramp must be walkable, got {gentle:?}"
    );

    // The slide in the gully: steeper than min_walk_normal, so it must NOT be
    // stood on. This is the branch a floor-only course never reaches.
    let slide = first_contact(s(0.0), s(2400.0), s(512.0));
    assert!(
        slide.is_on_plane(),
        "the gully slide must at least be solid, got {slide:?}"
    );
    assert!(
        !slide.is_grounded(),
        "the gully slide must slide, not be stood on — got {slide:?} against a \
         min_walk_normal of {min_walk}"
    );

    eprintln!(
        "gentle normal.z={:?}  slide normal.z={:?}  min_walk_normal={min_walk}",
        gentle.normal().map(|n| n.z),
        slide.normal().map(|n| n.z),
    );
}

#[test]
fn the_start_and_finish_volumes_are_where_a_run_would_cross_them() {
    // A start volume behind the spawn, or a finish volume the route never
    // reaches, makes the course untimeable in a way no compile check sees.
    let m = map();
    let start = m
        .triggers
        .iter()
        .find(|t| t.kind == TriggerKind::Start)
        .expect("a start volume");
    let finish = m
        .triggers
        .iter()
        .find(|t| t.kind == TriggerKind::Finish)
        .expect("a finish volume");

    assert!(
        start.bounds.mins.y > m.spawn.y,
        "the start volume at y={} is behind the spawn at y={} — the clock would \
         start before the player has run anywhere",
        start.bounds.mins.y,
        m.spawn.y
    );
    assert!(
        finish.bounds.mins.y > start.bounds.maxs.y,
        "the finish volume at y={} is not beyond the start volume at y={}",
        finish.bounds.mins.y,
        start.bounds.maxs.y
    );

    // And the checkpoints are between them, in source order.
    let mut last_y = start.bounds.maxs.y;
    for t in m.triggers.iter() {
        if let TriggerKind::Checkpoint(i) = t.kind {
            assert!(
                t.bounds.mins.y > last_y,
                "checkpoint {i} at y={} is not after the previous gate at y={last_y} — \
                 source order and route order disagree",
                t.bounds.mins.y
            );
            last_y = t.bounds.maxs.y;
        }
    }
    assert!(
        finish.bounds.mins.y > last_y,
        "the finish is not after the last checkpoint"
    );
}

/// A player who lands short of the finish ledge must be able to get back, not
/// be stuck in a hole for the rest of the run.
#[test]
fn overshooting_the_finish_lands_on_floor_rather_than_in_the_void() {
    // The probe measured the finish jump clearing the ledge at ~900 ups and
    // overshooting past it above that. The pit floor was extended to y=5376 for
    // exactly this reason, and this is the assertion that keeps it there.
    let profile = PhysicsProfile::cpm();
    let hull = profile.hull(false);
    for y in [4000.0f32, 4400.0, 4800.0, 5200.0] {
        let t = world().trace(&straf3_sim::world::Sweep {
            start: vec3(s(0.0), s(y), s(512.0)),
            end: vec3(s(0.0), s(y), s(-512.0)),
            half_extents: hull.half_extents,
            center_offset: hull.center_offset,
        });
        assert!(
            t.fraction < s(1.0),
            "an overshoot past the finish falls forever at y={y} — the run-out \
             floor does not reach that far"
        );
    }
}
