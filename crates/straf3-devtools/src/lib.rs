//! The on-screen overlay and its movement telemetry.
//!
//! Above the seam. This crate reads simulation state in order to display it,
//! and there is no API here through which anything could be fed back — no
//! `&mut SimState`, no command, no world. A run played with the overlay on and
//! a run played with it off produce the same checksum, and that is a property
//! of the shape of this crate rather than of anyone's discipline.
//!
//! # What is on screen
//!
//! Four readouts, which are what a movement run is judged by:
//!
//! | readout | where | why |
//! |---|---|---|
//! | horizontal speed | centre, large | the number a strafe-jumper is steering by |
//! | ground / slide / air | under the speed | the three states are three different sets of physics |
//! | run time | top centre | `m:ss.mmm`, from the simulation's own millisecond count |
//! | split vs. ghost | under the clock | signed, motorsport convention: negative is good |
//!
//! plus a dim corner line carrying the frame rate, vertical speed, tick count
//! and simulation time — the readout wave 3 printed to the terminal once a
//! second, which is how a stalled simulation is told apart from a slow one.
//!
//! # Using it
//!
//! ```no_run
//! # fn f(device: &wgpu::Device, queue: &wgpu::Queue, encoder: &mut wgpu::CommandEncoder,
//! #      target: &wgpu::TextureView, state: &straf3_sim::SimState, fps: u32) {
//! use straf3_devtools::{Hud, HudFrame, TelemetrySample};
//!
//! // once, against the surface format
//! let mut hud = Hud::new(device, wgpu::TextureFormat::Bgra8UnormSrgb);
//!
//! // per frame, after the world is recorded and before the encoder is submitted
//! let sample = TelemetrySample::of(state).with_fps(fps).with_split_ms(None);
//! hud.draw(
//!     HudFrame {
//!         device,
//!         queue,
//!         encoder,
//!         target,
//!         width: 1920,
//!         height: 1080,
//!         pixels_per_point: 1.0,
//!     },
//!     &sample,
//! );
//! # }
//! ```
//!
//! # Native is the target
//!
//! The overlay is judged natively; the browser client is deferred (spec rev 2,
//! criterion 9). Nothing here is native-only any more — dropping `egui-winit`
//! took `arboard` with it, which was the one thing that could not build for
//! `wasm32` — but no claim is made about how it looks in a browser, because
//! nobody has looked.

#![forbid(unsafe_code)]
#![warn(missing_docs)]
#![warn(clippy::all)]

pub mod hud;
// Native only: this reads `std::time::Instant`, which panics outright on
// `wasm32-unknown-unknown`, and writes a file, which a browser cannot. Frame
// pacing is measured on the host with the real GPU or it is not measured —
// see `docs/environment.md` §6.
#[cfg(not(target_arch = "wasm32"))]
pub mod pacing;
pub mod telemetry;

pub use hud::{Hud, HudFrame, HudStyle, compose};
pub use telemetry::{
    CLOCK_PLACEHOLDER, Phase, RunReadout, SpeedTrend, TelemetrySample, TrendFilter,
    format_clock_ms, format_run, format_speed, format_split_ms,
};
