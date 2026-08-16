//! `cargo run -p straf3-devtools --example hud-offscreen`
//!
//! Paints the overlay into an offscreen texture and reads the pixels back. No
//! window, no surface, no display server.
//!
//! # Why this exists
//!
//! `hud.rs`'s unit tests prove the overlay *composes* the right strings, with
//! no GPU in the process. They cannot prove those strings reach a texture:
//! everything between tessellation and a lit pixel — the atlas upload, the
//! vertex buffers, the load-op on the render pass, the sRGB entry point — is
//! only exercised by a real device. This runs that half and counts the pixels
//! it changed, so "the HUD draws" is an observation.
//!
//! It writes `target/hud-offscreen/*.ppm`, which is also the only way to *look*
//! at the overlay on a headless box.
//!
//! An example rather than a test on purpose: it needs a GPU adapter, and a
//! test that fails on a machine without one is a test that punishes the
//! correct environment.

// The whole thing is host-side pixel readback, which has no meaning in a wasm
// module. `--target wasm32-unknown-unknown --all-targets` compiles examples.
#[cfg(not(target_arch = "wasm32"))]
#[path = "run.rs"]
mod run;

fn main() {
    #[cfg(not(target_arch = "wasm32"))]
    run::main();

    #[cfg(target_arch = "wasm32")]
    unreachable!("hud-offscreen reads pixels back on the host");
}
