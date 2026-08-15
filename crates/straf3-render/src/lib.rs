//! wgpu + WGSL renderer.
//!
//! Above the seam: this is where the GPU is allowed to exist. Nothing below
//! the line may reach this crate — which is what lets the simulation run in
//! CI with no GPU at all (spec criterion 14).
//!
//! # What this crate is responsible for
//!
//! - [`arena`] — the hardcoded arena, as data. **The collision geometry lives
//!   here too**, because the thing you see and the thing you hit have to be
//!   the same thing. `straf3-game` imports [`arena::ARENA`] and hands it
//!   straight to [`straf3_sim::step_in_place`]; it does not write a second
//!   collision implementation. That module has no `wgpu` in it, so importing
//!   it costs nothing.
//! - [`camera`] — the eye, as a pure function of two [`PlayerState`]s and an
//!   interpolation factor.
//! - [`mesh`] — the arena, as triangles.
//! - [`Renderer`] — device, surface, draw loop. One implementation for native
//!   and for `wasm32-unknown-unknown`.
//!
//! # What it is not responsible for
//!
//! Windows, input, and time. `straf3-platform` owns those and [`Renderer`]
//! takes the window it made. The renderer never advances the simulation, never
//! reads a clock, and has no opinion about the frame rate: it draws whatever
//! two states it is handed, `alpha` of the way between them. That is criterion
//! 5's decoupling stated as an API — there is no way to express "step the
//! simulation" from in here.
//!
//! # One implementation, two targets
//!
//! The only structural difference between native and web is *when the device
//! arrives*. Native blocks on it; the browser cannot — `pollster::block_on`
//! deadlocks the main thread there — so acquisition is spawned and lands in a
//! later frame. Rather than fork the API, [`Renderer`] simply starts out not
//! ready: [`Renderer::render`] does nothing until the device exists, and
//! [`Renderer::is_ready`] says which it is. Callers write one loop.
//!
//! Backend selection is the caller's, not wgpu's. On the web the host page
//! chooses WebGPU in JS *before* entering wasm, because wgpu does not fall
//! back from WebGPU to WebGL2 on its own — with both backends compiled in and
//! no adapter, it panics inside the WebGPU backend (spec rev 6 Q2, measured).
//! The `webgl` wgpu feature is deliberately not enabled: it is 595 KiB of
//! translation layer for a fallback the Chrome/Firefox/Edge majority never
//! touch.

#![forbid(unsafe_code)]
#![warn(missing_docs)]
#![warn(clippy::all)]

pub mod arena;
pub mod camera;
mod gfx;
pub mod mesh;

use std::cell::RefCell;
use std::rc::Rc;
use std::sync::Arc;

use straf3_sim::PlayerState;
use winit::window::Window;

pub use camera::Camera;
pub use gfx::Scene;

/// How far between two simulation states the rendered frame sits, `0.0..=1.0`.
///
/// Rendering interpolates; it never advances the simulation (spec D2).
#[derive(Debug, Clone, Copy)]
pub struct InterpolationAlpha(pub f32);

/// The renderer.
///
/// Construct it once with the window, then call [`render`](Self::render) as
/// often as the display can take it.
///
/// # Why the device is behind an `Rc<RefCell<Option<_>>>`
///
/// On the web it does not exist yet when this returns. `requestAdapter` and
/// `requestDevice` are promises, the browser main thread cannot block on a
/// promise, and so device acquisition necessarily finishes in a later turn of
/// the event loop. The shared cell is where the spawned task deposits it. The
/// native path fills the same cell immediately; keeping one shape means there
/// is one code path to reason about rather than two that drift.
pub struct Renderer {
    gfx: Rc<RefCell<Option<gfx::Gfx>>>,
    /// Remembered so a resize that arrives before the device does is not lost —
    /// on the web that is the common case, not an edge case.
    pending_size: Option<(u32, u32)>,
}

impl Renderer {
    /// Start acquiring a device and surface for `window`.
    ///
    /// Native: blocks until the device is ready, so [`is_ready`](Self::is_ready)
    /// is true when this returns. Web: returns immediately, not ready, and
    /// becomes ready some frames later.
    #[must_use]
    pub fn new(window: Arc<Window>) -> Self {
        Self::with_backends(window, default_backends())
    }

