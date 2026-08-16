//! Valve 220 `.map` loading and the compiled map format.
//!
//! Below the seam. Parsing is delegated to the `quake-map` crate; this crate's
//! own work is the **compile** step — turning brush planes into the convex
//! hulls collision wants and the mesh rendering wants. No BSP tree and no PVS:
//! Defrag maps are small and convex brush hulls are sufficient (spec D4).
//!
//! Note the signature: [`compile`] takes `&str`, not a path. Reading bytes off
//! disk is the caller's job, above the seam. That is what lets `straf3-sim`
//! consume map data while knowing nothing about files — and what lets the
//! browser fetch a `.map` over HTTP and compile it in wasm through the exact
//! code path the native client uses.
//!
//! # What comes out
//!
//! One pass over one set of brush planes produces all three of:
//!
//! - **[`CompiledMap::hulls`]** — convex solids, each an intersection of
//!   outward half-spaces, for the tracer to sweep against.
//!   [`CompiledMap::collider`] hands them over as a
//!   [`World`](straf3_sim::world::World).
//! - **[`CompiledMap::mesh`]** — the same brushes as triangles. Not a second
//!   copy of the geometry, and not even a second *computation* of it: the face
//!   polygons the renderer draws are the ones `compile_hull` clipped to find
//!   the collision hull's bounds. This is `arena.rs`'s "one declaration, two
//!   consumers" argument at map scale.
//! - **[`CompiledMap::triggers`]** and **[`CompiledMap::spawns`]** — the Defrag
//!   entity conventions read out (C7 requirement 5), which is what turns
//!   imported geometry into something a run can be timed on.
//!
//! # Where the line with `straf3-collision` falls
//!
//! All of the *geometry* is `straf3-collision`'s
//! [`straf3_collision::compile_hull`]: plane construction from a
//! face's three points, duplicate removal, winding clipping, bounds, and the
//! bevel planes a swept box needs. That is q3map's sequence and every step of it
//! is a decision about where a player may stand, so it lives in the crate that
//! traces the result, once.
//!
//! What is left here is everything that is genuinely about a `.map` *file*:
//! making Quake 3's dialects parseable, deciding what a shader name means,
//! reading the Defrag entity conventions, building the render mesh, and binding
//! the whole thing to a digest.
//!
//! # Determinism, which is the whole point
//!
//! C7 requirement 3 puts this crate in the same verification path as C1's
//! `sin_cos` fix: the same `.map` source must compile to byte-identical hulls
//! on native glibc, musl, Windows and wasm, or a record set on one is
//! unverifiable on another. Three rules keep that true, and each is enforced by
//! construction rather than by care:
//!
//! 1. **No transcendental functions.** Every operation from the three points of
//!    a face to the compiled plane is an IEEE add, subtract, multiply, divide,
//!    compare or square root, and all six are *exactly* specified — two
//!    conforming implementations cannot disagree about them by one bit. `sin`,
//!    `cos`, `exp` and `powf` are not specified that way, which is why glibc and
//!    wasm's libm differ and why C1 exists; none of them appear on this path.
//! 2. **No hash-map iteration and no parallelism.** Everything is a `Vec` in
//!    source order: entity order, then brush order, then face order. Order is
//!    not incidental — hulls are traced in index order and ties between
//!    coincident faces go to the first, so the order *is* part of the geometry.
//! 3. **Nothing width-dependent.** Every length folded into a digest goes in as
//!    a `u64`, because wasm is a 32-bit target and a native `usize` would give
//!    the browser a different answer for identical geometry.
//!
//! Verified rather than argued: the fixture course compiles to the same
//! `collision_digest` on `x86_64-unknown-linux-gnu`,
//! `x86_64-unknown-linux-musl`, `x86_64-pc-windows-gnu` and
//! `wasm32-unknown-unknown`.
//!
//! [`CompiledMap::collision_digest`] is what a recording binds itself to, so a
//! change to *this compiler* invalidates old runs even when the `.map` file has
//! not changed a byte.
//!
//! # What this compiler does not do
//!
//! Stated here rather than discovered later:
//!
//! - **Curved surfaces are parsed and skipped, not tessellated.** A Quake 3
//!   `patchDef2` / `patchDef3` block is read well enough to get past it and is
//!   then discarded; there is no Bézier tessellation here, so every patch is
//!   collidable geometry that is simply gone, and a route that runs over a
//!   curved ramp has a hole where the ramp was. Every one is counted in
//!   [`Warning::PatchDropped`], which also carries a [`PatchLoss`] severity,
//!   because the difference in scale is a difference in kind: this crate's
//!   fixtures and the authored course lose 0–25 patches and remain playable,
//!   while `afterslime.map` loses 1123 and `courtfun.map` 706 — on a movement
//!   map the curves *are* the ramps and tubes, so those compile to something
//!   the player falls through. **Tessellating patches is a Wave 5 question**;
//!   until it is answered, treat [`PatchLoss::Substantial`] as "this map did
//!   not import" rather than "this map imported with blemishes".
//! - **Movers are frozen.** `func_door` and friends compile as static solids
//!   where the mapper left them; the [`World`] contract says the world is
//!   static for the duration of a run.
//! - **Liquids are mostly invisible to it.** Only a shader literally named
//!   `water`, `slime` or `lava` is recognised, and a real map's water is
//!   usually `liquids/clear_calm1`, which compiles as a solid block. Resolving
//!   that needs the `.shader` files C7 flags as an open licensing question.
//! - **Surface flags are per brush, not per face.** Quake 3 reports the flags
//!   of the *side* a trace hit; a hull here carries one set for the whole
//!   brush, ORed over its faces. A floor brush that is slick on top and plain
//!   on its sides comes out wholly slick. Real on Defrag ice maps, invisible
//!   everywhere else, and a contained change (`Vec<Plane>` becoming
//!   `Vec<(Plane, SurfaceFlags)>` in `straf3-collision`) when a wave needs it.
//! - **Textures are out of scope this wave** (C7 flags the licensing question).
//!   Faces are coloured by a hash of their shader name.
//!
//! [`World`]: straf3_sim::world::World

