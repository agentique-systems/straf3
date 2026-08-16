//! Entities: the keys that turn geometry into a course.
//!
//! # The Defrag conventions, and why they are read here
//!
//! ARCHITECTURE C7 requirement 5 states them: a run starts and stops at
//! `target_startTimer` / `target_stopTimer` entities, which are *point*
//! entities carrying a `targetname`, wired to `trigger_multiple` **brush**
//! entities whose `target` names them. The player spawns at
//! `info_player_deathmatch` or `info_player_start`.
//!
//! Those two are what a Defrag map carries, and they are preferred, but they
//! are not all a map may carry: a team or CTF map often declares only
//! `info_player_team1` / `info_player_team2` or `team_CTF_redplayer` /
//! `team_CTF_blueplayer`, and a compiler that reads only the first two rejects
//! it as having nowhere to stand. All six are read, in the preference order
//! [`SPAWN_CLASSNAMES`] fixes.
//!
//! The indirection is the part worth understanding, because it is why a trigger
//! cannot be classified by looking at it: the brush that the player crosses
//! says only `"target" "t1"`, and what `t1` *is* lives in a different entity
//! elsewhere in the file. Resolving that link is this module's job, and it is
//! why the compiler reads the whole entity list before it decides what any
//! single trigger means.
//!
//! C7 puts this mapping in `straf3-map` rather than in the web layer, so the
//! browser and the native client cannot disagree about where a run starts.

use glam::Vec3;
use straf3_collision::{Hull, trace_hull};
use straf3_sim::world::{Sweep, Trace};

use crate::bounds::Aabb;
use crate::digest::{Fnv1a, fold_hull};

/// One entity's key/value pairs, as written in the `.map` source.
///
/// Kept as a `Vec` and not a map: `.map` files may repeat a key, iteration
/// order is part of what the digest folds, and a `HashMap` would make the
/// compiler's output depend on hash iteration order — which is exactly what C7
/// requirement 3 forbids.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct MapEntity {
    /// The `classname` value, lowercased. Empty when the entity has none.
    ///
    /// Lowercased because Radiant, Defrag's own docs and hand edits disagree
    /// about the casing of `target_startTimer`, and nothing is gained by
    /// treating two spellings of one classname as two classes.
    pub classname: String,
    /// Every key/value pair, in source order, with keys lowercased and values
    /// left exactly as written.
    pub keys: Vec<(String, String)>,
}

impl MapEntity {
    /// The first value for `key`, if any.
    #[must_use]
    pub fn get(&self, key: &str) -> Option<&str> {
        self.keys
            .iter()
            .find(|(k, _)| k == key)
            .map(|(_, v)| v.as_str())
    }

    /// The `origin` key as a vector.
    ///
    /// `"128 -256 24"`. Parsed as `f32` directly: Rust's float parser is
    /// correctly rounded and target-independent, so this is one of the places
    /// cross-target identity is free rather than earned.
    #[must_use]
    pub fn origin(&self) -> Option<Vec3> {
        parse_vec3(self.get("origin")?)
    }

    /// Which way the entity faces, in degrees, Quake convention (0 is +X, and
    /// yaw increases anticlockwise).
    ///
    /// `angle` is a bare yaw; `angles` is `pitch yaw roll` and its middle field
    /// is the same number. Maps use both, sometimes in the same file.
    #[must_use]
    pub fn yaw(&self) -> Option<f32> {
        if let Some(angles) = self.get("angles") {
            let mut parts = angles.split_ascii_whitespace();
            let (_pitch, yaw) = (parts.next(), parts.next()?);
            return yaw.parse::<f32>().ok();
        }
        self.get("angle")?.parse::<f32>().ok()
    }

    pub(crate) fn fold(&self, h: &mut Fnv1a) {
        h.bytes(self.classname.as_bytes());
        h.len(self.keys.len());
        for (k, v) in &self.keys {
            h.bytes(k.as_bytes());
            h.bytes(v.as_bytes());
        }
    }
}

