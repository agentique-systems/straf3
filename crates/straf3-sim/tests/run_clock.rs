//! The run clock: does a command stream produce a time, and is it the *right*
//! time?
//!
//! ARCHITECTURE C4 asks for this suite by name, and asks for it here rather
//! than in a server or a map crate, because every rule it pins is a rule about
//! `Pmove` — which sweeps count as motion, and which of the ones that count are
//! subsequently thrown away. None of that is visible from above the seam.
//!
//! The three cases C4 names are [`a_step_up_does_not_credit_the_attempt_it_threw_away`],
//! [`a_blocked_lift_credits_nothing_beyond_where_it_stopped`] and
//! [`a_volume_thinner_than_one_command_of_travel_is_still_crossed`]. The last is
//! the one the whole design exists for: at 1,000 ups and 8 ms a player covers 8
//! units per command, so a finish line tested at the command's endpoint is
//! missed by exactly the runs a leaderboard cares about.
//!
//! # The world these tests use
//!
//! [`TriggerBoxes`] is a hand-written axis-aligned tracer, not
//! `straf3-collision` — `straf3-sim` does not depend on it, deliberately (see
//! that crate's manifest). It is also not a shortcut: implementing the
//! traversed-prefix rule twice, independently, in two crates that cannot see
//! each other, is what makes it a *contract* on [`World`] rather than a habit of
//! one tracer.

use straf3_sim::num::{Scalar, Vec3, s, vec3};
use straf3_sim::state::{GroundState, RunState};
use straf3_sim::world::{SurfaceFlags, Sweep, Trace, TriggerSet, World};
use straf3_sim::{Buttons, PhysicsProfile, SimState, TickRate, UserCmd, ViewAngles, step_in_place};

/// 125 Hz, the rate every number in this file is quoted at.
const MS: u16 = TickRate::HZ_125.command_millis();

// ═══ a world of boxes, some of which are timing volumes ═════════════════════

/// An axis-aligned box.
#[derive(Debug, Clone, Copy)]
struct Box3 {
    mins: Vec3,
    maxs: Vec3,
}

impl Box3 {
    fn new(mins: [f32; 3], maxs: [f32; 3]) -> Self {
        Self {
            mins: vec3(s(mins[0]), s(mins[1]), s(mins[2])),
            maxs: vec3(s(maxs[0]), s(maxs[1]), s(maxs[2])),
        }
    }

    /// The box grown by the hull's half extents, so the swept *box* becomes a
    /// swept *point* at the hull's centre.
    fn expanded(&self, half: Vec3) -> (Vec3, Vec3) {
        (self.mins - half, self.maxs + half)
    }
}

/// Solid boxes, plus timing volumes that are not solid.
#[derive(Debug, Clone, Default)]
struct TriggerBoxes {
    solids: Vec<Box3>,
    triggers: Vec<(Box3, TriggerSet)>,
}

impl TriggerBoxes {
    /// A floor at z=0 wide enough that nothing in these tests falls off it.
    fn floor() -> Self {
        Self {
            solids: vec![Box3::new([-4096.0, -4096.0, -512.0], [4096.0, 4096.0, 0.0])],
            triggers: Vec::new(),
        }
    }

    fn solid(mut self, mins: [f32; 3], maxs: [f32; 3]) -> Self {
        self.solids.push(Box3::new(mins, maxs));
        self
    }

    fn trigger(mut self, set: TriggerSet, mins: [f32; 3], maxs: [f32; 3]) -> Self {
        self.triggers.push((Box3::new(mins, maxs), set));
        self
    }
}

