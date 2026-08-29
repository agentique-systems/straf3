//! The course, derived from the map rather than written down.
//!
//! # The question this module answers
//!
//! An agent needs somewhere to go. `probes/coil-course` was told: aim at
//! `(0, 3456, 112)` once past `y = 3040`, and run for increasing `y` until
//! then. Every one of those numbers is a fact about `coil.map` typed into a
//! program, and a second map defeats all of them at once.
//!
//! What a compiled map already carries instead:
//!
//! - `CompiledMap::triggers` — the timing volumes, in source order, each
//!   classified as [`TriggerKind::Start`], [`TriggerKind::Checkpoint`] or
//!   [`TriggerKind::Finish`] by resolving its `target` through the entity list.
//! - `CompiledMap::collider()` — the world the shipped game collides against,
//!   which is what says where the ground under a volume is.
//! - `PhysicsProfile` — how big the player is, which is what says where inside
//!   a volume a player can be.
//!
//! [`CoursePlan::derive`] turns those three into an ordered list of waypoints
//! with an aim point each. It is the only place in this crate that decides
//! *where to go*, so it is the only place that could hide a per-map constant.
//!
//! # General rules and the fallbacks below them
//!
//! Two derivations are general rules — defensible on geometry nobody has seen:
//!
//! - **Horizontal: the centre of the volume's bounds.** A trigger is authored
//!   to be crossed, so the point furthest inside it in the plane the player
//!   runs in is the middle of it.
//! - **Vertical: where a player standing inside the volume would be.** Trace a
//!   player-sized box down through the volume and put the aim point at the
//!   origin it comes to rest at. This is strictly better than the probe's
//!   `mins.z + 48`, which is that same idea with coil's numbers already
//!   substituted in — and it is right for a volume whose floor is not its
//!   `mins.z`, which the probe's rule is not.
//!
//! Each has one fallback, and a fallback is a [`Note`] in the printout rather
//! than a silent substitution:
//!
//! - [`Horizontal::LargestPiece`], when the bounds centre is not inside the
//!   volume. One trigger entity may own several brushes — an L-shaped start
//!   line spanning a corner is two boxes, and the centre of their union is in
//!   the wall between them.
//! - [`Vertical::VolumeCentre`], when there is no ground under the volume the
//!   player could stand on, or when standing on it would put the player outside
//!   the volume. A finish line over a pit is a real thing to author; the honest
//!   answer there is the middle of the box and a note saying the rule did not
//!   apply.
//!
//! # What is deliberately *not* derived
//!
//! The order of the checkpoints. It is source order, because that is the only
//! numbering Defrag maps carry, and this module has no way to check it against
//! the author's intent. See [`Note::CheckpointOrderIsSourceOrder`].

use straf3_map::{Aabb, CompiledMap, SPAWN_CLEARANCE, TriggerKind, TriggerVolume};
use straf3_sim::PhysicsProfile;
use straf3_sim::num::{Scalar, Vec3, s, vec3};
use straf3_sim::world::{Sweep, World};

/// One step of a course: something the run clock reads, in the order a run
/// crosses it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Step {
    /// The start line. The clock starts here.
    Start,
    /// A checkpoint, by the index `straf3-map` assigned it from source order.
    Checkpoint(u32),
    /// The finish line. The clock stops here.
    Finish,
}

impl core::fmt::Display for Step {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            Self::Start => write!(f, "start"),
            Self::Checkpoint(index) => write!(f, "cp{index}"),
            Self::Finish => write!(f, "finish"),
        }
    }
}

/// How a target's horizontal aim was chosen.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Horizontal {
    /// GENERAL RULE. The centre of the volume's bounds in `x` and `y`.
    BoundsCentre,
    /// FALLBACK. The bounds centre is not inside the volume — the volume is
    /// several brushes and the middle of their union falls between them — so
    /// the centre of the largest piece is used instead. Carries the index of
    /// the piece within `TriggerVolume::hulls`.
    LargestPiece(usize),
}

/// How a target's vertical aim was chosen.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Vertical {
    /// GENERAL RULE. A player standing on the surface found inside the volume.
    /// Carries that surface's height.
    Standing(Scalar),
    /// FALLBACK. No standable surface inside the volume, so the middle of it.
    VolumeCentre,
}

