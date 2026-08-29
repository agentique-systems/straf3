//! The search: how the agent decides what to press, and why it is not greedy.
//!
//! # The question this module answers
//!
//! [`crate::course`] says *where to go*. This module says *how*, and it is the
//! half r9 is about: the search must be able to prefer an action that scores
//! worse at its own horizon because it is better at the goal, and the mechanism
//! by which it does so has to be nameable rather than emergent.
//!
//! # The prior art, and the measurement that ruled out the obvious fix
//!
//! Two searchers in this tree already answer "what do I press next", and both
//! are the same shape:
//!
//! - `probes/coil-course` scores each control by `origin.y` after a fixed
//!   rollout and takes the best.
//! - `probes/course-lab` fixes the metric — hull distance to the next goal
//!   *volume*, which is the predicate the run clock actually reads — and still
//!   scores each control by holding it for a rollout and taking the best.
//!
//! Both are **greedy one-step-per-window**: at each decision they evaluate every
//! control once, commit to the argmax, and never reconsider. `course-lab`'s
//! author measured what that costs on `cleave.map` and published it: from the
//! high branch, walking off the lip *into* the 448-unit gap reduces the distance
//! to the landing pad, so the searcher walks in, and the pad's south face is
//! then a 160-unit wall with the run-up thrown away. In one run it circled in
//! that hole for 43 seconds of simulated time. Every local minimum on that map
//! has the same shape, and naming each one by hand — "reach the goal without
//! entering this box" — is the per-map tuning r2 forbids.
//!
//! The tempting fix is a longer rollout. **It is measurably not the mechanism.**
//! `course-lab` already runs a 40-tick horizon per control and walks in anyway,
//! because a deeper rollout of a *single held control* is a longer greedy step,
//! not lookahead over alternatives. Depth alone cannot fix a search that discards
//! every alternative the moment it commits.
//!
//! # The mechanism, named
//!
//! **Best-first search over held-control edges, with a frontier that is kept
//! rather than discarded.** In b7's taxonomy this is architecture (b) — a
//! goal-value term — combined with the third form its M2 lists, a frontier
//! retaining horizon-dominated states:
//!
//! - A **node** is a whole [`SimState`] plus the trigger bits touched on the way
//!   to it. An **edge** is one control held for `stride` ticks, exactly the unit
//!   both probes commit in.
//! - Nodes are scored by [`Node::f`] — an estimate of cost *to the goal*, not of
//!   progress *at the horizon* — and every node generated goes into an open list.
//!   Nothing is committed: the command stream is reconstructed from the winning
//!   leaf once the goal is reached, so a line that looked bad for two hundred
//!   expansions can still be the one that ships.
//! - The open list is capped at [`SearchSpec::frontier`] nodes. **That cap is the
//!   single knob that degrades this search to the prior art.** At `frontier = 1`
//!   the loop pops the only node, expands it, keeps the single best successor and
//!   discards the rest — which is greedy one-step-per-window, structurally
//!   identical to both probes. Above 1, alternatives survive.
//!
//! Two things make the retention actually pay, and both are stated here because
//! a mechanism that only works with an undocumented helper is not documented:
//!
//! 1. **A cost so far.** `f` is `h + patience * (distance the player could have
//!    covered in the time spent)`, so a node deep inside a basin accumulates cost
//!    while the shallower alternative it was preferred over does not. This is
//!    what turns "the frontier remembers" into "the frontier eventually wins":
//!    without it, escaping a basin means exhausting every cell in it.
//! 2. **A visited set.** Nodes are keyed by a coarse [`Cell`] — position,
//!    velocity direction and speed, stance, and the bits touched — and a cell is
//!    expanded once. Without it the search re-derives the same circling the
//!    `course-lab` negative describes. Note this makes the `frontier = 1` control
//!    *stronger* than the prior art rather than weaker: it is greedy plus a
//!    loop-breaker, so beating it is not beating a strawman.
//!
//! # What is a search constant and what would be a map constant
//!
//! Everything this module quantises by comes from [`PhysicsProfile`]: cell size
//! is measured in player hull widths, speed buckets in fractions of
//! `profile.max_speed`, the time cost in `max_speed` times seconds elapsed. The
//! search constants — frontier width, stride, patience, the yaw-rate ladder —
//! are properties of the search and are the same numbers for every map, which is
//! what r27 asks for; a map-specific one would be a coordinate, a bearing or a
//! threshold read off a particular course, and there is none here.