/// Where a swept point enters and leaves a box, or `None` if it misses.
///
/// Returns `(enter, leave, normal)` with fractions in `0.0..=1.0`. `enter` is
/// negative when the point starts inside.
fn slab_clip(start: Vec3, delta: Vec3, lo: Vec3, hi: Vec3) -> Option<(Scalar, Scalar, Vec3)> {
    let mut enter = s(-1.0);
    let mut leave = s(1.0);
    let mut normal = Vec3::ZERO;

    for axis in 0..3 {
        if delta[axis].abs() < s(1e-9) {
            if start[axis] <= lo[axis] || start[axis] >= hi[axis] {
                return None;
            }
            continue;
        }
        let mut t_lo = (lo[axis] - start[axis]) / delta[axis];
        let mut t_hi = (hi[axis] - start[axis]) / delta[axis];
        let mut sign = s(-1.0); // crossing the low face: normal points -axis
        if t_lo > t_hi {
            core::mem::swap(&mut t_lo, &mut t_hi);
            sign = s(1.0);
        }
        if t_lo > enter {
            enter = t_lo;
            normal = Vec3::ZERO;
            normal[axis] = sign;
        }
        if t_hi < leave {
            leave = t_hi;
        }
        if enter > leave {
            return None;
        }
    }
    if leave < s(0.0) || enter >= s(1.0) {
        return None;
    }
    Some((enter, leave, normal))
}

impl World for TriggerBoxes {
    /// The earliest solid hit, plus the volumes overlapped over the prefix of
    /// the sweep that hit actually leaves travellable.
    ///
    /// The second half is the [`Trace::triggers`] contract, and the shape of it
    /// is the point: the volume loop runs **after** the solid loop and clamps to
    /// its `fraction`. Gathering volumes inside the solid loop, as they are
    /// reached, is the mistake C4 exists to forbid — it credits a player with
    /// every volume along the segment they *asked* to travel rather than the one
    /// they got.
    fn trace(&self, sweep: &Sweep) -> Trace {
        let start = sweep.start + sweep.center_offset;
        let end = sweep.end + sweep.center_offset;
        let delta = end - start;
        let half = sweep.half_extents;

        let mut best = Trace::clear();
        let mut start_solid = false;
        let mut all_solid = false;

        for solid in &self.solids {
            let (lo, hi) = solid.expanded(half);
            let inside = |p: Vec3| (0..3).all(|a| p[a] > lo[a] && p[a] < hi[a]);
            if inside(start) {
                start_solid = true;
                all_solid |= inside(end);
                continue;
            }
            let Some((enter, _leave, normal)) = slab_clip(start, delta, lo, hi) else {
                continue;
            };
            if enter < s(0.0) || normal == Vec3::ZERO {
                continue;
            }
            if enter < best.fraction {
                best.fraction = enter;
                best.normal = normal;
                best.surface = SurfaceFlags::NONE;
            }
        }

        if start_solid {
            // `all_solid` implies a zero fraction — the C8 obligation the
            // traversed-prefix rule leans on.
            best = Trace {
                fraction: s(0.0),
                normal: Vec3::Z,
                surface: SurfaceFlags::NONE,
                start_solid: true,
                all_solid,
                triggers: TriggerSet::NONE,
            };
        }

        // Rule 1: gather over `[start, start + delta * fraction]`.
        let travelled_end = start + delta * best.fraction;
        for (volume, set) in &self.triggers {
            let (lo, hi) = volume.expanded(half);
            let inside = |p: Vec3| (0..3).all(|a| p[a] > lo[a] && p[a] < hi[a]);
            let touched = inside(start)
                || slab_clip(start, travelled_end - start, lo, hi)
                    .is_some_and(|(enter, leave, _)| enter <= s(1.0) && leave >= s(0.0));
            if touched {
                best.triggers = best.triggers.with(*set);
            }
        }
        best
    }
}

// ═══ harness ════════════════════════════════════════════════════════════════

/// The player's origin when standing on the floor at z=0, held clear of it by
/// the same epsilon the mover uses on landing.
const RESTING_Z: f32 = 24.125;

fn standing_at(x: f32, y: f32) -> SimState {
    let mut st = SimState::spawned_at(vec3(s(x), s(y), s(RESTING_Z)), s(0.0));
    st.player.ground = GroundState::Grounded {
        normal: vec3(s(0.0), s(0.0), s(1.0)),
    };
    st
}

/// A command that runs forward along +X at full deflection.
fn forward() -> UserCmd {
    UserCmd {
        forward_move: 127,
        view: ViewAngles::looking_along(s(0.0)),
        ..UserCmd::still(MS)
    }
}

