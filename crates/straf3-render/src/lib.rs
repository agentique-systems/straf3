//! wgpu + WGSL renderer.
//!
//! Above the seam: this is where the GPU is allowed to exist. Nothing below
//! the line may reach this crate — which is what lets the simulation run in
//! CI with no GPU at all (spec criterion 14).
//!
//! # What this crate is responsible for
//!
//! - [`camera`] — the eye, as a pure function of two [`PlayerState`]s and an
//!   interpolation factor.
//! - [`mesh`] — a compiled map's triangles, in the GPU's vertex layout.
//!
//! **The geometry is not declared here any more.** Wave 3 kept the arena in
//! this crate so that the thing you see and the thing you hit were one piece of
//! data. `straf3-map` now does that job properly and below the seam: the hulls
//! and the triangles come out of one pass over one set of brush planes. This
//! crate draws whatever mesh it is handed and has no opinion about which world
//! that is — `straf3-game`'s `scene.rs` is the single place that decides.
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

/// A frame that has had the world drawn into it and has not been submitted yet.
///
/// Handed to an overlay so it can append its own passes to the same encoder.
/// Deliberately plain wgpu handles rather than a trait: `straf3-devtools`
/// depends on this crate for nothing, and an overlay that had to name a
/// `straf3-render` trait could not be tested against an offscreen texture.
pub struct Overlay<'a> {
    /// The device the renderer came up on.
    pub device: &'a wgpu::Device,
    /// Its queue.
    pub queue: &'a wgpu::Queue,
    /// The encoder the world was recorded into.
    pub encoder: &'a mut wgpu::CommandEncoder,
    /// The colour target, already holding the world. Load it; do not clear it.
    pub target: &'a wgpu::TextureView,
    /// Surface width in physical pixels.
    pub width: u32,
    /// Surface height in physical pixels.
    pub height: u32,
}

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
    ///
    /// `mesh` is the world to draw, already compiled. It is taken by value and
    /// moved into the acquisition task because on the web that task finishes
    /// several frames later — borrowing it would tie the renderer's construction
    /// to a lifetime the event loop does not have.
    #[must_use]
    pub fn new(window: Arc<Window>, mesh: mesh::GpuMesh) -> Self {
        Self::with_backends(window, default_backends(), mesh)
    }

    /// As [`new`](Self::new), but with the backend set chosen by the caller.
    ///
    /// The web host page uses this to pass the backend it picked in JS before
    /// entering wasm. See the crate docs for why that decision cannot be left
    /// to wgpu.
    ///
    /// # The host page must confirm an adapter exists first
    ///
    /// On the web, do not call this until `navigator.gpu.requestAdapter()` has
    /// resolved to something non-null. If it resolves to `null`, wgpu's WebGPU
    /// backend reads `.info` off that null and the module dies with a
    /// `TypeError` from inside its own glue, before any error this crate could
    /// return or any message it could print.
    ///
    /// That is not a guess. It was reproduced here, in Chrome 146, against
    /// wgpu 30 with only the WebGPU backend compiled in — independently
    /// confirming what spec rev 6 Q2 recorded for the two-backend case. There
    /// is no recovering from it on this side of the boundary; the host page
    /// asking first is the whole mitigation, which is why
    /// `crates/straf3-render/web/index.html` refuses to enter wasm at all when
    /// the adapter comes back null.
    #[must_use]
    pub fn with_backends(
        window: Arc<Window>,
        backends: wgpu::Backends,
        mesh: mesh::GpuMesh,
    ) -> Self {
        let gfx: Rc<RefCell<Option<gfx::Gfx>>> = Rc::new(RefCell::new(None));

        #[cfg(target_arch = "wasm32")]
        {
            let slot = gfx.clone();
            let w = window.clone();
            wasm_bindgen_futures::spawn_local(async move {
                let ready = gfx::Gfx::new(w.clone(), backends, mesh).await;
                *slot.borrow_mut() = Some(ready);
                // The frames drawn while this was in flight drew nothing, and
                // nothing else will ask for a redraw if the loop is idle.
                w.request_redraw();
            });
        }

        #[cfg(not(target_arch = "wasm32"))]
        {
            *gfx.borrow_mut() = Some(pollster::block_on(gfx::Gfx::new(window, backends, mesh)));
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
        self.render_with(prev, curr, alpha, |_| {});
    }

    /// As [`render`](Self::render), calling `overlay` after the world has been
    /// recorded and before the frame is submitted.
    ///
    /// The overlay is handed the same encoder, so it costs no extra submit and
    /// cannot be drawn under the world.
    pub fn render_with(
        &mut self,
        prev: &PlayerState,
        curr: &PlayerState,
        alpha: InterpolationAlpha,
        overlay: impl FnOnce(Overlay<'_>),
    ) {
        let mut slot = self.gfx.borrow_mut();
        let Some(g) = slot.as_mut() else {
            return;
        };
        if let Some((w, h)) = self.pending_size.take() {
            g.resize(w, h);
        }
        g.render_with(&Camera::between(prev, curr, alpha), overlay);
    }

    /// Run `f` with the device and the surface's texture format, once they
    /// exist. `None` while the device is still being acquired — which on the
    /// web is the first several frames, not an edge case.
    ///
    /// This is how an overlay is constructed: it has to be built against the
    /// same format the frame will be drawn into. egui compiles a different
    /// fragment entry point for sRGB and gamma-space targets, and a mismatch is
    /// not a validation error — it is a washed-out overlay.
    pub fn with_device<R>(
        &self,
        f: impl FnOnce(&wgpu::Device, wgpu::TextureFormat) -> R,
    ) -> Option<R> {
        self.gfx.borrow().as_ref().map(|g| {
            let (device, format) = g.device_and_format();
            f(device, format)
        })
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
    fn a_maps_geometry_is_reachable_without_touching_the_gpu() {
        // The property `straf3-game` and the test harness depend on, restated
        // for the map pipeline: the world handed to `step_in_place` and the
        // mesh handed to the renderer come out of the same compile, and asking
        // either of them a question must not require a device. This test links
        // wgpu and never creates an adapter.
        use straf3_sim::World as _;
        use straf3_sim::num::{s, vec3};
        use straf3_sim::world::Sweep;

        const COIL: &str = include_str!("../../../assets/maps/coil.map");
        let map = straf3_map::compile(COIL).expect("coil.map compiles");
        let world = map.collider();

        let hull = straf3_sim::PhysicsProfile::vq3().hull(false);
        let t = world.trace(&Sweep {
            start: map.spawn,
            end: map.spawn - vec3(s(0.0), s(0.0), s(64.0)),
            half_extents: hull.half_extents,
            center_offset: hull.center_offset,
        });
        assert!(t.hit(), "the floor is not under the spawn point");

        // And the same compile produced something to draw.
        assert!(!mesh::GpuMesh::from_map(&map.mesh).is_empty());
    }
}
