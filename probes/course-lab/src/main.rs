//! A course lab: compile any Straf3 map, measure its gates, and search it for a
//! line that satisfies a stated goal — with the search's objective, alphabet,
//! budget and forbidden regions all given as arguments rather than compiled in.
//!
//! # Why this exists next to `probes/coil-course`
//!
//! `coil-course` answered "does coil.map describe a course the real simulation
//! can run", and it answered it well. Three of its properties do not survive
//! contact with a map that has a fork in it, and none of them is a defect — they
//! are the shape of a probe written for one map:
//!
//! - its surface survey samples `x = 0` only, which is the centre line of a
//!   corridor and nowhere in particular on a map with a turn;
//! - its jump gates are three hardcoded launch points, swept along `+Y` at one
//!   yaw, so they measure one approach rather than the worst one;
//! - its search maximises `origin.y` and then minimises distance to a *point*
//!   inside the finish volume (`main.rs:566-574`). A fork's second branch is
//!   worse in `y` for a while by construction, so a `y`-progress objective
//!   cannot search it at all; and the clock is satisfied by *crossing a volume*
//!   (`step.rs:338-343`), which is a different problem from arriving at a point.
//!
//! It also writes `results/coil-run.txt` unconditionally whatever map it is
//! given (`main.rs:996`). Every output path here is derived from the map's file
//! stem, so running this on map A can never overwrite map B's evidence.
//!
//! # What a negative from this tool is worth
//!
//! A *positive* finding — "here is a line, here is the fixture" — verifies
//! itself: the command stream replays through the shipped `straf3` binary to a
//! checksum, and anyone can check that without trusting this program. A
//! *negative* — "no such line was found" — rests entirely on the search being
//! adequate, so every negative prints its coverage: states explored, commands
//! simulated, horizon, decision stride, control-set size, alphabet, budget and
//! termination reason. A negative without those numbers is not a result, and
//! this tool will not print one.
//!
//! # Running it
//!
//! ```sh
//! cd probes/course-lab
//! cargo run --release -- ../../assets/maps/cleave.map compile
//! cargo run --release -- ../../assets/maps/cleave.map gates --gate ...
//! cargo run --release -- ../../assets/maps/cleave.map search --goal finish
//! ```
//!
//! `--help` prints the full argument grammar.

use std::collections::BTreeMap;
use std::fmt::Write as _;

use straf3_map::{Aabb, CompiledMap, TriggerKind};
use straf3_sim::num::{Scalar, Vec3, s, vec3};
use straf3_sim::state::RunState;
use straf3_sim::world::{Sweep, TriggerSet, World};
use straf3_sim::{
    Buttons, GroundState, PhysicsProfile, SimState, TickRate, UserCmd, ViewAngles, step_in_place,
};
use straf3_collision::HullWorld;

// ---------------------------------------------------------------------------
// The course: the shipped compiler's world, and nothing derived a second time.

struct Volume {
    /// The `target` key, so a human can name it on the command line.
    name: String,
    /// The classname the target resolved to, or `?`.
    resolved: String,
    kind: TriggerKind,
    /// The simulation's own alphabet for this volume. Empty for volumes the
    /// clock has no bit for.
    set: TriggerSet,
    bounds: Aabb,
}

impl Volume {
    fn centre(&self) -> Vec3 {
        (self.bounds.mins + self.bounds.maxs) * s(0.5)
    }

    /// Distance from a player hull at `origin` to this box, zero once the hull
    /// overlaps it.
    ///
    /// This is the objective the run clock actually reads, expressed as a
    /// number a search can descend: the clock asks whether the swept hull
    /// *overlapped a volume* and this asks how far it is from doing so. A
    /// distance-to-centre objective answers a different question and prefers a
    /// line that stops in the middle of the box over one that passes through it
    /// at speed — which is how a searcher misses a fly-through.
    fn hull_distance(&self, origin: Vec3, half: Vec3, offset: Vec3) -> Scalar {
        let c = origin + offset;
        let mut d2 = s(0.0);
        for i in 0..3 {
            let lo = self.bounds.mins[i] - half[i];
            let hi = self.bounds.maxs[i] + half[i];
            let over = if c[i] < lo {
                lo - c[i]
            } else if c[i] > hi {
                c[i] - hi
            } else {
                s(0.0)
            };
            d2 += over * over;
        }
        d2.sqrt()
    }

}

/// Whether a player hull at `origin` overlaps `b`.
fn hull_in_box(b: &Aabb, origin: Vec3, half: Vec3, offset: Vec3) -> bool {
    let c = origin + offset;
    (0..3).all(|i| c[i] + half[i] >= b.mins[i] && c[i] - half[i] <= b.maxs[i])
}

struct Course {
    world: HullWorld,
    volumes: Vec<Volume>,
    spawn: Vec3,
    spawn_yaw: Scalar,
    map: CompiledMap,
    stem: String,
}

