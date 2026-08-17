// Hotbar shader.
//
// Screen-space quads that sample the same block texture array the world
// uses, so a hotbar slot shows the actual texture of the block it will
// place -- no separate icon atlas to keep in sync.
//
// Vertices arrive in normalised device coordinates; x is divided by the
// viewport aspect so slots stay square on any window shape. Depth
// testing is off, so the bar always sits on top of the world.
//
// `tint` doubles as the selection highlight and as the frame colour: a
// vertex with texture layer 0xFFFFFFFF draws as flat colour instead of
// sampling, which lets the frames and the selection box share this one
// pipeline.

struct Globals {
    view_proj: mat4x4<f32>,
    camera_pos: vec4<f32>,
    sun: vec4<f32>,
    fog_color: vec4<f32>,
    // w carries the viewport aspect
    fog_params: vec4<f32>,
    extra: vec4<f32>,
};

@group(0) @binding(0)
var<uniform> globals: Globals;

@group(1) @binding(0)
var block_textures: texture_2d_array<f32>;
@group(1) @binding(1)
var block_sampler: sampler;

const UNTEXTURED: u32 = 4294967295u;

struct VertexInput {
    @location(0) position: vec2<f32>,
    @location(1) uv: vec2<f32>,
    @location(2) tex_layer: u32,
    @location(3) tint: vec4<f32>,
};

struct VertexOutput {
    @builtin(position) clip_position: vec4<f32>,
    @location(0) uv: vec2<f32>,
    @location(1) @interpolate(flat) tex_layer: u32,
    @location(2) tint: vec4<f32>,
};

@vertex
fn vs_main(in: VertexInput) -> VertexOutput {
    var out: VertexOutput;
    let aspect = max(globals.fog_params.w, 0.0001);
    out.clip_position = vec4<f32>(in.position.x / aspect, in.position.y, 0.0, 1.0);
    out.uv = in.uv;
    out.tex_layer = in.tex_layer;
    out.tint = in.tint;
    return out;
}

@fragment
fn fs_main(in: VertexOutput) -> @location(0) vec4<f32> {
    if (in.tex_layer == UNTEXTURED) {
        return in.tint;
    }
    let sampled = textureSample(block_textures, block_sampler, in.uv, i32(in.tex_layer));
    return vec4<f32>(sampled.rgb * in.tint.rgb, sampled.a * in.tint.a);
}
