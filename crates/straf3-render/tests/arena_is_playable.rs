//! Does the simulation actually survive this arena?
//!
//! The unit tests in `arena.rs` check individual traces. This checks the thing
//! that matters to a player and that individual traces cannot: run the real
//! `step_in_place` against the real [`ARENA`] for thousands of commands and see
//! whether the player stays in the world, keeps their feet, and can build
//! speed. A collision implementation can pass every isolated trace and still
//! drop the player through the floor on the tick where two of them interact.
//!
//! No GPU here, on purpose: the arena is importable without one, and this file
//! is the proof of that as much as it is a physics test.

use straf3_render::arena::{ARENA, FLOOR_HALF, SPAWN, SPAWN_YAW};
use straf3_sim::num::{Scalar, Vec3, s};
use straf3_sim::{
    Buttons, PhysicsProfile, SimState, TickRate, UserCmd, ViewAngles, World, step_in_place,
};

/// The same scripted player the examples fly: forward on the ground, strafe in
/// the air, jump on landing, view sweeping steadily. The crudest strafejump
/// there is.
fn autopilot(state: &SimState, rate: TickRate, turn_rate: Scalar) -> UserCmd {
    let grounded = state.player.ground.is_grounded();
    UserCmd {
        duration_ms: rate.command_millis(),
        forward_move: if grounded { 127 } else { 0 },
        right_move: 127,
        up_move: 0,
        buttons: if grounded {
            Buttons::JUMP
        } else {
            Buttons::NONE
        },
        view: ViewAngles::from_degrees(
            s(0.0),
            SPAWN_YAW + turn_rate * (state.time_ms as Scalar / s(1000.0)),
            s(0.0),
        ),
    }
}

/// Every solid's bounding box, so "did the player end up inside geometry" is
/// answerable without asking the tracer that is under test.
fn inside_any_solid(p: Vec3) -> Option<&'static str> {
    let hit = |mins: Vec3, maxs: Vec3| {
        p.x > mins.x && p.x < maxs.x && p.y > mins.y && p.y < maxs.y && p.z > mins.z && p.z < maxs.z
    };
    ARENA
        .boxes
        .iter()
        .find(|b| hit(b.mins, b.maxs))
        .map(|b| b.label)
}

#[test]
fn a_scripted_player_survives_a_minute_of_strafejumping() {
    let rate = TickRate::DEFAULT;
    let profile = PhysicsProfile::cpm();
    let mut state = SimState::spawned_at(SPAWN, SPAWN_YAW);

    let mut top_speed = s(0.0);
    let mut landings = 0u32;
    let mut was_grounded = false;

    // 60 seconds at 125 Hz. Long enough to have crossed the arena several
    // times, hit every wall, and been on and off every ramp.
    for _ in 0..7_500 {
        let cmd = autopilot(&state, rate, s(75.0));
        step_in_place(&mut state, &cmd, &ARENA, &profile);

        let o = state.player.origin;
        assert!(
            o.is_finite(),
            "the player left the number line at t={} ms",
            state.time_ms
        );
        assert!(
            o.z > s(-64.0),
            "the player fell through the floor at t={} ms, origin {o:?}",
            state.time_ms
        );
        assert!(
            o.x.abs() < FLOOR_HALF + s(64.0) && o.y.abs() < FLOOR_HALF + s(64.0),
            "the player escaped the arena at t={} ms, origin {o:?}",
            state.time_ms
        );
        if let Some(label) = inside_any_solid(o) {
            panic!(
                "the player ended up inside {label} at t={} ms, origin {o:?}",
                state.time_ms
            );
        }

        top_speed = top_speed.max(state.player.velocity.truncate().length());
        let grounded = state.player.ground.is_grounded();
        if grounded && !was_grounded {
            landings += 1;
        }
        was_grounded = grounded;
    }

    // Shown with `--nocapture`: useful when tuning the arena, because "it
    // passed" and "it felt fast" are different questions.
    eprintln!(
        "60 s of autopilot: top speed {top_speed:.1} ups, {landings} landings, \
         finished at {:?}",
        state.player.origin
    );

    // The ground speed cap is 320; anything above it came from strafejumping,
    // which is the entire point of the arena existing.
    assert!(
        top_speed > s(400.0),
        "the arena never let the player build speed — top speed was {top_speed} ups"
    );
    // Counting *landings*, not grounded ticks: a bunny-hopping player spends
    // almost the whole minute in the air by design (a 45-unit jump against 800
    // gravity is ~470 ms of hang time against one tick of contact), so a tick
    // count would say nothing. What must not happen is the player landing once
    // and never touching anything again.
    assert!(
        landings > 30,
        "the player only landed {landings} times in a minute — the floor is not \
         catching them"
    );
}

