//! Criterion 1, item 2: the technique timing windows, in whole milliseconds.
//!
//! # Why milliseconds and not commands
//!
//! Every timer in the simulation is an integer millisecond
//! ([`straf3_sim::Timers`]), and every window is therefore counted down in
//! whole-command increments — 8 ms at 125 Hz. A window's *constant* and a
//! window's *usable* length are consequently not the same number, and the
//! difference is a property of the tick rate rather than of the mechanic. Both
//! are reported.
//!
//! # What is a window and what is not
//!
//! Three of the four things measured here are genuine windows: a boost is
//! available for a bounded time and then is not. The circle jump is not — its
//! cost is a wind-up, not a moment — and saying so is more useful than
//! inventing a tolerance for it. See the prose in the report.

use straf3_sim::num::{Scalar, s, vec3};
use straf3_sim::world::World;
use straf3_sim::{PhysicsProfile, SimState, UserCmd, step};

use crate::dataset::{Measurement, Section, Table};
use crate::geometry;
use crate::harness::{
    Axis, HZ, MS, holding, jump, running_at, settle_on, still, strafe_for, strafe_once, yaw_for,
};
use crate::measure::{pad, profiles};
use crate::num::horizontal_speed;

/// Speeds the ground-friction window is measured at.
const LANDING_SPEEDS: &[f32] = &[320.0, 500.0, 800.0, 1200.0];

/// Fractions of speed whose loss the ground window is reported against.
const LOSS_FRACTIONS: &[(u32, f32)] = &[(1, 0.01), (5, 0.05), (10, 0.10)];

/// The most commands a search will wait before declaring a window closed.
const SEARCH_CAP: usize = 400;

