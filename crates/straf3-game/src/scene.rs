//! Which world the session collides against, and where the player starts.
//!
//! # Why the arena is not defined here
//!
//! The hardcoded arena is `straf3-render`'s, as one piece of data consumed two
//! ways: drawn as a mesh, and traced as collision geometry. That is not a
//! layering nicety — if the game kept its own copy of the geometry, the thing
//! you can see and the thing you bump into would be two hardcoded box lists
//! that drift apart, and the first symptom would be walking through a visible
//! wall. So `straf3-game` imports the arena; it never re-states it.
//!
//! This module is the *only* place `straf3_render::arena` is named, so if that
//! module's shape moves, exactly one file has to follow it.
//!
//! # Why `flat` and `empty` are still selectable
//!
//! `straf3-headless` knows those two worlds and nothing else. Running the
//! windowed build in the flat world is what makes an end-to-end criterion-4
//! comparison possible between two separate binaries — record here, replay
//! there, compare checksums.

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

/// Which world to play in.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum WorldChoice {
    /// The hardcoded arena — floor, walls, platform and ramps. Requires the
    /// `render` feature, because the arena lives in `straf3-render`.
    #[default]
    Arena,
    /// An infinite plane at z=0. Reproducible by `straf3-headless`.
    Flat,
    /// No geometry at all. Reproducible by `straf3-headless`.
    Empty,
}

impl WorldChoice {
    /// Parse a command-line spelling.
    #[must_use]
    pub fn parse(name: &str) -> Option<Self> {
        match name {
            "arena" => Some(Self::Arena),
            "flat" => Some(Self::Flat),
            "empty" => Some(Self::Empty),
            _ => None,
        }
    }

    /// Whether this build can actually provide this world.
    #[must_use]
    pub const fn is_available(self) -> bool {
        match self {
            Self::Arena => cfg!(feature = "render"),
            Self::Flat | Self::Empty => true,
        }
    }

    /// This choice if the build can provide it, otherwise the flat world.
    ///
    /// A `--no-default-features` build has no renderer and therefore no arena;
    /// it still opens a window you can move in, which is what makes the input
    /// path and the pacing testable on a machine with no GPU at all.
    #[must_use]
    pub fn or_fallback(self) -> Self {
        if self.is_available() {
            self
        } else {
            log::warn!(
                "this build has no renderer, so it has no arena either — \
                 falling back to the flat world"
            );
            Self::Flat
        }
    }

    /// The collision geometry.
    #[must_use]
    pub fn world(self) -> &'static dyn World {
        match self {
            #[cfg(feature = "render")]
            Self::Arena => &straf3_render::arena::ARENA,
            #[cfg(not(feature = "render"))]
            Self::Arena => &FLAT_GROUND,
            Self::Flat => &FLAT_GROUND,
            Self::Empty => &EMPTY,
        }
    }

    /// Where the player starts, and which way they face.
    #[must_use]
    pub fn spawn(self) -> (Vec3, Scalar) {
        match self {
            #[cfg(feature = "render")]
            Self::Arena => (straf3_render::arena::SPAWN, straf3_render::arena::SPAWN_YAW),
            #[cfg(not(feature = "render"))]
            Self::Arena => (FLAT_SPAWN, FLAT_SPAWN_YAW),
            Self::Flat | Self::Empty => (FLAT_SPAWN, FLAT_SPAWN_YAW),
        }
    }

    /// How a recording should describe this world.
    #[must_use]
    pub const fn spec(self) -> WorldSpec {
        match self {
            Self::Arena => WorldSpec::Unrepresentable("arena"),
            Self::Flat => WorldSpec::Flat(s(0.0)),
            Self::Empty => WorldSpec::Empty,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use straf3_sim::world::Sweep;

    #[test]
    fn the_command_line_spellings_round_trip() {
        assert_eq!(WorldChoice::parse("arena"), Some(WorldChoice::Arena));
        assert_eq!(WorldChoice::parse("flat"), Some(WorldChoice::Flat));
        assert_eq!(WorldChoice::parse("empty"), Some(WorldChoice::Empty));
        assert_eq!(WorldChoice::parse("Arena"), None);
        assert_eq!(WorldChoice::parse(""), None);
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
    fn every_choice_yields_a_world_that_answers_traces() {
        for choice in [WorldChoice::Arena, WorldChoice::Flat, WorldChoice::Empty] {
            let (spawn, _) = choice.or_fallback().spawn();
            let world = choice.or_fallback().world();
            let trace = world.trace(&Sweep {
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
    fn a_build_without_a_renderer_falls_back_rather_than_failing() {
        let choice = WorldChoice::Arena.or_fallback();
        if cfg!(feature = "render") {
            assert_eq!(choice, WorldChoice::Arena);
        } else {
            assert_eq!(choice, WorldChoice::Flat);
        }
    }
}
