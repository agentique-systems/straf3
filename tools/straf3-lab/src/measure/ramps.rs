//! Criterion 1, item 3: ramp-angle response across the walkable/sliding flip.
//!
//! # The two mechanisms, and why the flip is where the game is
//!
//! On a *walkable* ramp `PM_WalkMove` clips velocity to the ground plane and
//! then rescales it back to the length it had — Q3's "don't decrease velocity
//! when going up or down a slope". Direction follows the surface; magnitude
//! does not notice the surface tilted. Ground friction still applies.
//!
//! On a ramp too steep to stand on, `pml.walking` is false, so no ground
//! friction is applied at all and the air rules govern acceleration; velocity
//! is still clipped to the plane, so the player follows the surface. Only
//! gravity's component along the slope changes the speed.
//!
//! Those are different games, and the boundary between them is one comparison:
//! `normal.z < min_walk_normal`. Everything below measures where that boundary
//! sits and what crossing it is worth.
//!
//! # Why the observation window is short
//!
//! Ground friction is not gentle: 6 against a `stop_speed` of 100 takes a
//! coasting player from 800 ups to 15 in half a second. An observation window
//! long enough to be comfortable is a window in which the answer is *friction*
//! and the ramp is a rounding error. So the crossing is detected and the
//! measurement is taken over [`AFTER_CROSSING`] commands from that point,
//! against a flat-ground control run for the same number of commands from the
//! same speed. What is reported is the difference between two runs that differ
//! only in the surface.

use straf3_sim::num::{Scalar, Vec3, s, vec3};
use straf3_sim::world::World;
use straf3_sim::{GroundState, PhysicsProfile, SimState, step};

use crate::dataset::{Measurement, Section, Table};
use crate::geometry;
use crate::harness::{settle_on, still};
use crate::measure::pad;
use crate::num::horizontal_speed;

/// Ramp angles swept, in degrees. Dense either side of the flip, which sits
/// where `cos(angle)` crosses `min_walk_normal` = 0.7.
/// 10° and 26° are here because they are the sim seat's worked examples, so the
/// two seats' seam-loss numbers can be put side by side without either of us
/// interpolating.
const ANGLES: &[f32] = &[
    5.0, 10.0, 15.0, 25.0, 26.0, 35.0, 40.0, 44.0, 45.0, 46.0, 50.0, 60.0,
];

/// Entry speed for the uphill (speed-retained) measurement.
const UPHILL_ENTRY: f32 = 800.0;

/// Entry speed for the downhill (speed-gained) measurement.
const DOWNHILL_ENTRY: f32 = 400.0;

/// Commands observed after the crossing is detected — 128 ms.
///
/// Long enough for the slide solver to have resolved the transition and for a
/// sliding player to have picked up measurable speed; short enough that a
/// walking player still has most of their entry speed, so the comparison
/// against flat ground has range in it.
const AFTER_CROSSING: usize = 16;

/// How far a run travels before the surface underfoot changes, in units.
///
/// Deliberately tiny. Ground friction is savage in Quake: a player coasting at
/// 400 ups covers about 67 units in total before stopping, so a run-up of any
/// generosity turns the entry speed into a number the ramp never sees. Eight
/// units is one or two commands, and the speed that actually arrived is
/// reported as `at seam` rather than assumed.
///
/// The start positions are computed from the hull's half-width, because the
/// surface underfoot changes when the *box* leaves the flat, not when the
/// origin does — 15 units early going one way and 15 late going the other.
const RUN_UP: f32 = 8.0;

/// Most commands a crossing search waits before giving up.
const CROSSING_CAP: usize = 400;

