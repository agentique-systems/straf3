//! Which world the session collides against, and where the player starts.
//!
//! # The one thing this module exists to guarantee
//!
//! The thing the player sees and the thing the player collides with are ONE
//! piece of data. Two copies drift the moment either is edited, and the failure
//! mode is the worst kind to debug: an invisible wall, or a ramp you fall
//! through.
//!
//! Wave 3 held that line by declaring the arena once in `straf3-render` and
//! reading it twice. Wave 4 holds it better, and one layer further down: a
//! [`CompiledMap`] comes out of `straf3-map` with its collision hulls and its
//! render triangles produced by a single pass over a single set of brush
//! planes. There is no second parse that could disagree, and the geometry is
//! now *data on disk* rather than Rust — so the property survives someone
//! editing the map, which the hardcoded version never really did.
//!
//! This module is the only place the game names a geometry source. If the map
//! pipeline's shape moves, exactly one file has to follow it.
//!
//! # Why the map is a process-lifetime singleton
//!
//! [`straf3_sim::World`] is handed to the simulation as `&'static dyn World`,
//! and that is not an accident of this file: it is what lets `Game<W>` hold the
//! world without a lifetime parameter infecting the event loop, the recorder
//! and the replay path. A map is loaded once at startup and lives until the
//! process exits, so `'static` is the honest lifetime for it rather than a
//! convenience — [`install`] puts it in a [`OnceLock`] instead of leaking a
//! `Box`, which gets the same lifetime with none of the "did we mean to do
//! that" that `Box::leak` always invites.
//!
//! Consequently a map cannot be swapped at runtime. When map switching is
//! wanted, this is the file that changes, and it is the *only* one.
//!
//! # Why `flat` and `empty` are still selectable
//!
//! `straf3-headless` knows those two worlds and nothing else. Running the
//! windowed build in the flat world is what makes an end-to-end criterion-4
//! comparison possible between two separate binaries — record here, replay
//! there, compare checksums. Neither takes a map, so neither can be broken by
//! one.

use std::sync::OnceLock;

// `HullWorld` through `straf3-map` rather than through `straf3-collision`
// directly: the map is where this crate's geometry comes from, and reaching
// past it for the collider type would be a second opinion about which crate
// owns that answer.
use straf3_map::{CompileError, CompiledMap, HullWorld, Warning};
use straf3_replay::WorldId;
use straf3_sim::World;
use straf3_sim::num::{Scalar, Vec3, s, vec3};
use straf3_sim::world::{EmptyWorld, FlatGround};

use crate::record::WorldSpec;

/// The infinite plane, as a static so it can be handed out as `&'static dyn`.
static FLAT_GROUND: FlatGround = FlatGround::at(s(0.0));
static EMPTY: EmptyWorld = EmptyWorld;

/// Spawn origin in the flat and empty worlds: stood on the plane.
///
/// Q3's player origin sits 24 units above the hull's underside, so a player
/// resting on a floor at z=0 has an origin at z=24.
const FLAT_SPAWN: Vec3 = vec3(s(0.0), s(0.0), s(24.0));
/// Looking along +Y.
const FLAT_SPAWN_YAW: Scalar = s(90.0);

/// The map this process plays in, and the collider built from it.
///
/// Both, together, because [`CompiledMap::collider`] returns an owned
/// [`HullWorld`] by design — it clones the hull data so the collider can
/// outlive the source text. Building it once here and keeping it beside the map
/// is what turns that owned value into the `&'static dyn World` the simulation
/// wants, without rebuilding it per frame or per replay.
pub struct LoadedMap {
    /// What to call this map — the `.map` file's stem, normally. Carried
    /// because a recording's [`WorldId`] names the world it was made in, and a
    /// personal best is filed under it.
    pub name: String,
    /// Everything the compiler produced: hulls, mesh, triggers, entities.
    pub map: CompiledMap,
    /// The same hulls, as something [`straf3_sim::step_in_place`] can sweep.
    pub collider: HullWorld,
}

static MAP: OnceLock<LoadedMap> = OnceLock::new();