mod bounds;
mod digest;
mod entity;
pub mod mesh;
mod source;
mod texture;

use glam::Vec3;
use straf3_collision::compile_hull;
use straf3_sim::world::SurfaceFlags;

use crate::digest::{Fnv1a, fold_hull};

pub use crate::bounds::Aabb;
pub use crate::entity::{MapEntity, SPAWN_CLEARANCE, Spawn, TriggerKind, TriggerVolume};
pub use crate::mesh::{Mesh, MeshVertex};

// The collision primitives belong to `straf3-collision`, which owns the trace
// that consumes them (C8). They are re-exported because a caller holding a
// `CompiledMap` should not have to name a second crate to read a hull out of it.
pub use straf3_collision::{Hull, HullWorld, Plane, TriggerHull};
// For the same reason one level further down: `collider()`'s contract is
// written in `TriggerSet`, and a caller reading `HullWorld::trigger_coverage`
// to ask whether a map can be timed needs to name it.
pub use straf3_sim::world::TriggerSet;

/// A compiled map: convex hulls for collision, plus what rendering and the run
/// clock need.
///
/// Everything in it is in source order. See the crate docs on why that is a
/// guarantee and not an implementation detail.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct CompiledMap {
    /// Player spawn position, in world units, already lifted clear of the floor
    /// by [`SPAWN_CLEARANCE`]. Zero when the map declared no spawn.
    pub spawn: Vec3,
    /// Which way the player faces at [`CompiledMap::spawn`], in degrees, Quake
    /// convention (0 is +X, increasing anticlockwise).
    pub spawn_yaw: f32,
    /// Every spawn point the map declared, best first.
    pub spawns: Vec<Spawn>,
    /// Solid convex hulls, in source order.
    pub hulls: Vec<Hull>,
    /// Trigger volumes, in source order: start, finish, checkpoints and the
    /// ones this crate only records.
    pub triggers: Vec<TriggerVolume>,
    /// The map as triangles.
    pub mesh: Mesh,
    /// Every entity's keys, in source order, including the point entities that
    /// produced no geometry.
    pub entities: Vec<MapEntity>,
    /// Bounds of every solid hull together.
    pub bounds: Aabb,
    /// Everything the compiler had to decide for itself. Never empty because of
    /// a problem it hid.
    pub warnings: Vec<Warning>,
}

