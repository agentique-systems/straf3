//! Criterion 1, item 6: the ceiling each technique actually reaches.
//!
//! # Why the angle is fixed and not optimised
//!
//! A pure air technique's ceiling is `wishspeed / cos θ`, which grows without
//! bound as θ approaches 90° while the gain per second falls to nothing. Ask
//! "what is the highest terminal speed over all angles" and the answer is
//! whatever the finest angle the search grid contains happens to be — a property
//! of the grid, not of the physics. So the angles here are **fixed**, the whole
//! row is published, and the reader can see the ceiling rising with the angle
//! rather than being handed one number that came from the edge of a sweep.
//!
//! The techniques with ground contact — the ground run, the ground turn, the
//! bunnyhop — do have real optima, because friction bounds them. Those show up
//! in the row as an interior maximum, which is the point of printing the row.
//!
//! # Why this is not the same as section 1's terminal column
//!
//! Section 1 measures a technique held in empty space: the ceiling of the
//! *mechanism*. This measures the ceiling of the *technique as played* —
//! including the ground contact a bunnyhop needs, the friction that contact
//! costs, and the jump frame's `PM_CmdScale` dip. The gap between them is the
//! price of touching the floor.

use straf3_sim::num::{Scalar, s, vec3};
use straf3_sim::world::{EmptyWorld, World};
use straf3_sim::{Buttons, PhysicsProfile, SimState, UserCmd, step};

use crate::dataset::{Measurement, Section, Table};
use crate::geometry;
use crate::harness::{Axis, HZ, flying_at, holding, settle_on, strafe_once, yaw_for};
use crate::measure::{pad, profiles};
use crate::num::{heading_degrees, horizontal_speed};

/// Seconds a technique is run for before it is declared unsettled.
const CAP_SECONDS: usize = 120;

/// A change of less than this over a whole second counts as settled — one
/// hundredth of a unit per second, which is the resolution speeds print at.
const SETTLED: Scalar = s(0.01);

/// Held angles, in degrees. Fixed; see the module docs.
const ANGLES: &[f32] = &[0.0, 15.0, 30.0, 45.0, 60.0, 75.0, 85.0];

/// The techniques measured, in report order.
const TECHNIQUES: &[(&str, &str)] = &[
    (
        "ground_run",
        "Holding forward on flat ground. The angle does nothing here; the row is \
         flat by construction and is the control.",
    ),
    (
        "ground_turn",
        "Turning by the angle every command while accelerating on the ground — \
         the circle-jump wind-up, run out to its ceiling.",
    ),
    (
        "air_forward",
        "Airborne from 320 ups, holding forward at the angle off the velocity. \
         VQ3's strafejump, and the technique CPM inherits unchanged.",
    ),
    (
        "air_strafe",
        "Airborne from 320 ups, holding one strafe key at the angle. CPM's \
         pure-strafe model; under VQ3 this is the same code path as \
         `air_forward` and the rows agree.",
    ),
    (
        "bunnyhop",
        "The whole technique: strafe in the air, jump on the command the player \
         lands, from a standing start on flat ground.",
    ),
];

pub(crate) fn measure() -> Section {
    let mut section = Section::new("6. Per-technique terminal speed");
    section.say(format!(
        "Each technique is run at each held angle until its horizontal speed \
         stops changing by more than {SETTLED:.2} ups in a second, or for \
         {CAP_SECONDS} seconds. A cell prefixed `>` had not settled: its ceiling \
         is above the number shown, not at it."
    ));

    let mut table = Table::with_headers(
        "**Terminal speed by technique and held angle** (ups).".to_string(),
        {
            let mut h = vec!["profile".to_string(), "technique".to_string()];
            h.extend(ANGLES.iter().map(|a| format!("{a:.0}°")));
            h
        },
    );

    for (name, profile) in profiles() {
        let floor = geometry::floor();

        for (technique, _) in TECHNIQUES {
            let mut row = vec![name.to_string(), (*technique).to_string()];
            for &degrees in ANGLES {
                let angle = s(degrees);
                let result = match *technique {
                    "ground_run" => ground_run(&floor, &profile),
                    "ground_turn" => ground_turn(&floor, &profile, angle),
                    "air_forward" => air_hold(&profile, angle, Axis::Forward),
                    "air_strafe" => air_hold(&profile, angle, Axis::Strafe),
                    "bunnyhop" => bunnyhop(&floor, &profile, angle),
                    other => unreachable!("unknown technique {other}"),
                };

                let key = format!(
                    "{name}.terminal.{technique}.angle{}",
                    pad(degrees as u32, 2)
                );
                section.record(Measurement::ups(format!("{key}.ups"), result.speed));
                section.record(Measurement::ms(format!("{key}.time_ms"), result.time_ms));
                section.record(Measurement::flag(format!("{key}.settled"), result.settled));

                row.push(if result.settled {
                    format!("{:.2}", result.speed)
                } else {
                    format!("> {:.2}", result.speed)
                });
            }
            table.push(row);
        }
    }

    section.table(table);
    for (technique, description) in TECHNIQUES {
        section.say(format!("- **`{technique}`** — {description}"));
    }
    section.say(
        "**What a player can actually reach.** The `bunnyhop` row is the only \
         one describing a technique that can be held on a map: it starts \
         standing, it touches the ground, and it pays friction for every hop. \
         Read the air rows as the ceiling the hop is climbing towards and the \
         bunnyhop row as how close to it a run gets.",
    );
    section.say(
        "**The fastest thing in this tree is not on this table.** Section 4's \
         drop launch converts a fall into horizontal speed in a single command: \
         400 ups off a 1024-unit drop peaks at 1276 ups, and the ceiling of the \
         mechanism is `sqrt(entry² + 2·g·h)`, which has no upper bound but the \
         height of the map. Every technique above needs seconds of held input to \
         reach a few hundred ups over the ground cap; a drop pays more than that \
         instantly and asks for nothing but geometry. Whether that is a mechanic \
         or a defect is a design question this instrument does not answer — but \
         it is the largest number in the movement language, and any argument \
         about the shape of a route has to start from it.",
    );

    section
}

