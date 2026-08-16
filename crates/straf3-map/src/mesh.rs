//! The triangles a compiled map draws as.
//!
//! # Why a renderer's data structure lives below the seam
//!
//! Because the alternative is two copies of the geometry. `arena.rs` states the
//! case in its own first paragraph: the thing the player collides with and the
//! thing the player sees must be the *same* geometry, or the failure mode is an
//! invisible wall or a ramp you fall through. A compiled map is that argument
//! at map scale — the hulls and the triangles come out of one pass over one set
//! of brush planes, and there is no second parse that could drift.
//!
//! Nothing here knows what a GPU is. [`MeshVertex`] is three arrays of floats,
//! deliberately laid out in the order `straf3-render`'s own vertex already
//! uses, so the crate above the seam turns this into a vertex buffer with a
//! `map`, and the seam check keeps finding no `wgpu` below the line.
//!
//! # Why flat colours from a hash
//!
//! Textures are out of scope this wave — C7 says so and flags why (the shader
//! sets live in `.pk3`s whose redistribution rights are unresolved). So a face
//! is drawn in a flat colour derived from its shader *name*: every brick face
//! in the map gets one colour and every metal face another, deterministically,
//! with no asset needed. It reads as a level rather than as one grey mass, and
//! it costs one hash per face.

use glam::Vec3;

use crate::digest::Fnv1a;

/// One vertex of the compiled render mesh.
///
/// `repr(C)` and field order chosen to match `straf3_render::mesh::Vertex`
/// exactly — position, normal, colour, all `[f32; 3]` — so the crate above the
/// seam can reinterpret rather than rebuild.
#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct MeshVertex {
    /// World position, Quake units, Z up.
    pub position: [f32; 3],
    /// The face's plane normal. Flat: every vertex of a face carries the same
    /// one, so brush edges stay crisp instead of being smoothed into a blob.
    pub normal: [f32; 3],
    /// Flat colour, linear RGB in `0.0..=1.0`.
    pub color: [f32; 3],
}

/// Every triangle of a compiled map.
///
/// Indexed, unlike the hardcoded arena's mesh: a real map is tens of thousands
/// of triangles rather than a few thousand, and every face's vertices are
/// shared by its own fan even though nothing is shared between faces (flat
/// normals make that impossible, and desirable).
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Mesh {
    /// Vertices, in face order.
    pub vertices: Vec<MeshVertex>,
    /// Triangle indices, three per triangle.
    pub indices: Vec<u32>,
}

impl Mesh {
    /// Add one convex face as a triangle fan.
    ///
    /// A fan is correct for any convex polygon, and every polygon here is
    /// convex by construction — it is the intersection of half-spaces with a
    /// plane. The winding is the source winding, which is clockwise seen from
    /// outside; `straf3-render`'s pipeline does not cull (see `gfx.rs`), so
    /// this is documentation rather than a requirement, and it stays correct if
    /// culling is ever turned on.
    pub(crate) fn push_face(&mut self, points: &[Vec3], normal: Vec3, color: [f32; 3]) {
        if points.len() < 3 {
            return;
        }
        let base = self.vertices.len() as u32;
        let normal = normal.to_array();
        for p in points {
            self.vertices.push(MeshVertex {
                position: snap_to_grid(*p).to_array(),
                normal,
                color,
            });
        }
        for i in 1..points.len() as u32 - 1 {
            self.indices.push(base);
            self.indices.push(base + i);
            self.indices.push(base + i + 1);
        }
    }

    /// Number of triangles.
    #[must_use]
    pub fn triangle_count(&self) -> usize {
        self.indices.len() / 3
    }

    pub(crate) fn fold(&self, h: &mut Fnv1a) {
        h.len(self.vertices.len());
        for v in &self.vertices {
            for c in v.position {
                h.f32(c);
            }
            for c in v.normal {
                h.f32(c);
            }
            for c in v.color {
                h.f32(c);
            }
        }
        h.len(self.indices.len());
        for i in &self.indices {
            h.u32(*i);
        }
    }
}

/// How far a vertex may be from a whole number and still be put on it.
///
/// `q3map`'s distance epsilon, and the same value `straf3-collision`'s
/// [`Plane::snapped`](straf3_collision::Plane::snapped) uses for plane
/// distances — different quantity, same reasoning.
const GRID_EPSILON: f32 = 0.01;

/// Put a render vertex back on the grid it was authored on.
///
/// **Render only.** The collision hull is defined by planes and never by these
/// points, so nothing here can move a wall.
///
/// It is worth doing because the face polygons are clipped in `f32` at map
/// scale, where one ulp near ±65536 is already about `0.0078`. Two floor
/// brushes that share an edge therefore produce corner coordinates that should
/// be identical and differ in the last bits, and the difference draws as a
/// hairline crack of background colour along every seam in the level.
fn snap_to_grid(p: Vec3) -> Vec3 {
    let snap = |v: f32| {
        let r = v.round();
        if (v - r).abs() < GRID_EPSILON { r } else { v }
    };
    Vec3::new(snap(p.x), snap(p.y), snap(p.z))
}

