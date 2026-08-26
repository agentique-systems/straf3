//! Criterion 1, item 1: the strafe speed-gain curve against entry speed.
//!
//! # What is being measured
//!
//! The technique is "hold the wish direction a fixed angle off your current
//! velocity". `PM_Accelerate` grants `accel · dt · wishspeed` along the wish
//! direction, but only up to `wishspeed − dot(velocity, wishdir)` — the clamp
//! is on the *projection*, not on the total. So the speed at which the clamp
//! closes is `wishspeed / cos(angle)`, and that is the technique's ceiling at
//! that angle. VQ3's `wishspeed` is `max_speed` (320); CPM's, on the pure
//! strafe axis, is `strafe_wish_speed_cap` (30) with a 70× larger acceleration
//! behind it.
//!
//! Both halves of that are checked here against the closed form, because a
//! curve that merely rises would be produced by several wrong implementations
//! and by one right one.

use straf3_sim::PhysicsProfile;
use straf3_sim::num::{Scalar, s};
use straf3_sim::world::EmptyWorld;

use crate::dataset::{Measurement, Section, Table};
use crate::harness::{Axis, HZ, flying_at, gain_per_second, optimal_angle, strafe_for};
use crate::measure::pad;
use crate::num::{cos_degrees, horizontal_speed};

/// Entry speeds swept, in units per second.
///
/// From the ground cap (320) to well past what any `coil` run has reached
/// (648 measured by a human this wave), because the interesting part of the
/// curve — where the optimal angle swings towards 90° and the gain collapses —
/// is above the speeds anyone has played at.
const ENTRY_SPEEDS: &[f32] = &[320.0, 400.0, 500.0, 640.0, 800.0, 1000.0, 1300.0, 1600.0];

/// Held angles swept, in degrees.
///
/// Denser between 25° and 50°, which is where the VQ3 optimum lives at playable
/// speeds, and carrying 0° and 90° because both are degenerate in ways worth
/// having on the record: 0° gains nothing, and 90° gains nothing for the
/// opposite reason.
const ANGLES: &[f32] = &[
    0.0, 10.0, 20.0, 25.0, 30.0, 35.0, 40.0, 45.0, 50.0, 60.0, 75.0, 85.0, 90.0,
];

/// The angles the human-readable table shows. The machine-readable section
/// carries all of [`ANGLES`]; a table with thirteen numeric columns is not
/// read by anybody.
const SHOWN: &[f32] = &[10.0, 20.0, 30.0, 40.0, 50.0, 60.0, 75.0, 85.0];

/// How long a terminal-speed search runs before it is called unbounded.
const TERMINAL_SECONDS: usize = 120;

