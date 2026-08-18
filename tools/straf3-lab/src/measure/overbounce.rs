//! Criterion 1, item 4: overbounce — which fall speeds and geometry produce it.
//!
//! # The mechanism, stated before it is searched for
//!
//! A falling player's command ends without the sweep reaching the floor, but
//! close enough that the *second* `PM_GroundTrace` of that command — the one Q3
//! runs after moving — finds the floor inside its 0.25-unit probe. The player is
//! therefore `Grounded` while still carrying their whole downward velocity.
//!
//! On the next command `PM_WalkMove` runs instead of `PM_AirMove`. `PM_Friction`
//! zeroes the horizontal velocity and leaves Z alone; then comes the pair of
//! lines that make Q3 ramps preserve speed:
//!
//! ```text
//! vel = VectorLength( pm->ps->velocity );
//! PM_ClipVelocity( velocity, groundTrace.plane.normal, velocity, OVERCLIP );
//! VectorNormalize( velocity );
//! VectorScale( velocity, vel, velocity );
//! ```
//!
//! The clip removes the downward component and `OVERCLIP` = 1.001 leaves a hair
//! of *upward*; the normalise-and-rescale then blows that hair back up to the
//! full length of the original velocity. A player falling at 1280 ups is thrown
//! upward at 1280 ups.
//!
//! So overbounce is not a bug in the speed-preservation rule; it *is* the
//! speed-preservation rule, applied to a vector that happened to be pointing
//! straight down. Whether it fires for a given fall is a question about where in
//! a command the hull ends up, which is why the sweep below varies the drop
//! height in sixteenths of a unit as well as in whole ones.
//!
//! # What is not overbounce
//!
//! Every ordinary landing leaves a small upward velocity: `OVERCLIP` is 1.001,
//! so clipping a 2560 ups impact leaves 2.56 ups going up. That is a residue,
//! not a technique. The classification below is by the *ratio* of upward speed
//! returned to downward speed arriving, which separates the two by two orders of
//! magnitude rather than by a threshold somebody chose.

use straf3_sim::num::{Scalar, s, vec3};
use straf3_sim::world::{FlatGround, World};
use straf3_sim::{GroundState, PhysicsProfile, SimState, step};

use crate::dataset::{Measurement, Section, Table};
use crate::geometry;
use crate::harness::{HZ, settle_on, still};
use crate::measure::{pad, profiles};
use crate::num::horizontal_speed;

/// The sweep: every drop height from [`FALL_MIN`] to [`FALL_MAX`], sampled every
/// [`FALL_SAMPLE`] units.
///
/// Uniform, not clustered. Whether overbounce fires depends on where inside a
/// command the hull ends up, so the sampling has to be fine — but it also has to
/// be *even*, because a rate quoted over a clustered sample cannot be compared
/// with anybody else's. These three numbers are chosen to reproduce the sim
/// seat's sweep exactly: 8064 samples over 16–1024, of which the first 1920 are
/// their 16–256 sub-sweep. Two independent implementations measuring the same
/// population is the only way the agreement below means anything.
const FALL_MIN: f32 = 16.0;
const FALL_MAX: f32 = 1024.0;
const FALL_SAMPLE: f32 = 0.125;

/// The upper bound of the sub-sweep whose rate is reported separately.
const SUBRANGE_MAX: f32 = 256.0;

/// A contact slower than this is not interesting: the mechanism needs a fall.
const CONTACT_SPEED: Scalar = s(100.0);

/// Fraction of the impact speed returned upward, above which a landing is a
/// **full** overbounce.
const FULL_RATIO: Scalar = s(0.5);

/// Fraction above which it is a **partial** one, and below which it is the
/// `OVERCLIP` residue every landing leaves.
const PARTIAL_RATIO: Scalar = s(0.05);

/// Heights called out individually, so a regression has named anchors and not
/// only an aggregate to move.
///
/// 16.000 and 16.500 are there because the sim seat reports that the first
/// overbounces and the second does not — the sharpest single claim either seat
/// makes about this behaviour, and the cheapest to check.
const PROBES: &[f32] = &[16.0, 16.5, 128.0, 512.0, 1024.0];

/// `(entry speed, drop height)` for the launch measurement.
///
/// The last three are the sim seat's own worked cases, so the two seats' numbers
/// can be put side by side. The odd height is theirs: 508.875 is a drop chosen to
/// land inside the window rather than a round number.
const LAUNCHES: &[(f32, f32)] = &[
    (300.0, 16.0),
    (300.0, 508.875),
    (600.0, 508.875),
    (400.0, 1024.0),
];