/// One volume the agent may aim at, with the aim point derived from it.
#[derive(Debug, Clone, PartialEq)]
pub struct Target {
    /// Index into [`CompiledMap::triggers`], so a reader can go back to the
    /// source of every number here.
    pub trigger: usize,
    /// The brush entity's `target` key — the name a mapper reads.
    pub name: Option<String>,
    /// The classname that `target` resolved to.
    pub target_classname: Option<String>,
    /// The volume's bounds, all pieces together.
    pub bounds: Aabb,
    /// How many convex pieces the volume has.
    pub pieces: usize,
    /// Where the agent should try to put the player's origin.
    pub aim: Vec3,
    /// How the horizontal half of [`Target::aim`] was decided.
    pub horizontal: Horizontal,
    /// How the vertical half of [`Target::aim`] was decided.
    pub vertical: Vertical,
    /// Whether a player hull at [`Target::aim`] actually overlaps the volume.
    ///
    /// False is a finding, not a crash: it means neither rule nor fallback put
    /// the aim inside the box, and a run that steers at it may cross the volume
    /// without the aim ever being reached.
    pub aim_inside: bool,
}

/// One step of the course, with every volume that satisfies it.
///
/// More than one is unusual but legal: `TriggerSet::START` is a single bit, so
/// a map with two start lines starts its clock at whichever the player crosses.
/// Checkpoints cannot share a step — each gets its own index — so a waypoint
/// with alternatives is always a start or a finish.
#[derive(Debug, Clone, PartialEq)]
pub struct Waypoint {
    /// Which step of the run this is.
    pub step: Step,
    /// The volumes that satisfy it, in source order. Never empty.
    pub targets: Vec<Target>,
}

impl Waypoint {
    /// The volume the plan's legs are measured against: the first in source
    /// order.
    #[must_use]
    pub fn primary(&self) -> &Target {
        &self.targets[0]
    }
}

/// The straight line between two consecutive aim points.
///
/// Descriptive, not prescriptive: nothing steers along a leg. It is here because
/// it is the cheapest honest answer to "does this map turn?", which is the
/// question a course built to defeat a `+y`-seeking bot has to be judged on.
#[derive(Debug, Clone, PartialEq)]
pub struct Leg {
    /// Where the leg starts, as it appears in the printout.
    pub from: String,
    /// Where it ends.
    pub to: String,
    /// Straight-line distance in world units.
    pub distance: Scalar,
    /// The same, ignoring height.
    pub ground_distance: Scalar,
    /// Height gained (negative for a drop).
    pub rise: Scalar,
    /// Compass bearing in degrees, Quake convention: 0 is `+x`, increasing
    /// anticlockwise, the same convention as a view yaw.
    ///
    /// Computed with `atan2`, which is not IEEE-specified, so this is printed
    /// to a tenth of a degree and no decision is taken on it. See the module
    /// docs of `straf3_sim::num` for why that distinction matters here.
    pub bearing_deg: Scalar,
    /// Change of bearing from the previous leg, wrapped to ±180°. `None` for
    /// the first leg, which has nothing to turn from.
    pub turn_deg: Option<Scalar>,
}

/// Something about this map the derivation had to decide, report, or give up on.
///
/// Notes are data rather than log lines, and they are the mechanism by which
/// this module refuses to make a silent guess.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Note {
    /// No `target_startTimer` volume. The clock can never start, so there is no
    /// course to run.
    NoStart,
    /// No `target_stopTimer` volume. A run can begin and can never end.
    NoFinish,
    /// More than one volume starts the clock. The plan aims at the first in
    /// source order and any of them would do.
    SeveralStarts(usize),
    /// More than one volume stops it.
    SeveralFinishes(usize),
    /// Checkpoint indices are not contiguous — index `0` is missing while a
    /// higher one exists. `straf3-map` numbers them from zero as it meets them,
    /// so this means a checkpoint was dropped after numbering: past
    /// `TriggerSet::MAX_CHECKPOINTS`, most likely, which the compiler reports
    /// separately.
    CheckpointGap(u32),
    /// The map has more than one checkpoint, so the route depends on an order
    /// this crate did not derive and cannot check. Defrag gives checkpoints no
    /// explicit index; `straf3-map` numbers them by order of appearance in the
    /// `.map` file, and a mapper who declares them out of order gets a plan
    /// that visits them out of order.
    CheckpointOrderIsSourceOrder(usize),
    /// Checkpoints in this map declare a numeric `count` key, and nothing reads
    /// it. The compiler takes six keys out of a `.map` — `classname`, `origin`,
    /// `angle`, `angles`, `target`, `targetname` — and `count` is not among
    /// them, so it orders nothing and reaches no compiled artefact. Both
    /// first-party maps declare it. Reported because a key that looks like a
    /// checkpoint index and is not one is a trap for the next map author.
    CheckpointCountIsNotRead(usize),
    /// Worse: the `count` keys imply a different order than the compiler
    /// assigned from source order. Carries the compiled checkpoint indices in
    /// the order the counts would put them. The compiled order is what every
    /// reader in this tree sees; the counts are prose in a file.
    CheckpointCountContradictsSourceOrder(Vec<u32>),
    /// The map has checkpoints and they do not gate the clock.
    /// `RunState::finish` reads `TriggerSet::FINISH` alone, so a run that skips
    /// every checkpoint still produces a time. They are used as intermediate
    /// goals because they are the author's statement of the route, and this
    /// note is the reason a completed run must say which of them it touched.
    CheckpointsDoNotGateTheClock(usize),
    /// A volume whose aim point ended up outside it. Carries the trigger index.
    AimOutsideVolume(usize),
    /// The player hull at the spawn overlaps solid geometry. The map's own
    /// problem, but a run from it is not a run of the map the author meant.
    SpawnInSolid,
    /// Nothing under the spawn within the map's bounds: the player starts over a
    /// void and falls.
    NothingUnderSpawn,
}