use std::cmp::Ordering;
use std::collections::{BinaryHeap, HashSet};

use straf3_map::Aabb;
use straf3_sim::num::{Scalar, Vec3, s, vec3};
use straf3_sim::state::RunState;
use straf3_sim::world::{TriggerSet, World};
use straf3_sim::{
    Buttons, GroundState, PhysicsProfile, SimState, TickRate, UserCmd, ViewAngles, step_in_place,
};

use crate::course::{CoursePlan, Step};

// ---------------------------------------------------------------------------
// The goals, taken from the plan.

/// One step of the course as the search sees it: the bits that satisfy it and
/// the volumes that carry them.
///
/// Built from [`CoursePlan`] alone, so the search inherits the plan's guarantee
/// that nothing here was typed in per map.
#[derive(Debug, Clone)]
pub struct Goal {
    /// Which step of the run this is.
    pub step: Step,
    /// The clock bits crossing it sets.
    pub bits: TriggerSet,
    /// Every volume that satisfies it, as bounds.
    pub boxes: Vec<Aabb>,
}

/// The clock bits a step sets, from the step alone.
#[must_use]
pub fn bits_of(step: Step) -> TriggerSet {
    match step {
        Step::Start => TriggerSet::START,
        Step::Checkpoint(index) => TriggerSet::checkpoint(index).unwrap_or(TriggerSet::NONE),
        Step::Finish => TriggerSet::FINISH,
    }
}

/// Turn a derived plan into the ordered goals the search descends toward.
#[must_use]
pub fn goals_of(plan: &CoursePlan) -> Vec<Goal> {
    plan.waypoints
        .iter()
        .map(|w| Goal {
            step: w.step,
            bits: bits_of(w.step),
            boxes: w.targets.iter().map(|t| t.bounds).collect(),
        })
        .collect()
}

/// Distance from a player hull at `origin` to `b`, zero once the hull overlaps.
///
/// The objective is a *box* distance rather than a distance to a point, and that
/// is `course-lab`'s finding rather than this crate's: the clock is satisfied by
/// the swept hull overlapping a volume, so a distance that falls to zero on
/// overlap prefers a fly-through, while a distance to the volume's centre
/// prefers stopping in the middle of it.
fn hull_distance(b: &Aabb, origin: Vec3, half: Vec3, offset: Vec3) -> Scalar {
    let c = origin + offset;
    let mut sum = s(0.0);
    for i in 0..3 {
        let (lo, hi) = (b.mins[i] - half[i], b.maxs[i] + half[i]);
        let over = if c[i] < lo {
            lo - c[i]
        } else if c[i] > hi {
            c[i] - hi
        } else {
            s(0.0)
        };
        sum += over * over;
    }
    sum.sqrt()
}

/// Ground speed: the length of the velocity's horizontal part, which is the
/// number every speed claim in this project is stated in.
#[must_use]
pub fn horizontal_speed(v: Vec3) -> Scalar {
    vec3(v.x, v.y, s(0.0)).length()
}

/// Whether a player hull at `origin` is inside `b`.
#[must_use]
pub fn hull_in_box(b: &Aabb, origin: Vec3, half: Vec3, offset: Vec3) -> bool {
    let c = origin + offset;
    (0..3).all(|i| c[i] + half[i] >= b.mins[i] && c[i] - half[i] <= b.maxs[i])
}

// ---------------------------------------------------------------------------
// The control alphabet.

/// What the search is allowed to press.
///
/// A parameter rather than a constant, because whether a map is solvable at all
/// can turn on it — `training-crouch-slide` demands a CROUCH a coil-shaped
/// control set does not have — and a result reported without its alphabet is not
/// reproducible. This is assumption a6 made operable.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Alphabet {
    /// Whether CROUCH may be held.
    pub crouch: bool,
}

