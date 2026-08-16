//! Body of the `ghost-offscreen` example. Split out of `main.rs` only so the
//! whole thing can be `cfg`'d off for `wasm32`, where none of it exists.

use straf3_render::camera::{DEFAULT_FOV_X, EYE_HEIGHT};
use straf3_render::{Camera, GhostPose, Scene};
use straf3_sim::PhysicsProfile;
use straf3_sim::num::{s, vec3};

// The same compiled course the other examples use: one map, its mesh drawn
// here and its hulls collided with there.
#[path = "../shared/course.rs"]
mod course;

const WIDTH: u32 = 960;
const HEIGHT: u32 = 540;
const FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Rgba8UnormSrgb;

/// wgpu requires buffer copy rows to be a multiple of this.
const ROW_ALIGN: u32 = 256;

/// How different two pixels must be before they count as changed. Small,
/// because the ghost is translucent and its faintest part is the point: a
/// threshold that only counted the rim would pass a ghost drawn as an outline.
const CHANGED: u8 = 3;

pub fn main() {
    let (spawn, spawn_yaw) = course::spawn();
    let hull = PhysicsProfile::cpm().hull(false);

    // Behind and slightly above the spawn, looking along the course — the
    // third-person angle a ghost is easiest to judge from. The player's own
    // camera is at the spawn, so a ghost standing *at* the spawn would be
    // inside the near plane.
    let camera = Camera {
        eye: vec3(spawn.x, spawn.y - s(180.0), spawn.z + EYE_HEIGHT + s(40.0)),
        pitch: s(-6.0),
        yaw: spawn_yaw,
        fov_x: DEFAULT_FOV_X,
    };

    // Three frames. The first is the control; the second must differ from it;
    // the third must not.
    let shots: Vec<(&str, Option<GhostPose>)> = vec![
        ("world-only", None),
        (
            "ghost-visible",
            Some(GhostPose {
                origin: spawn,
                yaw: spawn_yaw,
                half_extents: hull.half_extents,
                center_offset: hull.center_offset,
            }),
        ),
        (
            "ghost-behind-the-floor",
            Some(GhostPose {
                // Under the map. The floor is between it and the camera, so
                // the depth test must hide it completely — this is the shot
                // that would fail if the world pass discarded its depth or the
                // ghost pass ignored it.
                origin: vec3(spawn.x, spawn.y, spawn.z - s(512.0)),
                yaw: spawn_yaw,
                half_extents: hull.half_extents,
                center_offset: hull.center_offset,
            }),
        ),
    ];

    let out_dir = std::path::Path::new("target/ghost-offscreen");
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
        "ghost-offscreen: backend={:?} adapter={:?} type={:?}",
        info.backend, info.name, info.device_type
    );

    let (device, queue) = pollster::block_on(adapter.request_device(&wgpu::DeviceDescriptor {
        label: Some("straf3-ghost-offscreen"),
        required_features: wgpu::Features::empty(),
        required_limits: wgpu::Limits::downlevel_defaults().using_resolution(adapter.limits()),
        experimental_features: Default::default(),
        memory_hints: wgpu::MemoryHints::default(),
        trace: wgpu::Trace::Off,
    }))
    .expect("request_device");

    let mesh = straf3_render::mesh::GpuMesh::from_map(&course::get().map.mesh);
    let mut scene = Scene::new(device, queue, FORMAT, WIDTH, HEIGHT, &mesh);

    let target = scene.device().create_texture(&wgpu::TextureDescriptor {
        label: Some("straf3-ghost-offscreen-target"),
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
        label: Some("straf3-ghost-readback"),
        size: u64::from(padded_row) * u64::from(HEIGHT),
        usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
        mapped_at_creation: false,
    });

    let mut frames: Vec<(&str, Vec<u8>)> = Vec::new();
    for (name, pose) in &shots {
        let mut encoder = scene
            .device()
            .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                label: Some("straf3-ghost-offscreen"),
            });
        scene.draw(&mut encoder, &view, &camera, WIDTH, HEIGHT);
        if let Some(pose) = pose {
            scene.draw_ghost(&mut encoder, &view, &camera, WIDTH, HEIGHT, pose);
        }
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
            .expect("read back the rendered pixels")
            .to_vec();
        drop(slice);
        readback.unmap();

        let path = out_dir.join(format!("{name}.ppm"));
        write_ppm(&path, &data, padded_row);
        eprintln!("ghost-offscreen: wrote {}", path.display());
        frames.push((name, data));
    }

    let visible = changed_pixels(&frames[0].1, &frames[1].1, padded_row);
    let occluded = changed_pixels(&frames[0].1, &frames[2].1, padded_row);
    eprintln!("ghost-offscreen: ghost in the open changed {visible} pixels");
    eprintln!("ghost-offscreen: ghost under the floor changed {occluded} pixels");

    // Asserted, not merely printed: an example whose numbers nobody reads is a
    // screenshot with extra steps.
    assert!(
        visible > 2_000,
        "the ghost changed only {visible} pixels — it is not being drawn"
    );
    assert_eq!(
        occluded, 0,
        "the floor did not hide the ghost behind it: {occluded} pixels changed, \
         so the ghost pass is not depth-testing against the world"
    );
    eprintln!("ghost-offscreen: OK — drawn where it should be, hidden where it should not");
}

/// How many pixels differ between two frames by more than [`CHANGED`].
fn changed_pixels(a: &[u8], b: &[u8], padded_row: u32) -> u32 {
    let mut changed = 0;
    for y in 0..HEIGHT {
        let row = (y * padded_row) as usize;
        for x in 0..WIDTH {
            let p = row + (x * 4) as usize;
            if (0..3).any(|c| a[p + c].abs_diff(b[p + c]) > CHANGED) {
                changed += 1;
            }
        }
    }
    changed
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