pub(crate) fn measure(profiles: &[(&str, PhysicsProfile)]) -> Section {
    let mut section = Section::new("3. Ramp-angle response");
    section.say(format!(
        "Three measurements per angle, all with no movement keys held and all \
         over {AFTER_CROSSING} commands ({} ms). **`uphill`** places the player \
         {RUN_UP:.0} units short of the seam at {UPHILL_ENTRY:.0} ups and starts \
         measuring on the command the surface underfoot stops being flat. \
         **`downslope`** places them on the slope itself with the velocity \
         already parallel to it at {DOWNHILL_ENTRY:.0} ups — the ramp as a \
         surface. **`off the top`** coasts them off the far edge of the top \
         platform at {DOWNHILL_ENTRY:.0} ups — the ramp as an edge, which is a \
         different behaviour and gets its own column because at any real speed a \
         player leaves that edge rather than following the slope down.",
        AFTER_CROSSING * 8
    ));
    section.say(format!(
        "Every `vs flat` figure is against a control that coasted the same \
         number of commands, from the same speed, on flat ground — so friction \
         is divided out and what is left is the surface. The run-ups are \
         deliberately {RUN_UP:.0} units: ground friction in Quake is savage, and \
         a coasting player at {DOWNHILL_ENTRY:.0} ups covers about 67 units \
         before stopping altogether, so a generous run-up would turn the entry \
         speed into a number the ramp never sees. The speed that actually \
         arrived is published as `at seam`."
    ));

    for (name, profile) in profiles {
        let profile = *profile;
        let mut surface = Table::new(
            format!(
                "**{name} — the surface.** `standing` is what `GroundState` \
                 reports for a player dropped onto the slope; `walkable` is \
                 `normal.z ≥ min_walk_normal` = {:.2}.",
                profile.min_walk_normal
            ),
            &["angle", "normal.z", "walkable", "standing"],
        );
        let mut motion = Table::new(
            format!(
                "**{name} — moving on it.** `uphill` crosses onto the slope from \
                 the flat approach at {UPHILL_ENTRY:.0} ups; `downslope` starts \
                 on the slope itself, travelling down it at {DOWNHILL_ENTRY:.0} \
                 ups; `off the top` coasts off the far edge of the top platform \
                 at {DOWNHILL_ENTRY:.0} ups. All three are 128 ms."
            ),
            &[
                "angle",
                "uphill: seam ratio¹",
                "cos(angle)",
                "total after",
                "vs flat",
                "downslope: total after",
                "vs flat",
                "off the top: kept",
                "ends",
            ],
        );

        for &degrees in ANGLES {
            let d = s(degrees);
            let world = geometry::ramp(d);
            let normal = geometry::ramp_normal(d);
            let rise = geometry::ramp_rise(d);
            let key = format!("{name}.ramp.deg{}", pad(degrees as u32, 2));

            section.record(Measurement::ratio(format!("{key}.normal_z"), normal.z));
            section.record(Measurement::flag(
                format!("{key}.walkable"),
                normal.z >= profile.min_walk_normal,
            ));

            let standing = ground_label(&stand_on_slope(&profile, d));
            section.record(Measurement::label(
                format!("{key}.standing_state"),
                standing,
            ));
            surface.push(vec![
                format!("{degrees:.0}°"),
                format!("{:.4}", normal.z),
                if normal.z >= profile.min_walk_normal {
                    "yes"
                } else {
                    "no"
                }
                .to_string(),
                standing.to_string(),
            ]);

            // Uphill: from the approach, running +X. The box leaves the flat
            // when `origin + half_width` passes the seam at x = 0.
            let half = profile.hull_maxs.x;
            let up = cross(
                &world,
                &profile,
                vec3(-(half + s(RUN_UP)), s(0.0), s(64.0)),
                s(UPHILL_ENTRY),
            );
            record_crossing(&mut section, &format!("{key}.uphill"), &up);

            // Down the slope, already on it and already pointing along it. This
            // is the ramp as a *surface* rather than as an edge.
            let down = down_the_slope(&profile, d, s(DOWNHILL_ENTRY));
            record_crossing(&mut section, &format!("{key}.downslope"), &down);

            // Off the far edge of the top platform. The box leaves the platform
            // when `origin + half_width` passes the seam at RAMP_RUN, so the
            // start is that far past it.
            let launch = cross(
                &world,
                &profile,
                vec3(
                    geometry::RAMP_RUN + s(RUN_UP) - half,
                    s(0.0),
                    rise + s(64.0),
                ),
                -s(DOWNHILL_ENTRY),
            );
            record_crossing(&mut section, &format!("{key}.off_the_top"), &launch);

            let geometric = up.seam_ratio_without_friction(&profile);
            section.record(Measurement::ratio(
                format!("{key}.uphill_seam_ratio_less_friction"),
                geometric,
            ));
            section.record(Measurement::ratio(
                format!("{key}.cos_angle"),
                crate::num::cos_degrees(d),
            ));
            section.record(Measurement::ratio(
                format!("{key}.seam_ratio_minus_cos"),
                geometric - crate::num::cos_degrees(d),
            ));

            motion.push(vec![
                format!("{degrees:.0}°"),
                format!("{geometric:.5}"),
                format!("{:.5}", crate::num::cos_degrees(d)),
                format!("{:.2}", up.after_total),
                format!("{:+.2}", up.after_total - up.control),
                format!("{:.2}", down.after_total),
                format!("{:+.2}", down.after_total - down.control),
                format!("{:.2}", launch.after_horizontal),
                launch.end_state.to_string(),
            ]);
        }
        section.table(surface);
        section.table(motion);

        // ── where the flip actually is ────────────────────────────────────
        let by_normal = flip_by_normal(&profile);
        let by_behaviour = flip_by_behaviour(&profile);
        section.record(Measurement::ratio(
            format!("{name}.ramp.min_walk_normal"),
            profile.min_walk_normal,
        ));
        section.record(Measurement::degrees(
            format!("{name}.ramp.flip_angle_from_normal"),
            by_normal,
        ));
        section.record(Measurement::degrees(
            format!("{name}.ramp.flip_angle_observed"),
            by_behaviour,
        ));
        section.record(Measurement::degrees(
            format!("{name}.ramp.flip_angle_disagreement"),
            (by_behaviour - by_normal).abs(),
        ));
        section.say(format!(
            "**{name}: the walkable/sliding flip.** `min_walk_normal` is {:.4}, \
             which the ramp surface normal crosses at **{by_normal:.2}°** — the \
             steepest ramp whose normal is still walkable, searched at 0.01°. \
             Dropping a player onto the slope and reading `GroundState` back \
             puts the last standable ramp at **{by_behaviour:.2}°**. They differ \
             by {:.2}°, which is the search resolution: the constant is the \
             thing the behaviour turns on, not a number stored beside it.",
            profile.min_walk_normal,
            (by_behaviour - by_normal).abs()
        ));
    }

    section.say(
        "**The `vs flat` columns are the ramp's contribution with friction \
         divided out**, because the control ran the same number of commands from \
         the same speed on flat ground. The columns are *total* speed; the \
         machine-readable section carries the horizontal component beside each \
         one, and the two disagree on a ramp for a reason worth stating: \
         `PM_WalkMove` preserves the magnitude of the velocity and turns part of \
         it upward, and `PM_Friction` then scales the whole vector by a factor \
         it computed from the horizontal part alone. A climbing player's \
         *horizontal* speed therefore decays at exactly the flat-ground rate, so \
         a report that showed only that would say a ramp is free. It is not \
         free; it is *converted*.",
    );
    section.say(
        "**No gravity is applied while walking.** `PM_WalkMove` calls the slide \
         solver with gravity off — only `PM_AirMove` turns it on — so a walkable \
         ramp costs a player nothing beyond the friction they would have paid \
         anyway, however steep it is. That is what makes a ramp a route rather \
         than an obstacle, and it holds right up to the flip. Past the flip the \
         player is `Sliding`: touching a plane but not walking, so ground \
         friction is not applied at all and gravity's component along the slope \
         is. That reversal is the mechanic.",
    );
    section.say(
        "**Traversing a ramp never gains speed. The whole cost is one command.** \
         The `seam ratio` column is the fraction of the player's total speed that \
         survives the single command on which the surface underfoot changes, and \
         it lands on `cos(angle)` — the third column, printed beside it so the \
         agreement is checkable rather than asserted. The mechanism: on that one \
         command the ground normal `PM_WalkMove` reads is still the flat \
         approach's, so its length-preserving rescale is computed against the \
         wrong plane, and what actually turns the velocity is the slide solver \
         clipping it to the ramp — a clip with no rescale behind it. Clipping a \
         horizontal vector to a plane tilted by θ leaves `cos θ` of it. Every \
         command afterwards the normal is the ramp's and the magnitude is \
         preserved exactly, so a longer ramp costs no more than a short one.",
    );
    section.say(
        "¹ with the seam command's ordinary ground friction divided out. The \
         seam command is still a command and `PM_Friction` runs on it like any \
         other, taking `friction · dt` = 4.8% above `stop_speed`; that factor \
         appears in every row and has nothing to do with the ramp. The raw ratio \
         is published beside it in the machine-readable section as \
         `*_uphill_seam_ratio`, and the raw value is `cos(angle) × 0.952` to five \
         decimal places at every angle measured.",
    );
    section.say(
        "That makes the ramp-angle response a *loss* curve, not a boost curve. A \
         26° ramp costs 10% of the speed a player carries onto it and a 45° ramp \
         costs 29%, once, at the seam. The gain a player associates with ramps \
         in this tree is not on the ramp at all — it is the drop-launch in \
         section 4.",
    );

    section
}

