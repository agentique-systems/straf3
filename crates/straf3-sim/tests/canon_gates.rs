//! Gate evidence decidable from the implementation, measured rather than read.
//!
//! `docs/movement-canon.md` §1.3's gates are not all weighed criteria waiting on
//! the lab's sweep. Several are properties of `step.rs` and `profile.rs` that can
//! be established by running the mover, and two of them Part 1 **pre-disclosed**
//! as known before any candidate measurement existed — G5(a)'s dash exposure and
//! G5(a)'s wall-jump ruling under amendment 2's flat-ground reading.
//!
//! Those two disclosures are the reason this file exists. Part 1 says the dash
//! is known to fail G5(a) "from reading `crates/straf3-sim/src/step.rs` alone and
//! before any candidate measurement exists". A claim of that weight, which
//! decides a candidate at a gate with no weighing, should be *run* and not just
//! read — and if it turns out the code does not do what Part 1 said it does,
//! that is a finding about Part 1 that must surface before a verdict, not after.

use straf3_sim::num::{Scalar, s, vec3};
use straf3_sim::world::FlatGround;
use straf3_sim::{Buttons, PhysicsProfile, SimState, UserCmd, run, step};

const MS: u16 = 8;

fn still() -> UserCmd {
    UserCmd::still(MS)
}

fn jump_cmd() -> UserCmd {
    UserCmd {
        buttons: Buttons::JUMP,
        ..UserCmd::still(MS)
    }
}

fn settled(world: &FlatGround, profile: &PhysicsProfile) -> SimState {
    let spawn = SimState::spawned_at(vec3(s(0.0), s(0.0), s(100.0)), s(0.0));
    run(&spawn, &vec![still(); 400], world, profile)
}

fn horizontal_speed(st: &SimState) -> Scalar {
    let v = st.player.velocity;
    (v.x * v.x + v.y * v.y).sqrt()
}

/// **G5(a), dash — the exposure Part 1 disclosed, run rather than read.**
///
/// §1.3 G5(a): run a player who never exceeds `max_speed`, on flat open ground,
/// and count how many times the mechanic becomes available. *Fails if the count
/// on flat ground is not zero.*
///
/// The player here never exceeds `max_speed` by the widest possible margin:
/// they never move horizontally at all. They stand still and jump on the spot.
#[test]
fn the_dash_arms_on_flat_ground_at_zero_speed() {
    let p = PhysicsProfile::experimental();
    let world = FlatGround::at(s(0.0));

    let mut st = settled(&world, &p);
    let mut armings = 0;

    // Jump on the spot, land, repeat. Ten cycles is enough to establish that
    // the count is unbounded rather than incidental.
    for _ in 0..10 {
        st = step(&st, &jump_cmd(), &world, &p);
        assert!(!st.player.ground.is_grounded(), "premise: jumped");

        // Release the input and wait out the flight.
        while !st.player.ground.is_grounded() {
            st = step(&st, &still(), &world, &p);
        }

        // The landing that ended a jump arms a dash window.
        if st.player.timers.dash_ms > 0 {
            armings += 1;
        }

        assert!(
            horizontal_speed(&st) < s(1.0),
            "premise: this player never carried horizontal speed, and got {}",
            horizontal_speed(&st)
        );
    }

    assert_eq!(
        armings, 10,
        "G5(a): the dash armed {armings} times out of 10 landings at zero speed"
    );

    // The gate's own words: the count on flat ground is not zero, so the dash
    // fails G5(a) as written. Part 1 predicted this from the code before any
    // candidate was measured; this is that prediction executed.
}

/// **G5(a), crouch slide — passes, and for the reason `profile.rs` designed in.**
///
/// `slide_entry_speed` 400 is above `max_speed` 320, so a player who never
/// exceeds `max_speed` cannot arm a slide by ground acceleration alone.
#[test]
fn the_slide_never_arms_below_the_entry_speed_on_flat_ground() {
    let p = PhysicsProfile::experimental();
    let world = FlatGround::at(s(0.0));
    assert!(p.slide_entry_speed > p.max_speed, "premise: entry above the cap");

    let mut st = settled(&world, &p);
    // Accelerate forward on the ground for four seconds: ground acceleration
    // alone, which cannot exceed `max_speed`.
    let forward = UserCmd {
        forward_move: 127,
        ..still()
    };
    st = run(&st, &vec![forward; 500], &world, &p);
    assert!(
        horizontal_speed(&st) <= p.max_speed + s(1.0),
        "premise: ground acceleration stays at the cap, got {}",
        horizontal_speed(&st)
    );

    // Now crouch. The slide must not arm.
    let crouch = UserCmd {
        buttons: Buttons::CROUCH,
        forward_move: 127,
        ..still()
    };
    let crouched = step(&st, &crouch, &world, &p);
    assert_eq!(
        crouched.player.timers.slide_ms, 0,
        "G5(a): a slide armed at or below max_speed on flat ground"
    );
}

/// **G5(a), wall jump — passes on flat ground, which is amendment 2's ruling.**
///
/// `note_wall_contact` only records a plane whose `|normal.z|` is at or below
/// `wall_normal_max`. Flat ground's normal is (0,0,1), so no amount of running
/// about on it arms a wall jump. Amendment 2 ruled that flat open ground decides
/// the gate precisely so that a mechanic conditioned on geometry is not rejected
/// for being geometric.
#[test]
fn the_wall_jump_never_arms_on_flat_ground() {
    let p = PhysicsProfile::experimental();
    let world = FlatGround::at(s(0.0));

    let mut st = settled(&world, &p);
    let forward = UserCmd {
        forward_move: 127,
        ..still()
    };

    // Run, jump, land, repeat — everything flat ground affords.
    for _ in 0..10 {
        st = run(&st, &vec![forward; 20], &world, &p);
        st = step(&st, &jump_cmd(), &world, &p);
        while !st.player.ground.is_grounded() {
            st = step(&st, &forward, &world, &p);
        }
        assert_eq!(
            st.player.timers.wall_contact_ms, 0,
            "G5(a): flat ground armed a wall jump"
        );
    }
}

/// **G4, all three — no candidate reads an input outside the vocabulary.**
///
/// The vocabulary §1.3 names is the two move axes, the view, jump and crouch.
/// `Buttons` also carries `ATTACK` and `WALK`, which are *not* movement inputs
/// a candidate may read. So the test is behavioural rather than a bitfield
/// identity: hold the two non-vocabulary buttons, at a speed and from a state
/// where every candidate would otherwise be interested, and assert none of them
/// arms or fires.
#[test]
fn no_candidate_reads_an_input_outside_the_movement_vocabulary() {
    let p = PhysicsProfile::experimental();
    let world = FlatGround::at(s(0.0));

    let mut st = settled(&world, &p);
    // Well above `slide_entry_speed`, so a slide is only ever one crouch edge
    // away — the state in which reading a stray button would show up.
    st.player.velocity = vec3(p.slide_entry_speed + s(200.0), s(0.0), s(0.0));

    let stray = UserCmd {
        buttons: Buttons::ATTACK.with(Buttons::WALK),
        forward_move: 127,
        ..still()
    };

    let after = run(&st, &vec![stray; 50], &world, &p);
    assert_eq!(
        after.player.timers.slide_ms, 0,
        "G4: a slide armed from a button outside the vocabulary"
    );
    assert_eq!(
        after.player.timers.dash_ms, 0,
        "G4: a dash armed from a button outside the vocabulary"
    );
    assert_eq!(
        after.player.timers.wall_contact_ms, 0,
        "G4: a wall contact armed from a button outside the vocabulary"
    );
}