/// The number of dropped patches at or above which a map is called
/// [`PatchLoss::Substantial`].
///
/// # Why a hundred, and why a number at all
///
/// The count alone does not tell a caller anything, because the caller has no
/// scale to read it against. Measured on the corpus: the fixtures in this crate
/// and the authored course drop between zero and twenty-five patches, which is
/// trim, a decorative arch, a light fixture — the route is intact and the map
/// plays. `afterslime.map` drops 1123 and `courtfun.map` drops 706. On a
/// movement map the curved surfaces *are* the ramps and tubes, so those two are
/// not degraded, they are absent: the player falls through where the geometry
/// should be.
///
/// A hundred sits in the empty band between those two populations rather than
/// on a boundary anything is clustered near, which is the only property a
/// threshold like this needs. It is not a claim that ninety-nine patches are
/// fine; it is a claim that the two cases are far enough apart to separate, and
/// a refusal to make every caller rediscover where by hand.
pub const SUBSTANTIAL_PATCH_LOSS: usize = 100;

/// How badly dropped curved surfaces hurt a map.
///
/// The compiler cannot tessellate Bézier patches (see the crate docs), so every
/// patch is geometry that is simply gone. This says whether that is worth a
/// footnote or worth refusing to race on the result.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum PatchLoss {
    /// Fewer than [`SUBSTANTIAL_PATCH_LOSS`] patches. Decorative curves are
    /// missing; the map is playable and a route over brush geometry is intact.
    Partial,
    /// [`SUBSTANTIAL_PATCH_LOSS`] or more. Treat the compiled map as unusable
    /// rather than imperfect — enough of its geometry is missing that a run on
    /// it is not a run on the map the author made.
    Substantial,
}

impl PatchLoss {
    /// Classify a dropped-patch count.
    #[must_use]
    pub fn of(count: usize) -> Self {
        if count >= SUBSTANTIAL_PATCH_LOSS {
            Self::Substantial
        } else {
            Self::Partial
        }
    }
}

/// Something the compiler did that the map's author may not have intended.
///
/// Warnings are data, not log lines — nothing below the seam prints. They are
/// emitted in source order, so two compiles of the same source produce an
/// identical list.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Warning {
    /// Curved surfaces were dropped. Each one is missing collision.
    PatchDropped {
        /// How many.
        count: usize,
        /// Whether that count is a blemish or a hole where the map was. See
        /// [`PatchLoss`] — this exists so a caller can tell the two apart
        /// without knowing [`SUBSTANTIAL_PATCH_LOSS`] by folklore.
        severity: PatchLoss,
    },
    /// A brush whose planes enclose nothing. Usually a bad vertex edit; Radiant
    /// will load such a map without complaint.
    DegenerateBrush {
        /// Index into [`CompiledMap::entities`].
        entity: usize,
        /// Index of the brush within that entity.
        brush: usize,
    },
    /// A brush whose planes do not close, so it would extend to the edge of the
    /// world. Dropped: one of these would fill the map with solid.
    UnboundedBrush {
        /// Index into [`CompiledMap::entities`].
        entity: usize,
        /// Index of the brush within that entity.
        brush: usize,
    },
    /// A trigger naming a target that no entity answers to. The volume is kept
    /// as [`TriggerKind::Other`] rather than dropped.
    UnresolvedTarget {
        /// Index into [`CompiledMap::entities`].
        entity: usize,
        /// The `target` value that resolved to nothing.
        target: String,
    },
    /// A moving brush entity compiled as static geometry where it was authored.
    MoverFrozen {
        /// Index into [`CompiledMap::entities`].
        entity: usize,
        /// Its classname, e.g. `func_door`.
        classname: String,
    },
    /// A brush entity this compiler has no rule for. Its brushes were compiled
    /// as solid world geometry, which is the safe direction to guess in: an
    /// extra wall is visible, a missing one is a route through the level.
    UnknownBrushEntity {
        /// Index into [`CompiledMap::entities`].
        entity: usize,
        /// Its classname.
        classname: String,
    },
    /// A liquid brush compiled as empty space, because Straf3 has no swimming.
    NonSolidLiquid {
        /// Index into [`CompiledMap::entities`].
        entity: usize,
        /// Index of the brush within that entity.
        brush: usize,
    },
    /// The map has geometry but no `target_startTimer` / `target_stopTimer`
    /// pair, so nothing in it can produce a time.
    NoTimerTriggers,
    /// More `target_checkpoint` volumes than [`TriggerSet`] has bits for
    /// ([`TriggerSet::MAX_CHECKPOINTS`]). The volumes are still in
    /// [`CompiledMap::triggers`]; what they lose is a bit in the collider, so
    /// [`CompiledMap::collider`] leaves them out and crossing one splits
    /// nothing.
    ///
    /// The compiler says so rather than dropping them in silence: a run that
    /// is missing its 31st split looks exactly like a player who missed it.
    TooManyCheckpoints {
        /// How many checkpoints have no bit. They are the ones latest in
        /// source order, since numbering follows source order.
        dropped: usize,
    },
}

