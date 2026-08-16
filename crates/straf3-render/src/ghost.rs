//! Drawing a recorded run's player beside the live one.
//!
//! # What a ghost is, and what this file is allowed to assume about it
//!
//! A ghost is a *re-simulation*. `straf3-game` runs the recorded command
//! stream through the same `straf3-sim` the live player runs through, and what
//! arrives here is the state that came out of it — one pose per frame. There
//! is no interpolation of a recorded position track anywhere in this crate,
//! because there is no recorded position track: the `.s3d` file stores
//! commands. This module therefore knows nothing about time, runs or replay.
//! It is handed a box and draws it.
//!
//! # Why the box is the collision hull
//!
//! The ghost is drawn at the size the recorded player actually collided as —
//! [`GhostPose`] carries the hull rather than a decorative model size — so a
//! crouched ghost is a short box, and a gap the ghost fits through is a gap
//! you can see it fit through. A ghost drawn at a size nobody collided at
//! would be a lie in exactly the situation where you are looking hardest at
//! it: a near miss.

use bytemuck::{Pod, Zeroable};
use straf3_sim::num::{Scalar, Vec3};

use crate::camera::Camera;

/// Where the recorded player is this frame, and how big they are.
///
/// Built by `straf3-game` from a re-simulated [`PlayerState`], already
/// interpolated between two simulation states, because the caller is the one
/// that knows how the ghost's clock lines up with the live player's.
///
/// [`PlayerState`]: straf3_sim::PlayerState
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct GhostPose {
    /// The recorded player's origin, in world units.
    pub origin: Vec3,
    /// Where they were looking, in degrees, about Z.
    pub yaw: Scalar,
    /// Half extents of the hull they collided with.
    pub half_extents: Vec3,
    /// The hull's centre, relative to the origin.
    pub center_offset: Vec3,
}

/// The ghost's colour, linear RGB, and the alpha its body is blended at.
///
/// Cyan-white rather than a second player colour: it has to be identifiable as
/// *not you* at a glance and at speed, and it must not be mistaken for map
/// geometry. The alpha is the number to argue with — high enough to follow
/// through a corner, low enough that it never hides the wall behind it.
pub const GHOST_COLOR: [f32; 4] = [0.42, 0.82, 1.0, 0.34];

/// The uniform block, matching `ghost.wgsl`'s `Ghost`.
#[repr(C)]
#[derive(Debug, Clone, Copy, Pod, Zeroable)]
struct GhostUniform {
    view_proj: [[f32; 4]; 4],
    origin: [f32; 4],
    half_extents: [f32; 4],
    center_offset: [f32; 4],
    /// cos(yaw), sin(yaw), unused, unused.
    basis: [f32; 4],
    color: [f32; 4],
}

/// One vertex of the unit cube. Position and normal only — the colour is the
/// ghost's, not the geometry's.
#[repr(C)]
#[derive(Debug, Clone, Copy, Pod, Zeroable)]
struct GhostVertex {
    position: [f32; 3],
    normal: [f32; 3],
}

impl GhostVertex {
    const ATTRIBUTES: [wgpu::VertexAttribute; 2] =
        wgpu::vertex_attr_array![0 => Float32x3, 1 => Float32x3];

    const fn layout() -> wgpu::VertexBufferLayout<'static> {
        wgpu::VertexBufferLayout {
            array_stride: core::mem::size_of::<Self>() as wgpu::BufferAddress,
            step_mode: wgpu::VertexStepMode::Vertex,
            attributes: &Self::ATTRIBUTES,
        }
    }
}