impl Alphabet {
    /// The name that goes in a printout beside any number this alphabet produced.
    #[must_use]
    pub fn label(self) -> &'static str {
        if self.crouch { "jump+crouch" } else { "jump-only" }
    }
}

/// One held input: a turn rate, a move direction, and the two buttons.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Control {
    /// Degrees of yaw applied per command.
    pub yaw_rate: Scalar,
    /// `right_move`.
    pub right: i8,
    /// `forward_move`.
    pub forward: i8,
    /// Whether to press JUMP when there is ground to leave.
    pub jump: bool,
    /// Whether to hold CROUCH.
    pub crouch: bool,
}

/// The turn-rate ladder.
///
/// A search constant, not a map constant: it is the same eleven rates for every
/// course. Strafe acceleration is largest when the view turns at a rate matched
/// to the strafe direction, so the ladder is denser near zero where that match
/// is delicate and coarser at the ends where it is not.
const YAW_RATES: [Scalar; 11] = [-12.0, -8.0, -5.0, -3.0, -1.5, 0.0, 1.5, 3.0, 5.0, 8.0, 12.0];

/// The six move directions, as `(forward, right)`.
///
/// `(0, 0)` is included on purpose: with JUMP it is a ballistic flight with no
/// air-control input, which is the only way to express "stop steering and let
/// the arc happen" — and a fork whose far side is reached by a leap needs it.
const MOVES: [(i8, i8); 6] = [(127, 0), (127, 127), (127, -127), (0, 127), (0, -127), (0, 0)];

/// Every control the alphabet admits, in a fixed order so a run is reproducible.
#[must_use]
pub fn controls(alphabet: Alphabet) -> Vec<Control> {
    let crouches: &[bool] = if alphabet.crouch { &[false, true] } else { &[false] };
    let mut out = Vec::with_capacity(YAW_RATES.len() * MOVES.len() * 2 * crouches.len());
    for &yaw_rate in &YAW_RATES {
        for &(forward, right) in &MOVES {
            for jump in [false, true] {
                for &crouch in crouches {
                    out.push(Control {
                        yaw_rate,
                        right,
                        forward,
                        jump,
                        crouch,
                    });
                }
            }
        }
    }
    out
}

/// Hold one control for `ticks` commands, optionally recording them.
///
/// The JUMP button is pressed only when there is ground to leave, because Q3
/// requires releasing jump between hops; holding it would silently turn a
/// bunny-hop chain into one jump and a long fall.
fn hold(
    state: &SimState,
    c: Control,
    ticks: u32,
    rate: TickRate,
    world: &impl World,
    profile: &PhysicsProfile,
    mut record: Option<&mut Vec<UserCmd>>,
) -> (SimState, TriggerSet, u64) {
    let mut st = *state;
    let mut touched = TriggerSet::NONE;
    let mut simulated = 0u64;
    for _ in 0..ticks {
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
        touched = touched.with(step_in_place(&mut st, &cmd, world, profile));
        simulated += 1;
    }
    (st, touched, simulated)
}

// ---------------------------------------------------------------------------
// The visited set.

/// A coarse equivalence class of states, so the search does not re-derive the
/// same circling from a position it has already been in.
///
/// Every quantum comes from the profile: cells are measured in player hull
/// widths and speed in fractions of `max_speed`. No extent of any map appears.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
struct Cell {
    x: i32,
    y: i32,
    z: i32,
    /// Horizontal velocity octant, or 8 for "not moving".
    dir: u8,
    /// Speed bucket.
    speed: u8,
    /// 0 airborne, 1 sliding, 2 grounded.
    stance: u8,
    /// The clock bits touched: a state that has crossed a checkpoint is not the
    /// same state as one that has not, however identical the player is.
    touched: u32,
}

/// How the state space is quantised. Derived once from the profile and the
/// search's own settings.
#[derive(Debug, Clone, Copy)]
struct Quantiser {
    cell: Scalar,
    speed_bucket: Scalar,
}

