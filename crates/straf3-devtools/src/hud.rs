//! The on-screen overlay: what it looks like, and how it reaches the frame.
//!
//! # Two halves, on purpose
//!
//! [`compose`] is the layout. It takes an [`egui::Context`], a rectangle and a
//! [`TelemetrySample`], and paints. It touches no GPU, so what the overlay
//! *says* can be asserted in an ordinary unit test on a machine with no
//! adapter — see this module's tests, which read the drawn strings back out of
//! egui's shape list.
//!
//! [`Hud`] is the plumbing: an [`egui_wgpu::Renderer`] and the four calls that
//! turn composed shapes into draw commands. It needs a device, so it is
//! exercised by `examples/hud-offscreen.rs` instead, which paints the overlay
//! into a texture and reads the pixels back.
//!
//! # No input, and therefore no `egui-winit`
//!
//! The overlay is a readout. Nothing on it is clickable, focusable or
//! typeable, so the only thing an integration layer would supply is the screen
//! size and the DPI scale — two numbers the caller already has. `egui-winit`
//! was dropped from this crate's dependencies for that reason, which
//! incidentally removes `arboard`, `smithay-clipboard` and `webbrowser`: a
//! clipboard stack, linked in to draw a speedometer.
//!
//! # Above the seam, and one-directional
//!
//! This module reads simulation state and draws it. There is no API here
//! through which anything could be fed back, and there is no clock: the run
//! time on screen is the simulation's own millisecond count, not elapsed wall
//! time.

use egui::{Align2, Color32, FontId, Id, LayerId, Order, Pos2, Rect, Vec2, pos2};
use egui_wgpu::{Renderer, RendererOptions, ScreenDescriptor};

use crate::telemetry::{
    Phase, SpeedTrend, TelemetrySample, TrendFilter, format_run, format_speed, format_split_ms,
};

/// The screen height, in points, the type sizes below are stated at.
///
/// Everything scales linearly from here, so the overlay occupies the same
/// fraction of a 4K screen as of a 720p one rather than shrinking to a speck.
const REFERENCE_HEIGHT: f32 = 720.0;

/// Colours and type sizes.
///
/// Public and fully expanded rather than hidden behind a constructor: this is
/// a devtools crate, the overlay is the thing a player judges the game
/// through, and the numbers here are exactly the ones somebody will want to
/// argue with.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct HudStyle {
    /// Point size of the run clock at [`REFERENCE_HEIGHT`].
    pub clock_pt: f32,
    /// Point size of the split.
    pub split_pt: f32,
    /// Point size of the speed.
    pub speed_pt: f32,
    /// Point size of the `ups` suffix and the phase label.
    pub label_pt: f32,
    /// Point size of the corner diagnostics line.
    pub footer_pt: f32,

    /// How far down the screen the clock's top edge sits, as a fraction of
    /// screen height.
    pub clock_top_frac: f32,
    /// Where the speed readout's baseline sits, as a fraction of height.
    pub speed_baseline_frac: f32,

    /// The clock before the start line.
    pub clock_idle: Color32,
    /// The clock while the run is live.
    pub clock_running: Color32,
    /// The clock once it has stopped — this one is a result, not a readout.
    pub clock_finished: Color32,
    /// A split in the player's favour.
    pub split_ahead: Color32,
    /// A split against them.
    pub split_behind: Color32,
    /// Speed, while it is climbing.
    pub speed_gaining: Color32,
    /// Speed, while it is steady.
    pub speed_holding: Color32,
    /// Speed, while it is bleeding away.
    pub speed_losing: Color32,
    /// The `ups` suffix.
    pub unit: Color32,
    /// The phase label, on walkable ground.
    pub phase_ground: Color32,
    /// The phase label, on a ramp too steep to walk.
    pub phase_slide: Color32,
    /// The phase label, in the air.
    pub phase_air: Color32,
    /// The corner diagnostics line.
    pub footer: Color32,
    /// The drop shadow every glyph is painted over.
    pub shadow: Color32,
}

