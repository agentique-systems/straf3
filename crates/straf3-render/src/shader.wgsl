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
    @location(3) world: vec3<f32>,
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
    out.world = position;
    return out;
}

// Edge length of one checkerboard cell, in Quake units. 128 is two player
// widths, which reads as a floor tile at running speed rather than as noise.
const TILE: f32 = 128.0;
// How much darker the odd cells are.
const CHECKER: f32 = 0.86;

// Why there is a checkerboard at all: with flat colours and no textures, a
// large single-coloured floor has no optical flow. At 600 units a second the
// screen does not change, and the movement — the whole point of the project —
// cannot be felt. This is the cheapest thing that restores it.
//
// Why it is here and not in the mesh: the arena used to solve this by
// subdividing big faces into tiled quads, which is bounded when the world is
// ten boxes and is not when it is four thousand brushes. Per-fragment costs no
// triangles and works on a brush face of any shape or orientation.
//
// A 3-D checker sampled on the surface, rather than a 2-D one per face: it
// needs no per-face basis, and on any axis-aligned plane one of the three terms
// is constant, so what you see on a floor or a wall is an ordinary 2-D
// chequerboard. On a ramp it stays a regular pattern that moves with you, which
// is all the eye needs.
fn checker(world: vec3<f32>) -> f32 {
    let cell = floor(world / TILE);
    let parity = (cell.x + cell.y + cell.z) - 2.0 * floor((cell.x + cell.y + cell.z) * 0.5);
    return mix(1.0, CHECKER, parity);
}

// Fixed light, high and to one side. A world-space constant rather than a
// uniform: there is no lighting model to tune yet, and pretending otherwise
// would be an API nobody has a use for.
const LIGHT: vec3<f32> = vec3<f32>(0.371, 0.557, 0.743);

@fragment
fn fs_main(in: VsOut) -> @location(0) vec4<f32> {
    // Vertex colours are authored the way they should look on screen, and the
    // surface is an sRGB format — the hardware encodes on write. So the first
    // thing to do is get out of display space and into linear, where lighting
    // and fog are the only place they mean anything. Doing this at the end
    // instead (the obvious-looking arrangement) fogs geometry towards a black
    // that does not match the sky, and the horizon comes out as a dark band.
    let base = pow(in.color, vec3<f32>(2.2)) * checker(in.world);

    let n = normalize(in.normal);
    // A hemisphere ambient — brighter facing up, dimmer facing down — plus one
    // directional light. Two terms, because with one the walls facing away
    // from the light go to near-black and the arena reads as a cave.
    let ambient = 0.45 + 0.25 * n.z;
    let diffuse = 0.55 * max(dot(n, LIGHT), 0.0);
    let lit = base * (ambient + diffuse);

    // Exponential-squared fog, in linear space and towards the same colour the
    // pass clears to, so geometry at the far wall meets the sky rather than
    // ending at a visible edge.
    let f = clamp(exp(-pow(in.eye_distance * globals.eye.w, 2.0)), 0.0, 1.0);
    return vec4<f32>(mix(globals.fog_color.rgb, lit, f), 1.0);
}
