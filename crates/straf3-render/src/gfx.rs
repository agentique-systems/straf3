//! The GPU side: device acquisition, the one pipeline, and the draw.
//!
//! Split in two on purpose:
//!
//! - [`Scene`] owns everything that does not know where its pixels go — the
//!   pipeline, the map's vertex buffer, the uniforms, the depth buffer. It
//!   draws into any texture view it is handed.
//! - [`Gfx`] is [`Scene`] plus a window surface.
//!
//! The split is not tidiness. It is what lets [`Scene`] be driven headlessly
//! into an offscreen texture (see `tests/offscreen.rs`), so "the renderer draws
//! the map" is something that can be *asserted* on a machine with no display
//! — this one, and CI — rather than something a human has to squint at.

use std::sync::Arc;

use bytemuck::{Pod, Zeroable};
use straf3_sim::num::{Scalar, s};
use wgpu::util::DeviceExt as _;
use winit::window::Window;

use crate::camera::Camera;
use crate::mesh::{GpuMesh, Vertex};

/// Depth format. `Depth32Float` is the one format guaranteed on WebGPU's
/// downlevel limits as well as everywhere native.
const DEPTH_FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Depth32Float;

/// The colour distance fades into, and the colour of the sky. Not black: a
/// black horizon makes the world feel like a room with the lights off.
pub const CLEAR: wgpu::Color = wgpu::Color {
    r: 0.055,
    g: 0.075,
    b: 0.105,
    a: 1.0,
};

/// How quickly fog closes in, per unit. Tuned against the scale a course is
/// built at: at `1/5200` the far end of a 3000-unit run is lightly hazed and
/// nothing nearer is touched. Denser and the world reads as a fog bank; the
/// first pass at it was, which is why this number is measured against the
/// rendered image rather than picked.
const FOG_DENSITY: f32 = 1.0 / 5200.0;

/// The uniform block, matching `shader.wgsl`'s `Globals`.
#[repr(C)]
#[derive(Debug, Clone, Copy, Pod, Zeroable)]
struct Globals {
    view_proj: [[f32; 4]; 4],
    /// xyz = eye, w = fog density.
    eye: [f32; 4],
    fog_color: [f32; 4],
}

/// Everything needed to draw a compiled map into some texture view.
pub struct Scene {
    device: wgpu::Device,
    queue: wgpu::Queue,
    pipeline: wgpu::RenderPipeline,
    vertices: wgpu::Buffer,
    indices: wgpu::Buffer,
    index_count: u32,
    globals: wgpu::Buffer,
    bind_group: wgpu::BindGroup,
    depth: wgpu::TextureView,
    depth_size: (u32, u32),
}

