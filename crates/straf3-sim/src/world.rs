//! The collision seam: how the simulation asks about geometry.
//!
//! # Why this is a trait the simulation defines
//!
//! The simulation needs to know one thing about the world: *if I sweep the
//! player's hull from here to there, what stops me and what is the surface
//! like?* That is [`World::trace`], and it is the entire interface.
//!
//! It is a trait **defined here**, in `straf3-sim`, rather than a concrete
//! type imported from `straf3-collision`, for a reason spec section 4 states
//! outright: a geometry library — parry3d was the candidate — would have been
//! used for swept queries *subject to a determinism audit before we commit to
//! it*. If the physics called such a library directly, that audit would have no
//! cheap answer: a "no" would mean rewriting movement code. With the query
//! behind a trait the simulation owns, the answer is: write a different
//! implementor.
//!
//! That is what happened. `straf3-collision` answers with a hand-written Q3
//! brush tracer, the audit never had to be run, and parry3d is no longer a
//! workspace dependency at all. The seam is what made that reversible, and it
//! still is: `straf3-sim` does not depend on `straf3-collision`, and
//! `cargo xtask check-seam` holds its dependency tree to glam alone.
//!
//! The direction matters too. The *consumer* declares the interface, so the
//! shape is driven by what the physics needs, not by what a collision library
//! happens to return. Quake's `trace_t` is the shape being reproduced here
//! because the movement code being reproduced is written against it.
//!
//! It also makes the simulation trivially testable: [`EmptyWorld`] and
//! [`FlatGround`] below are complete, honest worlds in a dozen lines, and no
//! test of movement behaviour needs a compiled map to exist.

use crate::num::{Scalar, Vec3, s, vec3};

/// Properties of a surface that change how the player interacts with it.
///
/// A bitfield mirroring Quake's `SURF_*` flags. Only the ones that alter
/// movement are here; rendering flags are not the simulation's business and
/// must not leak in.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub struct SurfaceFlags(pub u32);

impl SurfaceFlags {
    /// No special properties.
    pub const NONE: Self = Self(0);
    /// Slick: no ground friction is applied (Q3 `SURF_SLICK`).
    pub const SLICK: Self = Self(1 << 0);
    /// Ladder: vertical movement is driven by input rather than gravity.
    pub const LADDER: Self = Self(1 << 1);
    /// Nothing may stand on this surface, whatever its normal says.
    pub const NOSTEP: Self = Self(1 << 2);

    /// Whether every flag in `other` is set.
    #[must_use]
    pub const fn contains(self, other: Self) -> bool {
        self.0 & other.0 == other.0
    }

    /// This set plus `other`.
    #[must_use]
    pub const fn with(self, other: Self) -> Self {
        Self(self.0 | other.0)
    }
}

/// Timing volumes a swept hull passed through.
///
/// A bitmask rather than a collection, for the reason ARCHITECTURE C4 gives:
/// the simulation allocates nothing on the movement path, and a map has a small
/// fixed number of timing volumes. Accumulating one is an `|=`, and saving and
/// restoring the accumulator across a rolled-back move is a register copy.
///
/// # Why the simulation names these and not the map
///
/// `straf3-sim` must not know what a `.map` is — it does not depend on
/// `straf3-map`, deliberately (see that crate's manifest and this module's
/// docs). But the run clock has to be *below* the seam, because a time that is
/// not a pure consequence of the command stream is not reproducible and cannot
/// be folded into [`SimState::checksum`].
///
/// The resolution is that geometry knowledge travels up the dependency chain as
/// *bits*, not as types. `straf3-map` knows that a `trigger_multiple` wired to a
/// `target_startTimer` is a start line; it compresses that to
/// [`TriggerSet::START`] when it builds the world. `straf3-collision` carries
/// the bits through the tracer without interpreting them. `straf3-sim` reads
/// [`Self::START`] and [`Self::FINISH`] and knows nothing else about the map.
/// This is the same inversion that makes [`World`] a trait the *consumer*
/// declares.
///
/// [`SimState::checksum`]: crate::SimState::checksum
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub struct TriggerSet(pub u32);

