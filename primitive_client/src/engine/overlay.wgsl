// Screen-space overlay: crosshair, and the loading screen shown while
// the world under the player streams in.
//
// Vertices arrive already in normalised device coordinates; the only
// transform is dividing x by the viewport aspect so the cross stays
// square when the window isn't. Drawn with depth testing disabled so it
// sits on top of the world regardless of what's in front of the camera.

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

struct VertexInput {
    @location(0) position: vec2<f32>,
    @location(1) color: vec4<f32>,
};

struct VertexOutput {
    @builtin(position) clip_position: vec4<f32>,
    @location(0) color: vec4<f32>,
};

@vertex
fn vs_main(in: VertexInput) -> VertexOutput {
    var out: VertexOutput;
    let aspect = max(globals.fog_params.w, 0.0001);
    out.clip_position = vec4<f32>(in.position.x / aspect, in.position.y, 0.0, 1.0);
    out.color = in.color;
    return out;
}

@fragment
fn fs_main(in: VertexOutput) -> @location(0) vec4<f32> {
    // Colour comes from the vertex now, so the same pipeline draws both
    // the crosshair and the loading screen.
    return in.color;
}