fn load(path: &str) -> Course {
    let source = std::fs::read_to_string(path).unwrap_or_else(|e| panic!("read {path}: {e}"));
    let map = straf3_map::compile(&source)
        .unwrap_or_else(|e| panic!("the shipped compiler rejected {path}: {e:?}"));
    let volumes = map
        .triggers
        .iter()
        .map(|v| Volume {
            name: v.target.clone().unwrap_or_else(|| "<untargeted>".into()),
            resolved: v.target_classname.clone().unwrap_or_else(|| "?".into()),
            kind: v.kind,
            set: v.kind.trigger_set().unwrap_or(TriggerSet::NONE),
            bounds: v.bounds,
        })
        .collect();
    let stem = std::path::Path::new(path)
        .file_stem()
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_else(|| "map".into());
    Course {
        world: map.collider(),
        volumes,
        spawn: map.spawn,
        spawn_yaw: map.spawn_yaw,
        map,
        stem,
    }
}

impl Course {
    /// Resolve a volume selector to the bits it names.
    ///
    /// `finish`, `start`, `cp:N`, `mask:0x..`, or a `target` name from the map.
    fn select(&self, spec: &str) -> Option<TriggerSet> {
        if let Some(hex) = spec.strip_prefix("mask:") {
            let raw = hex.trim_start_matches("0x");
            return u32::from_str_radix(raw, 16).ok().map(TriggerSet);
        }
        if let Some(n) = spec.strip_prefix("cp:") {
            return n.parse().ok().and_then(TriggerSet::checkpoint);
        }
        match spec {
            "start" => return Some(TriggerSet::START),
            "finish" => return Some(TriggerSet::FINISH),
            _ => {}
        }
        self.volumes
            .iter()
            .find(|v| v.name.eq_ignore_ascii_case(spec))
            .map(|v| v.set)
    }

    fn volumes_for(&self, set: TriggerSet) -> impl Iterator<Item = &Volume> {
        self.volumes
            .iter()
            .filter(move |v| !v.set.is_empty() && set.contains(v.set))
    }
}

// ---------------------------------------------------------------------------
// The searcher.

/// What inputs the searcher is allowed to press.
///
/// A parameter rather than a constant because whether a map's structure depends
/// on posture is decided entirely by this, and a baseline result reported
/// without stating its alphabet is not reproducible. `probes/coil-course` sets
/// `up_move: 0` and presses only JUMP or NONE, which is why it provably cannot
/// finish a map with a lintel — the fact is a property of its alphabet, not of
/// its search.
#[derive(Clone, Copy, PartialEq)]
struct Alphabet {
    crouch: bool,
}

impl Alphabet {
    fn label(self) -> &'static str {
        if self.crouch { "jump+crouch" } else { "jump-only" }
    }
}

/// How the searcher scores a candidate future.
#[derive(Clone, Copy, PartialEq)]
enum Progress {
    /// Raw `origin.y`. `probes/coil-course`'s metric, kept so the trivial
    /// baseline can be reproduced exactly rather than described.
    NorthY,
    /// Distance from the hull to the next goal volume it has not yet touched.
    /// The only metric that can search a branch which is worse in `y`.
    ToGoal,
}

/// A region the line must not enter.
///
/// Two forms, because a bypass hypothesis comes in two shapes: "reach FINISH
/// without touching checkpoint N" is a mask, and "reach FINISH without entering
/// this piece of the map" is a box.
#[derive(Clone)]
enum Forbid {
    Bits(TriggerSet),
    Box(Aabb),
}

#[derive(Clone, Copy)]
struct Control {
    yaw_rate: Scalar,
    right: i8,
    forward: i8,
    jump: bool,
    crouch: bool,
}

const YAW_RATES: [Scalar; 15] = [
    -12.0, -9.0, -7.0, -5.5, -4.0, -2.5, -1.0, 0.0, 1.0, 2.5, 4.0, 5.5, 7.0, 9.0, 12.0,
];

fn controls(alphabet: Alphabet) -> Vec<Control> {
    let mut out = Vec::new();
    for &yaw_rate in &YAW_RATES {
        for right in [-127i8, 0, 127] {
            for forward in [0i8, 127] {
                for jump in [true, false] {
                    for crouch in if alphabet.crouch {
                        &[false, true][..]
                    } else {
                        &[false][..]
                    } {
                        out.push(Control {
                            yaw_rate,
                            right,
                            forward,
                            jump,
                            crouch: *crouch,
                        });
                    }
                }
            }
        }
    }
    out
}

/// Why a search stopped. Printed with every negative.
#[derive(Debug, PartialEq)]
enum Stop {
    GoalReached,
    BudgetExhausted,
    NoAdmissibleControl,
    EnteredForbidden,
}

struct SearchSpec {
    /// The goal, as bits that must all have been touched. A volume-crossing
    /// predicate: the searcher stops when the *swept* hull has overlapped every
    /// named volume, which is the same question `step` answers for the clock.
    goal: TriggerSet,
    /// Ordered waypoints. The searcher descends toward the first goal volume it
    /// has not yet touched; with `ToGoal` and no waypoints that is the goal
    /// itself, which on a long course is too far away to steer by.
    waypoints: Vec<TriggerSet>,
    forbid: Vec<Forbid>,
    progress: Progress,
    alphabet: Alphabet,
    lookahead: u32,
    decide_every: u32,
    /// Total committed ticks the search may spend. The budget is a parameter so
    /// a ranking between two branches can be re-run at twice the budget and
    /// shown not to be an artefact of how hard each was searched.
    budget_ticks: u32,
}