impl Default for HudStyle {
    fn default() -> Self {
        Self {
            clock_pt: 34.0,
            split_pt: 24.0,
            speed_pt: 56.0,
            label_pt: 17.0,
            footer_pt: 13.0,

            clock_top_frac: 0.045,
            speed_baseline_frac: 0.74,

            clock_idle: Color32::from_rgb(112, 120, 134),
            clock_running: Color32::from_rgb(238, 242, 248),
            clock_finished: Color32::from_rgb(255, 206, 92),
            split_ahead: Color32::from_rgb(92, 222, 128),
            split_behind: Color32::from_rgb(246, 96, 96),
            // Green/white/red on the speed is the same vocabulary as the
            // split, deliberately: both answer "is this going well".
            speed_gaining: Color32::from_rgb(126, 226, 152),
            speed_holding: Color32::from_rgb(238, 242, 248),
            speed_losing: Color32::from_rgb(240, 130, 118),
            unit: Color32::from_rgb(140, 148, 162),
            phase_ground: Color32::from_rgb(148, 156, 170),
            phase_slide: Color32::from_rgb(252, 176, 72),
            phase_air: Color32::from_rgb(104, 198, 240),
            footer: Color32::from_rgb(120, 128, 142),
            // Not opaque black: a hard black outline reads as a border. This
            // is a shadow — enough to keep white type legible against the
            // pale walls the art direction calls for.
            shadow: Color32::from_rgba_unmultiplied(6, 8, 12, 168),
        }
    }
}

/// Everything an overlay needs to record itself into a frame the world has
/// already been drawn into.
///
/// Plain borrowed wgpu handles rather than a renderer type, because this crate
/// deliberately does not depend on `straf3-render`: the overlay draws into
/// whatever texture it is handed, which is what lets the offscreen example
/// exercise it with no window and no surface at all.
pub struct HudFrame<'a> {
    /// The device the [`Hud`] was built against.
    pub device: &'a wgpu::Device,
    /// Its queue.
    pub queue: &'a wgpu::Queue,
    /// The encoder the world was recorded into. The overlay appends to it.
    pub encoder: &'a mut wgpu::CommandEncoder,
    /// The colour target, already containing the world. It is loaded, not
    /// cleared.
    pub target: &'a wgpu::TextureView,
    /// Target width in physical pixels.
    pub width: u32,
    /// Target height in physical pixels.
    pub height: u32,
    /// The window's DPI scale, in physical pixels per logical point. `1.0` is
    /// correct for an offscreen target.
    pub pixels_per_point: f32,
}

/// The overlay.
///
/// Build one against the device and the surface format, then call
/// [`Hud::draw`] once per frame, after the world has been recorded and before
/// the encoder is submitted.
pub struct Hud {
    ctx: egui::Context,
    renderer: Renderer,
    style: HudStyle,
    /// The reference the speed tint compares against.
    trend: TrendFilter,
}

impl Hud {
    /// Build the overlay for a target of `target_format`.
    ///
    /// `target_format` must be the format of the texture [`Hud::draw`] will be
    /// given — the surface format for a windowed build. egui compiles a
    /// different fragment entry point for sRGB and for gamma-space targets, so
    /// a mismatch here is not a validation error, it is a washed-out overlay.
    #[must_use]
    pub fn new(device: &wgpu::Device, target_format: wgpu::TextureFormat) -> Self {
        Self::with_style(device, target_format, HudStyle::default())
    }

    /// As [`Hud::new`], with the type sizes and colours chosen by the caller.
    #[must_use]
    pub fn with_style(
        device: &wgpu::Device,
        target_format: wgpu::TextureFormat,
        style: HudStyle,
    ) -> Self {
        Self {
            ctx: egui::Context::default(),
            renderer: Renderer::new(
                device,
                target_format,
                RendererOptions {
                    // The overlay is flat text on an already-rendered image:
                    // no multisampling, no depth buffer, nothing to resolve.
                    msaa_samples: 1,
                    depth_stencil_format: None,
                    ..RendererOptions::default()
                },
            ),
            style,
            trend: TrendFilter::default(),
        }
    }