impl Quantiser {
    fn new(profile: &PhysicsProfile, cells_per_hull: Scalar) -> Self {
        let hull_width = profile.hull_maxs.x - profile.hull_mins.x;
        Self {
            cell: (hull_width * cells_per_hull).max(s(1.0)),
            // Scaled by the same knob as position, because the two failures they
            // cause are the same failure. A cell closed while the player was
            // slow can never be revisited while the player is fast, so a speed
            // bucket wider than the difference between "will clear that ledge"
            // and "will not" throws away the approach that works. Half of
            // walking speed was the first choice here and it cost `coil`: the
            // search stalled below the finish platform because the state that
            // could have jumped it shared a cell with one that could not.
            speed_bucket: (profile.max_speed * cells_per_hull * s(0.5)).max(s(1.0)),
        }
    }

    fn of(&self, state: &SimState, touched: TriggerSet) -> Cell {
        let o = state.player.origin;
        let v = state.player.velocity;
        let (vx, vy) = (v.x, v.y);
        let speed = vec3(vx, vy, s(0.0)).length();
        // Octants from sign and magnitude comparisons rather than from `atan2`:
        // exact, and free of a transcendental whose last bit is not specified.
        let dir = if speed < s(1.0) {
            8
        } else {
            let right = u8::from(vx >= s(0.0));
            let up = u8::from(vy >= s(0.0));
            let steep = u8::from(vy.abs() > vx.abs());
            (right << 2) | (up << 1) | steep
        };
        Cell {
            x: (o.x / self.cell).floor() as i32,
            y: (o.y / self.cell).floor() as i32,
            z: (o.z / self.cell).floor() as i32,
            dir,
            speed: (speed / self.speed_bucket).floor().min(s(255.0)) as u8,
            stance: match state.player.ground {
                GroundState::Airborne => 0,
                GroundState::Sliding { .. } => 1,
                GroundState::Grounded { .. } => 2,
            },
            touched: touched.0,
        }
    }
}

// ---------------------------------------------------------------------------
// The open list.

/// A node's place in the ordering: more goals reached is better, then lower `f`,
/// then earlier generation.
///
/// Compared with `total_cmp` rather than `partial_cmp`, so the order is total
/// even if a state ever went non-finite, and so two runs of the same search
/// order the frontier identically.
#[derive(Debug, Clone, Copy)]
struct Key {
    reached: u16,
    f: Scalar,
    seq: u64,
}

impl Ord for Key {
    fn cmp(&self, other: &Self) -> Ordering {
        // `BinaryHeap` is a max-heap and pops the greatest, so "greater" here
        // must mean "expand me first".
        self.reached
            .cmp(&other.reached)
            .then_with(|| other.f.total_cmp(&self.f))
            .then_with(|| other.seq.cmp(&self.seq))
    }
}

