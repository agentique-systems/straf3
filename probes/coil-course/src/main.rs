//! Does `assets/maps/coil.map` describe a course the real simulation can run?
//!
//! # Hypothesis
//!
//! H1. Every brush in coil.map compiles to a closed convex hull, and the player
//!     spawns in open air.
//! H2. The running surface along the course centre line is continuous at the
//!     heights the map's own comments claim, and the three ramp families
//!     classify as `Grounded` / `Grounded` / `Sliding` respectively.
//! H3. The speeds the course's jumps require (595 ups for the gully, 578 for
//!     the ramp-wave shortcut, 473 for the finish) are reachable by strafe
//!     jumping within the run-up each has.
//! H4. Each jump's stated landing window is what the simulation actually
//!     produces when a player leaves the lip at that speed.
//! H5. The course's trigger volumes, fed to the tracer as `TriggerHull`s, drive
//!     `RunState` from geometry alone: a bot that runs the corridor crosses
//!     `target_startTimer`, both checkpoints and `target_stopTimer` in order,
//!     and comes out the other side with a `u32` millisecond time.
//! H6. The *shipped* path does the same. `straf3_map::CompiledMap::collider()`
//!     — the one call `straf3-game`'s scene and `straf3-render` both go through
//!     — hands the timing volumes over, so a player in the native client
//!     produces a time rather than a `RunState` stuck at `NotStarted` forever.
//!     Evidenced by a command stream this phase writes out and the shipped
//!     `straf3 --replay` reproduces checksum-for-checksum.
//!
//! Any of these can come back false. H3 is the one most likely to: the arithmetic
//! that produced those numbers is a point-mass ballistic model, and the real
//! mover clips velocity into every plane it touches.
//!
//! H5 also carries a second, sharper question, which is why it prints two lists
//! rather than one. Sampling the trajectory once per decision window and asking
//! "was the hull inside a box at that instant" is the cheap way to detect a
//! crossing and is the bug ARCHITECTURE C4 exists to prevent; the accumulator
//! `Trace::triggers` feeds is the honest one. Both are printed. Where they
//! agree, that is a measurement about *this* course's volumes being generous,
//! not a licence to use the cheap one.
//!
//! # Which compiler each phase uses, and why they differ
//!
//! **H1–H5 use this file's own brush-to-hull path.** It was written while C7
//! was still in flight, so it is an independent second derivation of the same
//! compile step. That is the point: a probe that shared the compiler could not
//! disagree with it.
//!
//! **H6 uses `straf3-map`, deliberately.** Its question is not "is this course
//! runnable" but "does the thing the game ships hand the timing volumes over",
//! and only the shipped compiler can answer that. So H6 is the one phase whose
//! independence would be a bug rather than a virtue.
//!
//! The two worlds are *not* the same geometry and are not expected to be:
//! `compile_hull` adds the bevel planes a swept box needs and `Hull::from_planes`
//! here does not. So the bot's line differs between H3/H5 and H6, and a command
//! stream recorded against one does not reproduce in the other. That is why H6
//! generates its own fixture rather than reusing H3's run.
//!
//! # Running it
//!
//! ```sh
//! cd probes/coil-course
//! cargo run --release -- ../../assets/maps/coil.map | tee results/coil.txt
//! ```
//!
//! It takes a few minutes: the bot brute-forces 180 controls over a 40-tick
//! lookahead at every 8-tick decision window, all the way to the finish line,
//! and H3 and H6 each do it once.
//!
//! H6 writes `results/coil-run.txt`. Replaying it through the shipped binary is
//! the check that closes the loop, and it needs no window and no GPU:
//!
//! ```sh
//! cargo build --release -p straf3-game --bin straf3
//! ./target/release/straf3 --replay probes/coil-course/results/coil-run.txt \
//!     --map assets/maps/coil.map
//! ```
//!
//! Its `checksum` line must equal the one H6 prints. `SimState::checksum` folds
//! `RunState`, so that equality is not merely "the same path" — it is "the same
//! clock, started and stopped on the same commands".

use std::collections::BTreeMap;
use std::fs::File;

use straf3_collision::{Hull, HullWorld, Plane, TriggerHull};
use straf3_sim::num::{Scalar, Vec3, s, vec3};
use straf3_sim::state::RunState;
use straf3_sim::world::{SurfaceFlags, Sweep, TriggerSet, World};
use straf3_sim::{
    Buttons, GroundState, PhysicsProfile, SimState, TickRate, UserCmd, ViewAngles, step_in_place,
};

/// A trigger volume, kept as the axis-aligned box it is authored as.
struct Trigger {
    target: String,
    /// What crossing it means to the run clock, as the bit the simulation reads.
    set: TriggerSet,
    /// The classname the `target` key resolved to, or `?`.
    resolved: String,
    mins: Vec3,
    maxs: Vec3,
}