/// What the spawn point actually is, checked rather than assumed.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct SpawnCheck {
    /// The player origin the compiler chose, already lifted by
    /// [`SPAWN_CLEARANCE`].
    pub origin: Vec3,
    /// Which way the player faces, degrees, Quake convention.
    pub yaw: Scalar,
    /// Whether a hull there begins inside solid.
    pub start_solid: bool,
    /// Whether it is wholly inside solid.
    pub all_solid: bool,
    /// The surface height under the spawn, if there is one within the map.
    pub ground_z: Option<Scalar>,
}

/// A course: where to start, what to cross, in what order.
#[derive(Debug, Clone, PartialEq)]
pub struct CoursePlan {
    /// The profile whose hull the aim points were derived for.
    pub profile_name: String,
    /// The spawn, and what is under it.
    pub spawn: SpawnCheck,
    /// The steps of the run, in order. Empty when the map has no timing.
    pub waypoints: Vec<Waypoint>,
    /// Spawn to first waypoint, then waypoint to waypoint.
    pub legs: Vec<Leg>,
    /// Everything the derivation decided for itself, in the order it decided it.
    pub notes: Vec<Note>,
}

impl CoursePlan {
    /// Derive the course from a compiled map.
    ///
    /// The only inputs are the map and the profile. Nothing about any
    /// particular map reaches this function, which is the property r27 asks for
    /// and the one worth breaking a test over.
    #[must_use]
    pub fn derive(map: &CompiledMap, profile: &PhysicsProfile, profile_name: &str) -> Self {
        let world = map.collider();
        let mut notes = Vec::new();

        let spawn = check_spawn(map, &world, profile, &mut notes);
        let waypoints = waypoints(map, &world, profile, &mut notes);
        let legs = legs(&spawn, &waypoints);

        Self {
            profile_name: profile_name.to_owned(),
            spawn,
            waypoints,
            legs,
            notes,
        }
    }

    /// Whether this map has both ends of a run and a plan can be executed.
    ///
    /// The same question `CompiledMap::has_timing` answers, asked of the derived
    /// plan so that a caller cannot act on a plan that has no finish in it.
    #[must_use]
    pub fn is_runnable(&self) -> bool {
        self.waypoints
            .first()
            .is_some_and(|w| w.step == Step::Start)
            && self
                .waypoints
                .last()
                .is_some_and(|w| w.step == Step::Finish)
    }
}

// ---------------------------------------------------------------------------
// The spawn

fn check_spawn(
    map: &CompiledMap,
    world: &impl World,
    profile: &PhysicsProfile,
    notes: &mut Vec<Note>,
) -> SpawnCheck {
    let origin = map.spawn;
    let here = world.trace(&Sweep {
        start: origin,
        end: origin,
        half_extents: profile.hull_half_extents(),
        center_offset: profile.hull_center_offset(),
    });
    if here.start_solid {
        notes.push(Note::SpawnInSolid);
    }
    let ground_z = drop_to_ground(world, profile, origin, floor_of(map, profile))
        .map(|contact| surface_under(profile, contact));
    if ground_z.is_none() {
        notes.push(Note::NothingUnderSpawn);
    }
    SpawnCheck {
        origin,
        yaw: map.spawn_yaw,
        start_solid: here.start_solid,
        all_solid: here.all_solid,
        ground_z,
    }
}

