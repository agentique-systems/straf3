//! Body of the `hud-offscreen` example. Split out of `main.rs` only so the
//! whole thing can be `cfg`'d off for `wasm32`, where none of it exists.

use straf3_devtools::{Hud, HudFrame, Phase, RunReadout, TelemetrySample};

const WIDTH: u32 = 1280;
const HEIGHT: u32 = 720;

/// The surface format a native window actually comes up on here, so the sRGB
/// entry point this exercises is the one the game uses.
const FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Bgra8UnormSrgb;

/// wgpu requires buffer copy rows to be a multiple of this.
const ROW_ALIGN: u32 = 256;

/// The colour the world is cleared to, copied from `straf3-render`'s `CLEAR`.
/// Duplicated rather than depended on: this crate has no reason to link the
/// renderer, and the exact shade only matters here as "the background".
const CLEAR: wgpu::Color = wgpu::Color {
    r: 0.055,
    g: 0.075,
    b: 0.105,
    a: 1.0,
};

pub fn main() {
    let frames: Vec<(&str, TelemetrySample)> = vec![
        (
            "before-the-line",
            TelemetrySample {
                horizontal_speed: 0.0,
                phase: Phase::Ground,
                run: RunReadout::NotStarted,
                foot_clearance: None,
                fps: 240,
                ..TelemetrySample::default()
            },
        ),
        (
            "mid-run-ahead",
            TelemetrySample {
                horizontal_speed: 487.4,
                vertical_speed: -120.0,
                phase: Phase::Air,
                run: RunReadout::Running { elapsed_ms: 12_480 },
                split_ms: Some(-312),
                foot_clearance: None,
                fps: 241,
                tick: 1_560,
                sim_ms: 12_480,
            },
        ),
        (
            "on-a-ramp-behind",
            TelemetrySample {
                horizontal_speed: 1042.0,
                vertical_speed: 312.0,
                phase: Phase::Slide,
                run: RunReadout::Running { elapsed_ms: 31_007 },
                split_ms: Some(1_204),
                foot_clearance: None,
                fps: 238,
                tick: 3_875,
                sim_ms: 31_007,
            },
        ),
        (
            "finished",
            TelemetrySample {
                horizontal_speed: 96.0,
                vertical_speed: 0.0,
                phase: Phase::Ground,
                run: RunReadout::Finished { time_ms: 42_318 },
                split_ms: Some(-1_882),
                foot_clearance: None,
                fps: 240,
                tick: 5_290,
                sim_ms: 46_000,
            },
        ),
    ];

    let out_dir = std::path::Path::new("target/hud-offscreen");
    std::fs::create_dir_all(out_dir).expect("create the output directory");

    let instance =
        wgpu::Instance::new(wgpu::InstanceDescriptor::new_without_display_handle_from_env());
    let adapter = pollster::block_on(instance.request_adapter(&wgpu::RequestAdapterOptions {
        power_preference: wgpu::PowerPreference::HighPerformance,
        compatible_surface: None,
        force_fallback_adapter: false,
        apply_limit_buckets: false,
    }))
    .expect("no GPU adapter at all — not even a software one");
    let info = adapter.get_info();
    eprintln!(
        "hud-offscreen: backend={:?} adapter={:?} type={:?}",
        info.backend, info.name, info.device_type
    );

    let (device, queue) = pollster::block_on(adapter.request_device(&wgpu::DeviceDescriptor {
        label: Some("straf3-hud-offscreen"),
        required_features: wgpu::Features::empty(),
        required_limits: wgpu::Limits::downlevel_defaults().using_resolution(adapter.limits()),
        experimental_features: Default::default(),
        memory_hints: wgpu::MemoryHints::default(),
        trace: wgpu::Trace::Off,
    }))
    .expect("request_device");

    let target = device.create_texture(&wgpu::TextureDescriptor {
        label: Some("hud-offscreen-target"),
        size: wgpu::Extent3d {
            width: WIDTH,
            height: HEIGHT,
            depth_or_array_layers: 1,
        },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format: FORMAT,
        usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC,
        view_formats: &[],
    });
    let view = target.create_view(&wgpu::TextureViewDescriptor::default());

    let bytes_per_row = (WIDTH * 4).div_ceil(ROW_ALIGN) * ROW_ALIGN;
    let readback = device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("hud-offscreen-readback"),
        size: u64::from(bytes_per_row) * u64::from(HEIGHT),
        usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
        mapped_at_creation: false,
    });

    let mut hud = Hud::new(&device, FORMAT);
    let mut failures = 0usize;

    for (name, sample) in &frames {
        let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
            label: Some("hud-offscreen-frame"),
        });

        // Stand in for the world: clear the target to the sky colour, exactly
        // as `Scene::draw` would, so the overlay is composited over something
        // rather than over an undefined texture.
        encoder
            .begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("hud-offscreen-clear"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: &view,
                    depth_slice: None,
                    resolve_target: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Clear(CLEAR),
                        store: wgpu::StoreOp::Store,
                    },
                })],
                depth_stencil_attachment: None,
                timestamp_writes: None,
                occlusion_query_set: None,
                multiview_mask: None,
            })
            .forget_lifetime();

        hud.draw(
            HudFrame {
                device: &device,
                queue: &queue,
                encoder: &mut encoder,
                target: &view,
                width: WIDTH,
                height: HEIGHT,
                pixels_per_point: 1.0,
            },
            sample,
        );

        encoder.copy_texture_to_buffer(
            wgpu::TexelCopyTextureInfo {
                texture: &target,
                mip_level: 0,
                origin: wgpu::Origin3d::ZERO,
                aspect: wgpu::TextureAspect::All,
            },
            wgpu::TexelCopyBufferInfo {
                buffer: &readback,
                layout: wgpu::TexelCopyBufferLayout {
                    offset: 0,
                    bytes_per_row: Some(bytes_per_row),
                    rows_per_image: Some(HEIGHT),
                },
            },
            wgpu::Extent3d {
                width: WIDTH,
                height: HEIGHT,
                depth_or_array_layers: 1,
            },
        );
        queue.submit(Some(encoder.finish()));

        let slice = readback.slice(..);
        slice.map_async(wgpu::MapMode::Read, |r| r.expect("map readback buffer"));
        device
            .poll(wgpu::PollType::wait_indefinitely())
            .expect("poll the device");
        let data = slice
            .get_mapped_range()
            .expect("read back the rendered pixels");

        let painted = write_ppm(&out_dir.join(format!("{name}.ppm")), &data, bytes_per_row);
        drop(data);
        readback.unmap();
        // Every one of these frames has text on it. A frame that changed no
        // pixels means the overlay reached the tessellator and not the
        // texture, which is exactly the failure the unit tests cannot see.
        let ok = painted > 2_000;
        if !ok {
            failures += 1;
        }
        eprintln!(
            "hud-offscreen: {name:<18} {painted:>7} pixels painted over the background  {}",
            if ok { "ok" } else { "FAILED" }
        );
    }

    eprintln!(
        "hud-offscreen: wrote {} PPMs to {}",
        frames.len(),
        out_dir.display()
    );
    assert_eq!(failures, 0, "{failures} frame(s) drew no overlay");
}

/// Write the readback as a PPM and report how many pixels differ from the
/// background clear colour.
fn write_ppm(path: &std::path::Path, data: &[u8], bytes_per_row: u32) -> usize {
    // The clear colour as it lands in an 8-bit sRGB texture: the clear value
    // is linear, and the target is sRGB, so the stored bytes are the encoded
    // ones. Rather than reimplement the transfer function, take whatever the
    // top-left corner holds — nothing is ever painted there.
    let background = [data[0], data[1], data[2]];

    let mut ppm = format!("P6\n{WIDTH} {HEIGHT}\n255\n").into_bytes();
    let mut painted = 0usize;
    for y in 0..HEIGHT {
        let row = (y * bytes_per_row) as usize;
        for x in 0..WIDTH {
            let px = row + (x * 4) as usize;
            // Bgra8UnormSrgb.
            let (b, g, r) = (data[px], data[px + 1], data[px + 2]);
            if [b, g, r] != background {
                painted += 1;
            }
            ppm.extend_from_slice(&[r, g, b]);
        }
    }
    std::fs::write(path, ppm).expect("write the PPM");
    painted
}