/// The palette faces are coloured from.
///
/// Muted and low-contrast on purpose: these are stand-ins for textures, and a
/// map drawn in saturated primaries is harder to read the geometry of, not
/// easier. The values are the same family `arena.rs` picked by hand.
const PALETTE: [[f32; 3]; 12] = [
    [0.42, 0.44, 0.47],
    [0.30, 0.31, 0.34],
    [0.62, 0.34, 0.22],
    [0.20, 0.48, 0.50],
    [0.62, 0.55, 0.36],
    [0.40, 0.24, 0.42],
    [0.34, 0.42, 0.30],
    [0.52, 0.48, 0.44],
    [0.26, 0.36, 0.46],
    [0.56, 0.42, 0.30],
    [0.38, 0.38, 0.42],
    [0.46, 0.30, 0.30],
];

/// A stable colour for a shader name.
///
/// Deterministic in the sense C7 requires: the same name gives the same colour
/// on every target and in every run, because it is a fold over bytes and an
/// index into a fixed table — no floating point, no hash-map iteration, no
/// randomness.
#[must_use]
pub fn color_for(texture: &str) -> [f32; 3] {
    let mut h = Fnv1a::new();
    h.bytes(crate::texture::base_name(texture).as_bytes());
    // The low bits of an FNV digest are the well-mixed ones.
    PALETTE[(h.finish() % PALETTE.len() as u64) as usize]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_quad_becomes_two_triangles() {
        let mut m = Mesh::default();
        m.push_face(
            &[
                Vec3::new(0.0, 0.0, 0.0),
                Vec3::new(64.0, 0.0, 0.0),
                Vec3::new(64.0, 64.0, 0.0),
                Vec3::new(0.0, 64.0, 0.0),
            ],
            Vec3::Z,
            [0.5, 0.5, 0.5],
        );
        assert_eq!(m.vertices.len(), 4);
        assert_eq!(m.triangle_count(), 2);
        assert_eq!(m.indices, vec![0, 1, 2, 0, 2, 3]);
        assert!(m.vertices.iter().all(|v| v.normal == [0.0, 0.0, 1.0]));
    }

    #[test]
    fn a_degenerate_face_contributes_nothing() {
        let mut m = Mesh::default();
        m.push_face(&[Vec3::ZERO, Vec3::X], Vec3::Z, [0.0; 3]);
        assert!(m.vertices.is_empty() && m.indices.is_empty());
    }

    #[test]
    fn faces_after_the_first_index_from_their_own_base() {
        let tri = [Vec3::ZERO, Vec3::X, Vec3::Y];
        let mut m = Mesh::default();
        m.push_face(&tri, Vec3::Z, [0.0; 3]);
        m.push_face(&tri, Vec3::Z, [0.0; 3]);
        assert_eq!(m.indices, vec![0, 1, 2, 3, 4, 5]);
    }

    #[test]
    fn a_vertex_a_hair_off_the_grid_is_put_back_on_it() {
        // The crack-along-every-seam case: clipping at map scale lands a corner
        // a few ulps off the integer its author typed.
        assert_eq!(
            snap_to_grid(Vec3::new(63.999_996, -128.000_01, 0.5)),
            Vec3::new(64.0, -128.0, 0.5)
        );
        // …and geometry that is genuinely off-grid stays where it is.
        let off = Vec3::new(63.5, -128.25, 0.75);
        assert_eq!(snap_to_grid(off), off);
    }

    #[test]
    fn a_shader_name_always_gets_the_same_colour() {
        assert_eq!(
            color_for("base_wall/concrete"),
            color_for("base_wall/concrete")
        );
        // Path prefix and case are not part of the identity, same as the rest
        // of the compiler's shader handling.
        assert_eq!(
            color_for("textures/base_wall/concrete"),
            color_for("BASE_WALL/Concrete")
        );
        assert!(PALETTE.contains(&color_for("anything at all")));
    }

    #[test]
    fn different_shaders_generally_get_different_colours() {
        // Not a guarantee — twelve colours collide — but the map must not come
        // out one flat shade.
        let names = [
            "base_floor/clang_floor",
            "base_wall/concrete",
            "gothic_block/blocks18c",
            "liquids/proto_grue",
            "base_trim/pewter_shiny",
        ];
        let distinct: std::collections::BTreeSet<_> = names
            .iter()
            .map(|n| color_for(n).map(f32::to_bits))
            .collect();
        assert!(distinct.len() >= 3, "palette is not spreading these out");
    }
}
