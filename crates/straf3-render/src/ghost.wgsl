// The ghost: the player hull of a recorded run, drawn beside the live player.
//
// A separate shader from `shader.wgsl` rather than a flag on it, for two
// reasons that are both about honesty of the picture. The world shader applies
// a world-space checkerboard, which on a moving body would swim through it and
// read as a texture bug. And the world shader writes opaque: a ghost has to be
// see-through or it hides the corner you are about to take.

struct Ghost {
    view_proj: mat4x4<f32>,
    // xyz = where the recorded player's origin is this frame.
    origin: vec4<f32>,
    // xyz = the collision hull's half extents, so the shape you race is the
    // shape you would have collided as. w unused.
    half_extents: vec4<f32>,
    // xyz = the hull's centre relative to the origin. w unused.
    center_offset: vec4<f32>,
    // Yaw as its cosine and sine, computed on the CPU: the ghost faces the way
    // the recorded run was looking, which is what makes a strafe read as a
    // strafe rather than as a box sliding sideways.
    basis: vec4<f32>,
    // Linear RGB and the alpha the body is blended at.
    color: vec4<f32>,
};

@group(0) @binding(0) var<uniform> ghost: Ghost;

struct VsOut {
    @builtin(position) clip: vec4<f32>,
    @location(0) normal: vec3<f32>,
    // Direction from the surface to the eye, for the edge highlight.
    @location(1) to_eye: vec3<f32>,
};

// Rotate about Z by the yaw the CPU already resolved into cos/sin.
fn spin(v: vec3<f32>) -> vec3<f32> {
    let c = ghost.basis.x;
    let s = ghost.basis.y;
    return vec3<f32>(v.x * c - v.y * s, v.x * s + v.y * c, v.z);
}

@vertex
fn vs_main(
    @location(0) position: vec3<f32>,
    @location(1) normal: vec3<f32>,
) -> VsOut {
    // The cube arrives as ±1 on every axis, so one multiply gives the real
    // hull: 30 × 30 × 56 units standing, and a shorter box the moment the
    // recorded run was crouching.
    let local = spin(position * ghost.half_extents.xyz + ghost.center_offset.xyz);
    let world = ghost.origin.xyz + local;

    var out: VsOut;
    out.clip = ghost.view_proj * vec4<f32>(world, 1.0);
    out.normal = spin(normal);
    // The eye is not passed in: the inverse of the view-projection would give
    // it, but the highlight only needs a direction that varies over the body,
    // and the vector from the hull's centre outwards is exactly that.
    out.to_eye = normalize(local);
    return out;
}

@fragment
fn fs_main(in: VsOut) -> @location(0) vec4<f32> {
    let n = normalize(in.normal);

    // The same two-term lighting the world uses, so the ghost sits in the same
    // light as the map rather than looking pasted on.
    let ambient = 0.55 + 0.25 * n.z;
    let diffuse = 0.45 * max(dot(n, vec3<f32>(0.371, 0.557, 0.743)), 0.0);

    // Edges brighter than faces. A flat translucent box at a distance is a
    // smudge; the rim is what makes it read as a body, and it is what you can
    // still pick out against a pale wall.
    let facing = abs(dot(n, normalize(in.to_eye)));
    let rim = pow(1.0 - facing, 3.0);

    let lit = ghost.color.rgb * (ambient + diffuse) + vec3<f32>(rim * 0.55);
    // Premultiplied alpha: the blend state is One/OneMinusSrcAlpha, so the rim
    // adds light without also making the body more opaque where it glows.
    let alpha = clamp(ghost.color.a + rim * 0.35, 0.0, 1.0);
    return vec4<f32>(lit * alpha, alpha);
}
