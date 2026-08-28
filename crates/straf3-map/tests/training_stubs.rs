//! Are the training stubs in `assets/maps/` playable?
//!
//! The descendant of `straf3-render`'s `course_is_playable.rs`, asking the same
//! question of the training maps rather than of the course, and living in this
//! crate rather than that one because a training stub needs no GPU and no
//! renderer — it needs the compiler and the mover, which are both below the
//! line. A cold build here is `straf3-collision` and `straf3-sim`, not `wgpu`.
//!
//! A training stub isolates one movement primitive (VISION §6). What this file
//! checks is that the geometry really presents that primitive: not "the brushes
//! closed" but "the ceiling admits a crouched player and refuses a standing
//! one, at the y where the map says it does".
//!
//! # What this cannot tell you
//!
//! Whether the primitive is any good, or whether it composes inside a real
//! course — VISION §7's harder question, which no stub answers. These are
//! reachability and geometry bounds only.

use straf3_map::{CompiledMap, HullWorld, TriggerKind};
use straf3_sim::num::{Scalar, s, vec3};
use straf3_sim::world::Sweep;
use straf3_sim::{
    Buttons, PhysicsProfile, SimState, TickRate, UserCmd, ViewAngles, World, step_in_place,
};

const CROUCH_SLIDE: &str = include_str!("../../../assets/maps/training-crouch-slide.map");

fn compiled(source: &str) -> (CompiledMap, HullWorld) {
    let map = straf3_map::compile(source).expect("a committed training stub must compile");
    let world = map.collider();
    (map, world)
}

/// Is the hull with this `maxs.z` clear of solids at `origin`?
///
/// The same probe `testbed.rs`'s `ceiling_at` test uses, and deliberately built
/// from `PhysicsProfile::hull` rather than from hand-written extents: the
/// question is whether *the player's box* fits, so it has to be the player's
/// box.
fn hull_fits(world: &HullWorld, origin: straf3_sim::num::Vec3, crouched: bool) -> bool {
    let hull = PhysicsProfile::experimental().hull(crouched);
    !world
        .trace(&Sweep {
            start: origin,
            end: origin,
            half_extents: hull.half_extents,
            center_offset: hull.center_offset,
        })
        .start_solid
}

#[test]
fn the_crouch_slide_stub_compiles_to_a_timeable_run() {
    let (m, _) = compiled(CROUCH_SLIDE);
    assert!(!m.hulls.is_empty(), "the stub compiled to no solids at all");

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
        kinds.iter().any(|k| matches!(k, TriggerKind::Checkpoint(_))),
        "no checkpoints, so a run has no splits: {kinds:?}"
    );

    // A warning that means broken geometry, as opposed to one that is merely
    // informational. Every one of these would be a hole in the route.
    for w in &m.warnings {
        match w {
            straf3_map::Warning::DegenerateBrush { entity, brush }
            | straf3_map::Warning::UnboundedBrush { entity, brush } => {
                panic!("brush {brush} of entity {entity} did not close: {w:?}")
            }
            straf3_map::Warning::PatchDropped { count, .. } => {
                panic!("{count} curved surfaces dropped — this stub is all brushes")
            }
            straf3_map::Warning::UnresolvedTarget { .. }
            | straf3_map::Warning::NoTimerTriggers
            | straf3_map::Warning::TooManyCheckpoints { .. } => {
                panic!("the entity wiring is wrong: {w:?}")
            }
            _ => {}
        }
    }

    eprintln!(
        "training-crouch-slide: {} hulls, {} triggers, {} triangles, digest {:#018x}",
        m.hulls.len(),
        m.triggers.len(),
        m.mesh.triangle_count(),
        m.collision_digest(),
    );
}

#[test]
fn the_crouch_slide_spawn_is_not_inside_anything() {
    let (m, world) = compiled(CROUCH_SLIDE);
    assert!(
        hull_fits(&world, m.spawn, false),
        "the spawn at {:?} is inside a solid",
        m.spawn
    );
    assert_eq!(m.spawn_yaw, 90.0, "the spawn must face +Y down the corridor");
}