#[test]
fn the_player_can_run_up_the_gentle_ramp_onto_the_platform() {
    // Walking east from the open floor west of the ramp, no jumping. If the
    // ramp is not walkable the player stops dead at its foot; if it is, they
    // climb 128 units and arrive on the platform behind it.
    let rate = TickRate::DEFAULT;
    let profile = PhysicsProfile::cpm();
    let mut state =
        SimState::spawned_at(straf3_sim::num::vec3(s(-900.0), s(0.0), s(24.125)), s(0.0));

    let mut highest = state.player.origin.z;
    for _ in 0..500 {
        let cmd = UserCmd {
            duration_ms: rate.command_millis(),
            forward_move: 127,
            right_move: 0,
            up_move: 0,
            buttons: Buttons::NONE,
            view: ViewAngles::from_degrees(s(0.0), s(0.0), s(0.0)),
        };
        step_in_place(&mut state, &cmd, &ARENA, &profile);
        highest = highest.max(state.player.origin.z);
    }
    // The platform's top is at 128, so the origin ends up around 152.
    assert!(
        highest > s(140.0),
        "walked east for four seconds and never got above z={highest} — the ramp \
         is not climbable, or it does not meet the platform"
    );
}

#[test]
fn the_steep_ramp_slides_and_the_gentle_one_does_not() {
    // The `GroundState` the simulation reports on *first contact* is the whole
    // reason the arena carries ramps on both sides of `min_walk_normal`. It has
    // to be first contact: a player who lands on the steep ramp slides off it
    // onto the floor within a second, and the floor would answer "Grounded".
    let profile = PhysicsProfile::cpm();
    let rate = TickRate::DEFAULT;

    let first_contact = |x: Scalar, y: Scalar, from: Scalar| {
        let mut state = SimState::spawned_at(straf3_sim::num::vec3(x, y, from), s(0.0));
        for _ in 0..400 {
            step_in_place(
                &mut state,
                &UserCmd::still(rate.command_millis()),
                &ARENA,
                &profile,
            );
            if state.player.ground.is_on_plane() {
                return state.player.ground;
            }
        }
        panic!("dropped at ({x}, {y}) from z={from} and never touched anything");
    };

    // Middle of the gentle ramp: surface at z=64, so start just above it.
    let gentle = first_contact(s(-512.0), s(0.0), s(140.0));
    assert!(
        gentle.is_grounded(),
        "the 26° ramp must be walkable, got {gentle:?}"
    );
    assert!(
        gentle.normal().unwrap().z < s(0.999),
        "landed on the ramp but got a flat normal {gentle:?} — the floor answered, \
         not the ramp"
    );

    // Middle of the steep ramp: surface at z=96, start just above it.
    let steep = first_contact(s(320.0), s(400.0), s(170.0));
    assert!(
        steep.is_on_plane(),
        "the steep ramp must at least be solid, got {steep:?}"
    );
    assert!(
        !steep.is_grounded(),
        "the 56° ramp must slide, not be stood on — got {steep:?}"
    );
}

#[test]
fn holding_forward_from_the_spawn_goes_somewhere() {
    // Criterion 1 in miniature: the operator opens the window and presses W.
    // The arena shipped with `crate` ending at exactly SPAWN.x, so this fixture
    // used to travel 49 units and stop dead — a trace-level test never caught
    // it because the only forward-facing trace in the suite pointed south, away
    // from the direction a spawn facing north actually walks.
    //
    // No jumping, no strafing, no view movement: this is the plainest input
    // there is, and if it does not work nothing else about the arena matters.
    let rate = TickRate::DEFAULT;
    let profile = PhysicsProfile::cpm();
    let mut state = SimState::spawned_at(SPAWN, SPAWN_YAW);

    for _ in 0..125 {
        let cmd = UserCmd {
            duration_ms: rate.command_millis(),
            forward_move: 127,
            right_move: 0,
            up_move: 0,
            buttons: Buttons::NONE,
            view: ViewAngles::from_degrees(s(0.0), SPAWN_YAW, s(0.0)),
        };
        step_in_place(&mut state, &cmd, &ARENA, &profile);
    }

    let travelled = (state.player.origin - SPAWN).truncate().length();
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
fn the_spawn_point_is_not_inside_anything() {
    assert!(
        inside_any_solid(SPAWN).is_none(),
        "the spawn point is inside a solid"
    );
    // And the simulation agrees, which is the check that actually matters.
    let hull = PhysicsProfile::cpm().hull(false);
    let t = ARENA.trace(&straf3_sim::world::Sweep {
        start: SPAWN,
        end: SPAWN,
        half_extents: hull.half_extents,
        center_offset: hull.center_offset,
    });
    assert!(!t.start_solid && !t.all_solid);
}
