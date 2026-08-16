//! A compiled map's triangles, as the GPU wants them.
//!
//! # Why this file is now almost nothing
//!
//! It used to build triangles out of the hardcoded arena — the *other* consumer
//! of geometry that also had a collision implementation reading the same data.
//! That arrangement is retired. `straf3-map` now emits the hulls and the
//! triangles from one pass over one set of brush planes, below the seam, so
//! there is no longer a second construction of the world up here that could
//! disagree with the first.
//!
//! What is left is the part that genuinely belongs above the line: `wgpu`'s
//! vertex layout. [`straf3_map::MeshVertex`] is `repr(C)` and declares its
//! fields in exactly this order for exactly this reason, so the conversion is a
//! field copy the optimiser flattens, not a rebuild.
//!
//! # Why the checkerboard moved into the shader
//!
//! Flat colours only (no textures this wave), and a large flat-coloured floor
//! has no optical flow at all: at 600 units a second the screen simply does not
//! change and the movement — the entire point of the project — becomes
//! impossible to feel. The old arena mesh solved that by *subdividing* big
//! faces into tiled quads.
//!
//! That does not survive contact with a real map. Subdividing to 128-unit cells
//! is bounded when the world is ten boxes; measured against real third-party
//! maps of 2,300 to 4,100 brushes it is not, and the triangle count runs away
//! on exactly the largest surfaces. So the checker is computed per *fragment*
//! from world position instead (see `shader.wgsl`). Same visual property, no
//! geometry cost, and it works on brush faces of any shape and orientation
//! rather than only on axis-aligned quads.

use bytemuck::{Pod, Zeroable};
use straf3_map::MeshVertex;

/// One vertex, as the GPU sees it.
///
/// `repr(C)` and `Pod` because this is memcpy'd straight into a vertex buffer;
/// the layout is declared once, in [`Vertex::ATTRIBUTES`], and the shader's
/// `@location`s match it.
#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq, Pod, Zeroable)]
pub struct Vertex {
    /// World position, Quake units, Z up.
    pub position: [f32; 3],
    /// Face normal. Flat: every vertex of a face carries the face's normal, so
    /// there is no smoothing and brush edges stay crisp.
    pub normal: [f32; 3],
    /// Flat colour, linear RGB.
    pub color: [f32; 3],
}

impl Vertex {
    /// Vertex attribute layout, matching `shader.wgsl`'s `@location` bindings.
    pub const ATTRIBUTES: [wgpu::VertexAttribute; 3] =
        wgpu::vertex_attr_array![0 => Float32x3, 1 => Float32x3, 2 => Float32x3];

    /// The buffer layout to hand to the pipeline.
    #[must_use]
    pub const fn layout() -> wgpu::VertexBufferLayout<'static> {
        wgpu::VertexBufferLayout {
            array_stride: core::mem::size_of::<Self>() as wgpu::BufferAddress,
            step_mode: wgpu::VertexStepMode::Vertex,
            attributes: &Self::ATTRIBUTES,
        }
    }

    /// The same vertex the map compiler produced.
    #[must_use]
    pub const fn from_map(v: &MeshVertex) -> Self {
        Self {
            position: v.position,
            normal: v.normal,
            color: v.color,
        }
    }
}

/// A map's triangles, indexed, ready to upload.
///
/// Built once at load: a map is static geometry (the [`World`] trait says so in
/// as many words), so there is nothing to rebuild per frame.
///
/// [`World`]: straf3_sim::World
#[derive(Debug, Clone, Default, PartialEq)]
pub struct GpuMesh {
    /// Vertices, in face order.
    pub vertices: Vec<Vertex>,
    /// Triangle indices, three per triangle.
    pub indices: Vec<u32>,
}

impl GpuMesh {
    /// Convert a compiled map's mesh.
    #[must_use]
    pub fn from_map(mesh: &straf3_map::Mesh) -> Self {
        Self {
            vertices: mesh.vertices.iter().map(Vertex::from_map).collect(),
            indices: mesh.indices.clone(),
        }
    }