/// Errors produced while compiling `.map` source.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CompileError {
    /// The source was not valid `.map` text. Carries the parser's own message,
    /// which cites a line number.
    Parse(String),
    /// The map has geometry but declares no player spawn.
    ///
    /// An error and not a warning with a fallback: a map compiled without a
    /// spawn would put the player at the world origin, which is inside the
    /// floor of most maps, and the failure would present as "the game is
    /// broken" rather than as "this map is missing an entity".
    NoPlayerSpawn,
}

impl core::fmt::Display for CompileError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            Self::Parse(msg) => write!(f, "malformed .map source: {msg}"),
            Self::NoPlayerSpawn => write!(
                f,
                "the map has geometry but no info_player_deathmatch or info_player_start"
            ),
        }
    }
}

impl core::error::Error for CompileError {}

/// Compile `.map` source text into a map ready for simulation.
///
/// Takes text, not a path, on purpose — see the module docs.
///
/// # Errors
///
/// [`CompileError::Parse`] when the text is not a `.map`, and
/// [`CompileError::NoPlayerSpawn`] when it has brushes but nowhere to stand.
/// Everything else the compiler can decide for itself it decides, and records
/// in [`CompiledMap::warnings`].
pub fn compile(source: &str) -> Result<CompiledMap, CompileError> {
    let prepared = source::prepare(source);
    // `quake_map::parse` wants a reader; a byte slice is one. Nothing here
    // touches a file — that is the caller's job, above the seam.
    let parsed = quake_map::parse(&mut prepared.text.as_bytes())
        .map_err(|e| CompileError::Parse(e.to_string()))?;

    let entities: Vec<MapEntity> = parsed.entities.iter().map(read_entity).collect();
    let spawns = entity::spawns(&entities);

    let mut warnings = Vec::new();
    if prepared.patches_dropped > 0 {
        warnings.push(Warning::PatchDropped {
            count: prepared.patches_dropped,
            severity: PatchLoss::of(prepared.patches_dropped),
        });
    }

    let mut hulls = Vec::new();
    let mut triggers = Vec::new();
    let mut mesh = Mesh::default();
    let mut bounds = Aabb::empty();
    let mut next_checkpoint = 0u32;

    for (index, source_entity) in parsed.entities.iter().enumerate() {
        if source_entity.brushes.is_empty() {
            continue;
        }
        let classname = entities[index].classname.clone();
        let is_trigger = entity::is_trigger_classname(&classname);
        if !is_trigger {
            match entity::world_brush_role(&classname) {
                entity::BrushEntityRole::Static => {}
                entity::BrushEntityRole::Mover => warnings.push(Warning::MoverFrozen {
                    entity: index,
                    classname: classname.clone(),
                }),
                entity::BrushEntityRole::Unknown => warnings.push(Warning::UnknownBrushEntity {
                    entity: index,
                    classname: classname.clone(),
                }),
            }
        }

        let mut volume_hulls = Vec::new();
        for (brush_index, brush) in source_entity.brushes.iter().enumerate() {
            let Some(geometry) = compile_brush(brush) else {
                warnings.push(Warning::DegenerateBrush {
                    entity: index,
                    brush: brush_index,
                });
                continue;
            };
            if geometry.unbounded {
                warnings.push(Warning::UnboundedBrush {
                    entity: index,
                    brush: brush_index,
                });
                continue;
            }
            if geometry.liquid {
                warnings.push(Warning::NonSolidLiquid {
                    entity: index,
                    brush: brush_index,
                });
            }

            let hull = geometry.hull;
            if is_trigger {
                volume_hulls.push(hull);
                continue;
            }
            if !geometry.solid {
                continue;
            }
            bounds.add(hull.mins, hull.maxs);
            hulls.push(hull);
            for face in &geometry.faces {
                mesh.push_face(&face.points, face.normal, face.color);
            }
        }

        if is_trigger && !volume_hulls.is_empty() {
            let (kind, target, target_classname) =
                entity::classify_trigger(&entities[index], &entities, &mut next_checkpoint);
            if target_classname.is_none() {
                if let Some(name) = target.clone() {
                    warnings.push(Warning::UnresolvedTarget {
                        entity: index,
                        target: name,
                    });
                }
            }
            let mut volume_bounds = Aabb::empty();
            for hull in &volume_hulls {
                volume_bounds.add(hull.mins, hull.maxs);
            }
            triggers.push(TriggerVolume {
                kind,
                classname,
                target,
                target_classname,
                hulls: volume_hulls,
                bounds: volume_bounds,
            });
        }
    }

    let mut out = CompiledMap {
        spawn: spawns.first().map_or(Vec3::ZERO, |s| s.origin),
        spawn_yaw: spawns.first().map_or(0.0, |s| s.yaw),
        spawns,
        hulls,
        triggers,
        mesh,
        entities,
        bounds,
        warnings,
    };

    if out.hulls.is_empty() && out.triggers.is_empty() {
        // Nothing was compiled at all: an empty source, or a file of point
        // entities. Not an error — a caller may legitimately compile one.
        return Ok(out);
    }
    if out.spawns.is_empty() {
        return Err(CompileError::NoPlayerSpawn);
    }
    if !out.has_timing() {
        out.warnings.push(Warning::NoTimerTriggers);
    }
    let dropped = out
        .triggers
        .iter()
        .filter(|t| matches!(t.kind, TriggerKind::Checkpoint(_)))
        .filter(|t| t.kind.trigger_set().is_none())
        .count();
    if dropped > 0 {
        out.warnings.push(Warning::TooManyCheckpoints { dropped });
    }
    Ok(out)
}

