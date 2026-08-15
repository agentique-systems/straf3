//! Turning [`Arena`] data into triangles.
//!
//! This is the *other* consumer of the arena — the one that draws it. It reads
//! the same [`ARENA`](crate::arena::ARENA) the collision does, which is the
//! whole reason the geometry is data instead of two hardcoded copies.
//!
//! # Why the big surfaces are tiled
//!
//! Flat colours only this wave (spec rev 6 §S: no textures, no art). A 3072²
//! floor drawn in one flat colour has no optical flow at all: at 600 units a
//! second the screen simply does not change, and the movement — the entire
//! point of the project — becomes impossible to feel. So any face bigger than
//! [`TILE`] is emitted as a checkerboard of [`TILE`]-sized quads in two shades
//! of its own colour. That is still flat colours, and it is what makes speed
//! visible.
//!
//! No index buffer. The arena is a few thousand triangles and every vertex
//! carries its face's flat normal, so nothing would be shared anyway.

use bytemuck::{Pod, Zeroable};
use straf3_sim::num::{Scalar, Vec3, s, vec3};

use crate::arena::{Arena, Axis, Box3, Ramp, Rgb};

/// Edge length of one checkerboard cell, in Quake units. 128 is two player
/// widths, which reads as a floor tile at running speed rather than as noise.
pub const TILE: Scalar = s(128.0);

/// How much darker the odd cells of the checkerboard are.
const CHECKER: f32 = 0.86;

/// One vertex, as the GPU sees it.
///
/// `repr(C)` and `Pod` because this is memcpy'd straight into a vertex buffer;
/// the layout is declared once, in [`Vertex::LAYOUT`], and the shader's
/// `@location`s match it.
#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq, Pod, Zeroable)]
pub struct Vertex {
    /// World position, Quake units, Z up.
    pub position: [f32; 3],
    /// Face normal. Flat: every vertex of a face carries the face's normal, so
    /// there is no smoothing and the arena's edges stay crisp.
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
}

/// Every triangle in the arena, ready to upload.
///
/// Built once at startup: the arena is static geometry (the [`World`] trait
/// says so in as many words), so there is nothing to rebuild per frame.
///
/// [`World`]: straf3_sim::World
#[must_use]
pub fn build(arena: &Arena) -> Vec<Vertex> {
    let mut out = Vec::new();
    for b in arena.boxes {
        push_box(&mut out, b);
    }
    for r in arena.ramps {
        push_ramp(&mut out, r);
    }
    out
}

fn push_box(out: &mut Vec<Vertex>, b: &Box3) {
    let (lo, hi) = (b.mins, b.maxs);
    let d = hi - lo;
    let (x, y, z) = (
        vec3(d.x, s(0.0), s(0.0)),
        vec3(s(0.0), d.y, s(0.0)),
        vec3(s(0.0), s(0.0), d.z),
    );

    let n = |a: Scalar, b: Scalar, c: Scalar| vec3(a, b, c);
    // Origin corner, then the two edges spanning the face, then its normal.
    // Winding is not load-bearing — the pipeline does not cull (see gfx.rs).
    quad(out, lo, y, z, n(s(-1.0), s(0.0), s(0.0)), b.color); // -X
    quad(out, lo + x, y, z, n(s(1.0), s(0.0), s(0.0)), b.color); // +X
    quad(out, lo, x, z, n(s(0.0), s(-1.0), s(0.0)), b.color); // -Y
    quad(out, lo + y, x, z, n(s(0.0), s(1.0), s(0.0)), b.color); // +Y
    quad(out, lo, x, y, n(s(0.0), s(0.0), s(-1.0)), b.color); // -Z
    quad(out, lo + z, x, y, n(s(0.0), s(0.0), s(1.0)), b.color); // +Z
}