    /// Nothing to draw. What a session in the flat or empty world uploads —
    /// those worlds have no geometry, and the renderer must still start.
    #[must_use]
    pub fn empty() -> Self {
        Self::default()
    }

    /// Number of triangles.
    #[must_use]
    pub fn triangle_count(&self) -> usize {
        self.indices.len() / 3
    }

    /// Whether there is anything at all to draw.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.indices.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The course this repository ships. `include_str!` rather than a path read
    /// so the test cannot pass against a map that is not committed.
    const COIL: &str = include_str!("../../../assets/maps/coil.map");

    #[test]
    fn the_gpu_vertex_is_layout_compatible_with_the_compilers() {
        // The claim `straf3_map::MeshVertex`'s own doc comment makes, asserted
        // rather than trusted: if these ever diverge, the conversion silently
        // becomes a reshuffle and every normal in the world points sideways.
        assert_eq!(
            core::mem::size_of::<Vertex>(),
            core::mem::size_of::<MeshVertex>()
        );
        assert_eq!(
            core::mem::align_of::<Vertex>(),
            core::mem::align_of::<MeshVertex>()
        );
        assert_eq!(core::mem::size_of::<Vertex>(), 9 * 4);
    }

    #[test]
    fn the_shipped_course_becomes_whole_triangles_and_a_lot_of_them() {
        let map = straf3_map::compile(COIL).expect("coil.map compiles");
        let gpu = GpuMesh::from_map(&map.mesh);
        assert!(!gpu.is_empty());
        assert_eq!(
            gpu.indices.len() % 3,
            0,
            "a triangle list must be a multiple of 3"
        );
        assert_eq!(gpu.triangle_count(), map.mesh.triangle_count());

        // Every vertex must be finite: one NaN and the GPU drops the triangle
        // silently, which looks exactly like a hole in the map.
        for (i, v) in gpu.vertices.iter().enumerate() {
            for c in v.position.iter().chain(v.normal.iter()) {
                assert!(c.is_finite(), "vertex {i} is not finite: {v:?}");
            }
        }
        // And every index must address a vertex that exists.
        for &i in &gpu.indices {
            assert!(
                (i as usize) < gpu.vertices.len(),
                "index {i} is past the end of {} vertices",
                gpu.vertices.len()
            );
        }
    }

    #[test]
    fn what_is_drawn_is_inside_what_is_collided_with() {
        // The mesh and the collision must be the same geometry. Cheapest honest
        // check of that: no drawn vertex may lie outside the bounds the
        // collision hulls report. This is the invisible-wall test — if the two
        // ever came from different passes, this is what would catch it first.
        let map = straf3_map::compile(COIL).unwrap();
        let gpu = GpuMesh::from_map(&map.mesh);
        let (lo, hi) = (map.bounds.mins, map.bounds.maxs);
        for v in &gpu.vertices {
            for axis in 0..3 {
                assert!(
                    v.position[axis] >= lo[axis] - 0.01 && v.position[axis] <= hi[axis] + 0.01,
                    "drew a vertex at {:?}, outside the collision bounds {lo:?}..{hi:?}",
                    v.position
                );
            }
        }
    }

    #[test]
    fn every_normal_is_a_unit_vector() {
        // A non-unit normal is not a crash, it is a face that lights wrongly —
        // and it is the signature of a plane that was never normalised, which
        // *would* be a collision bug.
        let map = straf3_map::compile(COIL).unwrap();
        for v in GpuMesh::from_map(&map.mesh).vertices {
            let n = v.normal;
            let len2 = n[0] * n[0] + n[1] * n[1] + n[2] * n[2];
            assert!(
                (len2 - 1.0).abs() < 1e-3,
                "normal {n:?} has length squared {len2}"
            );
        }
    }

    #[test]
    fn a_world_with_no_map_still_produces_an_uploadable_mesh() {
        let m = GpuMesh::empty();
        assert!(m.is_empty());
        assert_eq!(m.triangle_count(), 0);
    }
}