impl Trigger {
    /// Does the player hull at `origin` overlap this volume?
    fn overlaps(&self, origin: Vec3, half: Vec3, offset: Vec3) -> bool {
        let c = origin + offset;
        (0..3).all(|i| {
            let (lo, hi) = (c[i] - half[i], c[i] + half[i]);
            hi >= self.mins[i] && lo <= self.maxs[i]
        })
    }
}

struct Course {
    world: HullWorld,
    triggers: Vec<Trigger>,
    spawn: Vec3,
    spawn_yaw: Scalar,
    textures: Vec<String>,
}

fn v(p: [f64; 3]) -> Vec3 {
    vec3(p[0] as Scalar, p[1] as Scalar, p[2] as Scalar)
}

fn key<'a>(ent: &'a quake_map::Entity, k: &str) -> Option<&'a str> {
    ent.edict
        .iter()
        .find(|(ek, _)| ek.to_str().ok() == Some(k))
        .and_then(|(_, ev)| ev.to_str().ok())
}

fn parse_origin(text: &str) -> Option<Vec3> {
    let mut it = text.split_whitespace().map(|t| t.parse::<f32>());
    match (it.next(), it.next(), it.next()) {
        (Some(Ok(x)), Some(Ok(y)), Some(Ok(z))) => Some(vec3(x, y, z)),
        _ => None,
    }
}

fn load(path: &str) -> Course {
    let mut f = File::open(path).expect("open map");
    let map = quake_map::parse(&mut f).expect("parse map");

    let mut hulls = Vec::new();
    let mut textures = Vec::new();
    let mut triggers = Vec::new();
    let mut spawn = Vec3::ZERO;
    let mut spawn_yaw = s(0.0);

    // What each `targetname` in the file is, so a trigger brush can be told
    // what it means. The indirection is the Defrag convention: the brush the
    // player crosses says only `"target" "t_start"`, and what `t_start` *is*
    // lives in a point entity elsewhere in the file — so nothing can be
    // classified until the whole entity list has been read once.
    let mut targets: BTreeMap<String, String> = BTreeMap::new();
    for ent in &map.entities {
        if let (Some(name), Some(class)) = (key(ent, "targetname"), key(ent, "classname")) {
            targets.insert(name.to_ascii_lowercase(), class.to_ascii_lowercase());
        }
    }
    let mut next_checkpoint = 0u32;

    for ent in &map.entities {
        let class = key(ent, "classname").unwrap_or("");
        match class {
            "info_player_start" | "info_player_deathmatch" => {
                spawn = key(ent, "origin")
                    .and_then(parse_origin)
                    .expect("spawn origin");
                spawn_yaw = key(ent, "angle")
                    .and_then(|a| a.parse().ok())
                    .unwrap_or(s(0.0));
            }
            // Trigger brushes are volumes, not solids: they must never reach
            // the tracer as geometry or the player would bump into the finish
            // line. Their bounds come from the same planes all the same.
            "trigger_multiple" | "trigger_once" | "trigger_push" | "trigger_teleport" => {
                let target = key(ent, "target").unwrap_or("<untargeted>").to_string();
                let resolved = targets
                    .get(&target.to_ascii_lowercase())
                    .cloned()
                    .unwrap_or_else(|| "?".to_string());
                // The whole mapping from map data to the simulation's alphabet,
                // in six lines. `straf3-sim` never sees any of these names.
                let set = match resolved.as_str() {
                    "target_starttimer" => TriggerSet::START,
                    "target_stoptimer" => TriggerSet::FINISH,
                    "target_checkpoint" => {
                        let bit = TriggerSet::checkpoint(next_checkpoint)
                            .expect("more checkpoints than bits");
                        next_checkpoint += 1;
                        bit
                    }
                    _ => TriggerSet::NONE,
                };
                for brush in &ent.brushes {
                    let (mins, maxs) = brush_bounds(brush);
                    triggers.push(Trigger {
                        target: target.clone(),
                        set,
                        resolved: resolved.clone(),
                        mins,
                        maxs,
                    });
                }
            }
            _ => {
                for brush in &ent.brushes {
                    let planes: Vec<Plane> = brush
                        .iter()
                        .map(|face| {
                            let hs = face.half_space;
                            Plane::from_points(v(hs[0]), v(hs[1]), v(hs[2]))
                                .expect("degenerate brush face")
                        })
                        .collect();
                    let hull = Hull::from_planes(&planes, SurfaceFlags::NONE)
                        .expect("brush encloses no volume");
                    textures.push(brush[0].texture.to_str().unwrap_or("?").to_string());
                    hulls.push(hull);
                }
            }
        }
    }

    // The volumes go into the world as `TriggerHull`s, so the run clock is
    // driven by the geometry through `Trace::triggers` rather than by anything
    // this probe samples on the side. Boxes rather than the authored planes:
    // every trigger in coil.map is axis-aligned, and `brush_bounds` already
    // derived the box.
    let volumes: Vec<TriggerHull> = triggers
        .iter()
        .filter(|t| !t.set.is_empty())
        .map(|t| TriggerHull::new(t.set, Hull::from_aabb(t.mins, t.maxs, SurfaceFlags::NONE)))
        .collect();

    Course {
        world: HullWorld::new(hulls).with_triggers(volumes),
        triggers,
        spawn,
        spawn_yaw,
        textures,
    }
}