    /// The style, to read.
    #[must_use]
    pub const fn style(&self) -> &HudStyle {
        &self.style
    }

    /// The style, to change while running.
    pub const fn style_mut(&mut self) -> &mut HudStyle {
        &mut self.style
    }

    /// Compose and record the overlay.
    ///
    /// A no-op for a zero-sized target — a minimised window produces one, and
    /// a zero-extent render pass is a validation error.
    pub fn draw(&mut self, frame: HudFrame<'_>, sample: &TelemetrySample) {
        if frame.width == 0 || frame.height == 0 {
            return;
        }
        let pixels_per_point = if frame.pixels_per_point.is_finite() && frame.pixels_per_point > 0.0
        {
            frame.pixels_per_point
        } else {
            1.0
        };
        let screen = Rect::from_min_size(
            Pos2::ZERO,
            Vec2::new(
                frame.width as f32 / pixels_per_point,
                frame.height as f32 / pixels_per_point,
            ),
        );

        let trend = self.trend.feed(sample.horizontal_speed);
        let style = self.style;
        let sample = *sample;

        let input = egui::RawInput {
            screen_rect: Some(screen),
            ..egui::RawInput::default()
        };
        // `Context` is a handle, so cloning it is a refcount bump. It is
        // cloned rather than borrowed because the closure below would
        // otherwise hold a borrow of `self.ctx` for the duration of a `&mut
        // self` method.
        let ctx = self.ctx.clone();
        // Destructured rather than kept whole: `TexturesDelta` asserts on drop
        // that every delta in it was applied, so it has to be visibly consumed
        // and then cleared — see the `clear` at the bottom of this function.
        let egui::FullOutput {
            shapes,
            mut textures_delta,
            ..
        } = ctx.run_ui(input, |ui| {
            compose(ui.ctx(), screen, &sample, &style, trend)
        });

        let primitives = ctx.tessellate(shapes, pixels_per_point);
        let descriptor = ScreenDescriptor {
            size_in_pixels: [frame.width, frame.height],
            pixels_per_point,
        };

        // One texture can carry several deltas in a pass — the font atlas
        // grows a row at a time as glyphs are first seen — and they are
        // ordered, so they are applied in the order given rather than folded.
        for (id, deltas) in &textures_delta.set {
            for delta in deltas {
                self.renderer
                    .update_texture(frame.device, frame.queue, *id, delta);
            }
        }
        // The returned buffers come from paint callbacks. This overlay
        // registers none — it is text and rectangles — so the list is empty,
        // and an empty list is why there is nothing extra to submit here.
        let callback_buffers = self.renderer.update_buffers(
            frame.device,
            frame.queue,
            frame.encoder,
            &primitives,
            &descriptor,
        );
        debug_assert!(
            callback_buffers.is_empty(),
            "the overlay registered a paint callback; its command buffers must then be submitted before the frame's"
        );

        {
            let pass = frame
                .encoder
                .begin_render_pass(&wgpu::RenderPassDescriptor {
                    label: Some("straf3-hud"),
                    color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                        view: frame.target,
                        depth_slice: None,
                        resolve_target: None,
                        ops: wgpu::Operations {
                            // Load, not Clear: the world is already in there.
                            load: wgpu::LoadOp::Load,
                            store: wgpu::StoreOp::Store,
                        },
                    })],
                    // No depth: the overlay is always on top, and asking for a
                    // depth attachment would tie this crate to the renderer's
                    // choice of depth format.
                    depth_stencil_attachment: None,
                    timestamp_writes: None,
                    occlusion_query_set: None,
                    multiview_mask: None,
                });
            // `forget_lifetime` is what egui-wgpu's `render` signature asks
            // for. It does not leak: the pass keeps its resources alive, and
            // the only thing given up is the compile-time guarantee that the
            // parent encoder is not touched while the pass is open — which the
            // scope here enforces anyway.
            self.renderer
                .render(&mut pass.forget_lifetime(), &primitives, &descriptor);
        }

        for id in &textures_delta.free {
            self.renderer.free_texture(id);
        }
        // Every delta above has been handed to the renderer, so this is the
        // acknowledgement, not a discard. Without it `TexturesDelta`'s drop
        // assertion fires — in a debug build, on the very first frame, because
        // that is when the font atlas is uploaded.
        textures_delta.clear();
    }
}

