// Stretches the small sky over the screen.
//
// The sky is the most expensive thing in the frame -- with the horizon
// in view it measured 0.68 ms of a 1.25 ms frame -- and every attempt
// to make its shader cheaper has failed on measurement. Its arithmetic
// is dense and its one sine turned out to be faster than the fifteen
// operations proposed to replace it.
//
// So it gets fewer pixels instead. Drawn into a texture at a fraction
// of the width and height, then pulled back up here: a quarter of the
// pixels at half scale, a ninth at a third, and the shader is untouched.
// This is the one saving on a fill-bound pass that cannot fail to work,
// because it is not an optimisation of the work -- it is less of it.
//
// What it costs is detail the sky has very little of. The gradient and
// the clouds survive it almost exactly: the clouds are already drawn on
// a grid about twelve blocks square, which is far coarser than a pixel.
// Stars, and the rims of the sun and the moon, are the things that
// soften -- which is why this is a setting rather than a decision.

@group(0) @binding(0)
var sky_texture: texture_2d<f32>;
@group(0) @binding(1)
var sky_sampler: sampler;

struct BlitOut {
    @builtin(position) clip_position: vec4<f32>,
    @location(0) uv: vec2<f32>,
};

// The same three-point triangle the sky itself uses, with texture
// coordinates instead of a ray.
@vertex
fn vs_blit(@builtin(vertex_index) index: u32) -> BlitOut {
    var out: BlitOut;
    let x = f32((index << 1u) & 2u) * 2.0 - 1.0;
    let y = f32(index & 2u) * 2.0 - 1.0;
    // z = 1: the far plane, so the depth test keeps this only where no
    // terrain was drawn -- exactly as the sky pass it replaces did.
    out.clip_position = vec4<f32>(x, y, 1.0, 1.0);
    // Clip space has y up and texture space has y down.
    out.uv = vec2<f32>(x * 0.5 + 0.5, 0.5 - y * 0.5);
    return out;
}

@fragment
fn fs_blit(in: BlitOut) -> @location(0) vec4<f32> {
    // Linear filtering, so the seams between the small texture's pixels
    // are not visible as a grid over the sky. The sampler decides; this
    // is one fetch either way.
    return textureSample(sky_texture, sky_sampler, in.uv);
}