impl PartialOrd for Key {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

impl PartialEq for Key {
    fn eq(&self, other: &Self) -> bool {
        self.cmp(other) == Ordering::Equal
    }
}

impl Eq for Key {}

#[derive(Debug, Clone, Copy)]
struct Entry {
    key: Key,
    node: u32,
}

impl Ord for Entry {
    fn cmp(&self, other: &Self) -> Ordering {
        self.key.cmp(&other.key)
    }
}

impl PartialOrd for Entry {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

impl PartialEq for Entry {
    fn eq(&self, other: &Self) -> bool {
        self.key == other.key
    }
}

impl Eq for Entry {}

// ---------------------------------------------------------------------------
// Nodes.

/// One state the search reached, and how it got there.
#[derive(Debug, Clone)]
struct Node {
    state: SimState,
    touched: TriggerSet,
    parent: u32,
    control: u16,
    depth: u32,
    reached: u16,
    f: Scalar,
    /// Expansions completed when this node was generated. With
    /// [`Node::expanded_at`] this gives the number of expansions it waited
    /// through, which is the r9 demonstration's central statistic.
    generated_at: u64,
    /// `f` of the successor a greedy search would have committed to at the
    /// expansion that generated this node — the horizon-argmax among its
    /// siblings. `f - argmax_f` is how much worse than that choice this node
    /// looked at the moment it was kept.
    argmax_f: Scalar,
    /// How many goals that argmax sibling had reached. A handicap is only a
    /// comparison when this equals the node's own `reached`; across different
    /// counts the two `f` values measure distance to different volumes.
    argmax_reached: u16,
    /// Whether this node *is* that argmax. A greedy search commits to exactly
    /// the nodes for which this is true, so a winning path containing a `false`
    /// is a path greedy could not have produced.
    is_argmax: bool,
    /// Expansions completed when this node was popped, or `None` if it never was.
    expanded_at: Option<u64>,
}

const NO_PARENT: u32 = u32::MAX;

// ---------------------------------------------------------------------------
// The specification and the result.

/// Everything the search is allowed to know.
#[derive(Debug, Clone)]
pub struct SearchSpec {
    /// The single knob that degrades this search to the prior art. `1` is
    /// exactly greedy one-step-per-window.
    pub frontier: usize,
    /// Commands per edge.
    pub stride: u32,
    /// Weight on time already spent, in units of the distance the player could
    /// have covered in it. Zero is pure greedy-best-first on distance.
    pub patience: Scalar,
    /// Cell size in player hull widths.
    pub cells_per_hull: Scalar,
    /// What may be pressed.
    pub alphabet: Alphabet,
    /// Maximum expansions before the search gives up.
    pub max_expansions: u64,
    /// Maximum edge simulations before it gives up.
    pub max_ticks: u64,
    /// Maximum edges on a single path, so a plan cannot grow without bound.
    pub max_depth: u32,
}

impl Default for SearchSpec {
    fn default() -> Self {
        Self {
            frontier: 512,
            stride: 8,
            patience: s(0.25),
            cells_per_hull: s(1.0),
            alphabet: Alphabet { crouch: true },
            max_expansions: 60_000,
            max_ticks: 60_000_000,
            max_depth: 4_000,
        }
    }
}

/// Why the search stopped. Printed with every outcome, and load-bearing for a
/// negative.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Stop {
    /// The run clock reached `Finished` with every declared goal touched.
    Finished,
    /// The expansion budget ran out.
    ExpansionsExhausted,
    /// The simulation budget ran out.
    TicksExhausted,
    /// The open list emptied: every reachable cell was expanded.
    FrontierEmpty,
}

/// One trigger volume crossed, as the reconstructed run crossed it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Crossing {
    /// Simulation time at the end of the command that crossed it.
    pub time_ms: u32,
    /// Run clock at the same moment.
    pub run_ms: u32,
    /// Which step of the declared course this was.
    pub step: Step,
}

/// A decision on the winning path that a greedy search could not have taken.
///
/// This is r9's evidence in its most direct form, and b7's M1 asked for exactly
/// these numbers: the action the search committed to, the strictly better action
/// at the horizon that it rejected, and the scores of both.
#[derive(Debug, Clone, PartialEq)]
pub struct Deferral {
    /// Edges from the spawn.
    pub depth: u32,
    /// `f` of the node the search committed to — action `A` in b7's terms.
    pub f: Scalar,
    /// `f` of the sibling a greedy search would have taken — action `B`.
    pub argmax_f: Scalar,
    /// `f - argmax_f`, when the two are comparable. Strictly positive means the
    /// search committed to a state that scored worse at the horizon.
    pub handicap: Option<Scalar>,
    /// Expansions the search spent elsewhere between generating this node and
    /// expanding it.
    pub waited: u64,
}

/// What the search found.
#[derive(Debug, Clone)]
pub struct SearchResult {
    /// Why it stopped.
    pub stop: Stop,
    /// The final state of the best path found.
    pub end: SimState,
    /// The command stream, re-simulated from the spawn along the winning
    /// controls rather than stitched together from cached fragments.
    pub cmds: Vec<UserCmd>,
    /// The bits that path touched.
    pub touched: TriggerSet,
    /// Every declared volume it crossed, in the order it crossed them.
    pub crossings: Vec<Crossing>,
    /// How many declared goals it satisfied, in declared order.
    pub reached: usize,
    /// Every node of the winning path whose handicap was strictly positive.
    pub deferrals: Vec<Deferral>,
    /// Decisions at which the committed successor was not the horizon-argmax.
    /// b7's M4 counts this per map.
    pub non_argmax_decisions: usize,
    /// Edges on the winning path.
    pub path_len: u32,
    /// Coverage: nodes expanded.
    pub expansions: u64,
    /// Coverage: nodes generated.
    pub generated: u64,
    /// Coverage: commands simulated.
    pub simulated: u64,
    /// Coverage: distinct cells closed.
    pub cells_closed: usize,
    /// Whether re-simulating the winning controls reproduced the node's own
    /// checksum. False is a defect in this module, not a finding about a map.
    pub reconstruction_agrees: bool,
    /// The checksum the re-simulated stream ends on — the number a replay
    /// through the shipped binary must match.
    pub checksum: u64,
}