/// Run `count` commands, returning the state and everything touched along the
/// way.
fn drive<W: World>(
    st: &mut SimState,
    world: &W,
    profile: &PhysicsProfile,
    cmd: &UserCmd,
    count: usize,
) -> TriggerSet {
    let mut touched = TriggerSet::NONE;
    for _ in 0..count {
        touched = touched.with(step_in_place(st, cmd, world, profile));
    }
    touched
}

/// Launch the player horizontally at `speed` and take one command.
fn one_command_at<W: World>(
    world: &W,
    profile: &PhysicsProfile,
    from: SimState,
    velocity: Vec3,
) -> (SimState, TriggerSet) {
    let mut st = from;
    st.player.velocity = velocity;
    let touched = step_in_place(&mut st, &forward(), world, profile);
    (st, touched)
}

// ═══ the three cases ARCHITECTURE C4 names ══════════════════════════════════

/// C4 case (c), and the one the whole design exists for: **swept, not sampled**.
///
/// # Why the numbers are what they are
///
/// The naive implementation this is aimed at tests the player's hull where the
/// command *ended*. That is only wrong when the hull can clear the volume
/// between two consecutive endpoints, and the player's hull is 30 units wide, so
/// the window in which a 4-unit volume overlaps it is 34 units of origin travel.
/// Walking speed does not step over that. A jump pad, a teleport exit or a
/// plasma climb does: 5,000 ups over an 8 ms command is 40 units, and the
/// arithmetic below is arranged so the crossing command runs from x=180 to
/// x=220 — hull spans [165, 195] and [205, 235], with the volume at [200, 204]
/// squarely between them and touching neither.
///
/// The test asserts that gap explicitly rather than trusting it, because if the
/// endpoints *did* overlap the volume this test would pass against exactly the
/// implementation it exists to fail.
#[test]
fn a_volume_thinner_than_one_command_of_travel_is_still_crossed() {
    let profile = PhysicsProfile::cpm();
    let world = TriggerBoxes::floor().trigger(
        TriggerSet::FINISH,
        [200.0, -64.0, -64.0],
        [204.0, 64.0, 512.0],
    );
    const VOLUME: (f32, f32) = (200.0, 204.0);
    const HALF_WIDTH: f32 = 15.0;

    // Airborne, so nothing slows the player: `PM_Friction` only bites below
    // walking speed in the air, and `PM_Accelerate` grants nothing to someone
    // already faster than `max_speed`.
    let mut st = SimState::spawned_at(vec3(s(-20.0), s(0.0), s(200.0)), s(0.0));
    st.player.velocity = vec3(s(5000.0), s(0.0), s(0.0));

    let mut crossed = None;
    for _ in 0..8 {
        let before = st.player.origin.x;
        let touched = step_in_place(&mut st, &forward(), &world, &profile);
        if touched.contains(TriggerSet::FINISH) {
            crossed = Some((before, st.player.origin.x));
            break;
        }
    }

    let (before, after) = crossed.expect("a 4-unit volume was stepped over between two commands");
    let overlaps = |x: Scalar| x + s(HALF_WIDTH) >= s(VOLUME.0) && x - s(HALF_WIDTH) <= s(VOLUME.1);
    assert!(
        !overlaps(before) && !overlaps(after),
        "this test is only meaningful if neither endpoint touches the volume; \
         the command ran x={before} -> x={after}"
    );
    assert!(
        st.player.velocity.x > s(4900.0),
        "a trigger is not solid and must not slow the player: {}",
        st.player.velocity.x
    );
}

