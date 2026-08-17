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
    // x: texture resolution in texels, y..w spare
    texture_params: vec4<f32>,
};

@group(0) @binding(0)
var<uniform> globals: Globals;

@group(1) @binding(0)
var block_textures: texture_2d_array<f32>;
@group(1) @binding(1)
var block_sampler: sampler;

// Everything but the position rides in one word. See `mesh::Vertex` for
// the layout and for why it is worth packing at all.
struct VertexInput {
    @location(0) position: vec3<f32>,
    @location(1) packed: u32,
};

// Must match `UV_SHIFT`, `LAYER_SHIFT` and `TINT_SHIFT` in mesh.rs.
const UV_SHIFT: u32 = 14u;
const LAYER_SHIFT: u32 = 16u;
const LAYER_MASK: u32 = 255u;
const TINT_SHIFT: u32 = 24u;
// Steps per climate axis; must match `TINT_LEVELS` in mesh.rs.
const TINT_LEVELS: u32 = 15u;
const LIGHT_MASK: u32 = 16383u; // (1 << UV_SHIFT) - 1

struct VertexOutput {
    @builtin(position) clip_position: vec4<f32>,
    @location(0) uv: vec2<f32>,
    @location(1) @interpolate(flat) tex_layer: u32,
    // How far this vertex is from the eye.
    //
    // A distance rather than the position it was worked out from: the
    // fragment shader wanted nothing else from it, and a `length` is a
    // square root that was being taken on every pixel of the screen to
    // recover a number three of its four neighbours already knew. The
    // interpolation is perspective-correct and the faces are a block
    // across, so what arrives is the same number to well within the
    // width of the fog ramp.
    @location(2) view_distance: f32,
    // sky, block, ao -- interpolated across the quad so AO reads smooth
    @location(3) light_terms: vec3<f32>,
    // How much sun this face catches. Constant across a quad -- it
    // depends only on which way the face points -- so it is worked out
    // once per vertex rather than per fragment, which also lets the
    // face index stop being carried at all.
    @location(4) @interpolate(flat) lambert: f32,
    @location(5) @interpolate(flat) translucent: u32,
    // Foliage colour for the climate this block grew in, or white.
    @location(6) @interpolate(flat) tint: vec3<f32>,
};

// What plant life looks like at the four corners of the climate square,
// as a multiplier over the texture's own colour.
//
// Multiplicative rather than a replacement: the textures are already
// green, and a tint that *is* the colour throws away everything the
// artist put in the image. These shift the hue and let the pixels keep
// their shape.
//
// The spread is deliberately wide, and the red channel carries most of
// it -- 0.45 in a swamp against 1.55 in dry steppe. A player never sees
// the whole climate square at once: the fields turn over about every
// seven hundred blocks, so a render distance of eight covers perhaps a
// third of the range. A palette whose corners are only just distinct
// reads as one colour from inside the world, however well it looks laid
// out side by side.
const TINT_COLD_DRY: vec3<f32> = vec3<f32>(0.80, 0.92, 0.72);  // tundra, washed out
const TINT_COLD_WET: vec3<f32> = vec3<f32>(0.55, 0.85, 0.62);  // taiga, dark and blue
const TINT_HOT_DRY: vec3<f32> = vec3<f32>(1.55, 1.15, 0.40);   // steppe, straw
const TINT_HOT_WET: vec3<f32> = vec3<f32>(0.45, 1.05, 0.32);   // swamp, deep green

// Unpacks the tint byte the mesher wrote. Zero means "not foliage",
// which is most of the world, and comes back as white.
fn foliage_tint(code: u32) -> vec3<f32> {
    if (code == 0u) {
        return vec3<f32>(1.0);
    }
    let index = code - 1u;
    let steps = f32(TINT_LEVELS - 1u);
    let temperature = f32(index / TINT_LEVELS) / steps;
    let humidity = f32(index % TINT_LEVELS) / steps;
    return mix(
        mix(TINT_COLD_DRY, TINT_HOT_DRY, temperature),
        mix(TINT_COLD_WET, TINT_HOT_WET, temperature),
        humidity,
    );
}

// Must match `mesh::TRANSLUCENT_BIT`.
const TRANSLUCENT_BIT: u32 = 8192u;
// Texels below this are thrown away rather than drawn. 0.5 is the usual
// choice: the leaf textures are 0 or 255, so anything in between only
// comes from filtering at the texel edges.
const ALPHA_CUTOFF: f32 = 0.5;
// How much of what is behind it a block of water lets through.
const WATER_ALPHA: f32 = 0.72;

