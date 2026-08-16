//! What a texture name means to collision and to rendering.
//!
//! # Why a name decides anything at all
//!
//! In Quake 3 it does not: a face names a *shader*, and the shader's
//! `surfaceparm` lines in a `.shader` file inside a `.pk3` decide whether the
//! brush is solid, slick, a trigger or invisible. Those `.pk3`s are exactly
//! what ARCHITECTURE C7 flags as a licensing question that cannot be resolved
//! here (§11 decision F), and the first browser client renders untextured
//! geometry anyway.
//!
//! What survives without them is the convention every Quake 3 mapper follows
//! and every Defrag map obeys, because Radiant's own toolset enforces it: the
//! `common/` shader set has fixed, universally-known meanings.
//! `common/caulk` is solid and never drawn in every map ever made. So the name
//! is read directly, and the rule is stated here where it can be checked and
//! corrected, rather than inferred from an archive we may not ship.
//!
//! The `.map` face's Quake 2 extension fields — the three trailing integers
//! Radiant writes for Q3 — are honoured on top of that when they are non-zero,
//! because when a mapper *has* set them explicitly they are more specific than
//! any name.

use straf3_sim::world::SurfaceFlags;

/// What the compiler does with a face and the brush it belongs to.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct TextureRole {
    /// Whether a brush carrying this face blocks the player.
    pub solid: bool,
    /// Whether the face becomes triangles.
    pub drawn: bool,
    /// Movement-relevant properties the face contributes.
    pub flags: SurfaceFlags,
}

/// Shader names that make a brush pass-through.
///
/// Every one of these is either a compiler hint that never reaches the game
/// (`hint`, `skip`, `areaportal`, `lightgrid`), a bot-navigation marker
/// (`botclip`, `donotenter`, `clusterportal`), or a volume rather than a solid
/// (`trigger`, `fog`, `nodrop`). A player walks through all of them.
///
/// Liquids are in this list on purpose and it is the compiler's weakest
/// inference, so it is worth stating plainly. `water`, `slime` and `lava` are
/// volumes with their own movement rules in Q3, and Straf3 has no swimming;
/// treating them as empty space is the honest behaviour for a movement
/// platform, and [`crate::Warning::NonSolidLiquid`] reports every one. But
/// only a shader *named* `water`, `slime` or `lava` is recognised, and a real
/// map's water is usually `liquids/clear_calm1` or similar. Those compile as
/// ordinary solid brushes — a pool becomes a block. Fixing that properly needs
/// the `.shader` files C7 flags as an unresolved licensing question, so until
/// then it is a known wrong answer rather than a hidden one.
const NONSOLID: &[&str] = &[
    "trigger",
    "hint",
    "hintskip",
    "skip",
    "origin",
    "areaportal",
    "clusterportal",
    "cluster",
    "antiportal",
    "lightgrid",
    "botclip",
    "donotenter",
    "nodrop",
    "fog",
    "water",
    "slime",
    "lava",
];

/// Liquid shaders, called out separately only so the warning can name them.
const LIQUIDS: &[&str] = &["water", "slime", "lava"];

/// Shader names that are solid but never drawn.
///
/// `caulk` and `nodraw` are the faces of a brush that are hidden behind other
/// brushes — drawing them would double the triangle count of a real map for
/// nothing. `clip` and its relatives are invisible walls, the mapper's tool for
/// smoothing a surface the player should not catch on, and a Defrag route often
/// depends on one being exactly where it is.
///
/// `sky` is here because a Q3 sky brush *is* solid: it is what stops you
/// leaving the level upward. Drawing it would put a flat-shaded box over the
/// map.
const SOLID_NODRAW: &[&str] = &[
    "caulk",
    "caulkshadow",
    "nodraw",
    "nodrawsolid",
    "clip",
    "playerclip",
    "full_clip",
    "weapclip",
    "weaponclip",
    "monsterclip",
    "invisible",
    "sky",
    "cushion",
];

// Quake 3 `SURF_*` bits, as Radiant writes them into the third field of a face.
const SURF_SLICK: i32 = 0x2;
const SURF_SKY: i32 = 0x4;
const SURF_LADDER: i32 = 0x8;
const SURF_NODRAW: i32 = 0x80;
const SURF_NOSTEPS: i32 = 0x2000;
const SURF_NONSOLID: i32 = 0x4000;

// Quake 3 `CONTENTS_*` bits, from the second field.
const CONTENTS_PLAYERCLIP: i32 = 0x10000;
const CONTENTS_TRIGGER: i32 = 0x4000_0000;

/// The part of a shader path that carries the meaning.
///
/// `textures/common/caulk`, `common/caulk` and `caulk` are the same shader
/// written three ways, and all three occur — Radiant writes the middle form,
/// hand-edited maps and Quake 1 maps write the last.
pub(crate) fn base_name(texture: &str) -> String {
    texture
        .rsplit(['/', '\\'])
        .next()
        .unwrap_or(texture)
        .to_ascii_lowercase()
}

