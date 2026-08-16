//! Body of the `offscreen` example. Split out of `main.rs` only so the
//! whole thing can be `cfg`'d off for `wasm32`, where none of it exists.

use straf3_render::camera::EYE_HEIGHT;
use straf3_render::{Camera, Scene, camera::DEFAULT_FOV_X};
use straf3_sim::num::{s, vec3};

// The course, shared with the `window` and `web-demo` drivers: one compiled
// map, its mesh drawn here and its hulls collided with there.
#[path = "../shared/course.rs"]
mod course;

const WIDTH: u32 = 960;
const HEIGHT: u32 = 540;
const FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Rgba8UnormSrgb;

/// wgpu requires buffer copy rows to be a multiple of this.
const ROW_ALIGN: u32 = 256;

pub fn main() {
    // Landmarks along the course, in the order a run meets them. The y values
    // are the ones the probe measured the route at (probes/coil-course), so a
    // shot that comes back empty means the geometry moved, not that the camera
    // was guessed badly.
    let (spawn, spawn_yaw) = course::spawn();
    let bounds = course::get().map.bounds;
    let shots: Vec<(&str, Camera)> = vec![
        (
            "spawn",
            Camera {
                // Exactly what the player sees on the first frame.
                eye: vec3(spawn.x, spawn.y, spawn.z + EYE_HEIGHT),
                pitch: s(0.0),
                yaw: spawn_yaw,
                fov_x: DEFAULT_FOV_X,
            },
        ),
        (
            "ramp-wave",
            Camera {
                // Inside the corridor, at the mouth of the ramp wave, looking
                // up it the way a player arrives. The corridor is 448 wide
                // (x -224..224), so a camera off to the side would be outside
                // the wall looking at its back.
                eye: vec3(s(0.0), s(1500.0), s(180.0)),
                pitch: s(6.0),
                yaw: s(90.0),
                fov_x: DEFAULT_FOV_X,
            },
        ),
        (
            "gully",
            Camera {
                // On the launch pad, looking across the gully at the landing
                // lip — the view the jump is actually judged from.
                eye: vec3(s(0.0), s(2260.0), s(230.0)),
                pitch: s(8.0),
                yaw: s(90.0),
                fov_x: DEFAULT_FOV_X,
            },
        ),
        (
            "finish",
            Camera {
                // On the approach, looking at the last jump and the narrow
                // ledge it has to land on.
                eye: vec3(s(0.0), s(3080.0), s(180.0)),
                pitch: s(6.0),
                yaw: s(90.0),
                fov_x: DEFAULT_FOV_X,
            },
        ),
        (
            "overview",
            Camera {
                // High and behind the start, looking up the length of the
                // course. Derived from the compiled bounds rather than
                // hardcoded, so editing the `.map` cannot leave this pointing
                // at empty space.
                eye: vec3(
                    (bounds.mins.x + bounds.maxs.x) * s(0.5),
                    bounds.mins.y - s(512.0),
                    bounds.maxs.z + s(1024.0),
                ),
                pitch: s(28.0),
                yaw: s(90.0),
                fov_x: s(100.0),
            },
        ),
    ];

    let out_dir = std::path::Path::new("target/offscreen");
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
        "offscreen: backend={:?} adapter={:?} type={:?}",
        info.backend, info.name, info.device_type
    );

    let (device, queue) = pollster::block_on(adapter.request_device(&wgpu::DeviceDescriptor {
        label: Some("straf3-offscreen"),
        required_features: wgpu::Features::empty(),
        required_limits: wgpu::Limits::downlevel_defaults().using_resolution(adapter.limits()),
        experimental_features: Default::default(),
        memory_hints: wgpu::MemoryHints::default(),
        trace: wgpu::Trace::Off,
    }))
    .expect("request_device");

    let mesh = straf3_render::mesh::GpuMesh::from_map(&course::get().map.mesh);
    let mut scene = Scene::new(device, queue, FORMAT, WIDTH, HEIGHT, &mesh);
    eprintln!("offscreen: course is {} triangles", scene.triangle_count());

    let target = scene.device().create_texture(&wgpu::TextureDescriptor {
        label: Some("straf3-offscreen-target"),
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

    let padded_row = WIDTH.div_ceil(ROW_ALIGN / 4) * ROW_ALIGN;
    let readback = scene.device().create_buffer(&wgpu::BufferDescriptor {
        label: Some("straf3-readback"),
        size: u64::from(padded_row) * u64::from(HEIGHT),
        usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
        mapped_at_creation: false,
    });

    for (name, camera) in &shots {
        let mut encoder = scene
            .device()
            .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                label: Some("straf3-offscreen"),
            });
        scene.draw(&mut encoder, &view, camera, WIDTH, HEIGHT);
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
                    bytes_per_row: Some(padded_row),
                    rows_per_image: Some(HEIGHT),
                },
            },
            wgpu::Extent3d {
                width: WIDTH,
                height: HEIGHT,
                depth_or_array_layers: 1,
            },
        );
        scene.queue().submit(Some(encoder.finish()));

        let slice = readback.slice(..);
        slice.map_async(wgpu::MapMode::Read, |r| r.expect("map readback buffer"));
        scene
            .device()
            .poll(wgpu::PollType::wait_indefinitely())
            .expect("poll the device");

        let data = slice
            .get_mapped_range()
            .expect("read back the rendered pixels");
        let path = out_dir.join(format!("{name}.ppm"));
        write_ppm(&path, &data, padded_row);
        eprintln!(
            "offscreen: wrote {}  ({})",
            path.display(),
            describe(&data, padded_row)
        );
        drop(data);
        readback.unmap();
    }
}

/// Plain P6 PPM: no image crate, no dependency, and `ffmpeg -i` reads it.
fn write_ppm(path: &std::path::Path, data: &[u8], padded_row: u32) {
    let mut out = Vec::with_capacity((WIDTH * HEIGHT * 3 + 32) as usize);
    out.extend_from_slice(format!("P6\n{WIDTH} {HEIGHT}\n255\n").as_bytes());
    for y in 0..HEIGHT {
        let row = (y * padded_row) as usize;
        for x in 0..WIDTH {
            let p = row + (x * 4) as usize;
            out.extend_from_slice(&data[p..p + 3]);
        }
    }
    std::fs::write(path, out).expect("write the image");
}

/// A one-line summary of what came back, so the run says something useful even
/// when nobody opens the file.
fn describe(data: &[u8], padded_row: u32) -> String {
    // The clear colour, as the sRGB surface stores it.
    let sky = [39u8, 45, 53];
    let mut sky_pixels = 0u32;
    let mut total = 0u32;
    let (mut r, mut g, mut b) = (0u64, 0u64, 0u64);
    for y in 0..HEIGHT {
        let row = (y * padded_row) as usize;
        for x in 0..WIDTH {
            let p = row + (x * 4) as usize;
            let (pr, pg, pb) = (data[p], data[p + 1], data[p + 2]);
            let near_sky =
                pr.abs_diff(sky[0]) < 8 && pg.abs_diff(sky[1]) < 8 && pb.abs_diff(sky[2]) < 8;
            if near_sky {
                sky_pixels += 1;
            }
            r += u64::from(pr);
            g += u64::from(pg);
            b += u64::from(pb);
            total += 1;
        }
    }
    let n = u64::from(total);
    format!(
        "{:.1}% geometry, mean rgb({}, {}, {})",
        100.0 * f64::from(total - sky_pixels) / f64::from(total),
        r / n,
        g / n,
        b / n
    )
}