struct SearchResult {
    end: SimState,
    stop: Stop,
    cmds: Vec<UserCmd>,
    touched: TriggerSet,
    crossings: Vec<(u32, u32, String)>,
    trail: Vec<(u32, Vec3, Scalar)>,
    /// Candidate futures evaluated. The coverage number a negative rests on.
    states_explored: u64,
    commands_simulated: u64,
}

fn horizontal(v: Vec3) -> Scalar {
    vec3(v.x, v.y, s(0.0)).length()
}

/// What one held control produced.
struct Held {
    state: SimState,
    touched: TriggerSet,
    simulated: u64,
    /// The hull was inside a forbidden box at the end of **some tick** of this
    /// hold, not merely at the end of it.
    ///
    /// Tested every tick deliberately. A forbidden region expresses "reach the
    /// goal without going through here", and a path that dips through a region
    /// and comes out the far side has gone through it. Sampling only the final
    /// state of a 40-tick horizon would admit exactly that.
    entered_forbidden: bool,
}

#[allow(clippy::too_many_arguments)]
fn hold(
    forbid: &[Aabb],
    half: Vec3,
    offset: Vec3,
    rate: TickRate,
    state: &SimState,
    c: Control,
    ticks: u32,
    course: &Course,
    profile: &PhysicsProfile,
    mut record: Option<&mut Vec<UserCmd>>,
) -> Held {
    let mut st = *state;
    let mut touched = TriggerSet::NONE;
    let mut simulated = 0u64;
    let mut entered_forbidden = false;
    for _ in 0..ticks {
        // Q3 requires releasing jump between hops, so the button is pressed
        // only when there is ground to leave.
        let grounded = matches!(st.player.ground, GroundState::Grounded { .. });
        let mut buttons = Buttons::NONE;
        if c.jump && grounded {
            buttons = buttons.with(Buttons::JUMP);
        }
        if c.crouch {
            buttons = buttons.with(Buttons::CROUCH);
        }
        let yaw = st.player.view.yaw_degrees() + c.yaw_rate;
        let cmd = UserCmd {
            duration_ms: rate.command_millis(),
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
        simulated += 1;
        if !entered_forbidden
            && forbid
                .iter()
                .any(|b| hull_in_box(b, st.player.origin, half, offset))
        {
            entered_forbidden = true;
        }
    }
    Held {
        state: st,
        touched,
        simulated,
        entered_forbidden,
    }
}

fn search(
    spec: &SearchSpec,
    rate: TickRate,
    start: SimState,
    course: &Course,
    profile: &PhysicsProfile,
) -> SearchResult {
    let half = profile.hull_half_extents();
    let offset = profile.hull_center_offset();
    let set = controls(spec.alphabet);
    let mut state = start;
    let mut touched = TriggerSet::NONE;
    let mut cmds: Vec<UserCmd> = Vec::new();
    let mut crossings: Vec<(u32, u32, String)> = Vec::new();
    let mut trail: Vec<(u32, Vec3, Scalar)> = Vec::new();
    let mut ticks = 0u32;
    let mut states_explored = 0u64;
    let mut commands_simulated = 0u64;
    let mut stop = Stop::BudgetExhausted;

    let forbidden_boxes: Vec<Aabb> = spec
        .forbid
        .iter()
        .filter_map(|f| match f {
            Forbid::Box(b) => Some(*b),
            Forbid::Bits(_) => None,
        })
        .collect();
    let forbidden_bits: TriggerSet = spec.forbid.iter().fold(TriggerSet::NONE, |a, f| match f {
        Forbid::Bits(b) => a.with(*b),
        Forbid::Box(_) => a,
    });

    while ticks < spec.budget_ticks {
        if touched.contains(spec.goal) {
            stop = Stop::GoalReached;
            break;
        }
        // The volume the searcher is currently descending toward: the first
        // waypoint not yet touched, else the goal itself.
        let aim: TriggerSet = spec
            .waypoints
            .iter()
            .copied()
            .find(|w| !touched.contains(*w))
            .unwrap_or(spec.goal);

        let mut best = None;
        let mut best_score = Scalar::NEG_INFINITY;
        for &c in &set {
            let h = hold(
                &forbidden_boxes,
                half,
                offset,
                rate,
                &state,
                c,
                spec.lookahead,
                course,
                profile,
                None,
            );
            let f = h.state;
            states_explored += 1;
            commands_simulated += h.simulated;
            if !f.player.origin.is_finite() || h.entered_forbidden {
                continue;
            }
            let speed = horizontal(f.player.velocity);
            let score = match spec.progress {
                Progress::NorthY => {
                    f.player.origin.y + s(0.25) * speed
                        - s(0.5) * (f.player.origin.z - state.player.origin.z).min(s(0.0))
                }
                Progress::ToGoal => {
                    let d = course
                        .volumes_for(aim)
                        .map(|v| v.hull_distance(f.player.origin, half, offset))
                        .fold(Scalar::INFINITY, Scalar::min);
                    // Speed breaks ties at a tenth the weight, which is what
                    // keeps the searcher from stalling one unit short of a face
                    // and what lets a fly-through outscore a landing.
                    -d + s(0.1) * speed
                }
            };
            if score > best_score {
                best_score = score;
                best = Some(c);
            }
        }
        let Some(c) = best else {
            stop = Stop::NoAdmissibleControl;
            break;
        };
        let committed = hold(
            &forbidden_boxes,
            half,
            offset,
            rate,
            &state,
            c,
            spec.decide_every,
            course,
            profile,
            Some(&mut cmds),
        );
        commands_simulated += committed.simulated;
        let crossed = committed.touched;
        state = committed.state;
        ticks += spec.decide_every;

        if crossed.intersects(forbidden_bits) || committed.entered_forbidden {
            touched = touched.with(crossed);
            stop = Stop::EnteredForbidden;
            break;
        }
        for v in &course.volumes {
            if v.set.is_empty() || !crossed.contains(v.set) || touched.contains(v.set) {
                continue;
            }
            crossings.push((
                state.time_ms,
                state.run.elapsed_ms(state.time_ms).unwrap_or(0),
                v.name.clone(),
            ));
        }
        touched = touched.with(crossed);
        trail.push((ticks, state.player.origin, horizontal(state.player.velocity)));
    }
    if touched.contains(spec.goal) {
        stop = Stop::GoalReached;
    }

    SearchResult {
        end: state,
        stop,
        cmds,
        touched,
        crossings,
        trail,
        states_explored,
        commands_simulated,
    }
}

// ---------------------------------------------------------------------------
// Ballistic gates, swept in two dimensions.

/// Launch from `origin` at `speed` along `yaw`, jump, and report where the
/// player comes to rest on ground again.
fn ballistic(
    course: &Course,
    origin: Vec3,
    speed: Scalar,
    yaw_deg: Scalar,
    profile: &PhysicsProfile,
    rate: TickRate,
) -> (Vec3, Scalar, bool) {
    let rad = yaw_deg.to_radians();
    let vel = vec3(speed * rad.cos(), speed * rad.sin(), s(0.0));
    let mut st = SimState::spawned_at(origin, yaw_deg);
    st.player.velocity = vel;
    let still = UserCmd {
        duration_ms: rate.command_millis(),
        forward_move: 0,
        right_move: 0,
        up_move: 0,
        buttons: Buttons::NONE,
        view: ViewAngles::from_degrees(s(0.0), yaw_deg, s(0.0)),
    };
    // Settle one tick so the mover reports the ground it is standing on, then
    // jump. Jumping from a state that has not seen the floor yet measures a
    // fall, not a jump.
    step_in_place(&mut st, &still, &course.world, profile);
    st.player.velocity = vel;
    let jump = UserCmd {
        buttons: Buttons::JUMP,
        ..still
    };
    step_in_place(&mut st, &jump, &course.world, profile);

    let mut airborne = false;
    let mut fell = false;
    for _ in 0..600 {
        step_in_place(&mut st, &still, &course.world, profile);
        match st.player.ground {
            GroundState::Airborne => airborne = true,
            _ if airborne => break,
            _ => {}
        }
        if st.player.origin.z < origin.z - s(1024.0) {
            fell = true;
            break;
        }
    }
    (st.player.origin, horizontal(st.player.velocity), fell)
}

// ---------------------------------------------------------------------------

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
        end: vec3(x, y, from_z - s(8192.0)),
        half_extents: half,
        center_offset: offset,
    });
    if t.fraction >= s(1.0) || t.start_solid {
        return None;
    }
    let origin_z = from_z - s(8192.0) * t.fraction;
    Some((origin_z + offset.z - half.z, t.normal))
}