/// The same course, compiled by the **shipped** compiler.
///
/// H1-H5 run against this file's own brush-to-hull path, which is the point of
/// them: an independent derivation can disagree with `straf3-map` and say so.
/// H6 asks a different question — *does the thing the game ships hand the
/// timing volumes over?* — and that question can only be answered by the thing
/// the game ships. So this builds its [`Course`] out of
/// [`straf3_map::CompiledMap::collider`], the single choke point
/// `straf3-game`'s scene and `straf3-render` both go through.
///
/// The two worlds are not the same geometry, and that is expected rather than a
/// discrepancy: `compile_hull` adds the bevel planes a swept box needs and this
/// file's `Hull::from_planes` does not. A command stream recorded against one
/// will not reproduce in the other, which is exactly why the fixture H6 writes
/// has to be generated here.
fn shipped_course(source: &str) -> (Course, straf3_map::CompiledMap) {
    let map = straf3_map::compile(source).expect("the shipped compiler must accept coil.map");

    let triggers = map
        .triggers
        .iter()
        .map(|volume| Trigger {
            target: volume
                .target
                .clone()
                .unwrap_or_else(|| "<untargeted>".into()),
            set: volume.kind.trigger_set().unwrap_or(TriggerSet::NONE),
            resolved: volume
                .target_classname
                .clone()
                .unwrap_or_else(|| "?".into()),
            mins: volume.bounds.mins,
            maxs: volume.bounds.maxs,
        })
        .collect();

    let course = Course {
        // The whole assertion of H6 is in this one call: everything downstream
        // is the same bot, the same mover and the same clock as H3/H5.
        world: map.collider(),
        triggers,
        spawn: map.spawn,
        spawn_yaw: map.spawn_yaw,
        textures: Vec::new(),
    };
    (course, map)
}

/// Write `cmds` in `straf3-headless`'s input format, byte-compatible with what
/// `straf3-game`'s `Recorder::to_fixture` emits.
///
/// Written here rather than reused because `straf3-game` is above the seam and
/// a probe below it cannot depend on the window. The format is a text contract,
/// so a second writer of it is a check on the contract rather than a fork: if
/// this drifts, `straf3 --replay` rejects the file by name instead of running a
/// different session.
///
/// Angles go out as degrees via `{:?}`, which is f32's shortest round-tripping
/// form, and come back through `ViewAngles::from_degrees`, which re-quantises
/// to the same 16 bits (C3). The checksum comparison in H6 is what proves that
/// round trip is lossless rather than assuming it.
fn fixture_text(cmds: &[UserCmd], rate: TickRate, spawn: Vec3, spawn_yaw: Scalar) -> String {
    let mut out = String::with_capacity(48 * cmds.len() + 512);
    out.push_str("# Generated by probes/coil-course, phase H6.\n");
    out.push_str(
        "# A greedy strafe-jumping bot's run of assets/maps/coil.map, driven against\n\
         # the SHIPPED collider (`straf3_map::CompiledMap::collider`). Replay it with\n\
         # `straf3 --replay <this file> --map assets/maps/coil.map` and the final\n\
         # checksum must equal the one H6 prints -- that equality is what says the\n\
         # shipped binary crossed the same start and finish volumes this did.\n\n",
    );
    out.push_str(&format!("rate {}\n", rate.hz()));
    out.push_str("profile cpm\n");
    out.push_str("world map\n");
    out.push_str(&format!(
        "spawn {:?} {:?} {:?}\n",
        spawn.x, spawn.y, spawn.z
    ));
    out.push_str(&format!("yaw {spawn_yaw:?}\n\n"));
    out.push_str("# cmd <repeat> <fwd> <right> <up> <buttons> <pitch> <yaw> <roll>\n");

    let mut index = 0;
    while index < cmds.len() {
        let cmd = cmds[index];
        let mut repeat = 1;
        while index + repeat < cmds.len() && cmds[index + repeat] == cmd {
            repeat += 1;
        }
        let buttons = if cmd.buttons == Buttons::JUMP {
            "jump"
        } else {
            "-"
        };
        out.push_str(&format!(
            "cmd {} {} {} {} {} {:?} {:?} {:?}\n",
            repeat,
            cmd.forward_move,
            cmd.right_move,
            cmd.up_move,
            buttons,
            cmd.view.pitch_degrees(),
            cmd.view.yaw_degrees(),
            cmd.view.roll_degrees(),
        ));
        index += repeat;
    }
    out
}

