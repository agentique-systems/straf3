//! The winit event loop: the thin shell around [`Game`].
//!
//! # What is deliberately *not* here
//!
//! No physics, no command construction, no accumulator arithmetic. All of that
//! is in [`crate::game`], [`crate::input_map`] and [`crate::tick`], where it
//! can be tested with no window in the process. This file only:
//!
//! 1. turns winit events into [`straf3_platform::InputState`] changes,
//! 2. asks the clock how much time passed and hands that number to
//!    [`Game::advance`],
//! 3. hands the two most recent states and the interpolation alpha to the
//!    renderer.
//!
//! If a rule about *how the game behaves* appears in this file, it is in the
//! wrong file — it would be untestable without a display server, and this
//! machine does not have a useful one (spec section 2).
//!
//! # The pacing, in three lines
//!
//! ```text
//! let delta = clock.frame().delta_ms;   // whole ms of real time
//! let ticks = game.advance(delta);      // 0, 1 or many fixed 8 ms commands
//! renderer.render(prev, curr, alpha);   // draw between the last two
//! ```
//!
//! The frame rate appears in the first line and the third. It does not appear
//! in the second, and that is criterion 5.

use std::sync::Arc;

use straf3_platform::{Clock, PointerGrab, WindowConfig};
use straf3_sim::{PhysicsProfile, TickRate, World};
use winit::application::ApplicationHandler;
use winit::event::{DeviceEvent, DeviceId, ElementState, WindowEvent};
use winit::event_loop::{ActiveEventLoop, ControlFlow, EventLoop};
use winit::keyboard::{KeyCode, PhysicalKey};
use winit::window::{Window, WindowId};

use crate::game::Game;
use crate::scene::WorldChoice;

/// How a session should be set up.
#[derive(Debug, Clone)]
pub struct Options {
    /// Which world to play in.
    pub world: WorldChoice,
    /// Which movement constants to play under.
    pub profile: PhysicsProfile,
    /// Name of that profile, for the recording header.
    pub profile_name: String,
    /// The command rate — part of the physics (spec D2).
    pub rate: TickRate,
    /// Record every command produced, for replay.
    pub record: bool,
    /// Close the window after this much wall time, in milliseconds.
    ///
    /// Not a gameplay feature: it is what makes an unattended run possible, so
    /// that "record a session in the windowed build, replay it through
    /// `straf3-headless`, compare checksums" can be a script rather than a
    /// person remembering to close a window at the right moment.
    pub exit_after_ms: Option<u64>,
    /// The window (or canvas) to open.
    pub window: WindowConfig,
}

impl Default for Options {
    fn default() -> Self {
        Self {
            world: WorldChoice::default(),
            profile: PhysicsProfile::cpm(),
            profile_name: "cpm".to_owned(),
            rate: crate::tick::DEFAULT_RATE,
            record: false,
            exit_after_ms: None,
            window: WindowConfig::straf3(),
        }
    }
}

/// How often the console line reporting speed is printed, in wall ms.
const TELEMETRY_INTERVAL_MS: u64 = 1_000;

/// The application: a window, a clock, a session, and (maybe) a renderer.
pub struct App {
    options: Options,
    window: Option<Arc<Window>>,
    clock: Clock,
    game: Game<&'static dyn World>,
    grab: PointerGrab,
    /// Whether the clock has been zeroed against the first frame yet.
    primed: bool,
    last_telemetry_ms: u64,
    frames: u64,
    /// The last frame rate [`App::report_telemetry`] computed, for the overlay.
    ///
    /// The overlay draws every frame and the rate is only measured once a
    /// second, so the number it shows is the last one measured rather than a
    /// per-frame reciprocal — which at 300 fps would be unreadable noise.
    last_fps: u32,
    #[cfg(feature = "render")]
    renderer: Option<straf3_render::Renderer>,
    /// The on-screen overlay, built on the first frame the device exists.
    #[cfg(feature = "render")]
    hud: Option<straf3_devtools::Hud>,
}

/// The triangles for the world this session is playing in.
///
/// One line, and it is the whole invariant: the mesh comes off the same
/// [`CompiledMap`](straf3_map::CompiledMap) whose hulls `scene.rs` handed to the
/// simulation. There is no path by which the renderer could be given a
/// different world from the one the player collides with — the flat and empty
/// worlds have no geometry and correctly draw nothing.
#[cfg(feature = "render")]
fn scene_mesh() -> straf3_render::mesh::GpuMesh {
    match crate::scene::loaded() {
        Some(loaded) => straf3_render::mesh::GpuMesh::from_map(&loaded.map.mesh),
        None => straf3_render::mesh::GpuMesh::empty(),
    }
}

