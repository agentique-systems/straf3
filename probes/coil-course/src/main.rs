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
//!
//! Any of these can come back false. H3 is the one most likely to: the arithmetic
//! that produced those numbers is a point-mass ballistic model, and the real
//! mover clips velocity into every plane it touches.
//!
//! # Why this probe does not use `straf3-map`
//!
//! It was written while C7 was still in flight, so its brush-to-hull path is an
//! independent second derivation of the same compile step. That is the point:
//! when `straf3-map` lands, the two can be compared, and a probe that shared
//! the compiler could not disagree with it.
//!
//! # Running it
//!
//! ```sh
//! cd probes/coil-course
//! cargo run --release -- ../../assets/maps/coil.map | tee results/coil.txt
//! ```

use std::collections::BTreeMap;
use std::fs::File;

use straf3_collision::{Hull, HullWorld, Plane};
use straf3_sim::num::{Scalar, Vec3, s, vec3};
use straf3_sim::world::{SurfaceFlags, Sweep, World};
use straf3_sim::{
    Buttons, GroundState, PhysicsProfile, SimState, TickRate, UserCmd, ViewAngles, step_in_place,
};

/// A trigger volume, kept as the axis-aligned box it is authored as.
struct Trigger {
    target: String,
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

    for ent in &map.entities {
        let class = key(ent, "classname").unwrap_or("");
        match class {
            "info_player_start" | "info_player_deathmatch" => {
                spawn = key(ent, "origin").and_then(parse_origin).expect("spawn origin");
                spawn_yaw = key(ent, "angle").and_then(|a| a.parse().ok()).unwrap_or(s(0.0));
            }
            // Trigger brushes are volumes, not solids: they must never reach
            // the tracer as geometry or the player would bump into the finish
            // line. Their bounds come from the same planes all the same.
            "trigger_multiple" | "trigger_once" | "trigger_push" | "trigger_teleport" => {
                for brush in &ent.brushes {
                    let (mins, maxs) = brush_bounds(brush);
                    triggers.push(Trigger {
                        target: key(ent, "target").unwrap_or("<untargeted>").to_string(),
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
                    textures.push(
                        brush[0].texture.to_str().unwrap_or("?").to_string(),
                    );
                    hulls.push(hull);
                }
            }
        }
    }

    Course {
        world: HullWorld::new(hulls),
        triggers,
        spawn,
        spawn_yaw,
        textures,
    }
}

/// Bounds of a brush, from the axis-aligned faces it is authored with.
fn brush_bounds(brush: &quake_map::Brush) -> (Vec3, Vec3) {
    let planes: Vec<Plane> = brush
        .iter()
        .map(|f| Plane::from_points(v(f.half_space[0]), v(f.half_space[1]), v(f.half_space[2])).unwrap())
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
fn ground_under(course: &Course, x: Scalar, y: Scalar, from_z: Scalar, profile: &PhysicsProfile) -> Option<(Scalar, Vec3)> {
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
                        out.push(Control { yaw_rate, right, forward, jump });
                    }
                }
            }
        }
        out
    }

    /// Apply one control for `ticks`, returning the resulting state.
    fn hold(
        &self,
        state: &SimState,
        c: Control,
        ticks: u32,
        course: &Course,
        profile: &PhysicsProfile,
    ) -> SimState {
        let mut st = *state;
        for _ in 0..ticks {
            // Bunny hopping: Q3 requires releasing jump between hops, so the
            // button is pressed only when there is ground to leave.
            let grounded = matches!(st.player.ground, GroundState::Grounded { .. });
            let buttons = if c.jump && grounded { Buttons::JUMP } else { Buttons::NONE };
            let yaw = st.player.view.yaw + c.yaw_rate;
            let cmd = UserCmd {
                duration_ms: self.rate.command_millis(),
                forward_move: c.forward,
                right_move: c.right,
                up_move: 0,
                buttons,
                view: ViewAngles { pitch: s(0.0), yaw, roll: s(0.0) },
            };
            step_in_place(&mut st, &cmd, &course.world, profile);
        }
        st
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
    ) -> (SimState, Vec<(u32, Vec3, Scalar)>) {
        let controls = Self::controls();
        let mut trail = Vec::new();
        let mut ticks = 0;
        while ticks < max_ticks && state.player.origin.y < until_y {
            let mut best = None;
            let mut best_score = Scalar::NEG_INFINITY;
            for &c in &controls {
                let f = self.hold(&state, c, self.lookahead, course, profile);
                if !f.player.origin.is_finite() {
                    continue;
                }
                let speed = vec3(f.player.velocity.x, f.player.velocity.y, s(0.0)).length();
                // Progress along the course dominates; speed breaks ties, which
                // is what stops the bot trading its whole run for one long slide.
                let score = f.player.origin.y + s(0.25) * speed - s(0.5) * (f.player.origin.z - state.player.origin.z).min(s(0.0));
                if score > best_score {
                    best_score = score;
                    best = Some(c);
                }
            }
            let Some(c) = best else { break };
            state = self.hold(&state, c, self.decide_every, course, profile);
            ticks += self.decide_every;
            let speed = vec3(state.player.velocity.x, state.player.velocity.y, s(0.0)).length();
            trail.push((ticks, state.player.origin, speed));
        }
        (state, trail)
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
        view: ViewAngles { pitch: s(0.0), yaw: s(90.0), roll: s(0.0) },
    };
    step_in_place(&mut st, &still, &course.world, profile);
    st.player.velocity = vec3(s(0.0), speed, s(0.0));
    let jump = UserCmd { buttons: Buttons::JUMP, ..still };
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
    let path = std::env::args().nth(1).unwrap_or_else(|| "../../assets/maps/coil.map".into());
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
        println!("   {:<12} {:?} .. {:?}", t.target, t.mins, t.maxs);
    }
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
    let under = ground_under(&course, course.spawn.x, course.spawn.y, course.spawn.z, &profile);
    println!("ground under spawn: {under:?}  (want surface z=0, normal z=1)");

    // ---- H2: the running surface -----------------------------------------
    println!("\n== H2  surface survey along x=0, every 32 units of y ==");
    println!("{:>7} {:>9} {:>9} {:>10}", "y", "surface", "normal.z", "state");
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
        ("ramp-wave shortcut", vec3(s(0.0), s(1780.0), s(112.0 + 24.0)), s(350.0), s(25.0)),
        ("the gully           ", vec3(s(0.0), s(2292.0), s(128.0 + 24.0)), s(450.0), s(25.0)),
        ("the finish          ", vec3(s(0.0), s(3124.0), s(64.0 + 24.0)), s(350.0), s(25.0)),
    ];
    for (name, origin, first, stepping) in gates {
        println!("\n-- {name} from {:?}", origin);
        println!("{:>8} {:>10} {:>9} {:>10}", "ups", "landed y", "landed z", "exit ups");
        let mut sp = first;
        while sp <= s(1000.0) {
            let (end, exit) = ballistic(&course, origin, sp, &profile, rate);
            println!("{sp:>8.0} {:>10.0} {:>9.0} {exit:>10.0}", end.y, end.z);
            sp += stepping;
        }
    }

    // ---- H3: is that speed reachable? ------------------------------------
    println!("\n== H3  strafe-jumping from the spawn ==");
    let bot = Bot { rate, lookahead: 40, decide_every: 8 };
    let start = SimState::spawned_at(course.spawn, course.spawn_yaw);
    let (end, trail) = bot.run(start, s(1780.0), 5_000, &course, &profile);
    println!("{:>7} {:>9} {:>9} {:>9} {:>9}", "tick", "x", "y", "z", "ups");
    for (t, p, sp) in trail.iter().step_by(4) {
        println!("{t:>7} {:>9.0} {:>9.0} {:>9.0} {sp:>9.0}", p.x, p.y, p.z);
    }
    let peak = trail.iter().map(|(_, _, s)| *s).fold(s(0.0), Scalar::max);
    println!(
        "\nreached y={:.0} z={:.0} in {} ms; peak horizontal speed {peak:.0} ups",
        end.player.origin.y,
        end.player.origin.z,
        end.time_ms
    );

    // Which triggers did the bot cross on the way?
    println!("\n== triggers the bot's path overlapped ==");
    let mut hit: Vec<&str> = Vec::new();
    for (_, p, _) in &trail {
        for tr in &course.triggers {
            if tr.overlaps(*p, half, offset) && !hit.contains(&tr.target.as_str()) {
                hit.push(&tr.target);
            }
        }
    }
    println!("{hit:?}   (sampled once per decision window, so this under-reports)");

    println!("\n== texture families in the compiled solids ==");
    let mut counts: BTreeMap<&str, u32> = BTreeMap::new();
    for t in &course.textures {
        *counts.entry(t.as_str()).or_default() += 1;
    }
    for (k, n) in counts {
        println!("  {k:<18} {n}");
    }
}