// ---------------------------------------------------------------------------
// The waypoints

fn waypoints(
    map: &CompiledMap,
    world: &impl World,
    profile: &PhysicsProfile,
    notes: &mut Vec<Note>,
) -> Vec<Waypoint> {
    // Source order throughout: `CompiledMap::triggers` is documented as being in
    // it, and it is the only ordering a `.map` file supplies.
    let of_kind = |want: fn(TriggerKind) -> bool| -> Vec<usize> {
        map.triggers
            .iter()
            .enumerate()
            .filter(|(_, t)| want(t.kind))
            .map(|(i, _)| i)
            .collect()
    };

    let starts = of_kind(|k| k == TriggerKind::Start);
    let finishes = of_kind(|k| k == TriggerKind::Finish);
    let mut checkpoints: Vec<(u32, usize)> = map
        .triggers
        .iter()
        .enumerate()
        .filter_map(|(i, t)| match t.kind {
            TriggerKind::Checkpoint(index) => Some((index, i)),
            _ => None,
        })
        .collect();
    // By the index the compiler assigned, which is source order — sorted anyway
    // rather than assumed, because the plan's correctness rests on the order and
    // not on `triggers` and the numbering happening to agree.
    checkpoints.sort_by_key(|(index, _)| *index);

    if starts.is_empty() {
        notes.push(Note::NoStart);
    }
    if starts.len() > 1 {
        notes.push(Note::SeveralStarts(starts.len()));
    }
    if finishes.is_empty() {
        notes.push(Note::NoFinish);
    }
    if finishes.len() > 1 {
        notes.push(Note::SeveralFinishes(finishes.len()));
    }
    for (expected, (index, _)) in checkpoints.iter().enumerate() {
        if *index != expected as u32 {
            notes.push(Note::CheckpointGap(expected as u32));
            break;
        }
    }
    if checkpoints.len() > 1 {
        notes.push(Note::CheckpointOrderIsSourceOrder(checkpoints.len()));
    }
    check_declared_counts(map, &checkpoints, notes);
    if !checkpoints.is_empty() {
        notes.push(Note::CheckpointsDoNotGateTheClock(checkpoints.len()));
    }

    let mut steps: Vec<(Step, Vec<usize>)> = Vec::new();
    if !starts.is_empty() {
        steps.push((Step::Start, starts));
    }
    for (index, trigger) in checkpoints {
        steps.push((Step::Checkpoint(index), vec![trigger]));
    }
    if !finishes.is_empty() {
        steps.push((Step::Finish, finishes));
    }

    steps
        .into_iter()
        .map(|(step, triggers)| Waypoint {
            step,
            targets: triggers
                .into_iter()
                .map(|i| target(i, &map.triggers[i], map, world, profile, notes))
                .collect(),
        })
        .collect()
}

/// Compare the `count` keys the map's checkpoints declare against the order the
/// compiler actually assigned, and report a disagreement rather than resolving
/// it.
///
/// `count` is Defrag's checkpoint index and it is the one key in these maps that
/// looks authoritative and is not: `straf3-map` reads `classname`, `origin`,
/// `angle`, `angles`, `target` and `targetname`, and nothing else survives
/// compilation. Checkpoint numbering comes from the order the entities appear in
/// the file. On both first-party maps the two happen to agree, so nothing has
/// ever diverged — which is exactly the condition under which the next author
/// discovers it the hard way.
///
/// Not resolved here, deliberately: the compiled index is what every reader in
/// this tree sees, so silently preferring `count` would make this crate the one
/// component that disagrees with the game.
fn check_declared_counts(map: &CompiledMap, checkpoints: &[(u32, usize)], notes: &mut Vec<Note>) {
    let declared: Vec<(u32, i64)> = checkpoints
        .iter()
        .filter_map(|(index, trigger)| {
            declared_count(map, &map.triggers[*trigger]).map(|count| (*index, count))
        })
        .collect();
    if declared.is_empty() {
        return;
    }
    notes.push(Note::CheckpointCountIsNotRead(declared.len()));

    let mut by_count = declared.clone();
    by_count.sort_by_key(|(_, count)| *count);
    let counted: Vec<u32> = by_count.iter().map(|(index, _)| *index).collect();
    let compiled: Vec<u32> = declared.iter().map(|(index, _)| *index).collect();
    if counted != compiled {
        notes.push(Note::CheckpointCountContradictsSourceOrder(counted));
    }
}