// Texel-snapped texture coordinates.
//
// Anisotropic filtering is only legal in wgpu when *every* filter mode
// is linear, magnification included -- and linear magnification turns
// 16x16 pixel art into a smear the moment you stand next to a block.
//
// So the coordinate is fixed rather than the sampler. Inside a texel the
// UV is pulled to that texel's centre; only across the boundary between
// two texels is it allowed to ramp, and the ramp is exactly one pixel
// wide in screen space (`fwidth`). Under magnification that reproduces
// nearest-neighbour with an antialiased edge; under minification the
// ramp covers whole texels and the sampler does its ordinary filtered,
// mipmapped, anisotropic work -- which is where the shimmer this exists
// to remove actually is.
//
// `ramp` -- one screen pixel measured in texels -- is passed in rather
// than taken here, and that is not a matter of taste. It used to come
// from `fwidth`, which is a derivative, and a derivative may only be
// taken in uniform control flow. Handing it in lets the caller skip this
// whole function on the fragments that provably do not need it (see
// `sample_block`), which is most of the screen.
fn crisp_uv(uv: vec2<f32>, resolution: f32, ramp: vec2<f32>) -> vec2<f32> {
    if (resolution <= 0.0) {
        return uv;
    }
    let texel = uv * resolution;

    // The seam is the *boundary* between two texels -- an integer -- and
    // that is what everything is measured from.
    //
    // Getting this backwards is subtle and total. Measuring from the
    // texel *centre* (`floor(texel) + 0.5`) and pushing away from it
    // lands every sample on a boundary, where a linear filter returns a
    // 50/50 blend of the two texels either side. That is not a slightly
    // soft image, it is every texture in the game permanently blurred,
    // and it looks exactly like the filtering the snapping was supposed
    // to defeat.
    let seam = floor(texel + 0.5);
    let offset = texel - seam;

    // Pushed away from the seam to the nearest texel centre, except
    // within one screen pixel of the seam, where it ramps across. Under
    // magnification that is nearest-neighbour with an antialiased edge.
    let snapped = seam + clamp(offset / ramp, vec2<f32>(-0.5), vec2<f32>(0.5));
    return snapped / resolution;
}

