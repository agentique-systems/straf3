//! `straf3-agent` — run a straf3 course without a person.
//!
//! # Why this crate exists
//!
//! The milestone this crate serves is self-verifying by operator decision: no
//! claim in it may rest on a human having played something. That makes an
//! automated runner the instrument every other claim is measured with, and it
//! raises the bar on what the runner has to be. `probes/coil-course` already
//! completes `coil` — it is a greedy one-step hill climber with a
//! hand-specified funnel at the finish line, and it says so in its own comments.
//! Two properties of it do not survive contact with a second map:
//!
//! 1. **Its goals are hand-written.** The finish aim point, the switch
//!    threshold `y > mins.y - 384`, and the assumption that the course runs in
//!    `+y` are all facts about `coil.map` typed into the bot's source.
//! 2. **It is a probe.** It has its own lockfile and is not a workspace member,
//!    on purpose: it is published evidence of a past run, not a build input.
//!
//! This crate answers the first with [`course`], which derives the goals from
//! the compiled map's own trigger volumes, and the second by being a workspace
//! member — see this crate's `Cargo.toml` for why that is load-bearing rather
//! than tidy.
//!
//! # What is derived and what is assumed
//!
//! Everything the course plan contains comes from the map: the volumes and
//! their order from `straf3_map::CompiledMap::triggers`, the aim points from
//! each volume's own geometry and the ground under it, the player's dimensions
//! from the [`PhysicsProfile`](straf3_sim::PhysicsProfile). No coordinate,
//! threshold or bearing from any particular map appears in this source, and
//! [`course::Note`] names every place where the derivation had to fall back to
//! something weaker than a general rule, per map, in the printout.
//!
//! Two assumptions are made and are stated rather than hidden:
//!
//! - **Checkpoint order is source order.** `straf3-map` numbers checkpoints by
//!   order of appearance because Defrag gives them no explicit index. A map
//!   author who declares them out of order gets a course plan in the wrong
//!   order, and [`course::Note::CheckpointOrderIsSourceOrder`] says so on every
//!   map that has more than one.
//! - **Checkpoints are route guidance, not gates.** `RunState::finish` does not
//!   consult them: crossing start and then finish produces a time whether or
//!   not any checkpoint was touched. They are still used as intermediate goals
//!   because they are the author's own statement of the intended route, and
//!   [`course::Note::CheckpointsDoNotGateTheClock`] records the distinction on
//!   any map that has them.
//!
//! # What this crate is not
//!
//! It does not choose straf3's movement constants, does not touch
//! `crates/straf3-sim`, and produces no number that is a course record. Times it
//! reports are upper bounds on what the course can be run in — see
//! `docs/movement-agent.md`, which states that plainly and is the document to
//! read before quoting one of them.

#![forbid(unsafe_code)]
#![warn(missing_docs)]
#![warn(clippy::all)]

pub mod course;
pub mod profile;
pub mod report;