impl TriggerSet {
    /// No volumes at all.
    pub const NONE: Self = Self(0);
    /// The start line: crossing it starts the clock.
    pub const START: Self = Self(1 << 0);
    /// The finish line: crossing it stops the clock.
    pub const FINISH: Self = Self(1 << 1);

    /// The bit [`Self::checkpoint`] gives checkpoint zero. Bits below it are
    /// [`Self::START`] and [`Self::FINISH`].
    pub const FIRST_CHECKPOINT_BIT: u32 = 2;

    /// How many checkpoints fit in the remaining bits.
    pub const MAX_CHECKPOINTS: u32 = 32 - Self::FIRST_CHECKPOINT_BIT;

    /// The bit for checkpoint `index`, or `None` past [`Self::MAX_CHECKPOINTS`].
    ///
    /// `None` rather than a silent wrap: a map with 31 checkpoints must be
    /// reported as unrepresentable by whoever compiled it, not quietly given a
    /// checkpoint that shares a bit with the finish line.
    #[must_use]
    pub const fn checkpoint(index: u32) -> Option<Self> {
        if index < Self::MAX_CHECKPOINTS {
            Some(Self(1 << (Self::FIRST_CHECKPOINT_BIT + index)))
        } else {
            None
        }
    }

    /// Whether every volume in `other` is in this set.
    ///
    /// Note that an empty `other` is contained by everything, as with
    /// [`SurfaceFlags::contains`].
    #[must_use]
    pub const fn contains(self, other: Self) -> bool {
        self.0 & other.0 == other.0
    }

    /// Whether the two sets share any volume.
    #[must_use]
    pub const fn intersects(self, other: Self) -> bool {
        self.0 & other.0 != 0
    }

    /// Whether nothing was touched.
    #[must_use]
    pub const fn is_empty(self) -> bool {
        self.0 == 0
    }

    /// This set plus `other`.
    #[must_use]
    pub const fn with(self, other: Self) -> Self {
        Self(self.0 | other.0)
    }
}

/// A request to sweep the player's hull through the world.
///
/// The hull is an axis-aligned box, as Quake's was. That is not a placeholder
/// for something better: an AABB player hull is *why* edge clipping and
/// overbounce feel the way they do, and a capsule would remove them.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Sweep {
    /// Where the hull's origin starts.
    pub start: Vec3,
    /// Where the hull's origin would end up if nothing were in the way.
    pub end: Vec3,
    /// Half the hull's size on each axis, relative to the origin.
    pub half_extents: Vec3,
    /// Origin-relative offset of the hull's centre.
    ///
    /// Quake's player hull is not centred on the origin: it spans
    /// `mins = (-15, -15, -24)` to `maxs = (15, 15, 32)`, so the origin sits
    /// 24 units above the hull's underside and the box centre is 4 units
    /// *above* the origin. Keeping that offset explicit rather than assumed
    /// means crouching and the various hull sizes do not each need their own
    /// convention, and an implementor never has to guess where the box is.
    pub center_offset: Vec3,
}

