//! Feasibility probe: does straf3's above-the-line render stack reach a
//! browser canvas, and at what shipped size?
//!
//! This is deliberately NOT a port. It opens a window (a `<canvas>` on web),
//! acquires a wgpu surface, and clears it to a colour every frame. That is
//! the whole of the question "can this stack render in a browser at all".
//!
//! Build stages are selected by feature so each one can be measured
//! separately — see `measure.sh`.

use std::cell::RefCell;
use std::rc::Rc;
use std::sync::Arc;

use winit::application::ApplicationHandler;
use winit::event::WindowEvent;
use winit::event_loop::{ActiveEventLoop, ControlFlow, EventLoop};
use winit::window::{Window, WindowId};

/// Everything that had to be acquired asynchronously before a frame can be
/// drawn. On native this is `block_on`; on web it cannot be, which is the
/// first real structural difference the probe exists to expose.
struct Gfx {
    surface: wgpu::Surface<'static>,
    device: wgpu::Device,
    queue: wgpu::Queue,
    config: wgpu::SurfaceConfiguration,
    /// Recorded so the report can state which backend actually got picked in
    /// the browser rather than guessing.
    backend: wgpu::Backend,
    frame: u32,

    #[cfg(feature = "ui")]
    egui: EguiLayer,
}

#[cfg(feature = "ui")]
struct EguiLayer {
    ctx: egui::Context,
    renderer: egui_wgpu::Renderer,
    #[cfg(feature = "ui-winit")]
    state: egui_winit::State,
}

impl Gfx {
    async fn new(window: Arc<Window>, backends: wgpu::Backends) -> Self {
        // The backend set has to be decided by the *caller*, not left to
        // wgpu: see `pick_backends`. wgpu's WebGPU backend does not degrade
        // to WebGL2 on its own.
        let mut desc = wgpu::InstanceDescriptor::new_without_display_handle_from_env();
        desc.backends = backends;
        let instance = wgpu::Instance::new(desc);

        let surface = instance
            .create_surface(window.clone())
            .expect("create_surface");

        let adapter = instance
            .request_adapter(&wgpu::RequestAdapterOptions {
                power_preference: wgpu::PowerPreference::default(),
                compatible_surface: Some(&surface),
                force_fallback_adapter: false,
                apply_limit_buckets: false,
            })
            .await
            .expect("no suitable GPU adapter");

        let info = adapter.get_info();
        log::info!(
            "adapter: backend={:?} name={:?} type={:?}",
            info.backend,
            info.name,
            info.device_type
        );

        // WebGL2 exposes a genuinely smaller limit set than WebGPU. Asking for
        // downlevel_webgl2_defaults is what makes the same binary run on both.
        let required_limits = if cfg!(feature = "webgl") {
            wgpu::Limits::downlevel_webgl2_defaults().using_resolution(adapter.limits())
        } else {
            wgpu::Limits::downlevel_defaults().using_resolution(adapter.limits())
        };

        let (device, queue) = adapter
            .request_device(&wgpu::DeviceDescriptor {
                label: Some("straf3-probe-device"),
                required_features: wgpu::Features::empty(),
                required_limits,
                experimental_features: Default::default(),
                memory_hints: wgpu::MemoryHints::default(),
                trace: wgpu::Trace::Off,
            })
            .await
            .expect("request_device");

        let size = window.inner_size();
        let width = size.width.max(1);
        let height = size.height.max(1);

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

        #[cfg(feature = "ui")]
        let egui = {
            let ctx = egui::Context::default();
            let renderer = egui_wgpu::Renderer::new(
                &device,
                format,
                egui_wgpu::RendererOptions {
                    msaa_samples: 1,
                    depth_stencil_format: None,
                    dithering: true,
                    predictable_texture_filtering: false,
                },
            );
            #[cfg(feature = "ui-winit")]
            let state = egui_winit::State::new(
                ctx.clone(),
                egui::ViewportId::ROOT,
                &window,
                Some(window.scale_factor() as f32),
                None,
                None,
            );
            EguiLayer {
                ctx,
                renderer,
                #[cfg(feature = "ui-winit")]
                state,
            }
        };

        Self {
            surface,
            device,
            queue,
            config,
            backend: info.backend,
            frame: 0,
            #[cfg(feature = "ui")]
            egui,
        }
    }

    fn resize(&mut self, width: u32, height: u32) {
        if width == 0 || height == 0 {
            return;
        }
        self.config.width = width;
        self.config.height = height;
        self.surface.configure(&self.device, &self.config);
    }