/// Parse `"x y z"`, tolerating any run of ASCII whitespace between fields.
fn parse_vec3(text: &str) -> Option<Vec3> {
    let mut parts = text.split_ascii_whitespace();
    let x = parts.next()?.parse::<f32>().ok()?;
    let y = parts.next()?.parse::<f32>().ok()?;
    let z = parts.next()?.parse::<f32>().ok()?;
    Some(Vec3::new(x, y, z))
}

/// What crossing a trigger volume means to a run.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum TriggerKind {
    /// `target_startTimer`: the clock starts.
    Start,
    /// `target_stopTimer`: the clock stops and the run is a time.
    Finish,
    /// `target_checkpoint`, numbered by order of appearance in the source.
    ///
    /// Defrag does not give checkpoints an explicit index, so source order is
    /// the only stable numbering available — and it is stable, because it comes
    /// from the file rather than from anything computed.
    Checkpoint(u32),
    /// `trigger_teleport`. Recorded, not implemented: the volume is here so the
    /// run-clock and movement tracks have the data, and nothing in this crate
    /// moves a player.
    Teleport,
    /// `trigger_push`, the jump pad. Recorded, not implemented, as above.
    Push,
    /// A trigger this compiler has no meaning for — a hurt volume, a map script
    /// hook. Kept rather than dropped, so the geometry is not silently lost.
    Other,
}

impl TriggerKind {
    /// A stable tag for the digest. Explicit numbers, not `as u32` on the
    /// variant order: reordering the enum must not change a map's identity.
    const fn tag(self) -> u32 {
        match self {
            Self::Start => 1,
            Self::Finish => 2,
            Self::Checkpoint(_) => 3,
            Self::Teleport => 4,
            Self::Push => 5,
            Self::Other => 6,
        }
    }
}

/// A trigger: a brush entity's volume plus what crossing it means.
///
/// The volume is a **set** of convex hulls, because one `trigger_multiple`
/// entity may own several brushes, and a start line spanning an L-shaped
/// corridor really is two boxes. Inside any one of them is inside the volume.
#[derive(Debug, Clone, PartialEq)]
pub struct TriggerVolume {
    /// What this volume does to a run.
    pub kind: TriggerKind,
    /// The brush entity's own classname, e.g. `trigger_multiple`.
    pub classname: String,
    /// The `target` key: the name of the point entity that gives it meaning.
    pub target: Option<String>,
    /// The classname of the entity `target` resolved to, when it resolved.
    pub target_classname: Option<String>,
    /// The convex pieces of the volume, in source brush order.
    pub hulls: Vec<Hull>,
    /// Bounds of every piece together, for a cheap early-out.
    pub bounds: Aabb,
}

impl TriggerVolume {
    /// Whether a point is inside any piece of the volume.
    ///
    /// A zero-extent sweep that goes nowhere: `start_solid` is exactly the
    /// question being asked.
    #[must_use]
    pub fn contains_point(&self, point: Vec3) -> bool {
        self.overlaps(point, Vec3::ZERO)
    }

    /// Whether a box centred at `center` overlaps any piece.
    ///
    /// This is the query the run clock needs, and it deliberately goes through
    /// [`trace_hull`] rather than testing the planes here. A player crosses a
    /// start line as a 30×30×56 box, and *where that box is* is decided by the
    /// swept-box plane expansion and by the bevel planes `compile_hull` added.
    /// A hand-written point-in-hull test would answer a slightly different
    /// question than the trace does, and "the clock started a hundredth of a
    /// unit before the wall stopped you" is the kind of disagreement that shows
    /// up as an unreproducible time.
    #[must_use]
    pub fn intersects_box(&self, center: Vec3, half_extents: Vec3) -> bool {
        self.overlaps(center, half_extents)
    }

