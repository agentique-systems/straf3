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
//! outright: parry is used for swept queries *subject to a determinism audit
//! before we commit to it*. If the physics called parry directly, that audit
//! would have no cheap answer — a "no" would mean rewriting movement code.
//! With the query behind a trait the simulation owns, the answer is: write a
//! different implementor. `straf3-sim` does not depend on `straf3-collision`
//! at all, and `cargo xtask check-seam` will show parry3d absent from its
//! dependency tree.
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
    pub all_solid: bool,
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
pub trait World {
    /// Sweep the player's hull and report what stopped it.
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
