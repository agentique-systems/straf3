//! Workspace automation for straf3.
//!
//! Two checks live here, and both enforce a property the project cannot
//! verify by reading code:
//!
//! - [`seam`] enforces the one-directional dependency rule from spec
//!   section 4 against the real resolved dependency graph.
//! - [`determinism`] runs one reference command stream on every target the
//!   project ships or verifies on and fails if their rolling digests differ
//!   (architecture item C2).
//! - [`probes`] compiles every crate under `probes/`, which no `--workspace`
//!   command reaches, so a probe cannot rot unnoticed the way two did when
//!   C3 changed the command boundary.
//! - [`evidence`] re-derives every number a document publishes as a live claim
//!   and fails when one stops reproducing, so a checksum pasted into prose
//!   cannot expire silently the way `coil.txt`'s did for nine days.
//!
//! Three more are instruments rather than checks — they exist so that a claim
//! about how the game moves, or about the real GPU, is something a reader can
//! reproduce with one command instead of a paragraph of shell:
//!
//! - [`lab`] measures the movement language and publishes the numbers, so a
//!   decision about how the game moves can be argued from evidence instead of
//!   from taste.
//! - [`capture`] takes a screenshot of the running client (criterion 8).
//! - [`pacing`] runs and analyses frame-time measurements (criterion 7).

pub mod capture;
pub mod determinism;
pub mod evidence;
pub mod lab;
pub mod pacing;
pub mod probes;
pub mod seam;
