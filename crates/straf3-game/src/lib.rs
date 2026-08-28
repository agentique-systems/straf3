//! straf3's glue: the event loop, the fixed-step accumulator, and the path
//! from a held key to a [`straf3_sim::UserCmd`].
//!
//! # Why there is a library here at all
//!
//! `straf3-headless` is a thin `bin/` wrapper over a pure `straf3-sim`, and
//! that split is what makes the simulation testable. This crate does the same
//! thing one layer up: everything that decides *what the game does* lives in
//! the library, and `src/main.rs` is a winit entry point with no logic in it.
//! The consequence is that the three properties Wave 3 has to demonstrate are
//! all reachable from a test with no window and no GPU in the process:
//!
//! | Criterion | Where it lives | How it is checked |
//! |---|---|---|
//! | 3 — input becomes the same integer-ms `UserCmd` | [`input_map::command_from_input`] | synthetic [`straf3_platform::InputState`] in, `UserCmd` out |
//! | 4 — the renderer changes nothing below the seam | [`tick::advance_one`] | diffed against [`straf3_sim::step_in_place`] called directly |
//! | 5 — frame pacing is decoupled from simulation stepping | [`tick::plan_ticks`] | synthetic frame deltas, integer arithmetic |
//!
//! # Watching a record: `--play`
//!
//! [`app::Options::playback`] drives a session from a recorded command stream
//! with the window open and the frame drawn, instead of from the keyboard. It
//! is the foundation of "watch a record", and it is what lets a complete run be
//! driven deterministically on a real GPU rather than by a person who has to
//! play well on demand.
//!
//! It is deliberately **not** a separate mode. Playback is a swap of where a
//! tick's command comes from, inside [`Game::advance`], so there is exactly one
//! stepping path in this crate and the windowed build cannot drift from the
//! headless one. [`game`]'s module docs give the full argument, including the
//! bug this shape avoided and the five-way checksum equality it produced on the
//! real GPU — read them before consolidating anything here.
//!
//! # The one-way flow
//!
//! ```text
//!   winit event ─► InputState ─► UserCmd ─► step_in_place ─► SimState
//!                     (raw)      (8 ms)      (the seam)         │
//!                                                               ▼
//!                                           Renderer::render(prev, curr, α)
//! ```
//!
//! Nothing flows the other way. The renderer is handed two finished states and
//! a number between them; it has no API through which it could ask the
//! simulation to advance, which is what makes "the renderer feeds the seam, it
//! does not bypass it" a structural property rather than a promise.
//!
//! # Playing in a browser: [`web`]
//!
//! The same [`App`], the same [`Game`], the same seam. [`web`] is the entry
//! point a page calls, and everything specific to running from a URL — the
//! map and physics a `/play/<map>` link names, refusing a physics digest this
//! build does not implement, `?ghost=` degrading rather than refusing,
//! `/watch/` taking its identities from the recording's own header — is in
//! that one module rather than sprinkled through the loop.
//!
//! # `unsafe`
//!
//! Not forbidden here, unlike every other straf3 crate, for one reason:
//! `#[wasm_bindgen]` expands to generated glue containing `unsafe`, and
//! [`web`]'s entry point needs it. No hand-written `unsafe` appears in this
//! crate.

#![warn(missing_docs)]
#![warn(clippy::all)]

pub mod app;
pub mod game;
pub mod ghost;
pub mod input_map;
pub mod pb;
pub mod profile;
pub mod record;
pub mod replay;
pub mod scene;
pub mod tick;
#[cfg(target_arch = "wasm32")]
pub mod web;

pub use app::{App, Options, Playback, run};
pub use game::Game;
pub use ghost::{Ghost, GhostError};
pub use input_map::command_from_input;
pub use record::{Recorder, WorldSpec};
pub use replay::{Fixture, ReplayOptions, TRACE_HEADER, trace_line};
pub use scene::WorldChoice;
pub use tick::{DEFAULT_RATE, FixedStep, TickPlan, advance_one, plan_ticks};

#[cfg(target_arch = "wasm32")]
pub use web::start_web;