/// C4 case (a): the rollback in `PM_StepSlideMove`.
///
/// # The geometry, because the numbers are load-bearing
///
/// A 16-unit step at x=0 — inside `step_height`, so it is climbed rather than
/// walked into. The player starts at x=-30 travelling +X fast enough to want 16
/// units in one command.
///
/// - The **first attempt** runs at the standing height and is stopped by the
///   riser with the hull's front face at x≈0, so the hull has swept over
///   x ∈ [-45, 0] at z ∈ [0.125, 56.125]. That covers the volume.
/// - It is then **discarded**: `p.origin` is overwritten with the stepped-up
///   position and `p.velocity` with its pre-attempt value.
/// - The **lift** runs from the *original* x=-30, where the hull spans
///   x ∈ [-45, -15] and never reaches the volume.
/// - The **second attempt** runs 16 units up, at z ∈ [16.125, 72.125], clearing
///   the volume's ceiling at z=14.
/// - The **drop** is blocked by the step top after 0.125 units, so its traversed
///   prefix stays above the volume too.
///
/// So the only sweep that ever overlapped the volume is the one the physics
/// threw away, and the accumulator must not contain it.
#[test]
fn a_step_up_does_not_credit_the_attempt_it_threw_away() {
    let profile = PhysicsProfile::cpm();
    let world = TriggerBoxes::floor()
        // The step: 16 units, climbable.
        .solid([0.0, -4096.0, -512.0], [4096.0, 4096.0, 16.0])
        // A low volume in front of the riser, under the height the stepped-up
        // hull reaches.
        .trigger(TriggerSet::FINISH, [-6.0, -64.0, 0.0], [-2.0, 64.0, 14.0]);

    let (st, touched) = one_command_at(
        &world,
        &profile,
        standing_at(-30.0, 0.0),
        vec3(s(2000.0), s(0.0), s(0.0)),
    );

    assert!(
        st.player.origin.z > s(RESTING_Z + 8.0),
        "the step must actually have been climbed, ended at z={}",
        st.player.origin.z
    );
    assert!(
        !touched.contains(TriggerSet::FINISH),
        "the discarded first attempt credited a volume the player never occupied"
    );
    assert_eq!(st.run, RunState::NotStarted);
}

/// C4 case (b): a lift that is blocked credits nothing beyond where it stopped.
///
/// The same step, but now with a slab just above the player's head. The lift is
/// issued over the full `step_height`, and the hull swept over that whole
/// segment would reach the volume at z ∈ [60, 70]; the lift is stopped after
/// nothing at all by the slab.
///
/// This is the case that fails under C4's call-site table alone — the table says
/// the up-lift counts — and passes under rule 1, which is why rule 1 is a
/// contract on the tracer rather than a note in the mover.
#[test]
fn a_blocked_lift_credits_nothing_beyond_where_it_stopped() {
    let profile = PhysicsProfile::cpm();
    let world = TriggerBoxes::floor()
        .solid([0.0, -4096.0, -512.0], [4096.0, 4096.0, 16.0])
        // A slab a hair above the standing hull's crown at z=56.125.
        .solid([-4096.0, -4096.0, 56.2], [4096.0, 4096.0, 58.0])
        // Beyond the lift, reachable only by the segment the lift asked for.
        .trigger(TriggerSet::FINISH, [-64.0, -64.0, 60.0], [64.0, 64.0, 70.0]);

    let (_, touched) = one_command_at(
        &world,
        &profile,
        standing_at(-30.0, 0.0),
        vec3(s(2000.0), s(0.0), s(0.0)),
    );

    assert!(
        !touched.contains(TriggerSet::FINISH),
        "a lift that travelled nowhere credited a volume above the ceiling that stopped it"
    );
}

// ═══ the accounting rules, one at a time ════════════════════════════════════