impl CompiledMap {
    /// A 64-bit digest of the exact bits of every compiled hull and trigger
    /// volume — C7 requirement 4.
    ///
    /// # Why a recording binds to this and not to a hash of the `.map` file
    ///
    /// Because the geometry the physics ran against is the *output* of this
    /// compiler, not its input. Change how a plane is snapped, or the order
    /// faces are emitted in, and the same source file produces a different
    /// world — one where a ramp is a hundredth of a unit lower and a jump that
    /// used to land no longer does. A recording bound only to the source hash
    /// would replay against that silently and produce a different time with a
    /// straight face.
    ///
    /// So this folds what the hulls actually are: every plane normal and
    /// distance, every bound, every surface flag, in trace order, as bits. The
    /// render mesh is deliberately **not** in it — changing a face's colour must
    /// not invalidate a leaderboard.
    #[must_use]
    pub fn collision_digest(&self) -> u64 {
        let mut h = Fnv1a::new();
        // A tag, so that a future change to *what* this digest covers is itself
        // a change to the digest rather than a silent redefinition.
        h.bytes(b"straf3-map/collision/1");
        h.len(self.hulls.len());
        for hull in &self.hulls {
            fold_hull(hull, &mut h);
        }
        h.len(self.triggers.len());
        for trigger in &self.triggers {
            trigger.fold(&mut h);
        }
        h.finish()
    }

    /// A digest over everything the compiler produced, the render mesh and
    /// entity keys included.
    ///
    /// Not what a recording binds to — that is [`CompiledMap::collision_digest`]
    /// — but what a cross-target determinism check should compare, because it
    /// notices a divergence in the mesh that collision would not see.
    #[must_use]
    pub fn full_digest(&self) -> u64 {
        let mut h = Fnv1a::new();
        h.u64(self.collision_digest());
        self.mesh.fold(&mut h);
        h.vec3(self.spawn);
        h.f32(self.spawn_yaw);
        h.len(self.entities.len());
        for entity in &self.entities {
            entity.fold(&mut h);
        }
        h.finish()
    }