impl App {
    /// Build the application. No window is created until the event loop
    /// resumes — winit owns that moment, on both targets.
    #[must_use]
    pub fn new(options: Options) -> Self {
        let world = options.world.or_fallback();
        let (spawn, spawn_yaw) = world.spawn();
        let mut game = Game::new(
            world.world(),
            options.profile,
            options.rate,
            spawn,
            spawn_yaw,
        );
        if options.record {
            game.record();
        }
        Self {
            options: Options { world, ..options },
            window: None,
            clock: Clock::new(),
            game,
            grab: PointerGrab::Released,
            primed: false,
            last_telemetry_ms: 0,
            frames: 0,
            last_fps: 0,
            #[cfg(feature = "render")]
            renderer: None,
            #[cfg(feature = "render")]
            hud: None,
        }
    }

    /// The session, for a caller that wants the recording out of it afterwards.
    #[must_use]
    pub const fn game(&self) -> &Game<&'static dyn World> {
        &self.game
    }

    /// The options this app was built with, after availability fallbacks.
    #[must_use]
    pub const fn options(&self) -> &Options {
        &self.options
    }

    /// The session's recording as a `straf3-headless` input file, if
    /// recording was turned on.
    #[must_use]
    pub fn fixture(&self) -> Option<String> {
        self.game
            .recorder()
            .map(|r| r.to_fixture(self.options.world.spec(), &self.options.profile_name))
    }

    /// One frame: read the clock, run whatever ticks that buys, draw.
    fn frame(&mut self, event_loop: &ActiveEventLoop) {
        if !self.primed {
            self.primed = true;
            self.clock.prime();
        }
        let delta = self.clock.frame();
        self.game.advance(delta.delta_ms);
        self.frames += 1;

        if let Some(limit) = self.options.exit_after_ms
            && delta.timing.elapsed_ms >= limit
        {
            log::info!("--exit-after {limit} ms reached");
            self.finish();
            event_loop.exit();
            return;
        }

        #[cfg(feature = "render")]
        if let Some(renderer) = &mut self.renderer {
            // Built on the first frame the device exists — which natively is
            // the first frame and on the web is several frames in.
            if self.hud.is_none() {
                self.hud = renderer.with_device(straf3_devtools::Hud::new);
            }
            // The split is the one number the simulation cannot know. No ghost
            // is loaded yet, so it is `None` and the overlay draws no split at
            // all rather than `+0.000`, which would claim the player was level
            // with a personal best that is not there.
            let split_ms: Option<i32> = None;
            let sample = straf3_devtools::TelemetrySample::of(self.game.state())
                .with_fps(self.last_fps)
                .with_split_ms(split_ms);
            let pixels_per_point = self
                .window
                .as_ref()
                .map_or(1.0, |w| w.scale_factor() as f32);
            let hud = self.hud.as_mut();
            renderer.render_with(
                &self.game.previous().player,
                &self.game.state().player,
                straf3_render::InterpolationAlpha(self.game.alpha()),
                |o| {
                    if let Some(hud) = hud {
                        hud.draw(
                            straf3_devtools::HudFrame {
                                device: o.device,
                                queue: o.queue,
                                encoder: o.encoder,
                                target: o.target,
                                width: o.width,
                                height: o.height,
                                pixels_per_point,
                            },
                            &sample,
                        );
                    }
                },
            );
        }

        self.report_telemetry(delta.timing.elapsed_ms);
    }

    /// Report where the run ended up.
    ///
    /// The checksum is the point: it is the same 64-bit digest
    /// `straf3-headless` prints, so a recorded session replayed through the
    /// headless runner is compared by reading two numbers rather than by
    /// eyeballing two positions — a last-bit divergence that would grow into a
    /// visibly different run 30 seconds later is invisible to the eye and
    /// obvious to this.
    fn finish(&self) {
        let state = self.game.state();
        log::info!(
            "final: tick {} sim {} ms origin ({} {} {}) checksum {:#018x}",
            state.tick,
            state.time_ms,
            state.player.origin.x,
            state.player.origin.y,
            state.player.origin.z,
            state.checksum(),
        );
        if self.game.step().dropped_total_ms() > 0 {
            log::warn!(
                "{} ms of wall time was dropped to the per-frame tick cap over this session",
                self.game.step().dropped_total_ms()
            );
        }
    }

    /// A speed readout, once a second.
    ///
    /// The overlay now draws the same numbers on screen, but this line stays:
    /// it is the only readout that survives into a redirected log file, which
    /// is what an unattended `--exit-after` run leaves behind.
    fn report_telemetry(&mut self, now_ms: u64) {
        if now_ms.saturating_sub(self.last_telemetry_ms) < TELEMETRY_INTERVAL_MS {
            return;
        }
        let elapsed_ms = now_ms - self.last_telemetry_ms;
        self.last_telemetry_ms = now_ms;
        // Whole-millisecond arithmetic only: this is a frames-per-second
        // readout, not the criterion-3 duration-to-seconds conversion, and it
        // must never let a float-seconds value exist even transiently.
        // `fps_milli` is thousandths of a frame-per-second, so the final
        // division by 1000 lands on whole fps without ever multiplying a
        // duration by a scale-of-a-thousand literal.
        let fps_milli = self.frames * 1_000_000 / elapsed_ms.max(1);
        let fps = fps_milli / 1000;
        self.last_fps = fps as u32;
        self.frames = 0;

        let state = self.game.state();
        log::info!(
            "speed {:>6.1} ups   origin ({:>8.1} {:>8.1} {:>8.1})   {}   \
             tick {}   sim {} ms   {} fps",
            self.game.horizontal_speed(),
            state.player.origin.x,
            state.player.origin.y,
            state.player.origin.z,
            if state.player.ground.is_grounded() {
                "ground"
            } else if state.player.ground.is_on_plane() {
                "slide "
            } else {
                "air   "
            },
            state.tick,
            state.time_ms,
            fps,
        );
    }

    /// Take the pointer for mouse-look. On web this only succeeds inside a
    /// user gesture, which is why it is also called on click.
    fn grab_pointer(&mut self) {
        if self.grab == PointerGrab::Grabbed {
            return;
        }
        if let Some(window) = &self.window {
            self.grab = straf3_platform::grab_pointer(window);
        }
    }

    fn release_pointer(&mut self) {
        if let Some(window) = &self.window {
            self.grab = straf3_platform::release_pointer(window);
        }
        self.game.input.release_all();
    }
}

