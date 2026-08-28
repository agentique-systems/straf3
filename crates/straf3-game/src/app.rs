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

use std::fmt::Write as _;
use std::sync::Arc;

use straf3_platform::{Clock, PointerGrab, WindowConfig};
use straf3_sim::num::{Scalar, Vec3};
use straf3_sim::{PhysicsProfile, TickRate, UserCmd, World};
use winit::application::ApplicationHandler;
use winit::event::{DeviceEvent, DeviceId, ElementState, WindowEvent};
use winit::event_loop::{ActiveEventLoop, ControlFlow, EventLoop};
use winit::keyboard::{KeyCode, PhysicalKey};
use winit::window::{Window, WindowId};

use crate::game::Game;
use crate::scene::WorldChoice;

/// A recorded command stream to drive a windowed session from.
///
/// # Why the spawn travels with the commands
///
/// A command stream is meaningless without the state it was applied from: the
/// same commands from a different origin re-simulate to a different run. The
/// recording says where it began, and that is what the session must spawn at —
/// not the map's spawn marker, which can be moved without changing the map's
/// collision identity. So the spawn is carried here rather than looked up.
#[derive(Debug, Clone)]
pub struct Playback {
    /// The commands, in order, expanded from their repeat counts.
    pub cmds: Vec<UserCmd>,
    /// Where the recorded run began.
    pub spawn: Vec3,
    /// Which way the recorded player faced at that spawn, in degrees.
    pub yaw: Scalar,
    /// Where the stream came from, for the log line that says what is playing.
    pub source: String,
}

/// Somewhere to hand a run that has just crossed the finish line.
///
/// # Why this exists rather than the session writing the file itself
///
/// Natively a finished run goes to `runs/<map>.<profile>.s3d` and that is the
/// end of it. In the browser there is no filesystem and the page — not this
/// crate — decides what happens to the bytes: submit them, offer them as a
/// download, or both. The wave contract is explicit that the client never
/// talks to `/v1` and never sees a token, so the run leaves through here and
/// the decision is made above.
///
/// The [`straf3_replay::Recording`] is handed over rather than raw bytes
/// because the two callers want different serialisations of it: a personal
/// best is stored without the per-command checksum trace (a ghost does not
/// need one), and a run used as *evidence* is stored with it, so that a
/// disagreement names the first diverging command instead of merely reporting
/// that two machines disagree.
pub trait RunSink: core::fmt::Debug {
    /// A run has finished. Called once per run, on the frame the finish line
    /// was crossed, before the personal best is considered.
    fn finished(&self, recording: &straf3_replay::Recording);
}

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
    /// Drive the session from this recorded stream instead of from the
    /// keyboard, with a window open and the frame drawn.
    ///
    /// This is `--play`. It is the same simulation the headless `--replay`
    /// runs — the commands go through [`Game::advance`] either way — with a
    /// window on the front of it, which is what lets a complete run be driven
    /// deterministically on a real GPU and what "watch a record" is built out
    /// of.
    pub playback: Option<Playback>,
    /// Write a per-frame timing log to this path when the session ends.
    ///
    /// Measurement only. It never reaches the simulation, which keeps taking
    /// whole-millisecond deltas from [`Clock`] through exactly the code path it
    /// uses without this flag — a measurement that changed what it measures
    /// would be worthless (coordinator decision D-B7).
    pub pacing_log: Option<String>,
    /// A run to race as a ghost, supplied by the caller rather than read from
    /// [`Self::pb_dir`].
    ///
    /// This is how `?ghost=<run>` reaches the session: the browser has no
    /// filesystem to keep personal bests in, so the recording arrives over the
    /// network and is handed straight in. It takes precedence over the saved
    /// personal best when both are present — a URL that names a ghost is asking
    /// to race *that* one.
    ///
    /// A recording that cannot be raced here (recompiled geometry, different
    /// physics) is reported and dropped, and the session plays on without a
    /// ghost. URLS.md §4 behaviour 4: a missing ghost is not a reason to refuse
    /// the map.
    pub ghost: Option<straf3_replay::Recording>,
    /// Where a finished run goes, in addition to the personal-best file.
    pub run_sink: Option<Arc<dyn RunSink>>,
    /// The window (or canvas) to open.
    pub window: WindowConfig,
}

/// High-resolution frame deltas, collected for `--pacing-log`.
///
/// # Why `Instant` and not [`Clock`]
///
/// [`Clock`] hands the simulation whole milliseconds, and that is the contract
/// the fixed step is built on. 165 fps is 6.06 ms, so whole-millisecond
/// truncation would destroy a p99 before anyone saw it. This reads the
/// high-resolution clock alongside, and the simulation never sees the number.
///
/// # Why the samples are kept in memory
///
/// Formatting a line and touching the filesystem inside the frame loop would
/// put the cost of the measurement into the measurement. Pushing a `u64` into a
/// pre-reserved `Vec` does not allocate; the file is written once, at exit.
struct PacingLog {
    path: String,
    /// The start of the previous frame. The first frame has no predecessor and
    /// contributes no sample.
    last: Option<std::time::Instant>,
    deltas_ns: Vec<u64>,
    /// The capacity reserved up front, so the file can say whether the hot path
    /// ever had to reallocate — that would be one distorted sample, and a
    /// distortion nobody was told about is the kind this project does not ship.
    reserved: usize,
}

impl PacingLog {
    /// Frames reserved up front: about 1.7 hours at 165 fps, against GPU runs
    /// that are `--exit-after`-bounded to seconds. 8 MiB, allocated once.
    const RESERVE: usize = 1 << 20;

