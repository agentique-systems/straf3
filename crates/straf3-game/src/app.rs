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
    /// Where personal bests are kept, or `None` to neither load nor save one.
    ///
    /// With a directory set, the session loads the best run saved for this map
    /// and this profile, races it as a ghost, and writes a new file whenever a
    /// finished run beats it. Recording is turned on for the whole session as a
    /// consequence — a run that was not recorded cannot be saved, and a player
    /// does not know in advance which attempt is going to be the good one.
    pub pb_dir: Option<String>,
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
            pb_dir: Some(crate::pb::DEFAULT_DIR.to_owned()),
            exit_after_ms: None,
            window: WindowConfig::straf3(),
        }
    }
}

/// How often the console line reporting speed is printed, in wall ms.
const TELEMETRY_INTERVAL_MS: u64 = 1_000;

/// `m:ss.mmm`, by integer division only.
///
/// The overlay has its own copy of this (`straf3_devtools::format_clock_ms`)
/// and this is not a duplicate of it by accident: the overlay is behind the
/// `render` feature, and a `--no-default-features` build still finishes runs
/// and still saves times, so the log line cannot depend on it. Whole
/// milliseconds throughout — a run time never becomes a float (spec: no float
/// seconds, anywhere).
fn clock_ms(ms: u32) -> String {
    let rest = ms % 60_000;
    format!("{}:{:02}.{:03}", ms / 60_000, rest / 1_000, rest % 1_000)
}

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
    /// Where this session's personal best is read from and written to, once
    /// the world and the profile are known. `None` when personal bests are off
    /// or the world cannot produce a time.
    pb_path: Option<String>,
    /// The best saved run for this map and profile, as loaded.
    ///
    /// Kept beside the ghost because the two answer different questions: this
    /// one says *what the time to beat is*, and is what a new run is compared
    /// against; the ghost is where that run *was*, which only exists if the
    /// recording could actually be re-simulated here.
    personal_best: Option<straf3_replay::Recording>,
    /// The personal best, re-simulated and ready to race.
    ghost: Option<crate::ghost::Ghost>,
    /// Whether the run currently on the clock has already been saved.
    ///
    /// A finished run stays finished until the player respawns, so without
    /// this the same run would be re-saved on every frame after the line.
    run_saved: bool,
    /// Milliseconds ahead of (negative) or behind (positive) the ghost, as of
    /// the last frame.
    split_ms: Option<i32>,
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
        // A personal best needs the commands of whichever attempt turns out to
        // be the good one, and nobody knows that in advance — so recording is
        // on for the whole session whenever personal bests are.
        let pb_path = options.pb_dir.as_ref().map(|dir| {
            crate::pb::path_in(dir, world.name(), &options.profile_name)
        });
        if options.record || pb_path.is_some() {
            game.record();
        }

        let (personal_best, ghost) = match &pb_path {
            Some(path) => Self::load_personal_best(path, world, &options.profile),
            None => (None, None),
        };

        Self {
            options: Options { world, ..options },
            window: None,
            clock: Clock::new(),
            game,
            grab: PointerGrab::Released,
            primed: false,
            last_telemetry_ms: 0,
            frames: 0,
            pb_path,
            personal_best,
            ghost,
            run_saved: false,
            split_ms: None,
            last_fps: 0,
            #[cfg(feature = "render")]
            renderer: None,
            #[cfg(feature = "render")]
            hud: None,
        }
    }

    /// Read the saved personal best for this world, and re-simulate it into a
    /// ghost.
    ///
    /// Every way this can fail is a log line and a session that plays on. A
    /// missing file is the ordinary case (nobody has run the course yet); a
    /// file that will not decode, or that was set on geometry this build no
    /// longer compiles to, is a real finding and is *said out loud* — but none
    /// of them is a reason to refuse to start the game.
    fn load_personal_best(
        path: &str,
        world: WorldChoice,
        profile: &PhysicsProfile,
    ) -> (
        Option<straf3_replay::Recording>,
        Option<crate::ghost::Ghost>,
    ) {
        let recording = match crate::pb::fetch(path) {
            Ok(recording) => recording,
            Err(crate::pb::PbError::Io(e)) if e.kind() == std::io::ErrorKind::NotFound => {
                log::info!("no personal best saved at {path} yet — this session sets it");
                return (None, None);
            }
            Err(e) => {
                log::warn!("ignoring the personal best at {path}: {e}");
                return (None, None);
            }
        };

        let Some(world_id) = world.world_id() else {
            log::warn!("no world identity for {world:?}, so {path} cannot be raced");
            return (Some(recording), None);
        };

        // `&world.world()` and not `world.world()`: the recording is checked
        // against the identity of exactly these hulls, which is the C6 promise
        // the ghost depends on.
        match crate::ghost::Ghost::from_recording(&recording, &world.world(), &world_id, profile) {
            Ok(ghost) => {
                log::info!(
                    "personal best {} — racing it as a ghost over {} re-simulated states",
                    clock_ms(ghost.run_time_ms()),
                    ghost.sample_count(),
                );
                (Some(recording), Some(ghost))
            }
            Err(e) => {
                // The case worth being loud about: the map was recompiled, so
                // the saved run is a time on geometry that no longer exists.
                log::warn!("the personal best at {path} cannot be raced here: {e}");
                (Some(recording), None)
            }
        }
    }

    /// Save the run that has just finished, if it beat the one on disk.
    ///
    /// The recording is *made* here rather than accumulated as the run went:
    /// `straf3_replay::Recording::record` re-simulates the commands and takes
    /// the digest from that, so the file's digest belongs to the file's own
    /// command stream by construction. A recorder that folded the digest live
    /// would produce a plausible file for a stream it had dropped a command
    /// from.
    fn save_personal_best_if_better(&mut self) {
        let (Some(path), Some(recorder)) = (self.pb_path.clone(), self.game.recorder()) else {
            return;
        };
        let Some(world_id) = self.options.world.world_id() else {
            return;
        };

        let recording = straf3_replay::Recording::record(
            recorder.start(),
            recorder.commands().to_vec(),
            &self.game.world(),
            world_id.clone(),
            &self.options.profile,
            self.options.profile_name.clone(),
        );

        let candidate = recording.claimed().run_time_ms;
        let current = self
            .personal_best
            .as_ref()
            .and_then(|r| r.claimed().run_time_ms);
        if !crate::pb::beats(candidate, current) {
            if let (Some(new), Some(old)) = (candidate, current) {
                log::info!(
                    "run {} — personal best {} stands, by {} ms",
                    clock_ms(new),
                    clock_ms(old),
                    new - old
                );
            }
            return;
        }

        if let Err(e) = crate::pb::store(&path, &recording) {
            log::error!("could not write the personal best to {path}: {e}");
            return;
        }
        match current {
            Some(old) => log::info!(
                "NEW PERSONAL BEST {} — {} ms faster than {}, saved to {path}",
                clock_ms(candidate.unwrap_or(0)),
                old - candidate.unwrap_or(0),
                clock_ms(old),
            ),
            None => log::info!(
                "FIRST TIME SET {} — saved to {path}",
                clock_ms(candidate.unwrap_or(0))
            ),
        }

        // Race the new best from the next attempt: the ghost is rebuilt from
        // the run just saved, by re-simulating it exactly as a loaded file
        // would be.
        match crate::ghost::Ghost::from_recording(
            &recording,
            &self.game.world(),
            &world_id,
            &self.options.profile,
        ) {
            Ok(ghost) => self.ghost = Some(ghost),
            Err(e) => log::warn!("the run just saved cannot be raced back: {e}"),
        }
        self.personal_best = Some(recording);
    }

    /// Where the run stands against the ghost, this frame.
    ///
    /// `None` before the start line, and whenever there is no ghost — the
    /// overlay then draws no split at all rather than `+0.000`, which would
    /// claim the player was level with a personal best that is not there.
    fn update_split(&mut self) {
        let state = self.game.state();
        let (Some(elapsed_ms), Some(ghost)) = (state.run.elapsed_ms(state.time_ms), &mut self.ghost)
        else {
            self.split_ms = None;
            return;
        };
        self.split_ms = Some(ghost.split_ms(state.player.origin, elapsed_ms));
    }

    /// The session, for a caller that wants the recording out of it afterwards.
    #[must_use]
    pub const fn game(&self) -> &Game<&'static dyn World> {
        &self.game
    }

    /// The ghost being raced, if a personal best was loaded and re-simulated.
    #[must_use]
    pub const fn ghost(&self) -> Option<&crate::ghost::Ghost> {
        self.ghost.as_ref()
    }

    /// The split against the ghost as of the last frame.
    #[must_use]
    pub const fn split_ms(&self) -> Option<i32> {
        self.split_ms
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

        // In this order, and all of it before the frame is drawn: crossing the
        // finish line may replace the ghost being raced, and the split is what
        // the overlay is about to be handed.
        if !self.run_saved && matches!(self.game.state().run, straf3_sim::RunState::Finished { .. })
        {
            self.run_saved = true;
            self.save_personal_best_if_better();
        }
        self.update_split();

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
            // The split is the one number the simulation cannot know: it is a
            // comparison with a run that happened on a different day. `None`
            // when no ghost is loaded, and the overlay then draws no split at
            // all rather than `+0.000`, which would claim the player was level
            // with a personal best that is not there.
            let sample = straf3_devtools::TelemetrySample::of(self.game.state())
                .with_fps(self.last_fps)
                .with_split_ms(self.split_ms);
            // Where the ghost is right now: its own run-elapsed time, matched
            // to the player's, so the two leave the start line together
            // however long either of them loitered at the spawn.
            let ghost = self.ghost.as_ref().map(|ghost| {
                let elapsed = self
                    .game
                    .state()
                    .run
                    .elapsed_ms(self.game.state().time_ms)
                    .unwrap_or(0);
                let at = ghost.sample_at(elapsed);
                let hull = self.options.profile.hull(at.crouched);
                straf3_render::GhostPose {
                    origin: at.origin,
                    yaw: at.yaw,
                    half_extents: hull.half_extents,
                    center_offset: hull.center_offset,
                }
            });
            let pixels_per_point = self
                .window
                .as_ref()
                .map_or(1.0, |w| w.scale_factor() as f32);
            let hud = self.hud.as_mut();
            renderer.render_frame(
                straf3_render::Frame {
                    prev: &self.game.previous().player,
                    curr: &self.game.state().player,
                    alpha: straf3_render::InterpolationAlpha(self.game.alpha()),
                    ghost,
                },
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
        // The run itself, said in the units a player thinks in — and said even
        // when nothing was saved, so a session that ended mid-run does not look
        // like a session that never started one.
        match state.run {
            straf3_sim::RunState::NotStarted => log::info!("no run: the start line was not crossed"),
            straf3_sim::RunState::Running { .. } => log::info!(
                "run unfinished: {} on the clock at the finish line",
                clock_ms(state.run.elapsed_ms(state.time_ms).unwrap_or(0))
            ),
            straf3_sim::RunState::Finished { .. } => log::info!(
                "run {}{}",
                clock_ms(state.run.elapsed_ms(state.time_ms).unwrap_or(0)),
                match self.split_ms {
                    Some(split) => format!(" ({split:+} ms against the ghost)"),
                    None => String::new(),
                }
            ),
        }
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
        // The run clock and the split ride on the same line rather than on one
        // of their own: a log with one line a second is readable, and a log
        // with three is a wall.
        let run = match state.run.elapsed_ms(state.time_ms) {
            Some(ms) => format!(
                "   run {}{}",
                clock_ms(ms),
                match self.split_ms {
                    Some(split) => format!(" {split:+} ms"),
                    None => String::new(),
                }
            ),
            None => String::new(),
        };
        log::info!(
            "speed {:>6.1} ups   origin ({:>8.1} {:>8.1} {:>8.1})   {}   \
             tick {}   sim {} ms   {} fps{run}",
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
                        // A new attempt: the run clock is back at NotStarted,
                        // the recorder has started again from the spawn, and
                        // the ghost has to be matched from the start line
                        // rather than from wherever the last attempt died.
                        self.run_saved = false;
                        self.split_ms = None;
                        if let Some(ghost) = &mut self.ghost {
                            ghost.rewind();
                        }
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