/// The whole point of the map: a ceiling in the open band between the crouched
/// and standing hull heights.
///
/// A player resting on a floor at z=0 has their origin at 24.125 —
/// `-hull_mins.z` plus `SURFACE_CLIP_EPSILON` — so the standing head is at
/// 56.125 and the crouched head at 40.125. The lintel's underside is at 48,
/// which is inside that band and inside nothing else.
#[test]
fn the_lintel_admits_a_crouched_player_and_refuses_a_standing_one() {
    let (_, world) = compiled(CROUCH_SLIDE);
    let at = |y: f32| vec3(s(0.0), s(y), s(24.125));

    // Under the flat lintel, y 2304..2496.
    for y in [2320.0f32, 2400.0, 2480.0] {
        assert!(
            hull_fits(&world, at(y), true),
            "the crouched hull must fit under the lintel at y={y}"
        );
        assert!(
            !hull_fits(&world, at(y), false),
            "the standing hull must NOT fit under the lintel at y={y} — \
             this map has no reason to exist if it does"
        );
    }

    // On the entry pad and the exit run, both hulls fit: this is the open
    // ground where the tap-and-stand line is available. 2240 is the far end of
    // the entry pad, which the map's S2 promises is "192 flat units at full
    // height" — so a standing player has to fit on the last of them.
    for y in [2100.0f32, 2200.0, 2240.0, 2600.0, 3000.0, 3200.0] {
        assert!(
            hull_fits(&world, at(y), false),
            "the standing hull must fit in the open at y={y}"
        );
    }

    // Where the soffit starts refusing a standing player.
    //
    // The map header's y≈2293.2 is where the sloped *plane* passes below the
    // standing head height: its underside runs (y=2240, z=96) to (y=2304,
    // z=48), so it reaches 56.125 at 2240 + (96 − 56.125) / 0.75. But a player
    // is a box, not an eye. `hull_fits` traces that box, the soffit descends
    // with y, and so what meets the slope first is the box's leading top
    // corner — `half_extents.y` ahead of the origin. The last standing-clear
    // ORIGIN is a hull half-length earlier than the plane crossing, and the
    // two numbers are not interchangeable.
    const SOFFIT_MEETS_STANDING_HEAD: f32 = 2240.0 + (96.0 - 56.125) / 0.75;
    let lead = PhysicsProfile::experimental().hull(false).half_extents.y;
    let last_clear = SOFFIT_MEETS_STANDING_HEAD - lead;

    // Pinned on both sides, so the entry pad can neither shrink nor grow
    // silently: a shallower lead-in would let a standing player further in, a
    // steeper or lower one would cut the pad short.
    assert!(
        hull_fits(&world, at(last_clear - 0.5), false),
        "the soffit refuses a standing player before y={last_clear} — the entry pad is short"
    );
    assert!(
        !hull_fits(&world, at(last_clear + 0.5), false),
        "the soffit still admits a standing player past y={last_clear} — the lead-in is \
         too shallow, and the lintel's refusal starts later than the map says"
    );
}

/// No wall-jump surface anywhere the route passes through the low section.
///
/// The corridor walls are vertical and therefore are wall-jump surfaces, which
/// the map's header says outright. What must not happen is the *ceiling*
/// becoming one: a vertical lintel face would put a second candidate mechanic
/// inside a stub built to isolate one. The soffit exists to prevent that.
#[test]
fn the_soffit_is_a_ceiling_and_not_a_wall() {
    let (m, _) = compiled(CROUCH_SLIDE);
    let max = PhysicsProfile::experimental().wall_normal_max;

    // Every plane of every hull whose y-extent lies inside the soffit's run and
    // whose normal has a +z or -z lean must be clear of the wall threshold.
    let mut soffit_planes = 0;
    for hull in &m.hulls {
        if hull.mins.y < 2240.0 || hull.maxs.y > 2304.0 || hull.maxs.z <= 48.0 {
            continue;
        }
        for p in &hull.planes {
            // The sloped underside: the only plane here with a normal that is
            // neither axis-aligned nor vertical.
            if p.normal.z < s(-0.5) && p.normal.y != s(0.0) {
                soffit_planes += 1;
                assert!(
                    p.normal.z.abs() > max,
                    "the soffit plane {:?} is a wall-jump surface (|z| <= {max})",
                    p.normal
                );
            }
        }
    }
    assert!(
        soffit_planes > 0,
        "no sloped soffit plane found — the lintel's lead-in is missing or flat"
    );
}