/// `PM_GroundTrace`'s downward probe is a question about the floor, not motion:
/// the hull does not go there. It fires twice per command, on every command.
///
/// # Getting this to discriminate at all
///
/// The probe reaches `ground_trace_probe` — a quarter of a unit — below the
/// hull, so a volume has to be placed inside that quarter unit to tell the two
/// implementations apart, and it has to be somewhere the *committed* move does
/// not then reach anyway. Both are arranged here: the player is airborne at a
/// round z=100 with the volume's crown at 75.9, a tenth of a unit under the
/// hull's underside at 76.0, and one command of free fall from rest drops the
/// hull by 0.026 — nowhere near it. The probe sweeps to 75.75 and goes straight
/// through it.
///
/// The tenth of a unit is the whole margin, so this test asserts the fall
/// distance rather than assuming it: if gravity ever carried the hull to the
/// volume, the test would pass for the wrong reason.
#[test]
fn the_ground_probe_is_a_question_not_a_move() {
    let profile = PhysicsProfile::cpm();
    assert_eq!(
        profile.ground_trace_probe,
        s(0.25),
        "the margin below depends on this"
    );

    let world = TriggerBoxes::default().trigger(
        TriggerSet::START,
        [-64.0, -64.0, 60.0],
        [64.0, 64.0, 75.9],
    );

    // Airborne over open space, hull underside at exactly 100 - 24 = 76.
    let mut st = SimState::spawned_at(vec3(s(0.0), s(0.0), s(100.0)), s(0.0));
    let touched = step_in_place(&mut st, &UserCmd::still(MS), &world, &profile);

    let fell = s(100.0) - st.player.origin.z;
    assert!(
        fell < s(0.1),
        "one command of free fall moved the hull {fell} units, into the volume the \
         probe was supposed to be the only thing reaching"
    );
    assert!(
        !touched.contains(TriggerSet::START),
        "the downward ground probe was counted as motion"
    );
    assert_eq!(st.run, RunState::NotStarted, "the clock started by itself");
}

/// The same claim over a long stand: a volume under the floor a player is
/// standing on is never crossed, however long they stand there.
#[test]
fn standing_still_on_a_floor_never_crosses_what_is_under_it() {
    let profile = PhysicsProfile::cpm();
    let world =
        TriggerBoxes::floor().trigger(TriggerSet::START, [-64.0, -64.0, -8.0], [64.0, 64.0, -1.0]);

    let mut st = standing_at(0.0, 0.0);
    let touched = drive(&mut st, &world, &profile, &UserCmd::still(MS), 200);

    assert!(!touched.contains(TriggerSet::START));
    assert_eq!(st.run, RunState::NotStarted, "the clock started by itself");
}

/// `PM_CheckDuck`'s stand-up probe is zero-length and uses a *different* hull —
/// the standing one, while the player is crouched. A volume that only the
/// standing hull reaches must not be credited to a player who stayed crouched.
#[test]
fn the_stand_up_probe_does_not_count_and_uses_its_own_hull() {
    let profile = PhysicsProfile::cpm();
    let crouched = profile.hull(true);
    let standing = profile.hull(false);
    // Between the crouched crown and the standing crown, so only the stand-up
    // probe's hull could reach it.
    let crouched_top = RESTING_Z + (crouched.center_offset.z + crouched.half_extents.z);
    let standing_top = RESTING_Z + (standing.center_offset.z + standing.half_extents.z);
    assert!(
        standing_top > crouched_top + s(4.0),
        "the hulls must differ"
    );

    let world = TriggerBoxes::floor()
        // A ceiling that refuses the stand-up, right above the crouched crown.
        .solid(
            [-4096.0, -4096.0, crouched_top + s(1.0)],
            [4096.0, 4096.0, crouched_top + s(4.0)],
        )
        .trigger(
            TriggerSet::START,
            [-64.0, -64.0, crouched_top + s(1.5)],
            [64.0, 64.0, crouched_top + s(3.5)],
        );

    let crouch = UserCmd {
        buttons: Buttons::CROUCH,
        ..UserCmd::still(MS)
    };
    let mut st = standing_at(0.0, 0.0);
    st.player.crouched = true;
    // Crouch held for a while, then released under the ceiling: the stand-up
    // probe fires on every released command and is refused every time.
    let mut touched = drive(&mut st, &world, &profile, &crouch, 5);
    touched = touched.with(drive(&mut st, &world, &profile, &UserCmd::still(MS), 5));

    assert!(st.player.crouched, "the ceiling must refuse the stand-up");
    assert!(
        !touched.contains(TriggerSet::START),
        "the stand-up probe was counted as motion"
    );
}

// ═══ the clock itself ═══════════════════════════════════════════════════════