/// The ±1 cube, as six independent quads so every face carries its own normal.
///
/// Flat faces, not a smoothed cube: the hull *is* a box, and a smoothed one
/// would read as a capsule — which is precisely the shape Q3 movement does not
/// have.
fn cube() -> (Vec<GhostVertex>, Vec<u32>) {
    // (normal, the face's four corners in counter-clockwise order seen from
    // outside — the pipeline culls back faces, so the winding is what stops
    // the far side of the box blending over the near side.)
    const FACES: [([f32; 3], [[f32; 3]; 4]); 6] = [
        (
            [0.0, 0.0, 1.0],
            [
                [-1.0, -1.0, 1.0],
                [1.0, -1.0, 1.0],
                [1.0, 1.0, 1.0],
                [-1.0, 1.0, 1.0],
            ],
        ),
        (
            [0.0, 0.0, -1.0],
            [
                [-1.0, 1.0, -1.0],
                [1.0, 1.0, -1.0],
                [1.0, -1.0, -1.0],
                [-1.0, -1.0, -1.0],
            ],
        ),
        (
            [1.0, 0.0, 0.0],
            [
                [1.0, -1.0, -1.0],
                [1.0, 1.0, -1.0],
                [1.0, 1.0, 1.0],
                [1.0, -1.0, 1.0],
            ],
        ),
        (
            [-1.0, 0.0, 0.0],
            [
                [-1.0, 1.0, -1.0],
                [-1.0, -1.0, -1.0],
                [-1.0, -1.0, 1.0],
                [-1.0, 1.0, 1.0],
            ],
        ),
        (
            [0.0, 1.0, 0.0],
            [
                [1.0, 1.0, -1.0],
                [-1.0, 1.0, -1.0],
                [-1.0, 1.0, 1.0],
                [1.0, 1.0, 1.0],
            ],
        ),
        (
            [0.0, -1.0, 0.0],
            [
                [-1.0, -1.0, -1.0],
                [1.0, -1.0, -1.0],
                [1.0, -1.0, 1.0],
                [-1.0, -1.0, 1.0],
            ],
        ),
    ];

    let mut vertices = Vec::with_capacity(24);
    let mut indices = Vec::with_capacity(36);
    for (normal, corners) in FACES {
        let base = vertices.len() as u32;
        for position in corners {
            vertices.push(GhostVertex { position, normal });
        }
        indices.extend_from_slice(&[base, base + 1, base + 2, base, base + 2, base + 3]);
    }
    (vertices, indices)
}

/// The half-drawn frame the ghost is recorded into.
pub(crate) struct GhostFrame<'a> {
    pub queue: &'a wgpu::Queue,
    pub encoder: &'a mut wgpu::CommandEncoder,
    /// The colour target, already holding the world.
    pub target: &'a wgpu::TextureView,
    /// The depth the world left behind, so the ghost is hidden by walls.
    pub depth: &'a wgpu::TextureView,
    pub camera: &'a Camera,
    /// Width over height, for the projection.
    pub aspect: Scalar,
}

/// The pipeline and the one box it draws.
///
/// Built once with the [`Scene`](crate::Scene), whether or not a ghost is ever
/// loaded: a cube is 24 vertices, and a pipeline that only exists when a
/// personal best happens to be on disk is a pipeline that is first compiled at
/// the moment somebody beats their time.
pub(crate) struct GhostPipeline {
    pipeline: wgpu::RenderPipeline,
    vertices: wgpu::Buffer,
    indices: wgpu::Buffer,
    index_count: u32,
    uniform: wgpu::Buffer,
    bind_group: wgpu::BindGroup,
}