    fn overlaps(&self, center: Vec3, half_extents: Vec3) -> bool {
        let sweep = Sweep {
            start: center,
            end: center,
            half_extents,
            // The caller gives a centre, so the box is centred on it. Callers
            // holding a player *origin* pass the profile's own offset instead.
            center_offset: Vec3::ZERO,
        };
        self.hulls.iter().any(|hull| {
            let mut trace = Trace::clear();
            trace_hull(hull, &sweep, &mut trace);
            trace.start_solid
        })
    }

    pub(crate) fn fold(&self, h: &mut Fnv1a) {
        h.u32(self.kind.tag());
        if let TriggerKind::Checkpoint(index) = self.kind {
            h.u32(index);
        }
        h.bytes(self.classname.as_bytes());
        h.len(self.hulls.len());
        for hull in &self.hulls {
            fold_hull(hull, h);
        }
        self.bounds.fold(h);
    }
}

/// Where the player starts, and which way they look.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Spawn {
    /// The player's **origin**, not the entity's — see [`SPAWN_CLEARANCE`].
    pub origin: Vec3,
    /// Yaw in degrees, Quake convention. Zero when the entity gave none.
    pub yaw: f32,
    /// The classname it came from, so a map with both kinds is diagnosable.
    pub classname: &'static str,
}

/// How far above the `.map` entity the player's origin is placed.
///
/// An eighth of a unit, and it is the same eighth `arena.rs` explains at
/// length: Radiant places a spawn entity so the player's hull rests exactly on
/// the floor, and *exactly on* counts as inside a brush, so the very first
/// trace of the run would report `start_solid`. Q3 solved this by adding 9
/// units and letting the player drop; Straf3 adds `SURFACE_CLIP_EPSILON`, the
/// same distance the mover holds the player off a surface after landing, so the
/// player starts already at rest instead of falling. The difference is
/// invisible to a time — the clock starts at a trigger, not at the spawn — and
/// it makes the first frame of every run identical to the frame after it.
pub const SPAWN_CLEARANCE: f32 = 0.125;

/// Spawn classnames, in the order they are preferred.
///
/// `info_player_deathmatch` first because that is what Defrag maps use and what
/// Q3's own FFA spawn selection reads; `info_player_start` is the single-player
/// entity, present in many maps as a leftover at the same spot.
///
/// The four team entries come last, and the order matters more than it looks.
/// A team map often carries no FFA spawn at all — `catharsis.map` has six
/// `info_player_team1` and six `info_player_team2` and nothing else, and was
/// rejected outright with [`crate::CompileError::NoPlayerSpawn`] until they were
/// read here. Appending rather than interleaving keeps every map that *does*
/// have an FFA spawn choosing exactly the spawn it chose before, which is what
/// makes this change invisible to an existing recording: [`CompiledMap::spawn`]
/// is `spawns[0]`, and `spawns[0]` only moves for a map that previously had no
/// spawns to order.
///
/// `info_player_intermission` is deliberately absent. It is the end-of-match
/// camera position, and mappers put it inside the ceiling or out beyond the
/// walls where a camera can see the level — spawning a player there puts them
/// in solid.
///
/// [`CompiledMap::spawn`]: crate::CompiledMap::spawn
const SPAWN_CLASSNAMES: [&str; 6] = [
    "info_player_deathmatch",
    "info_player_start",
    "info_player_team1",
    "info_player_team2",
    "team_ctf_redplayer",
    "team_ctf_blueplayer",
];

/// Every spawn point in the map, best first.
///
/// Preference is by classname, then by source order within a classname. Both
/// halves are needed: source order alone would pick a leftover
/// `info_player_start`, and classname alone would leave a map with eight
/// deathmatch spawns undecided.
pub(crate) fn spawns(entities: &[MapEntity]) -> Vec<Spawn> {
    let mut out = Vec::new();
    for wanted in SPAWN_CLASSNAMES {
        for entity in entities {
            if entity.classname != wanted {
                continue;
            }
            let Some(origin) = entity.origin() else {
                continue;
            };
            out.push(Spawn {
                origin: origin + Vec3::new(0.0, 0.0, SPAWN_CLEARANCE),
                yaw: entity.yaw().unwrap_or(0.0),
                classname: wanted,
            });
        }
    }
    out
}