    fn render(&mut self, _window: &Window) {
        use wgpu::CurrentSurfaceTexture as Cst;
        let frame = match self.surface.get_current_texture() {
            Cst::Success(t) | Cst::Suboptimal(t) => t,
            // Outdated/Lost want a reconfigure; Timeout/Occluded just skip.
            other => {
                log::warn!("surface acquire: {other:?}");
                self.surface.configure(&self.device, &self.config);
                return;
            }
        };
        let view = frame
            .texture
            .create_view(&wgpu::TextureViewDescriptor::default());
        let mut encoder = self
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor { label: None });

        // A slow pulse, so "it rendered" is visually distinguishable from
        // "the canvas happens to be that colour".
        self.frame = self.frame.wrapping_add(1);
        let t = (self.frame as f64 / 120.0).sin() * 0.5 + 0.5;

        {
            let _pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("clear"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: &view,
                    depth_slice: None,
                    resolve_target: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Clear(wgpu::Color {
                            r: 0.05,
                            g: t * 0.6,
                            b: 0.35,
                            a: 1.0,
                        }),
                        store: wgpu::StoreOp::Store,
                    },
                })],
                depth_stencil_attachment: None,
                timestamp_writes: None,
                occlusion_query_set: None,
                multiview_mask: None,
            });
        }

        #[cfg(feature = "ui")]
        self.render_egui(_window, &mut encoder, &view);

        self.queue.submit(Some(encoder.finish()));
        // wgpu 30 moved presentation onto the queue.
        self.queue.present(frame);
    }

    #[cfg(feature = "ui")]
    fn render_egui(
        &mut self,
        window: &Window,
        encoder: &mut wgpu::CommandEncoder,
        view: &wgpu::TextureView,
    ) {
        #[cfg(feature = "ui-winit")]
        let input = self.egui.state.take_egui_input(window);
        // Without egui-winit the host has to build RawInput itself. This is
        // the honest cost of that crate not compiling for web: screen rect,
        // scale factor, and every pointer/key event, by hand.
        #[cfg(not(feature = "ui-winit"))]
        let input = {
            let ppp = window.scale_factor() as f32;
            egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(
                    egui::Pos2::ZERO,
                    egui::vec2(
                        self.config.width as f32 / ppp,
                        self.config.height as f32 / ppp,
                    ),
                )),
                ..Default::default()
            }
        };
        let output = self.egui.ctx.run_ui(input, |ui| {
            let ctx = ui.ctx().clone();
            egui::Window::new("straf3 telemetry").show(&ctx, |ui| {
                // Text at all is the point: it forces the font atlas, which is
                // the part of egui most likely to be wasm-hostile.
                ui.label(format!("backend: {:?}", self.backend));
                ui.label(format!("frame: {}", self.frame));
                ui.label("horizontal speed: 320.0 ups");
            });
        });

        #[cfg(feature = "ui-winit")]
        self.egui
            .state
            .handle_platform_output(window, output.platform_output);

        let tris = self
            .egui
            .ctx
            .tessellate(output.shapes, output.pixels_per_point);
        for (id, deltas) in &output.textures_delta.set {
            for delta in deltas {
                self.egui
                    .renderer
                    .update_texture(&self.device, &self.queue, *id, delta);
            }
        }
        let desc = egui_wgpu::ScreenDescriptor {
            size_in_pixels: [self.config.width, self.config.height],
            pixels_per_point: output.pixels_per_point,
        };
        self.egui
            .renderer
            .update_buffers(&self.device, &self.queue, encoder, &tris, &desc);

        let pass = encoder
            .begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("egui"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view,
                    depth_slice: None,
                    resolve_target: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Load,
                        store: wgpu::StoreOp::Store,
                    },
                })],
                depth_stencil_attachment: None,
                timestamp_writes: None,
                occlusion_query_set: None,
                multiview_mask: None,
            })
            .forget_lifetime();
        self.egui.renderer.render(&mut { pass }, &tris, &desc);

        for id in &output.textures_delta.free {
            self.egui.renderer.free_texture(id);
        }
    }
}

struct App {
    backends: wgpu::Backends,
    window: Option<Arc<Window>>,
    // Rc<RefCell<..>> rather than a plain Option because on web the device is
    // only available after a JS promise resolves, outside this callback.
    gfx: Rc<RefCell<Option<Gfx>>>,
}

impl App {
    fn new(backends: wgpu::Backends) -> Self {
        Self {
            backends,
            window: None,
            gfx: Rc::new(RefCell::new(None)),
        }
    }
}