/// What a sweep hit.
///
/// Deliberately shaped like Quake's `trace_t`, because the movement code that
/// will consume it is being reproduced from source written against that shape.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Trace {
    /// Fraction of the requested motion actually travelled, in `0.0..=1.0`.
    /// `1.0` means nothing was hit.
    pub fraction: Scalar,
    /// Surface normal at the impact point. Meaningless when nothing was hit;
    /// implementors should return zero then rather than stale data.
    pub normal: Vec3,
    /// Properties of the surface that was hit.
    pub surface: SurfaceFlags,
    /// The hull started inside solid geometry.
    ///
    /// The physics must handle this rather than trusting it never happens —
    /// it is reachable through teleports, spawn points placed badly, and the
    /// float error that produces overbounce.
    pub start_solid: bool,
    /// The hull was inside solid geometry for the whole sweep.
    ///
    /// Implementors must ensure this implies `fraction == 0.0`. ARCHITECTURE C8
    /// states it as an obligation rather than leaving it as an accident of one
    /// tracer, because [`Self::triggers`] leans on it: a hull that starts inside
    /// solid has not legitimately travelled anywhere, and a nonzero fraction
    /// alongside `all_solid` would credit an aborted move with volumes it never
    /// reached.
    pub all_solid: bool,
    /// Timing volumes the hull overlapped along the part of this sweep it
    /// **actually travelled** — over `start ..= start + (end - start) *
    /// fraction`, not over the whole requested segment.
    ///
    /// Triggers are non-solid, so they never affect [`Self::fraction`] or
    /// [`Self::normal`]: this is what the sweep *passed through*, not what
    /// stopped it. But it must stop where the sweep stopped, and that is a
    /// contract on the implementor, not a hint. The physics issues sweeps whose
    /// motion it then discards — `PM_SlideMove`'s zero-fraction bumps,
    /// `PM_StepSlideMove`'s abandoned lift — and reporting volumes beyond
    /// `fraction` credits a player with a finish line they never reached.
    ///
    /// Gathering is clamped to `fraction`, not to the mover's
    /// `SURFACE_CLIP_EPSILON`-backed-off fraction, which the tracer cannot see.
    /// That over-reports by at most the epsilon backoff at the moment of
    /// impact — a hull flush against a wall reports a volume it is touching but
    /// not quite standing in. On a finish line that is the correct direction to
    /// err, and it is bounded by a constant rather than by the length of the
    /// move.
    ///
    /// An implementor with no timing volumes leaves this
    /// [`TriggerSet::NONE`]; [`Trace::clear`] already does.
    pub triggers: TriggerSet,
}

impl Trace {
    /// A sweep that completed without touching anything.
    #[must_use]
    pub const fn clear() -> Self {
        Self {
            fraction: s(1.0),
            normal: Vec3::ZERO,
            surface: SurfaceFlags::NONE,
            start_solid: false,
            all_solid: false,
            triggers: TriggerSet::NONE,
        }
    }

    /// Whether the sweep was interrupted by geometry.
    #[must_use]
    pub fn hit(&self) -> bool {
        self.fraction < s(1.0)
    }
}

/// Static geometry the simulation can ask about.
///
/// Implemented outside this crate — by a compiled map, by a collision library
/// behind an adapter, or by a test stub. See the module docs for why the
/// simulation owns this trait instead of importing a concrete tracer.
///
/// # Contract
///
/// Implementors **must be pure and deterministic**: the same [`Sweep`] against
/// the same geometry must return bit-identical results every time, within a
/// run and across runs of the same binary. The simulation's determinism
/// guarantee is only as good as this promise. In particular an implementor
/// must not consult the clock, cache anything that depends on call order, or
/// use work-stealing parallelism — reduction order changes float results.
///
/// The world is static for the duration of a run. Moving platforms and doors,
/// if they ever exist, get their own interface rather than making this one
/// time-dependent.
///
/// Two further obligations, both of which the run clock depends on and neither
/// of which the type system can state (ARCHITECTURE C4 and C8):
///
/// 1. **`all_solid` implies `fraction == 0.0`.**
/// 2. **[`Trace::triggers`] covers the traversed prefix only** — the volumes the
///    hull overlaps over `start ..= start + (end - start) * fraction`, not over
///    the requested segment. The natural BSP implementation violates this: the
///    descent visits trigger leaves along the whole query segment, and OR-ing
///    each one as it is reached gathers volumes past the impact point.
///
/// There is deliberately no separate `fn triggers(&self, sweep) -> TriggerSet`.
/// `Pmove` issues up to a dozen sweeps per command along a genuinely bent path,
/// and a single coarse query from command-start to command-end is a chord across
/// that bend which can miss a volume the player's real hull went through. Riding
/// on `Trace` makes the granularity of trigger testing the granularity of
/// tracing, by construction, with no second call site whose cadence could drift —
/// and it inherits the exact hull of the sweep that found it, which matters
/// because `PM_CheckDuck` changes that hull mid-command.
pub trait World {
    /// Sweep the player's hull and report what stopped it, and what it passed
    /// through on the way.
    fn trace(&self, sweep: &Sweep) -> Trace;
}

// A blanket impl so `&W`, `Box<W>` and friends are usable wherever a `World`
// is expected. Callers should not have to care about indirection.
impl<W: World + ?Sized> World for &W {
    fn trace(&self, sweep: &Sweep) -> Trace {
        (**self).trace(sweep)
    }
}