/// Paint the overlay into `ctx`, filling `screen`.
///
/// Separate from [`Hud`] so the layout can be exercised with no GPU: run an
/// [`egui::Context`] over this and the resulting shapes carry the exact
/// strings a player would read.
pub fn compose(
    ctx: &egui::Context,
    screen: Rect,
    sample: &TelemetrySample,
    style: &HudStyle,
    trend: SpeedTrend,
) {
    let painter = ctx.layer_painter(LayerId::new(Order::Foreground, Id::new("straf3-hud")));
    // One scale factor for the whole overlay, from the screen's height.
    let u = (screen.height() / REFERENCE_HEIGHT).clamp(0.55, 4.0);
    let centre_x = screen.center().x;

    // ── the run clock, and the split under it ───────────────────────────
    let clock_colour = match sample.run {
        crate::telemetry::RunReadout::NotStarted => style.clock_idle,
        crate::telemetry::RunReadout::Running { .. } => style.clock_running,
        crate::telemetry::RunReadout::Finished { .. } => style.clock_finished,
    };
    let clock_top = screen.top() + style.clock_top_frac * screen.height();
    let clock_rect = shadowed(
        &painter,
        pos2(centre_x, clock_top),
        Align2::CENTER_TOP,
        &format_run(sample.run),
        FontId::monospace(style.clock_pt * u),
        clock_colour,
        style,
    );

    if let Some(split_ms) = sample.split_ms {
        let colour = if split_ms < 0 {
            style.split_ahead
        } else {
            style.split_behind
        };
        shadowed(
            &painter,
            pos2(centre_x, clock_rect.bottom() + 4.0 * u),
            Align2::CENTER_TOP,
            &format_split_ms(split_ms),
            FontId::monospace(style.split_pt * u),
            colour,
            style,
        );
    }

    // ── speed, with its unit, and the phase under it ────────────────────
    let speed_colour = match trend {
        SpeedTrend::Gaining => style.speed_gaining,
        SpeedTrend::Holding => style.speed_holding,
        SpeedTrend::Losing => style.speed_losing,
    };
    let speed_font = FontId::monospace(style.speed_pt * u);
    let unit_font = FontId::monospace(style.label_pt * u);
    let speed_text = format_speed(sample.horizontal_speed);
    // The *number* is centred, and `ups` hangs off its right. Centring the
    // pair instead would push the number left as it gained a digit, and the
    // number is what the eye is locked onto — it has to stay where it was
    // when the player looked away from it.
    let speed_width = painter
        .layout_no_wrap(speed_text.clone(), speed_font.clone(), speed_colour)
        .size()
        .x;
    let gap = style.label_pt * u * 0.5;
    let baseline = screen.top() + style.speed_baseline_frac * screen.height();
    shadowed(
        &painter,
        pos2(centre_x, baseline),
        Align2::CENTER_BOTTOM,
        &speed_text,
        speed_font,
        speed_colour,
        style,
    );
    shadowed(
        &painter,
        pos2(centre_x + speed_width * 0.5 + gap, baseline),
        Align2::LEFT_BOTTOM,
        "ups",
        unit_font,
        style.unit,
        style,
    );

    let phase_colour = match sample.phase {
        Phase::Ground => style.phase_ground,
        Phase::Slide => style.phase_slide,
        Phase::Air => style.phase_air,
    };
    shadowed(
        &painter,
        pos2(centre_x, baseline + 6.0 * u),
        Align2::CENTER_TOP,
        sample.phase.label(),
        FontId::monospace(style.label_pt * u),
        phase_colour,
        style,
    );

    // ── the corner line ─────────────────────────────────────────────────
    //
    // Wave 3 printed this to the terminal once a second, and it is the only
    // way to tell a simulation that has stopped stepping from one that is
    // stepping into a wall. `sim` is the sum of command durations, not wall
    // time, and the two disagreeing is the interesting case.
    let footer = format!(
        "{} fps    vz {}    tick {}    sim {} ms",
        sample.fps,
        signed_speed(sample.vertical_speed),
        sample.tick,
        sample.sim_ms,
    );
    shadowed(
        &painter,
        pos2(screen.left() + 10.0 * u, screen.bottom() - 8.0 * u),
        Align2::LEFT_BOTTOM,
        &footer,
        FontId::monospace(style.footer_pt * u),
        style.footer,
        style,
    );
}