/// Bounds of a brush, from the axis-aligned faces it is authored with.
fn brush_bounds(brush: &quake_map::Brush) -> (Vec3, Vec3) {
    let planes: Vec<Plane> = brush
        .iter()
        .map(|f| {
            Plane::from_points(v(f.half_space[0]), v(f.half_space[1]), v(f.half_space[2])).unwrap()
        })
        .collect();
    let hull = Hull::from_planes(&planes, SurfaceFlags::NONE).expect("trigger encloses no volume");
    let mut lo = Vec3::splat(Scalar::INFINITY);
    let mut hi = Vec3::splat(Scalar::NEG_INFINITY);
    for w in hull.windings() {
        for p in w {
            lo = lo.min(p);
            hi = hi.max(p);
        }
    }
    (lo, hi)
}

// ---------------------------------------------------------------------------

fn classify(normal: Vec3, profile: &PhysicsProfile) -> &'static str {
    if normal.z >= profile.min_walk_normal {
        "Grounded"
    } else {
        "Sliding "
    }
}

/// Drop a player hull straight down and report what it lands on.
fn ground_under(
    course: &Course,
    x: Scalar,
    y: Scalar,
    from_z: Scalar,
    profile: &PhysicsProfile,
) -> Option<(Scalar, Vec3)> {
    let half = profile.hull_half_extents();
    let offset = profile.hull_center_offset();
    let t = course.world.trace(&Sweep {
        start: vec3(x, y, from_z),
        end: vec3(x, y, s(-2048.0)),
        half_extents: half,
        center_offset: offset,
    });
    if t.fraction >= s(1.0) || t.start_solid {
        return None;
    }
    let origin_z = from_z + (s(-2048.0) - from_z) * t.fraction;
    // The surface itself is the underside of the hull.
    Some((origin_z + offset.z - half.z, t.normal))
}

// ---------------------------------------------------------------------------
// Phase B — how fast can the corridor actually be run?

/// One greedy strafe-jumping decision: try every (yaw rate, strafe, forward)
/// and keep the one whose simulated future is fastest along `goal`.
///
/// Greedy one-step-per-window hill climbing, not an optimal line. That makes
/// every speed it reports a LOWER bound on what a player can do, which is the
/// direction a course-validation probe wants to be wrong in.
struct Bot {
    rate: TickRate,
    lookahead: u32,
    decide_every: u32,
    /// Once the player is past `.0` in y, stop running for distance and start
    /// running *at* the point `.1` — the finish volume's own centre.
    ///
    /// Off (`None`) for the measuring phases, whose numbers are about what the
    /// course affords and must not move because a later phase was added.
    ///
    /// H6 turns it on because its job is different: it has to *complete* a run,
    /// and a finish line is a box with six sides. Maximising `y` alone walks
    /// past one shoulder of it — observed, ending at x=-143 against a volume
    /// spanning x=-96..96 with the clock still running — and steering on `x`
    /// alone walks off the ledge in front of it, observed at z=-125 against a
    /// volume spanning z=64..768. Neither is a collider that lost a volume, and
    /// being able to tell those apart is the whole point of the phase.
    ///
    /// The switch reads the *current* state, not the candidate future, so every
    /// control in one decision window is scored on the same scale. Switching on
    /// the candidate would compare a distance against a y coordinate.
    funnel: Option<(Scalar, Vec3)>,
}

#[derive(Clone, Copy)]
struct Control {
    yaw_rate: Scalar,
    right: i8,
    forward: i8,
    jump: bool,
}

const YAW_RATES: [Scalar; 15] = [
    -12.0, -9.0, -7.0, -5.5, -4.0, -2.5, -1.0, 0.0, 1.0, 2.5, 4.0, 5.5, 7.0, 9.0, 12.0,
];

impl Bot {
    fn controls() -> Vec<Control> {
        let mut out = Vec::new();
        for &yaw_rate in &YAW_RATES {
            for right in [-127i8, 0, 127] {
                for forward in [0i8, 127] {
                    for jump in [true, false] {
                        out.push(Control {
                            yaw_rate,
                            right,
                            forward,
                            jump,
                        });
                    }
                }
            }
        }
        out
    }