impl SearchResult {
    /// The run time in milliseconds, if the clock finished.
    #[must_use]
    pub fn run_ms(&self) -> Option<u32> {
        match self.end.run {
            RunState::Finished {
                started_at_ms,
                finished_at_ms,
            } => Some(finished_at_ms - started_at_ms),
            _ => None,
        }
    }

    /// The largest handicap on the winning path: the strongest single instance
    /// of the search committing to a state that scored worse at the horizon
    /// than one it could have had instead.
    #[must_use]
    pub fn max_handicap(&self) -> Scalar {
        self.deferrals
            .iter()
            .filter_map(|d| d.handicap)
            .fold(s(0.0), Scalar::max)
    }
}

// ---------------------------------------------------------------------------
// The search itself.

/// How many goals of `goals`, in declared order, `touched` satisfies.
fn reached_count(goals: &[Goal], touched: TriggerSet) -> u16 {
    let mut n = 0u16;
    for g in goals {
        if touched.contains(g.bits) {
            n += 1;
        } else {
            break;
        }
    }
    n
}

/// Distance from a hull at `origin` to the nearest volume of the next goal.
fn heuristic(
    goals: &[Goal],
    reached: u16,
    origin: Vec3,
    half: Vec3,
    offset: Vec3,
) -> Scalar {
    let Some(goal) = goals.get(reached as usize) else {
        return s(0.0);
    };
    goal.boxes
        .iter()
        .map(|b| hull_distance(b, origin, half, offset))
        .fold(Scalar::INFINITY, Scalar::min)
}