/// Downward speeds the mechanism is handed outright, with no horizontal speed
/// and no fall — the bare rule, and the sim seat's own experiment.
const HANDED_FALL_SPEEDS: &[f32] = &[100.0, 300.0, 600.0, 1000.0];

/// The most commands a fall is followed for. A 4096-unit drop takes about
/// 3.2 s, and a full overbounce sends the player back up for as long again.
const FALL_CAP: usize = 1600;

pub(crate) fn measure() -> Section {
    let mut section = Section::new("4. Overbounce");
    section.say(format!(
        "The sweep: a motionless player is dropped onto the floor from every \
         height between {FALL_MIN:.0} and {FALL_MAX:.0} units, sampled every \
         {FALL_SAMPLE} units — {} falls per world, of which the first {} lie in \
         the {FALL_MIN:.0}–{SUBRANGE_MAX:.0} sub-range reported beside them. Each \
         fall is followed to rest. A contact arriving faster than \
         {CONTACT_SPEED:.0} ups and leaving upward is classified by how much of \
         the arriving speed came back: at least {:.0}% is a **full** overbounce, \
         at least {:.0}% a partial one, and anything less is the `OVERCLIP` \
         residue an ordinary landing leaves.",
        samples(),
        subrange_samples(),
        FULL_RATIO * s(100.0),
        PARTIAL_RATIO * s(100.0),
    ));

    let brush_floor = geometry::floor();
    let analytic = FlatGround::at(s(0.0));

    let mut summary = Table::new(
        format!(
            "**The sweep.** `full` and `partial` are counts of falls in each \
             class; `sub-range` is the full count within \
             {FALL_MIN:.0}–{SUBRANGE_MAX:.0} units."
        ),
        &[
            "profile",
            "world",
            "falls",
            "full",
            "rate",
            "sub-range",
            "partial",
            "max return",
            "lowest full drop",
        ],
    );
    let mut probes = Table::new(
        "**Named drops.** 16.000 and 16.500 are the sim seat's sharpest claim: \
         the first overbounces, the second does not.",
        &["profile", "drop", "impact", "upward", "return"],
    );
    let mut launches = Table::new(
        "**The launch.** A player falling while carrying horizontal speed meets \
         the floor with both, and `PM_WalkMove`'s rescale hands the whole \
         magnitude back *horizontally*. `closed form` is \
         `sqrt(entry² + 2·gravity·drop)`; `1 s later` is what ground friction \
         has left of the peak a second afterwards.",
        &[
            "profile",
            "entry",
            "drop",
            "peak",
            "fall at launch",
            "constructed",
            "closed form",
            "1 s later",
        ],
    );

    for (name, profile) in profiles() {
        for (world_name, sweep) in [
            ("brush floor", sweep_drops(&brush_floor, &profile)),
            ("analytic plane", sweep_drops(&analytic, &profile)),
        ] {
            let key = format!("{name}.overbounce.{}", world_name.replace(' ', "_"));
            section.record(Measurement::count(format!("{key}.falls"), sweep.falls));
            section.record(Measurement::count(format!("{key}.full"), sweep.full));
            section.record(Measurement::count(
                format!("{key}.full_in_subrange"),
                sweep.full_in_subrange,
            ));
            section.record(Measurement::count(
                format!("{key}.falls_in_subrange"),
                sweep.falls_in_subrange,
            ));
            section.record(Measurement::ratio(
                format!("{key}.full_rate"),
                s(sweep.full as f32) / s(sweep.falls as f32),
            ));
            section.record(Measurement::count(format!("{key}.partial"), sweep.partial));
            section.record(Measurement::ratio(
                format!("{key}.max_return_ratio"),
                sweep.max_ratio,
            ));
            section.record(Measurement::ups(
                format!("{key}.max_upward_ups"),
                sweep.max_upward,
            ));
            section.record(Measurement::units(
                format!("{key}.height_at_max_upward"),
                sweep.height_at_max,
            ));
            section.record(Measurement::units(
                format!("{key}.lowest_full_drop"),
                sweep.lowest_full,
            ));
            section.record(Measurement::ups(
                format!("{key}.fastest_impact_ups"),
                sweep.fastest_impact,
            ));

            summary.push(vec![
                name.to_string(),
                world_name.to_string(),
                format!("{}", sweep.falls),
                format!("{}", sweep.full),
                format!("{:.2}%", 100.0 * sweep.full as f32 / sweep.falls as f32),
                format!("{}/{}", sweep.full_in_subrange, sweep.falls_in_subrange),
                format!("{}", sweep.partial),
                format!("{:.4}", sweep.max_ratio),
                if sweep.full > 0 {
                    format!("{:.3}", sweep.lowest_full)
                } else {
                    "—".to_string()
                },
            ]);
        }

        for &height in PROBES {
            let landing = drop_onto(&brush_floor, &profile, s(height), s(0.0));
            let key = format!(
                "{name}.overbounce.drop{}",
                pad((height * 1000.0) as u32, 7)
            );
            section.record(Measurement::ups(format!("{key}.impact_ups"), landing.impact));
            section.record(Measurement::ups(format!("{key}.upward_ups"), landing.upward));
            section.record(Measurement::ratio(format!("{key}.return_ratio"), landing.ratio()));
            probes.push(vec![
                name.to_string(),
                format!("{height:.3}"),
                format!("{:.2}", landing.impact),
                format!("{:.2}", landing.upward),
                format!("{:.4}", landing.ratio()),
            ]);
        }

        // The bare rule: a grounded player handed a purely downward velocity.
        // No fall, no horizontal speed, nothing between the arithmetic and the
        // answer.
        for &fall in HANDED_FALL_SPEEDS {
            let mut st = settle_on(&brush_floor, &profile, vec3(s(0.0), s(0.0), s(64.0)));
            st.player.velocity = vec3(s(0.0), s(0.0), -s(fall));
            let after = step(&st, &still(), &brush_floor, &profile);
            section.record(Measurement::ups(
                format!(
                    "{name}.overbounce.handed{}.returned_vz",
                    pad(fall as u32, 4)
                ),
                after.player.velocity.z,
            ));
        }

        for &(entry, height) in LAUNCHES {
            let l = launch(&brush_floor, &profile, s(height), s(entry));
            let key = format!(
                "{name}.overbounce.launch{}from{}",
                pad(entry as u32, 4),
                pad((height * 1000.0) as u32, 7)
            );
            section.record(Measurement::ups(format!("{key}.peak_ups"), l.peak));
            section.record(Measurement::ups(
                format!("{key}.fall_at_launch_ups"),
                l.fall_at_launch,
            ));
            section.record(Measurement::ups(
                format!("{key}.after_a_second_ups"),
                l.after_a_second,
            ));
            section.record(Measurement::ups(format!("{key}.closed_form_ups"), l.closed));
            section.record(Measurement::ups(
                format!("{key}.gain_ups"),
                l.peak - s(entry),
            ));

            // The same conversion with the precondition constructed rather than
            // arrived at: given the ideal fall speed outright, does the rule
            // return all of it?
            let ideal_fall = (s(2.0) * profile.gravity * s(height)).sqrt();
            let (constructed, closed) =
                launch_from_velocity(&brush_floor, &profile, s(entry), ideal_fall);
            section.record(Measurement::ups(
                format!("{key}.constructed_ups"),
                constructed,
            ));
            section.record(Measurement::ups(
                format!("{key}.constructed_closed_form_ups"),
                closed,
            ));

            launches.push(vec![
                name.to_string(),
                format!("{entry:.0}"),
                format!("{height:.3}"),
                format!("{:.2}", l.peak),
                format!("{:.2}", l.fall_at_launch),
                format!("{constructed:.2}"),
                format!("{:.2}", l.closed),
                format!("{:.2}", l.after_a_second),
            ]);
        }
    }

    section.table(summary);
    section.table(probes);
    section.table(launches);
    section.say(
        "**The launch is the same mechanism, and it is the largest speed gain in \
         the current vocabulary.** `PM_WalkMove` takes the *length* of the \
         velocity, clips it to the ground plane, and rescales to that length. A \
         player who lands with the ground probe already under them still carries \
         their fall speed, so the rescale returns `sqrt(horizontal² + fall²)` — \
         and on flat ground the clip leaves nothing but horizontal, so all of it \
         is speed the player keeps. It is not a bug in the ramp rule; it *is* \
         the ramp rule, applied to a velocity that happened to be pointing \
         mostly down. Compare the numbers above against every strafe technique \
         in section 6: nothing else in this tree pays like a drop.",
    );
    section.say(
        "**`constructed` and `peak` are not the same number, and the gap is the \
         result.** `constructed` hands a grounded player the ideal fall speed \
         `sqrt(2·g·h)` outright and steps once: it measures the rule, and it \
         lands on the closed form. `peak` is what an actual fall from that \
         height produces, and it is lower — because the downward speed a player \
         *has* on the command that launches them is the `fall at launch` column, \
         which is below the ideal. A fall is quantised into 8 ms commands and the \
         launch fires on whichever one happens to end with the feet inside the \
         0.25-unit ground probe; the player is caught before they have fallen \
         the whole way. So the closed form is a ceiling on the technique, not a \
         prediction of it, and a route planner wants the `peak` column.",
    );
    section.say(
        "**The analytic-plane row is a control, not a duplicate.** It shares the \
         whole mover and differs only in what answers the trace, so a difference \
         between the two rows would place the behaviour in the brush tracer's \
         bevels and epsilons rather than in the movement code. Read them \
         together: agreement says overbounce is a property of `PM_WalkMove` and \
         `OVERCLIP`, which is what the mechanism above predicts.",
    );
    section.say(
        "**This is the number most likely to move.** Whether a fall overbounces \
         is a question about which fraction of a command the hull is inside when \
         it meets the floor, and `pmove_msec` sub-stepping (`step.rs`, \
         `TODO(wave3)`) changes exactly that. Every count here is a statement \
         about the current single-step integration and should be re-taken when \
         sub-stepping lands.",
    );

    section
}