    /// How much curved geometry this map lost, if any.
    ///
    /// `None` when nothing was dropped. Provided so a caller can gate on the
    /// answer — refuse to publish a time, or mark the map unraceable — without
    /// pattern-matching [`Warning`] itself.
    #[must_use]
    pub fn patch_loss(&self) -> Option<(usize, PatchLoss)> {
        self.warnings.iter().find_map(|w| match w {
            Warning::PatchDropped { count, severity } => Some((*count, *severity)),
            _ => None,
        })
    }

    /// Every volume of one kind, in source order.
    pub fn triggers_of(&self, kind: TriggerKind) -> impl Iterator<Item = &TriggerVolume> {
        self.triggers.iter().filter(move |t| t.kind == kind)
    }

    /// Whether the map has both ends of a run: something to start the clock and
    /// something to stop it.
    #[must_use]
    pub fn has_timing(&self) -> bool {
        self.triggers_of(TriggerKind::Start).next().is_some()
            && self.triggers_of(TriggerKind::Finish).next().is_some()
    }

    /// The map as something the simulation can sweep against — C7 requirement 2,
    /// and the link that was missing between `straf3-map` and `straf3-sim`.
    ///
    /// A [`HullWorld`] implements [`World`], so this is what
    /// [`straf3_sim::step_in_place`] takes. Deliberately *not* a second brush
    /// tracer written here: two implementations of the sweep means two
    /// behaviours, and `arena.rs`'s own opening argument is about exactly that
    /// drift — the thing the player collides with and the thing that says they
    /// did must be one piece of code.
    ///
    /// It clones the hulls, which is a copy of the whole map's plane data. That
    /// is a once-per-map cost at load, not a per-frame one, and it buys a
    /// collider that owns its geometry and can outlive the `CompiledMap` — the
    /// browser drops the source text and the entity table after loading.
    ///
    /// The timing volumes go in alongside the solids, in source order, as
    /// [`TriggerHull`]s carrying the [`TriggerSet`] bits
    /// [`TriggerKind::trigger_set`] assigns. They are a separate list inside
    /// [`HullWorld`] and are never traced as solid: a player runs *through* a
    /// finish line. This is what makes [`straf3_sim::step_in_place`] advance
    /// the run clock — it is driven by the geometry, through `Trace::triggers`,
    /// with no second query and nothing above the seam deciding when a run
    /// started.
    ///
    /// A volume whose kind has no bit — a teleporter, a jump pad, an
    /// unrecognised trigger — is left out rather than given a spare bit. The
    /// one case where that is a loss rather than a classification is a
    /// checkpoint past [`TriggerSet::MAX_CHECKPOINTS`], and `compile` reports
    /// it as [`Warning::TooManyCheckpoints`].
    ///
    /// [`World`]: straf3_sim::world::World
    #[must_use]
    pub fn collider(&self) -> HullWorld {
        let mut volumes = Vec::new();
        for trigger in &self.triggers {
            let Some(set) = trigger.kind.trigger_set() else {
                continue;
            };
            // One trigger entity may own several brushes; each is a piece of
            // the same volume and carries the same bits.
            for hull in &trigger.hulls {
                volumes.push(TriggerHull::new(set, hull.clone()));
            }
        }
        HullWorld::new(self.hulls.clone()).with_triggers(volumes)
    }
}

/// One face of a compiled brush, ready for the mesh.
struct FaceGeometry {
    points: Vec<Vec3>,
    normal: Vec3,
    color: [f32; 3],
}

/// A brush after compilation, before it is filed as solid or as a trigger.
struct BrushGeometry {
    hull: Hull,
    solid: bool,
    liquid: bool,
    unbounded: bool,
    faces: Vec<FaceGeometry>,
}

/// Anything beyond this is not geometry, it is a brush that failed to close.
///
/// Quake's world is ±65536, which is also where `straf3-collision` starts a
/// face's base winding — so a brush whose planes do not close comes back with
/// its bounds exactly there, untouched, and this is the test that catches it.
const MAX_MAP_COORD: f32 = 65_536.0;