    fn new(path: String) -> Self {
        let deltas_ns = Vec::with_capacity(Self::RESERVE);
        Self {
            path,
            last: None,
            reserved: deltas_ns.capacity(),
            deltas_ns,
        }
    }

    /// Record the start of a frame. Call it once, first thing.
    fn frame(&mut self, now: std::time::Instant) {
        if let Some(last) = self.last {
            self.deltas_ns.push(
                now.duration_since(last)
                    .as_nanos()
                    .min(u128::from(u64::MAX)) as u64,
            );
        }
        self.last = Some(now);
    }

    /// The measurements, warm-up excluded: `(true frame index, delta_ns)`.
    ///
    /// The first *rendered* frame contributes nothing — it has no predecessor.
    /// The interval after it is swapchain warm-up, and it is not a frame time:
    /// two runs of the same session on the 3060 Ti measured 49 ms and 421 ms
    /// against a steady 6. So it is not a data row either. See [`Self::to_csv`].
    fn measurements(&self) -> impl Iterator<Item = (usize, u64)> + '_ {
        self.deltas_ns
            .iter()
            .enumerate()
            .skip(1)
            .map(|(frame, delta_ns)| (frame, *delta_ns))
    }

    /// The swapchain warm-up interval, which is reported but is not a frame
    /// time.
    fn warmup_ns(&self) -> Option<u64> {
        self.deltas_ns.first().copied()
    }

    /// The CSV, in the format `cargo xtask pacing` parses (D-B7).
    ///
    /// # Why the warm-up is not a row
    ///
    /// It used to be row 0, with a `#` comment saying to exclude it. The
    /// consumers of this file are a parser and a reviewer skimming for a
    /// number, and neither reads header prose — so a parser taking every data
    /// row would publish a 421 ms worst-case frame time on a 165 Hz display,
    /// and it would look like a finding rather than a swapchain coming up. It
    /// is now `warmup_ns` in the header, still available to anyone who wants
    /// it, and impossible to include by accident.
    ///
    /// `frame` stays the **true** frame index, so the data starts at 1 rather
    /// than being renumbered from 0. Renumbering would misalign this file
    /// against the session's own logs to save one line of parsing.
    fn to_csv(&self) -> String {
        // One row is at most 27 characters; sizing for that up front keeps the
        // exit path from growing the string a hundred thousand times.
        let mut out = String::with_capacity(32 * self.deltas_ns.len() + 512);
        // v2, not v1: the data section's meaning changed — the warm-up interval
        // used to be a row and is not one any more. A parser written against v1
        // must refuse this file rather than quietly read one fewer sample than
        // it expects, which is the same rule that put the warm-up in the header
        // in the first place.
        out.push_str("# straf3 pacing log v2\n");
        out.push_str(&format!(
            "# present_mode_requested={}  build={}\n",
            requested_present_mode(),
            if cfg!(debug_assertions) {
                "debug"
            } else {
                "release"
            },
        ));
        out.push_str(&format!(
            "# warmup_ns={}  frames={}\n",
            self.warmup_ns().unwrap_or(0),
            self.measurements().count(),
        ));
        // The field is named for what it holds, not for what a reader wants it
        // to hold. This crate asks for nothing and configures nothing:
        // `straf3-render` picks the mode and logs the one it actually got,
        // including a fallback when the adapter does not support the request.
        // A field called `present_mode` would have been read as the mode
        // measured, which is not a fact this file is in a position to state.
        out.push_str(
            "# present_mode_requested is the STRAF3_PRESENT_MODE value; the renderer's \
             own log line records what was configured\n",
        );
        out.push_str(
            "# warmup_ns is the first-to-second rendered frame: swapchain warm-up, not a \
             frame time, and NOT a data row below\n",
        );
        if self.deltas_ns.len() > self.reserved {
            out.push_str(&format!(
                "# WARNING: {} frames exceeded the {} reserved, so at least one sample \
                 includes a reallocation\n",
                self.deltas_ns.len(),
                self.reserved,
            ));
        }
        out.push_str("frame,delta_ns\n");
        for (frame, delta_ns) in self.measurements() {
            // `writeln!` rather than `push_str(&format!(..))`: this runs once
            // per frame of the session, and the latter allocates a `String` per
            // row only to copy it and drop it. Writing into `out` cannot fail.
            let _ = writeln!(out, "{frame},{delta_ns}");
        }
        out
    }
}