/// What one fall produced.
struct Landing {
    /// Downward speed arriving at the contact that produced the most upward
    /// velocity, as a positive number.
    impact: Scalar,
    /// The upward velocity that contact produced.
    upward: Scalar,
}

impl Landing {
    fn ratio(&self) -> Scalar {
        if self.impact > s(0.0) {
            self.upward / self.impact
        } else {
            s(0.0)
        }
    }
}

/// Aggregate over a whole sweep.
struct Sweep {
    falls: u32,
    falls_in_subrange: u32,
    full: u32,
    full_in_subrange: u32,
    partial: u32,
    max_ratio: Scalar,
    max_upward: Scalar,
    height_at_max: Scalar,
    lowest_full: Scalar,
    fastest_impact: Scalar,
}

/// How many heights the sweep visits.
fn samples() -> u32 {
    ((FALL_MAX - FALL_MIN) / FALL_SAMPLE) as u32
}

/// How many of them lie in the reported sub-range.
fn subrange_samples() -> u32 {
    ((SUBRANGE_MAX - FALL_MIN) / FALL_SAMPLE) as u32
}

/// Follow a fall to rest, watching for a contact that returns speed upward.
///
/// The whole fall is followed and not just the first contact, because a full
/// overbounce throws the player back into the air and the interesting number is
/// the largest return anywhere in the fall — a player who bounces and bounces
/// again has still overbounced.
fn follow<W: World>(world: &W, profile: &PhysicsProfile, mut st: SimState) -> Landing {
    let mut best = Landing {
        impact: s(0.0),
        upward: s(0.0),
    };
    let mut at_rest = 0;
    for _ in 0..FALL_CAP {
        let before = st.player.velocity.z;
        st = step(&st, &still(), world, profile);
        let after = st.player.velocity.z;

        if before < -CONTACT_SPEED && after > s(0.0) {
            let candidate = Landing {
                impact: -before,
                upward: after,
            };
            if candidate.ratio() > best.ratio() {
                best = candidate;
            }
        }

        if st.player.ground.is_grounded() && after.abs() < s(1.0) {
            at_rest += 1;
            if at_rest >= 2 {
                break;
            }
        } else {
            at_rest = 0;
        }
    }
    best
}