fn classify(normal: Vec3, profile: &PhysicsProfile) -> &'static str {
    if normal.z >= profile.min_walk_normal {
        "Grounded"
    } else {
        "Sliding "
    }
}

/// `straf3-headless`'s input format, byte-compatible with `Recorder::to_fixture`.
fn fixture_text(
    cmds: &[UserCmd],
    rate: TickRate,
    spawn: Vec3,
    spawn_yaw: Scalar,
    map_path: &str,
    note: &str,
) -> String {
    let mut out = String::with_capacity(48 * cmds.len() + 512);
    let _ = write!(out, "# Generated by probes/course-lab.\n# {note}\n");
    let _ = write!(
        out,
        "# Replay it with `straf3 --replay <this file> --map {map_path}`; the\n\
         # final checksum must equal the one course-lab printed beside it.\n\n"
    );
    let _ = writeln!(out, "rate {}", rate.hz());
    out.push_str("profile cpm\n");
    out.push_str("world map\n");
    let _ = writeln!(out, "spawn {:?} {:?} {:?}", spawn.x, spawn.y, spawn.z);
    let _ = writeln!(out, "yaw {spawn_yaw:?}\n");
    out.push_str("# cmd <repeat> <fwd> <right> <up> <buttons> <pitch> <yaw> <roll>\n");

    let mut i = 0;
    while i < cmds.len() {
        let cmd = cmds[i];
        let mut repeat = 1;
        while i + repeat < cmds.len() && cmds[i + repeat] == cmd {
            repeat += 1;
        }
        let mut names: Vec<&str> = Vec::new();
        if cmd.buttons.contains(Buttons::JUMP) {
            names.push("jump");
        }
        if cmd.buttons.contains(Buttons::CROUCH) {
            names.push("crouch");
        }
        let buttons = if names.is_empty() {
            "-".to_string()
        } else {
            names.join("+")
        };
        let _ = writeln!(
            out,
            "cmd {} {} {} {} {} {:?} {:?} {:?}",
            repeat,
            cmd.forward_move,
            cmd.right_move,
            cmd.up_move,
            buttons,
            cmd.view.pitch_degrees(),
            cmd.view.yaw_degrees(),
            cmd.view.roll_degrees(),
        );
        i += repeat;
    }
    out
}

