//! Body of the `window` example. Split out of `main.rs` only so the
//! whole thing can be `cfg`'d off for `wasm32`, where none of it exists.

use std::sync::Arc;
use std::time::Instant;

use straf3_render::Renderer;
use winit::application::ApplicationHandler;
use winit::event::WindowEvent;
use winit::event_loop::{ActiveEventLoop, ControlFlow, EventLoop};
use winit::keyboard::{KeyCode, PhysicalKey};
use winit::window::{Window, WindowId};

#[path = "../shared/driver.rs"]
mod driver;

struct Demo {
    window: Option<Arc<Window>>,
    renderer: Option<Renderer>,
    sim: driver::Autopilot,
    started: Instant,
    frames: u32,
    last_report: Instant,
}

impl ApplicationHandler for Demo {
    fn resumed(&mut self, event_loop: &ActiveEventLoop) {
        if self.window.is_some() {
            return;
        }
        let window = Arc::new(
            event_loop
                .create_window(
                    Window::default_attributes()
                        .with_title("straf3 — arena flythrough (renderer slice)")
                        .with_inner_size(winit::dpi::LogicalSize::new(1280, 720)),
                )
                .expect("create a window"),
        );
        self.renderer = Some(Renderer::new(window.clone()));
        self.window = Some(window);
        self.started = Instant::now();
        self.last_report = self.started;
    }

    fn window_event(&mut self, event_loop: &ActiveEventLoop, _id: WindowId, event: WindowEvent) {
        let (Some(window), Some(renderer)) = (self.window.clone(), self.renderer.as_mut()) else {
            return;
        };
        match event {
            WindowEvent::CloseRequested => event_loop.exit(),
            WindowEvent::KeyboardInput { event, .. }
                if event.physical_key == PhysicalKey::Code(KeyCode::Escape) =>
            {
                event_loop.exit();
            }
            WindowEvent::Resized(size) => {
                renderer.resize(size.width, size.height);
                window.request_redraw();
            }
            WindowEvent::RedrawRequested => {
                // Frames are paced by the display; commands are paced by the
                // clock in whole 8 ms steps. The two are not related, which is
                // the point of criterion 5.
                let wall_ms = self.started.elapsed().as_millis() as u64;
                let alpha = self.sim.advance_to(wall_ms);
                renderer.render(&self.sim.prev, &self.sim.state.player, alpha);

                self.frames += 1;
                if self.last_report.elapsed().as_secs_f32() >= 2.0 {
                    let fps = self.frames as f32 / self.last_report.elapsed().as_secs_f32();
                    eprintln!(
                        "t={:>6} ms  tick={:<6} fps={fps:>5.1}  speed={:>6.1} ups  origin={:?}",
                        self.sim.state.time_ms,
                        self.sim.state.tick,
                        self.sim.speed(),
                        self.sim.state.player.origin,
                    );
                    self.frames = 0;
                    self.last_report = Instant::now();
                }
                window.request_redraw();
            }
            _ => {}
        }
    }
}

pub fn main() {
    let event_loop = EventLoop::new().expect("event loop");
    // Poll rather than Wait: there is always another frame to draw.
    event_loop.set_control_flow(ControlFlow::Poll);
    let mut demo = Demo {
        window: None,
        renderer: None,
        sim: driver::Autopilot::new(),
        started: Instant::now(),
        frames: 0,
        last_report: Instant::now(),
    };
    event_loop.run_app(&mut demo).expect("run_app");
}