/// How a technique finished.
#[derive(Clone, Copy)]
struct Settled {
    speed: Scalar,
    time_ms: u32,
    settled: bool,
}

/// Run one second's worth of a technique repeatedly until the speed stops
/// changing.
fn settle_out<F>(mut second: F, start: SimState) -> Settled
where
    F: FnMut(&SimState) -> SimState,
{
    let mut st = start;
    let mut last = horizontal_speed(st.player.velocity);
    for elapsed in 1..=CAP_SECONDS {
        st = second(&st);
        let now = horizontal_speed(st.player.velocity);
        if (now - last).abs() < SETTLED {
            return Settled {
                speed: now,
                time_ms: (elapsed * 1000) as u32,
                settled: true,
            };
        }
        last = now;
    }
    Settled {
        speed: last,
        time_ms: (CAP_SECONDS * 1000) as u32,
        settled: false,
    }
}

fn ground_run<W: World>(world: &W, profile: &PhysicsProfile) -> Settled {
    let start = settle_on(world, profile, vec3(s(0.0), s(0.0), s(64.0)));
    let forward = holding(Axis::Forward, s(0.0));
    settle_out(
        |st| {
            let mut out = *st;
            for _ in 0..HZ {
                out = step(&out, &forward, world, profile);
            }
            out
        },
        start,
    )
}

fn ground_turn<W: World>(world: &W, profile: &PhysicsProfile, angle: Scalar) -> Settled {
    let start = settle_on(world, profile, vec3(s(0.0), s(0.0), s(64.0)));
    settle_out(
        |st| {
            let mut out = *st;
            for _ in 0..HZ {
                out = strafe_once(world, profile, &out, angle, Axis::Forward);
            }
            out
        },
        start,
    )
}

fn air_hold(profile: &PhysicsProfile, angle: Scalar, axis: Axis) -> Settled {
    let world = EmptyWorld;
    settle_out(
        |st| {
            let mut out = *st;
            for _ in 0..HZ {
                out = strafe_once(&world, profile, &out, angle, axis);
            }
            out
        },
        flying_at(s(320.0)),
    )
}

/// The whole technique: strafe in the air, and press jump on the command the
/// player is grounded.
///
/// Jump is pressed *only* when grounded, which also re-arms it: `PmoveSingle`
/// clears `PMF_JUMP_HELD` on any command that does not ask to jump, and every
/// airborne command here is one.
fn bunnyhop<W: World>(world: &W, profile: &PhysicsProfile, angle: Scalar) -> Settled {
    let start = settle_on(world, profile, vec3(s(0.0), s(0.0), s(64.0)));
    settle_out(
        |st| {
            let mut out = *st;
            for _ in 0..HZ {
                let want = heading_degrees(out.player.velocity) + angle;
                let mut cmd = holding(Axis::Forward, yaw_for(Axis::Forward, want));
                if out.player.ground.is_grounded() {
                    cmd = UserCmd {
                        buttons: Buttons::JUMP,
                        ..cmd
                    };
                }
                out = step(&out, &cmd, world, profile);
            }
            out
        },
        start,
    )
}