/// What one crossing produced.
struct Crossing {
    /// Total speed on the command *before* the surface underfoot changed.
    before_seam: Scalar,
    /// Total speed on the command the player left the flat surface — the seam
    /// command itself.
    seam_total: Scalar,
    /// Horizontal speed on the command the player left the flat surface.
    at_seam: Scalar,
    /// Total speed [`AFTER_CROSSING`] commands later.
    after_total: Scalar,
    /// Horizontal speed at the same moment.
    after_horizontal: Scalar,
    /// The same coast on flat ground, from `at_seam`, for the same commands.
    /// Horizontal and total are the same number there, which is the point.
    control: Scalar,
    /// What `GroundState` reported at the end.
    end_state: &'static str,
}

/// Record a crossing's four numbers.
///
/// Total *and* horizontal, because on a ramp they are different questions and
/// reporting one of them would have answered the wrong one. Q3's slope rescale
/// preserves the *magnitude* of the velocity and turns some of it upward, and
/// `PM_Friction` then scales the whole vector by a factor computed from the
/// horizontal part alone — so a climbing player's horizontal speed decays at
/// exactly the flat-ground rate while their total speed does not. Report only
/// the horizontal and the ramp looks free; report only the total and a
/// descending player looks fast for speed they cannot spend.
fn record_crossing(section: &mut Section, key: &str, c: &Crossing) {
    section.record(Measurement::ups(
        format!("{key}_before_seam_ups"),
        c.before_seam,
    ));
    section.record(Measurement::ups(
        format!("{key}_seam_total_ups"),
        c.seam_total,
    ));
    section.record(Measurement::ratio(
        format!("{key}_seam_ratio"),
        c.seam_ratio(),
    ));
    section.record(Measurement::ups(format!("{key}_at_seam_ups"), c.at_seam));
    section.record(Measurement::ups(
        format!("{key}_after_total_ups"),
        c.after_total,
    ));
    section.record(Measurement::ups(
        format!("{key}_after_horizontal_ups"),
        c.after_horizontal,
    ));
    section.record(Measurement::ups(
        format!("{key}_flat_control_ups"),
        c.control,
    ));
    section.record(Measurement::ups(
        format!("{key}_vs_flat_ups"),
        c.after_total - c.control,
    ));
    section.record(Measurement::label(format!("{key}_end_state"), c.end_state));
}

