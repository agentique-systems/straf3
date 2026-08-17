//! Body of the `window` example. Split out of `main.rs` only so the
//! whole thing can be `cfg`'d off for `wasm32`, where none of it exists.
//!
//! # Two jobs
//!
//! It flies the autopilot down the course so the renderer's slice can be
//! *seen* working, and — with `--pacing-log` — it records how long each frame
//! took so that slice can be *measured*. The measurement half exists because
//! frame pacing is a property of the surface and the display, not of the game
//! logic: this example configures the same `Gfx`, in the same present mode,
//! with the same `ControlFlow::Poll` + `request_redraw` loop shape as
//! `straf3-game`, so a number taken here is a number about the renderer and
//! the panel rather than about the autopilot.
//!
//! It is **not** the game, and a pacing number from here must be published as
//! what it is. `straf3-game`'s own `--pacing-log` is the client measurement.

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

/// What the command line asked for.
struct Options {
    /// Close the window after this much wall time. Unattended runs must be
    /// bounded: this opens a window on somebody's actual desktop.
    exit_after_ms: Option<u64>,
    /// Where to write the frame-time CSV.
    pacing_log: Option<std::path::PathBuf>,
}

fn parse_options() -> Result<Options, String> {
    let mut exit_after_ms = None;
    let mut pacing_log = None;
    let mut args = std::env::args().skip(1);
    while let Some(arg) = args.next() {
        let mut value = || args.next().ok_or_else(|| format!("`{arg}` needs a value"));
        match arg.as_str() {
            "--exit-after" => {
                let raw = value()?;
                exit_after_ms = Some(
                    raw.parse::<u64>()
                        .map_err(|_| format!("`--exit-after {raw}` is not a number"))?,
                );
            }
            "--pacing-log" => pacing_log = Some(std::path::PathBuf::from(value()?)),
            other => {
                return Err(format!(
                    "unknown argument: {other}\n\
                     usage: --exit-after <ms> --pacing-log <file.csv>"
                ));
            }
        }
    }
    Ok(Options {
        exit_after_ms,
        pacing_log,
    })
}

struct Demo {
    window: Option<Arc<Window>>,
    renderer: Option<Renderer>,
    sim: driver::Autopilot,
    started: Instant,
    frames: u32,
    last_report: Instant,
    options: Options,
    pacing: Option<straf3_devtools::pacing::PacingLog>,
    /// Set once, on the first frame the device exists, so the log header names
    /// the mode the surface was actually granted.
    present_recorded: bool,
}

impl Demo {
    /// Write the pacing log, if one was asked for. Called on every exit path.
    fn finish(&mut self) {
        let (Some(path), Some(log)) = (self.options.pacing_log.as_ref(), self.pacing.as_ref())
        else {
            return;
        };
        match log.write_csv(path) {
            Ok(()) => eprintln!(
                "pacing: wrote {} frame intervals to {}",
                log.len(),
                path.display()
            ),
            Err(e) => eprintln!("pacing: could not write {}: {e}", path.display()),
        }
    }
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
                        .with_title("straf3 — course flythrough (renderer slice)")
                        .with_inner_size(winit::dpi::LogicalSize::new(1280, 720)),
                )
                .expect("create a window"),
        );
        // The same compiled map the autopilot is colliding with — one
        // `CompiledMap`, its mesh to the GPU and its hulls to the tracer.
        let mesh = straf3_render::mesh::GpuMesh::from_map(&driver::course::get().map.mesh);
        self.renderer = Some(Renderer::new(window.clone(), mesh));
        self.window = Some(window);
        self.started = Instant::now();
        self.last_report = self.started;
    }

    fn window_event(&mut self, event_loop: &ActiveEventLoop, _id: WindowId, event: WindowEvent) {
        let (Some(window), Some(renderer)) = (self.window.clone(), self.renderer.as_mut()) else {
            return;
        };
        match event {
            WindowEvent::CloseRequested => {
                self.finish();
                event_loop.exit();
            }
            WindowEvent::KeyboardInput { event, .. }
                if event.physical_key == PhysicalKey::Code(KeyCode::Escape) =>
            {
                self.finish();
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

                // After the present, so an interval spans present-to-present:
                // that is the quantity a display refresh actually bounds, and
                // the one an uncapped/vsynced comparison is about.
                if let Some(log) = &mut self.pacing {
                    if self.present_recorded {
                        log.frame();
                    } else if let Some(selection) = renderer.present_mode() {
                        self.present_recorded = true;
                        log.set_present_mode(straf3_render::present::name(selection.actual));
                        log.note("source", "straf3-render example window");
                        log.note("frame_latency", &selection.frame_latency.to_string());
                        if selection.fell_back {
                            log.note("fell_back", "true");
                        }
                        // Open the first interval here and record nothing for
                        // this frame. Calling `start` and `frame` in the same
                        // pass would log a zero-nanosecond interval — an
                        // artefact of the instrumentation rather than a frame
                        // that happened, and one that drags the minimum and
                        // the mean down. Timing therefore begins at the first
                        // present after the device exists, which is also where
                        // steady state begins.
                        log.start();
                    }
                }

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

                if let Some(limit) = self.options.exit_after_ms
                    && self.started.elapsed().as_millis() as u64 >= limit
                {
                    eprintln!("--exit-after {limit} ms reached");
                    self.finish();
                    event_loop.exit();
                    return;
                }

                window.request_redraw();
            }
            _ => {}
        }
    }
}

pub fn main() {
    let options = match parse_options() {
        Ok(o) => o,
        Err(e) => {
            eprintln!("{e}");
            std::process::exit(1);
        }
    };
    // Reserved up front so no frame ever pays for a `Vec` growth. Sized for
    // the whole session at an unreachable 2000 fps, which is cheap: 16 bytes a
    // frame.
    let pacing = options.pacing_log.as_ref().map(|_| {
        straf3_devtools::pacing::PacingLog::for_session(
            options.exit_after_ms.unwrap_or(60_000).div_ceil(1000),
            2_000,
        )
    });

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
        options,
        pacing,
        present_recorded: false,
    };
    event_loop.run_app(&mut demo).expect("run_app");
}
