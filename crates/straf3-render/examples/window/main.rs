//! `cargo run -p straf3-render --example window`
//!
//! Opens a window and flies the autopilot down the course. This is the
//! renderer's own slice, standing on its own: no input path, no `straf3-game`,
//! nothing from another crate's worktree. If this draws, the device, the
//! surface, the pipeline, the camera and the compiled map are all working.
//!
//! It is not the game. `straf3-game` is the game; it owns the window and the
//! input, and it calls the same [`Renderer`] this does.

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
    unreachable!("the `window` example opens a native window; the browser driver is `web-demo`");
}