// ---------------------------------------------------------------------------
// Argument handling. Deliberately explicit: every knob a claim depends on has
// to be nameable on the command line, so someone who is not the author can put
// this tool to their own hypothesis.

struct Args {
    map: String,
    phase: String,
    goal: String,
    waypoints: Vec<String>,
    forbid: Vec<String>,
    progress: Progress,
    crouch: bool,
    lookahead: u32,
    decide_every: u32,
    budget_ticks: u32,
    tag: String,
    write_fixture: bool,
    gates: Vec<String>,
    line: Vec<(Scalar, Scalar)>,
    out_dir: String,
}

const USAGE: &str = "\
course-lab <map.map> <phase> [options]

phases
  compile     compile through straf3-map; print warnings, the trigger table
              with the bits each volume resolves to, digests and mesh size
  spawn       is the spawn in open air, and what is under it
  survey      running-surface survey along a polyline (--line), not along x=0
  gates       ballistic gate sweeps, speed x approach yaw (--gate, repeatable)
  search      goal-directed search; writes a replay fixture with --fixture
  baseline    the trivial heuristic: volumes sorted by y, steer at the next
              one, progress = origin.y. Run it twice, --crouch and without.

search options
  --goal <sel>        volume-crossing predicate: bits that must all be touched
  --waypoint <sel>    ordered intermediate volume, repeatable
  --forbid <sel>      bits that must not be touched, repeatable
  --forbid-box x0,x1,y0,y1,z0,z1     region the hull must not enter
  --progress y|goal   scoring metric (default goal)
  --crouch            add CROUCH to the control alphabet
  --lookahead <n>     horizon in ticks (default 40)
  --stride <n>        committed ticks per decision (default 8)
  --budget <n>        total committed ticks (default 20000)
  --fixture           write <out>/<stem>-<tag>.txt
  --tag <name>        fixture tag (default the goal selector)
  --out <dir>         output directory (default results)

  <sel> is start | finish | cp:N | mask:0xNN | a target name from the map

gates options
  --gate name:x,y,z:spd0,spd1,step[:yaw0,yaw1,step]
              default yaw sweep is 90,90,1 (due +Y, one column)

survey options
  --line x,y;x,y;...  polyline sampled every 32 units (default the map's
                      x = centre line)
";

fn parse_args() -> Args {
    let argv: Vec<String> = std::env::args().skip(1).collect();
    if argv.is_empty() || argv[0] == "--help" || argv[0] == "-h" {
        println!("{USAGE}");
        std::process::exit(0);
    }
    let mut a = Args {
        map: argv[0].clone(),
        phase: argv.get(1).cloned().unwrap_or_else(|| "compile".into()),
        goal: "finish".into(),
        waypoints: Vec::new(),
        forbid: Vec::new(),
        progress: Progress::ToGoal,
        crouch: false,
        lookahead: 40,
        decide_every: 8,
        budget_ticks: 20_000,
        tag: String::new(),
        write_fixture: false,
        gates: Vec::new(),
        line: Vec::new(),
        out_dir: "results".into(),
    };
    let mut i = 2;
    let next = |i: &mut usize| -> String {
        *i += 1;
        argv.get(*i)
            .cloned()
            .unwrap_or_else(|| panic!("missing value for {}", argv[*i - 1]))
    };
    while i < argv.len() {
        match argv[i].as_str() {
            "--goal" => a.goal = next(&mut i),
            "--waypoint" => a.waypoints.push(next(&mut i)),
            "--forbid" => a.forbid.push(next(&mut i)),
            "--forbid-box" => a.forbid.push(format!("box:{}", next(&mut i))),
            "--progress" => {
                a.progress = match next(&mut i).as_str() {
                    "y" => Progress::NorthY,
                    _ => Progress::ToGoal,
                }
            }
            "--crouch" => a.crouch = true,
            "--lookahead" => a.lookahead = next(&mut i).parse().expect("--lookahead"),
            "--stride" => a.decide_every = next(&mut i).parse().expect("--stride"),
            "--budget" => a.budget_ticks = next(&mut i).parse().expect("--budget"),
            "--fixture" => a.write_fixture = true,
            "--tag" => a.tag = next(&mut i),
            "--out" => a.out_dir = next(&mut i),
            "--gate" => a.gates.push(next(&mut i)),
            "--line" => {
                for pair in next(&mut i).split(';') {
                    let mut it = pair.split(',').map(|t| t.trim().parse::<Scalar>());
                    if let (Some(Ok(x)), Some(Ok(y))) = (it.next(), it.next()) {
                        a.line.push((x, y));
                    }
                }
            }
            other => panic!("unknown option {other}\n\n{USAGE}"),
        }
        i += 1;
    }
    if a.tag.is_empty() {
        a.tag = a.goal.replace([':', '/', '\\', 'x'], "");
    }
    a
}