/// A world with no geometry at all: every sweep completes.
///
/// Useful for testing the parts of the simulation that are not about
/// collision — determinism, timing, command handling — without a map.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct EmptyWorld;

impl World for EmptyWorld {
    fn trace(&self, _sweep: &Sweep) -> Trace {
        Trace::clear()
    }
}

/// An infinite horizontal plane at a given height, and nothing else.
///
/// Enough geometry to have a floor to stand on, land on and slide along, with
/// no map pipeline involved. The smallest world in which "did the player land"
/// is a meaningful question.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct FlatGround {
    /// Z coordinate of the plane's surface.
    pub height: Scalar,
    /// Properties of the plane's surface.
    pub surface: SurfaceFlags,
}

impl FlatGround {
    /// A plane at `height` with ordinary surface properties.
    #[must_use]
    pub const fn at(height: Scalar) -> Self {
        Self {
            height,
            surface: SurfaceFlags::NONE,
        }
    }
}

impl Default for FlatGround {
    fn default() -> Self {
        Self::at(s(0.0))
    }
}

impl World for FlatGround {
    fn trace(&self, sweep: &Sweep) -> Trace {
        // The hull's lowest point is its origin plus the centre offset minus
        // the half extent on Z.
        let foot = |p: Vec3| p.z + sweep.center_offset.z - sweep.half_extents.z;
        let start_z = foot(sweep.start);
        let end_z = foot(sweep.end);

        if start_z < self.height {
            return Trace {
                fraction: s(0.0),
                normal: vec3(s(0.0), s(0.0), s(1.0)),
                surface: self.surface,
                start_solid: true,
                all_solid: end_z < self.height,
                triggers: TriggerSet::NONE,
            };
        }
        if end_z >= self.height {
            return Trace::clear();
        }

        // Linear: the only motion that can cross the plane is on Z, and the
        // sweep is a straight line.
        let travelled = start_z - self.height;
        let total = start_z - end_z;
        Trace {
            fraction: (travelled / total).clamp(s(0.0), s(1.0)),
            normal: vec3(s(0.0), s(0.0), s(1.0)),
            surface: self.surface,
            start_solid: false,
            all_solid: false,
            triggers: TriggerSet::NONE,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sweep(from: Vec3, to: Vec3) -> Sweep {
        Sweep {
            start: from,
            end: to,
            half_extents: vec3(s(15.0), s(15.0), s(28.0)),
            center_offset: vec3(s(0.0), s(0.0), s(28.0)),
        }
    }

    #[test]
    fn empty_world_never_stops_anything() {
        let t = EmptyWorld.trace(&sweep(Vec3::ZERO, vec3(s(0.0), s(0.0), s(-1000.0))));
        assert!(!t.hit());
        assert_eq!(t.fraction, s(1.0));
    }

    #[test]
    fn falling_onto_flat_ground_stops_halfway_when_it_should() {
        let ground = FlatGround::at(s(0.0));
        // Feet start at z=10, end at z=-10: the plane is crossed at half.
        let t = ground.trace(&sweep(
            vec3(s(0.0), s(0.0), s(10.0)),
            vec3(s(0.0), s(0.0), s(-10.0)),
        ));
        assert!(t.hit());
        assert_eq!(t.fraction, s(0.5));
        assert_eq!(t.normal, vec3(s(0.0), s(0.0), s(1.0)));
        assert!(!t.start_solid);
    }

    #[test]
    fn moving_above_flat_ground_is_unobstructed() {
        let ground = FlatGround::at(s(0.0));
        let t = ground.trace(&sweep(
            vec3(s(0.0), s(0.0), s(10.0)),
            vec3(s(100.0), s(0.0), s(10.0)),
        ));
        assert!(!t.hit());
    }

    #[test]
    fn starting_below_the_plane_reports_start_solid() {
        let ground = FlatGround::at(s(0.0));
        let t = ground.trace(&sweep(
            vec3(s(0.0), s(0.0), s(-5.0)),
            vec3(s(0.0), s(0.0), s(-6.0)),
        ));
        assert!(t.start_solid);
        assert!(t.all_solid);
        assert_eq!(t.fraction, s(0.0));
    }
}