/// The `count` value on the point entity a trigger's `target` names, if it has
/// one that parses as a number.
fn declared_count(map: &CompiledMap, volume: &TriggerVolume) -> Option<i64> {
    let name = volume.target.as_deref()?;
    map.entities
        .iter()
        .find(|e| {
            e.get("targetname")
                .is_some_and(|t| t.eq_ignore_ascii_case(name))
        })
        .and_then(|e| e.get("count"))
        .and_then(|value| value.trim().parse::<i64>().ok())
}

fn target(
    index: usize,
    volume: &TriggerVolume,
    map: &CompiledMap,
    world: &impl World,
    profile: &PhysicsProfile,
    notes: &mut Vec<Note>,
) -> Target {
    let (column, horizontal) = horizontal_aim(volume, profile);
    let (aim, vertical) = vertical_aim(volume, column, map, world, profile);
    let aim_inside = hull_overlaps(volume, aim, profile);
    if !aim_inside {
        notes.push(Note::AimOutsideVolume(index));
    }
    Target {
        trigger: index,
        name: volume.target.clone(),
        target_classname: volume.target_classname.clone(),
        bounds: volume.bounds,
        pieces: volume.hulls.len(),
        aim,
        horizontal,
        vertical,
        aim_inside,
    }
}

/// Where in the plane to cross the volume.
///
/// The general rule is the centre of its bounds. The fallback exists because a
/// volume is a *set* of convex pieces and the centre of their union need not be
/// in any of them.
fn horizontal_aim(
    volume: &TriggerVolume,
    profile: &PhysicsProfile,
) -> ((Scalar, Scalar), Horizontal) {
    let centre = midpoint(volume.bounds.mins, volume.bounds.maxs);
    // A player-sized box at the centre of the bounds, asked of the volume
    // itself rather than of its bounds: a union of two brushes has a bounding
    // box that contains the gap between them.
    if volume.intersects_box(centre, profile.hull_half_extents()) {
        return ((centre.x, centre.y), Horizontal::BoundsCentre);
    }
    // Largest by volume of its own bounds. A piece big enough to be the main
    // body of an L is the one a route through it should use, and "biggest box"
    // is a property of the geometry rather than of any map.
    let largest = volume
        .hulls
        .iter()
        .enumerate()
        .max_by(|(_, a), (_, b)| {
            box_volume(a.mins, a.maxs)
                .partial_cmp(&box_volume(b.mins, b.maxs))
                .unwrap_or(core::cmp::Ordering::Equal)
        })
        .map(|(i, hull)| (i, midpoint(hull.mins, hull.maxs)));
    match largest {
        Some((i, c)) => ((c.x, c.y), Horizontal::LargestPiece(i)),
        // A volume with no pieces cannot happen — the compiler drops such a
        // trigger — but the bounds centre is still the honest answer if it does.
        None => ((centre.x, centre.y), Horizontal::BoundsCentre),
    }
}

/// How high to cross it: where a player standing inside would be.
///
/// The trace starts with the hull's top flush against the volume's ceiling and,
/// failing that, with its feet on the volume's floor. Both are positions
/// expressed in the volume's own geometry and the player's own size; neither is
/// a number about a map. It ends at the map's own floor, so a volume hanging
/// over a pit finds the pit's bottom and is then rejected by the overlap test
/// rather than by a made-up depth limit.
fn vertical_aim(
    volume: &TriggerVolume,
    (x, y): (Scalar, Scalar),
    map: &CompiledMap,
    world: &impl World,
    profile: &PhysicsProfile,
) -> (Vec3, Vertical) {
    let half = profile.hull_half_extents();
    let offset = profile.hull_center_offset();
    let (mins, maxs) = (volume.bounds.mins, volume.bounds.maxs);
    let from = [
        // Hull top flush with the volume's ceiling.
        maxs.z - offset.z - half.z,
        // Hull bottom on the volume's floor, held off it the way a spawn is.
        mins.z - offset.z + half.z + SPAWN_CLEARANCE,
    ];
    let floor = floor_of(map, profile);
    for start_z in from {
        let Some(contact) = drop_to_ground(world, profile, vec3(x, y, start_z), floor) else {
            continue;
        };
        let origin = stood_up(contact);
        if hull_overlaps(volume, origin, profile) {
            return (origin, Vertical::Standing(surface_under(profile, contact)));
        }
    }
    let centre = midpoint(mins, maxs);
    (vec3(x, y, centre.z) - offset, Vertical::VolumeCentre)
}