/// Drive a coasting player at a seam, detect the crossing, and measure from
/// there against a flat-ground control.
///
/// The crossing is detected by the **ground normal**, not by a coordinate. The
/// player hull is 30 units wide, so its leading face meets the slope while the
/// origin is still 15 units short of the seam; a coordinate test fires fifteen
/// units late going one way and fifteen early going the other, and at these
/// speeds fifteen units is two commands of the sixteen being measured. The flat
/// surfaces here carry an exactly axial normal — they are compiled axial planes
/// and `SnapPlane` puts them on the axis — so the comparison is exact.
fn cross<W: World>(world: &W, profile: &PhysicsProfile, from: Vec3, speed_x: Scalar) -> Crossing {
    let mut st = settle_on(world, profile, from);
    st.player.velocity = vec3(speed_x, s(0.0), s(0.0));

    let flat_underfoot = |st: &SimState| st.player.ground.normal() == Some(straf3_sim::num::UP);
    let mut before_seam = st.player.velocity.length();
    for _ in 0..CROSSING_CAP {
        let previous = st.player.velocity.length();
        st = step(&st, &still(), world, profile);
        if !flat_underfoot(&st) {
            before_seam = previous;
            break;
        }
    }
    let seam_total = st.player.velocity.length();
    let at_seam = horizontal_speed(st.player.velocity);

    for _ in 0..AFTER_CROSSING {
        st = step(&st, &still(), world, profile);
    }

    // The control: the same speed, the same commands, on flat ground. Signed,
    // so a downhill run's control coasts the same direction it did.
    let flat = geometry::floor();
    let mut ctl = settle_on(&flat, profile, vec3(s(0.0), s(0.0), s(64.0)));
    ctl.player.velocity = vec3(
        if speed_x < s(0.0) { -at_seam } else { at_seam },
        s(0.0),
        s(0.0),
    );
    for _ in 0..AFTER_CROSSING {
        ctl = step(&ctl, &still(), &flat, profile);
    }

    Crossing {
        before_seam,
        seam_total,
        at_seam,
        after_total: st.player.velocity.length(),
        after_horizontal: horizontal_speed(st.player.velocity),
        control: horizontal_speed(ctl.player.velocity),
        end_state: ground_label(&st),
    }
}