pub(crate) fn measure(profiles: &[(&str, PhysicsProfile)]) -> Section {
    let mut section = Section::new("1. Strafe speed-gain curve against entry speed");
    section.say(
        "Every run below starts an airborne player at the stated entry speed in \
         empty space and holds the wish direction a fixed angle off the current \
         velocity, re-aimed every command. Gain is measured over exactly one \
         second (125 commands at 125 Hz). Nothing but the air rules can touch \
         the result: there is no ground, no ceiling and no geometry.",
    );
    section.say(
        "`forward` is Quake's `movementDir` 0 — the axis VQ3 strafejumps on. \
         `strafe` is `movementDir` 6, the axis CPM's `strafe_accelerate` / \
         `strafe_wish_speed_cap` pair is written for. The two are separate \
         techniques with separate ceilings, and a player uses both.",
    );

    let world = EmptyWorld;

    for (name, profile) in profiles {
        let profile = *profile;
        for axis in [Axis::Forward, Axis::Strafe] {
            let mut gains = Table::with_headers(
                format!(
                    "**{name} / {}** — speed gained per second of held strafe (ups/s).",
                    axis.key()
                ),
                {
                    let mut h = vec!["entry ups".to_string()];
                    h.extend(SHOWN.iter().map(|a| format!("{a:.0}°")));
                    h
                },
            );
            let mut summary = Table::new(
                format!(
                    "**{name} / {}** — best held angle (searched at 1°) and where it ends up.",
                    axis.key()
                ),
                &[
                    "entry ups",
                    "optimal angle",
                    "gain at optimum",
                    "terminal speed",
                    "time to terminal",
                    "closed form",
                ],
            );

            for &entry in ENTRY_SPEEDS {
                let start = flying_at(s(entry));
                let entry_key =
                    format!("{name}.strafe.{}.entry{}", axis.key(), pad(entry as u32, 4));

                let mut row = vec![format!("{entry:.0}")];
                for &angle in ANGLES {
                    let gain = gain_per_second(&world, &profile, &start, s(angle), axis);
                    section.record(Measurement::ups(
                        format!("{entry_key}.angle{}.gain_per_s", pad(angle as u32, 2)),
                        gain,
                    ));
                    if SHOWN.contains(&angle) {
                        row.push(format!("{gain:.2}"));
                    }
                }
                gains.push(row);

                let (best_angle, best_gain) = optimal_angle(&world, &profile, &start, axis);
                section.record(Measurement::degrees(
                    format!("{entry_key}.optimal_angle"),
                    best_angle,
                ));
                section.record(Measurement::ups(
                    format!("{entry_key}.optimal_gain_per_s"),
                    best_gain,
                ));

                // The ceiling of the technique *at that angle*, run out until it
                // stops moving. Deliberately not the ceiling over all angles: a
                // player holds one angle, and the number they can use is what
                // holding it is worth.
                let (terminal, seconds) = settle(&world, &profile, &start, best_angle, axis);
                section.record(Measurement::ups(
                    format!("{entry_key}.terminal_ups"),
                    terminal,
                ));
                section.record(Measurement::ms(
                    format!("{entry_key}.terminal_time_ms"),
                    (seconds * 1000) as u32,
                ));
                section.record(Measurement::flag(
                    format!("{entry_key}.terminal_converged"),
                    seconds < TERMINAL_SECONDS,
                ));

                let closed = closed_form(&profile, axis, best_angle);
                section.record(Measurement::ups(
                    format!("{entry_key}.closed_form_cap"),
                    closed,
                ));

                summary.push(vec![
                    format!("{entry:.0}"),
                    format!("{best_angle:.0}°"),
                    format!("{best_gain:.2}"),
                    format!("{terminal:.2}"),
                    if seconds < TERMINAL_SECONDS {
                        format!("{} s", seconds)
                    } else {
                        format!("> {TERMINAL_SECONDS} s")
                    },
                    format!("{closed:.2}"),
                ]);
            }

            section.table(gains);
            section.table(summary);
        }
    }

    section.say(
        "**The closed form.** `PM_Accelerate` stops granting speed when \
         `dot(velocity, wishdir)` reaches `wishspeed`, so a held angle θ caps at \
         `wishspeed / cos θ`. The `closed form` column is that expression \
         evaluated at the optimal angle, and the `terminal speed` column is what \
         the simulation actually reached. Where they agree, the mechanism is the \
         one the source describes. Where they do not, the run had not converged \
         inside the cap — read the `time to terminal` column before reading the \
         disagreement as a defect.",
    );

    section
}

/// The wish speed `PM_Accelerate` is clamped against for this profile and axis.
///
/// This is the whole of the closed form, and it is the one place the report
/// asserts a mechanism rather than reporting a number: VQ3 clamps against
/// `max_speed`; CPM on the pure-strafe axis clamps against
/// `strafe_wish_speed_cap` instead, which is why its ceiling at a given angle
/// is *lower* than VQ3's while its gain rate is far higher.
fn closed_form(profile: &straf3_sim::PhysicsProfile, axis: Axis, angle: Scalar) -> Scalar {
    let wishspeed = if axis == Axis::Strafe && profile.strafe_accelerate != s(0.0) {
        profile.strafe_wish_speed_cap
    } else {
        profile.max_speed
    };
    let c = cos_degrees(angle);
    if c <= s(0.0) {
        // 90° exactly: the cap is unbounded and the gain is zero, so there is
        // no finite number to print. Reported as zero with the flag beside it
        // rather than as an infinity that would render as `inf` in a table.
        s(0.0)
    } else {
        wishspeed / c
    }
}

/// Run the held angle out until the speed stops changing, in whole seconds.
///
/// Returns the speed and how many seconds it took; `TERMINAL_SECONDS` means it
/// was still climbing when the search gave up, which is a result and not a
/// failure — CPM near 90° genuinely has a ceiling nobody reaches.
fn settle<W: straf3_sim::World>(
    world: &W,
    profile: &straf3_sim::PhysicsProfile,
    start: &straf3_sim::SimState,
    angle: Scalar,
    axis: Axis,
) -> (Scalar, usize) {
    let mut st = *start;
    let mut last = horizontal_speed(st.player.velocity);
    for elapsed in 1..=TERMINAL_SECONDS {
        st = strafe_for(world, profile, &st, angle, axis, HZ);
        let now = horizontal_speed(st.player.velocity);
        if (now - last).abs() < s(0.01) {
            return (now, elapsed);
        }
        last = now;
    }
    (last, TERMINAL_SECONDS)
}