    /// As [`new`](Self::new), but with the backend set chosen by the caller.
    ///
    /// The web host page uses this to pass the backend it picked in JS before
    /// entering wasm. See the crate docs for why that decision cannot be left
    /// to wgpu.
    #[must_use]
    pub fn with_backends(window: Arc<Window>, backends: wgpu::Backends) -> Self {
        let gfx: Rc<RefCell<Option<gfx::Gfx>>> = Rc::new(RefCell::new(None));

        #[cfg(target_arch = "wasm32")]
        {
            let slot = gfx.clone();
            let w = window.clone();
            wasm_bindgen_futures::spawn_local(async move {
                let ready = gfx::Gfx::new(w.clone(), backends).await;
                *slot.borrow_mut() = Some(ready);
                // The frames drawn while this was in flight drew nothing, and
                // nothing else will ask for a redraw if the loop is idle.
                w.request_redraw();
            });
        }

        #[cfg(not(target_arch = "wasm32"))]
        {
            *gfx.borrow_mut() = Some(pollster::block_on(gfx::Gfx::new(window, backends)));
        }

        Self {
            gfx,
            pending_size: None,
        }
    }

    /// Whether the device has arrived and frames are actually being drawn.
    ///
    /// Callers do not have to check this — [`render`](Self::render) is a no-op
    /// until it is true — but a loading indicator wants to know.
    #[must_use]
    pub fn is_ready(&self) -> bool {
        self.gfx.borrow().is_some()
    }

    /// Which backend the device came up on, once it exists.
    #[must_use]
    pub fn backend(&self) -> Option<wgpu::Backend> {
        self.gfx.borrow().as_ref().map(|g| g.backend)
    }

    /// Tell the renderer the window is now `width` × `height` physical pixels.
    ///
    /// Safe to call before the device exists: the size is remembered and
    /// applied when it arrives.
    pub fn resize(&mut self, width: u32, height: u32) {
        match self.gfx.borrow_mut().as_mut() {
            Some(g) => g.resize(width, height),
            None => self.pending_size = Some((width, height)),
        }
    }

    /// Draw one frame of the world, seen from `alpha` of the way between two
    /// simulation states.
    ///
    /// Does nothing until the device exists. That is not defensive coding — on
    /// the web it is guaranteed to happen for the first several frames, and
    /// making it the caller's problem would put a `cfg(target_arch)` in every
    /// event loop.
    pub fn render(&mut self, prev: &PlayerState, curr: &PlayerState, alpha: InterpolationAlpha) {
        let mut slot = self.gfx.borrow_mut();
        let Some(g) = slot.as_mut() else {
            return;
        };
        if let Some((w, h)) = self.pending_size.take() {
            g.resize(w, h);
        }
        g.render(&Camera::between(prev, curr, alpha));
    }
}

/// The backends to use when the caller has no opinion.
///
/// On the web this is WebGPU and only WebGPU, per spec rev 6 Q2. Natively it
/// is whatever the platform offers.
#[must_use]
pub fn default_backends() -> wgpu::Backends {
    #[cfg(target_arch = "wasm32")]
    {
        wgpu::Backends::BROWSER_WEBGPU
    }
    #[cfg(not(target_arch = "wasm32"))]
    {
        wgpu::Backends::from_env().unwrap_or(wgpu::Backends::PRIMARY)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn alpha_is_carried_verbatim() {
        assert_eq!(InterpolationAlpha(0.5).0, 0.5);
    }

    #[test]
    fn the_arena_is_reachable_without_touching_the_gpu() {
        // The property `straf3-game` and the test harness depend on: the world
        // they hand to `step_in_place` comes from this crate, and asking it
        // where the floor is must not require a device.
        use straf3_sim::World as _;
        use straf3_sim::num::{s, vec3};
        use straf3_sim::world::Sweep;

        let hull = straf3_sim::PhysicsProfile::vq3().hull(false);
        let t = arena::ARENA.trace(&Sweep {
            start: arena::SPAWN,
            end: arena::SPAWN - vec3(s(0.0), s(0.0), s(64.0)),
            half_extents: hull.half_extents,
            center_offset: hull.center_offset,
        });
        assert!(t.hit(), "the floor is not under the spawn point");
    }
}
