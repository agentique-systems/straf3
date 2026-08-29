//! Writing `.map` text: boxes, entities, and the winding rule that decides
//! whether a floor is a floor.
//!
//! # Why this is a module rather than a test helper
//!
//! These functions began inside `course::tests` as a way to invent fixtures the
//! derivation could be tested against. [`crate::fixture`] needs exactly the same
//! primitives to generate a committed `.map`, and a shipped generator that
//! duplicated a test's geometry helpers would be two implementations of the
//! winding rule below — which is the one part of writing a `.map` that fails
//! silently rather than loudly. So there is one copy, and both callers use it.
//!
//! # The winding rule, which is the whole difficulty
//!
//! A `.map` face is three points, and Quake takes its plane normal as
//! `(p0 - p1) × (p2 - p1)`. Get the order wrong and the brush is inside out: the
//! compiler either rejects it as degenerate or, worse, accepts a solid whose
//! faces point inward and which a player falls straight through.
//!
//! [`face`] derives the winding from the outward normal instead of hand-writing
//! it. For a face at `o` spanning in-plane directions `u` and `v` it emits
//! `o+u, o, o+v`, which makes the normal `u × v`. The six faces of a box then
//! pick their `u`/`v` from `x × y = z`, `y × z = x`, `z × x = y` and the three
//! negatives, which are the same pairs swapped. Nothing has to be remembered.

/// The three positive axis vectors, at the 64-unit scale the face points use.
const AXIS: [[i32; 3]; 3] = [[64, 0, 0], [0, 64, 0], [0, 0, 64]];

/// One `.map` face: three points, a texture, and a default texture alignment.
///
/// `o` is a corner of the face and `u`, `v` span it. The outward normal is
/// `u × v` — see the module docs for why that is the only thing worth knowing
/// here.
#[must_use]
pub fn face(o: [i32; 3], u: [i32; 3], v: [i32; 3], texture: &str) -> String {
    let p =
        |a: [i32; 3], b: [i32; 3]| format!("( {} {} {} )", a[0] + b[0], a[1] + b[1], a[2] + b[2]);
    format!(
        "{} {} {} {texture} [ 1 0 0 0 ] [ 0 -1 0 0 ] 0 0.5 0.5\n",
        p(o, u),
        p(o, [0, 0, 0]),
        p(o, v),
    )
}

/// An axis-aligned box brush spanning `mins..maxs`.
///
/// Each of the six faces takes its winding from [`face`], so a box written with
/// `mins` and `maxs` the wrong way round produces a degenerate brush the
/// compiler rejects rather than a solid that quietly faces inward.
#[must_use]
pub fn box_brush(mins: [i32; 3], maxs: [i32; 3], texture: &str) -> String {
    let [x, y, z] = AXIS;
    let lo = mins;
    let hi_x = [maxs[0], mins[1], mins[2]];
    let hi_y = [mins[0], maxs[1], mins[2]];
    let hi_z = [mins[0], mins[1], maxs[2]];

    let mut out = String::from("{\n");
    out.push_str(&face(lo, z, y, texture)); // -X
    out.push_str(&face(hi_x, y, z, texture)); // +X
    out.push_str(&face(lo, x, z, texture)); // -Y
    out.push_str(&face(hi_y, z, x, texture)); // +Y
    out.push_str(&face(lo, y, x, texture)); // -Z
    out.push_str(&face(hi_z, x, y, texture)); // +Z
    out.push_str("}\n");
    out
}

/// A point entity: a classname and its keys.
#[must_use]
pub fn point_entity(classname: &str, keys: &[(&str, &str)]) -> String {
    let mut out = format!("{{\n\"classname\" \"{classname}\"\n");
    for (k, v) in keys {
        out.push_str(&format!("\"{k}\" \"{v}\"\n"));
    }
    out.push_str("}\n");
    out
}

/// A brush entity: a classname, its keys, and the brushes it owns.
#[must_use]
pub fn brush_entity(classname: &str, keys: &[(&str, &str)], brushes: &[String]) -> String {
    let mut out = format!("{{\n\"classname\" \"{classname}\"\n");
    for (k, v) in keys {
        out.push_str(&format!("\"{k}\" \"{v}\"\n"));
    }
    for b in brushes {
        out.push_str(b);
    }
    out.push_str("}\n");
    out
}

/// A timing trigger in the two-part shape Defrag maps use: a point entity
/// carrying the meaning, and a `trigger_multiple` brush entity that targets it.
#[must_use]
pub fn timed_trigger(target_classname: &str, name: &str, brushes: &[String]) -> String {
    timed_trigger_with(target_classname, name, &[], brushes)
}

/// The same, with extra keys on the point entity — used to author the `count`
/// key both shipped maps carry and nothing reads.
#[must_use]
pub fn timed_trigger_with(
    target_classname: &str,
    name: &str,
    extra: &[(&str, &str)],
    brushes: &[String],
) -> String {
    let mut keys: Vec<(&str, &str)> = vec![("targetname", name), ("origin", "0 0 32")];
    keys.extend_from_slice(extra);
    let mut out = point_entity(target_classname, &keys);
    out.push_str(&brush_entity(
        "trigger_multiple",
        &[("target", name)],
        brushes,
    ));
    out
}
