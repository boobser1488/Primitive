// Chunk terrain shader.
//
// Lighting model, per fragment:
//   sky   = baked skylight level (0..1) x current daylight strength
//   block = baked block-light level (0..1), unaffected by time of day
//   light = max(sky x lambert, block) + ambient
//
// Keeping the two channels separate is what makes the day/night cycle
// free: the mesh never changes, only `globals.sun.w` does. Taking the max
// (rather than adding) is the standard voxel approximation -- a torch in
// daylight shouldn't blow the surface out to white.
//
// Fog is applied last, toward the sky colour, so the render-distance
// boundary dissolves into the horizon instead of showing as a hard edge.
//
// Transparency comes in two kinds, and they are not interchangeable:
//
//   * **Cutout** -- leaves. Their texture is fully opaque or fully
//     absent per texel, so the empty texels are simply discarded. This
//     runs in the opaque pass and keeps writing depth, which is what
//     lets a tree be drawn in any order.
//   * **Blended** -- water. Flagged per vertex by the mesher, drawn in
//     a second pass with depth writes off. Without the flag the water
//     texture's own alpha (which is 1.0 everywhere) would make lakes
//     look like poured concrete.

struct Globals {
    view_proj: mat4x4<f32>,
    camera_pos: vec4<f32>,
    // xyz: direction the sunlight travels; w: daylight strength 0..1
    sun: vec4<f32>,
    fog_color: vec4<f32>,
    // x: fog start, y: fog end, z: ambient, w: viewport aspect
    fog_params: vec4<f32>,
    // x: block-light boost, y: AO strength, z: underwater flag, w: fog on/off
    extra: vec4<f32>,
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
    @location(2) tex_layer: u32,
    @location(3) light: u32,
};

struct VertexOutput {
    @builtin(position) clip_position: vec4<f32>,
    @location(0) uv: vec2<f32>,
    @location(1) @interpolate(flat) tex_layer: u32,
    @location(2) world_position: vec3<f32>,
    // sky, block, ao -- interpolated across the quad so AO reads smooth
    @location(3) light_terms: vec3<f32>,
    @location(4) @interpolate(flat) face: u32,
    @location(5) @interpolate(flat) translucent: u32,
};

// Must match `mesh::TRANSLUCENT_BIT`.
const TRANSLUCENT_BIT: u32 = 8192u;
// Texels below this are thrown away rather than drawn. 0.5 is the usual
// choice: the leaf textures are 0 or 255, so anything in between only
// comes from filtering at the texel edges.
const ALPHA_CUTOFF: f32 = 0.5;
// How much of what is behind it a block of water lets through.
const WATER_ALPHA: f32 = 0.72;

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
    out.clip_position = globals.view_proj * vec4<f32>(in.position, 1.0);
    out.uv = in.uv;
    out.tex_layer = in.tex_layer;
    out.world_position = in.position;
    out.face = (in.light >> 10u) & 7u;
    out.translucent = in.light & TRANSLUCENT_BIT;

    let sky = f32(in.light & 15u) / 15.0;
    let block = f32((in.light >> 4u) & 15u) / 15.0;
    let ao = f32((in.light >> 8u) & 3u) / 3.0;
    out.light_terms = vec3<f32>(sky, block, ao);
    return out;
}

// Solid terrain: no `discard` anywhere in this entry point.
//
// That is the entire reason there are two. A fragment shader that can
// discard forces the GPU to run it before it knows whether the fragment
// survives, so hardware early-depth rejection is disabled for every draw
// using that shader. With one shared shader, the alpha cutout that makes
// leaves work was costing early-Z on all the terrain -- which is most of
// the triangles in the frame, and exactly what the near-to-far draw
// order exists to let the depth test throw away cheaply.
@fragment
fn fs_solid(in: VertexOutput) -> @location(0) vec4<f32> {
    let sampled = textureSample(block_textures, block_sampler, in.uv, i32(in.tex_layer));
    return shade(in, sampled);
}

// Leaves. Same shading, plus the cutout -- and the early-Z cost, which
// is now paid only by the handful of chunks that contain a tree.
@fragment
fn fs_cutout(in: VertexOutput) -> @location(0) vec4<f32> {
    let sampled = textureSample(block_textures, block_sampler, in.uv, i32(in.tex_layer));

    // Leaf textures carry alpha-0 texels, and without this they were
    // drawn as whatever colour happened to sit under them -- a solid
    // block of it, since the pass does not blend. This is the whole of
    // the fix for "trees are cubes".
    if (sampled.a < ALPHA_CUTOFF) {
        discard;
    }
    return shade(in, sampled);
}

fn shade(in: VertexOutput, sampled: vec4<f32>) -> vec4<f32> {

    let normal = face_normal(in.face);
    let sky_level = in.light_terms.x;
    let block_level = in.light_terms.y;
    let ao = in.light_terms.z;

    // Half-lambert: a face turned away from the sun goes dim, not black,
    // which is what you want when the only other light source is ambient.
    let lambert = max(dot(normal, -globals.sun.xyz), 0.0) * 0.65 + 0.35;
    let sun_term = sky_level * globals.sun.w * lambert;
    let block_term = block_level * globals.extra.x;

    var light = max(sun_term, block_term) + globals.fog_params.z;
    // Ambient occlusion darkens creases; strength is configurable so it
    // can be dialled down without touching the mesher.
    let ao_factor = mix(1.0 - globals.extra.y, 1.0, ao);
    light = clamp(light * ao_factor, 0.0, 1.4);

    var color = sampled.rgb * light;

    if (globals.extra.w > 0.5) {
        let distance = length(in.world_position - globals.camera_pos.xyz);
        let fog_start = globals.fog_params.x;
        let fog_end = max(globals.fog_params.y, fog_start + 1.0);
        // Squared falloff: closer to how real aerial perspective behaves
        // than a straight linear ramp, and it keeps the near field clear.
        let t = clamp((distance - fog_start) / (fog_end - fog_start), 0.0, 1.0);
        color = mix(color, globals.fog_color.rgb, t * t);
    }

    var alpha = sampled.a;
    if (in.translucent != 0u) {
        alpha = WATER_ALPHA;
    }
    return vec4<f32>(color, alpha);
}
