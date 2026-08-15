//! Workspace automation for straf3.
//!
//! The only thing in here that matters is [`seam`], which enforces the
//! one-directional dependency rule from spec section 4 against the real
//! resolved dependency graph.

pub mod seam;
