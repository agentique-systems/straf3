//! `.s3d` — a saved run, bound to the geometry and the physics it was made
//! against.
//!
//! Below the seam. Depends on `straf3-sim` for the command type and the step
//! function; `straf3-sim` must never depend on this crate. Depends on nothing
//! else at all: not on `serde`, and not on the filesystem — see `Cargo.toml`
//! for why each of those absences is deliberate.
//!
//! # What a recording is
//!
//! The inputs, not the resulting positions. Storing inputs is what makes a
//! replay a regression test: replaying it *re-runs the simulation*, and the
//! run either reproduces or it does not. Storing positions would make a replay
//! a video.
//!
//! It follows that a command stream is not self-describing. A position says
//! where the player was; a key press says only what they asked for, and what
//! that produced depends entirely on the world it was asked in. So the file
//! carries four things, and the last two are the ones that make it worth
//! anything:
//!
//! 1. the **command stream** — every [`UserCmd`](straf3_sim::UserCmd), with
//!    its 16-bit view angles stored as the shorts they are;
//! 2. the **rolling digest** — folded over every command's
//!    [`SimState::checksum`](straf3_sim::SimState::checksum), the same scheme
//!    and the same seed criterion 2's cross-target check folds over its
//!    reference stream;
//! 3. a **map identity** — [`WorldId`], which for a compiled map is
//!    `CompiledMap::collision_digest`: a fold over the hulls the physics
//!    actually ran against, so recompiling the map, or changing the compiler,
//!    changes it;
//! 4. a **physics identity** — [`PhysicsId`], derived from the bits of every
//!    field of the [`PhysicsProfile`](straf3_sim::PhysicsProfile) in effect.
//!
//! # Contract item C6, in one sentence
//!
//! A recording bound to stale collision geometry is **detected**, not silently
//! replayed against different geometry — and the way it is detected is that
//! there is no function on [`Recording`] that yields a runnable command stream
//! without being told what world and what physics it is about to run under.
//! [`Recording::commands_for`] takes them and returns [`Mismatch`] when they
//! are not the ones the run was made with. That is what makes a ghost worth
//! racing: the alternative is a ghost that lands where a ramp used to be,
//! beside a player who falls short, showing a time nobody ever set.
//!
//! ```
//! # use straf3_replay::{Recording, RunStart, WorldId};
//! # use straf3_sim::num::{s, vec3};
//! # use straf3_sim::world::FlatGround;
//! # use straf3_sim::{PhysicsProfile, TickRate, UserCmd};
//! let world = FlatGround::at(s(0.0));
//! let profile = PhysicsProfile::cpm();
//! let start = RunStart { rate: TickRate::HZ_125, spawn: vec3(s(0.0), s(0.0), s(64.0)), yaw: s(0.0) };
//! let commands = vec![UserCmd::still_at(TickRate::HZ_125); 8];
//!
//! // Recorded on one compilation of a map...
//! let rec = Recording::record(
//!     start, commands, &world,
//!     WorldId::map("coil", 0xaaaa_aaaa_aaaa_aaaa),
//!     &profile, "cpm",
//! );
//!
//! // ...and refused against another.
//! let recompiled = WorldId::map("coil", 0xbbbb_bbbb_bbbb_bbbb);
//! let err = rec.commands_for(&recompiled, &profile).unwrap_err();
//! assert!(err.is_stale_geometry());
//! ```
//!
//! # `.s3d` and `straf3_game::record`'s text fixture both stay
//!
//! They are not two formats for one job; they are two jobs. Stated here
//! because the question is a fair one and the answer needs to be findable.
//!
//! **The text fixture is a cross-binary interop format.** Its entire purpose
//! is that a *different binary with an independently written parser* —
//! `straf3-sim`'s `bin/headless.rs` — can read what the windowed build wrote,
//! so the two processes' checksums can be compared. Two implementations that
//! agree is evidence; one implementation cannot disagree with itself, so it
//! cannot demonstrate anything. It is also human-diffable, and its
//! degree-as-text round trip is exact for all 65 536 representable shorts,
//! measured exhaustively. That is a good format for what it does.
//!
//! **`.s3d` is the production artefact**: a personal best, a ghost, a run
//! somebody might dispute. It carries what the text format structurally cannot
//! — the map identity, the physics identity, the rolling digest and the run
//! time — and it refuses to be replayed against geometry it was not recorded
//! on.
//!
//! Extending the text format to carry those was considered and is the wrong
//! move twice over. Mechanically, `headless.rs` errors on any directive it
//! does not know (`unknown directive`, one match arm), so every new line would
//! break the reader whose existence is the format's whole reason to be.
//! Substantively, the two want opposite things: the fixture wants to be
//! readable and stable enough that a second parser can be written against it,
//! and `.s3d` wants to be byte-exact and to change its version number whenever
//! anything about it changes. Making one format serve both would mean the
//! interop reader has to track the production format's every revision.
//!
//! So: nothing in `straf3-game`'s recorder is superseded and nothing there
//! needs to change. A session can be written both ways, and when the run
//! matters it should be — the fixture proves two binaries agree, the `.s3d`
//! proves four targets do.
//!
//! # Cross-target reproduction
//!
//! Criterion 7 asks for more than a round trip: the same saved run,
//! re-simulated on `x86_64-unknown-linux-gnu`, `x86_64-unknown-linux-musl`,
//! `x86_64-pc-windows-gnu` and `wasm32-unknown-unknown`, must produce the same
//! final time and the same digest. [`crosstarget`] is the harness that
//! measures it and `crosstarget/verify.sh` is what runs it on all four; its
//! published results live in `crates/straf3-replay/crosstarget/results/`.

#![forbid(unsafe_op_in_unsafe_fn)]

mod codec;
pub mod crosstarget;
pub mod digest;
mod error;
pub mod identity;
mod recording;

pub use codec::{COMMAND_BYTES, FLAG_TRACE, FORMAT_VERSION, MAGIC, MAX_NAME_BYTES};
pub use error::{LoadError, Mismatch, VerifyError};
pub use identity::{PhysicsId, WorldId, physics_digest};
pub use recording::{Outcome, Recording, RunStart};

#[cfg(test)]
mod tests;
