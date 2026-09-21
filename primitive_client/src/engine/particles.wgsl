// Particles: quads with a real UV and a colour, measured from the frame's
// render origin like every other vertex in the world.
//
// **Not in world space**, and that was a bug a player photographed as
// blood in the sky. `view_proj` is built around the render origin
// (`Camera::view_proj_about`) and `camera_pos` is relative to it, so a
// world position fed in here is drawn the origin further on -- the
// player's own altitude straight up. `Particles::build_into` subtracts
// the origin before the vertex is written.
//
// ## Why they are not drawn by the terrain shader
//
// They were, and the thing that could not be expressed there is the one
// thing a chip of a broken block needs. A terrain vertex packs its UV
// into *two bits* -- block faces are quads mapped corner to corner, so
// zero and one are the only values it has ever needed, and that is what
// buys the format its sixteen bytes. A chip wearing the whole of the
// stone texture is a chip with a picture of a wall on it: at thumbnail
// size it reads as a grey square with mortar lines, four of them in
// different places, and the eye sees pattern rather than debris.
//
// What it should wear is **one texel of it** -- a single colour off the
// block that was broken -- and that is a UV of a sixteenth, which the
// terrain vertex cannot say. So particles have a vertex with two floats
// of UV in it and a pipeline of their own.
//
// What that also buys, once the vertex is no longer packed to the last
// bit: a per-particle colour, which is how a spark can be hotter than
// its texture and how everything here can fade out instead of blinking
// off.
//
// ## What it still shares
//
// The texture array, the globals, and the fog -- so a particle forty
// metres off goes the same grey as the terrain behind it, and the pass
// costs one pipeline rather than a second copy of the world's lighting.

struct Globals {
    view_proj: mat4x4<f32>,
    camera_pos: vec4<f32>,
    sun: vec4<f32>,
    fog_color: vec4<f32>,
    // x: start, y: end, z: unused, w: aspect
    fog_params: vec4<f32>,
    // x: block-light boost, y: AO strength, z: underwater, w: fog on
    extra: vec4<f32>,
    texture_params: vec4<f32>,
    inv_view_proj: mat4x4<f32>,
    sky_params: vec4<f32>,
    hand_view_proj: mat4x4<f32>,
};

@group(0) @binding(0)
var<uniform> globals: Globals;

@group(1) @binding(0)
var block_textures: texture_2d_array<f32>;
@group(1) @binding(1)
var block_sampler: sampler;

struct VertexInput {
    @location(0) position: vec3<f32>,
    @location(1) uv: vec2<f32>,
    // Texture layer in the top half, the terrain's light word in the
    // bottom -- the same arrangement the hand and the dropped items use.
    @location(2) packed: u32,
    @location(3) tint: vec4<f32>,
};

struct VertexOutput {
    @builtin(position) clip_position: vec4<f32>,
    @location(0) uv: vec2<f32>,
    @location(1) @interpolate(flat) tex_layer: u32,
    @location(2) tint: vec4<f32>,
    @location(3) shade: f32,
    @location(4) view_distance: f32,
};

const LAYER_SHIFT: u32 = 16u;
const LAYER_MASK: u32 = 0xFFFFu;

@vertex
fn vs_main(in: VertexInput) -> VertexOutput {
    var out: VertexOutput;
    out.clip_position = globals.view_proj * vec4<f32>(in.position, 1.0);
    out.uv = in.uv;
    out.tex_layer = (in.packed >> LAYER_SHIFT) & LAYER_MASK;
    out.tint = in.tint;
    out.view_distance = length(in.position - globals.camera_pos.xyz);

    // Lit by the sky and by whatever the particle carries in its own
    // light word. No lambert term: a particle is a speck, it has no
    // face turned to the sun, and shading one as though it had makes a
    // shower flicker as the drops tumble.
    let light = in.packed & 0xFFu;
    let sky = f32(light & 15u) / 15.0;
    let block = f32((light >> 4u) & 15u) / 15.0;
    let daylight = max(globals.sun.w, 0.0);
    // **The terrain's night, not a night of its own.** The sky's share was
    // `0.25 + 0.75 * daylight`, which never went under a quarter: at
    // midnight a drop, a chip of stone or a leaf in the open was lit four
    // or five times as brightly as the ground it fell past, whose sky light
    // is `sun.w` and a floor (`shader.wgsl`, the `light_floor` note) -- the
    // "затемнение частиц в зависимости от света" the player asked for.
    // Now the sky lights a particle by exactly the daylight it lights the
    // ground with, and the dark floor is the terrain's own ambient
    // (`fog_params.z`), so a speck is never brighter than what it is in
    // front of unless a lamp is lighting it.
    out.shade = clamp(
        max(sky * daylight, block * (1.0 + globals.extra.x)),
        globals.fog_params.z,
        1.0,
    );
    return out;
}

@fragment
fn fs_main(in: VertexOutput) -> @location(0) vec4<f32> {
    var colour = textureSample(block_textures, block_sampler, in.uv, i32(in.tex_layer));
    colour = colour * in.tint;
    if (colour.a < 0.05) {
        discard;
    }
    colour = vec4<f32>(colour.rgb * in.shade, colour.a);

    // The same fog the world gets, so a drop at the edge of sight is as
    // grey as the hillside behind it.
    if (globals.extra.w > 0.5) {
        let start = globals.fog_params.x;
        let end = max(globals.fog_params.y, start + 0.001);
        let fog = clamp((in.view_distance - start) / (end - start), 0.0, 1.0);
        colour = vec4<f32>(mix(colour.rgb, globals.fog_color.rgb, fog), colour.a * (1.0 - fog));
    }
    return colour;
}