/// Drop a motionless player from `height + offset` and follow them to rest.
fn drop_onto<W: World>(
    world: &W,
    profile: &PhysicsProfile,
    height: Scalar,
    offset: Scalar,
) -> Landing {
    let mut st = SimState::spawned_at(
        vec3(
            s(0.0),
            s(0.0),
            geometry::resting_origin_z(s(0.0)) + height + offset,
        ),
        s(0.0),
    );
    st.player.ground = GroundState::Airborne;
    follow(world, profile, st)
}

/// Every drop in the sweep, aggregated.
///
/// The height is computed as `FALL_MIN + k · FALL_SAMPLE` from an integer `k`
/// rather than accumulated, so the sampling grid is exact and reproducible
/// rather than drifting by whatever `0.125` does not represent.
fn sweep_drops<W: World>(world: &W, profile: &PhysicsProfile) -> Sweep {
    let mut out = Sweep {
        falls: 0,
        falls_in_subrange: 0,
        full: 0,
        full_in_subrange: 0,
        partial: 0,
        max_ratio: s(0.0),
        max_upward: s(0.0),
        height_at_max: s(0.0),
        lowest_full: s(0.0),
        fastest_impact: s(0.0),
    };
    for k in 0..samples() {
        let height = s(FALL_MIN) + s(k as f32) * s(FALL_SAMPLE);
        let in_subrange = k < subrange_samples();
        let landing = drop_onto(world, profile, height, s(0.0));
        let ratio = landing.ratio();

        out.falls += 1;
        if in_subrange {
            out.falls_in_subrange += 1;
        }
        if landing.impact > out.fastest_impact {
            out.fastest_impact = landing.impact;
        }
        if ratio >= FULL_RATIO {
            out.full += 1;
            if in_subrange {
                out.full_in_subrange += 1;
            }
            if out.lowest_full == s(0.0) {
                out.lowest_full = height;
            }
        } else if ratio >= PARTIAL_RATIO {
            out.partial += 1;
        }
        if ratio > out.max_ratio {
            out.max_ratio = ratio;
        }
        if landing.upward > out.max_upward {
            out.max_upward = landing.upward;
            out.height_at_max = height;
        }
    }
    out
}