// ---------------------------------------------------------------------------
// Geometry helpers. Everything here is in terms of the player's size and the
// map's own extent; no constant below describes a particular map.

/// How far down a trace may look before the world has run out.
///
/// The map's own low bound, less one player height so a surface resting exactly
/// on it is still hit rather than missed by the end of the sweep.
fn floor_of(map: &CompiledMap, profile: &PhysicsProfile) -> Scalar {
    map.bounds.mins.z - profile.hull_half_extents().z * s(2.0)
}

/// Drop a player hull straight down from `from` and return the origin at which
/// it touches ground, or `None` if it starts inside solid or never lands.
///
/// The contact origin, not a standing one: the caller decides whether to hold
/// the player clear of the surface, because the surface height is read off this
/// and adding the clearance first would report a floor an eighth of a unit
/// above where it is.
fn drop_to_ground(
    world: &impl World,
    profile: &PhysicsProfile,
    from: Vec3,
    to_z: Scalar,
) -> Option<Vec3> {
    if to_z >= from.z {
        return None;
    }
    let end = vec3(from.x, from.y, to_z);
    let trace = world.trace(&Sweep {
        start: from,
        end,
        half_extents: profile.hull_half_extents(),
        center_offset: profile.hull_center_offset(),
    });
    if trace.start_solid || trace.fraction >= s(1.0) {
        return None;
    }
    Some(from + (end - from) * trace.fraction)
}

/// A player origin held clear of the surface it rests on, the same eighth of a
/// unit `straf3-map` holds a spawn off the floor and for the same reason:
/// resting exactly on a brush counts as being inside it.
fn stood_up(contact: Vec3) -> Vec3 {
    vec3(contact.x, contact.y, contact.z + SPAWN_CLEARANCE)
}

/// The surface height under a player origin — the underside of the hull.
fn surface_under(profile: &PhysicsProfile, origin: Vec3) -> Scalar {
    origin.z + profile.hull_center_offset().z - profile.hull_half_extents().z
}

/// Whether a player hull whose *origin* is `origin` overlaps the volume.
///
/// `TriggerVolume::intersects_box` takes the box's centre, and a player origin
/// is not its hull's centre — Quake's hull sits four units low. Converting here
/// rather than at each call is what keeps that off-by-four from being
/// rediscovered.
fn hull_overlaps(volume: &TriggerVolume, origin: Vec3, profile: &PhysicsProfile) -> bool {
    volume.intersects_box(
        origin + profile.hull_center_offset(),
        profile.hull_half_extents(),
    )
}

fn midpoint(a: Vec3, b: Vec3) -> Vec3 {
    (a + b) * s(0.5)
}

fn box_volume(mins: Vec3, maxs: Vec3) -> Scalar {
    let d = maxs - mins;
    d.x * d.y * d.z
}

// ---------------------------------------------------------------------------
// The legs

fn legs(spawn: &SpawnCheck, waypoints: &[Waypoint]) -> Vec<Leg> {
    let mut points: Vec<(String, Vec3)> = vec![("spawn".to_owned(), spawn.origin)];
    for w in waypoints {
        points.push((w.step.to_string(), w.primary().aim));
    }

    let mut out: Vec<Leg> = Vec::new();
    for pair in points.windows(2) {
        let ((from_name, from), (to_name, to)) = (&pair[0], &pair[1]);
        let d = *to - *from;
        let bearing_deg = d.y.atan2(d.x).to_degrees();
        let turn_deg = out
            .last()
            .map(|prev| wrap180(bearing_deg - prev.bearing_deg));
        out.push(Leg {
            from: from_name.clone(),
            to: to_name.clone(),
            distance: d.length(),
            ground_distance: vec3(d.x, d.y, s(0.0)).length(),
            rise: d.z,
            bearing_deg,
            turn_deg,
        });
    }
    out
}

/// Wrap an angle difference into ±180°, so a leg that turns 10° right of north
/// reads as -10 and not as 350.
fn wrap180(mut degrees: Scalar) -> Scalar {
    while degrees > s(180.0) {
        degrees -= s(360.0);
    }
    while degrees < s(-180.0) {
        degrees += s(360.0);
    }
    degrees
}

#[cfg(test)]
mod tests;
