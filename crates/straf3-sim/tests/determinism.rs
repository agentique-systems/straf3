//! Determinism: the property every other feature is downstream of.
//!
//! Spec section 4 scopes it as *same binary, same machine, bit-identical*.
//! These tests hold that line at three levels:
//!
//! 1. within one process, repeated runs (`identical_input_produces_identical_bits`);
//! 2. across independent OS processes, via the headless runner
//!    (`a_fresh_process_reproduces_the_same_run`) — the one that would catch a
//!    dependency on address-space layout, hash seeding or environment;
//! 3. sensitivity: the checks would actually notice a change
//!    (`a_single_changed_input_bit_changes_the_result`).
//!
//! These live in `tests/` rather than `src/` for the same reason the runner
//! lives in `bin/`: a determinism test wants to spawn a process and write a
//! temporary file, and nothing under `src/` is permitted to do either.

use std::io::Write as _;
use std::process::Command;

use straf3_sim::num::{s, vec3};
use straf3_sim::world::{EmptyWorld, FlatGround};
use straf3_sim::{Buttons, PhysicsProfile, SimState, TickRate, UserCmd, ViewAngles, run, step};

/// A run long and varied enough that a divergence would have room to grow:
/// a fall, a landing, turning while accelerating, and a jump.
fn interesting_commands(rate: TickRate) -> Vec<UserCmd> {
    let ms = rate.command_millis();
    let mut cmds = Vec::new();
    for i in 0..2_000u32 {
        // Angles that are not representable exactly in binary, so any sloppy
        // rounding anywhere has something to bite on.
        let yaw = s(i as f32) * s(0.37) + s(0.1);
        cmds.push(UserCmd {
            duration_ms: ms,
            forward_move: 127,
            right_move: if i % 2 == 0 { 127 } else { -127 },
            up_move: 0,
            buttons: if i % 97 == 0 {
                Buttons::JUMP
            } else {
                Buttons::NONE
            },
            view: ViewAngles {
                pitch: s(-3.3),
                yaw,
                roll: s(0.0),
            },
        });
    }
    cmds
}

fn spawn_state() -> SimState {
    SimState::spawned_at(vec3(s(0.0), s(0.0), s(200.0)), s(90.0))
}

#[test]
fn identical_input_produces_identical_bits() {
    let rate = TickRate::HZ_125;
    let cmds = interesting_commands(rate);
    let world = FlatGround::at(s(0.0));
    let profile = PhysicsProfile::cpm();

    let a = run(&spawn_state(), &cmds, &world, &profile);
    let b = run(&spawn_state(), &cmds, &world, &profile);

    // Checksum first: it names the failure. Then the full state, so a bug in
    // the checksum itself cannot make this test vacuous.
    assert_eq!(a.checksum(), b.checksum(), "checksums diverged");
    assert_eq!(a, b, "states diverged");
    assert_eq!(a.tick, 2_000);
    assert_eq!(a.time_ms, 2_000 * 8);
}

#[test]
fn determinism_holds_for_every_profile_and_rate() {
    for rate in [TickRate::HZ_76, TickRate::HZ_125, TickRate::HZ_250] {
        for profile in [PhysicsProfile::vq3(), PhysicsProfile::cpm()] {
            let cmds = interesting_commands(rate);
            let world = FlatGround::at(s(0.0));
            let a = run(&spawn_state(), &cmds, &world, &profile);
            let b = run(&spawn_state(), &cmds, &world, &profile);
            assert_eq!(a.checksum(), b.checksum(), "diverged at {} Hz", rate.hz());
        }
    }
}

#[test]
fn stepping_one_at_a_time_matches_running_the_whole_sequence() {
    // `run` must be exactly the loop it claims to be. If these ever differ,
    // a replay verified with one and recorded with the other would disagree.
    let rate = TickRate::HZ_125;
    let cmds = interesting_commands(rate);
    let world = FlatGround::at(s(0.0));
    let profile = PhysicsProfile::cpm();

    let bulk = run(&spawn_state(), &cmds, &world, &profile);

    let mut one_by_one = spawn_state();
    for cmd in &cmds {
        one_by_one = step(&one_by_one, cmd, &world, &profile);
    }

    assert_eq!(bulk.checksum(), one_by_one.checksum());
}

#[test]
fn a_single_changed_input_bit_changes_the_result() {
    // Guards against the tests above passing for the boring reason that the
    // simulation ignores its input.
    //
    // The perturbation is a command *duration*, because that is the input the
    // placeholder physics genuinely consumes today. See
    // `movement_axes_are_still_ignored_by_the_placeholder` below for the
    // stronger version of this test, and why it cannot be written yet.
    let rate = TickRate::HZ_125;
    let world = FlatGround::at(s(0.0));
    let profile = PhysicsProfile::cpm();

    let cmds = interesting_commands(rate);
    let mut perturbed = cmds.clone();
    perturbed[500].duration_ms += 1;

    let a = run(&spawn_state(), &cmds, &world, &profile);
    let b = run(&spawn_state(), &perturbed, &world, &profile);
    assert_ne!(a.checksum(), b.checksum());
}

