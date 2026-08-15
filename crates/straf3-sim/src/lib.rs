//! The straf3 simulation: a pure function from `(state, command)` to state.
//!
//! # The one rule
//!
//! This crate does not know that rendering, windowing, GPUs, files or clocks
//! exist. It never will. Everything the project wants to be able to do later —
//! replays, ghosts, regression tests, a headless server, an RL environment —
//! is downstream of that single property, and every one of them is expensive
//! to retrofit and cheap to keep.
//!
//! Concretely, and enforced by `cargo xtask check-seam` against the resolved
//! dependency graph rather than by convention:
//!
//! - no dependency on any crate above the line, at any depth;
//! - no `std::fs`, `std::net`, `std::env`, `std::process`, `Instant` or
//!   `SystemTime` anywhere in `src/`;
//! - one third-party dependency, `glam`, for vector maths.
//!
//! If something here needs to read a file, it is in the wrong crate. The
//! headless runner (`bin/straf3-headless`) is the worked example: it reads the
//! input file and calls in.
//!
//! # The four seams
//!
//! Each of these exists so a decision that is not settled yet can be changed
//! later without rewriting movement code:
//!
//! | Seam | Where | Keeps changeable |
//! |---|---|---|
//! | Time | [`UserCmd::duration_ms`], [`TickRate`] | the command rate, per spec D2 |
//! | Collision | [`World`] | whether parry is used at all (spec section 4) |
//! | Constants | [`PhysicsProfile`] | VQ3 vs CPM, and tuning (spec D1) |
//! | Arithmetic | [`num`] | `f32` vs fixed-point later |
//!
//! # Determinism, precisely
//!
//! Same binary, same machine, bit-identical — spec section 4. Cross-platform
//! bit-exactness is explicitly *not* promised in this cut; the [`num`] module
//! exists so that promise could be made later without rewriting the physics.
//!
//! What holds it up: [`step`] is pure; command durations are integers so
//! simulation time cannot drift; all timers are integers; the collision
//! implementor is contractually required to be deterministic too
//! ([`World`]); and `glam`'s `fast-math` feature — which permits float
//! reassociation — is denied workspace-wide by the seam check.
//!
//! # Example
//!
//! ```
//! use straf3_sim::{PhysicsProfile, SimState, TickRate, UserCmd, run};
//! use straf3_sim::num::{s, vec3};
//! use straf3_sim::world::FlatGround;
//!
//! let rate = TickRate::HZ_125;                 // 8 ms commands, per D2
//! let world = FlatGround::at(s(0.0));
//! let profile = PhysicsProfile::cpm();         // D1 default
//! let start = SimState::spawned_at(vec3(s(0.0), s(0.0), s(64.0)), s(90.0));
//!
//! let cmds = vec![UserCmd::still_at(rate); 250];
//! let a = run(&start, &cmds, &world, &profile);
//! let b = run(&start, &cmds, &world, &profile);
//!
//! assert_eq!(a.checksum(), b.checksum());      // bit-identical, always
//! assert_eq!(a.time_ms, 2_000);                // exact integer time
//! ```
//!
//! # The movement
//!
//! [`step`] is a transcription of Quake 3's `PmoveSingle`: friction,
//! acceleration, the jump check, the multi-plane slide solver and step-up, in
//! that order, against the [`World`] seam. VQ3's constants are id's, asserted
//! numerically in [`PhysicsProfile`]'s tests; CPM's air control, air-stop,
//! strafe acceleration and double jump are the *same* code path with different
//! numbers, which is why [`PhysicsProfile`] is data and there is no profile
//! branch anywhere in the physics.
//!
//! # What is not here yet
//!
//! - Sub-stepping a long command (`pmove_msec`), which must land before any
//!   reference replay is recorded, because it changes results.
//! - Everything scripted: jump pads, teleporters, knockback. The state and the
//!   timer they need ([`Timers::movement_locked_ms`]) are here; the triggers
//!   are not, because there is no map format yet.
//! - Water, ladders and `SURF_LADDER`.
//! - The CPM constants are community reconstructions, not id source. They are
//!   pinned by a test so they cannot drift, but they are not *verified*, and
//!   the operator tunes them on the Windows build.

#![forbid(unsafe_code)]
#![warn(missing_docs)]
#![warn(clippy::all)]

pub mod cmd;
pub mod num;
pub mod profile;
pub mod state;
pub mod step;
pub mod world;

pub use cmd::{Buttons, TickRate, UserCmd, ViewAngles};
pub use profile::PhysicsProfile;
pub use state::{GroundState, PlayerState, RunState, SimState, Timers};
pub use step::{run, step, step_in_place};
pub use world::{SurfaceFlags, Sweep, Trace, World};