/// What a trigger's `target` classname means.
///
/// Defrag's timer entities, plus the two `trigger_*` classnames that move a
/// player and therefore matter to a route even though this crate does not
/// implement them.
fn kind_for(trigger_classname: &str, target_classname: Option<&str>) -> TriggerKind {
    match target_classname {
        Some("target_starttimer") => return TriggerKind::Start,
        Some("target_stoptimer") => return TriggerKind::Finish,
        // Checkpoint numbering is assigned by the caller, which is the only
        // place source order is known.
        Some("target_checkpoint") => return TriggerKind::Checkpoint(u32::MAX),
        _ => {}
    }
    match trigger_classname {
        "trigger_teleport" => TriggerKind::Teleport,
        "trigger_push" => TriggerKind::Push,
        _ => TriggerKind::Other,
    }
}

/// Classify one brush entity's trigger, resolving its `target` through the
/// whole entity list.
///
/// `next_checkpoint` is the number the next checkpoint gets, and is advanced
/// when one is found — which is what makes checkpoint numbering follow source
/// order rather than anything computed.
pub(crate) fn classify_trigger(
    entity: &MapEntity,
    all: &[MapEntity],
    next_checkpoint: &mut u32,
) -> (TriggerKind, Option<String>, Option<String>) {
    let target = entity.get("target").map(str::to_string);
    let target_classname = target.as_deref().and_then(|name| {
        all.iter()
            .find(|e| {
                e.get("targetname")
                    .is_some_and(|t| t.eq_ignore_ascii_case(name))
            })
            .map(|e| e.classname.clone())
    });

    let mut kind = kind_for(&entity.classname, target_classname.as_deref());
    if matches!(kind, TriggerKind::Checkpoint(_)) {
        kind = TriggerKind::Checkpoint(*next_checkpoint);
        *next_checkpoint += 1;
    }
    (kind, target, target_classname)
}

/// Whether a classname names a brush entity whose brushes are trigger volumes
/// rather than solid geometry.
pub(crate) fn is_trigger_classname(classname: &str) -> bool {
    classname.starts_with("trigger_")
}

/// What a non-trigger brush entity's brushes are, to this compiler.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum BrushEntityRole {
    /// World geometry that was never going to move anyway.
    Static,
    /// Geometry that moves in Quake 3 and does not here.
    Mover,
    /// A classname with no rule. Compiled as solid, and reported.
    Unknown,
}