impl ApplicationHandler for App {
    fn resumed(&mut self, event_loop: &ActiveEventLoop) {
        if self.window.is_some() {
            return;
        }
        let attrs = window_attributes();
        let window = Arc::new(event_loop.create_window(attrs).expect("create_window"));
        self.window = Some(window.clone());

        let slot = self.gfx.clone();
        let backends = self.backends;

        #[cfg(target_arch = "wasm32")]
        {
            let w = window.clone();
            wasm_bindgen_futures::spawn_local(async move {
                let gfx = Gfx::new(w.clone(), backends).await;
                *slot.borrow_mut() = Some(gfx);
                w.request_redraw();
            });
        }

        #[cfg(not(target_arch = "wasm32"))]
        {
            *slot.borrow_mut() = Some(pollster::block_on(Gfx::new(window.clone(), backends)));
            window.request_redraw();
        }
    }

    fn window_event(&mut self, event_loop: &ActiveEventLoop, _id: WindowId, event: WindowEvent) {
        let Some(window) = self.window.clone() else {
            return;
        };

        #[cfg(feature = "ui-winit")]
        if let Some(gfx) = self.gfx.borrow_mut().as_mut() {
            let _ = gfx.egui.state.on_window_event(&window, &event);
        }

        match event {
            WindowEvent::CloseRequested => event_loop.exit(),
            WindowEvent::Resized(size) => {
                if let Some(gfx) = self.gfx.borrow_mut().as_mut() {
                    gfx.resize(size.width, size.height);
                }
                window.request_redraw();
            }
            WindowEvent::RedrawRequested => {
                if let Some(gfx) = self.gfx.borrow_mut().as_mut() {
                    gfx.render(&window);
                }
                window.request_redraw();
            }
            _ => {}
        }
    }
}

fn window_attributes() -> winit::window::WindowAttributes {
    let attrs = Window::default_attributes().with_title("straf3 wasm render probe");

    #[cfg(target_arch = "wasm32")]
    {
        use winit::platform::web::WindowAttributesExtWebSys;
        let canvas = web_sys::window()
            .and_then(|w| w.document())
            .and_then(|d| d.get_element_by_id("straf3-canvas"))
            .and_then(|e| e.dyn_into::<web_sys::HtmlCanvasElement>().ok())
            .expect("no #straf3-canvas in the document");
        use wasm_bindgen::JsCast as _;
        return attrs.with_canvas(Some(canvas));
    }

    #[allow(unreachable_code)]
    attrs
}

/// Entry point. Native `main` calls this directly; on web it is exported to JS.
#[cfg(target_arch = "wasm32")]
#[wasm_bindgen::prelude::wasm_bindgen]
pub fn start_web(backend: &str) {
    run(match backend {
        "webgpu" => wgpu::Backends::BROWSER_WEBGPU,
        "webgl" => wgpu::Backends::GL,
        _ => wgpu::Backends::all(),
    });
}

/// Entry point. Native `main` calls this; on web JS calls `start_web`.
pub fn run(backends: wgpu::Backends) {
    #[cfg(target_arch = "wasm32")]
    {
        console_error_panic_hook::set_once();
        let _ = console_log::init_with_level(log::Level::Info);
    }
    #[cfg(not(target_arch = "wasm32"))]
    {
        env_logger::init();
    }

    // Referenced only so the linker keeps straf3's real above-the-line crates
    // in the binary — the point of the `game` stage is measuring their cost.
    #[cfg(feature = "game")]
    {
        let a = straf3_render::InterpolationAlpha(0.5);
        let t = straf3_platform::FrameTiming { elapsed_ms: 8 };
        #[cfg(feature = "game-devtools")]
        let s = straf3_devtools::TelemetrySample::default();
        #[cfg(not(feature = "game-devtools"))]
        let s = "devtools-excluded (arboard has no wasm32 backend)";
        log::info!(
            "straf3 crates linked: {:?} {:?} {:?} gltf_reachable={}",
            a,
            t,
            s,
            std::mem::size_of::<gltf::Gltf>()
        );
    }

    let event_loop = EventLoop::new().expect("event loop");
    event_loop.set_control_flow(ControlFlow::Poll);
    let app = App::new(backends);

    #[cfg(target_arch = "wasm32")]
    {
        // The browser event loop never returns — `run_app` would trap. This is
        // the structural difference winit's web backend forces on you.
        use winit::platform::web::EventLoopExtWebSys;
        event_loop.spawn_app(app);
    }

    #[cfg(not(target_arch = "wasm32"))]
    {
        let mut app = app;
        event_loop.run_app(&mut app).expect("run_app");
    }
}