/// A tripwire, not a specification.
///
/// The placeholder [`step`] integrates velocity under gravity and does not yet
/// read the movement axes or the view angles, so perturbing them mid-run
/// changes nothing. That means the sensitivity check above is currently weaker
/// than it should be — it can only perturb a duration.
///
/// This test asserts that limitation explicitly so it is impossible to forget.
/// **When Wave 2 implements acceleration, this test will start failing.** That
/// is the intended signal: delete it, and strengthen
/// `a_single_changed_input_bit_changes_the_result` to perturb `view.yaw` at
/// command 500 instead, which is the real property — a strafejump run must
/// diverge from a one-bit change in a mouse angle.
#[test]
fn movement_axes_are_still_ignored_by_the_placeholder() {
    let world = FlatGround::at(s(0.0));
    let profile = PhysicsProfile::cpm();
    let cmds = interesting_commands(TickRate::HZ_125);

    let mut perturbed = cmds.clone();
    let yaw = perturbed[500].view.yaw;
    perturbed[500].view.yaw = f32::from_bits(yaw.to_bits() + 1);
    perturbed[500].forward_move = -127;
    perturbed[500].right_move = 0;

    let a = run(&spawn_state(), &cmds, &world, &profile);
    let b = run(&spawn_state(), &perturbed, &world, &profile);
    assert_eq!(
        a.checksum(),
        b.checksum(),
        "movement input now affects the simulation — Wave 2 has landed. \
         Delete this test and strengthen a_single_changed_input_bit_changes_the_result \
         to perturb view.yaw."
    );
}

#[test]
fn the_profile_is_part_of_the_result() {
    // VQ3 and CPM are two values of one struct, so a run is only reproducible
    // if the profile is recorded alongside it. Prove the profile actually
    // reaches the physics rather than being a decorative argument.
    let world = FlatGround::at(s(0.0));
    let cmds = interesting_commands(TickRate::HZ_125);

    let heavy = PhysicsProfile {
        gravity: s(1600.0),
        ..PhysicsProfile::cpm()
    };
    let normal = run(&spawn_state(), &cmds, &world, &PhysicsProfile::cpm());
    let fast = run(&spawn_state(), &cmds, &world, &heavy);
    assert_ne!(normal.checksum(), fast.checksum());
}

#[test]
fn the_world_is_part_of_the_result() {
    // Same, for the collision seam: the trait implementor must be reachable
    // from the physics, or the seam is decorative.
    let cmds = interesting_commands(TickRate::HZ_125);
    let profile = PhysicsProfile::cpm();
    let on_ground = run(&spawn_state(), &cmds, &FlatGround::at(s(0.0)), &profile);
    let in_void = run(&spawn_state(), &cmds, &EmptyWorld, &profile);
    assert_ne!(on_ground.checksum(), in_void.checksum());
    assert!(on_ground.player.ground.is_grounded());
    assert!(!in_void.player.ground.is_grounded());
}

/// The strongest check available on one machine: two independent OS processes.
///
/// Within a process, a hidden shared cache would still produce matching
/// results. Across processes it would not, and neither would a dependency on
/// a randomised hash seed, on address-space layout, or on the environment.
/// This is the closest thing to the guarantee a replay actually needs.
#[test]
fn a_fresh_process_reproduces_the_same_run() {
    let exe = env!("CARGO_BIN_EXE_straf3-headless");

    // Written to the target directory rather than a fixed /tmp path so
    // parallel test runs cannot collide, and so `cargo clean` removes it.
    let dir =
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../target/straf3-sim-tests");
    std::fs::create_dir_all(&dir).expect("create test scratch dir");
    let input_path = dir.join("determinism-fresh-process.straf3in");

    let mut f = std::fs::File::create(&input_path).expect("write fixture");
    writeln!(
        f,
        "rate 125\nprofile cpm\nworld flat 0\nspawn 0 0 200\nyaw 90"
    )
    .unwrap();
    for i in 0..500 {
        writeln!(
            f,
            "cmd 1 127 {} 0 {} 0 {}",
            if i % 2 == 0 { 127 } else { -127 },
            if i % 97 == 0 { "jump" } else { "-" },
            90.0 + f64::from(i) * 0.37
        )
        .unwrap();
    }
    drop(f);

    let checksum_of = || {
        let out = Command::new(exe)
            .arg(&input_path)
            .output()
            .expect("run straf3-headless");
        assert!(
            out.status.success(),
            "runner failed: {}",
            String::from_utf8_lossy(&out.stderr)
        );
        let stdout = String::from_utf8(out.stdout).expect("utf-8 output");
        stdout
            .lines()
            .find_map(|l| l.trim().strip_prefix("checksum"))
            .map(|v| v.trim().to_string())
            .unwrap_or_else(|| panic!("no checksum in output:\n{stdout}"))
    };

    let first = checksum_of();
    let second = checksum_of();
    assert_eq!(first, second, "two processes produced different runs");

    // And it is a real value, not an empty string matching itself.
    assert!(first.starts_with("0x"), "unexpected checksum {first:?}");
    assert_ne!(first, "0x0000000000000000");
}

/// The headless runner must run with no display, no GPU and no ambient
/// graphics environment — spec criterion 7, and the reason WSL2 is a usable
/// build host at all (spec section 2).
#[test]
fn the_headless_runner_needs_no_display() {
    let exe = env!("CARGO_BIN_EXE_straf3-headless");
    let input = "rate 125\nprofile vq3\nworld flat 0\nspawn 0 0 100\ncmd 200 0 0 0 - 0 0\n";

    let dir =
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../target/straf3-sim-tests");
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("headless-no-display.straf3in");
    std::fs::write(&path, input).unwrap();

    let out = Command::new(exe)
        .arg(&path)
        .env_remove("DISPLAY")
        .env_remove("WAYLAND_DISPLAY")
        .env_remove("XDG_RUNTIME_DIR")
        .output()
        .expect("run straf3-headless");

    assert!(
        out.status.success(),
        "runner failed with no display: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(
        stdout.contains("checksum"),
        "no final state printed:\n{stdout}"
    );
    assert!(
        stdout.contains("tick          200"),
        "wrong tick count:\n{stdout}"
    );
}