    /// Apply one control for `ticks`, returning the resulting state.
    ///
    /// Returns the volumes those commands passed through as well as the state.
    /// Speculative lookahead calls throw that away — a future the bot considered
    /// and did not take must not start the clock — and only the committed call
    /// in [`Bot::run`] keeps it. That is the same distinction `Pmove` draws one
    /// level down between a probe and a committed sweep.
    ///
    /// `record`, when given, collects the commands actually issued. Speculation
    /// passes `None`: a future the bot considered and did not take must not
    /// reach a recording any more than it may start the clock.
    fn hold(
        &self,
        state: &SimState,
        c: Control,
        ticks: u32,
        course: &Course,
        profile: &PhysicsProfile,
        mut record: Option<&mut Vec<UserCmd>>,
    ) -> (SimState, TriggerSet) {
        let mut st = *state;
        let mut touched = TriggerSet::NONE;
        for _ in 0..ticks {
            // Bunny hopping: Q3 requires releasing jump between hops, so the
            // button is pressed only when there is ground to leave.
            let grounded = matches!(st.player.ground, GroundState::Grounded { .. });
            let buttons = if c.jump && grounded {
                Buttons::JUMP
            } else {
                Buttons::NONE
            };
            // C3 made ViewAngles 16-bit at the command boundary, so the turn
            // has to accumulate in degrees and quantise on the way into the
            // command — which is what a real recording does, and what the
            // state carries back on the next tick.
            let yaw = st.player.view.yaw_degrees() + c.yaw_rate;
            let cmd = UserCmd {
                duration_ms: self.rate.command_millis(),
                forward_move: c.forward,
                right_move: c.right,
                up_move: 0,
                buttons,
                view: ViewAngles::from_degrees(s(0.0), yaw, s(0.0)),
            };
            if let Some(rec) = record.as_deref_mut() {
                rec.push(cmd);
            }
            touched = touched.with(step_in_place(&mut st, &cmd, &course.world, profile));
        }
        (st, touched)
    }

    /// Run from `state` until `until_y`, or until `max_ticks` runs out.
    /// Returns the trajectory sampled once per decision window.
    fn run(
        &self,
        mut state: SimState,
        until_y: Scalar,
        max_ticks: u32,
        course: &Course,
        profile: &PhysicsProfile,
    ) -> (
        SimState,
        Vec<(u32, Vec3, Scalar)>,
        Vec<(u32, u32, String)>,
        Vec<UserCmd>,
    ) {
        let controls = Self::controls();
        let mut trail = Vec::new();
        let mut committed: Vec<UserCmd> = Vec::new();
        let mut crossings: Vec<(u32, u32, String)> = Vec::new();
        let mut seen = TriggerSet::NONE;
        let mut ticks = 0;
        while ticks < max_ticks
            && state.player.origin.y < until_y
            && !matches!(state.run, RunState::Finished { .. })
        {
            let mut best = None;
            let mut best_score = Scalar::NEG_INFINITY;
            // Decided once per window, from the state every candidate shares.
            let aim_at = self
                .funnel
                .filter(|(from_y, _)| state.player.origin.y > *from_y)
                .map(|(_, centre)| centre);
            for &c in &controls {
                let (f, _speculative) = self.hold(&state, c, self.lookahead, course, profile, None);
                if !f.player.origin.is_finite() {
                    continue;
                }
                let speed = vec3(f.player.velocity.x, f.player.velocity.y, s(0.0)).length();
                // Progress along the course dominates; speed breaks ties, which
                // is what stops the bot trading its whole run for one long slide.
                let score = match aim_at {
                    // Endgame: get the hull inside the box. Speed still breaks
                    // ties, at a tenth the weight, so the bot does not stall
                    // one unit short of the face.
                    Some(centre) => -(f.player.origin - centre).length() + s(0.1) * speed,
                    None => {
                        f.player.origin.y + s(0.25) * speed
                            - s(0.5) * (f.player.origin.z - state.player.origin.z).min(s(0.0))
                    }
                };
                if score > best_score {
                    best_score = score;
                    best = Some(c);
                }
            }
            let Some(c) = best else { break };
            let (next, touched) = self.hold(
                &state,
                c,
                self.decide_every,
                course,
                profile,
                Some(&mut committed),
            );
            state = next;
            ticks += self.decide_every;

            // Edge detection is the caller's job: the accumulator reports
            // *overlapped this command*, and a player is inside a start volume
            // for several commands running.
            for volume in &course.triggers {
                if volume.set.is_empty()
                    || !touched.contains(volume.set)
                    || seen.contains(volume.set)
                {
                    continue;
                }
                seen = seen.with(volume.set);
                crossings.push((
                    state.time_ms,
                    state.run.elapsed_ms(state.time_ms).unwrap_or(0),
                    volume.target.clone(),
                ));
            }

            let speed = vec3(state.player.velocity.x, state.player.velocity.y, s(0.0)).length();
            trail.push((ticks, state.player.origin, speed));
        }
        (state, trail, crossings, committed)
    }
}

// ---------------------------------------------------------------------------
// Phase C — the ballistic gates, measured rather than derived