/// What a fall carrying horizontal speed produced.
struct Launch {
    /// The peak horizontal speed reached at or after the landing.
    peak: Scalar,
    /// Downward speed the player actually had on the command that launched
    /// them. Not `sqrt(2·g·h)`: see [`launch`].
    fall_at_launch: Scalar,
    /// Horizontal speed a whole second later, after ground friction has had it.
    after_a_second: Scalar,
    /// `sqrt(entry² + 2·gravity·drop)` — what the rescale hands back if the
    /// whole of the *ideal* fall speed is converted.
    closed: Scalar,
}

/// The mechanism on its own, with nothing between it and the arithmetic.
///
/// A player standing on flat ground is given `(entry, 0, −fall)` outright and
/// stepped once. This is the precondition `PM_WalkMove` needs, constructed
/// rather than arrived at, so the result should be exactly
/// `sqrt(entry² + fall²)` and any departure from it is the movement code rather
/// than the approach.
///
/// It exists beside [`launch`] because the two answer different questions —
/// "what does the rule do" and "what can a player get" — and the gap between
/// them turned out to be the interesting part. See the cross-validation section.
fn launch_from_velocity<W: World>(
    world: &W,
    profile: &PhysicsProfile,
    entry: Scalar,
    fall: Scalar,
) -> (Scalar, Scalar) {
    let mut st = settle_on(world, profile, vec3(s(0.0), s(0.0), s(64.0)));
    st.player.velocity = vec3(entry, s(0.0), -fall);
    let after = step(&st, &still(), world, profile);
    (
        horizontal_speed(after.player.velocity),
        (entry * entry + fall * fall).sqrt(),
    )
}

/// Fall `height` while carrying `entry` ups horizontally, and report the peak.
///
/// **Placed airborne rather than run off a platform**, and this is the whole
/// design of the measurement. A run-up does not preserve an entry speed: ground
/// friction takes a coasting player at 600 ups back under 320 in a tenth of a
/// second, and holding forward *raises* 300 to 320 over the same distance, so
/// whatever speed the player has at the edge is a property of the run-up rather
/// than the number that was asked for. Starting in the air at exactly `entry`
/// makes the independent variable independent.
///
/// The **peak** is reported, not the speed some fixed time later. The launch is
/// a one-command event; what happens afterwards is friction, and friction is
/// measured in section 2. `after_a_second` is published beside it so nobody
/// mistakes the peak for something a player keeps.
fn launch<W: World>(
    world: &W,
    profile: &PhysicsProfile,
    height: Scalar,
    entry: Scalar,
) -> Launch {
    let mut st = SimState::spawned_at(
        vec3(
            s(0.0),
            s(0.0),
            geometry::resting_origin_z(s(0.0)) + height,
        ),
        s(0.0),
    );
    st.player.ground = GroundState::Airborne;
    st.player.velocity = vec3(entry, s(0.0), s(0.0));

    let mut peak = entry;
    let mut fall_at_launch = s(0.0);
    let mut landed_at: Option<usize> = None;
    let mut after_a_second = entry;
    for command in 0..FALL_CAP {
        let falling = -st.player.velocity.z;
        st = step(&st, &still(), world, profile);
        let now = horizontal_speed(st.player.velocity);
        if st.player.ground.is_on_plane() && landed_at.is_none() {
            landed_at = Some(command);
        }
        if let Some(landing) = landed_at {
            if now > peak {
                peak = now;
                fall_at_launch = falling;
            }
            if command == landing + HZ {
                after_a_second = now;
                break;
            }
        }
    }

    Launch {
        peak,
        fall_at_launch,
        after_a_second,
        closed: (entry * entry + s(2.0) * profile.gravity * height).sqrt(),
    }
}