/// Compile `source` and install it as this process's map.
///
/// Takes text, not a path: reading bytes is the caller's job, above the seam,
/// and `straf3-map`'s own API is text-in for the same reason — it is what lets
/// the browser fetch a `.map` over HTTP and compile it through this identical
/// code path.
///
/// # Errors
///
/// Whatever [`straf3_map::compile`] refused the source for.
///
/// # Panics
///
/// Never. A second call is ignored and reports the map already installed —
/// [`WorldChoice`] cannot express two maps, so a second one would be a caller
/// bug rather than a recoverable condition, and silently keeping the first is
/// the behaviour that cannot produce a world/mesh mismatch.
pub fn install(name: &str, source: &str) -> Result<&'static LoadedMap, CompileError> {
    if let Some(existing) = MAP.get() {
        log::warn!("a map is already installed; keeping it and ignoring the second");
        return Ok(existing);
    }
    let map = straf3_map::compile(source)?;

    // Warnings are data, not log lines, below the seam — so this is where they
    // become visible. They are the difference between "the map loaded" and "the
    // map loaded and here is what the compiler had to decide for you", and the
    // patch count in particular decides whether a route has holes in it.
    report(&map);

    let collider = map.collider();
    let name = name.to_owned();
    Ok(MAP.get_or_init(|| LoadedMap {
        name,
        map,
        collider,
    }))
}

/// Say out loud what the compiler decided on the author's behalf.
fn report(map: &CompiledMap) {
    log::info!(
        "map: {} hulls, {} triggers, {} triangles, collision digest {:#018x}",
        map.hulls.len(),
        map.triggers.len(),
        map.mesh.triangle_count(),
        map.collision_digest(),
    );
    for warning in &map.warnings {
        match warning {
            // Called out separately because it is the only warning that can
            // make a map unplayable rather than imperfect: a dropped patch is
            // missing *collision*, so a route over a curved surface has a hole
            // in it exactly where the surface was. Measured against real
            // third-party maps, the counts run to four figures.
            Warning::PatchDropped { count, .. } => log::warn!(
                "map: {count} curved surfaces dropped — every one of them is \
                 missing collision, not just missing from the picture"
            ),
            Warning::NoTimerTriggers => log::warn!(
                "map: no target_startTimer/target_stopTimer pair, so this map \
                 cannot produce a time"
            ),
            other => log::warn!("map: {other:?}"),
        }
    }
}

/// The installed map, if [`install`] has been called and succeeded.
#[must_use]
pub fn loaded() -> Option<&'static LoadedMap> {
    MAP.get()
}

/// Which world to play in.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum WorldChoice {
    /// The compiled map installed by [`install`]. The default, and the only
    /// world with geometry, ramps or a finish line in it.
    #[default]
    Map,
    /// An infinite plane at z=0. Reproducible by `straf3-headless`.
    Flat,
    /// No geometry at all. Reproducible by `straf3-headless`.
    Empty,
}

impl WorldChoice {
    /// Parse a command-line spelling.
    ///
    /// `arena` is deliberately *not* accepted any more. The hardcoded arena is
    /// gone, and accepting the name would silently run a recording made in it
    /// against different geometry and hand back a different answer with a
    /// straight face. Refusing by name is the same choice `straf3-headless`
    /// already makes about worlds it does not know.
    #[must_use]
    pub fn parse(name: &str) -> Option<Self> {
        match name {
            "map" => Some(Self::Map),
            "flat" => Some(Self::Flat),
            "empty" => Some(Self::Empty),
            _ => None,
        }
    }

    /// Whether this build, and this process, can actually provide this world.
    #[must_use]
    pub fn is_available(self) -> bool {
        match self {
            Self::Map => loaded().is_some(),
            Self::Flat | Self::Empty => true,
        }
    }

    /// This choice if it can be provided, otherwise the flat world.
    ///
    /// A build with no map installed — because none was given, or because the
    /// one given did not compile — still opens a window you can move in. That
    /// is what keeps the input path and the frame pacing testable on a machine
    /// that cannot load a map at all, which was the same reason the arena had a
    /// fallback before it.
    #[must_use]
    pub fn or_fallback(self) -> Self {
        if self.is_available() {
            self
        } else {
            log::warn!("no map is installed — falling back to the flat world");
            Self::Flat
        }
    }