fn parse_forbid(course: &Course, specs: &[String]) -> Vec<Forbid> {
    specs
        .iter()
        .map(|spec| {
            if let Some(rest) = spec.strip_prefix("box:") {
                let n: Vec<Scalar> = rest
                    .split(',')
                    .map(|t| t.trim().parse().expect("--forbid-box wants six numbers"))
                    .collect();
                assert_eq!(n.len(), 6, "--forbid-box wants x0,x1,y0,y1,z0,z1");
                Forbid::Box(Aabb {
                    mins: vec3(n[0].min(n[1]), n[2].min(n[3]), n[4].min(n[5])),
                    maxs: vec3(n[0].max(n[1]), n[2].max(n[3]), n[4].max(n[5])),
                })
            } else {
                Forbid::Bits(
                    course
                        .select(spec)
                        .unwrap_or_else(|| panic!("no volume matches --forbid {spec}")),
                )
            }
        })
        .collect()
}

// ---------------------------------------------------------------------------

fn print_compile(course: &Course, path: &str) {
    let map = &course.map;
    println!("== compile ==");
    println!("map            {path}");
    println!("stem           {}", course.stem);
    println!(
        "solids         {} hulls   mesh {} vertices / {} triangles",
        map.hulls.len(),
        map.mesh.vertices.len(),
        map.mesh.indices.len() / 3
    );
    println!(
        "digests        collision {:#018x}  full {:#018x}",
        map.collision_digest(),
        map.full_digest()
    );
    println!(
        "bounds         {:?} .. {:?}",
        map.bounds.mins, map.bounds.maxs
    );
    println!(
        "spawn          {:?} yaw {}  ({} spawn entities)",
        map.spawn,
        map.spawn_yaw,
        map.spawns.len()
    );
    if map.warnings.is_empty() {
        println!("warnings       none");
    } else {
        for w in &map.warnings {
            println!("warning        {w:?}");
        }
    }
    println!("\ntriggers       {}", course.volumes.len());
    for v in &course.volumes {
        let meaning = match v.kind {
            TriggerKind::Start => "START".to_string(),
            TriggerKind::Finish => "FINISH".to_string(),
            TriggerKind::Checkpoint(n) => format!("checkpoint {n} bit {:#010x}", v.set.0),
            other => format!("{other:?} (no clock bit)"),
        };
        println!(
            "   {:<14} -> {:<20} {:<28} {:?} .. {:?}",
            v.name, v.resolved, meaning, v.bounds.mins, v.bounds.maxs
        );
    }
    let coverage = course.world.trigger_coverage();
    println!(
        "\ncoverage       {:#010x}   has_timing()={}  START={}  FINISH={}",
        coverage.0,
        map.has_timing(),
        coverage.contains(TriggerSet::START),
        coverage.contains(TriggerSet::FINISH)
    );
    let unresolved: Vec<&str> = course
        .volumes
        .iter()
        .filter(|v| v.set.is_empty())
        .map(|v| v.name.as_str())
        .collect();
    if unresolved.is_empty() {
        println!("every trigger volume resolves to a clock bit.");
    } else {
        println!(
            "UNRESOLVED, and this is not a compile error — these volumes keep \
             their geometry and do nothing: {unresolved:?}"
        );
    }
    // Volume depth along +Y, because check's independent verifier point-samples
    // at tick ends: a volume thinner than a fast line's per-tick step could be
    // missed by it even though the swept clock sees it.
    println!("\nvolume depth along +Y (a fast line steps ~8 units per tick at 1000 ups)");
    for v in &course.volumes {
        let d = v.bounds.maxs.y - v.bounds.mins.y;
        println!("   {:<14} {:>6.0} units", v.name, d);
    }
}