pub(crate) fn measure() -> Section {
    let mut section = Section::new("2. Technique timing windows");
    let world = geometry::floor();

    let mut dj = Table::new(
        "**Double jump.** `double_jump_window_ms` is the constant; `usable delay` \
         is the largest measured gap between the landing command and a jump that \
         is still boosted.",
        &[
            "profile",
            "constant",
            "armed on landing",
            "usable delay",
            "plain jump",
            "boosted jump",
            "window spent by one jump",
        ],
    );
    let mut ground = Table::new(
        "**The bunnyhop window.** Milliseconds a landing player may spend on the \
         ground before ground friction has taken this fraction of their speed. \
         No input is held: holding forward changes nothing above 320 ups, \
         because `PM_Accelerate`'s clamp has already closed.",
        &[
            "profile",
            "landing speed",
            "−1%",
            "−5%",
            "−10%",
            "speed after one command",
        ],
    );
    let mut circle = Table::new(
        "**Circle jump.** A standing start, turning by the stated angle every \
         command while holding one axis, then jumping. `exit` is horizontal \
         speed on the command the jump is taken.",
        &[
            "profile",
            "axis",
            "best turn/command",
            "ground terminal",
            "wind-up to 90%",
            "wind-up to 99%",
            "exit speed",
            "over a straight run",
        ],
    );

    for (name, profile) in profiles() {
        // ── the double jump ───────────────────────────────────────────────
        let landed = land_from_jump(&world, &profile);
        let armed = landed.player.timers.double_jump_ms;
        let plain_vz = profile.jump_velocity - profile.gravity * s(f32::from(MS) * 0.001);
        let boosted_vz = plain_vz + profile.double_jump_boost;
        // Halfway between the two: anything above it took the boost, and there
        // is nothing in between for it to be confused with.
        let boosted_if_above = (plain_vz + boosted_vz) * s(0.5);

        let mut usable_commands: Option<usize> = None;
        for k in 0..=SEARCH_CAP {
            let waited = wait(&landed, &world, &profile, k);
            let jumped = step(&waited, &jump(), &world, &profile);
            if jumped.player.velocity.z > boosted_if_above {
                usable_commands = Some(k);
            } else if usable_commands.is_some() {
                break; // the window closed; no need to keep looking
            } else if k * usize::from(MS) > 2_000 {
                break; // never opened, and two seconds is long enough to say so
            }
        }
        let available = usable_commands.is_some();
        let usable_ms = usable_commands.unwrap_or(0) as u32 * u32::from(MS);

        // A window buys exactly one boosted jump, not a boosted state.
        let first = step(&landed, &jump(), &world, &profile);
        let spent = first.player.timers.double_jump_ms == 0;

        let key = format!("{name}.window.double_jump");
        section.record(Measurement::ms(
            format!("{key}.constant_ms"),
            u32::from(profile.double_jump_window_ms),
        ));
        section.record(Measurement::ms(format!("{key}.armed_on_landing_ms"), u32::from(armed)));
        section.record(Measurement::flag(format!("{key}.available"), available));
        section.record(Measurement::ms(format!("{key}.usable_delay_ms"), usable_ms));
        section.record(Measurement::ups(
            format!("{key}.plain_jump_vz"),
            first_jump_vz(&landed, &world, &profile),
        ));
        section.record(Measurement::ups(
            format!("{key}.boosted_jump_vz"),
            first.player.velocity.z,
        ));
        section.record(Measurement::flag(format!("{key}.spent_by_one_jump"), spent));

        dj.push(vec![
            name.to_string(),
            format!("{} ms", profile.double_jump_window_ms),
            format!("{armed} ms"),
            if available {
                format!("{usable_ms} ms")
            } else {
                "—".to_string()
            },
            format!("{:.2}", first_jump_vz(&landed, &world, &profile)),
            format!("{:.2}", first.player.velocity.z),
            if available {
                if spent { "yes" } else { "NO" }.to_string()
            } else {
                "—".to_string()
            },
        ]);

        // ── the jump re-arm edge ──────────────────────────────────────────
        let rearm = rearm_release_ms(&world, &profile);
        section.record(Measurement::flag(
            format!("{name}.window.jump_rearm.requires_release"),
            rearm.is_some(),
        ));
        section.record(Measurement::ms(
            format!("{name}.window.jump_rearm.release_ms"),
            rearm.unwrap_or(0),
        ));
        section.record(Measurement::flag(
            format!("{name}.window.jump_rearm.held_jump_ever_refires"),
            held_jump_refires(&world, &profile),
        ));

        // ── the ground (bunnyhop) window ──────────────────────────────────
        for &speed in LANDING_SPEEDS {
            let start = running_at(&world, &profile, s(speed));
            let key = format!("{name}.window.ground.entry{}", pad(speed as u32, 4));
            let mut row = vec![name.to_string(), format!("{speed:.0}")];
            for &(percent, fraction) in LOSS_FRACTIONS {
                let ms = ms_to_lose(&world, &profile, &start, s(fraction));
                section.record(Measurement::ms(
                    format!("{key}.lose{}pct_ms", pad(percent, 2)),
                    ms,
                ));
                row.push(format!("{ms} ms"));
            }
            let one = step(&start, &still(), &world, &profile);
            let after = horizontal_speed(one.player.velocity);
            section.record(Measurement::ups(format!("{key}.after_one_command"), after));
            row.push(format!("{after:.2}"));
            ground.push(row);
        }

        // ── the circle jump ───────────────────────────────────────────────
        let straight = straight_run_exit(&world, &profile);
        section.record(Measurement::ups(
            format!("{name}.window.straight_run.exit_ups"),
            straight,
        ));
        for axis in [Axis::Forward, Axis::Strafe] {
            let cj = circle_jump(&world, &profile, axis, straight);
            let key = format!("{name}.window.circle_jump.{}", axis.key());
            section.record(Measurement::degrees(
                format!("{key}.best_turn_per_command"),
                cj.angle,
            ));
            section.record(Measurement::ups(
                format!("{key}.ground_terminal_ups"),
                cj.ground_terminal,
            ));
            section.record(Measurement::ms(format!("{key}.windup_90pct_ms"), cj.windup_90));
            section.record(Measurement::ms(format!("{key}.windup_99pct_ms"), cj.windup_99));
            section.record(Measurement::ups(format!("{key}.exit_ups"), cj.exit));
            section.record(Measurement::ups(
                format!("{key}.gain_over_straight_ups"),
                cj.exit - straight,
            ));
            circle.push(vec![
                name.to_string(),
                axis.key().to_string(),
                format!("{:.0}°", cj.angle),
                format!("{:.2}", cj.ground_terminal),
                format!("{} ms", cj.windup_90),
                format!("{} ms", cj.windup_99),
                format!("{:.2}", cj.exit),
                format!("{:+.2}", cj.exit - straight),
            ]);
        }
    }

    section.say(
        "Every number here is taken at 125 Hz, where a command is 8 ms. A window \
         is counted down once per command, so a *usable* window is the constant \
         rounded down to a whole number of commands and then reduced by however \
         many of them the landing itself consumed. That difference is a property \
         of the command rate, not of the mechanic: at 250 Hz the same constant \
         yields a different usable window, and a player who changes `com_maxfps` \
         changes it.",
    );
    section.table(dj);
    section.say(
        "**The jump re-arm is an edge, not a timer.** Holding the jump input \
         across a landing never fires a second jump, however long it is held — \
         `PmoveSingle` clears `PMF_JUMP_HELD` only on a command that does not \
         ask to jump. The measured release is one command, which is the shortest \
         thing the command stream can express; there is no tolerance to tune and \
         no window to widen. This is what makes bunnyhop timing a skill rather \
         than a hold.",
    );
    section.table(ground);
    section.say(
        "The bunnyhop window is the one above, and it is not a constant anywhere \
         in the code: it falls out of `pm_friction` = 6 against `pm_stopspeed` = \
         100. Above `stop_speed` the loss is proportional, so the *fraction* lost \
         in a given time is the same at every landing speed while the absolute \
         loss is not — which is why a fast player feels the same window as a slow \
         one but pays far more for missing it.",
    );
    section.table(circle);
    section.say(
        "**The circle jump has no timing window at the jump itself.** Once the \
         ground turn has reached its terminal speed, the command the jump is \
         taken on changes the direction of the exit and not its speed, so there \
         is no moment to hit. What it costs is the wind-up in the two columns \
         above — that is the number a route planner needs, and calling it a \
         window would have invented a tolerance that does not exist.",
    );

    section
}