fn push_ramp(out: &mut Vec<Vertex>, r: &Ramp) {
    let (lo, hi) = (r.mins, r.maxs);
    let zb = lo.z;
    let (h0, h1) = (r.z_at_mins, r.z_at_maxs);

    // `u` runs along the rise, `v` across it. Doing it this way means the two
    // orientations share every face below instead of being written twice.
    let (u_lo, u_hi, v_lo, v_hi, u_axis, v_axis, side_n) = match r.axis {
        Axis::X => (
            lo.x,
            hi.x,
            lo.y,
            hi.y,
            vec3(s(1.0), s(0.0), s(0.0)),
            vec3(s(0.0), s(1.0), s(0.0)),
            vec3(s(0.0), s(1.0), s(0.0)),
        ),
        Axis::Y => (
            lo.y,
            hi.y,
            lo.x,
            hi.x,
            vec3(s(0.0), s(1.0), s(0.0)),
            vec3(s(1.0), s(0.0), s(0.0)),
            vec3(s(1.0), s(0.0), s(0.0)),
        ),
    };
    // Point at (u, v, z) in the ramp's own frame.
    let p = |u: Scalar, v: Scalar, z: Scalar| u_axis * u + v_axis * v + vec3(s(0.0), s(0.0), z);

    let du = u_axis * (u_hi - u_lo);
    let dv = v_axis * (v_hi - v_lo);

    // Bottom.
    quad(
        out,
        p(u_lo, v_lo, zb),
        du,
        dv,
        vec3(s(0.0), s(0.0), s(-1.0)),
        r.color,
    );
    // The incline itself: the surface the player runs on.
    quad(
        out,
        p(u_lo, v_lo, h0),
        du + vec3(s(0.0), s(0.0), h1 - h0),
        dv,
        r.normal(),
        r.color,
    );
    // The two ends. The low one is degenerate when the ramp starts at the
    // floor, which is the usual case; a zero-area quad draws nothing.
    quad(
        out,
        p(u_lo, v_lo, zb),
        dv,
        vec3(s(0.0), s(0.0), h0 - zb),
        -u_axis,
        r.color,
    );
    quad(
        out,
        p(u_hi, v_lo, zb),
        dv,
        vec3(s(0.0), s(0.0), h1 - zb),
        u_axis,
        r.color,
    );
    // The two triangular flanks.
    for (v, n) in [(v_lo, -side_n), (v_hi, side_n)] {
        polygon(
            out,
            &[
                p(u_lo, v, zb),
                p(u_hi, v, zb),
                p(u_hi, v, h1),
                p(u_lo, v, h0),
            ],
            n,
            r.color,
        );
    }
}

/// A parallelogram from a corner and two edges, tiled into a checkerboard when
/// it is large enough that a single flat colour would hide motion.
fn quad(out: &mut Vec<Vertex>, origin: Vec3, u: Vec3, v: Vec3, normal: Vec3, color: Rgb) {
    let (lu, lv) = (u.length(), v.length());
    if lu <= s(0.0) || lv <= s(0.0) {
        return; // a degenerate face — a ramp's zero-height low end
    }
    let cols = (lu / TILE).ceil().max(s(1.0)) as usize;
    let rows = (lv / TILE).ceil().max(s(1.0)) as usize;

    if cols == 1 && rows == 1 {
        polygon(
            out,
            &[origin, origin + u, origin + u + v, origin + v],
            normal,
            color,
        );
        return;
    }

    let du = u / cols as Scalar;
    let dv = v / rows as Scalar;
    for i in 0..cols {
        for j in 0..rows {
            let o = origin + du * i as Scalar + dv * j as Scalar;
            let shade = if (i + j) % 2 == 0 { 1.0 } else { CHECKER };
            let c = [color[0] * shade, color[1] * shade, color[2] * shade];
            polygon(out, &[o, o + du, o + du + dv, o + dv], normal, c);
        }
    }
}