fn main() {
    let args = parse_args();
    let profile = PhysicsProfile::straf3();
    let rate = TickRate::HZ_125;
    let course = load(&args.map);
    let half = profile.hull_half_extents();
    let offset = profile.hull_center_offset();

    // Canon and the CPM reconstruction are numerically equal today
    // (profile.rs::straf3_and_cpm_agree_today_but_are_not_linked), which is why
    // a fixture this tool writes says `profile cpm` and still describes a canon
    // run. Stated rather than left for a reader to infer.
    println!(
        "profile        straf3 (canon), {} Hz; equal to cpm today",
        rate.hz()
    );

    match args.phase.as_str() {
        "compile" => print_compile(&course, &args.map),

        "spawn" => {
            let t = course.world.trace(&Sweep {
                start: course.spawn,
                end: course.spawn,
                half_extents: half,
                center_offset: offset,
            });
            println!("== spawn ==");
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
            println!("ground under spawn: {under:?}");
        }

        "survey" => {
            let line = if args.line.is_empty() {
                let b = &course.map.bounds;
                vec![
                    (s(0.0), b.mins.y + s(32.0)),
                    (s(0.0), b.maxs.y - s(32.0)),
                ]
            } else {
                args.line.clone()
            };
            println!("== surface survey along the given polyline, every 32 units ==");
            println!("{:>9} {:>9} {:>9} {:>9} {:>10}", "s", "x", "y", "surface", "normal.z");
            let mut families: BTreeMap<&str, (Scalar, Scalar, u32)> = BTreeMap::new();
            let mut travelled = s(0.0);
            let mut voids = 0u32;
            for w in line.windows(2) {
                let (x0, y0) = w[0];
                let (x1, y1) = w[1];
                let seg = vec3(x1 - x0, y1 - y0, s(0.0));
                let len = seg.length();
                let dir = seg / len;
                let mut t = s(0.0);
                while t <= len {
                    let x = x0 + dir.x * t;
                    let y = y0 + dir.y * t;
                    match ground_under(&course, x, y, s(4096.0), &profile) {
                        Some((z, nrm)) => {
                            let st = classify(nrm, &profile);
                            println!(
                                "{:>9.0} {x:>9.0} {y:>9.0} {z:>9.1} {:>9.4}  {st}",
                                travelled + t,
                                nrm.z
                            );
                            let e = families.entry(st).or_insert((nrm.z, nrm.z, 0));
                            e.0 = e.0.min(nrm.z);
                            e.1 = e.1.max(nrm.z);
                            e.2 += 1;
                        }
                        None => {
                            println!(
                                "{:>9.0} {x:>9.0} {y:>9.0} {:>9} {:>9}  VOID",
                                travelled + t,
                                "-",
                                "-"
                            );
                            voids += 1;
                        }
                    }
                    t += s(32.0);
                }
                travelled += len;
            }
            println!("\nvoid samples: {voids}");
            for (k, (lo, hi, n)) in &families {
                println!("  {k}: {n} samples, normal.z {lo:.4}..{hi:.4}");
            }
        }

        "gates" => {
            println!("== ballistic gates, swept over speed x approach yaw ==");
            println!(
                "Every gate reports the WORST approach as well as the best, because a \
                 single\ncolumn from one yaw measures one approach rather than the \
                 admissible window."
            );
            for spec in &args.gates {
                let parts: Vec<&str> = spec.split(':').collect();
                assert!(
                    parts.len() >= 3,
                    "--gate name:x,y,z:spd0,spd1,step[:yaw0,yaw1,step]"
                );
                let name = parts[0];
                let o: Vec<Scalar> = parts[1]
                    .split(',')
                    .map(|t| t.trim().parse().expect("gate origin"))
                    .collect();
                let sp: Vec<Scalar> = parts[2]
                    .split(',')
                    .map(|t| t.trim().parse().expect("gate speeds"))
                    .collect();
                let yw: Vec<Scalar> = if parts.len() > 3 {
                    parts[3]
                        .split(',')
                        .map(|t| t.trim().parse().expect("gate yaws"))
                        .collect()
                } else {
                    vec![s(90.0), s(90.0), s(1.0)]
                };
                let origin = vec3(o[0], o[1], o[2]);
                println!("\n-- {name} from {origin:?}, yaw {}..{} step {}", yw[0], yw[1], yw[2]);
                println!(
                    "{:>8} {:>10} {:>10} {:>10} {:>10} {:>9}",
                    "ups", "best y", "best z", "worst y", "worst z", "exit ups"
                );
                let mut speed = sp[0];
                while speed <= sp[1] + s(0.001) {
                    let mut best = (Scalar::NEG_INFINITY, s(0.0), s(0.0));
                    let mut worst = (Scalar::INFINITY, s(0.0), s(0.0));
                    let mut yaw = yw[0];
                    while yaw <= yw[1] + s(0.001) {
                        let (end, exit, _fell) =
                            ballistic(&course, origin, speed, yaw, &profile, rate);
                        if end.y > best.0 {
                            best = (end.y, end.z, exit);
                        }
                        if end.y < worst.0 {
                            worst = (end.y, end.z, exit);
                        }
                        yaw += yw[2];
                        if yw[2] <= s(0.0) {
                            break;
                        }
                    }
                    println!(
                        "{speed:>8.0} {:>10.0} {:>10.0} {:>10.0} {:>10.0} {:>9.0}",
                        best.0, best.1, worst.0, worst.1, best.2
                    );
                    speed += sp[2];
                    if sp[2] <= s(0.0) {
                        break;
                    }
                }
            }
        }

        "search" | "baseline" => {
            let baseline = args.phase == "baseline";
            let goal = course
                .select(&args.goal)
                .unwrap_or_else(|| panic!("no volume matches --goal {}", args.goal));
            let waypoints: Vec<TriggerSet> = if baseline {
                // The trivial heuristic, reproduced exactly: sort the timing
                // volumes by the y of their centroid and steer at the next one.
                let mut vs: Vec<&Volume> = course
                    .volumes
                    .iter()
                    .filter(|v| !v.set.is_empty())
                    .collect();
                vs.sort_by(|a, b| {
                    a.centre()
                        .y
                        .partial_cmp(&b.centre().y)
                        .unwrap_or(std::cmp::Ordering::Equal)
                });
                println!(
                    "baseline order (volumes sorted by centroid y): {:?}",
                    vs.iter().map(|v| v.name.as_str()).collect::<Vec<_>>()
                );
                vs.iter().map(|v| v.set).collect()
            } else {
                args.waypoints
                    .iter()
                    .map(|w| {
                        course
                            .select(w)
                            .unwrap_or_else(|| panic!("no volume matches --waypoint {w}"))
                    })
                    .collect()
            };
            let spec = SearchSpec {
                goal,
                waypoints,
                forbid: parse_forbid(&course, &args.forbid),
                progress: if baseline {
                    Progress::NorthY
                } else {
                    args.progress
                },
                alphabet: Alphabet {
                    crouch: args.crouch,
                },
                lookahead: args.lookahead,
                decide_every: args.decide_every,
                budget_ticks: args.budget_ticks,
            };
            let control_count = controls(spec.alphabet).len();
            println!(
                "== {} ==\ngoal {} ({:#010x})  progress {}  alphabet {}  \
                 horizon {} stride {} budget {}  controls {}",
                args.phase,
                args.goal,
                goal.0,
                if spec.progress == Progress::NorthY {
                    "origin.y"
                } else {
                    "hull distance to next goal volume"
                },
                spec.alphabet.label(),
                spec.lookahead,
                spec.decide_every,
                spec.budget_ticks,
                control_count
            );

            let start = SimState::spawned_at(course.spawn, course.spawn_yaw);
            let r = search(&spec, rate, start, &course, &profile);

            println!("\n{:>12} {:>10}  volume", "window end", "run ms");
            for (at, elapsed, name) in &r.crossings {
                println!("{at:>12} {elapsed:>10}  {name}");
            }
            println!("\ntouched mask   {:#010x}", r.touched.0);
            match r.end.run {
                RunState::NotStarted => println!("clock          never started"),
                RunState::Running { started_at_ms } => println!(
                    "clock          RUNNING from {started_at_ms} ms, {} ms at cut-off",
                    r.end.run.elapsed_ms(r.end.time_ms).unwrap_or(0)
                ),
                RunState::Finished {
                    started_at_ms,
                    finished_at_ms,
                } => {
                    let ms = finished_at_ms - started_at_ms;
                    println!(
                        "clock          FINISHED  start {started_at_ms} ms  finish \
                         {finished_at_ms} ms  time {}.{:03} s ({ms} ms)",
                        ms / 1000,
                        ms % 1000
                    );
                }
            }
            let peak = r
                .trail
                .iter()
                .map(|(_, _, sp)| *sp)
                .fold(s(0.0), Scalar::max);
            println!(
                "end            {:?}  {:.0} ups  peak {peak:.0} ups  after {} ms",
                r.end.player.origin,
                horizontal(r.end.player.velocity),
                r.end.time_ms
            );
            // Coverage. Printed for every outcome, and load-bearing for a
            // negative: "found nothing" is only a result alongside how hard it
            // was looked for.
            println!(
                "\ncoverage       stop={:?}  states explored {}  commands simulated {}\n\
                 \x20              horizon {} ticks  stride {}  controls {}  alphabet {}  \
                 budget {} ticks",
                r.stop,
                r.states_explored,
                r.commands_simulated,
                spec.lookahead,
                spec.decide_every,
                control_count,
                spec.alphabet.label(),
                spec.budget_ticks
            );
            if r.stop != Stop::GoalReached {
                println!(
                    "NEGATIVE: no line satisfying the goal was found at this budget. That \
                     is a\nstatement about this search, not a proof that none exists."
                );
            }
            println!("final checksum {:#018x}", r.end.checksum());

            if args.write_fixture {
                let dir = &args.out_dir;
                let _ = std::fs::create_dir_all(dir);
                // Derived from the map's own stem, never a constant: running
                // this on map A must not be able to overwrite map B's evidence.
                let path = format!("{dir}/{}-{}.txt", course.stem, args.tag);
                let note = format!(
                    "{} search of {} for goal {} ({:#010x}); reported {} ms, checksum {:#018x}",
                    args.phase,
                    args.map,
                    args.goal,
                    goal.0,
                    match r.end.run {
                        RunState::Finished {
                            started_at_ms,
                            finished_at_ms,
                        } => finished_at_ms - started_at_ms,
                        _ => 0,
                    },
                    r.end.checksum()
                );
                let text = fixture_text(
                    &r.cmds,
                    rate,
                    course.spawn,
                    course.spawn_yaw,
                    &args.map,
                    &note,
                );
                match std::fs::write(&path, &text) {
                    Ok(()) => println!(
                        "wrote {path} ({} bytes, {} cmd lines)",
                        text.len(),
                        text.lines().filter(|l| l.starts_with("cmd ")).count()
                    ),
                    Err(e) => println!("could not write {path}: {e}"),
                }
            }
        }

        other => panic!("unknown phase {other}\n\n{USAGE}"),
    }
}