/// Turn one `.map` brush into a collision hull and its face polygons.
///
/// The geometry itself is `straf3-collision`'s
/// [`compile_hull`](straf3_collision::compile_hull), and deliberately so: it is
/// q3map's own sequence — drop duplicate planes, clip each plane's base winding
/// into a face polygon, take the bounds from those polygons, then add the bevel
/// planes a swept *box* needs and permute the list into q3map's canonical plane
/// order. Every one of those steps is a decision about where the player may
/// stand, so there must be exactly one implementation of it, in the crate that
/// traces the result. What is left here is the part that is genuinely about
/// `.map` files: which shader a face carries, and therefore what it means.
///
/// `None` when the brush encloses nothing — which real maps contain, because
/// Radiant will happily save a brush a bad vertex drag has turned inside out.
fn compile_brush(brush: &[quake_map::Surface]) -> Option<BrushGeometry> {
    let mut planes: Vec<Plane> = Vec::with_capacity(brush.len());
    let mut roles = Vec::with_capacity(brush.len());
    let mut textures: Vec<String> = Vec::with_capacity(brush.len());

    let mut solid = true;
    let mut liquid = false;
    let mut surface = SurfaceFlags::NONE;

    for face in brush {
        let texture = face.texture.to_string_lossy().into_owned();
        let role = texture::role_of(&texture, face.q2ext.content_flags, face.q2ext.surface_flags);
        // One non-solid face makes the whole brush non-solid: a trigger, a hint
        // or an origin brush is textured that way on every side, and a brush
        // that mixes the two is not a shape Quake's own compiler would keep.
        solid &= role.solid;
        liquid |= texture::is_liquid(&texture);
        surface = surface.with(role.flags);

        let [p0, p1, p2] = face.half_space;
        // In file order, and the order is not interchangeable: `from_points` is
        // q3map's `PlaneFromPoints`, whose cross-product order is what makes the
        // normal point *out* of the brush. Reversed, every brush in the map
        // becomes a hollow shell and the player falls through the floor.
        let Some(plane) = Plane::from_points(point(p0), point(p1), point(p2)) else {
            continue;
        };
        planes.push(plane);
        roles.push(role);
        textures.push(texture);
    }

    let compiled = compile_hull(&planes, surface)?;
    let hull = compiled.hull;

    // `faces` is parallel to the *input* planes, which is what carries each
    // face's shader through to the mesh. `hull.planes` cannot: by now it has
    // been deduplicated, permuted and extended with bevels.
    let mut faces = Vec::with_capacity(planes.len());
    for (index, winding) in compiled.faces.iter().enumerate() {
        if winding.len() < 3 || !(roles[index].drawn && solid) {
            continue;
        }
        faces.push(FaceGeometry {
            normal: planes[index].normal,
            color: mesh::color_for(&textures[index]),
            points: winding.clone(),
        });
    }

    let unbounded = hull.mins.abs().max_element() >= MAX_MAP_COORD
        || hull.maxs.abs().max_element() >= MAX_MAP_COORD;

    Some(BrushGeometry {
        hull,
        solid,
        liquid: liquid && !solid,
        unbounded,
        faces,
    })
}

/// `quake-map` parses coordinates as `f64`; the compiler works in the
/// simulation's `f32`.
///
/// Narrowed here, at the boundary, rather than after computing the plane in
/// `f64`. A plane derived in double precision and rounded afterwards is a
/// *different* plane from the one q3map produced from the same three points,
/// by a few ulps — and `straf3-collision`'s bevel test compares normal
/// components against `1.0` exactly. Being bit-for-bit what q3map was is worth
/// more here than being closer to the real number.
fn point(p: [f64; 3]) -> Vec3 {
    Vec3::new(p[0] as f32, p[1] as f32, p[2] as f32)
}

/// One entity's keys, lowercased where lowercasing is safe.
///
/// Keys and the classname are lowercased because `.map` files disagree about
/// casing and nothing is gained by treating `Target` and `target` as two keys.
/// Values are left exactly as written: a `targetname` is matched
/// case-insensitively where it is *used*, but a `message` is text a player may
/// one day read.
fn read_entity(entity: &quake_map::Entity) -> MapEntity {
    let mut keys = Vec::with_capacity(entity.edict.len());
    let mut classname = String::new();
    for (k, v) in &entity.edict {
        let key = k.to_string_lossy().to_ascii_lowercase();
        let value = v.to_string_lossy().into_owned();
        if key == "classname" && classname.is_empty() {
            classname = value.to_ascii_lowercase();
        }
        keys.push((key, value));
    }
    MapEntity { classname, keys }
}

#[cfg(test)]
mod tests;