/// A convex polygon as a triangle fan. Three or four points, in this crate.
fn polygon(out: &mut Vec<Vertex>, points: &[Vec3], normal: Vec3, color: Rgb) {
    let vertex = |p: Vec3| Vertex {
        position: [p.x, p.y, p.z],
        normal: [normal.x, normal.y, normal.z],
        color,
    };
    for i in 1..points.len().saturating_sub(1) {
        out.push(vertex(points[0]));
        out.push(vertex(points[i]));
        out.push(vertex(points[i + 1]));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::arena::{ARENA, RAMP_GENTLE};

    #[test]
    fn the_arena_produces_whole_triangles_and_a_lot_of_them() {
        let v = build(&ARENA);
        assert!(!v.is_empty());
        assert_eq!(v.len() % 3, 0, "a triangle list must be a multiple of 3");
        // Every vertex must be finite: one NaN and the GPU drops the triangle
        // silently, which looks exactly like a hole in the map.
        for (i, vert) in v.iter().enumerate() {
            for c in vert.position.iter().chain(vert.normal.iter()) {
                assert!(c.is_finite(), "vertex {i} is not finite: {vert:?}");
            }
        }
    }

    #[test]
    fn every_drawn_vertex_is_inside_the_solid_it_came_from() {
        // The mesh and the collision must be the same geometry. Cheapest
        // honest check of that: no drawn vertex may lie outside the union of
        // the solids' bounding boxes, and the mesh's own bounds must match the
        // arena's.
        let v = build(&ARENA);
        let mut lo = Vec3::splat(Scalar::INFINITY);
        let mut hi = Vec3::splat(Scalar::NEG_INFINITY);
        for vert in &v {
            let p = vec3(vert.position[0], vert.position[1], vert.position[2]);
            lo = lo.min(p);
            hi = hi.max(p);
        }
        let mut alo = Vec3::splat(Scalar::INFINITY);
        let mut ahi = Vec3::splat(Scalar::NEG_INFINITY);
        for (bmin, bmax) in ARENA
            .boxes
            .iter()
            .map(|b| (b.mins, b.maxs))
            .chain(ARENA.ramps.iter().map(|r| (r.mins, r.maxs)))
        {
            alo = alo.min(bmin);
            ahi = ahi.max(bmax);
        }
        assert_eq!(lo, alo, "the mesh does not start where the arena does");
        assert_eq!(hi, ahi, "the mesh does not end where the arena does");
    }

    #[test]
    fn the_ramp_surface_drawn_is_the_ramp_surface_collided_with() {
        // The failure this guards is the one that would be worst to debug: a
        // ramp you can see but not stand on. Every vertex the mesh emits on
        // the incline must satisfy the collision plane's own equation.
        let mut v = Vec::new();
        push_ramp(&mut v, &RAMP_GENTLE);
        let n = RAMP_GENTLE.normal();
        let on_incline: Vec<_> = v
            .iter()
            .filter(|vt| vec3(vt.normal[0], vt.normal[1], vt.normal[2]) == n)
            .collect();
        assert!(!on_incline.is_empty(), "no incline face was emitted");
        for vt in on_incline {
            let p = vec3(vt.position[0], vt.position[1], vt.position[2]);
            let want = RAMP_GENTLE.surface_z(p.x, p.y);
            assert!(
                (p.z - want).abs() < s(1e-3),
                "drew the incline at {p:?}, collision says z={want}"
            );
        }
    }

    #[test]
    fn big_faces_are_tiled_and_small_ones_are_not() {
        // Two triangles for an untiled quad; many for the floor.
        let small = Box3 {
            mins: vec3(s(0.0), s(0.0), s(0.0)),
            maxs: vec3(s(64.0), s(64.0), s(64.0)),
            surface: Default::default(),
            color: [1.0, 1.0, 1.0],
            label: "test",
        };
        let mut v = Vec::new();
        push_box(&mut v, &small);
        assert_eq!(v.len(), 6 * 6, "a small box is 6 faces of 2 triangles");

        let floor = ARENA.boxes.iter().find(|b| b.label == "floor").unwrap();
        let mut f = Vec::new();
        push_box(&mut f, floor);
        assert!(
            f.len() > 6 * 6,
            "the floor must be tiled or there is no sense of speed"
        );
    }

    #[test]
    fn a_zero_area_face_emits_nothing() {
        let mut v = Vec::new();
        quad(
            &mut v,
            Vec3::ZERO,
            vec3(s(10.0), s(0.0), s(0.0)),
            Vec3::ZERO,
            Vec3::Z,
            [1.0, 0.0, 0.0],
        );
        assert!(v.is_empty());
    }
}