/// A hole in the route is a player falling out of the world.
#[test]
fn the_centre_line_is_solid_all_the_way() {
    let (_, world) = compiled(CROUCH_SLIDE);
    let hull = PhysicsProfile::experimental().hull(true);
    let mut voids = Vec::new();

    let mut y = -750.0f32;
    while y <= 3500.0 {
        let t = world.trace(&Sweep {
            // From just under the lintel, so the trace is not stopped by the
            // ceiling on its way down.
            start: vec3(s(0.0), s(y), s(40.0)),
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
        "nothing under the centre line at y={voids:?} — the player falls out there"
    );
}

/// The plainest input there is: open the window and hold W.
#[test]
fn holding_forward_from_the_spawn_goes_somewhere() {
    let (m, world) = compiled(CROUCH_SLIDE);
    let rate = TickRate::DEFAULT;
    let profile = PhysicsProfile::experimental();
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
        step_in_place(&mut state, &cmd, &world, &profile);
    }

    let travelled = (state.player.origin - m.spawn).truncate().length();
    eprintln!(
        "one second of forward: {travelled:.1} units, ending at {:?}",
        state.player.origin
    );
    assert!(
        travelled > s(280.0),
        "a second of holding forward moved the player {travelled} units — \
         the spawn faces into geometry"
    );
}

/// Can a strafejumping player actually reach `slide_entry_speed` inside the
/// run-up this map gives them, and does a slide entered at the pad carry them
/// through the lintel?
///
/// A greedy one-window hill climb, the same shape as
/// `course_is_playable.rs`'s bot and for the same reason: every speed it
/// reaches is a LOWER bound on what a human can do, which is the direction a
/// map-validation test wants to be wrong in. The control set carries crouch,
/// because without it the bot cannot pass the lintel at all.
#[test]
fn a_strafejumping_player_can_arm_a_slide_and_clear_the_lintel() {
    let (m, world) = compiled(CROUCH_SLIDE);
    let profile = PhysicsProfile::experimental();
    let rate = TickRate::DEFAULT;
    let mut state = SimState::spawned_at(m.spawn, m.spawn_yaw);

    #[derive(Clone, Copy)]
    struct Control {
        yaw_rate: Scalar,
        right: i8,
        forward: i8,
        crouch: bool,
    }
    let mut all = Vec::new();
    for &yaw_rate in &[-9.0f32, -6.0, -4.0, -2.0, 0.0, 2.0, 4.0, 6.0, 9.0] {
        for right in [-127i8, 0, 127] {
            for forward in [0i8, 127] {
                for crouch in [false, true] {
                    all.push(Control {
                        yaw_rate: s(yaw_rate),
                        right,
                        forward,
                        crouch,
                    });
                }
            }
        }
    }

    let hold = |state: &SimState, c: Control, ticks: u32| -> SimState {
        let mut st = *state;
        for _ in 0..ticks {
            let grounded = st.player.ground.is_grounded();
            let cmd = UserCmd {
                duration_ms: rate.command_millis(),
                forward_move: c.forward,
                right_move: c.right,
                // Q3 spells crouch as `up_move < 0`.
                up_move: if c.crouch { -127 } else { 0 },
                // Bunny hopping: jump only where there is ground to leave, and
                // never while asking to crouch.
                buttons: if grounded && !c.crouch {
                    Buttons::JUMP
                } else {
                    Buttons::NONE
                },
                view: ViewAngles::from_degrees(
                    s(0.0),
                    st.player.view.yaw_degrees() + c.yaw_rate,
                    s(0.0),
                ),
            };
            step_in_place(&mut st, &cmd, &world, &profile);
        }
        st
    };

    let finish_y = m
        .triggers
        .iter()
        .find(|t| t.kind == TriggerKind::Finish)
        .expect("a finish volume")
        .bounds
        .mins
        .y;

    const LOOKAHEAD: u32 = 12;
    const DECIDE_EVERY: u32 = 4;
    const MAX_TICKS: u32 = 6_000;

    let mut top_speed = s(0.0);
    let mut speed_at_pad = s(0.0);
    let mut slid = false;
    let mut ticks = 0u32;

    while ticks < MAX_TICKS && state.player.origin.y < finish_y {
        let mut best = None;
        let mut best_score = Scalar::NEG_INFINITY;
        for &c in &all {
            let f = hold(&state, c, LOOKAHEAD);
            if !f.player.origin.is_finite() {
                continue;
            }
            let score = f.player.origin.y + s(0.25) * f.player.velocity.truncate().length();
            if score > best_score {
                best_score = score;
                best = Some(c);
            }
        }
        let c = best.expect("every control led somewhere non-finite");

        for _ in 0..DECIDE_EVERY {
            state = hold(&state, c, 1);
            let o = state.player.origin;
            assert!(o.is_finite(), "the player left the number line");
            top_speed = top_speed.max(state.player.velocity.truncate().length());
            if o.y >= s(2048.0) && o.y < s(2240.0) {
                speed_at_pad = speed_at_pad.max(state.player.velocity.truncate().length());
            }
            if state.player.timers.slide_ms > 0 {
                slid = true;
            }
        }
        ticks += DECIDE_EVERY;
    }

    eprintln!(
        "greedy bot: reached y={:.0} (finish at {finish_y:.0}) in {} ms, \
         top speed {top_speed:.1} ups, best speed on the entry pad {speed_at_pad:.1} ups, \
         slide armed: {slid}",
        state.player.origin.y, state.time_ms
    );

    assert!(
        state.player.origin.y >= finish_y,
        "the bot only reached y={:.0} of {finish_y:.0} — the route is blocked, or \
         the lintel cannot be passed",
        state.player.origin.y
    );
    // The whole premise of the run-up: it has to reach `slide_entry_speed`,
    // which is above the 320 ground cap and therefore only reachable by
    // strafejumping. A greedy bot is a lower bound on a human.
    assert!(
        speed_at_pad >= profile.slide_entry_speed,
        "the run-up only reached {speed_at_pad} ups on the entry pad, below \
         slide_entry_speed {} — the strafe corridor is too short",
        profile.slide_entry_speed
    );
}