    /// The collision geometry.
    ///
    /// The same hulls the mesh in [`LoadedMap::map`] was built from, which is
    /// the whole point of this module.
    #[must_use]
    pub fn world(self) -> &'static dyn World {
        match self {
            Self::Map => match loaded() {
                Some(loaded) => &loaded.collider,
                // Unreachable through `or_fallback`, which every caller goes
                // through. Answering with the flat plane rather than panicking
                // keeps a mis-sequenced caller playable instead of dead.
                None => &FLAT_GROUND,
            },
            Self::Flat => &FLAT_GROUND,
            Self::Empty => &EMPTY,
        }
    }

    /// Where the player starts, and which way they face.
    #[must_use]
    pub fn spawn(self) -> (Vec3, Scalar) {
        match self {
            Self::Map => match loaded() {
                Some(loaded) => (loaded.map.spawn, loaded.map.spawn_yaw),
                None => (FLAT_SPAWN, FLAT_SPAWN_YAW),
            },
            Self::Flat | Self::Empty => (FLAT_SPAWN, FLAT_SPAWN_YAW),
        }
    }

    /// What a `.s3d` recording binds itself to when it is made in this world.
    ///
    /// This is contract item C6's end of the deal, and the single place the
    /// obligation `straf3_replay::Recording::record` cannot check for itself is
    /// discharged: the digest handed over is the one belonging to the very
    /// hulls [`Self::world`] returns, taken from the same [`LoadedMap`]. A
    /// recompiled map has a different digest, so a personal best set on the old
    /// geometry is refused rather than raced.
    ///
    /// `None` for [`Self::Map`] with no map installed — the same
    /// mis-sequenced-caller case [`Self::world`] answers with the flat plane,
    /// except that here there is no honest answer to give, and inventing one
    /// would bind a recording to a world nobody played in.
    #[must_use]
    pub fn world_id(self) -> Option<WorldId> {
        match self {
            Self::Map => loaded().map(|l| WorldId::map(&l.name, l.map.collision_digest())),
            Self::Flat => Some(WorldId::flat(s(0.0))),
            Self::Empty => Some(WorldId::Empty),
        }
    }

    /// What to call this world in a file name.
    #[must_use]
    pub fn name(self) -> &'static str {
        match self {
            Self::Map => loaded().map_or("map", |l| l.name.as_str()),
            Self::Flat => "flat",
            Self::Empty => "empty",
        }
    }

    /// How a recording should describe this world.
    ///
    /// A map is `Unrepresentable` because `straf3-headless` has no spelling for
    /// one, exactly as the arena was. Binding a recording to the map's
    /// [`collision_digest`](CompiledMap::collision_digest) is
    /// [`Self::world_id`]'s job and lives in the `.s3d` format, not in this
    /// enum.
    #[must_use]
    pub const fn spec(self) -> WorldSpec {
        match self {
            Self::Map => WorldSpec::Unrepresentable("map"),
            Self::Flat => WorldSpec::Flat(s(0.0)),
            Self::Empty => WorldSpec::Empty,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use straf3_sim::world::Sweep;

    /// The course this repository ships, compiled in the test rather than
    /// referenced by path: `include_str!` is resolved by the compiler, so the
    /// test cannot pass against a map that is not actually committed.
    const COIL: &str = include_str!("../../../assets/maps/coil.map");

    #[test]
    fn the_command_line_spellings_round_trip() {
        assert_eq!(WorldChoice::parse("map"), Some(WorldChoice::Map));
        assert_eq!(WorldChoice::parse("flat"), Some(WorldChoice::Flat));
        assert_eq!(WorldChoice::parse("empty"), Some(WorldChoice::Empty));
        assert_eq!(WorldChoice::parse("Map"), None);
        assert_eq!(WorldChoice::parse(""), None);
    }

    #[test]
    fn the_retired_arena_is_refused_by_name_not_silently_substituted() {
        // The failure this guards: a recording made in the arena replaying
        // against the course and reporting a time for a run that never
        // happened.
        assert_eq!(WorldChoice::parse("arena"), None);
    }

    #[test]
    fn the_flat_spawn_is_stood_on_the_floor_not_buried_in_it() {
        let (spawn, _) = WorldChoice::Flat.spawn();
        // Q3's hull reaches 24 units below the origin, so anything less than
        // 24 spawns the player inside the plane and every trace reports
        // start_solid.
        assert_eq!(spawn.z, s(24.0));
    }

    #[test]
    fn the_worlds_that_need_no_map_answer_traces_with_the_player_in_the_open() {
        for choice in [WorldChoice::Flat, WorldChoice::Empty] {
            let (spawn, _) = choice.spawn();
            let trace = choice.world().trace(&Sweep {
                start: spawn,
                end: spawn,
                half_extents: vec3(s(15.0), s(15.0), s(28.0)),
                center_offset: vec3(s(0.0), s(0.0), s(4.0)),
            });
            assert!(
                !trace.start_solid,
                "{choice:?} spawns the player inside geometry"
            );
        }
    }

    #[test]
    fn a_process_with_no_map_falls_back_rather_than_failing() {
        // `MAP` is process-wide and these tests share a process, so this asserts
        // the invariant that actually holds either way: `Map` is available
        // exactly when a map is loaded, and falls back to `Flat` when not.
        if loaded().is_some() {
            assert_eq!(WorldChoice::Map.or_fallback(), WorldChoice::Map);
        } else {
            assert_eq!(WorldChoice::Map.or_fallback(), WorldChoice::Flat);
        }
    }

    #[test]
    fn the_shipped_course_compiles_and_is_a_timed_run() {
        // Not `install()`: that is a process-wide singleton and would make this
        // test's result depend on which other test ran first. Compiling
        // directly asserts the same thing about the same bytes.
        let map = straf3_map::compile(COIL).expect("assets/maps/coil.map must compile");
        assert!(
            map.has_timing(),
            "the shipped course must have both a start and a finish, or it \
             cannot produce a time"
        );
        assert_eq!(map.triggers_of(straf3_map::TriggerKind::Start).count(), 1);
        assert_eq!(map.triggers_of(straf3_map::TriggerKind::Finish).count(), 1);
        assert!(!map.hulls.is_empty());
        assert!(map.mesh.triangle_count() > 0);
    }

    #[test]
    fn the_shipped_course_has_no_curved_surface_the_compiler_had_to_drop() {
        // Every dropped patch is missing *collision*. On our own map that is
        // never acceptable, and unlike a third-party map it is entirely in our
        // control — so it is an assertion here rather than a warning.
        let map = straf3_map::compile(COIL).unwrap();
        for warning in &map.warnings {
            assert!(
                !matches!(warning, Warning::PatchDropped { .. }),
                "the shipped course must not depend on curved surfaces: {warning:?}"
            );
        }
    }

    #[test]
    fn the_shipped_course_spawns_the_player_in_open_air() {
        // The single failure that makes a map unplayable on the first frame,
        // and the cheapest one to catch: a spawn inside solid reports
        // start_solid on every trace and the mover never recovers.
        let map = straf3_map::compile(COIL).unwrap();
        let world = map.collider();
        let trace = world.trace(&Sweep {
            start: map.spawn,
            end: map.spawn,
            half_extents: vec3(s(15.0), s(15.0), s(28.0)),
            center_offset: vec3(s(0.0), s(0.0), s(4.0)),
        });
        assert!(
            !trace.start_solid,
            "coil.map spawns the player inside solid"
        );

        // And there must be a floor under them rather than a void.
        let down = world.trace(&Sweep {
            start: map.spawn,
            end: map.spawn - vec3(s(0.0), s(0.0), s(4096.0)),
            half_extents: vec3(s(15.0), s(15.0), s(28.0)),
            center_offset: vec3(s(0.0), s(0.0), s(4.0)),
        });
        assert!(down.fraction < s(1.0), "coil.map's spawn is over a void");
        assert!(
            down.normal.z > s(0.7),
            "coil.map's spawn is over ground too steep to stand on: {:?}",
            down.normal
        );
    }

    #[test]
    fn compiling_the_shipped_course_twice_gives_the_same_geometry() {
        // The property a recorded time is bound to. Not the cross-target check
        // — that is C2's — but the one that would break first and locally.
        let a = straf3_map::compile(COIL).unwrap();
        let b = straf3_map::compile(COIL).unwrap();
        assert_eq!(a.collision_digest(), b.collision_digest());
        assert_eq!(a.full_digest(), b.full_digest());
    }
}