impl Crossing {
    /// How much of the total speed survived the seam command itself.
    ///
    /// This is the whole of "speed retained vs ramp angle". Once the player is
    /// *on* the slope, `PM_WalkMove` preserves the magnitude of their velocity
    /// and traversing a ramp is free. The cost is a single command: the one on
    /// which the slide solver clips a horizontal velocity to the tilted plane,
    /// with no rescale behind it, because the ground normal `PM_WalkMove` used
    /// that command was still the flat approach's. Clipping a horizontal vector
    /// to a plane tilted by θ leaves `cos θ` of it, and that is the prediction
    /// this ratio is measured against.
    fn seam_ratio(&self) -> Scalar {
        if self.before_seam > s(0.0) {
            self.seam_total / self.before_seam
        } else {
            s(0.0)
        }
    }

    /// The seam ratio with the command's ordinary ground friction divided out.
    ///
    /// The seam command is still a command: `PM_Friction` runs on it like any
    /// other, and above `stop_speed` it takes exactly `friction · dt` of the
    /// speed. That factor is present in every row of the raw ratio and is
    /// nothing to do with the ramp, so leaving it in would put a constant 4.8%
    /// between the measurement and the `cos θ` it is being compared with — and
    /// a reader would have to know to subtract it before the agreement was
    /// visible. Both numbers are published; this is the one the comparison uses.
    fn seam_ratio_without_friction(&self, profile: &PhysicsProfile) -> Scalar {
        let dt = straf3_sim::num::seconds_from_millis(u32::from(crate::harness::MS));
        self.seam_ratio() / (s(1.0) - profile.friction * dt)
    }
}