/// Run the search.
///
/// `goals` comes from [`goals_of`], so everything about where to go traces back
/// to the compiled map. `spec` is the same for every map by construction — the
/// caller does not get a per-map hook, because there is none to give.
#[must_use]
pub fn run(
    goals: &[Goal],
    spec: &SearchSpec,
    rate: TickRate,
    start: SimState,
    world: &impl World,
    profile: &PhysicsProfile,
) -> SearchResult {
    let half = profile.hull_half_extents();
    let offset = profile.hull_center_offset();
    let set = controls(spec.alphabet);
    let quant = Quantiser::new(profile, spec.cells_per_hull);
    // How far the player could travel in one edge at full speed: the exchange
    // rate between "time spent" and "distance to go", so `patience` is
    // dimensionless.
    let edge_units = profile.max_speed * (rate.command_millis() as Scalar / s(1000.0))
        * spec.stride as Scalar;

    let mut nodes: Vec<Node> = Vec::new();
    let mut open: BinaryHeap<Entry> = BinaryHeap::new();
    let mut closed: HashSet<Cell> = HashSet::new();
    let mut seq = 0u64;
    let mut expansions = 0u64;
    let mut generated = 0u64;
    let mut simulated = 0u64;
    let mut non_argmax = 0usize;

    let root_touched = TriggerSet::NONE;
    let root_reached = reached_count(goals, root_touched);
    let root_h = heuristic(goals, root_reached, start.player.origin, half, offset);
    nodes.push(Node {
        state: start,
        touched: root_touched,
        parent: NO_PARENT,
        control: 0,
        depth: 0,
        reached: root_reached,
        f: root_h,
        generated_at: 0,
        argmax_f: root_h,
        argmax_reached: root_reached,
        is_argmax: true,
        expanded_at: None,
    });
    open.push(Entry {
        key: Key {
            reached: root_reached,
            f: root_h,
            seq,
        },
        node: 0,
    });
    seq += 1;

    // The best node seen, for the negative case: a search that does not finish
    // still reports the furthest it got, which is what makes a negative a
    // measurement rather than a shrug.
    let mut best_node = 0u32;
    let mut best_key = (root_reached, root_h);
    let mut goal_node: Option<u32> = None;
    let mut stop = Stop::FrontierEmpty;







    while let Some(entry) = open.pop() {
        if expansions >= spec.max_expansions {
            stop = Stop::ExpansionsExhausted;
            break;
        }
        if simulated >= spec.max_ticks {
            stop = Stop::TicksExhausted;
            break;
        }
        let index = entry.node as usize;
        let cell = quant.of(&nodes[index].state, nodes[index].touched);
        if !closed.insert(cell) {
            continue;
        }
        nodes[index].expanded_at = Some(expansions);
        expansions += 1;

        let parent_state = nodes[index].state;
        let parent_touched = nodes[index].touched;
        let parent_depth = nodes[index].depth;
        if parent_depth >= spec.max_depth {
            continue;
        }

        // Every admissible successor is generated before any is judged, because
        // the horizon-argmax has to be known to say whether the committed one
        // was it.
        //
        // "Admissible" excludes a successor whose cell is already closed, and
        // that filter runs *before* the argmax rather than after. The difference
        // matters: computed over all successors, the argmax can be a state the
        // search already expanded, and then even the `frontier = 1` control
        // looks as though it departed from greedy when all it did was decline to
        // walk in a circle. The greedy control must be "the best move still
        // open", or the statistic that carries r9 counts the loop-breaker.
        // JUMP is only pressed when there is ground to leave, so from an
        // airborne state the two halves of the alphabet that differ only in it
        // produce bit-identical successors. Skipping them is exact rather than
        // an approximation, and it halves the branching factor for the whole
        // airborne part of a run — which on a strafe-jumping course is most of
        // it.
        let grounded = matches!(parent_state.player.ground, GroundState::Grounded { .. });

        let mut batch: Vec<(usize, u16, Scalar, SimState, TriggerSet)> =
            Vec::with_capacity(set.len());
        for (i, c) in set.iter().enumerate() {
            if c.jump && !grounded {
                continue;
            }
            let (st, crossed, ticks) =
                hold(&parent_state, *c, spec.stride, rate, world, profile, None);
            simulated += ticks;
            if !st.player.origin.is_finite() {
                continue;
            }
            let touched = parent_touched.with(crossed);
            if closed.contains(&quant.of(&st, touched)) {
                continue;
            }
            let reached = reached_count(goals, touched);
            let h = heuristic(goals, reached, st.player.origin, half, offset);
            let g = spec.patience * edge_units * (parent_depth + 1) as Scalar;
            batch.push((i, reached, h + g, st, touched));
        }

        // The one a greedy search would commit to here. Computed on every
        // expansion whatever this search does with it, because "the search chose
        // an action that was not the argmax" is only sayable if the argmax is
        // known.
        //
        // The third comparison is not decoration. `max_by` returns the LAST
        // maximum, while the open list's key breaks ties toward the lowest
        // sequence number and so pops the FIRST. Without agreeing on ties, two
        // controls with identical scores make the `frontier = 1` control look as
        // though it departed from greedy at every tie — which it does not.
        let argmax = batch
            .iter()
            .max_by(|a, b| {
                a.1.cmp(&b.1)
                    .then_with(|| b.2.total_cmp(&a.2))
                    .then_with(|| b.0.cmp(&a.0))
            })
            .map(|entry| (entry.0, entry.1, entry.2));

        for (i, reached, f, st, touched) in batch {
            let (argmax_index, argmax_reached, argmax_f) = argmax.unwrap_or((i, reached, f));
            let node_index = nodes.len() as u32;
            nodes.push(Node {
                state: st,
                touched,
                parent: entry.node,
                control: i as u16,
                depth: parent_depth + 1,
                reached,
                f,
                generated_at: expansions,
                argmax_f,
                argmax_reached,
                is_argmax: i == argmax_index,
                expanded_at: None,
            });
            generated += 1;
            open.push(Entry {
                key: Key { reached, f, seq },
                node: node_index,
            });
            seq += 1;

            if reached > best_key.0 || (reached == best_key.0 && f < best_key.1) {
                best_key = (reached, f);
                best_node = node_index;
            }
            if reached as usize == goals.len()
                && matches!(st.run, RunState::Finished { .. })
                && goal_node.is_none()
            {
                goal_node = Some(node_index);
            }
        }

        if goal_node.is_some() {
            stop = Stop::Finished;
            break;
        }

        // Trim back to the frontier cap, after every expansion and to exactly
        // the cap.
        //
        // An earlier version let the list grow to a multiple of the cap before
        // trimming, to amortise the cost. That is wrong for the one setting the
        // whole demonstration rests on: whenever a successor batch was smaller
        // than the slack, `frontier = 1` quietly kept siblings and could return
        // to one later — which is not greedy one-step-per-window, and the
        // control would have been measuring something other than the prior art.
        //
        // Selection rather than a sort, so this stays linear: the entries are
        // totally ordered because every key carries a unique sequence number, so
        // *which* entries survive is deterministic even though their order after
        // partitioning is not, and the heap is rebuilt from them anyway.
        if open.len() > spec.frontier {
            let mut kept: Vec<Entry> = std::mem::take(&mut open).into_vec();
            let cut = kept.len() - spec.frontier;
            kept.select_nth_unstable(cut);
            kept.drain(..cut);
            open = BinaryHeap::from(kept);
        }
    }

    // Whichever node the search is standing behind: the goal if it found one,
    // else the furthest it got.
    let winner = goal_node.unwrap_or(best_node);

    // Record which committed decisions were not the horizon-argmax, and how long
    // each node of the winning path waited. Both are read off the path rather
    // than accumulated during the search, so they describe the run that shipped.
    let mut path: Vec<u32> = Vec::new();
    let mut walk = winner;
    while walk != NO_PARENT {
        path.push(walk);
        walk = nodes[walk as usize].parent;
    }
    path.reverse();

    let mut deferrals = Vec::new();
    for &n in &path {
        let node = &nodes[n as usize];
        if node.parent == NO_PARENT {
            continue;
        }
        if node.is_argmax {
            continue;
        }
        // The search committed to a successor greedy would have passed over.
        non_argmax += 1;
        deferrals.push(Deferral {
            depth: node.depth,
            f: node.f,
            argmax_f: node.argmax_f,
            // Only a comparison when both scored distance to the same volume.
            // A sibling that crossed a checkpoint this one did not is better for
            // a reason `f` does not express, and subtracting the two would
            // invent a number.
            handicap: (node.argmax_reached == node.reached).then(|| node.f - node.argmax_f),
            waited: node
                .expanded_at
                .unwrap_or(expansions)
                .saturating_sub(node.generated_at),
        });
    }

    // Re-simulate the winning controls from the spawn. This produces the command
    // stream *and* checks the search's own bookkeeping: the reconstructed state
    // must have the checksum the node recorded, or the path does not describe a
    // run anybody can replay.
    let mut cmds: Vec<UserCmd> = Vec::new();
    let mut state = start;
    let mut touched = TriggerSet::NONE;
    let mut crossings: Vec<Crossing> = Vec::new();
    for &n in path.iter().skip(1) {
        let control = set[nodes[n as usize].control as usize];
        let before = cmds.len();
        let (next, crossed, _) = hold(
            &state,
            control,
            spec.stride,
            rate,
            world,
            profile,
            Some(&mut cmds),
        );
        debug_assert!(cmds.len() > before);
        state = next;
        for g in goals {
            if crossed.contains(g.bits) && !touched.contains(g.bits) {
                crossings.push(Crossing {
                    time_ms: state.time_ms,
                    run_ms: state.run.elapsed_ms(state.time_ms).unwrap_or(0),
                    step: g.step,
                });
            }
        }
        touched = touched.with(crossed);
    }

    let reconstruction_agrees = state.checksum() == nodes[winner as usize].state.checksum();

    SearchResult {
        stop,
        end: state,
        cmds,
        touched,
        crossings,
        reached: reached_count(goals, touched) as usize,
        deferrals,
        non_argmax_decisions: non_argmax,
        path_len: path.len().saturating_sub(1) as u32,
        expansions,
        generated,
        simulated,
        cells_closed: closed.len(),
        reconstruction_agrees,
        checksum: state.checksum(),
    }
}

#[cfg(test)]
mod tests;