impl Scene {
    /// Build the pipeline and upload `mesh`.
    ///
    /// The mesh is a parameter rather than something this file reaches for,
    /// because the geometry now comes from a compiled map chosen at runtime.
    /// `straf3-render` no longer knows what world it is drawing, which is the
    /// point: there is exactly one place that decides, and it is
    /// `straf3-game`'s `scene.rs`.
    pub fn new(
        device: wgpu::Device,
        queue: wgpu::Queue,
        format: wgpu::TextureFormat,
        width: u32,
        height: u32,
        mesh: &GpuMesh,
    ) -> Self {
        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("straf3-map"),
            source: wgpu::ShaderSource::Wgsl(include_str!("shader.wgsl").into()),
        });

        let bind_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("straf3-globals-layout"),
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

        let globals = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("straf3-globals"),
            size: core::mem::size_of::<Globals>() as wgpu::BufferAddress,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });

        let bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("straf3-globals"),
            layout: &bind_layout,
            entries: &[wgpu::BindGroupEntry {
                binding: 0,
                resource: globals.as_entire_binding(),
            }],
        });

        let layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("straf3-map"),
            bind_group_layouts: &[Some(&bind_layout)],
            immediate_size: 0,
        });

        let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("straf3-map"),
            layout: Some(&layout),
            vertex: wgpu::VertexState {
                module: &shader,
                entry_point: Some("vs_main"),
                compilation_options: Default::default(),
                buffers: &[Some(Vertex::layout())],
            },
            primitive: wgpu::PrimitiveState {
                topology: wgpu::PrimitiveTopology::TriangleList,
                // No back-face culling. A map is closed convex solids, so
                // culling would save a little fill — but a face emitted with
                // the wrong winding would then be *invisible*, which is the
                // one class of geometry bug that looks like a hole in the map
                // and takes an afternoon to find. Correctness of what you see
                // over a saving that does not matter at 5k triangles.
                cull_mode: None,
                ..Default::default()
            },
            depth_stencil: Some(wgpu::DepthStencilState {
                format: DEPTH_FORMAT,
                depth_write_enabled: Some(true),
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
                    blend: Some(wgpu::BlendState::REPLACE),
                    write_mask: wgpu::ColorWrites::ALL,
                })],
            }),
            multiview_mask: None,
            cache: None,
        });

        // A map is static geometry, so this is uploaded once and never touched
        // again. There is no per-frame vertex traffic at all.
        //
        // Indexed, unlike the arena's mesh: a real map runs to tens of
        // thousands of triangles rather than a few thousand. `create_buffer_init`
        // rejects a zero-length buffer, and the flat and empty worlds legitimately
        // have nothing to draw, so both buffers get one padding element in that
        // case and `index_count` stays 0 — the draw is then a no-op rather than
        // a special case at the call site.
        let empty = mesh.is_empty();
        let verts: &[Vertex] = if empty {
            &[Vertex::zeroed()]
        } else {
            &mesh.vertices
        };
        let idx: &[u32] = if empty { &[0] } else { &mesh.indices };
        let vertices = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("straf3-map-mesh"),
            contents: bytemuck::cast_slice(verts),
            usage: wgpu::BufferUsages::VERTEX,
        });
        let indices = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("straf3-map-indices"),
            contents: bytemuck::cast_slice(idx),
            usage: wgpu::BufferUsages::INDEX,
        });

        let depth = depth_view(&device, width.max(1), height.max(1));

        Self {
            device,
            queue,
            pipeline,
            vertices,
            indices,
            index_count: if empty { 0 } else { mesh.indices.len() as u32 },
            globals,
            bind_group,
            depth,
            depth_size: (width.max(1), height.max(1)),
        }
    }

    /// How many triangles the map came out as.
    #[must_use]
    pub fn triangle_count(&self) -> u32 {
        self.index_count / 3
    }

    /// The device, for callers that need to allocate alongside the scene.
    #[must_use]
    pub fn device(&self) -> &wgpu::Device {
        &self.device
    }

    /// The queue, likewise.
    #[must_use]
    pub fn queue(&self) -> &wgpu::Queue {
        &self.queue
    }

    fn ensure_depth(&mut self, width: u32, height: u32) {
        let want = (width.max(1), height.max(1));
        if want != self.depth_size {
            self.depth = depth_view(&self.device, want.0, want.1);
            self.depth_size = want;
        }
    }

    /// Record one frame of the map into `target`.
    pub fn draw(
        &mut self,
        encoder: &mut wgpu::CommandEncoder,
        target: &wgpu::TextureView,
        camera: &Camera,
        width: u32,
        height: u32,
    ) {
        self.ensure_depth(width, height);

        let aspect = width as Scalar / height.max(1) as Scalar;
        let globals = Globals {
            view_proj: camera.view_proj(aspect).to_cols_array_2d(),
            eye: [camera.eye.x, camera.eye.y, camera.eye.z, FOG_DENSITY],
            fog_color: [CLEAR.r as f32, CLEAR.g as f32, CLEAR.b as f32, s(1.0)],
        };
        self.queue
            .write_buffer(&self.globals, 0, bytemuck::bytes_of(&globals));

        let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some("straf3-map"),
            color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                view: target,
                depth_slice: None,
                resolve_target: None,
                ops: wgpu::Operations {
                    load: wgpu::LoadOp::Clear(CLEAR),
                    store: wgpu::StoreOp::Store,
                },
            })],
            depth_stencil_attachment: Some(wgpu::RenderPassDepthStencilAttachment {
                view: &self.depth,
                depth_ops: Some(wgpu::Operations {
                    load: wgpu::LoadOp::Clear(1.0),
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

fn depth_view(device: &wgpu::Device, width: u32, height: u32) -> wgpu::TextureView {
    device
        .create_texture(&wgpu::TextureDescriptor {
            label: Some("straf3-depth"),
            size: wgpu::Extent3d {
                width,
                height,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: DEPTH_FORMAT,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
            view_formats: &[],
        })
        .create_view(&wgpu::TextureViewDescriptor::default())
}

/// A [`Scene`] bound to a window's surface.
pub struct Gfx {
    surface: wgpu::Surface<'static>,
    config: wgpu::SurfaceConfiguration,
    scene: Scene,
    /// Which backend actually got picked, so a bug report can say so rather
    /// than guess.
    pub backend: wgpu::Backend,
    /// Kept alive because the surface borrows it for its whole life.
    _window: Arc<Window>,
}

impl Gfx {
    /// Acquire an adapter, device and surface for `window`.
    ///
    /// `backends` is the caller's decision, never wgpu's: on the web the host
    /// page picks WebGPU before entering wasm, because wgpu's WebGPU backend
    /// does not degrade to WebGL2 on its own — it panics inside itself
    /// (spec rev 6 Q2, measured).
    pub async fn new(window: Arc<Window>, backends: wgpu::Backends, mesh: GpuMesh) -> Self {
        let mut desc = wgpu::InstanceDescriptor::new_without_display_handle_from_env();
        desc.backends = backends;
        let instance = wgpu::Instance::new(desc);

        let surface = instance
            .create_surface(window.clone())
            .expect("create a surface for the window");

        let adapter = instance
            .request_adapter(&wgpu::RequestAdapterOptions {
                power_preference: wgpu::PowerPreference::HighPerformance,
                compatible_surface: Some(&surface),
                force_fallback_adapter: false,
                apply_limit_buckets: false,
            })
            .await
            .expect("no GPU adapter for the chosen backend");

        let info = adapter.get_info();
        log_line(&format!(
            "straf3-render: backend={:?} adapter={:?} type={:?}",
            info.backend, info.name, info.device_type
        ));

        let (device, queue) = adapter
            .request_device(&wgpu::DeviceDescriptor {
                label: Some("straf3-device"),
                required_features: wgpu::Features::empty(),
                // Downlevel defaults, not the adapter's: asking for more than
                // the browser's baseline is how a native-only renderer happens
                // by accident.
                required_limits: wgpu::Limits::downlevel_defaults()
                    .using_resolution(adapter.limits()),
                experimental_features: Default::default(),
                memory_hints: wgpu::MemoryHints::default(),
                trace: wgpu::Trace::Off,
            })
            .await
            .expect("request_device");

        let size = window.inner_size();
        let (width, height) = (size.width.max(1), size.height.max(1));

        let caps = surface.get_capabilities(&adapter);
        let format = caps
            .formats
            .iter()
            .copied()
            .find(|f| f.is_srgb())
            .unwrap_or(caps.formats[0]);

        let config = wgpu::SurfaceConfiguration {
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
            format,
            color_space: Default::default(),
            width,
            height,
            present_mode: caps.present_modes[0],
            alpha_mode: caps.alpha_modes[0],
            view_formats: vec![],
            desired_maximum_frame_latency: 2,
        };
        surface.configure(&device, &config);

        let scene = Scene::new(device, queue, format, width, height, &mesh);
        log_line(&format!(
            "straf3-render: map is {} triangles",
            scene.triangle_count()
        ));

        Self {
            surface,
            config,
            scene,
            backend: info.backend,
            _window: window,
        }
    }

    /// Reconfigure for a new window size. Zero-sized (a minimised window) is
    /// ignored rather than passed on: configuring a zero-extent surface is a
    /// validation error.
    pub fn resize(&mut self, width: u32, height: u32) {
        if width == 0 || height == 0 || (width, height) == (self.config.width, self.config.height) {
            return;
        }
        self.config.width = width;
        self.config.height = height;
        self.surface.configure(self.scene.device(), &self.config);
    }

    /// Draw one frame from `camera` and present it.
    pub fn render(&mut self, camera: &Camera) {
        use wgpu::CurrentSurfaceTexture as Cst;
        let frame = match self.surface.get_current_texture() {
            Cst::Success(t) | Cst::Suboptimal(t) => t,
            // Outdated and Lost want the surface rebuilt; Timeout and Occluded
            // just mean there is nothing to draw into this instant. Skipping
            // the frame is correct for all of them — the simulation is not
            // driven from here, so nothing is lost by missing one.
            other => {
                log_line(&format!("straf3-render: surface acquire: {other:?}"));
                self.surface.configure(self.scene.device(), &self.config);
                return;
            }
        };

        let view = frame
            .texture
            .create_view(&wgpu::TextureViewDescriptor::default());
        let mut encoder =
            self.scene
                .device()
                .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                    label: Some("straf3-frame"),
                });
        self.scene.draw(
            &mut encoder,
            &view,
            camera,
            self.config.width,
            self.config.height,
        );
        self.scene.queue().submit(Some(encoder.finish()));
        // wgpu 30 moved presentation onto the queue.
        self.scene.queue().present(frame);
    }
}

/// One line of diagnostics, to wherever this platform's diagnostics go.
///
/// `log` is not a dependency of this crate: `straf3-render` is meant to be
/// cheap to link into a wasm bundle, and a facade plus a browser logger is
/// most of a kilobyte for four lines of startup text.
fn log_line(msg: &str) {
    #[cfg(target_arch = "wasm32")]
    web_log(msg);
    #[cfg(not(target_arch = "wasm32"))]
    eprintln!("{msg}");
}

#[cfg(target_arch = "wasm32")]
fn web_log(msg: &str) {
    use wasm_bindgen::prelude::*;
    #[wasm_bindgen]
    unsafe extern "C" {
        #[wasm_bindgen(js_namespace = console)]
        fn log(s: &str);
    }
    log(msg);
}