/// Settle, jump, and step until the landing command — returning the state at
/// the end of the command on which the player became grounded again.
fn land_from_jump<W: World>(world: &W, profile: &PhysicsProfile) -> SimState {
    let base = settle_on(world, profile, vec3(s(0.0), s(0.0), s(64.0)));
    let mut st = step(&base, &jump(), world, profile);
    for _ in 0..SEARCH_CAP {
        if st.player.ground.is_grounded() {
            return st;
        }
        st = step(&st, &still(), world, profile);
    }
    panic!("a jump on flat ground never landed");
}

/// `k` still commands.
fn wait<W: World>(st: &SimState, world: &W, profile: &PhysicsProfile, k: usize) -> SimState {
    let mut out = *st;
    for _ in 0..k {
        out = step(&out, &still(), world, profile);
    }
    out
}

/// The vertical velocity of an ordinary, unboosted jump from rest.
fn first_jump_vz<W: World>(landed: &SimState, world: &W, profile: &PhysicsProfile) -> Scalar {
    // Waiting out any window first, so this really is the plain number even
    // under a profile that has one.
    let cold = wait(landed, world, profile, 200);
    step(&cold, &jump(), world, profile).player.velocity.z
}

/// How many milliseconds the jump input must be released before it re-arms, or
/// `None` if a held jump re-fires on its own.
fn rearm_release_ms<W: World>(world: &W, profile: &PhysicsProfile) -> Option<u32> {
    let landed = land_from_jump(world, profile);
    // Land with the jump input still down — which is the state a player who
    // never let go is in — then release for `k` commands and press again.
    let held = step(&landed, &jump(), world, profile);
    let grounded = {
        let mut st = held;
        for _ in 0..SEARCH_CAP {
            if st.player.ground.is_grounded() {
                break;
            }
            st = step(&st, &jump(), world, profile);
        }
        st
    };
    for k in 0..=8 {
        let released = wait(&grounded, world, profile, k);
        let again = step(&released, &jump(), world, profile);
        if again.player.velocity.z > profile.jump_velocity * s(0.5) {
            return Some(k as u32 * u32::from(MS));
        }
    }
    None
}