/// What this face means, from its shader name and its explicit flags.
pub(crate) fn role_of(texture: &str, content_flags: i32, surface_flags: i32) -> TextureRole {
    let name = base_name(texture);

    let mut solid = !NONSOLID.contains(&name.as_str());
    let mut drawn = solid && !SOLID_NODRAW.contains(&name.as_str());
    let mut flags = SurfaceFlags::NONE;

    if name.contains("slick") {
        flags = flags.with(SurfaceFlags::SLICK);
    }
    if name.contains("ladder") {
        flags = flags.with(SurfaceFlags::LADDER);
    }

    // Explicit flags override the naming convention: a mapper who typed the
    // number meant it. Only non-zero fields say anything — Radiant writes
    // `0 0 0` for every face that inherits its shader's parms, which is most.
    if surface_flags & SURF_SLICK != 0 {
        flags = flags.with(SurfaceFlags::SLICK);
    }
    if surface_flags & SURF_LADDER != 0 {
        flags = flags.with(SurfaceFlags::LADDER);
    }
    if surface_flags & SURF_NOSTEPS != 0 {
        flags = flags.with(SurfaceFlags::NOSTEP);
    }
    if surface_flags & (SURF_NODRAW | SURF_SKY) != 0 {
        drawn = false;
    }
    if surface_flags & SURF_NONSOLID != 0 {
        solid = false;
        drawn = false;
    }
    if content_flags & CONTENTS_TRIGGER != 0 {
        solid = false;
        drawn = false;
    }
    if content_flags & CONTENTS_PLAYERCLIP != 0 {
        solid = true;
        drawn = false;
    }

    TextureRole {
        solid,
        drawn,
        flags,
    }
}

/// Whether this shader is a liquid, for the warning that says so.
pub(crate) fn is_liquid(texture: &str) -> bool {
    LIQUIDS.contains(&base_name(texture).as_str())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_path_prefix_does_not_change_the_meaning() {
        for name in [
            "caulk",
            "common/caulk",
            "textures/common/caulk",
            "COMMON/Caulk",
        ] {
            let r = role_of(name, 0, 0);
            assert!(r.solid, "{name} must be solid");
            assert!(!r.drawn, "{name} must not be drawn");
        }
    }

    #[test]
    fn ordinary_world_texture_is_solid_and_drawn() {
        let r = role_of("base_floor/clang_floor3", 0, 0);
        assert!(r.solid && r.drawn);
        assert_eq!(r.flags, SurfaceFlags::NONE);
    }

    #[test]
    fn triggers_and_hints_are_walked_through() {
        for name in [
            "common/trigger",
            "common/hint",
            "common/skip",
            "common/origin",
        ] {
            let r = role_of(name, 0, 0);
            assert!(!r.solid, "{name} must not block the player");
            assert!(!r.drawn);
        }
    }

    #[test]
    fn clip_blocks_without_being_seen_and_sky_does_too() {
        for name in [
            "common/clip",
            "common/playerclip",
            "common/weapclip",
            "common/sky",
        ] {
            let r = role_of(name, 0, 0);
            assert!(r.solid, "{name} must block the player");
            assert!(!r.drawn, "{name} must not be drawn");
        }
    }

    #[test]
    fn slick_and_ladder_reach_the_simulation() {
        assert!(
            role_of("common/slick", 0, 0)
                .flags
                .contains(SurfaceFlags::SLICK)
        );
        // A real map names them like this, not as a bare `slick`.
        assert!(
            role_of("liquids/slime_slick", 0, 0)
                .flags
                .contains(SurfaceFlags::SLICK)
        );
        assert!(
            role_of("common/ladder", 0, 0)
                .flags
                .contains(SurfaceFlags::LADDER)
        );
    }

    #[test]
    fn explicit_flags_beat_the_naming_convention() {
        // A shader with an ordinary name, marked slick by hand.
        let r = role_of("base_wall/concrete", 0, SURF_SLICK);
        assert!(r.flags.contains(SurfaceFlags::SLICK));
        assert!(r.solid && r.drawn);

        // …and marked non-solid by hand.
        let r = role_of("base_wall/concrete", 0, SURF_NONSOLID);
        assert!(!r.solid);

        // Contents saying "this is a trigger" wins over a drawable name.
        let r = role_of("base_wall/concrete", CONTENTS_TRIGGER, 0);
        assert!(!r.solid && !r.drawn);
    }

    #[test]
    fn zero_flags_say_nothing() {
        // The common case in a real map: Radiant writes `0 0 0` and the meaning
        // comes entirely from the name.
        assert_eq!(role_of("common/caulk", 0, 0), role_of("common/caulk", 0, 0));
        let r = role_of("gothic_block/blocks18c", 0, 0);
        assert!(r.solid && r.drawn && r.flags == SurfaceFlags::NONE);
    }

    #[test]
    fn liquids_are_recognised_for_the_warning() {
        assert!(is_liquid("liquids/water"));
        assert!(is_liquid("common/lava"));
        assert!(!is_liquid("common/caulk"));
    }
}