/// A whole run, start line to finish line, on the geometry rather than on a
/// call from outside.
#[test]
fn a_command_stream_between_two_lines_is_a_time() {
    let profile = PhysicsProfile::cpm();
    let world = TriggerBoxes::floor()
        .trigger(TriggerSet::START, [-2.0, -64.0, 0.0], [2.0, 64.0, 128.0])
        .trigger(
            TriggerSet::FINISH,
            [1000.0, -64.0, 0.0],
            [1004.0, 64.0, 128.0],
        );

    let mut st = standing_at(-200.0, 0.0);
    st.player.velocity = vec3(s(400.0), s(0.0), s(0.0));

    let mut ticks_while_running = 0;
    for _ in 0..600 {
        step_in_place(&mut st, &forward(), &world, &profile);
        if matches!(st.run, RunState::Running { .. }) {
            ticks_while_running += 1;
        }
        if matches!(st.run, RunState::Finished { .. }) {
            break;
        }
    }

    let RunState::Finished {
        started_at_ms,
        finished_at_ms,
    } = st.run
    else {
        panic!("the run never finished: {:?}", st.run);
    };
    assert!(started_at_ms > 0, "the clock started before the start line");
    assert!(finished_at_ms > started_at_ms);

    let elapsed = st
        .run
        .elapsed_ms(st.time_ms)
        .expect("a finished run has a time");
    // Every quantity here is an integer sum of command durations, so this is an
    // equality and not a tolerance.
    assert_eq!(elapsed, finished_at_ms - started_at_ms);
    assert_eq!(elapsed, u32::from(MS) * ticks_while_running);
    assert_eq!(
        elapsed % u32::from(MS),
        0,
        "a time is a whole number of commands"
    );
}

/// **Sub-stepping makes the clock finer, exactly as ARCHITECTURE C4 predicts.**
///
/// C4 says the accumulator is consumed at the end of each `Pmove::run`, so a
/// start or finish is stamped at the *sub-step* boundary it was crossed on —
/// still an exact integer sum of durations, still no interpolation, but no
/// longer rounded out to the whole command.
///
/// The case that shows it is a run that begins and ends inside one command. A
/// single step would OR both volumes into one `TriggerSet`, stamp both at the
/// command's end and report a time of **zero** — a finish line crossed by a
/// player who, by the clock, never started. Sub-stepped, the two crossings
/// land on different boundaries and the time is a time.
///
/// # Why the player is airborne and fast
///
/// So that the horizontal speed is a constant and the geometry below is
/// arithmetic rather than a guess: air friction only bites below 1 ups, and at
/// 800 ups `PM_Accelerate`'s clamp grants nothing towards a 320 ups wish
/// speed. 800 ups is 52.8 units per 66 ms sub-step. The volumes span z
/// 0..512, so the fall does not enter into it.
#[test]
fn a_run_that_starts_and_finishes_inside_one_command_still_has_a_time() {
    let profile = PhysicsProfile::cpm();
    let world = TriggerBoxes::floor()
        .trigger(TriggerSet::START, [-2.0, -64.0, 0.0], [2.0, 64.0, 512.0])
        .trigger(
            TriggerSet::FINISH,
            [208.0, -64.0, 0.0],
            [212.0, 64.0, 512.0],
        );

    /// Long enough to contain the whole run: seven sub-steps, 66×6 + 4.
    const LONG_MS: u16 = 400;

    let airborne_at_800 = || {
        let mut st = SimState::spawned_at(vec3(s(-100.0), s(0.0), s(100.0)), s(0.0));
        st.player.velocity = vec3(s(800.0), s(0.0), s(0.0));
        st
    };

    // The whole route in one command.
    let mut long = airborne_at_800();
    let touched = step_in_place(
        &mut long,
        &UserCmd {
            forward_move: 127,
            ..UserCmd::still(LONG_MS)
        },
        &world,
        &profile,
    );
    assert!(
        touched.contains(TriggerSet::START) && touched.contains(TriggerSet::FINISH),
        "the premise: one command that crosses both lines",
    );

    let RunState::Finished {
        started_at_ms,
        finished_at_ms,
    } = long.run
    else {
        panic!("the run never finished: {:?}", long.run);
    };
    assert!(
        finished_at_ms > started_at_ms,
        "start and finish were stamped at the same instant: the run took \
         {finished_at_ms} − {started_at_ms} = 0 ms, which is the single-step answer",
    );
    let bound = u32::from(straf3_sim::PMOVE_SUBSTEP_MAX_MS);
    for stamp in [started_at_ms, finished_at_ms] {
        assert_eq!(
            stamp % bound,
            0,
            "a stamp landed off a sub-step boundary, so something interpolated",
        );
        assert!(stamp <= u32::from(LONG_MS));
    }
    let long_elapsed = finished_at_ms - started_at_ms;

    // The same route at the rate the game is actually played at. This is the
    // honest answer the long command's time is being measured against: 50
    // commands of 8 ms cover the same ground and stamp on 8 ms boundaries.
    let mut short = airborne_at_800();
    let count = usize::from(LONG_MS / MS);
    drive(&mut short, &world, &profile, &forward(), count);
    let RunState::Finished {
        started_at_ms: short_start,
        finished_at_ms: short_finish,
    } = short.run
    else {
        panic!("the 125 Hz run never finished: {:?}", short.run);
    };
    let short_elapsed = short_finish - short_start;

    assert!(
        long_elapsed.abs_diff(short_elapsed) <= bound,
        "one 400 ms command timed the run at {long_elapsed} ms where fifty 8 ms \
         commands over the same ground timed it at {short_elapsed} ms; the two \
         should agree to within one sub-step",
    );
}