impl ApplicationHandler for App {
    fn resumed(&mut self, event_loop: &ActiveEventLoop) {
        if self.window.is_some() {
            return;
        }
        let window = match event_loop.create_window(self.options.window.attributes()) {
            Ok(window) => Arc::new(window),
            Err(e) => {
                log::error!("could not create a window: {e}");
                event_loop.exit();
                return;
            }
        };
        self.window = Some(window.clone());

        #[cfg(all(feature = "render", target_arch = "wasm32"))]
        {
            // The host page already established that WebGPU is available (see
            // `crate::start_web`), so the backend set is stated explicitly and
            // narrowly. Handing wgpu a wider set would let it pick a backend
            // the page has not checked, and it does not degrade gracefully
            // when that fails — it crashes inside the backend (spec rev 6 §Q2).
            self.renderer = Some(straf3_render::Renderer::with_backends(
                window.clone(),
                wgpu::Backends::BROWSER_WEBGPU,
                scene_mesh(),
            ));
        }
        #[cfg(all(feature = "render", not(target_arch = "wasm32")))]
        {
            self.renderer = Some(straf3_render::Renderer::new(window.clone(), scene_mesh()));
        }
        #[cfg(not(feature = "render"))]
        log::warn!(
            "built without the `render` feature: the window opens and input \
             drives the simulation, but nothing is drawn"
        );

        // Native can take the pointer immediately. The browser refuses outside
        // a user gesture, so on web the first click does it (see `MouseInput`).
        #[cfg(not(target_arch = "wasm32"))]
        self.grab_pointer();

        log::info!(
            "straf3 {} — world {:?}, {} profile, {} Hz ({} ms commands). \
             Click to capture the mouse, Esc to release, R to respawn.",
            env!("CARGO_PKG_VERSION"),
            self.options.world,
            self.options.profile_name,
            self.options.rate.hz(),
            self.options.rate.command_millis(),
        );

        // Startup is not gameplay, and the first frame has no previous frame
        // to be measured against. Both are handled by priming the clock at the
        // top of that first frame rather than here: on web there is a further
        // gap between the window appearing and the first redraw — module
        // instantiation, the async device request — and charging it to the
        // simulation makes the very first frame try to run hundreds of ticks
        // at once. Measured in headless Chrome: 651 ticks wanted in frame one.
        self.primed = false;
        self.last_telemetry_ms = self.clock.now().elapsed_ms;

        window.request_redraw();
    }

