// Remote-player hitbox shader. Shares the chunk shader's globals so
// actors are lit by the same sun and swallowed by the same fog -- an
// unfogged player box floating in a fogged world is exactly the kind of
// detail that makes a scene look wrong without anyone being able to say
// why.

struct Globals {
    view_proj: mat4x4<f32>,
    camera_pos: vec4<f32>,
    sun: vec4<f32>,
    fog_color: vec4<f32>,
    fog_params: vec4<f32>,
    extra: vec4<f32>,
};

@group(0) @binding(0)
var<uniform> globals: Globals;

struct VertexInput {
    @location(0) position: vec3<f32>,
    @location(1) color: vec3<f32>,
    @location(2) normal: vec3<f32>,
};

struct VertexOutput {
    @builtin(position) clip_position: vec4<f32>,
    @location(0) color: vec3<f32>,
    @location(1) world_position: vec3<f32>,
    @location(2) normal: vec3<f32>,
};

@vertex
fn vs_main(in: VertexInput) -> VertexOutput {
    var out: VertexOutput;
    out.clip_position = globals.view_proj * vec4<f32>(in.position, 1.0);
    out.color = in.color;
    out.world_position = in.position;
    out.normal = in.normal;
    return out;
}

@fragment
fn fs_main(in: VertexOutput) -> @location(0) vec4<f32> {
    let lambert = max(dot(normalize(in.normal), -globals.sun.xyz), 0.0) * 0.6 + 0.4;
    // Players are lit as if standing in the open; there's no per-entity
    // light sampling yet, so they'd otherwise go pitch black in caves.
    let light = clamp(globals.sun.w * lambert + globals.fog_params.z + 0.25, 0.0, 1.3);
    var color = in.color * light;

    if (globals.extra.w > 0.5) {
        let distance = length(in.world_position - globals.camera_pos.xyz);
        let fog_start = globals.fog_params.x;
        let fog_end = max(globals.fog_params.y, fog_start + 1.0);
        let t = clamp((distance - fog_start) / (fog_end - fog_start), 0.0, 1.0);
        color = mix(color, globals.fog_color.rgb, t * t);
    }

    return vec4<f32>(color, 1.0);
}