/// The time depends on the command stream and on nothing else.
///
/// The same commands replayed produce the same milliseconds; the same *route*
/// walked at a different tick rate does not, and that is the deliberate
/// consequence C4 records rather than a bug.
#[test]
fn the_same_commands_produce_the_same_time() {
    let profile = PhysicsProfile::cpm();
    let world = TriggerBoxes::floor()
        .trigger(TriggerSet::START, [-2.0, -64.0, 0.0], [2.0, 64.0, 128.0])
        .trigger(
            TriggerSet::FINISH,
            [1000.0, -64.0, 0.0],
            [1004.0, 64.0, 128.0],
        );

    let run_it = |ms: u16| {
        let mut st = standing_at(-200.0, 0.0);
        st.player.velocity = vec3(s(400.0), s(0.0), s(0.0));
        let cmd = UserCmd {
            forward_move: 127,
            view: ViewAngles::looking_along(s(0.0)),
            ..UserCmd::still(ms)
        };
        for _ in 0..(600 * 8 / usize::from(ms)) {
            step_in_place(&mut st, &cmd, &world, &profile);
            if matches!(st.run, RunState::Finished { .. }) {
                break;
            }
        }
        (st.run.elapsed_ms(st.time_ms), st.checksum())
    };

    let (first, digest_a) = run_it(MS);
    let (second, digest_b) = run_it(MS);
    assert_eq!(first, second);
    assert_eq!(digest_a, digest_b);
    assert!(first.is_some_and(|t| t > 0), "no time was produced");

    // Different tick rate, different simulation, therefore a different time.
    // Recorded as an assertion because it is the reason the ranked tick rate is
    // fixed, not an accident to be fixed later.
    let (finer, _) = run_it(TickRate::HZ_250.command_millis());
    assert!(finer.is_some());
    assert_ne!(finer, first);
}

/// The clock is folded into the state digest, so a recording cannot claim a
/// time its command stream does not produce.
#[test]
fn the_clock_is_part_of_the_state_a_recording_is_verified_against() {
    let mut not_started = standing_at(0.0, 0.0);
    let mut running = not_started;
    running.run.start(1000);
    let mut finished = running;
    finished.run.finish(2000);

    assert_ne!(not_started.checksum(), running.checksum());
    assert_ne!(running.checksum(), finished.checksum());

    // And a different time is a different state, which is what makes the digest
    // a check on the number rather than only on the trajectory.
    let mut slower = running;
    slower.run.finish(2008);
    assert_ne!(finished.checksum(), slower.checksum());

    not_started.run.finish(500);
    assert_eq!(
        not_started.run,
        RunState::NotStarted,
        "a finish before a start is not a run"
    );
}

