//! The measured jump: canon Part 3, point 4.
//!
//! `docs/movement-canon.md` Part 3 point 4 recorded that Straf3's jump height
//! and airborne duration were **arithmetic, not measurement** — `270²/(2·800)`
//! and `2·270/800`, agreeing with a comment in `assets/maps/coil.map`, which
//! was built to those numbers. Nobody had run the mover and looked.
//!
//! This file does that. It is deliberately a *measurement* rather than a
//! derivation: it drops a player onto flat ground, lets them settle, presses
//! jump, and watches the origin. Nothing here computes an expected value from
//! `jump_velocity` and `gravity` — that is the whole point, because the closed
//! form is what is being checked.
//!
//! Why the closed form is not obviously right: `step.rs` integrates gravity at
//! the **average of the start and end vertical speeds** within a sub-step, and
//! sub-steps at [`PMOVE_SUBSTEP_MAX_MS`]. The textbook `v²/2g` assumes
//! continuous motion; a per-command integrator lands somewhere near it and not
//! necessarily on it. §1.8 point 3 fixes the rate at 125 Hz, so that is the
//! rate these numbers are taken at, and the rate is part of the physics.

use straf3_sim::num::{Scalar, Vec3, s, vec3};
use straf3_sim::world::FlatGround;
use straf3_sim::{Buttons, PhysicsProfile, SimState, UserCmd, run, step};

/// 8 ms — 125 Hz, the rate every number in canon and the lab is taken at.
const MS: u16 = 8;

fn still(ms: u16) -> UserCmd {
    UserCmd::still(ms)
}

fn jump_cmd(ms: u16) -> UserCmd {
    UserCmd {
        buttons: Buttons::JUMP,
        ..UserCmd::still(ms)
    }
}

/// Drop the player onto the floor and let them come to rest.
fn settled(world: &FlatGround, profile: &PhysicsProfile) -> SimState {
    let spawn = SimState::spawned_at(vec3(s(0.0), s(0.0), s(100.0)), s(0.0));
    run(&spawn, &vec![still(MS); 400], world, profile)
}

/// What one jump did, measured rather than derived.
struct Jump {
    /// Peak origin z minus the resting origin z, in units.
    apex: Scalar,
    /// Milliseconds from the jump command until the player is grounded again.
    airborne_ms: u32,
    /// The command count that produced `airborne_ms`, for the record.
    commands: u32,
}

/// Press jump once at `ms` per command and watch until the player lands.
///
/// The clock starts on the jump command itself: that is the command that
/// leaves the ground, so counting from it is counting the time the player is
/// not standing. The jump input is released immediately afterwards, because
/// `holding_jump_does_not_re_trigger_it` shows a held jump is a different
/// experiment.
fn measure_jump(profile: &PhysicsProfile, ms: u16) -> Jump {
    let world = FlatGround::at(s(0.0));
    let rest = settled(&world, profile);
    let rest_z = rest.player.origin.z;

    let mut st = step(&rest, &jump_cmd(ms), &world, profile);
    assert!(
        !st.player.ground.is_grounded(),
        "premise: the jump left the ground"
    );

    let mut apex = st.player.origin.z;
    let mut commands: u32 = 1;
    while !st.player.ground.is_grounded() {
        st = step(&st, &still(ms), &world, profile);
        commands += 1;
        if st.player.origin.z > apex {
            apex = st.player.origin.z;
        }
        assert!(commands < 10_000, "never came back down");
    }

    Jump {
        apex: apex - rest_z,
        airborne_ms: commands * ms as u32,
        commands,
    }
}

/// The number Part 3 publishes. Printed as well as asserted, because a
/// measurement nobody can read is not evidence.
#[test]
fn the_canonical_jump_measured_at_125hz() {
    for (name, profile) in [
        ("vq3", PhysicsProfile::vq3()),
        ("cpm", PhysicsProfile::cpm()),
    ] {
        assert_eq!(profile.jump_velocity, s(270.0));
        assert_eq!(profile.gravity, s(800.0));

        let j = measure_jump(&profile, MS);
        let closed_form_apex = s(270.0) * s(270.0) / (s(2.0) * s(800.0));
        let closed_form_ms = s(2.0) * s(270.0) / s(800.0) * s(1000.0);

        println!(
            "{name} @125Hz: apex {:.6} units over {} ms ({} commands); \
             closed form says {:.6} units over {:.1} ms",
            j.apex, j.airborne_ms, j.commands, closed_form_apex, closed_form_ms
        );

        // The apex lands on the closed form to within a hundredth of a unit.
        // Published to three decimals in canon Part 3 as 45.562.
        assert!(
            (j.apex - closed_form_apex).abs() < s(0.01),
            "{name}: measured apex {} against closed form {closed_form_apex}",
            j.apex
        );

        // The airborne time is *quantised to the command grid* and the closed
        // form is not observable directly: the player is still airborne at the
        // end of command 84 and grounded at the end of command 85, so the
        // continuous landing sits in (672, 680] ms. 675 falls inside that
        // bracket, which is the sense in which `coil.map`'s 0.675 s is right.
        assert_eq!(j.commands, 85, "{name}: landing command moved");
        assert_eq!(j.airborne_ms, 680, "{name}: airborne duration moved");
        assert!(
            closed_form_ms > s(j.airborne_ms as f32 - MS as f32)
                && closed_form_ms <= s(j.airborne_ms as f32),
            "{name}: the closed form left the measured bracket"
        );
    }
}

/// Whether the measured jump depends on the command rate — the claim
/// `step.rs`'s averaged-gravity integrator makes for itself, tested rather
/// than repeated.
#[test]
fn the_jump_is_nearly_independent_of_the_command_rate() {
    let profile = PhysicsProfile::vq3();
    let mut apexes = Vec::new();
    for ms in [4u16, 8, 16] {
        let j = measure_jump(&profile, ms);
        println!(
            "vq3 @{}Hz ({ms} ms): apex {:.6} units over {} ms",
            1000 / ms as u32,
            j.apex,
            j.airborne_ms
        );
        apexes.push(j.apex);
    }

    // "Nearly independent" turns out to understate it: the apex is *identical*
    // across 250, 125 and 62.5 Hz, and only the airborne duration moves, by
    // the width of one command. That is the averaged-gravity integrator's own
    // claim in `step.rs`, measured rather than repeated.
    for a in &apexes {
        assert!(
            (*a - apexes[0]).abs() < s(0.001),
            "apex moved with the command rate: {apexes:?}"
        );
    }
}

/// Guard against `Vec3` being unused if the file is trimmed later.
#[allow(dead_code)]
fn _unused(_: Vec3) {}
