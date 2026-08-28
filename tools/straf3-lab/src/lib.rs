//! The movement lab: an instrument that measures the movement language and
//! publishes the numbers.
//!
//! # Why this exists
//!
//! Every movement decision in this project so far has rested on Quake 3
//! heritage and on taste. The vision's Goal 1 asks for evidence instead, and
//! there was no instrument. This is it: `cargo xtask lab` runs it and it writes
//! `docs/movement-lab.md`.
//!
//! It answers, for every profile in [`measure::profiles`]:
//!
//! 1. the strafe speed-gain curve against entry speed and held angle;
//! 2. the technique timing windows, in whole milliseconds;
//! 3. ramp-angle response, across the walkable/sliding flip;
//! 4. whether overbounce occurs, and under what conditions;
//! 5. edge-clip and step-up behaviour;
//! 6. the terminal speed each technique actually reaches.
//!
//! # The two properties that make it an instrument rather than a script
//!
//! **Deterministic.** Same invocation, same bytes out, on any machine. There is
//! no clock, no `Instant`, no randomness, no parallelism, and no iteration over
//! a container whose order is not part of the type. The one arctangent the
//! measurement loop needs is written out in [`num`] from IEEE primitives rather
//! than taken from libm, for the same reason `straf3_sim::num::sin_cos` is —
//! a 1-ulp disagreement between two libms does not stay 1 ulp when its output
//! is fed back in as the next command's view angle.
//!
//! The one thing that is *not* fixed is the provenance header: the tree state
//! is passed in from outside, because a tool that read `git rev-parse` itself
//! would produce a different document on every commit and could not be checked
//! against a fixture. See [`report::Provenance`].
//!
//! **Headless.** No GPU, no window, no map file. Every world it measures in is
//! built from brushes in [`geometry`] and traced by `straf3-collision`, the
//! same tracer the game uses.
//!
//! # Where it sits
//!
//! Above the seam, by construction — it writes a file, and nothing below the
//! line may. It is under `tools/` rather than `crates/` because it is something
//! the project runs *at itself*, not something the game links. Nothing below
//! the line may depend on it, and `cargo xtask check-seam` does not need to
//! know it exists.

#![forbid(unsafe_code)]
#![warn(missing_docs)]
#![warn(clippy::all)]

pub mod candidate;
pub mod dataset;
pub mod gates;
pub mod geometry;
pub mod harness;
pub mod measure;
pub mod num;
pub mod refine;
pub mod report;

pub use dataset::{Change, Dataset, Measurement, Section, Table, diff, summarise};
pub use report::Provenance;

/// Take every measurement and collect the sections of the report.
///
/// Deterministic and pure: no arguments, because there is nothing to vary. Two
/// calls in one process, or on two machines, produce the same sections.
#[must_use]
pub fn run() -> Vec<Section> {
    measure::all()
}

/// Take every measurement and collect them into one dataset.
#[must_use]
pub fn measurements() -> Dataset {
    Dataset::from_sections(&run())
}