// Samples a block texture with the snapped coordinate but the *original*
// gradients.
//
// This split is not optional, and getting it wrong is what made every
// texture in the game blurry and cost a large slice of the frame rate.
//
// `textureSample` derives the mip level from the derivative of whatever
// coordinate it is handed. The snapped coordinate is deliberately flat
// inside a texel and near-vertical at the boundary between two, so its
// derivative is nonsense: almost zero across most of a face and enormous
// along the seams. Fed to the mip selector that reads as "this fragment
// covers a huge area", so the hardware fetches from the smallest mip it
// has -- a 1x1 average of the whole texture. Hence the blur. And because
// neighbouring fragments then disagree wildly about which level to read,
// the texture cache misses on nearly every fetch, which is where the
// frame rate went.
//
// `textureSampleGrad` takes the gradients explicitly, so the mip level
// comes from the real UV while the fetch position comes from the snapped
// one. Crisp up close, correctly filtered at distance.
fn sample_block(uv: vec2<f32>, layer: u32) -> vec4<f32> {
    let resolution = globals.texture_params.x;

    // **Taken unconditionally, and that is the whole trick.**
    //
    // `textureSample` works out its own gradients from the pixel quad,
    // which the language only permits in uniform control flow -- so it
    // cannot be tucked inside the branch below. Hoisted up here it is
    // legal, and on every fragment that turns out to be minified it is
    // also the entire answer.
    //
    // That matters because the explicit-gradient form is not merely a
    // different spelling. Handing the gradients in by hand switches off
    // the hardware that derives them, and on this class of GPU that is
    // a materially slower instruction -- which the stage breakdown puts
    // at about two thirds of the frame, in a shader that measurement
    // shows is not limited by texture fetches at all (sixteen
    // anisotropic taps cost the same as one). The instruction was the
    // cost, not the taps, and it was being paid on every pixel of
    // terrain to serve the handful that are close enough to need it.
    let plain = textureSample(block_textures, block_sampler, uv, i32(layer));

    // Filtering off: the sampler is nearest and there are no mips in
    // play, so the coordinate needs no fixing. The branch is on a
    // uniform, so it costs nothing.
    if (resolution <= 0.0) {
        return plain;
    }

    let ddx = dpdx(uv);
    let ddy = dpdy(uv);

    // One screen pixel measured in texels, from the gradients that were
    // going to be taken anyway. Guarded because a face seen exactly
    // edge-on gives a derivative of zero.
    let ramp = max((abs(ddx) + abs(ddy)) * resolution, vec2<f32>(1e-5));

    // **Snapping is a magnification trick, and most of the screen is
    // minified.**
    //
    // Once a texel is smaller than a pixel the ramp spans whole texels,
    // the clamp stops clamping, and `crisp_uv` provably returns the
    // coordinate it was handed. An unsnapped coordinate sampled with its
    // own gradients is exactly what `textureSample` already computed --
    // the same position, the same mip, the same answer -- so the
    // expensive call has nothing left to contribute and the cheap one
    // taken above is returned instead.
    //
    // The branch is safe: `textureSampleGrad` is handed its gradients
    // explicitly and so is legal in non-uniform control flow, which is
    // precisely what `textureSample` would not have been.
    if (max(ramp.x, ramp.y) >= 1.0) {
        return plain;
    }
    let snapped = crisp_uv(uv, resolution, ramp);
    return textureSampleGrad(block_textures, block_sampler, snapped, i32(layer), ddx, ddy);
}

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
    let light = in.packed & LIGHT_MASK;

    out.clip_position = globals.view_proj * vec4<f32>(in.position, 1.0);
    out.uv = vec2<f32>(
        f32((in.packed >> UV_SHIFT) & 1u),
        f32((in.packed >> (UV_SHIFT + 1u)) & 1u),
    );
    out.tex_layer = (in.packed >> LAYER_SHIFT) & LAYER_MASK;
    out.view_distance = length(in.position - globals.camera_pos.xyz);
    out.translucent = light & TRANSLUCENT_BIT;
    out.tint = foliage_tint(in.packed >> TINT_SHIFT);

    // Half-lambert: a face turned away from the sun goes dim, not black,
    // which is what you want when the only other light source is
    // ambient.
    let normal = face_normal((light >> 10u) & 7u);
    out.lambert = max(dot(normal, -globals.sun.xyz), 0.0) * 0.65 + 0.35;

    let sky = f32(light & 15u) / 15.0;
    let block = f32((light >> 4u) & 15u) / 15.0;
    let ao = f32((light >> 8u) & 3u) / 3.0;
    out.light_terms = vec3<f32>(sky, block, ao);
    return out;
}

// Dropped items: a sprite given thickness, rather than a cube.
//
// Its own vertex format and so its own entry point, because the terrain
// vertex packs its texture coordinate into two bits -- block faces are
// mapped corner to corner and never need anything else -- and a sprite
// quad covers an arbitrary rectangle of its texture. Everything after
// this is shared: the output struct is the terrain's, so `fs_cutout`
// draws these with exactly the light, fog and cutout the world around
// them gets.
struct ItemVertexInput {
    @location(0) position: vec3<f32>,
    @location(1) uv: vec2<f32>,
    // Texture layer in the top half, the light word in the bottom.
    @location(2) packed: u32,
};

@vertex
fn vs_item(in: ItemVertexInput) -> VertexOutput {
    var out: VertexOutput;
    let light = in.packed & LIGHT_MASK;

    out.clip_position = globals.view_proj * vec4<f32>(in.position, 1.0);
    out.uv = in.uv;
    out.tex_layer = in.packed >> 16u;
    out.view_distance = length(in.position - globals.camera_pos.xyz);
    out.translucent = 0u;
    out.tint = vec3<f32>(1.0);

    let normal = face_normal((light >> 10u) & 7u);
    out.lambert = max(dot(normal, -globals.sun.xyz), 0.0) * 0.65 + 0.35;

    let sky = f32(light & 15u) / 15.0;
    let block = f32((light >> 4u) & 15u) / 15.0;
    let ao = f32((light >> 8u) & 3u) / 3.0;
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
    let sampled = sample_block(in.uv, in.tex_layer);
    return shade(in, sampled);
}