/// Launch from `origin` at `speed` along +Y, jump on the first tick, and report
/// where the player comes to rest on ground again.
fn ballistic(
    course: &Course,
    origin: Vec3,
    speed: Scalar,
    profile: &PhysicsProfile,
    rate: TickRate,
) -> (Vec3, Scalar) {
    let mut st = SimState::spawned_at(origin, s(90.0));
    st.player.velocity = vec3(s(0.0), speed, s(0.0));
    // Settle one tick so the mover reports the ground it is standing on, then
    // jump. Jumping from a state that has not seen the floor yet would measure
    // a fall, not a jump.
    let still = UserCmd {
        duration_ms: rate.command_millis(),
        forward_move: 0,
        right_move: 0,
        up_move: 0,
        buttons: Buttons::NONE,
        view: ViewAngles::from_degrees(s(0.0), s(90.0), s(0.0)),
    };
    step_in_place(&mut st, &still, &course.world, profile);
    st.player.velocity = vec3(s(0.0), speed, s(0.0));
    let jump = UserCmd {
        buttons: Buttons::JUMP,
        ..still
    };
    step_in_place(&mut st, &jump, &course.world, profile);

    let mut airborne_seen = false;
    for _ in 0..400 {
        step_in_place(&mut st, &still, &course.world, profile);
        match st.player.ground {
            GroundState::Airborne => airborne_seen = true,
            _ if airborne_seen => break,
            _ => {}
        }
        if st.player.origin.z < s(-1024.0) {
            break;
        }
    }
    let sp = vec3(st.player.velocity.x, st.player.velocity.y, s(0.0)).length();
    (st.player.origin, sp)
}

// ---------------------------------------------------------------------------

