// The first-person view model: the player's arm and whatever is in it.
//
// Everything else in the game arrives in world coordinates and is put on
// screen by `globals.view_proj`. This does not. The hand is built in
// view space -- x right, y up, -z forward, the eye at the origin -- and
// gets a projection of its own, `hand_view_proj`, for two reasons:
//
//   * it is welded to the camera, so expressing it in world coordinates
//     would mean rebuilding it from the camera basis every time the
//     player turns their head, with every rounding error in that basis
//     landing a hand's breadth from the eye; and
//   * it sits closer than the world's near plane can comfortably reach.
//     Its own projection has a near plane of a centimetre and a narrower
//     field of view, which is what keeps a forearm from being both
//     clipped away and stretched into the corner of the frame.
//
// Depth is the renderer's problem rather than this shader's -- see
// `hand_pipeline` and the viewport depth slice it draws into.
//
// `tint` doubles as flat colour and as a multiplier, the same trick the
// hotbar shader uses: a vertex whose texture layer is UNTEXTURED draws
// in its own colour and samples nothing, which is how a bare forearm and
// a textured pick ride in one pipeline.

struct Globals {
    view_proj: mat4x4<f32>,
    camera_pos: vec4<f32>,
    // xyz: direction the sunlight travels; w: daylight strength 0..1
    sun: vec4<f32>,
    fog_color: vec4<f32>,
    fog_params: vec4<f32>,
    // x: block-light boost, y: AO strength, z: underwater flag, w: fog
    extra: vec4<f32>,
    texture_params: vec4<f32>,
    inv_view_proj: mat4x4<f32>,
    sky_params: vec4<f32>,
    // View space straight to clip space. Built by the renderer from the
    // viewport aspect and nothing else -- the hand does not move.
    hand_view_proj: mat4x4<f32>,
};

@group(0) @binding(0)
var<uniform> globals: Globals;

@group(1) @binding(0)
var block_textures: texture_2d_array<f32>;
@group(1) @binding(1)
var block_sampler: sampler;

// Must match `hand::UNTEXTURED`.
const UNTEXTURED: u32 = 65535u;
// Must match `mesh::pack_light`'s layout, and `LIGHT_MASK` in shader.wgsl.
const LIGHT_MASK: u32 = 16383u;
const ALPHA_CUTOFF: f32 = 0.5;

// Where the light on the hand comes from.
//
// **A fixed direction in view space, not the sun.** The sun is a world
// vector and the hand is in view space, so lighting it by the real sun
// would mean carrying the view matrix into this pass purely to turn one
// direction round. What that would buy is a forearm whose shading swings
// as the player turns on the spot, which is not a thing anybody has ever
// wanted to see: the hand is the one object in the frame that never
// moves relative to the viewer, and a light that stays with it is what
// makes it read as attached rather than as scenery.
//
// Over the player's left shoulder, the standard place to put a key light
// for something held out in front of the camera.
const KEY_LIGHT: vec3<f32> = vec3<f32>(-0.45, 0.72, 0.53);

struct VertexInput {
    @location(0) position: vec3<f32>,
    @location(1) uv: vec2<f32>,
    @location(2) packed: u32,
    @location(3) tint: vec4<f32>,
};

struct VertexOutput {
    @builtin(position) clip_position: vec4<f32>,
    @location(0) uv: vec2<f32>,
    @location(1) @interpolate(flat) tex_layer: u32,
    @location(2) tint: vec4<f32>,
    // sky and block light, already resolved to a single multiplier. Flat
    // because the hand is one object at one place: there is nothing for
    // it to vary across.
    @location(3) @interpolate(flat) light: f32,
};

// Face order is the mesher's: 0 +Y, 1 -Y, 2 +X, 3 -X, 4 +Z, 5 -Z.
fn face_normal(face: u32) -> vec3<f32> {
    if (face == 0u) { return vec3<f32>(0.0, 1.0, 0.0); }
    if (face == 1u) { return vec3<f32>(0.0, -1.0, 0.0); }
    if (face == 2u) { return vec3<f32>(1.0, 0.0, 0.0); }
    if (face == 3u) { return vec3<f32>(-1.0, 0.0, 0.0); }
    if (face == 4u) { return vec3<f32>(0.0, 0.0, 1.0); }
    return vec3<f32>(0.0, 0.0, -1.0);
}

@vertex
fn vs_main(in: VertexInput) -> VertexOutput {
    var out: VertexOutput;
    out.clip_position = globals.hand_view_proj * vec4<f32>(in.position, 1.0);
    out.uv = in.uv;
    out.tex_layer = in.packed >> 16u;
    out.tint = in.tint;

    let word = in.packed & LIGHT_MASK;
    let lambert = max(dot(face_normal((word >> 10u) & 7u), KEY_LIGHT), 0.0) * 0.55 + 0.45;
    // The same two channels the terrain uses, taken at the player's own
    // head: skylight follows the time of day, block light does not, and
    // the brighter of the two wins. A hand that ignored this stayed lit
    // in a cave, which is precisely where the player is paying attention
    // to how dark it is.
    let sky = f32(word & 15u) / 15.0 * globals.sun.w;
    let block = f32((word >> 4u) & 15u) / 15.0 * globals.extra.x;
    // A floor under it, and a low one. The hand is the player's own, and
    // a limb that goes completely black in an unlit cave reads as a bug
    // rather than as darkness -- but it must still be seen to darken, or
    // the torch stops meaning anything.
    out.light = clamp(max(sky * lambert, block) + globals.fog_params.z + 0.12, 0.12, 1.2);
    return out;
}

@fragment
fn fs_main(in: VertexOutput) -> @location(0) vec4<f32> {
    var color = in.tint.rgb;
    if (in.tex_layer != UNTEXTURED) {
        // Plain sampling, no texel snapping: this pipeline is bound to
        // the always-nearest UI sampler, so there is nothing to snap.
        // See `ui_texture_bind_group` and `crisp_uv` in shader.wgsl for
        // the problem that machinery exists to solve and why a view
        // model, which is only ever magnified, does not have it.
        let sampled = textureSample(block_textures, block_sampler, in.uv, i32(in.tex_layer));
        // A tool is a sprite with holes in it. Cut them out rather than
        // blending, or the transparent corners of the picture come out
        // as whatever the item plate had behind it.
        if (sampled.a < ALPHA_CUTOFF) {
            discard;
        }
        color = sampled.rgb * in.tint.rgb;
    }
    return vec4<f32>(color * in.light, 1.0);
}
