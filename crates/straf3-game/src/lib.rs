//! straf3's glue: the event loop, the fixed-step accumulator, and the path
//! from a held key to a [`straf3_sim::UserCmd`].
//!
//! # Why there is a library here at all
//!
//! `straf3-headless` is a thin `bin/` wrapper over a pure `straf3-sim`, and
//! that split is what makes the simulation testable. This crate does the same
//! thing one layer up: everything that decides *what the game does* lives in
//! the library, and `src/main.rs` is a winit entry point with no logic in it.
//! The consequence is that the three properties Wave 3 has to demonstrate are
//! all reachable from a test with no window and no GPU in the process:
//!
//! | Criterion | Where it lives | How it is checked |
//! |---|---|---|
//! | 3 — input becomes the same integer-ms `UserCmd` | [`input_map::command_from_input`] | synthetic [`straf3_platform::InputState`] in, `UserCmd` out |
//! | 4 — the renderer changes nothing below the seam | [`tick::advance_one`] | diffed against [`straf3_sim::step_in_place`] called directly |
//! | 5 — frame pacing is decoupled from simulation stepping | [`tick::plan_ticks`] | synthetic frame deltas, integer arithmetic |
//!
//! # The one-way flow
//!
//! ```text
//!   winit event ─► InputState ─► UserCmd ─► step_in_place ─► SimState
//!                     (raw)      (8 ms)      (the seam)         │
//!                                                               ▼
//!                                           Renderer::render(prev, curr, α)
//! ```
//!
//! Nothing flows the other way. The renderer is handed two finished states and
//! a number between them; it has no API through which it could ask the
//! simulation to advance, which is what makes "the renderer feeds the seam, it
//! does not bypass it" a structural property rather than a promise.
//!
//! # `unsafe`
//!
//! Not forbidden here, unlike every other straf3 crate, for one reason:
//! `#[wasm_bindgen]` expands to generated glue containing `unsafe`, and the
//! web entry point below needs it. No hand-written `unsafe` appears in this
//! crate.

#![warn(missing_docs)]
#![warn(clippy::all)]

pub mod app;
pub mod game;
pub mod input_map;
pub mod record;
pub mod replay;
pub mod scene;
pub mod tick;

pub use app::{App, Options, run};
pub use game::Game;
pub use input_map::command_from_input;
pub use record::{Recorder, WorldSpec};
pub use replay::{Fixture, ReplayOptions, TRACE_HEADER, trace_line};
pub use scene::WorldChoice;
pub use tick::{DEFAULT_RATE, FixedStep, TickPlan, advance_one, plan_ticks};

/// The web entry point: JS calls this after choosing a backend.
///
/// # Why the backend is JS's decision and not ours
///
/// Spec rev 6 §Q2, measured rather than assumed: wgpu does **not** fall back
/// from WebGPU to WebGL2 on its own. With both backends compiled in and
/// `requestAdapter()` returning null, it crashes inside the WebGPU backend
/// instead of degrading. straf3 therefore ships WebGPU-only — every visitor
/// would otherwise pay 595 KiB for a fallback the Chrome/Firefox/Edge majority
/// never touches — and the host page decides, *before* entering wasm, whether
/// this browser can run it at all.
///
/// So this function's contract is: the page has already established that
/// `navigator.gpu.requestAdapter()` yields an adapter, and says so by passing
/// `"webgpu"`. Anything else is refused here, loudly, rather than being turned
/// into a panic several stack frames inside wgpu.
#[cfg(target_arch = "wasm32")]
#[wasm_bindgen::prelude::wasm_bindgen]
pub fn start_web(backend: &str) {
    console_error_panic_hook::set_once();
    let _ = console_log::init_with_level(log::Level::Info);

    if backend != "webgpu" {
        log::error!(
            "straf3 is WebGPU-only (spec rev 6 §Q2) and the host page reported \
             backend `{backend}`. Refusing to start: wgpu does not fall back to \
             WebGL2, it crashes. Use a browser with WebGPU enabled."
        );
        report_to_page(&format!(
            "straf3 needs WebGPU, and this browser reported `{backend}`."
        ));
        return;
    }

    log::info!("straf3 starting on WebGPU");
    run(Options::default());
}

/// Put a message where a person looking at the page can see it.
///
/// The console is where the log goes, but a visitor whose browser cannot run
/// straf3 will be looking at a blank canvas, not at devtools.
#[cfg(target_arch = "wasm32")]
fn report_to_page(message: &str) {
    if let Some(element) = web_sys::window()
        .and_then(|w| w.document())
        .and_then(|d| d.get_element_by_id("straf3-status"))
    {
        element.set_text_content(Some(message));
    }
}