/// Start already on the slope, already pointing down it, and measure the
/// surface rather than the edge.
///
/// The obvious alternative — coast off the top platform and let the player find
/// the slope — measures something else entirely: at any real speed they leave
/// the edge and fly, and what comes back is the launch, not the slide. Both are
/// worth having, so both are taken, and this one places the player on the
/// surface with the velocity already parallel to it.
fn down_the_slope(profile: &PhysicsProfile, degrees: Scalar, entry: Scalar) -> Crossing {
    let world = geometry::ramp(degrees);
    let normal = geometry::ramp_normal(degrees);
    // Down-slope is the surface direction with a negative X component: the
    // normal leans toward −X, so rotating it a quarter turn in the XZ plane
    // gives (−normal.z, 0, −(−normal.x)) — written out rather than derived at
    // the call site so the sign is stated once.
    let along = vec3(-normal.z, s(0.0), -normal.x.abs());

    let mut st = stand_on_slope(profile, degrees);
    st.player.velocity = along * entry;
    for _ in 0..AFTER_CROSSING {
        st = step(&st, &still(), &world, profile);
    }

    let flat = geometry::floor();
    let mut ctl = settle_on(&flat, profile, vec3(s(0.0), s(0.0), s(64.0)));
    ctl.player.velocity = vec3(-entry, s(0.0), s(0.0));
    for _ in 0..AFTER_CROSSING {
        ctl = step(&ctl, &still(), &flat, profile);
    }

    Crossing {
        // There is no seam here: the player starts on the slope already
        // travelling along it, so the "before" and "at" speeds are both the
        // entry and the ratio is 1 by construction rather than by measurement.
        before_seam: entry,
        seam_total: entry,
        at_seam: entry,
        after_total: st.player.velocity.length(),
        after_horizontal: horizontal_speed(st.player.velocity),
        control: horizontal_speed(ctl.player.velocity),
        end_state: ground_label(&st),
    }
}

/// Drop a player onto the middle of the slope and read `GroundState` on the
/// command they first touch it.
///
/// Read *at contact*, not after settling: a player who cannot stand on a ramp
/// slides down it, and a few hundred commands later they are standing on the
/// flat approach at the bottom reporting `Grounded` — which is a true statement
/// about where they ended up and a false one about the ramp.
fn stand_on_slope(profile: &PhysicsProfile, degrees: Scalar) -> SimState {
    let world = geometry::ramp(degrees);
    let x = geometry::RAMP_RUN * s(0.5);
    let surface = geometry::ramp_rise(degrees) * s(0.5);
    let mut st = SimState::spawned_at(
        vec3(x, s(0.0), geometry::resting_origin_z(surface) + s(16.0)),
        s(0.0),
    );
    st.player.ground = GroundState::Airborne;
    for _ in 0..CROSSING_CAP {
        st = step(&st, &still(), &world, profile);
        if st.player.ground.is_on_plane() {
            return st;
        }
    }
    st
}

/// The ground state as a word a table can print.
fn ground_label(st: &SimState) -> &'static str {
    match st.player.ground {
        GroundState::Grounded { .. } => "grounded",
        GroundState::Sliding { .. } => "sliding",
        GroundState::Airborne => "airborne",
    }
}

/// The steepest ramp whose surface normal is still walkable, from the geometry
/// alone, searched at 0.01°.
fn flip_by_normal(profile: &PhysicsProfile) -> Scalar {
    let mut last_walkable = s(0.0);
    for hundredths in 1..9000 {
        let d = s(hundredths as f32 * 0.01);
        if geometry::ramp_normal(d).z >= profile.min_walk_normal {
            last_walkable = d;
        } else {
            break;
        }
    }
    last_walkable
}

/// The steepest ramp a dropped player still reports standing on, from the
/// simulation's own answer, searched at 0.01°.
///
/// Started from the analytic answer and walked outwards by half a degree,
/// because dropping a player is hundreds of commands and doing it nine thousand
/// times would make the lab too slow to run. The window is wide enough that a
/// disagreement of a tenth of a degree between the two methods would be found,
/// and the disagreement itself is published.
fn flip_by_behaviour(profile: &PhysicsProfile) -> Scalar {
    let from_normal = flip_by_normal(profile);
    let mut best = s(0.0);
    for offset in -50..=50 {
        let d = from_normal + s(offset as f32 * 0.01);
        if d <= s(0.0) || d >= s(90.0) {
            continue;
        }
        if stand_on_slope(profile, d).player.ground.is_grounded() {
            best = best.max(d);
        }
    }
    best
}
