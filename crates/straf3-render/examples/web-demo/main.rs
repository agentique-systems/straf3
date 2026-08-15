//! The same flythrough as the `window` example, on a `<canvas>`.
//!
//! Built as a `cdylib` for `wasm32-unknown-unknown` and bundled with
//! `wasm-bindgen`. This target is also the **stage-D bundle** spec rev 6 §P2
//! left unmeasured: it links `straf3-render`, `straf3-platform`, `gltf`, and
//! `parry3d` arriving transitively through `straf3-map`, which is the
//! combination nobody had a number for. See `web/` next to this file.
//!
//! Two things differ from native, and only two:
//!
//! 1. The device arrives asynchronously. Nothing here has to care —
//!    [`Renderer::render`] is a no-op until it lands.
//! 2. The event loop never returns, so it is `spawn_app` rather than
//!    `run_app`. winit throws a sentinel exception to unwind out of the call;
//!    the host page treats that as normal, because it is.
//!
//! Backend selection is JS's, before this module is entered: spec rev 6 Q2.

// Empty on every other target. `cargo build`/`cargo test` compile every
// example for the host, and winit's `spawn_app`, `with_canvas` and the
// `web-sys` bindings below simply do not exist there.
#![cfg(target_arch = "wasm32")]

use std::sync::Arc;

use straf3_render::Renderer;
use wasm_bindgen::prelude::*;
use winit::application::ApplicationHandler;
use winit::event::WindowEvent;
use winit::event_loop::{ActiveEventLoop, ControlFlow, EventLoop};
use winit::platform::web::{EventLoopExtWebSys, WindowAttributesExtWebSys};
use winit::window::{Window, WindowId};

#[path = "../shared/driver.rs"]
mod driver;

/// The canvas the host page provides.
const CANVAS_ID: &str = "straf3-canvas";

struct Demo {
    window: Option<Arc<Window>>,
    renderer: Option<Renderer>,
    sim: driver::Autopilot,
    backends: wgpu::Backends,
    start_ms: f64,
    frames: u32,
    last_report_ms: f64,
    reported_ready: bool,
}

/// Milliseconds since page load, from the browser's monotonic clock.
fn now_ms() -> f64 {
    web_sys::window()
        .and_then(|w| w.performance())
        .map_or(0.0, |p| p.now())
}

impl ApplicationHandler for Demo {
    fn resumed(&mut self, event_loop: &ActiveEventLoop) {
        if self.window.is_some() {
            return;
        }
        let canvas = web_sys::window()
            .and_then(|w| w.document())
            .and_then(|d| d.get_element_by_id(CANVAS_ID))
            .and_then(|e| e.dyn_into::<web_sys::HtmlCanvasElement>().ok())
            .expect("the host page must provide a #straf3-canvas");
        let size = (canvas.width(), canvas.height());

        let window = Arc::new(
            event_loop
                .create_window(
                    Window::default_attributes()
                        .with_title("straf3")
                        .with_canvas(Some(canvas)),
                )
                .expect("create a window on the canvas"),
        );

        let mut renderer = Renderer::with_backends(window.clone(), self.backends);
        // Arrives before the device does; the renderer remembers it.
        renderer.resize(size.0, size.1);
        self.renderer = Some(renderer);
        self.window = Some(window);
        self.start_ms = now_ms();
        self.last_report_ms = self.start_ms;
        web_sys::console::log_1(&"straf3: window created, awaiting device".into());
    }

    fn window_event(&mut self, event_loop: &ActiveEventLoop, _id: WindowId, event: WindowEvent) {
        let _ = event_loop;
        let (Some(window), Some(renderer)) = (self.window.clone(), self.renderer.as_mut()) else {
            return;
        };
        match event {
            WindowEvent::Resized(size) => {
                renderer.resize(size.width, size.height);
                window.request_redraw();
            }
            WindowEvent::RedrawRequested => {
                let wall_ms = (now_ms() - self.start_ms).max(0.0) as u64;
                let alpha = self.sim.advance_to(wall_ms);
                renderer.render(&self.sim.prev, &self.sim.state.player, alpha);

                if renderer.is_ready() && !self.reported_ready {
                    self.reported_ready = true;
                    web_sys::console::log_1(
                        &format!("straf3: device ready on {:?}", renderer.backend()).into(),
                    );
                }

                self.frames += 1;
                let since = now_ms() - self.last_report_ms;
                if since >= 1000.0 {
                    // Mirrored into the DOM by the host page, so a headless
                    // Chrome run can read it back without a devtools client.
                    web_sys::console::log_1(
                        &format!(
                            "straf3: t={} ms tick={} fps={:.1} speed={:.1} ups origin={:?}",
                            self.sim.state.time_ms,
                            self.sim.state.tick,
                            self.frames as f64 * 1000.0 / since,
                            self.sim.speed(),
                            self.sim.state.player.origin,
                        )
                        .into(),
                    );
                    self.frames = 0;
                    self.last_report_ms = now_ms();
                }
                window.request_redraw();
            }
            _ => {}
        }
    }
}

/// Entry point. The host page calls this with the backend **it** chose, having
/// asked `navigator.gpu` for an adapter before entering wasm — wgpu will not
/// degrade from WebGPU to WebGL2 by itself, it panics inside its own backend
/// (spec rev 6 Q2, measured, not read from docs).
#[wasm_bindgen]
pub fn start(backend: &str) {
    console_error_panic_hook::set_once();

    let backends = match backend {
        "webgpu" => wgpu::Backends::BROWSER_WEBGPU,
        other => {
            web_sys::console::error_1(
                &format!(
                    "straf3: backend {other:?} is not compiled in — this bundle is WebGPU-only \
                     (spec rev 6 Q2). Refusing to start rather than panicking inside wgpu."
                )
                .into(),
            );
            return;
        }
    };

    let event_loop = EventLoop::new().expect("event loop");
    event_loop.set_control_flow(ControlFlow::Poll);
    event_loop.spawn_app(Demo {
        window: None,
        renderer: None,
        sim: driver::Autopilot::new(),
        backends,
        start_ms: 0.0,
        frames: 0,
        last_report_ms: 0.0,
        reported_ready: false,
    });
}
