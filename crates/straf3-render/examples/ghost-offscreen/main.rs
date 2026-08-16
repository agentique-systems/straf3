//! `cargo run -p straf3-render --example ghost-offscreen`
//!
//! Draws the course twice — once with a ghost standing in it and once without
//! — reads both back, and reports how many pixels the ghost changed. No
//! window, no surface, no display server.
//!
//! # Why this exists
//!
//! "The ghost is rendered alongside the live player" is otherwise a claim
//! resting on somebody having looked at a window, and this machine is a
//! software-rendered WSL2 box. Differencing two frames turns it into an
//! observation with a number attached, and it checks the two things that are
//! actually easy to get wrong and impossible to notice in a still: that the
//! ghost is *there*, and that a wall in front of it hides it.
//!
//! An example rather than a test, for the same reason `offscreen` is one: it
//! needs a GPU adapter, and a test that fails on a machine without one
//! punishes the correct environment.

// Native only, exactly as `offscreen` is: reading pixels back to a file has no
// meaning inside a wasm module.
#[cfg(not(target_arch = "wasm32"))]
#[path = "run.rs"]
mod run;

fn main() {
    #[cfg(not(target_arch = "wasm32"))]
    run::main();

    #[cfg(target_arch = "wasm32")]
    unreachable!(
        "the `ghost-offscreen` example reads pixels back on the host; there is nothing to read back in a wasm module"
    );
}
