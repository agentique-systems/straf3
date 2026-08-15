// straf3's only shader. Flat colours, one directional light, distance fog.
//
// No textures and no material system: spec rev 6 §S puts art out of scope for
// this wave. What is here is the minimum that makes geometry readable — a
// surface facing the light is brighter than one facing away, so a ramp reads
// as a ramp instead of as a flat silhouette — plus fog, which is the cheapest
// way to tell how far away a wall is when there is nothing else to judge by.
//
// WGSL goes straight to the browser on the WebGPU backend: no naga, no
// wgpu-hal, no translation layer. That is most of why the web bundle is small
// (spec rev 6 §P2).

struct Globals {
    view_proj: mat4x4<f32>,
    // xyz = eye position, w = fog density.
    eye: vec4<f32>,
    // rgb = the colour distance fades into, matching the clear colour.
    fog_color: vec4<f32>,
};

@group(0) @binding(0) var<uniform> globals: Globals;

struct VsOut {
    @builtin(position) clip: vec4<f32>,
    @location(0) normal: vec3<f32>,
    @location(1) color: vec3<f32>,
    @location(2) eye_distance: f32,
};

@vertex
fn vs_main(
    @location(0) position: vec3<f32>,
    @location(1) normal: vec3<f32>,
    @location(2) color: vec3<f32>,
) -> VsOut {
    var out: VsOut;
    out.clip = globals.view_proj * vec4<f32>(position, 1.0);
    out.normal = normal;
    out.color = color;
    out.eye_distance = length(position - globals.eye.xyz);
    return out;
}

// Fixed light, high and to one side. A world-space constant rather than a
// uniform: there is no lighting model to tune yet, and pretending otherwise
// would be an API nobody has a use for.
const LIGHT: vec3<f32> = vec3<f32>(0.371, 0.557, 0.743);

@fragment
fn fs_main(in: VsOut) -> @location(0) vec4<f32> {
    let n = normalize(in.normal);
    // Ambient is brighter on upward faces: a cheap stand-in for sky light that
    // keeps floors legible without a second light.
    let ambient = 0.34 + 0.16 * max(n.z, 0.0);
    let diffuse = 0.62 * max(dot(n, LIGHT), 0.0);
    let lit = in.color * (ambient + diffuse);

    // Exponential-squared fog, so near geometry is untouched and the far wall
    // fades rather than being cut off by the far plane.
    let f = clamp(exp(-pow(in.eye_distance * globals.eye.w, 2.0)), 0.0, 1.0);
    let shaded = mix(globals.fog_color.rgb, lit, f);

    // The vertex colours are authored the way they look on screen, so the
    // final step back into the sRGB surface's linear space happens here.
    return vec4<f32>(pow(shaded, vec3<f32>(2.2)), 1.0);
}