/// Classify a brush entity's classname.
///
/// `func_group` is Radiant's grouping entity: the map compiler folds its
/// brushes straight into the world, and so does this. `func_static` is what its
/// name says. Everything else in the `func_` family is a mover — a door, a
/// plat, a rotating hazard — and the [`World`] contract says outright that the
/// world is static for the duration of a run, so those are compiled as solid
/// geometry frozen where the mapper left them, with
/// [`crate::Warning::MoverFrozen`] naming each one.
///
/// [`World`]: straf3_sim::world::World
pub(crate) fn world_brush_role(classname: &str) -> BrushEntityRole {
    match classname {
        "worldspawn" | "func_group" | "func_static" | "func_detail" => BrushEntityRole::Static,
        _ if classname.starts_with("func_") => BrushEntityRole::Mover,
        _ => BrushEntityRole::Unknown,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn entity(classname: &str, keys: &[(&str, &str)]) -> MapEntity {
        MapEntity {
            classname: classname.to_string(),
            keys: keys
                .iter()
                .map(|(k, v)| ((*k).to_string(), (*v).to_string()))
                .collect(),
        }
    }

    #[test]
    fn origin_and_angle_are_read_the_way_maps_write_them() {
        let e = entity(
            "info_player_deathmatch",
            &[("origin", "128 -256 24"), ("angle", "90")],
        );
        assert_eq!(e.origin(), Some(Vec3::new(128.0, -256.0, 24.0)));
        assert_eq!(e.yaw(), Some(90.0));
    }

    #[test]
    fn the_angles_triple_yields_its_yaw() {
        let e = entity("info_player_start", &[("angles", "0 270 0")]);
        assert_eq!(e.yaw(), Some(270.0));
    }

    #[test]
    fn a_spawn_sits_an_eighth_of_a_unit_off_the_floor() {
        // Radiant puts the entity where the hull rests exactly on the floor.
        // Exactly on is inside, and the first trace of the run would say so.
        let e = entity("info_player_deathmatch", &[("origin", "0 -768 24")]);
        let s = spawns(std::slice::from_ref(&e));
        assert_eq!(s.len(), 1);
        assert_eq!(s[0].origin, Vec3::new(0.0, -768.0, 24.125));
        // The same number arena.rs's SPAWN uses, for the same reason.
        assert_eq!(s[0].origin.z, 24.0 + SPAWN_CLEARANCE);
    }

    #[test]
    fn deathmatch_spawns_are_preferred_and_source_order_breaks_the_tie() {
        let ents = vec![
            entity("info_player_start", &[("origin", "0 0 24")]),
            entity("info_player_deathmatch", &[("origin", "100 0 24")]),
            entity("info_player_deathmatch", &[("origin", "200 0 24")]),
        ];
        let s = spawns(&ents);
        assert_eq!(s.len(), 3);
        assert_eq!(s[0].origin.x, 100.0, "deathmatch wins over start");
        assert_eq!(s[1].origin.x, 200.0, "then source order");
        assert_eq!(s[2].classname, "info_player_start");
    }

    #[test]
    fn a_team_only_map_has_spawns() {
        // catharsis.map's shape: six team1, six team2, no FFA spawn at all.
        // Before these classnames were read this compiled to NoPlayerSpawn.
        let ents = vec![
            entity("info_player_team1", &[("origin", "0 0 24")]),
            entity("info_player_team2", &[("origin", "100 0 24")]),
            entity("team_ctf_redplayer", &[("origin", "200 0 24")]),
            entity("team_ctf_blueplayer", &[("origin", "300 0 24")]),
        ];
        let s = spawns(&ents);
        assert_eq!(s.len(), 4);
        assert_eq!(s[0].classname, "info_player_team1");
        assert_eq!(s[3].classname, "team_ctf_blueplayer");
    }

    #[test]
    fn a_team_spawn_never_outranks_an_ffa_one() {
        // The compatibility guarantee: `spawns[0]` — and therefore
        // `CompiledMap::spawn`, which is folded into `full_digest` — must not
        // move for any map that already had an FFA spawn, however many team
        // entities sit earlier in the file.
        let ents = vec![
            entity("info_player_team1", &[("origin", "0 0 24")]),
            entity("team_ctf_redplayer", &[("origin", "100 0 24")]),
            entity("info_player_deathmatch", &[("origin", "200 0 24")]),
            entity("info_player_start", &[("origin", "300 0 24")]),
        ];
        let s = spawns(&ents);
        assert_eq!(s[0].origin.x, 200.0, "deathmatch still wins");
        assert_eq!(s[1].origin.x, 300.0, "then start");
        assert_eq!(s[2].origin.x, 0.0, "team entities come after both");
    }

    #[test]
    fn an_intermission_camera_is_not_a_spawn() {
        // It is a viewpoint, and mappers put it in the ceiling or outside the
        // level. Reading it as a spawn puts the player in solid.
        let ents = vec![entity(
            "info_player_intermission",
            &[("origin", "0 0 4096")],
        )];
        assert!(spawns(&ents).is_empty());
    }

    #[test]
    fn a_spawn_without_an_origin_is_not_a_spawn() {
        let ents = vec![entity("info_player_deathmatch", &[("angle", "90")])];
        assert!(spawns(&ents).is_empty());
    }

    #[test]
    fn a_timer_trigger_is_classified_through_its_target() {
        let ents = vec![
            entity("trigger_multiple", &[("target", "t1")]),
            entity(
                "target_starttimer",
                &[("targetname", "t1"), ("origin", "0 0 0")],
            ),
        ];
        let mut next = 0;
        let (kind, target, target_classname) = classify_trigger(&ents[0], &ents, &mut next);
        assert_eq!(kind, TriggerKind::Start);
        assert_eq!(target.as_deref(), Some("t1"));
        assert_eq!(target_classname.as_deref(), Some("target_starttimer"));
    }

    #[test]
    fn a_trigger_whose_target_does_not_exist_is_still_a_volume() {
        let ents = vec![entity("trigger_multiple", &[("target", "missing")])];
        let mut next = 0;
        let (kind, _, target_classname) = classify_trigger(&ents[0], &ents, &mut next);
        assert_eq!(kind, TriggerKind::Other, "unresolved, not discarded");
        assert!(target_classname.is_none());
    }

    #[test]
    fn checkpoints_are_numbered_in_source_order() {
        let ents = vec![
            entity("trigger_multiple", &[("target", "c1")]),
            entity("trigger_multiple", &[("target", "c2")]),
            entity("target_checkpoint", &[("targetname", "c1")]),
            entity("target_checkpoint", &[("targetname", "c2")]),
        ];
        let mut next = 0;
        let first = classify_trigger(&ents[0], &ents, &mut next).0;
        let second = classify_trigger(&ents[1], &ents, &mut next).0;
        assert_eq!(first, TriggerKind::Checkpoint(0));
        assert_eq!(second, TriggerKind::Checkpoint(1));
    }

    #[test]
    fn a_teleporter_keeps_its_volume_even_though_nothing_implements_it() {
        let ents = vec![entity("trigger_teleport", &[("target", "dest")])];
        let mut next = 0;
        assert_eq!(
            classify_trigger(&ents[0], &ents, &mut next).0,
            TriggerKind::Teleport
        );
    }

    #[test]
    fn targetname_matching_ignores_case() {
        // Mappers are not consistent about this and Radiant does not enforce it.
        let ents = vec![
            entity("trigger_multiple", &[("target", "Timer_Start")]),
            entity("target_starttimer", &[("targetname", "timer_start")]),
        ];
        let mut next = 0;
        assert_eq!(
            classify_trigger(&ents[0], &ents, &mut next).0,
            TriggerKind::Start
        );
    }

    #[test]
    fn brush_entity_classes_are_told_apart() {
        assert!(is_trigger_classname("trigger_multiple"));
        assert!(is_trigger_classname("trigger_teleport"));
        assert!(!is_trigger_classname("target_starttimer"));

        assert_eq!(world_brush_role("worldspawn"), BrushEntityRole::Static);
        assert_eq!(world_brush_role("func_group"), BrushEntityRole::Static);
        assert_eq!(world_brush_role("func_door"), BrushEntityRole::Mover);
        assert_eq!(world_brush_role("misc_thing"), BrushEntityRole::Unknown);
    }

    #[test]
    fn the_kind_tags_are_fixed_numbers() {
        // Reordering the enum must not change a map's collision digest, which
        // is what would silently invalidate every recording made against it.
        assert_eq!(TriggerKind::Start.tag(), 1);
        assert_eq!(TriggerKind::Finish.tag(), 2);
        assert_eq!(TriggerKind::Checkpoint(7).tag(), 3);
        assert_eq!(TriggerKind::Other.tag(), 6);
    }
}
