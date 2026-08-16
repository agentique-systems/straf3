//! `cargo run -p straf3-render --example offscreen`
//!
//! Renders the course into an offscreen texture and writes the pixels out as
//! PPM files. No window, no surface, no display server.
//!
//! # Why this exists
//!
//! "The renderer works" is otherwise a claim resting on a human having looked
//! at a window. This machine is a software-rendered WSL2 box and CI has no
//! display at all, so that claim would be unverifiable exactly where it most
//! needs checking. Reading the pixels back turns it into an observation: the
//! image is either mostly sky, or it has a course in it, and the difference is
//! measurable.
//!
//! It is an example rather than a test on purpose. It needs a GPU adapter, and
//! a test that fails on a machine without one would be a test that punishes
//! the correct environment.

// Native only. `cargo clippy --all-targets --target wasm32-unknown-unknown`
// compiles every example, and none of what this one does — a desktop window,
// `pollster::block_on`, reading pixels back to a file — has a meaning there.
#[cfg(not(target_arch = "wasm32"))]
#[path = "run.rs"]
mod run;

fn main() {
    #[cfg(not(target_arch = "wasm32"))]
    run::main();

    #[cfg(target_arch = "wasm32")]
    unreachable!(
        "the `offscreen` example reads pixels back on the host; there is nothing to read back in a wasm module"
    );
}