    fn window_event(&mut self, event_loop: &ActiveEventLoop, _id: WindowId, event: WindowEvent) {
        match &event {
            WindowEvent::CloseRequested => {
                self.finish();
                event_loop.exit();
                return;
            }
            WindowEvent::Resized(size) => {
                #[cfg(feature = "render")]
                if let Some(renderer) = &mut self.renderer {
                    renderer.resize(size.width, size.height);
                }
                #[cfg(not(feature = "render"))]
                let _ = size;
            }
            WindowEvent::RedrawRequested => {
                self.frame(event_loop);
                return;
            }
            WindowEvent::MouseInput {
                state: ElementState::Pressed,
                ..
            } => {
                // Doubles as the user gesture the browser demands before it
                // will grant pointer lock.
                self.grab_pointer();
            }
            WindowEvent::Focused(false) => {
                self.release_pointer();
                return;
            }
            // The two keys the game itself answers, rather than passing to the
            // input state: they are commands to the *session*, not movement.
            WindowEvent::KeyboardInput { event: key, .. }
                if key.state == ElementState::Pressed && !key.repeat =>
            {
                match key.physical_key {
                    PhysicalKey::Code(KeyCode::Escape) => {
                        self.release_pointer();
                        return;
                    }
                    PhysicalKey::Code(KeyCode::KeyR) => {
                        self.game.respawn();
                        log::info!("respawned");
                        return;
                    }
                    _ => {}
                }
            }
            _ => {}
        }

        self.game.input.apply_window_event(&event);
    }

    fn device_event(&mut self, _loop: &ActiveEventLoop, _id: DeviceId, event: DeviceEvent) {
        // Mouse motion only turns the view while the pointer is captured —
        // otherwise moving the mouse across a windowed build would spin the
        // camera while the player is clicking on something else.
        if self.grab == PointerGrab::Grabbed {
            self.game.input.apply_device_event(&event);
        }
    }

    fn about_to_wait(&mut self, _event_loop: &ActiveEventLoop) {
        if let Some(window) = &self.window {
            window.request_redraw();
        }
    }
}

/// Build the event loop and hand it the app.
///
/// Native runs the loop to completion and returns the session's recording, if
/// one was asked for. Web *cannot*: the browser's event loop never returns to
/// its caller, so winit's `spawn_app` takes ownership, this function returns
/// immediately, and there is nothing to hand back. That difference is
/// structural, not a convenience, which is why it is spelled out here rather
/// than hidden.
pub fn run(options: Options) -> Option<String> {
    let event_loop = match EventLoop::new() {
        Ok(event_loop) => event_loop,
        Err(e) => {
            log::error!("could not create an event loop: {e}");
            return None;
        }
    };
    // Poll rather than Wait: the simulation is paced by the clock, and a frame
    // that runs no ticks still costs almost nothing, so there is no reason to
    // sleep until an input event arrives.
    event_loop.set_control_flow(ControlFlow::Poll);

    let app = App::new(options);

    #[cfg(target_arch = "wasm32")]
    {
        use winit::platform::web::EventLoopExtWebSys;
        event_loop.spawn_app(app);
        None
    }

    #[cfg(not(target_arch = "wasm32"))]
    {
        let mut app = app;
        if let Err(e) = event_loop.run_app(&mut app) {
            log::error!("event loop stopped: {e}");
        }
        app.fixture()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_default_options_are_the_ones_the_spec_chose() {
        let options = Options::default();
        assert_eq!(options.rate, TickRate::HZ_125);
        assert_eq!(options.rate.command_millis(), 8);
        assert_eq!(options.profile, PhysicsProfile::cpm());
        assert_eq!(options.world, WorldChoice::Map);
        assert!(!options.record);
    }

    #[test]
    fn building_an_app_opens_no_window_and_touches_no_gpu() {
        // Constructing `App` must be inert: this test runs in CI with no
        // display server and no adapter.
        let app = App::new(Options {
            world: WorldChoice::Flat,
            ..Options::default()
        });
        assert!(app.window.is_none());
        assert_eq!(app.game().state().tick, 0);
        assert_eq!(app.options().world, WorldChoice::Flat);
    }

    #[test]
    fn an_unavailable_world_is_resolved_at_construction_not_at_first_frame() {
        let app = App::new(Options {
            world: WorldChoice::Map,
            ..Options::default()
        });
        // No map is installed in this test process, so `Map` is unavailable and
        // `App::new` must already have fallen back rather than leaving a world
        // that would panic — or silently draw nothing — at the first frame.
        assert!(app.options().world.is_available());
        assert_eq!(app.options().world, WorldChoice::Flat);
    }
}
