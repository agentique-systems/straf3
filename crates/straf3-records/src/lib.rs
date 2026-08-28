//! The straf3 records service.
//!
//! # Above the seam, depending downward
//!
//! This crate links `straf3-sim`, `straf3-replay` and `straf3-map` directly.
//! That is architecture decision D2 and it is the whole reason server-side
//! verification is cheap here: the crates below the seam are pure and headless,
//! so the code that decides whether a run happened is *the same code* that made
//! it happen in the browser, not a reimplementation of it. `cargo xtask
//! check-seam` is what enforces that the arrow only points one way.
//!
//! # What "verified" means here, exactly
//!
//! A submitted run is ranked only after this service re-simulates every command
//! in the recording and agrees. Two things about that are easy to get wrong and
//! are load-bearing:
//!
//! - **The time is computed, not accepted.** The recording's own claimed time
//!   is stored in `runs.client_time_ms` for diagnostics. It is never ranked,
//!   never compared against, and never rendered as authoritative. The ranked
//!   number comes out of `SimState.run` on this machine (ARCHITECTURE §7.2
//!   step 5).
//! - **The comparison is against the rolling digest, not the end state.**
//!   ARCHITECTURE §1.3: the determinism probe found a run whose *final*
//!   checksum matched across builds while 29 of its 1,200 intermediate states
//!   did not. Anything sampled can miss a divergence that reconverges, so what
//!   is compared is the FNV-1a fold over every single command's
//!   `SimState::checksum()` — which is exactly what `straf3_replay::Recording`
//!   already computes and what the `.s3d` header carries.
//!
//! # Two binaries, one crate
//!
//! `straf3-records-api` serves `/v1` and does only what is cheap and
//! synchronous. `straf3-records-verifier` claims pending rows with `for update
//! skip locked` and spends the CPU. They are separate processes for the reason
//! ARCHITECTURE §7.1 gives, and the same crate because a verifier that linked a
//! different `straf3-sim` than the intake decoded against would be verifying a
//! different game.

pub mod auth;
pub mod catalog;
pub mod config;
pub mod db;
pub mod digest16;
pub mod error;
pub mod intake;
pub mod limits;
pub mod profiles;
pub mod routes;
pub mod seed;
pub mod simbuild;
pub mod state;
pub mod verify;

pub use error::{ApiError, ApiResult};
pub use state::AppState;