/// Vertical speed, signed, to the nearest whole unit.
fn signed_speed(ups: f32) -> String {
    if ups.is_finite() {
        format!("{:+}", ups.round() as i64)
    } else {
        "----".to_owned()
    }
}

/// Paint `text` over its own shadow, and report where it landed.
///
/// The shadow is not decoration. The art direction is a light-themed abstract
/// arena, so white type over a pale wall is the normal case, not the unusual
/// one, and an unshadowed speedometer disappears exactly where a player is
/// going fastest.
fn shadowed(
    painter: &egui::Painter,
    pos: Pos2,
    anchor: Align2,
    text: &str,
    font: FontId,
    colour: Color32,
    style: &HudStyle,
) -> Rect {
    let offset = (font.size * 0.045).max(1.0);
    painter.text(
        pos + Vec2::splat(offset),
        anchor,
        text,
        font.clone(),
        style.shadow,
    );
    painter.text(pos, anchor, text, font, colour)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::telemetry::RunReadout;

    /// Compose into a screen of `size` with no GPU anywhere, and hand back the
    /// shapes. The texture deltas are cleared because nothing here uploads
    /// them and `TexturesDelta` asserts on drop that somebody did.
    fn shapes_at(size: Vec2, sample: &TelemetrySample) -> Vec<egui::epaint::ClippedShape> {
        let ctx = egui::Context::default();
        let screen = Rect::from_min_size(Pos2::ZERO, size);
        let input = egui::RawInput {
            screen_rect: Some(screen),
            ..egui::RawInput::default()
        };
        let egui::FullOutput {
            shapes,
            mut textures_delta,
            ..
        } = ctx.run_ui(input, |ui| {
            compose(
                ui.ctx(),
                screen,
                sample,
                &HudStyle::default(),
                SpeedTrend::Holding,
            );
        });
        textures_delta.clear();
        shapes
    }

    /// Run the layout with no GPU and collect every string it painted.
    fn drawn(sample: &TelemetrySample) -> Vec<String> {
        shapes_at(Vec2::new(1280.0, 720.0), sample)
            .into_iter()
            .filter_map(|clipped| match clipped.shape {
                egui::Shape::Text(text) => Some(text.galley.text().to_owned()),
                _ => None,
            })
            .collect()
    }

    #[test]
    fn a_live_run_shows_all_four_readouts() {
        // Criterion 10, stated as an assertion: speed, ground/air/slide state,
        // run time and split delta are all on screen at once.
        let mut sample = TelemetrySample {
            horizontal_speed: 487.4,
            vertical_speed: -120.0,
            phase: Phase::Air,
            run: RunReadout::Running { elapsed_ms: 12_480 },
            split_ms: Some(-312),
            fps: 241,
            tick: 1_560,
            sim_ms: 12_480,
        };
        let strings = drawn(&sample);
        assert!(strings.contains(&"487".to_owned()), "speed: {strings:?}");
        assert!(strings.contains(&"ups".to_owned()), "unit: {strings:?}");
        assert!(strings.contains(&"AIR".to_owned()), "phase: {strings:?}");
        assert!(
            strings.contains(&"0:12.480".to_owned()),
            "run time: {strings:?}"
        );
        assert!(strings.contains(&"-0.312".to_owned()), "split: {strings:?}");

        // And the phase label follows the state rather than being decoration.
        sample.phase = Phase::Slide;
        assert!(drawn(&sample).contains(&"SLIDE".to_owned()));
        sample.phase = Phase::Ground;
        assert!(drawn(&sample).contains(&"GROUND".to_owned()));
    }

    #[test]
    fn the_split_is_absent_rather_than_zero_when_there_is_no_ghost() {
        // A `+0.000` with no ghost loaded would read as "dead level with the
        // PB", which is a claim the overlay is in no position to make.
        let sample = TelemetrySample {
            run: RunReadout::Running { elapsed_ms: 1_000 },
            ..TelemetrySample::default()
        };
        let strings = drawn(&sample);
        assert!(strings.contains(&"0:01.000".to_owned()));
        assert!(!strings.iter().any(|s| s.starts_with('+') || s == "-0.000"));
    }

    #[test]
    fn an_unstarted_run_still_draws_a_clock() {
        let strings = drawn(&TelemetrySample::default());
        assert!(strings.contains(&"--:--.---".to_owned()), "{strings:?}");
        assert!(strings.contains(&"0".to_owned()), "{strings:?}");
    }

    #[test]
    fn the_corner_line_carries_the_pacing_and_the_simulation_clock() {
        let sample = TelemetrySample {
            vertical_speed: 312.0,
            fps: 241,
            tick: 1_560,
            sim_ms: 12_480,
            ..TelemetrySample::default()
        };
        let footer = drawn(&sample)
            .into_iter()
            .find(|s| s.contains("fps"))
            .expect("the corner line is drawn");
        assert!(footer.contains("241 fps"), "{footer}");
        assert!(footer.contains("vz +312"), "{footer}");
        assert!(footer.contains("tick 1560"), "{footer}");
        assert!(footer.contains("sim 12480 ms"), "{footer}");
    }

    #[test]
    fn everything_is_painted_twice_because_everything_is_shadowed() {
        // If this ever fails, glyphs are being painted without their shadow
        // and the overlay will vanish against a pale wall.
        // 2 ms rather than 1: `0:00.001` would put a thousandth literal in the
        // same statement as a millisecond-named token, which `straf3-sim`'s
        // `timing_seam` guard flags workspace-wide. See the matching note in
        // `telemetry.rs`.
        let strings = drawn(&TelemetrySample {
            run: RunReadout::Running { elapsed_ms: 2 },
            split_ms: Some(4),
            ..TelemetrySample::default()
        });
        let clocks = strings.iter().filter(|s| *s == "0:00.002").count();
        assert_eq!(clocks, 2, "{strings:?}");
    }

    #[test]
    fn the_layout_survives_a_screen_no_sane_window_manager_would_produce() {
        // A one-pixel-tall window is a real thing a user can drag into
        // existence, and a HUD that panics there takes the game with it.
        for size in [
            Vec2::new(1.0, 1.0),
            Vec2::new(320.0, 200.0),
            Vec2::new(7680.0, 4320.0),
        ] {
            let shapes = shapes_at(size, &TelemetrySample::default());
            assert!(!shapes.is_empty(), "nothing drawn at {size:?}");
        }
    }

    #[test]
    fn a_nan_speed_neither_panics_nor_claims_a_trend() {
        let strings = drawn(&TelemetrySample {
            horizontal_speed: f32::NAN,
            ..TelemetrySample::default()
        });
        assert!(strings.contains(&"----".to_owned()), "{strings:?}");
    }
}