/// The present mode this process was *asked* for, as `straf3-render` reads it
/// (coordinator decision D-B8). `unknown` when nothing asked.
fn requested_present_mode() -> String {
    match std::env::var("STRAF3_PRESENT_MODE") {
        Ok(mode) if !mode.is_empty() => mode,
        _ => "unknown".to_owned(),
    }
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
            playback: None,
            pacing_log: None,
            ghost: None,
            run_sink: None,
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
    /// Whether [`App::split_ms`] is the finished run's result and must not be
    /// recomputed. See [`App::update_split`].
    split_final: bool,
    /// Per-frame timing, when `--pacing-log` asked for it.
    pacing: Option<PacingLog>,
    /// Whether the end of the played stream has already been reported.
    ///
    /// The stream runs out once and the window stays open afterwards, so
    /// without this the "playback finished" line would be printed on every
    /// frame from then on.
    playback_reported: bool,
    /// The last frame rate [`App::report_telemetry`] computed, for the overlay.
    ///
    /// The overlay draws every frame and the rate is only measured once a
    /// second, so the number it shows is the last one measured rather than a
    /// per-frame reciprocal — which at 300 fps would be unreadable noise.
    last_fps: u32,
    #[cfg(feature = "render")]
    renderer: Option<straf3_render::Renderer>,
    /// The on-screen overlay, built on the first frame the device exists.
    #[cfg(feature = "devtools")]
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
        // A played session spawns where the *recording* began, not where the
        // map's spawn marker is: moving a marker deliberately does not change
        // the map's collision identity, so only the file can say where its own
        // commands were applied from.
        let (spawn, spawn_yaw) = options
            .playback
            .as_ref()
            .map_or_else(|| world.spawn(), |p| (p.spawn, p.yaw));
        let mut game = Game::new(
            world.world(),
            options.profile,
            options.rate,
            spawn,
            spawn_yaw,
        );
        Self::announce_profile(&options.profile_name);
        // A personal best needs the commands of whichever attempt turns out to
        // be the good one, and nobody knows that in advance — so recording is
        // on for the whole session whenever personal bests are.
        let pb_path = options
            .pb_dir
            .as_ref()
            .map(|dir| crate::pb::path_in(dir, world.name(), &options.profile_name));
        if options.record || pb_path.is_some() {
            game.record();
        }
        // After `record()`, so the recorder is in place before the first
        // command is applied: the played stream has to land in the recording
        // too, or a played run that finishes would save nothing.
        if let Some(playback) = &options.playback {
            log::info!(
                "playing {} — {} commands at {} Hz ({} of simulated time), \
                 spawn ({} {} {}) yaw {}",
                playback.source,
                playback.cmds.len(),
                options.rate.hz(),
                // Saturating rather than `as u32`: this is a log line, and a
                // stream long enough to overflow it should print a wrong
                // duration rather than a wrapped one.
                clock_ms(
                    u32::try_from(
                        playback.cmds.len() as u64 * u64::from(options.rate.command_millis())
                    )
                    .unwrap_or(u32::MAX)
                ),
                spawn.x,
                spawn.y,
                spawn.z,
                spawn_yaw,
            );
            game.play(playback.cmds.clone());
        }

        // A ghost handed in by the caller wins over the saved personal best:
        // `?ghost=<run>` is a request to race *that* run, and quietly racing a
        // local file instead would put a different opponent on screen from the
        // one the link named. It is not a personal best either — it is somebody
        // else's run as often as not — so it is raced without becoming the time
        // this session is measured against.
        let (personal_best, ghost) = match (&options.ghost, &pb_path) {
            (Some(recording), _) => (
                None,
                Self::race(recording, world, &options.profile, &options.profile_name),
            ),
            (None, Some(path)) => {
                Self::load_personal_best(path, world, &options.profile, &options.profile_name)
            }
            (None, None) => (None, None),
        };

        // Reserved here rather than on the first frame: an 8 MiB allocation is
        // not something to do inside the loop being measured.
        let pacing = options.pacing_log.clone().map(PacingLog::new);

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
            split_final: false,
            pacing,
            playback_reported: false,
            last_fps: 0,
            #[cfg(feature = "render")]
            renderer: None,
            #[cfg(feature = "devtools")]
            hud: None,
        }
    }

    /// Say what a non-canonical profile means before anyone plays under it.
    ///
    /// Two facts, and neither is inferable from the window: an experimental
    /// time is never ranked against `cpm` or `vq3` (spec D2), and — until
    /// `straf3-sim` lands the constants — the profile is a placeholder that
    /// plays exactly like CPM. A session that felt identical to canon and said
    /// nothing would be indistinguishable from one where the new mechanics
    /// simply did nothing, which is the single most misleading thing this flag
    /// could do.
    fn announce_profile(profile_name: &str) {
        if crate::profile::is_canon(profile_name) {
            return;
        }
        log::warn!(
            "profile `{profile_name}` is not canon: its personal bests are kept under \
             their own name and are never ranked against a cpm or vq3 time"
        );
        if profile_name == "experimental" && crate::profile::is_stub() {
            log::warn!(
                "`experimental` is currently CPM's constants — straf3-sim has not landed \
                 PhysicsProfile::experimental() yet, so this session is experimental in \
                 name and record-keeping only, not in how it plays"
            );
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
        profile_name: &str,
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

        // A recording that cannot be raced is still the time to beat — hence
        // `Some(recording)` on both arms below. Only the *ghost* is optional.
        let ghost = Self::race(&recording, world, profile, profile_name);
        if ghost.is_none() {
            log::warn!("the personal best at {path} is not being raced this session");
        }
        (Some(recording), ghost)
    }

    /// Re-simulate `recording` into a ghost that can be raced here, or say why
    /// it cannot be.
    ///
    /// Every refusal is a log line and a session that plays on. This is the one
    /// place a recording becomes an on-screen opponent, whether it arrived from
    /// `runs/` or from `?ghost=<run>`, so the checks cannot be different for
    /// the two.
    fn race(
        recording: &straf3_replay::Recording,
        world: WorldChoice,
        profile: &PhysicsProfile,
        profile_name: &str,
    ) -> Option<crate::ghost::Ghost> {
        // The second half of spec D2's rule, and the half the file name cannot
        // enforce: `runs/<map>.<profile>.s3d` means a session never *opens*
        // another profile's record, but a file copied or renamed into this
        // namespace — or named by a URL — would be raced and ranked as if it
        // belonged here. An experimental time is not a CPM time, so this is
        // refused by the name the recording carries rather than trusted to the
        // path it was found at.
        //
        // The physics digest below would catch it once the profiles' constants
        // actually differ. It does not while `experimental` is still CPM's
        // constants (see `crate::profile::is_stub`), which is exactly why this
        // check does not depend on it.
        if recording.physics().name != profile_name {
            log::warn!(
                "not racing a run set under the `{}` profile in a `{profile_name}` session. \
                 Times under different profiles are different games and are never ranked \
                 against each other.",
                recording.physics().name,
            );
            return None;
        }

        let world_id = world.world_id().or_else(|| {
            log::warn!("no world identity for {world:?}, so nothing can be raced in it");
            None
        })?;

        // `&world.world()` and not `world.world()`: the recording is checked
        // against the identity of exactly these hulls, which is the C6 promise
        // the ghost depends on.
        match crate::ghost::Ghost::from_recording(recording, &world.world(), &world_id, profile) {
            Ok(ghost) => {
                log::info!(
                    "racing {} as a ghost over {} re-simulated states",
                    clock_ms(ghost.run_time_ms()),
                    ghost.sample_count(),
                );
                Some(ghost)
            }
            Err(e) => {
                // The case worth being loud about: the map was recompiled, so
                // the run is a time on geometry that no longer exists.
                log::warn!("that run cannot be raced here: {e}");
                None
            }
        }
    }

    /// Hand the run that has just finished to everyone who wants it.
    ///
    /// The recording is built **once**, here, and offered to both consumers.
    /// Building it twice would re-simulate 7 500 commands for nothing, and —
    /// worse — would invite the two copies to be built from different
    /// arguments one day, so that the bytes submitted and the bytes saved
    /// described different runs.
    ///
    /// The recording is *made* rather than accumulated as the run went:
    /// `straf3_replay::Recording::record` re-simulates the commands and takes
    /// the digest from that, so the file's digest belongs to the file's own
    /// command stream by construction. A recorder that folded the digest live
    /// would produce a plausible file for a stream it had dropped a command
    /// from.
    fn report_finished_run(&mut self) {
        // Nothing wants it: do not pay for the re-simulation. This keeps a
        // `--record`-only native session exactly as cheap as it was before the
        // sink existed.
        if self.pb_path.is_none() && self.options.run_sink.is_none() {
            return;
        }
        let (Some(recorder), Some(world_id)) =
            (self.game.recorder(), self.options.world.world_id())
        else {
            return;
        };

        let recording = straf3_replay::Recording::record(
            recorder.start(),
            recorder.commands().to_vec(),
            &self.game.world(),
            world_id,
            &self.options.profile,
            self.options.profile_name.clone(),
        );

        // The sink first, and unconditionally: a run that did not beat the
        // personal best is still a run the page may want to submit, download or
        // show. Ranking is somebody else's decision and it is made elsewhere.
        if let Some(sink) = self.options.run_sink.clone() {
            sink.finished(&recording);
        }
        self.save_personal_best_if_better(recording);
    }

    /// Save the run that has just finished, if it beat the one on disk.
    fn save_personal_best_if_better(&mut self, recording: straf3_replay::Recording) {
        let Some(path) = self.pb_path.clone() else {
            return;
        };
        let Some(world_id) = self.options.world.world_id() else {
            return;
        };

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
    ///
    /// # At the finish there is nothing to approximate
    ///
    /// [`crate::ghost::Ghost::split_ms`] matches the player onto the ghost's
    /// path by *position*, because mid-run there are no checkpoint times to
    /// difference. At the line both runs have a time, so the split is the
    /// subtraction and no heuristic is involved.
    ///
    /// That is not a refinement, it is the difference between a true number and
    /// a false one. The split is computed once per *frame*, so the frame that
    /// crosses the line has usually run several commands past it, and the
    /// nearest-point match then answers for wherever the player ended that
    /// frame. Measured: a recording raced against itself reported `+8 ms` — a
    /// whole tick of deficit for a run that tied — and the figure grew with the
    /// frame delta, which is exactly the frame-rate dependence the rest of this
    /// crate exists to keep out of what the player sees.
    ///
    /// # A finished run's split stops moving
    ///
    /// Once the line is crossed the split is a *result*. The run clock has
    /// stopped, but the player has not: they carry on past the finish while the
    /// ghost's path has ended, so recomputing would drag the number away from
    /// the one that was true at the line — and the figure left on screen, in
    /// the log and in a screenshot would be a comparison against a stretch of
    /// course the run did not include. A run that finished with no ghost
    /// loaded therefore keeps *no* split, rather than acquiring `+0` against
    /// the record it has just set.
    fn update_split(&mut self) {
        if self.split_final {
            return;
        }
        let state = self.game.state();
        let run = state.run;
        let finished = matches!(run, straf3_sim::RunState::Finished { .. });
        match (run.elapsed_ms(state.time_ms), &mut self.ghost) {
            (Some(elapsed_ms), Some(ghost)) if finished => {
                let difference = i64::from(elapsed_ms) - i64::from(ghost.run_time_ms());
                self.split_ms = Some(
                    difference
                        .clamp(i64::from(i32::MIN), i64::from(i32::MAX))
                        .try_into()
                        .expect("clamped into range"),
                );
            }
            (Some(elapsed_ms), Some(ghost)) => {
                self.split_ms = Some(ghost.split_ms(state.player.origin, elapsed_ms));
            }
            _ => self.split_ms = None,
        }
        self.split_final = finished;
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

    /// Everything a frame does to the *session*, with no window and no
    /// renderer in it: step the simulation by `delta_ms` of wall time, save a
    /// run that has just finished, and recompute the split.
    ///
    /// Returns whether `--exit-after` has come due, `session_elapsed_ms` being
    /// how long the session has been running.
    ///
    /// # Why this is public
    ///
    /// A windowed playback has to be checkable *without* a window, or the claim
    /// "the run on screen is bit-identical to the headless replay of the same
    /// file" is only ever an eyeball comparison. A test drives this over a
    /// deliberately hostile frame schedule and compares the resulting checksum
    /// with [`crate::replay::replay`]'s. The winit redraw handler calls this and
    /// then draws — there is no second stepping path for a test to be fooled by.
    pub fn simulate_frame(&mut self, delta_ms: u64, session_elapsed_ms: u64) -> bool {
        self.game.advance(delta_ms);
        self.report_playback_end();

        // The split first, and all of it before the frame is drawn. Crossing
        // the line may replace the ghost with the run that has just been set,
        // and the number a player wants at the finish is how they did against
        // the record they were *racing* — measured against themselves it is
        // zero by construction, whatever they took off the record.
        self.update_split();
        if !self.run_saved && matches!(self.game.state().run, straf3_sim::RunState::Finished { .. })
        {
            self.run_saved = true;
            self.report_finished_run();
        }

        match self.options.exit_after_ms {
            Some(limit) if session_elapsed_ms >= limit => {
                log::info!("--exit-after {limit} ms reached");
                true
            }
            _ => false,
        }
    }

    /// One frame: read the clock, run whatever ticks that buys, draw.
    fn frame(&mut self, event_loop: &ActiveEventLoop) {
        // First thing in the frame, and beside the simulation's clock rather
        // than in front of it: the number below never reaches `Game::advance`.
        if let Some(pacing) = &mut self.pacing {
            pacing.frame(std::time::Instant::now());
        }
        // Before the input is read into this frame's commands, so a frame that
        // discovers the lock was lost does not also build a command out of keys
        // the player can no longer see they are holding.
        #[cfg(target_arch = "wasm32")]
        self.sync_pointer_lock();
        if !self.primed {
            self.primed = true;
            self.clock.prime();
        }
        let delta = self.clock.frame();
        self.frames += 1;

        if self.simulate_frame(delta.delta_ms, delta.timing.elapsed_ms) {
            self.finish();
            event_loop.exit();
            return;
        }

        #[cfg(feature = "render")]
        if let Some(renderer) = &mut self.renderer {
            // Built on the first frame the device exists — which natively is
            // the first frame and on the web is several frames in.
            #[cfg(feature = "devtools")]
            if self.hud.is_none() {
                self.hud = renderer.with_device(straf3_devtools::Hud::new);
            }
            // The split is the one number the simulation cannot know: it is a
            // comparison with a run that happened on a different day. `None`
            // when no ghost is loaded, and the overlay then draws no split at
            // all rather than `+0.000`, which would claim the player was level
            // with a personal best that is not there.
            #[cfg(feature = "devtools")]
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
            #[cfg(feature = "devtools")]
            let pixels_per_point = self
                .window
                .as_ref()
                .map_or(1.0, |w| w.scale_factor() as f32);
            #[cfg(feature = "devtools")]
            let hud = self.hud.as_mut();
            let frame = straf3_render::Frame {
                prev: &self.game.previous().player,
                curr: &self.game.state().player,
                alpha: straf3_render::InterpolationAlpha(self.game.alpha()),
                ghost,
            };
            // Two calls rather than one with a `cfg` inside the closure: the
            // closure's body is what decides whether `straf3-devtools` is
            // linked at all, and a `cfg!` there would still name the crate.
            // Without `devtools` the overlay hook is handed a closure that
            // does nothing, and egui is absent from the binary — which is what
            // ARCHITECTURE §0 item 7 asks of the web bundle.
            #[cfg(feature = "devtools")]
            renderer.render_frame(frame, |o| {
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
            });
            #[cfg(not(feature = "devtools"))]
            renderer.render_frame(frame, |_| {});
        }

        self.report_telemetry(delta.timing.elapsed_ms);
        #[cfg(target_arch = "wasm32")]
        self.publish_debug_state();
    }

    /// Hand this frame's state to [`crate::web::straf3_debug_state`].
    ///
    /// After the frame is drawn and the telemetry taken, so what a harness
    /// reads is the state the frame it just watched was drawn from.
    #[cfg(target_arch = "wasm32")]
    fn publish_debug_state(&self) {
        let state = self.game.state();
        crate::web::publish_debug_state(crate::web::DebugState {
            tick: state.tick,
            time_ms: state.time_ms,
            origin: (
                state.player.origin.x,
                state.player.origin.y,
                state.player.origin.z,
            ),
            pitch: self.game.input.look.pitch(),
            yaw: self.game.input.look.yaw(),
            speed: self.game.horizontal_speed(),
            grounded: state.player.ground.is_grounded(),
            pointer_locked: self.grab == PointerGrab::Grabbed,
            run: match state.run {
                straf3_sim::RunState::NotStarted => 0,
                straf3_sim::RunState::Running { .. } => 1,
                straf3_sim::RunState::Finished { .. } => 2,
            },
            run_ms: state.run.elapsed_ms(state.time_ms).unwrap_or(0),
            fps: self.last_fps,
        });
    }

    /// Say, once, that the recorded stream has run out.
    ///
    /// The checksum is on this line for the same reason it is on
    /// [`App::finish`]'s: it is the number `straf3 --replay` prints for the
    /// same file, so "the windowed playback and the headless replay agree" is
    /// checked by reading two numbers rather than by watching two windows.
    ///
    /// The session is *not* ended here. Holding the last state with the window
    /// open is what lets the finish-line overlay be read and photographed;
    /// `--exit-after` remains the only thing that closes an unattended run.
    fn report_playback_end(&mut self) {
        if self.playback_reported || self.game.playback_remaining() != Some(0) {
            return;
        }
        self.playback_reported = true;
        let state = self.game.state();
        log::info!(
            "playback finished: {} commands applied, tick {}, sim {} ms, \
             checksum {:#018x} — holding the final state",
            self.options.playback.as_ref().map_or(0, |p| p.cmds.len()),
            state.tick,
            state.time_ms,
            state.checksum(),
        );
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
            straf3_sim::RunState::NotStarted => {
                log::info!("no run: the start line was not crossed")
            }
            // The finish line was NOT reached — that is what `Running` means
            // here. This line used to say "on the clock at the finish line",
            // which read as a completed time and was untrue of every session
            // it was ever printed for.
            straf3_sim::RunState::Running { .. } => log::info!(
                "run unfinished: {} on the clock, still short of the finish line",
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
        self.write_pacing_log();
    }

    /// Write the per-frame timings out, if `--pacing-log` asked for them.
    ///
    /// Once, at the end, for the reason [`PacingLog`] gives. A session killed
    /// rather than closed leaves no file — which is correct: a truncated pacing
    /// log looks exactly like a complete one, and `cargo xtask pacing` would
    /// compute a p99 over however much of the run happened to be flushed.
    fn write_pacing_log(&self) {
        let Some(pacing) = &self.pacing else {
            return;
        };
        // Fewer than three frames leaves nothing but the warm-up interval,
        // which is not a frame time. Writing a file whose data section is empty
        // would hand the analysis a zero-row set to compute a p99 over; saying
        // so and writing nothing is the same choice `--pacing-log` with
        // `--replay` makes, for the same reason.
        let measured = pacing.measurements().count();
        if measured == 0 {
            log::warn!(
                "no frame timings to write to {}: the session drew {} frame(s), and the \
                 only interval a session that short produces is swapchain warm-up",
                pacing.path,
                pacing.deltas_ns.len() + 1,
            );
            return;
        }
        match std::fs::write(&pacing.path, pacing.to_csv()) {
            Ok(()) => log::info!(
                "pacing log written to {} — {} frame deltas (plus {} ns of swapchain \
                 warm-up, reported in the header and excluded from the rows), present \
                 mode requested `{}` (the renderer logs what was configured), {} build",
                pacing.path,
                measured,
                pacing.warmup_ns().unwrap_or(0),
                requested_present_mode(),
                if cfg!(debug_assertions) {
                    "debug"
                } else {
                    "release"
                },
            ),
            Err(e) => log::error!("could not write the pacing log to {}: {e}", pacing.path),
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
        // Foot clearance, to three decimals because the band it exists to make
        // visible is a quarter of a unit wide (`Game::foot_clearance`). One
        // sample a second cannot show a landing — nothing sampled at 1 Hz can
        // say anything about a sub-tick event — so this is here to make the
        // number checkable in an unattended run's log, not to be watched.
        let clearance = match self.game.foot_clearance() {
            Some(units) => format!("   clear {units:.3}"),
            None => String::new(),
        };
        log::info!(
            "speed {:>6.1} ups   origin ({:>8.1} {:>8.1} {:>8.1})   {}   \
             tick {}   sim {} ms   {} fps{run}{clearance}",
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
        let Some(window) = &self.window else {
            return;
        };
        let grab = straf3_platform::grab_pointer(window);
        // Natively the call has either grabbed the pointer or not by the time
        // it returns, so its answer is the answer.
        //
        // On the web it is a *request*: `requestPointerLock()` resolves later,
        // and winit reports success as soon as it has asked. Believing it
        // would mean flipping to `Grabbed` before the browser had agreed —
        // measured, that produced an `onPointerLock(true)` immediately
        // followed by `onPointerLock(false)` as the next frame's
        // reconciliation found `document.pointerLockElement` still null, and
        // only then a second `true` when it was granted 30 ms later. So on the
        // web nothing but `sync_pointer_lock` writes this flag, and the
        // browser's own answer is the only one anybody sees.
        #[cfg(not(target_arch = "wasm32"))]
        {
            self.grab = grab;
        }
        #[cfg(target_arch = "wasm32")]
        let _ = grab;
    }

    fn release_pointer(&mut self) {
        if let Some(window) = &self.window {
            self.grab = straf3_platform::release_pointer(window);
        }
        self.game.input.release_all();
    }

    /// Make [`Self::grab`] agree with what the browser actually holds.
    ///
    /// # Why this is polled rather than deduced from events
    ///
    /// Escape is the browser's own way out of pointer lock, and Chrome
    /// **consumes** that keystroke: the page never sees a `keydown`, so the
    /// `WindowEvent::KeyboardInput` arm that calls [`Self::release_pointer`]
    /// natively does not run on the web. Without this, `grab` would stay
    /// `Grabbed` while the pointer was free, and [`Self::grab_pointer`]'s
    /// early return would then refuse to re-lock on the next click — the player
    /// presses Escape once and can never get the mouse back.
    ///
    /// `document.pointerLockElement` is the only thing that actually knows, so
    /// it is what is asked, once a frame. It also covers the lock being lost
    /// for reasons nothing in this process can observe: the tab being hidden,
    /// the user switching windows, the browser dropping it on a security
    /// prompt.
    ///
    /// Releasing the held keys on the way out matters for the same reason
    /// `Focused(false)` does natively — a player who leaves mid-strafe should
    /// not come back still strafing.
    #[cfg(target_arch = "wasm32")]
    fn sync_pointer_lock(&mut self) {
        let locked = web_sys::window()
            .and_then(|w| w.document())
            .and_then(|d| d.pointer_lock_element())
            .is_some();
        let was_locked = self.grab == PointerGrab::Grabbed;
        if locked == was_locked {
            return;
        }
        self.grab = if locked {
            PointerGrab::Grabbed
        } else {
            self.game.input.release_all();
            PointerGrab::Released
        };
        log::info!(
            "pointer lock {}",
            if locked { "taken" } else { "released" }
        );
        crate::web::pointer_lock_changed(locked);
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
        //
        // A played session does not take it at all: the mouse controls nothing
        // while the file is driving, and capturing the operator's cursor for a
        // window they are only watching is a cost with no benefit.
        #[cfg(not(target_arch = "wasm32"))]
        if self.options.playback.is_none() {
            self.grab_pointer();
        }

        log::info!(
            "straf3 {} — world {:?}, {} profile, {} Hz ({} ms commands). {}",
            env!("CARGO_PKG_VERSION"),
            self.options.world,
            self.options.profile_name,
            self.options.rate.hz(),
            self.options.rate.command_millis(),
            if self.options.playback.is_some() {
                "Playing a recording: the keyboard and mouse are ignored, Esc \
                 and closing the window still work."
            } else {
                "Click to capture the mouse, Esc to release, R to respawn."
            },
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
                // Natively, losing focus must drop the grab: nothing else
                // would, and a window that kept the mouse after the player
                // alt-tabbed would be holding their cursor hostage.
                //
                // On the web the browser owns that decision and has already
                // made it — pointer lock is released when the document loses
                // focus, by specification. Calling `exitPointerLock()` here as
                // well only adds a second opinion, and it is the one that
                // loses: a page that regains focus without a fresh click would
                // stay released even where the browser would have kept the
                // lock. The held keys are still let go, for the reason they
                // always were — a player who leaves mid-strafe should not come
                // back still strafing — and `sync_pointer_lock` reconciles the
                // flag on the next frame either way.
                #[cfg(not(target_arch = "wasm32"))]
                self.release_pointer();
                #[cfg(target_arch = "wasm32")]
                self.game.input.release_all();
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
                        // A respawn part-way through a played stream would run
                        // the rest of the file from the spawn instead of from
                        // where the recorded player actually was, and the run
                        // on screen would stop being the run in the file.
                        if self.options.playback.is_some() {
                            log::info!(
                                "R ignored: this session is playing a recording, and a \
                                 respawn would desync it from the run it is replaying"
                            );
                            return;
                        }
                        self.game.respawn();
                        // A new attempt: the run clock is back at NotStarted,
                        // the recorder has started again from the spawn, and
                        // the ghost has to be matched from the start line
                        // rather than from wherever the last attempt died.
                        self.run_saved = false;
                        self.split_ms = None;
                        self.split_final = false;
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

        // A played session takes its movement from the file. Dropping the
        // event here rather than letting it update an `InputState` nothing
        // reads is the difference between "live input is ignored" being a
        // structural fact and being a consequence somebody could undo.
        if self.options.playback.is_none() {
            self.game.input.apply_window_event(&event);
        }
    }

    fn device_event(&mut self, _loop: &ActiveEventLoop, _id: DeviceId, event: DeviceEvent) {
        // Mouse motion only turns the view while the pointer is captured —
        // otherwise moving the mouse across a windowed build would spin the
        // camera while the player is clicking on something else. And never
        // during playback, for the reason above.
        if self.grab == PointerGrab::Grabbed && self.options.playback.is_none() {
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

    // Web only, and it is mouse-look that needs it. winit's default is
    // `WhenFocused`, and on the web "focused" means the *canvas element* holds
    // DOM focus — which pointer lock does not grant. A canvas that is locked
    // but not focused therefore delivers no `DeviceEvent::MouseMotion` at all
    // and the view simply does not turn, with nothing in the log to say why.
    //
    // Widening this costs nothing here because the consumer is already gated:
    // `App::device_event` ignores motion unless the pointer is grabbed, and
    // `sync_pointer_lock` above keeps that flag honest. Native is left on the
    // default, because native has no such gap and r12 asks that native play not
    // be touched by web work.
    #[cfg(target_arch = "wasm32")]
    event_loop.listen_device_events(winit::event_loop::DeviceEvents::Always);

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
    use crate::replay::{ReplayOptions, parse, replay};

    /// A run long enough to leave the ground and land again, on the flat world
    /// — which is the only world a test in this process can rely on, since the
    /// map is a process-wide singleton (`scene::install`).
    const FIXTURE: &str = "\
rate 125
profile cpm
world flat 0
spawn 0 0 24
yaw 90

cmd 60 127 0 0 - 0 90
cmd 1 127 0 127 jump 0 90
cmd 40 127 127 0 - 0 95.5
cmd 1 127 127 127 jump 0 101.0
cmd 80 127 127 0 - 0 108.25
";

    /// Frame schedules a real machine can produce, including the ones that
    /// break a loop which assumes one frame is one tick: a frame shorter than a
    /// command, a frame of nothing at all, and a 250 ms hitch that owes the
    /// simulation 31 commands at once.
    const HOSTILE_SCHEDULES: &[&[u64]] = &[
        &[8],
        &[1, 97, 3, 250, 8],
        &[0, 0, 0, 33],
        &[250],
        &[1],
        &[16, 17],
    ];

    fn playback_options(fixture: &crate::replay::Fixture, source: &str) -> Options {
        Options {
            world: fixture.world,
            profile: fixture.profile,
            profile_name: fixture.profile_name.clone(),
            rate: fixture.rate,
            // No personal best: this test is about the simulation, and a test
            // that wrote into `runs/` would be a test with a side effect.
            pb_dir: None,
            playback: Some(Playback {
                cmds: fixture.cmds.clone(),
                spawn: fixture.spawn,
                yaw: fixture.yaw,
                source: source.to_owned(),
            }),
            ..Options::default()
        }
    }

    /// **The acceptance test for `--play`** (coordinator decision D-B1).
    ///
    /// A windowed playback session, driven through `App`'s own per-frame path
    /// over frame schedules no real machine would be kind enough to produce,
    /// must land on exactly the checksum the headless `--replay` of the same
    /// file produces — and must do so at *every* tick, not merely at the last.
    /// A divergence can reconverge: a run has been measured agreeing on its
    /// final state while 29 of 1200 intermediate states differed, and an
    /// end-state comparison would have called that identical.
    ///
    /// If these numbers could differ, everything downstream of this — the
    /// personal best a played run saves, the ghost it becomes, the split — would
    /// be measuring a run nobody recorded.
    #[test]
    fn a_played_session_matches_the_headless_replay_at_every_tick() {
        let fixture = parse(FIXTURE).unwrap();
        let reference = replay(&fixture, &ReplayOptions::default()).unwrap();
        assert_eq!(reference.len(), fixture.cmds.len() + 1);

        for schedule in HOSTILE_SCHEDULES {
            let mut app = App::new(playback_options(&fixture, "test fixture"));
            assert_eq!(
                app.game().state().checksum(),
                reference[0].checksum(),
                "schedule {schedule:?}: the session did not even start where the file did"
            );

            let mut applied = 0usize;
            let mut elapsed_ms = 0u64;
            for frame in 0..100_000 {
                let delta = schedule[frame % schedule.len()];
                elapsed_ms += delta;
                app.simulate_frame(delta, elapsed_ms);
                applied += app.game().state().tick as usize - applied;
                assert_eq!(
                    app.game().state().checksum(),
                    reference[applied].checksum(),
                    "schedule {schedule:?}: diverged after {applied} commands"
                );
                if app.game().playback_remaining() == Some(0) {
                    break;
                }
            }

            assert_eq!(
                applied,
                fixture.cmds.len(),
                "schedule {schedule:?}: the stream did not run to the end"
            );
            assert_eq!(
                app.game().state().checksum(),
                reference.last().unwrap().checksum(),
                "schedule {schedule:?}"
            );
            assert_eq!(
                app.game().state().time_ms,
                reference.last().unwrap().time_ms
            );
        }
    }

    #[test]
    fn a_finished_stream_holds_its_state_however_long_the_window_stays_open() {
        let fixture = parse(FIXTURE).unwrap();
        let mut app = App::new(playback_options(&fixture, "test fixture"));
        let mut elapsed_ms = 0;
        while app.game().playback_remaining() != Some(0) {
            elapsed_ms += 8;
            app.simulate_frame(8, elapsed_ms);
        }
        let held = app.game().state().checksum();

        for _ in 0..500 {
            elapsed_ms += 16;
            app.simulate_frame(16, elapsed_ms);
        }
        assert_eq!(app.game().state().checksum(), held);
        assert_eq!(app.game().state().tick as usize, fixture.cmds.len());
        // And the interpolation has a fixed point rather than swinging between
        // the last two states for as long as the window is open.
        assert_eq!(
            app.game().previous().checksum(),
            app.game().state().checksum()
        );
    }

    #[test]
    fn a_played_session_spawns_where_the_recording_began_not_where_the_world_says() {
        // The failure this catches is silent: the same commands from a
        // different origin re-simulate to a different run, and nothing about it
        // looks wrong on screen.
        let mut fixture = parse(FIXTURE).unwrap();
        fixture.spawn = straf3_sim::num::vec3(
            straf3_sim::num::s(128.0),
            straf3_sim::num::s(-64.0),
            straf3_sim::num::s(24.0),
        );
        let app = App::new(playback_options(&fixture, "test fixture"));
        assert_eq!(app.game().state().player.origin, fixture.spawn);
        assert!(app.game().is_playing());
    }

    #[test]
    fn a_played_session_ignores_respawn_because_it_would_desync_the_stream() {
        let fixture = parse(FIXTURE).unwrap();
        let mut app = App::new(playback_options(&fixture, "test fixture"));
        for _ in 0..20 {
            app.simulate_frame(8, 0);
        }
        let mid_run = app.game().state().checksum();
        let consumed = app.game().state().tick;

        // `App` refuses the key, and `Game` refuses the operation, so a future
        // caller cannot reintroduce the desync by going round the event loop.
        app.game.respawn();
        assert_eq!(app.game().state().checksum(), mid_run);
        assert_eq!(app.game().state().tick, consumed);
    }

    /// A played session must record what it played, or a played run that
    /// finishes saves nothing — `save_personal_best_if_better` builds the
    /// `.s3d` out of the recorder, and `Game::apply` never fed it.
    #[test]
    fn a_played_session_records_the_commands_it_was_driven_by() {
        let fixture = parse(FIXTURE).unwrap();
        let mut options = playback_options(&fixture, "test fixture");
        options.record = true;
        let mut app = App::new(options);
        while app.game().playback_remaining() != Some(0) {
            app.simulate_frame(8, 0);
        }

        let recorded = app.game().recorder().expect("recording was asked for");
        assert_eq!(recorded.commands(), fixture.cmds.as_slice());
        assert_eq!(recorded.start().spawn, fixture.spawn);
        assert_eq!(recorded.start().yaw, fixture.yaw);
        assert_eq!(recorded.start().rate, fixture.rate);
    }

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