/// Whether holding jump across a landing ever fires a second jump on its own.
fn held_jump_refires<W: World>(world: &W, profile: &PhysicsProfile) -> bool {
    let base = settle_on(world, profile, vec3(s(0.0), s(0.0), s(64.0)));
    let mut st = step(&base, &jump(), world, profile);
    let mut landed_once = false;
    for _ in 0..SEARCH_CAP {
        let before = st.player.velocity.z;
        st = step(&st, &jump(), world, profile);
        if st.player.ground.is_grounded() {
            landed_once = true;
        }
        // A second jump would show as vertical velocity appearing from nowhere
        // after the player had already come down.
        if landed_once && st.player.velocity.z > before + profile.jump_velocity * s(0.5) {
            return true;
        }
    }
    false
}

/// Milliseconds of ground contact before `fraction` of the entry speed is gone.
fn ms_to_lose<W: World>(
    world: &W,
    profile: &PhysicsProfile,
    start: &SimState,
    fraction: Scalar,
) -> u32 {
    let entry = horizontal_speed(start.player.velocity);
    let floor_speed = entry * (s(1.0) - fraction);
    let mut st = *start;
    for command in 0..SEARCH_CAP {
        if horizontal_speed(st.player.velocity) <= floor_speed {
            return command as u32 * u32::from(MS);
        }
        st = step(&st, &still(), world, profile);
    }
    SEARCH_CAP as u32 * u32::from(MS)
}

/// What a circle jump is worth, and what it costs to wind up.
struct CircleJump {
    angle: Scalar,
    ground_terminal: Scalar,
    windup_90: u32,
    windup_99: u32,
    exit: Scalar,
}

/// Horizontal speed leaving the ground after a straight run-up — the baseline a
/// circle jump is measured against.
fn straight_run_exit<W: World>(world: &W, profile: &PhysicsProfile) -> Scalar {
    let base = settle_on(world, profile, vec3(s(0.0), s(0.0), s(64.0)));
    let forward = holding(Axis::Forward, s(0.0));
    let mut st = base;
    for _ in 0..(HZ * 4) {
        st = step(&st, &forward, world, profile);
    }
    let jumped = step(
        &st,
        &UserCmd {
            buttons: straf3_sim::Buttons::JUMP,
            ..forward
        },
        world,
        profile,
    );
    horizontal_speed(jumped.player.velocity)
}

/// Sweep the turn rate, take the best, and report what it cost to reach.
fn circle_jump<W: World>(
    world: &W,
    profile: &PhysicsProfile,
    axis: Axis,
    _straight: Scalar,
) -> CircleJump {
    let base = settle_on(world, profile, vec3(s(0.0), s(0.0), s(64.0)));

    // 1° per command is 125°/s, and 89 is a full quarter turn every three
    // commands. Anything outside that is not a turn a hand makes.
    let mut best = (s(1.0), Scalar::NEG_INFINITY);
    for whole in 1..=89 {
        let angle = s(whole as f32);
        let end = strafe_for(world, profile, &base, angle, axis, HZ * 3);
        let speed = horizontal_speed(end.player.velocity);
        if speed > best.1 {
            best = (angle, speed);
        }
    }
    let angle = best.0;

    // Wind-up: how long until the ground turn is within 10% and 1% of where it
    // ends up. Sampled per command, because that is the resolution a player's
    // route has.
    let terminal = best.1;
    let (mut w90, mut w99) = (None, None);
    let mut st = base;
    for command in 0..(HZ * 3) {
        let speed = horizontal_speed(st.player.velocity);
        if w90.is_none() && speed >= terminal * s(0.90) {
            w90 = Some(command as u32 * u32::from(MS));
        }
        if w99.is_none() && speed >= terminal * s(0.99) {
            w99 = Some(command as u32 * u32::from(MS));
            break;
        }
        st = strafe_once(world, profile, &st, angle, axis);
    }

    // The exit: one more command of the same turn, with jump pressed.
    let wound = strafe_for(world, profile, &base, angle, axis, HZ * 3);
    let want = crate::num::heading_degrees(wound.player.velocity) + angle;
    let exit_cmd = UserCmd {
        buttons: straf3_sim::Buttons::JUMP,
        ..holding(axis, yaw_for(axis, want))
    };
    let exit = step(&wound, &exit_cmd, world, profile);

    CircleJump {
        angle,
        ground_terminal: terminal,
        windup_90: w90.unwrap_or(0),
        windup_99: w99.unwrap_or((HZ * 3) as u32 * u32::from(MS)),
        exit: horizontal_speed(exit.player.velocity),
    }
}