fn main() {
    let path = std::env::args()
        .nth(1)
        .unwrap_or_else(|| "../../assets/maps/coil.map".into());
    let profile = PhysicsProfile::cpm();
    let rate = TickRate::HZ_125;
    let course = load(&path);
    let half = profile.hull_half_extents();
    let offset = profile.hull_center_offset();

    println!("== coil-course probe ==");
    println!("map          {path}");
    println!("solids       {} hulls", course.world.hulls().len());
    println!("triggers     {}", course.triggers.len());
    for t in &course.triggers {
        let meaning = if t.set == TriggerSet::START {
            "START".to_string()
        } else if t.set == TriggerSet::FINISH {
            "FINISH".to_string()
        } else if t.set.is_empty() {
            "(untimed)".to_string()
        } else {
            format!("checkpoint bit {:#010x}", t.set.0)
        };
        println!(
            "   {:<12} -> {:<20} {:<24} {:?} .. {:?}",
            t.target, t.resolved, meaning, t.mins, t.maxs
        );
    }
    println!(
        "clock        the world reports {:#010x}; a timeable map needs START|FINISH",
        course.world.trigger_coverage().0
    );
    let bounds = course.world.bounds().unwrap();
    println!("bounds       {:?} .. {:?}", bounds.0, bounds.1);
    println!("profile      cpm, {} Hz", rate.hz());

    // ---- H1: the spawn ----------------------------------------------------
    println!("\n== H1  spawn ==");
    let t = course.world.trace(&Sweep {
        start: course.spawn,
        end: course.spawn,
        half_extents: half,
        center_offset: offset,
    });
    println!(
        "spawn {:?} yaw {}  start_solid={}  all_solid={}",
        course.spawn, course.spawn_yaw, t.start_solid, t.all_solid
    );
    let under = ground_under(
        &course,
        course.spawn.x,
        course.spawn.y,
        course.spawn.z,
        &profile,
    );
    println!("ground under spawn: {under:?}  (want surface z=0, normal z=1)");

    // ---- H2: the running surface -----------------------------------------
    println!("\n== H2  surface survey along x=0, every 32 units of y ==");
    println!(
        "{:>7} {:>9} {:>9} {:>10}",
        "y", "surface", "normal.z", "state"
    );
    let mut y = s(-800.0);
    let mut gaps: Vec<Scalar> = Vec::new();
    let mut families: BTreeMap<&str, (Scalar, Scalar, u32)> = BTreeMap::new();
    while y <= s(3968.0) {
        match ground_under(&course, s(0.0), y, s(900.0), &profile) {
            Some((z, n)) => {
                let state = classify(n, &profile);
                println!("{y:>7.0} {z:>9.1} {:>9.4} {state:>10}", n.z);
                let e = families.entry(state).or_insert((n.z, n.z, 0));
                e.0 = e.0.min(n.z);
                e.1 = e.1.max(n.z);
                e.2 += 1;
            }
            None => {
                println!("{y:>7.0} {:>9} {:>9} {:>10}", "-", "-", "VOID");
                gaps.push(y);
            }
        }
        y += s(32.0);
    }
    println!("\nvoid samples on the centre line: {}", gaps.len());
    if !gaps.is_empty() {
        println!("  at y = {gaps:?}");
    }
    for (k, (lo, hi, n)) in &families {
        println!("  {k}: {n} samples, normal.z {lo:.4}..{hi:.4}");
    }

    // ---- H4: the ballistic gates -----------------------------------------
    // Measured before H3 because H3's verdict is read against these numbers.
    println!("\n== H4  jump gates, launched along +Y at a sweep of speeds ==");
    let gates: [(&str, Vec3, Scalar, Scalar); 3] = [
        // (name, launch origin (feet on the lip), first speed, step)
        (
            "ramp-wave shortcut",
            vec3(s(0.0), s(1780.0), s(112.0 + 24.0)),
            s(350.0),
            s(25.0),
        ),
        (
            "the gully           ",
            vec3(s(0.0), s(2292.0), s(128.0 + 24.0)),
            s(450.0),
            s(25.0),
        ),
        (
            "the finish          ",
            vec3(s(0.0), s(3124.0), s(64.0 + 24.0)),
            s(350.0),
            s(25.0),
        ),
    ];
    for (name, origin, first, stepping) in gates {
        println!("\n-- {name} from {:?}", origin);
        println!(
            "{:>8} {:>10} {:>9} {:>10}",
            "ups", "landed y", "landed z", "exit ups"
        );
        let mut sp = first;
        while sp <= s(1000.0) {
            let (end, exit) = ballistic(&course, origin, sp, &profile, rate);
            println!("{sp:>8.0} {:>10.0} {:>9.0} {exit:>10.0}", end.y, end.z);
            sp += stepping;
        }
    }

    // ---- H3: is that speed reachable? ------------------------------------
    println!("\n== H3  strafe-jumping from the spawn ==");
    let bot = Bot {
        rate,
        lookahead: 40,
        decide_every: 8,
        funnel: None,
    };
    let start = SimState::spawned_at(course.spawn, course.spawn_yaw);
    // Run for the finish line, not for a y coordinate: `Bot::run` stops when
    // `RunState` says the clock has stopped. The y limit is only a backstop for
    // a bot that gets past the finish volume without crossing it.
    let (end, trail, crossings, _cmds) = bot.run(start, s(3600.0), 20_000, &course, &profile);
    println!(
        "{:>7} {:>9} {:>9} {:>9} {:>9}",
        "tick", "x", "y", "z", "ups"
    );
    for (t, p, sp) in trail.iter().step_by(4) {
        println!("{t:>7} {:>9.0} {:>9.0} {:>9.0} {sp:>9.0}", p.x, p.y, p.z);
    }
    let peak = trail.iter().map(|(_, _, s)| *s).fold(s(0.0), Scalar::max);
    println!(
        "\nreached y={:.0} z={:.0} in {} ms; peak horizontal speed {peak:.0} ups",
        end.player.origin.y, end.player.origin.z, end.time_ms
    );

    // ---- H5: does the geometry produce a time? ----------------------------
    //
    // The two ways of asking, side by side. The first samples the trajectory
    // once per decision window and asks whether the hull overlapped a box at
    // that instant; the second is the accumulator the simulation itself keeps,
    // fed by every committed sweep inside every command. The gap between them
    // is the bug ARCHITECTURE C4 exists to prevent, and it is reported rather
    // than argued.
    println!("\n== H5  the run clock ==");
    let mut sampled: Vec<&str> = Vec::new();
    for (_, p, _) in &trail {
        for tr in &course.triggers {
            if tr.overlaps(*p, half, offset) && !sampled.contains(&tr.target.as_str()) {
                sampled.push(&tr.target);
            }
        }
    }
    println!("sampled once per decision window : {sampled:?}");
    let swept: Vec<&str> = crossings.iter().map(|(_, _, n)| n.as_str()).collect();
    println!("swept, from Trace::triggers      : {swept:?}");

    // The bot commits 8 commands per decision, so this column is the end of the
    // window a crossing was detected in, not the command that crossed. The
    // clock itself has no such coarseness — `RunState` below is stamped by the
    // command that touched the line — and the two are printed together rather
    // than one being passed off as the other.
    println!("\n{:>12} {:>10}  volume", "window end", "run ms");
    for (at, elapsed, name) in &crossings {
        println!("{at:>12} {elapsed:>10}  {name}");
    }
    match end.run {
        RunState::NotStarted => println!("\nclock: never started"),
        RunState::Running { started_at_ms } => println!(
            "\nclock: RUNNING, started at {started_at_ms} ms, {} ms elapsed at the cut-off",
            end.run.elapsed_ms(end.time_ms).unwrap_or(0)
        ),
        RunState::Finished {
            started_at_ms,
            finished_at_ms,
        } => {
            let ms = finished_at_ms - started_at_ms;
            println!(
                "\nclock: FINISHED  start {started_at_ms} ms  finish {finished_at_ms} ms  \
                 time {}.{:03} s ({ms} ms, u32)",
                ms / 1000,
                ms % 1000
            );
        }
    }

    // ---- H6: does the SHIPPED collider carry the volumes? ----------------
    //
    // Everything above this line runs in a world this file built. A player does
    // not: they run in `CompiledMap::collider()`'s world, reached through
    // `straf3-game`'s scene. Until that call attaches the timing volumes, the
    // clock is correct and never fed, and the difference is invisible from any
    // test that builds its own world -- including every one above.
    println!("\n== H6  the shipped collider, and a fixture the shipped binary can replay ==");
    let source = std::fs::read_to_string(&path).expect("read map source");
    let (shipped, map) = shipped_course(&source);
    println!(
        "straf3-map compiled {} solids, {} trigger volumes, collision digest {:#018x}",
        map.hulls.len(),
        map.triggers.len(),
        map.collision_digest()
    );
    for w in &map.warnings {
        println!("  warning: {w:?}");
    }
    let coverage = shipped.world.trigger_coverage();
    println!(
        "collider(): {} solid hulls, {} timing volumes, coverage {:#010x}",
        shipped.world.hulls().len(),
        shipped.world.triggers().len(),
        coverage.0
    );
    println!(
        "has_timing()={}  START={}  FINISH={}",
        map.has_timing(),
        coverage.contains(TriggerSet::START),
        coverage.contains(TriggerSet::FINISH)
    );
    if !coverage.contains(TriggerSet::START) || !coverage.contains(TriggerSet::FINISH) {
        println!(
            "\nH6 FALSIFIED: the shipped collider has no start/finish pair, so a player \
             in the native client can never produce a time no matter how well they run."
        );
    }

    // Aim at the finish line's own centre, read out of the volume rather than
    // written down here, and stop past its far face rather than at a guess.
    let finish = shipped
        .triggers
        .iter()
        .find(|t| t.set == TriggerSet::FINISH)
        .expect("H6 needs a finish volume; coverage said there is one");
    // The centre of the volume's floor rather than of the volume: coil.map's
    // finish is 704 units tall and a player crosses it at running height, so
    // aiming at the middle of the column would aim 350 units into the air.
    let centre = vec3(
        s(0.5) * (finish.mins.x + finish.maxs.x),
        s(0.5) * (finish.mins.y + finish.maxs.y),
        finish.mins.z + s(48.0),
    );
    let aiming = Bot {
        funnel: Some((finish.mins.y - s(384.0), centre)),
        ..bot
    };
    println!(
        "finish volume x {:.0}..{:.0}  y {:.0}..{:.0}  z {:.0}..{:.0}",
        finish.mins.x, finish.maxs.x, finish.mins.y, finish.maxs.y, finish.mins.z, finish.maxs.z
    );
    println!(
        "aiming at {:.0} {:.0} {:.0} once past y={:.0}",
        centre.x,
        centre.y,
        centre.z,
        finish.mins.y - s(384.0)
    );

    let shipped_start = SimState::spawned_at(shipped.spawn, shipped.spawn_yaw);
    let (shipped_end, _shipped_trail, shipped_crossings, cmds) = aiming.run(
        shipped_start,
        finish.maxs.y + s(256.0),
        4_000,
        &shipped,
        &profile,
    );
    println!(
        "\nbot committed {} commands at {} Hz ({} ms)",
        cmds.len(),
        rate.hz(),
        shipped_end.time_ms
    );
    println!("\n{:>12} {:>10}  volume", "window end", "run ms");
    for (at, elapsed, name) in &shipped_crossings {
        println!("{at:>12} {elapsed:>10}  {name}");
    }
    match shipped_end.run {
        RunState::NotStarted => println!("\nclock: never started"),
        RunState::Running { started_at_ms } => println!(
            "\nclock: RUNNING, started at {started_at_ms} ms, {} ms elapsed at the cut-off",
            shipped_end.run.elapsed_ms(shipped_end.time_ms).unwrap_or(0)
        ),
        RunState::Finished {
            started_at_ms,
            finished_at_ms,
        } => {
            let ms = finished_at_ms - started_at_ms;
            println!(
                "\nclock: FINISHED  start {started_at_ms} ms  finish {finished_at_ms} ms  \
                 time {}.{:03} s ({ms} ms, u32)",
                ms / 1000,
                ms % 1000
            );
        }
    }

    // The number the shipped binary has to reproduce. `SimState::checksum`
    // folds `RunState`, so a replay that agrees on this digit-for-digit did not
    // merely follow the same path -- it started and stopped its clock on the
    // same commands. That is the whole of H6's evidence, and it needs no change
    // anywhere above the seam to collect.
    println!(
        "\nfinal checksum {:#018x}  (origin {:.3} {:.3} {:.3})",
        shipped_end.checksum(),
        shipped_end.player.origin.x,
        shipped_end.player.origin.y,
        shipped_end.player.origin.z
    );
    let fixture = fixture_text(&cmds, rate, shipped.spawn, shipped.spawn_yaw);
    let fixture_path = "results/coil-run.txt";
    match std::fs::write(fixture_path, &fixture) {
        Ok(()) => println!(
            "wrote {fixture_path} ({} bytes, {} cmd lines)\n\nverify with:\n  \
             cargo run --release -p straf3-game --bin straf3 -- \\\n    \
             --replay probes/coil-course/{fixture_path} --map assets/maps/coil.map",
            fixture.len(),
            fixture.lines().filter(|l| l.starts_with("cmd ")).count()
        ),
        Err(e) => println!("could not write {fixture_path}: {e}"),
    }

    println!("\n== texture families in the compiled solids ==");
    let mut counts: BTreeMap<&str, u32> = BTreeMap::new();
    for t in &course.textures {
        *counts.entry(t.as_str()).or_default() += 1;
    }
    for (k, n) in counts {
        println!("  {k:<18} {n}");
    }
}
