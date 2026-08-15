//! Creating the window, on both targets, and the pointer grab mouse-look needs.
//!
//! # The two targets differ in exactly one place
//!
//! Native creates a window. Web attaches to a `<canvas>` that the host page
//! already put in the document — winit's web backend cannot create a top-level
//! window, and the canvas has to exist before the event loop starts. That is
//! the whole difference, and it is confined to [`WindowConfig::attributes`].

/// How the window should be created.
#[derive(Debug, Clone)]
pub struct WindowConfig {
    /// Title bar text. Ignored on web.
    pub title: String,
    /// Initial inner size in logical pixels. Ignored on web, where the canvas
    /// element's own size wins.
    pub size: (u32, u32),
    /// `id` of the `<canvas>` to attach to. Ignored on native.
    pub canvas_id: String,
}

impl WindowConfig {
    /// The straf3 window.
    #[must_use]
    pub fn straf3() -> Self {
        Self {
            title: "straf3".to_owned(),
            size: (1280, 720),
            canvas_id: "straf3-canvas".to_owned(),
        }
    }

    /// Turn this into winit's attributes for the current target.
    #[must_use]
    pub fn attributes(&self) -> winit::window::WindowAttributes {
        let attributes = winit::window::Window::default_attributes().with_title(self.title.clone());

        // Native only. On web, winit turns a requested inner size into an
        // explicit CSS `width`/`height` on the canvas element, which overrides
        // the host page's `100vw`/`100vh` and pins the game to a 1280×720 box
        // in the corner of the viewport. Measured in headless Chrome, not
        // reasoned about: the canvas came back `style="width: 1280px; height:
        // 720px"` while its backing store was the viewport's real size. The
        // page owns the canvas's size on web; we do not.
        #[cfg(not(target_arch = "wasm32"))]
        let attributes =
            attributes.with_inner_size(winit::dpi::LogicalSize::new(self.size.0, self.size.1));

        #[cfg(target_arch = "wasm32")]
        {
            use wasm_bindgen::JsCast as _;
            use winit::platform::web::WindowAttributesExtWebSys;

            let canvas = web_sys::window()
                .and_then(|w| w.document())
                .and_then(|d| d.get_element_by_id(&self.canvas_id))
                .and_then(|e| e.dyn_into::<web_sys::HtmlCanvasElement>().ok());

            if canvas.is_none() {
                log::error!(
                    "no <canvas id=\"{}\"> in the document; the host page must \
                     provide one before starting straf3",
                    self.canvas_id
                );
            }
            // `prevent_default` keeps the browser from scrolling the page or
            // opening a context menu while the player is strafing.
            return attributes.with_canvas(canvas).with_prevent_default(true);
        }

        #[cfg(not(target_arch = "wasm32"))]
        attributes
    }
}

impl Default for WindowConfig {
    fn default() -> Self {
        Self::straf3()
    }
}

/// Whether the pointer is currently captured for mouse-look.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum PointerGrab {
    /// The pointer is free; mouse motion should not turn the view.
    #[default]
    Released,
    /// The pointer is captured and hidden.
    Grabbed,
}

/// Capture the pointer for mouse-look, and hide it.
///
/// Tries `Locked` first and falls back to `Confined`, because no single mode
/// is supported everywhere: X11 and the browser want `Locked`, some Wayland
/// compositors only offer `Confined`. Returns what actually happened rather
/// than asserting, since on web the browser refuses pointer lock unless it is
/// called from a user gesture — a refusal there is normal, not a bug.
pub fn grab_pointer(window: &winit::window::Window) -> PointerGrab {
    use winit::window::CursorGrabMode;

    let grabbed = window
        .set_cursor_grab(CursorGrabMode::Locked)
        .or_else(|_| window.set_cursor_grab(CursorGrabMode::Confined))
        .is_ok();

    if grabbed {
        window.set_cursor_visible(false);
        PointerGrab::Grabbed
    } else {
        log::debug!("pointer grab refused; mouse-look stays off until it is granted");
        PointerGrab::Released
    }
}

/// Release the pointer and make it visible again.
pub fn release_pointer(window: &winit::window::Window) -> PointerGrab {
    let _ = window.set_cursor_grab(winit::window::CursorGrabMode::None);
    window.set_cursor_visible(true);
    PointerGrab::Released
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_default_window_is_the_straf3_one() {
        let config = WindowConfig::default();
        assert_eq!(config.title, "straf3");
        assert_eq!(config.canvas_id, "straf3-canvas");
    }

    #[test]
    fn attributes_carry_the_title_through() {
        // Constructing attributes touches no windowing system, so this is
        // safe in a headless test runner.
        let config = WindowConfig::straf3();
        assert_eq!(config.attributes().title, "straf3");
    }
}