impl GhostPipeline {
    pub(crate) fn new(
        device: &wgpu::Device,
        format: wgpu::TextureFormat,
        depth_format: wgpu::TextureFormat,
    ) -> Self {
        use wgpu::util::DeviceExt as _;

        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("straf3-ghost"),
            source: wgpu::ShaderSource::Wgsl(include_str!("ghost.wgsl").into()),
        });

        let bind_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("straf3-ghost-layout"),
            entries: &[wgpu::BindGroupLayoutEntry {
                binding: 0,
                visibility: wgpu::ShaderStages::VERTEX_FRAGMENT,
                ty: wgpu::BindingType::Buffer {
                    ty: wgpu::BufferBindingType::Uniform,
                    has_dynamic_offset: false,
                    min_binding_size: None,
                },
                count: None,
            }],
        });

        let uniform = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("straf3-ghost-uniform"),
            size: core::mem::size_of::<GhostUniform>() as wgpu::BufferAddress,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });

        let bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("straf3-ghost"),
            layout: &bind_layout,
            entries: &[wgpu::BindGroupEntry {
                binding: 0,
                resource: uniform.as_entire_binding(),
            }],
        });

        let layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("straf3-ghost"),
            bind_group_layouts: &[Some(&bind_layout)],
            immediate_size: 0,
        });

        let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("straf3-ghost"),
            layout: Some(&layout),
            vertex: wgpu::VertexState {
                module: &shader,
                entry_point: Some("vs_main"),
                compilation_options: Default::default(),
                buffers: &[Some(GhostVertex::layout())],
            },
            primitive: wgpu::PrimitiveState {
                topology: wgpu::PrimitiveTopology::TriangleList,
                // Back faces culled, unlike the world pass. The world is opaque
                // and culling there would only risk hiding a mis-wound face;
                // here the box is translucent, and drawing both sides would
                // blend it twice and make it twice as solid as it should be.
                //
                // `Back` is measured, not assumed. A convex box has the same
                // silhouette whichever half survives, so a pixel count cannot
                // tell them apart — the shading can. The face towards this
                // example's camera has its normal pointing away from the fixed
                // light and so takes no diffuse term: culling `Back` gives mean
                // (126, 134, 136) over the ghost and culling `Front` gives
                // (131, 143, 145), so `Back` is the one keeping the faces you
                // are actually looking at. `ghost-offscreen` still prints that
                // mean, so the check can be repeated rather than believed.
                cull_mode: Some(wgpu::Face::Back),
                ..Default::default()
            },
            depth_stencil: Some(wgpu::DepthStencilState {
                format: depth_format,
                // Tested against the world, but not written: the ghost must be
                // hidden by a wall it is behind, and must not occlude anything
                // drawn after it.
                depth_write_enabled: Some(false),
                depth_compare: Some(wgpu::CompareFunction::Less),
                stencil: wgpu::StencilState::default(),
                bias: wgpu::DepthBiasState::default(),
            }),
            multisample: wgpu::MultisampleState::default(),
            fragment: Some(wgpu::FragmentState {
                module: &shader,
                entry_point: Some("fs_main"),
                compilation_options: Default::default(),
                targets: &[Some(wgpu::ColorTargetState {
                    format,
                    // Premultiplied alpha, matching what the shader emits.
                    blend: Some(wgpu::BlendState::PREMULTIPLIED_ALPHA_BLENDING),
                    write_mask: wgpu::ColorWrites::ALL,
                })],
            }),
            multiview_mask: None,
            cache: None,
        });

        let (verts, idx) = cube();
        let vertices = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("straf3-ghost-hull"),
            contents: bytemuck::cast_slice(&verts),
            usage: wgpu::BufferUsages::VERTEX,
        });
        let indices = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("straf3-ghost-hull-indices"),
            contents: bytemuck::cast_slice(&idx),
            usage: wgpu::BufferUsages::INDEX,
        });

        Self {
            pipeline,
            vertices,
            indices,
            index_count: idx.len() as u32,
            uniform,
            bind_group,
        }
    }

    /// Record the ghost into the frame, over a world that has already been
    /// drawn into it and whose depth is still there.
    pub(crate) fn draw(&self, frame: GhostFrame<'_>, pose: &GhostPose) {
        let GhostFrame {
            queue,
            encoder,
            target,
            depth,
            camera,
            aspect,
        } = frame;
        // sin/cos on the CPU, in degrees, matching the convention the rest of
        // the renderer reads yaw in. This is above the seam, so an ordinary
        // library trig call is fine here — the owned `sin_cos` that criterion 1
        // is about exists because the *simulation* may not disagree by a ULP
        // across targets, and nothing about a picture depends on that.
        let (sin, cos) = pose.yaw.to_radians().sin_cos();
        let uniform = GhostUniform {
            view_proj: camera.view_proj(aspect).to_cols_array_2d(),
            origin: [pose.origin.x, pose.origin.y, pose.origin.z, 1.0],
            half_extents: [
                pose.half_extents.x,
                pose.half_extents.y,
                pose.half_extents.z,
                0.0,
            ],
            center_offset: [
                pose.center_offset.x,
                pose.center_offset.y,
                pose.center_offset.z,
                0.0,
            ],
            basis: [cos, sin, 0.0, 0.0],
            color: GHOST_COLOR,
        };
        queue.write_buffer(&self.uniform, 0, bytemuck::bytes_of(&uniform));

        let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some("straf3-ghost"),
            color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                view: target,
                depth_slice: None,
                resolve_target: None,
                ops: wgpu::Operations {
                    // Load, emphatically: the world is already in there.
                    load: wgpu::LoadOp::Load,
                    store: wgpu::StoreOp::Store,
                },
            })],
            depth_stencil_attachment: Some(wgpu::RenderPassDepthStencilAttachment {
                view: depth,
                depth_ops: Some(wgpu::Operations {
                    load: wgpu::LoadOp::Load,
                    store: wgpu::StoreOp::Discard,
                }),
                stencil_ops: None,
            }),
            timestamp_writes: None,
            occlusion_query_set: None,
            multiview_mask: None,
        });
        pass.set_pipeline(&self.pipeline);
        pass.set_bind_group(0, &self.bind_group, &[]);
        pass.set_vertex_buffer(0, self.vertices.slice(..));
        pass.set_index_buffer(self.indices.slice(..), wgpu::IndexFormat::Uint32);
        pass.draw_indexed(0..self.index_count, 0, 0..1);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_hull_is_six_flat_faces_and_nothing_is_shared_between_them() {
        let (vertices, indices) = cube();
        assert_eq!(vertices.len(), 24, "shared corners would smooth the normals");
        assert_eq!(indices.len(), 36);
        for &i in &indices {
            assert!((i as usize) < vertices.len());
        }
    }

    #[test]
    fn every_face_is_wound_so_its_normal_points_out_of_the_box() {
        // Back-face culling is what stops the far side of a translucent box
        // blending over the near side, and culling with the winding backwards
        // shows the inside of the ghost instead of the outside — which looks
        // like a hole rather than like a bug.
        let (vertices, indices) = cube();
        for tri in indices.chunks(3) {
            let [a, b, c] = [
                vertices[tri[0] as usize].position,
                vertices[tri[1] as usize].position,
                vertices[tri[2] as usize].position,
            ];
            let ab = [b[0] - a[0], b[1] - a[1], b[2] - a[2]];
            let ac = [c[0] - a[0], c[1] - a[1], c[2] - a[2]];
            let cross = [
                ab[1] * ac[2] - ab[2] * ac[1],
                ab[2] * ac[0] - ab[0] * ac[2],
                ab[0] * ac[1] - ab[1] * ac[0],
            ];
            let n = vertices[tri[0] as usize].normal;
            let dot = cross[0] * n[0] + cross[1] * n[1] + cross[2] * n[2];
            assert!(dot > 0.0, "face {n:?} is wound inwards");
        }
    }

    #[test]
    fn the_uniform_block_is_what_the_shader_declares() {
        // 16-byte alignment throughout, because WGSL's uniform layout rules
        // pad a `vec3` to a `vec4` and a mismatch here is silent garbage in
        // the ghost's position rather than a validation error.
        assert_eq!(core::mem::size_of::<GhostUniform>(), 64 + 5 * 16);
        assert_eq!(core::mem::size_of::<GhostVertex>(), 6 * 4);
    }

    #[test]
    fn the_ghost_is_translucent_enough_to_see_the_wall_behind_it() {
        const { assert!(GHOST_COLOR[3] > 0.0 && GHOST_COLOR[3] < 0.6) }
    }
}