/// Crossing the start line again mid-run does not restart the clock, and
/// crossing the finish twice does not extend it.
#[test]
fn the_lines_are_edges_not_states() {
    let profile = PhysicsProfile::cpm();
    // A start volume the player sits inside for many commands, then a finish.
    let world = TriggerBoxes::floor()
        .trigger(TriggerSet::START, [-64.0, -64.0, 0.0], [64.0, 64.0, 128.0])
        .trigger(
            TriggerSet::FINISH,
            [400.0, -64.0, 0.0],
            [4096.0, 64.0, 128.0],
        );

    let mut st = standing_at(0.0, 0.0);
    st.player.velocity = vec3(s(600.0), s(0.0), s(0.0));

    let mut started_at = None;
    let mut finished_at = None;
    for _ in 0..400 {
        step_in_place(&mut st, &forward(), &world, &profile);
        if let RunState::Running { started_at_ms } = st.run {
            let first = *started_at.get_or_insert(started_at_ms);
            assert_eq!(first, started_at_ms, "the clock restarted mid-volume");
        }
        if let RunState::Finished { finished_at_ms, .. } = st.run {
            let first = *finished_at.get_or_insert(finished_at_ms);
            assert_eq!(
                first, finished_at_ms,
                "the clock kept running past the finish"
            );
        }
    }
    assert!(started_at.is_some() && finished_at.is_some());
}

/// Checkpoints are reported to the caller but do not grow `SimState`.
///
/// Splits are a caller's bookkeeping; the physics never reads them. Putting
/// them in the simulation state would change every digest ever taken to carry
/// data nothing below the seam consults.
///
/// Note what the caller has to do, and why it is not done for them: the returned
/// set is *overlapped this command*, not *entered this command*, so a player
/// inside a volume reports it on every command they are inside it. Edge
/// detection is the caller's, because the honest primitive is the overlap —
/// `RunState` does its own idempotency (a second start does not restart), and a
/// checkpoint's semantics are the caller's to choose.
#[test]
fn checkpoints_are_reported_without_being_stored() {
    let profile = PhysicsProfile::cpm();
    let first = TriggerSet::checkpoint(0).expect("checkpoint 0 fits");
    let second = TriggerSet::checkpoint(1).expect("checkpoint 1 fits");
    assert!(!first.intersects(TriggerSet::START));
    assert!(!first.intersects(TriggerSet::FINISH));
    assert!(!first.intersects(second));
    assert_eq!(TriggerSet::checkpoint(TriggerSet::MAX_CHECKPOINTS), None);

    let world = TriggerBoxes::floor()
        .trigger(TriggerSet::START, [-2.0, -64.0, 0.0], [2.0, 64.0, 128.0])
        .trigger(first, [300.0, -64.0, 0.0], [304.0, 64.0, 128.0])
        .trigger(second, [600.0, -64.0, 0.0], [604.0, 64.0, 128.0]);

    let mut st = standing_at(-100.0, 0.0);
    st.player.velocity = vec3(s(600.0), s(0.0), s(0.0));

    let mut splits: Vec<(u32, u32)> = Vec::new();
    let mut seen = TriggerSet::NONE;
    for _ in 0..400 {
        let touched = step_in_place(&mut st, &forward(), &world, &profile);
        for index in 0..2u32 {
            let bit = TriggerSet::checkpoint(index).unwrap();
            // The caller's edge detection: first overlap wins, later commands
            // inside the same volume are the same crossing.
            if touched.contains(bit)
                && !seen.contains(bit)
                && let Some(elapsed) = st.run.elapsed_ms(st.time_ms)
            {
                seen = seen.with(bit);
                splits.push((index, elapsed));
            }
        }
    }

    assert_eq!(splits.len(), 2, "both checkpoints should have been crossed");
    assert_eq!(splits[0].0, 0);
    assert_eq!(splits[1].0, 1);
    assert!(
        splits[1].1 > splits[0].1,
        "splits must increase: {splits:?}"
    );
}