// Leaves. Same shading, plus the cutout -- and the early-Z cost, which
// is now paid only by the handful of chunks that contain a tree.
@fragment
fn fs_cutout(in: VertexOutput) -> @location(0) vec4<f32> {
    let sampled = sample_block(in.uv, in.tex_layer);

    // Leaf textures carry alpha-0 texels, and without this they were
    // drawn as whatever colour happened to sit under them -- a solid
    // block of it, since the pass does not blend. This is the whole of
    // the fix for "trees are cubes".
    if (sampled.a < ALPHA_CUTOFF) {
        discard;
    }
    return shade(in, sampled);
}

// The cracks on the block being mined.
//
// **This does not draw anything. It decides how much darker the block
// underneath gets**, and the pipeline multiplies rather than blends --
// so what ends up on screen is the block's own texture, its own light
// and its own fog, with the damage taken out of it. Nothing is laid over
// it.
//
// The difference is not subtle at the two moments that matter. Laid over
// the top, the cracks carried a brightness of their own: they had to, or
// they would be invisible on a block being mined in a cave, where the
// light on that face is zero. So they were drawn at full brightness --
// grey damage glowing faintly on a black wall, reading as a decal
// hanging in front of the block rather than as the block giving way. And
// on a bright sunlit face the same fixed grey was too weak to see.
// A multiplier has neither problem, because it has no colour of its own:
// it takes a share of whatever is already there, so the damage is as
// dark as the block is and no darker.
//
// `mix` rather than the sampled colour outright: the crack images are
// transparent everywhere except the damage, so alpha is what says
// "there is a crack here" and the undamaged texels have to come out as
// 1.0 -- multiplying by a transparent black texel would paint the face
// solid black.
@fragment
fn fs_crack(in: VertexOutput) -> @location(0) vec4<f32> {
    let sampled = sample_block(in.uv, in.tex_layer);
    return vec4<f32>(mix(vec3<f32>(1.0), sampled.rgb, sampled.a), 1.0);
}

fn shade(in: VertexOutput, sampled: vec4<f32>) -> vec4<f32> {

    let sky_level = in.light_terms.x;
    let block_level = in.light_terms.y;
    let ao = in.light_terms.z;

    let sun_term = sky_level * globals.sun.w * in.lambert;
    let block_term = block_level * globals.extra.x;

    var light = max(sun_term, block_term) + globals.fog_params.z;
    // Ambient occlusion darkens creases; strength is configurable so it
    // can be dialled down without touching the mesher.
    let ao_factor = mix(1.0 - globals.extra.y, 1.0, ao);
    light = clamp(light * ao_factor, 0.0, 1.4);

    // Tinted in proportion to how green the texel already is.
    //
    // This is what lets the *side* of a grass block take the colour: it
    // is one image of turf over dirt, and tinting the whole of it turns
    // the exposed earth savanna-yellow, which reads as a bug. Greenness
    // separates the two without a second texture, a second layer or a
    // bit in the vertex to say which is which -- the picture already
    // knows. For a leaf or a blade, every texel is green and the whole
    // thing is tinted; for soil the term is zero and nothing happens.
    //
    // Free for everything that is not foliage: the tint is white there,
    // so the mix has nothing to do either way.
    // Branching past this when the tint is white -- which is most of
    // the world -- was tried and measured: `solid` 0.402 ms against
    // 0.397, which is the noise. The same answer the sine-free hash and
    // the anisotropy gave, and for the same reason: this shader is not
    // waiting on its arithmetic.
    let greenness = clamp((sampled.g - max(sampled.r, sampled.b)) * 4.0, 0.0, 1.0);
    var color = sampled.rgb * mix(vec3<f32>(1.0), in.tint, greenness) * light;
    let distance = in.view_distance;

    // Under water, before fog and independently of it.
    //
    // This used to ride entirely on the fog, which meant pressing F --
    // the fog toggle -- made being submerged look exactly like being in
    // open air. Water is not haze: it absorbs red first and keeps
    // absorbing with depth, and that is a property of the medium rather
    // than a distance cue the player may switch off.
    if (globals.extra.z > 0.5) {
        // Beer-Lambert per channel, red soaked up fastest. Even at zero
        // distance there is water between the eye and everything, so
        // there is a floor under the absorption.
        let absorb = vec3<f32>(0.42, 0.11, 0.06);
        let depth = distance + 1.5;
        color *= exp(-absorb * depth);
        // ...and everything trends toward the colour of the water rather
        // than to black, or deep water reads as a cave.
        let murk = clamp(depth / 26.0, 0.0, 0.85);
        color = mix(color, globals.fog_color.rgb * 0.55, murk);
    }

    if (globals.extra.w > 0.5) {
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
