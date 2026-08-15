//! Windowing, raw input, and timing.
//!
//! Above the seam: this is where `winit` is allowed to exist. Nothing below
//! the line may reach this crate.
//!
//! # What this crate is responsible for
//!
//! Exactly three things, and deliberately nothing else:
//!
//! - [`clock`] — the wall clock, read as **whole milliseconds since start**.
//!   The simulation never asks what time it is; this is the only place the
//!   question is asked, and the answer leaves here as an integer.
//! - [`input`] — raw keyboard and mouse events, folded into a small
//!   [`InputState`] that holds *what is currently held* and *where the player
//!   is currently looking*. It does not build [`straf3_sim::UserCmd`]s: that
//!   is `straf3-game`'s job, because the command rate is game policy.
//! - [`window`] — creating the window, on both targets, and the pointer grab
//!   that mouse-look needs.
//!
//! # Why the split with `straf3-game` is where it is
//!
//! Everything here is *raw device state*. Turning device state into a command
//! is a decision about the tick rate and the key bindings, and it has to be
//! testable with no window open at all — so it lives one crate up, where
//! `harness` can call it as a pure function. The rule this crate follows: a
//! `winit` type may appear in an argument, never in a field of anything a
//! caller has to construct. [`InputState`] can be built and driven entirely
//! from synthetic input.
//!
//! # Determinism
//!
//! Nothing here is inside the determinism boundary — the simulation's inputs
//! are [`straf3_sim::UserCmd`]s, and this crate only decides *which* commands
//! get produced, not what they do. But two properties still matter, because
//! criterion 4 (replay equivalence) leans on them:
//!
//! - view angles leave here **absolute**, in degrees, exactly as
//!   [`straf3_sim::ViewAngles`] documents. Mouse deltas are accumulated here
//!   and never travel further.
//! - the clock is read as an integer, and *differences of truncated integers*
//!   are used for pacing, so sub-millisecond truncation cannot accumulate.

#![forbid(unsafe_code)]
#![warn(missing_docs)]
#![warn(clippy::all)]

pub mod clock;
pub mod input;
pub mod window;

pub use clock::{Clock, FrameDelta, FrameTiming};
pub use input::{Action, InputState, MouseLook};
pub use window::{PointerGrab, WindowConfig, grab_pointer, release_pointer};
