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
    // The four fields between here and the one this shader wants are
    // declared and unread. A uniform block is an *offset table*: leaving
    // one out does not skip it, it shifts everything after it. See the
    // `every_shader_reads_the_same_globals` test.
    inv_view_proj: mat4x4<f32>,
    sky_params: vec4<f32>,
    render_origin: vec4<f32>,
    hand_view_proj: mat4x4<f32>,
    // x: the first layer of the animated run, y: how many frames,
    // z: frames per second, w: spare. See `animated`.
    anim: vec4<f32>,
    // The colour of the direct light, and of the sky that fills its
    // shadows. Both at luminance one, so they carry hue and nothing
    // else -- see `shade`.
    sun_color: vec4<f32>,
    fill_color: vec4<f32>,
    // The sun's shadow map, read only by the `_shadowed` entry points
    // at the end of this file. Appended, for the reason `anim` gives.
    // See `engine::shadow`.
    shadow_view_proj: mat4x4<f32>,
    // x: how much of the direct light a shadow takes, y..z: the fade
    // toward the map's edge in blocks, w: tap spacing in map uv
    shadow_params: vec4<f32>,
    // x: normal offset in blocks, y: depth bias in map depth, z: the leaf
    // push in map depth, w: 1 for hard shadows and 0 for soft (`shadow::Mode`)
    shadow_bias: vec4<f32>,
    // The sunset toward the sun -- xyz its colour, w how strongly it
    // replaces the fog colour -- and the halo round the sun, xyz already
    // scaled by its strength. Zero at the Simple step. See `glow_over`.
    // Appended, for the reason `anim` gives.
    horizon_glow: vec4<f32>,
    sun_haze: vec4<f32>,
    // xz: the sun's compass bearing, flat and unit length, worked out once
    // a frame so no pixel has to. See `glow_lobe`.
    glow_dir: vec4<f32>,
    // The moon: xyz the direction its light travels, w how much the night's
    // floor under open sky is scaled by, less one (nought by day). See
    // `Sky::moon_uniform`. Appended, for the reason `anim` gives.
    moon: vec4<f32>,
    // x: how much of the weather's rain has reached the ground, 0..1 --
    // what `WET_DARKEN` below is scaled by. Appended, for the reason `anim`
    // gives; the other shaders that declare this block declare a prefix of
    // it and are untouched by a field on the end.
    weather: vec4<f32>,
};

// **Which lighting step this module was compiled for**: 0 Simple, 1
// Balanced, 2 High. Rewritten by `engine::lighting::Quality::specialise`
// before the source is compiled, so every `if (LIGHTING >= 1u)` below is a
// branch on a constant the compiler removes, and the Simple step is this
// file with some dead code in it. The line is matched exactly -- see
// `every_shader_carries_the_lighting_switch_exactly_once` before touching
// it.
const LIGHTING: u32 = 0u;

/// The layer to actually sample, given the one the vertex named.
///
/// **The only animation in the game, and it costs a compare.** A mesh is
/// built once and its vertices carry a fixed texture layer, so nothing
/// drawn out of a chunk can change picture -- unless the *choice of
/// picture* is made here instead. The fire's frames are a contiguous run
/// in the array (see `texture::EXTRA_FLAME`), and a layer that falls
/// inside that run is advanced by the clock.
///
/// Everything else in the world tests one comparison against a number it
/// will fail, which is free next to the texture fetch that follows.
fn animated(layer: u32) -> u32 {
    let first = u32(globals.anim.x);
    let frames = u32(globals.anim.y);
    let total = u32(globals.anim.w);
    if (frames < 2u || layer < first || layer >= first + total) {
        return layer;
    }
    // **Which sheet, then which frame of it.** The run holds more than
    // one animation back to back -- the fire is two sheets of four, one
    // per crossing quad -- so the layer names both, and advancing it
    // means advancing *within* its own sheet. Dividing finds the sheet;
    // the remainder is the frame it happens to be showing.
    //
    // `sky_params.z` is seconds since the client started -- a clock that
    // only counts up, unlike the time of day, which wraps at midnight
    // and would make the fire stutter once a day.
    let sheet = (layer - first) / frames;
    let step = u32(globals.sky_params.z * globals.anim.z) % frames;
    return first + sheet * frames + step;
}

@group(0) @binding(0)
var<uniform> globals: Globals;

@group(1) @binding(0)
var block_textures: texture_2d_array<f32>;
@group(1) @binding(1)
var block_sampler: sampler;

// Bound only to the shadowed pipelines. An entry point that does not
// name these does not need them in its layout, which is what lets the
// ordinary terrain pipelines stay exactly as they were.
@group(2) @binding(0)
var shadow_map: texture_depth_2d;
@group(2) @binding(1)
var shadow_sampler: sampler_comparison;
// The fires' shadows, bound with the sun's map and read by the same entry
// points (see `engine::lamp_shadow`): which cells round the player stop
// light, a byte a cell, and the fires nearest the eye.
@group(2) @binding(2)
var lamp_cells: texture_3d<f32>;
@group(2) @binding(3)
var<uniform> lamps: Lamps;
// The row over the highest thing that casts in each column of the volume, a
// byte a column: what lets a ray toward the sun pass a whole column it is
// above without looking at a cell of it. See `sun_ray`.
@group(2) @binding(4)
var lamp_heights: texture_2d<f32>;

// `lamp_shadow::LampUniform`, byte for byte.
struct Lamps {
    // xyz: the volume's lowest corner relative to the render origin,
    // w: how many of `lamps` are filled
    volume: vec4<f32>,
    // x: 1 when a volume has been written, y: the row over the highest thing
    // in it that casts, counted from the corner
    grid: vec4<f32>,
    // xyz: the middle of a fire's cell relative to the render origin,
    // w: the level it gives out
    lamps: array<vec4<f32>, 16>,
};

// Everything but the position rides in one word. See `mesh::Vertex` for
// the layout and for why it is worth packing at all.
struct VertexInput {
    @location(0) position: vec3<f32>,
    // Where this vertex's chunk is, relative to the point the frame is
    // drawn around. One value per draw rather than per vertex -- see
    // `mesh::Vertex::instance_layout`, and note that this is the whole
    // reason `position` is a number between -1 and 17 instead of the
    // seven-digit one it used to be.
    @location(2) chunk_offset: vec4<f32>,
    @location(1) packed: u32,
    /// Where this corner sits on the texture, counted in **cells rather
    /// than corners**.
    ///
    /// It used to be two bits inside `packed`: a face was one block
    /// across, so a corner was either 0 or 1 on each axis and two bits
    /// said everything there was to say. A merged rectangle spans
    /// several cells and has to tile the picture across them, which
    /// needs five bits an axis and does not fit. See `Vertex::uv` in
    /// `mesh.rs`; the vertex grew four bytes to carry it.
    @location(3) uv_cells: u32,
};

// Must match `LAYER_SHIFT` and `TINT_SHIFT` in mesh.rs.
const LAYER_SHIFT: u32 = 16u;
const LAYER_MASK: u32 = 255u;
// The layer's ninth bit, on its own below the other eight: the field ran
// out at 256 layers and the atlas reached it. Must match
// `LAYER_HIGH_SHIFT` in mesh.rs -- a shader reading eight bits of a
// nine-bit layer dresses every picture past the 256th in the picture 256
// before it, which looks like a mistake in blocks.toml and is not.
const LAYER_HIGH_SHIFT: u32 = 15u;
// The tenth and eleventh, in the other word (`uv_cells`): nine bits ran out
// at 512 and the atlas reached 522. Must match `LAYER_TOP_SHIFT` in mesh.rs.
// Which *array* a layer is in is not decided here -- the layer stays one
// number, and `texture::AtlasSplit::specialise` splits it at the fetch.
const LAYER_TOP_SHIFT: u32 = 29u;
const TINT_SHIFT: u32 = 24u;
// Steps per climate axis; must match `TINT_LEVELS` in mesh.rs.
const TINT_LEVELS: u32 = 15u;
// Where a surface tint starts -- soot, ash on a furrow; must match
// `SURFACE_TINT_BASE` in mesh.rs, which says what they are.
const SURFACE_TINT_BASE: u32 = 226u;
// A quad lying flush on a face drawn under it -- leaf litter -- is pulled
// toward the eye by `DECAL_DEPTH` and wears no tint. Must match `DECAL_TINT`
// in mesh.rs, which weighs it against a lift and a pipeline bias.
const DECAL_TINT: u32 = 255u;
// How far, as a share of the depth range: sixteen steps of a 32-bit float
// buffer where the ground is (depth past a half, where a step is 2^-24) --
// and more the more edge-on the eye sees the face, which is the polygon
// offset's slope term done by hand: the error two triangles of one plane
// disagree by grows with how fast depth changes across a pixel, and for a
// floor that is the camera's height over it, whatever the distance (depth
// per pixel is near * pixel angle / height). Measured by `litter_repro`
// (`how_litter_lies_on_the_ground`): with no nudge the turf came through
// half the litter from every eye; with the constant alone, none from a
// standing player's eyes (1.62 over the floor) or higher, and a fifth of the
// litter a lying body sees an arm's length off (0.25 over it); with the
// slope term as well, none from any eye the tool looks from. Sixteen steps
// are a hundredth of a block at ten blocks away and a fifth of one at a
// hundred -- a sliver of litter a pixel high showing through the foot of a
// trunk, where a lift was a sheet hovering over the whole floor.
const DECAL_DEPTH: f32 = 9.5e-7;
const DECAL_SLOPE: f32 = 6.0e-7;
// The light word is still the bottom fourteen bits of `packed`. Above it
// sit the block-face flag and the layer's ninth bit -- see `uv_cells`,
// which took the texture coordinate out of this word and freed them.
const LIGHT_MASK: u32 = 16383u;
// Must match `V_SHIFT` and `UV_MASK` in mesh.rs.
const V_SHIFT: u32 = 5u;
const UV_MASK: u32 = 31u;
// A coordinate that is a place in the picture rather than a count of
// cells: the top bit set, `u` in the fourteen bits below `FINE_V_SHIFT` and
// `v` in the fourteen above them, in 256ths of a picture. What a model's small faces and a
// part-height block's sides wear, so they are cut from the picture rather
// than squeezed. Must match `FINE_UV_BIT`, `FINE_V_SHIFT` and `FINE_UNITS`
// in mesh.rs, where the argument is.
const FINE_UV_BIT: u32 = 2147483648u;
const FINE_V_SHIFT: u32 = 14u;
const FINE_MASK: u32 = 16383u;
const FINE_UNITS: f32 = 256.0;
struct VertexOutput {
    @builtin(position) clip_position: vec4<f32>,
    // **Centroid, because the frame is multisampled.** With several
    // samples per pixel a pixel on the edge of a face is covered by
    // some of them and not others, and the fragment shader still runs
    // once, at the pixel *centre* -- which for such a pixel lies outside
    // the face. A plain interpolant is then extrapolated past the edge:
    // at the top edge of a flower's crossed quads `v` came out a little
    // below zero, the block sampler wraps (`BLOCK_ADDRESS_MODE`), and
    // what was fetched was the *bottom* row of the picture -- the stem
    // and the soil, opaque -- painted along the top edge of every plant
    // as a bright hairline, a small cross hanging over each flower.
    // `centroid` moves the evaluation point to a covered sample, which is
    // always inside the face, so the coordinate never leaves the quad.
    // At one sample it is the pixel centre as before and changes nothing.
    @location(0) @interpolate(perspective, centroid) uv: vec2<f32>,
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
    // **Where on the block grid this fragment is**, measured from the
    // frame's origin and stepped half a block into the face -- which
    // is to say, the cell whose shade this fragment wears. See
    // `BLOCK_VARIATION`.
    //
    // Interpolated, and it used to be the finished colour with `flat`
    // on it. That was correct only for as long as one quad meant one
    // block. A merged rectangle stands for as many as thirty-one of
    // them (see `mesh::MERGE_COPLANAR_FACES`), and a flat value takes
    // the provoking vertex's cell for the whole of it -- so an open
    // meadow came out as a chequerboard of sixteen-block squares, each
    // a few percent lighter or darker than the next, and a mine's flat
    // walls grew pale patches where one rectangle met another. The
    // shade has to be worked out where the block is known, which is
    // here, per fragment.
    //
    // Relative to the render origin rather than absolute, for the same
    // reason the position is: an absolute world coordinate is an f32
    // holding seven digits, and interpolating it quantises. The origin
    // is a whole number of blocks, so `floor` and the addition commute
    // and the pattern still stays welded to the world.
    @location(7) shade_cell: vec3<f32>,
    // Whether this fragment is a block face at all -- see
    // `mesh::MOTTLED_BIT`. An animal or a dropped item is one object
    // and gets no mottling.
    @location(8) @interpolate(flat) mottled: u32,
    // How many cells of water stand under this fragment, or zero where
    // there is none. See `WATER_DEPTH_FADE`.
    //
    // Flat, because it is a property of the cell rather than of the
    // corner: one water face is one block, liquids being the one thing
    // the mesher never merges.
    @location(9) @interpolate(flat) water_depth: f32,
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
    // A surface tint comes back *negative*, which is how the fragment
    // shader tells it from a climate: a climate is weighed by how green a
    // texel is, and soot lies on all of it. Three stages of soot, each a
    // step darker and a touch warmer (the brown of old smoke), and ash on
    // a furrow, which greys the earth rather than darkening it.
    if (code >= SURFACE_TINT_BASE) {
        let stage = code - SURFACE_TINT_BASE;
        if (stage == 4u) {
            return -vec3<f32>(1.10, 1.08, 1.06);
        }
        // Moss (`MOSS_TINT`, stage 9): a deep yellow-green that keeps the
        // stone's grain -- dark where the rock is dark -- and never as dark as
        // soot, so a mossy wall and a sooted one are not one wall.
        if (stage == 9u) {
            return -vec3<f32>(0.62, 0.86, 0.42);
        }
        // A tired furrow (`TIRED_FURROW_TINT`, stage 10): the earth paled
        // and dried towards straw. Before the weather's arm, which would
        // otherwise draw it as a rotten board.
        if (stage == 10u) {
            return -vec3<f32>(1.12, 1.04, 0.84);
        }
        // A board's years in the rain (`weathering`), stages 5..8: the
        // brown goes out of it first -- grey, a cold multiply that takes
        // more red than blue -- then it darkens, and rotten is dark and a
        // little green. Never as black as soot, so a sooted ceiling and a
        // rotten roof are not the same board at a glance.
        if (stage >= 5u) {
            let years = f32(min(stage, 8u) - 4u);
            let dark = 1.0 - 0.13 * years;
            let green = select(0.0, 0.05, stage >= 8u);
            return -vec3<f32>(dark * 0.84, dark * (0.90 + green), dark * 1.0);
        }
        let dark = 1.0 - 0.22 * f32(min(stage, 3u));
        return -vec3<f32>(dark * 1.02, dark, dark * 0.96);
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

// How the light budget is split between the beam and the sky.
//
// They sum to one, and both colours arrive at luminance one, so this
// pair changes *what colour* a lit face is and not how bright it is:
// turning coloured light on moved the exposure of the game by nothing.
//
// Sixteen per cent is a real sky and not a stylistic one. It is enough
// that a shaded face is visibly cooler than a lit one -- which is the
// relation the eye uses to read a surface as a surface -- and little
// enough that a wall in shadow is still obviously in shadow.
const SUN_SHARE: f32 = 0.84;
const SKY_SHARE: f32 = 0.16;

// What a torch, a hearth or a seam of glowstone puts out.
//
// Warm, and it has to be: it was white, and white fire is the other
// half of the plastic problem. Every cave in the game was lit by a
// colourless lamp, so stone underground came out as the same grey it is
// in the texture -- and nothing in the world ever told the eye what
// kind of light it was standing in.
const FIRE_COLOR: vec3<f32> = vec3<f32>(1.35, 0.97, 0.57);

// The floor under everything, so nothing is ever absolutely black.
//
// Faintly cool rather than neutral. It stands for light that has
// bounced its way somewhere the sky cannot reach, and a dead-grey floor
// under a warm torch is the one place a cave still looked moulded.
const AMBIENT_COLOR: vec3<f32> = vec3<f32>(0.90, 0.94, 1.06);

// **Fire, past the Simple step**: a deeper orange at the same luminance
// (1.01 against 1.02), so a torch lights exactly as far and as brightly
// and only its colour moves. Against the cooler shade that step gives
// daylight, the old lamp read as a pale yellow bulb; this is flame.
const FIRE_COLOR_WARM: vec3<f32> = vec3<f32>(1.48, 0.93, 0.46);

// **The floor under open sky at night, past the Simple step.** Moonlit
// blue at the luminance `AMBIENT_COLOR` has (0.93 against 0.94), so the
// night is exactly as dark as it was -- which was asked for twice -- and
// reads as night rather than as an unlit grey room.
//
// Mixed in by the fragment's sky light, and that is what keeps a cave a
// cave: rock with no sky over it keeps `AMBIENT_COLOR`, and the blue only
// reaches ground the moon could.
const MOONLIT_FLOOR: vec3<f32> = vec3<f32>(0.76, 0.95, 1.30);

// How much of the half-Lambert floor the sky's colour takes past the
// Simple step. See the note in `shade_lit` for why it is not all of it.
const SKY_OF_FLOOR: f32 = 0.7;

// ---- the horizon, shared with sky.wgsl ----
//
// **Both shaders carry this block, character for character**, and
// `the_sky_and_the_terrain_draw_the_same_horizon` in `engine::lighting`
// compares the two copies. The terrain fades into this colour and the sky
// is this colour at its horizon; if the two ever computed it differently
// the edge of the world would show as a line -- and toward a sunset,
// where the colour is changing fastest, as a bright one.

// How fast the sunset fades going up the sky: `1 / (1 + GLOW_RISE * up)^2`
// of the sine of the height above the horizon, so it is a third as strong
// fourteen degrees up and an eighth by thirty-five. A squared rational
// rather than the `exp` it replaced, which drew nearly the same curve for
// a transcendental function on every pixel of fog and sky.
const GLOW_RISE: f32 = 3.0;

// How much of the sunset stands in direction `dir`, 0..1: a lobe round the
// sun's compass bearing, fading upward. Wide on purpose -- still a third
// at sixty degrees off -- because a sunset lights a quarter of the sky,
// not a disc of it.
//
// **`dir` need not be normalised, and the bearing arrives flat and unit
// length** (`glow_dir.xz`, once a frame on the CPU). The first version
// normalised both here -- two square roots a pixel, one of them of a
// number that was the same for every pixel of the frame -- and Balanced
// measured 0.7 ms dearer than Simple in the view with the most fragments.
fn glow_lobe(dir: vec3<f32>) -> f32 {
    let across = dot(dir.xz, dir.xz) + 1e-8;
    let facing = dot(dir.xz, globals.glow_dir.xz) * inverseSqrt(across) * 0.5 + 0.5;
    let wide = facing * facing;
    let up = max(dir.y, 0.0) * inverseSqrt(across + dir.y * dir.y);
    let rise = 1.0 / (1.0 + GLOW_RISE * up);
    return globals.horizon_glow.w * wide * wide * rise * rise;
}

// `base` -- the fog colour, or the sky -- as it looks in direction `dir`
// once the sunset and the sun's halo are put in. `dir` need not be
// normalised. Returns `base` untouched at the Simple step.
fn glow_over(base: vec3<f32>, dir: vec3<f32>) -> vec3<f32> {
    var colour = base;
    if (LIGHTING >= 1u) {
        colour = mix(colour, globals.horizon_glow.rgb, glow_lobe(dir));
    }
    if (LIGHTING >= 2u) {
        // Two powers of the angle to the sun rather than one `pow`: a
        // wide soft skirt and a tighter bright core, the shape a real halo
        // has, from multiplies alone.
        let c = max(dot(dir, -globals.sun.xyz), 0.0) * inverseSqrt(dot(dir, dir) + 1e-8);
        let c2 = c * c;
        let c4 = c2 * c2;
        let c16 = c4 * c4 * c4 * c4;
        colour = colour + globals.sun_haze.rgb * (c4 * 0.45 + c16 * 0.55);
    }
    return colour;
}
// ---- end of the shared horizon ----

// **The shoulder**, past the Simple step: where the brightest channel
// passes `KNEE`, the colour is scaled down along its own hue toward a
// ceiling a hair over one.
//
// What it is for is the warm light. A sunlit face of sand under a golden
// key comes out around `(1.15, 0.97, 0.66)`, and the screen clips the red
// at one: what is drawn is *less* orange than what was lit, and the
// warmest surfaces in the world were the ones the warmth was taken out of.
// Scaling all three channels by the one ratio keeps the hue.
//
// Rejected: a filmic curve over the whole range (Reinhard, ACES). Both
// darken the middle of the picture to make room at the top, and this
// game's exposure is decided by numbers people argued over -- the night,
// the ambient floor -- which a curve under all of them would quietly move.
// Below the knee this is the identity, so none of them moves.
//
// A rational shoulder, `over * ROOM / (over + ROOM)`, which leaves the knee
// at the same slope of one and approaches the same kind of ceiling: the
// `exp` it replaced was a transcendental function on every bright pixel
// of a noon savanna, which is most of them.
const KNEE: f32 = 0.9;
const SHOULDER_ROOM: f32 = 0.15;

// ---- four ways a surface stops being a painted one ----
//
// **What "пластиково" names is not one fault.** The light in this file is
// already a warm key against a cool sky fill, with occlusion, a moonlit
// floor and a sunset in the fog. What was left is that *every material in
// the world behaved identically*: a perfect diffuser, returning the same
// colour in every direction, dry in a downpour, opaque from behind. Four
// small terms, each of them something one real material does and no other --
// water mirrors, wet ground darkens, a leaf passes light, and a thing that
// is not the world takes its ambient from the sky above it.
//
// **Every one is zero-able**, and `look_repro` compiles a copy with all four
// at nought to photograph the same seat before and after. They are constants
// and not settings on purpose: a player asked for the game not to look like
// plastic, not for a menu about it.

// **How much of the sky a water surface mirrors**, over Fresnel.
//
// Water is the one material in this game whose look the eye knows without
// being told, and it was a perfectly matte blue sheet -- which is a painted
// floor, exactly. Looked into from above water is a window and keeps its
// bed; looked along it is a mirror, and the Fresnel curve below carries the
// whole of that shape.
//
// **A ceiling, not a strength, and it is there because a sea has waves.**
// Schlick over a flat surface goes to one at the horizon, and a mirror of a
// noon sky is white: photographed on the beach of seed 32 at midday, the far
// half of the sea came out the colour of the haze over it and the water
// stopped being water. A real sea is rough at every scale, and the roughness
// is what stops the grazing reflection ever getting there -- every facet
// points somewhere else. Five and a half tenths leaves the sea its own blue
// at any angle, and it is still most of the picture where it matters.
const WATER_SHEEN: f32 = 0.55;
// What water gives back at normal incidence -- the real number, and it
// matters: at nought a pond looked straight down into goes dead, and this is
// the faint sheen a still puddle has from directly above.
const WATER_F0: f32 = 0.02;
// **The sun's own glitter on it**, in a tight lobe round the mirror
// direction. Bright, because a specular highlight is brighter than the
// surface it sits on, and narrow -- the forty-eighth power -- so a low sun
// lays a path across a lake rather than a flare over half of it. At the
// twenty-fourth, which is where this started, the path was a soft white
// patch a third of the frame wide and read as a bloom rather than as the
// sun on water.
const WATER_GLITTER: f32 = 0.75;

// **What rain leaves on what it falls on**, as a multiplier over the albedo.
//
// A wet surface is much darker than the same surface dry: the film on it
// lets light in and gives less of it back. Three tenths is the order of it
// for soil and stone, and it is the answer a storm needs -- the light going
// grey while the ground does not change at all is the moment a world reads
// as painted rather than lit.
const WET_DARKEN: f32 = 0.70;

// **How brightly a leaf lit from behind glows.**
//
// A blade of grass and a birch leaf are thin enough that the sun goes
// through them, and the far side comes out the leaf's own colour at several
// times the brightness of the same leaf lit from the front. It is the
// loudest thing a real canopy does; without it a tree against a low sun is a
// green cut-out. Scaled by how green the texel already is, so this happens
// to foliage and costs stone a multiply by nought.
const LEAF_GLOW: f32 = 0.55;

// **How much of a model's ambient comes from the sky rather than from every
// direction at once.** See `model_openness`.
const MODEL_SKY_FILL: f32 = 0.7;

// **How much colour the air between the eye and a surface takes out of it**,
// at the far end of that curve.
//
// Photographed at noon over the forest at spawn, the trees a hundred blocks
// away were exactly as saturated and exactly as contrasty as the trunk an arm
// from the camera -- so the whole picture sat on one plane and read as a
// printed backdrop. That is the largest single thing the word "пластиково"
// was pointing at in the frame it was reported from, and it is not a
// lighting fault: the light was fine and the *air* was missing.
//
// **This is not the fog, and it must not become it.** The fog begins at three
// quarters of the render distance and finishes on the sky's exact colour, so
// the edge of the loaded world dissolves; that number is the player's and two
// other things agree with it. What air does is different and continuous: it
// scatters its own light in over the whole distance, and what the eye reads
// from it is that far things are *less saturated*, not that they are hidden.
// So this takes saturation only -- toward the sky's own hue at the surface's
// own brightness -- and it saturates at a third, which is depth rather than
// haze. A surface a hundred blocks off keeps three quarters of its colour and
// the fog, when it arrives, still finds the same picture it used to.
//
// **In blocks, not in a share of the render distance.** Air does not know how
// far a machine can draw, and a player who turns the distance up would
// otherwise find the middle of their world going flat again.
const AIR_DEPTH: f32 = 0.34;
// Where half of it has happened. Fifty-five blocks is about where a real
// hillside starts visibly losing its green, and it is inside the render
// distance of every machine this runs on -- a curve whose knee is past the
// far plane is a constant.
const AIR_HALF: f32 = 55.0;

fn shoulder(c: vec3<f32>) -> vec3<f32> {
    let peak = max(max(c.r, c.g), c.b);
    if (peak <= KNEE) {
        return c;
    }
    let over = peak - KNEE;
    let squeezed = KNEE + over * SHOULDER_ROOM / (over + SHOULDER_ROOM);
    return c * (squeezed / peak);
}

// How much a block's own shade varies from its neighbours', either way.
//
// **The second half of the fix.** A wall of stone is one image repeated
// a hundred times, and a hundred identical copies of anything read as a
// printed sheet rather than as a hundred rocks -- however good the
// image is. Real materials vary between one piece and the next, and the
// eye is extremely good at noticing when they do not.
//
// Two axes, because one was not enough and the wrong one. Brightness
// alone gives a wall of the same colour at slightly different volumes,
// which is the plastic problem in miniature; what a heap of real stone
// varies in is *temperature* -- some of it warmer, some cooler -- and
// that is the half the eye actually reads.
//
// The stone texture in this game is four greys with `r == g == b` (79,
// 67, 92, 85) and the grass is eight levels of one saturated green. Neither
// has a hue to vary on its own, so this is where it comes from.
//
// Deliberately small. At two per cent it does nothing; past about eight
// the wall goes blotchy and the variation becomes the pattern.
// **The torch in the player's own hand**, in the same light levels the
// chunk map is baked in.
//
// It is the one light in this game that is not a cell of the world.
// Every other lamp pours into the chunk light map, which is baked per
// block and re-flooded when a block changes -- exactly right for a lamp
// that stays where it is put, and useless for one that walks. A carried
// light cannot live there at all: it has no cell, it moves every frame,
// and re-flooding the map at sixty hertz around a walking player would
// be the whole lighting engine run as a per-frame cost.
//
// So it is not lit *from* anywhere: it falls off with the distance from
// the camera, which is already on this vertex for the fog. That is a lie
// about geometry -- a wall between the player and a corner does not
// shadow it -- and it is the right lie: what a torch actually does for a
// player is show them the few blocks they are standing among, and those
// are the blocks with nothing between.
//
// **One level per block, which is the rule the light map itself
// propagates by**, so a torch of emission twelve carried in the hand
// reaches exactly as far as the same torch would if it could be set in
// the wall. The first draft was an inverse square with the emission as
// a fraction, and it was wrong by a factor of four at arm's length and
// by everything at six blocks: photographed at night, the ground under
// the player came out eight per cent brighter than with no torch at
// all. A light nobody can see is not a light.
//
// `texture_params.z` is the emission, in levels, and it is zero
// whenever nothing is alight -- which is nearly always, and this then
// costs one subtract and one clamp.
fn carried_level(view_distance: f32) -> f32 {
    return max(globals.texture_params.z - view_distance, 0.0) / LIGHT_LEVELS;
}

/// How many steps of light there are, matching `types::MAX_LIGHT`.
const LIGHT_LEVELS: f32 = 15.0;

const BLOCK_VARIATION: f32 = 0.055;
/// ...and how far the warm/cool drift goes, which is the half that
/// stops it being a brightness knob.
const BLOCK_TEMPERATURE: f32 = 0.035;

// A hash of a block's position, in 0..1.
//
// Integer arithmetic rather than the usual `fract(sin(dot(...)))`: that
// idiom depends on the precision of `sin` at huge arguments, which is
// not specified and differs between drivers -- so the same wall would
// be mottled differently on two machines, and on some of them would
// band visibly.
fn block_hash(cell: vec3<i32>) -> f32 {
    var h = u32(cell.x) * 73856093u ^ u32(cell.y) * 19349663u ^ u32(cell.z) * 83492791u;
    h = h ^ (h >> 13u);
    h = h * 1274126177u;
    h = h ^ (h >> 16u);
    return f32(h & 0xffffffu) / f32(0xffffffu);
}

// Must match `mesh::TRANSLUCENT_BIT`.
const TRANSLUCENT_BIT: u32 = 8192u;
// Must match `mesh::MOTTLED_BIT`.
const MOTTLED_BIT: u32 = 16384u;
// **This face looks up**, worked out in the vertex stage from the normal it
// already builds and carried in the spare bottom of the `mottled` slot.
//
// Not from mesh.rs: that word arrives holding `MOTTLED_BIT` and nothing below
// it, and a flag the fragment stage wants and the vertex stage can answer for
// free has no business costing a bit in the vertex format. What wants it is
// the water sheen, which is a property of a horizontal surface and would be
// nonsense on the cut side of a column.
//
// **Why not the depth byte, which was tried.** Only the top face carries a
// depth past one, so `water_depth > 1.0` looked like a test for "is the lid"
// -- and it is the wrong way round: a *side* also carries one, and so does
// the lid of every pond and every flooded footprint a block deep. On the
// beach of seed 32 that condition put the sheen on the open sea and left it
// off every inch of the shore, which is the water a player is actually
// standing in.
const UPWARD_BIT: u32 = 1u;
// **This fragment is foliage**, set beside it and for the same reason: the
// leaf glow needs to know, and greenness alone does not say so.
//
// A skin, a garment and a dropped stack all come through `shade_lit` with a
// white tint and a `shade_cell` of nought -- the models have no cell, they
// never needed one -- so a green shirt would have been lit from behind by a
// ray computed from the origin of the frame. Which is a lamp nobody can see,
// and the last time this game had one of those it was a bug report.
const FOLIAGE_BIT: u32 = 2u;
// Must match `mesh::CHIPPED_BIT`: the cut face of a part-dug block. Only
// ever set beside `FINE_UV_BIT`, and handed to the fragment in the
// `mottled` slot beside `MOTTLED_BIT` -- see `chip_shade`.
const CHIPPED_BIT: u32 = 268435456u;
// **The chip picture**, a row a number and two bits a texel from the left:
// 0 the stone as it was, 1 a groove, 2 a fresh edge, 3 a pit. Drawn in
// `mesh::CHIP_MARKS`, which is the copy to edit; a test holds this one to it.
//
// A private variable rather than a `const` array because it is indexed by
// the texel under the fragment, and a runtime index into a value array is
// what naga turned away in the shaders this was tried in.
var<private> CHIP_ROWS: array<u32, 16> = array<u32, 16>(
    134217856u,
    100860000u,
    150995520u,
    67109120u,
    402654720u,
    268960780u,
    537273344u,
    69632u,
    50487328u,
    16408u,
    134217872u,
    101449792u,
    150995328u,
    16908544u,
    33655040u,
    17408u
);
// What each code does to the light: a groove is in its own shadow, a fresh
// edge is paler than the weathered face round it, and a pit is the darkest.
// Chosen on the pictures themselves, the marks laid over every rock and
// soil at these numbers: at a quarter darker they vanished into a speckled
// stone like gneiss, and much past a third darker the face stops being the
// same rock with marks on it and becomes a different, darker block.
const CHIP_GROOVE: f32 = 0.68;
const CHIP_EDGE: f32 = 1.16;
const CHIP_PIT: f32 = 0.56;

// How the light on a fresh-cut face is changed by the chip picture at `uv`:
// the texel of `CHIP_ROWS` under the fragment, on the same sixteen-texel grid
// as the stone's own picture, so a groove is one texel of the stone and not
// a line drawn across it.
fn chip_shade(uv: vec2<f32>) -> f32 {
    let texel = vec2<u32>(clamp(floor(fract(uv) * 16.0), vec2<f32>(0.0), vec2<f32>(15.0)));
    let code = (CHIP_ROWS[texel.y] >> (texel.x * 2u)) & 3u;
    if (code == 1u) {
        return CHIP_GROOVE;
    }
    if (code == 2u) {
        return CHIP_EDGE;
    }
    if (code == 3u) {
        return CHIP_PIT;
    }
    return 1.0;
}
// Texels below this are thrown away rather than drawn. 0.5 is the usual
// choice: the leaf textures are 0 or 255, so anything in between only
// comes from filtering at the texel edges.
const ALPHA_CUTOFF: f32 = 0.5;
// How much of what is behind it a block of water hides, seen at a
// glancing angle. See `WATER_ALPHA_OVERHEAD` for straight down.
const WATER_ALPHA: f32 = 0.72;

// **...and seen from straight above.** A surface reflects the sky at a
// glancing angle and lets the eye through when looked into, and a pond
// looked down into from its bank shows its bed better than the same pond
// from across it. One number for both was a lid that hid a reef three
// blocks down exactly as well from a boat as from the shore. Mixed in
// with the fourth power of the steepness, so it is the overhead view
// that clears and anything flatter than about forty-five degrees keeps
// within a few hundredths of `WATER_ALPHA`.
const WATER_ALPHA_OVERHEAD: f32 = 0.54;

// The flattest a ray through the surface is taken to be, as the sine of
// its angle below the horizon. Only there so the path length below has
// a ceiling: at this and flatter, a block of depth is twenty of water.
const WATER_GRAZE: f32 = 0.05;

// **How fast the sea closes over its own bed, per block of depth.**
//
// The alpha above used to be the whole story: one number, applied to
// every water fragment in the world, so ten blocks of ocean hid what
// was under them exactly as poorly as a puddle. That is the bug a
// player photographed and described as white noise -- and it was not
// noise, it was the sea bed.
//
// Why the bed is *speckled* rather than merely visible: skylight lost
// three levels for every block of water it crossed (water's
// `light_opacity` was 2, and every step costs one more; it is two a
// block now, see water's row in blocks.rs), so a bed that
// shelves away gently comes out in hard brightness terraces, one per
// block of depth. Seen from a beach at a grazing angle those terraces
// are one and two pixels tall, and through a 28% window they read as
// light dots and short horizontal dashes strewn over dark blue --
// thicker toward the horizon, because perspective packs more of them
// into every pixel, and twinkling whenever the camera turns, because
// nothing here is anti-aliased and a one-pixel feature is decided by
// which side of a pixel centre it falls on.
//
// So the window closes with depth, which is what water actually does.
// The constants the submerged camera already uses (`absorb`, `murk`
// further down) are the same physics from the other side of the
// surface; this is that view from above, expressed in the one channel
// a single blended pass has.
//
// The number is measured rather than chosen. On the beach of world
// `12345`, standing on the headland at (140, 41, 80) and looking down
// the shelf, the light thrown up by the bed was 142 levels of
// luminance above the water's own median -- which is the same 142 an
// earlier investigation measured and could not account for. Opaque
// water leaves 14. At 0.30 per block the figure is 72; at 0.55 it is
// 52, and what is left of it is the shoreline, which is supposed to
// show its sand.
//
// The fade is measured from the *first* cell, so a one-block puddle is
// exactly as clear as it always was: `alpha` is `WATER_ALPHA` at depth
// 1. That matters more than it sounds. Shallow water reading as
// shallow is what tells a player how deep it is before they walk in,
// and a steeper curve that started at zero would turn every pond to
// slate to cure a fault that only the open sea has.
//
// **Per block of water the ray crosses, not per block of depth.** At
// 0.55 per block of depth a reef five blocks down let three per cent of
// itself through when looked at from straight above, which is a sea
// that shows nothing: a player on a boat over a tropical reef saw the
// water's texture and no reef (measured by
// `how_much_of_the_bed_shows_through_the_sea`). But the specks were a
// *grazing* fault -- terraces one or two pixels tall far out from a
// beach -- and a ray at a grazing angle crosses many blocks of water for
// every block of depth. Dividing by the steepness is that path, and it
// takes the two complaints apart instead of trading one for the other:
// 0.08 per block of path is the old 0.55 per block of depth at about
// eight degrees below the horizon, closes the sea harder than before
// anywhere flatter -- where the terraces were -- and opens it looking
// down into it. The light that reaches the bed is the other half of
// seeing a reef, and is `light_opacity` of water in the shared crate.
const WATER_DEPTH_FADE: f32 = 0.08;

// **Water is a flat surface here, on purpose.**
//
// It reflected the sky for a while: Fresnel against the horizon
// colour, two crossing wavelets tilting the normal, and the sun
// scattered into glitter on the ripple. It read well and it cost
// about 0.05 ms of the water pass -- and it went, because the game
// asked for it to go. What is left is what a lake was before: a
// translucent surface with its own texture and its own depth tint,
// which is also the one thing on the screen that never argues with
// the night being dark.
//
// If it comes back, it comes back whole: the reflectance belongs in
// the alpha as well as in the colour, or the far end of a lake is a
// mirror you can see the sand through.

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
fn sample_block(uv: vec2<f32>, named: u32) -> vec4<f32> {
    let layer = animated(named);
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

    // The pixel's extent along each texture axis, in texels, the cheap way:
    // the same expression as `cell_footprint`, so the compiler keeps one.
    let across = (abs(ddx) + abs(ddy)) * resolution;

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
    //
    // **Both axes, and past the square root of two.** It used to leave on
    // `max(ramp) >= 1`: a face minified along one texture axis and still
    // magnified along the other went to the plain bilinear fetch unsnapped
    // across the axis where a texel is still wider than a pixel. And the
    // threshold is on the cheap extent, which is up to `sqrt(2)` longer
    // than the ramp below; leaving at one would send a band of fragments
    // whose ramp is still under a texel to the unsnapped fetch.
    if (min(across.x, across.y) >= 1.4143) {
        return plain;
    }

    // One screen pixel measured in texels *across a seam*. A seam is a line
    // of constant u (or v), and the distance from it in pixels is the
    // coordinate over the length of its gradient -- so the ramp is that
    // length, not the sum of the two derivatives. The sum is the same on a
    // face square to the screen and up to `sqrt(2)` wider on one turned
    // off it -- which is every face a player looks at from anywhere but
    // straight down a row of blocks -- and at anisotropy 4 and 16 every
    // texel edge was drawn a pixel and a half soft where the unfiltered
    // sampler draws it hard: "с анизотропией текстуры мыльные". Guarded
    // because a face seen exactly edge-on gives a derivative of zero.
    let ramp = max(sqrt(ddx * ddx + ddy * ddy) * resolution, vec2<f32>(1e-5));
    // And an axis already minified is handed back unchanged rather than
    // squeezed: `offset / ramp` with a ramp over one texel pulls the sample
    // *toward* the seam, a blend of the two texels either side of it.
    let snapped = crisp_uv(uv, resolution, min(ramp, vec2<f32>(1.0)));
    // **A magnified axis is handed to the sampler with no extent.** The
    // snap has already chosen the texel along it and ramped the one-pixel
    // seam; the pixel's real footprint along that axis only tells an
    // anisotropic sampler to spread its taps around the snapped point --
    // half a pixel either side of it, into the neighbouring texel -- and
    // to blend each tap bilinearly. That undid the snap: every texel edge
    // up close came out two or three pixels soft at anisotropy 4 and 16
    // and hard at 1, the other half of "с анизотропией мыльно". Zeroing
    // only the magnified axis leaves the level and the tap count of a
    // minified one exactly as they were: the level is chosen from the
    // longer axis over the tap limit, which is still the minified one,
    // and on a face magnified both ways the level was the full-size
    // picture already.
    let spread = select(vec2<f32>(1.0), vec2<f32>(0.0), ramp < vec2<f32>(1.0));
    return textureSampleGrad(block_textures, block_sampler, snapped, i32(layer), ddx * spread, ddy * spread);
}

// **A cut-out picture ends at the top of its quad, and the sampler does not
// know it.** The sampler repeats, because a merged face tiles its picture
// across the cells it covers; so a fetch at the very top of a plant's plane
// -- `v` a hair above zero -- filters across that edge into the bottom row of
// the same picture, which is the opaque foot of the stems. Each of a tuft's
// two crossed planes kept a line of it along its top, and from above the pair
// drew a small cross over every tuft, flower, crop and flame: "над креста
// образными объектами по типу травы или огня есть какой то крестик который мы
// чинили но не починили". `centroid` on the interpolated UV was that first
// repair, and it closed the other way the edge reaches the foot -- a sample
// taken outside the triangle -- not this one. `cross_top_repro` takes the two
// apart.
//
// **Held down at the top edge only, and by no more than two texels.** Magnified
// that is half a texel, the centre of the top row, which `crisp_uv` then never
// ramps past; minified, half the footprint plus the level's own reach. Every
// other edge was left to wrap: the sides and the bottom of a plant meet
// transparent columns and rows across the wrap, and on a merged leaf face an
// edge inside the quad is a seam the repeat is there to tile over. The cap
// is the rejected alternative's lesson: the control in `cross_top_repro`
// clamped every edge by the whole footprint, which far off is half the
// picture, and flattened the top half of every distant leaf face to keep a
// line nobody can see at that distance.
//
// No branch, so `sample_block`'s derivatives stay in uniform control flow.
fn sample_cutout(uv: vec2<f32>, named: u32) -> vec4<f32> {
    // **The picture's own size, never `texture_params.x`.** That uniform is
    // the resolution only while filtering is on and zero with anisotropy
    // off -- it is `sample_block`'s switch, not a measurement. Read here as
    // `max(.., 1.0)` it made a texel the whole picture: the reach below came
    // out at half a picture or more, every fetch in the top half of a plant
    // was held on its middle row, and with anisotropy off each tuft, flower,
    // flame and campfire was drawn as solid vertical stripes of one row
    // stretched up the plane ("при отключении анизотропной фильтрации
    // растения, огонь и костры ломаются"). `aniso_repro` photographs it.
    let resolution = f32(textureDimensions(block_textures).x);
    let ddx = dpdx(uv);
    let ddy = dpdy(uv);
    let lx = length(ddx);
    let ly = length(ddy);
    let footprint = max(min(lx, ly), max(lx, ly) / 16.0);
    let reach = min(0.5 * (abs(ddx.y) + abs(ddy.y)) + max(0.5 / resolution, footprint), 2.0 / resolution);
    let held = vec2<f32>(uv.x, max(uv.y, reach));
    let filtered = sample_block(held, named);
    // **Magnified, the cut-out is the texel under the fragment's alpha, not
    // the filtered one.** `crisp_uv` ramps across the pixel at a seam, which
    // along a straight edge crosses one half exactly on the seam -- the same
    // edge either way -- but at a corner it blends four texels, and inside
    // the one opaque texel of a convex corner the blend is a quarter plus a
    // bit, under the cut-off: a fragment a third of a pixel inside the
    // picture was thrown away. On a leaf that is a corner shaved by a
    // fraction of a pixel. On a stone lying on the ground (`engine::relief`)
    // there is nothing behind its top but the ground under it, and it was a
    // pixel of grass in the middle of a stick -- found in `relief_repro`'s
    // `where_the_seams_on_a_stone_come_from`, where this alone took those
    // out. Minified the filtered alpha stands: the ramp covers whole texels
    // there and the mip chain is what keeps a distant canopy from sparkling.
    // `textureLoad` takes no derivative, so it is legal past nothing and
    // costs one fetch on the near fragments of a cut-out.
    let ramp = (abs(ddx) + abs(ddy)) * resolution;
    // **Behind the branch, not behind a `select`.** `select` is a value and
    // both of its arms are evaluated, so the fetch below was taken on every
    // cut-out fragment in the frame and thrown away on the minified ones --
    // which is most of a canopy and every distant tuft. `textureLoad` takes
    // no derivative, so it is legal in non-uniform control flow and the
    // branch is allowed where `textureSampleGrad`'s would not be. The
    // answer is unchanged to the byte; what changes is one fetch a fragment
    // on the far two thirds of the foliage.
    var alpha = filtered.a;
    if (max(ramp.x, ramp.y) < 1.0) {
        let size = vec2<f32>(textureDimensions(block_textures).xy);
        alpha = textureLoad(
            block_textures,
            vec2<i32>(clamp(fract(held) * size, vec2<f32>(0.0), size - 1.0)),
            i32(animated(named)),
            0,
        ).a;
    }
    return vec4<f32>(filtered.rgb, alpha);
}

// How much of the sun a face turned along `normal` catches, 0.35..1.
//
// Half-lambert: a face turned away from the sun goes dim, not black, which
// is what you want when the only other light source is ambient.
//
// **Below the horizon the beam has no direction, and it is taken away.**
// The sun's direction is a vector through the world, and nothing stopped it
// when the sun set: from dusk to dawn the light travelled *up*, so the faces
// that caught it were the ones looking down and the ones looking at a sun
// already under the hills. Measured through this shader, one face direction
// at a time (`what_the_night_looks_like_up_close`): half an hour of game time
// after sunset the walls facing where the sun went down were lit as brightly
// as the open ground in front of them, and in a forest more than twice as
// brightly as the floor under the crowns -- walls glowing at a sun nobody
// could see, and every block edge where one met the dusk drawn as a line.
// That is the report of a night "too harsh, with the corners showing". Taken
// away, those walls lose a quarter to two fifths of their light, the step
// between neighbouring pixels -- how hard the edges are -- falls by twelve to
// twenty per cent looking toward the lost sun, and the frame comes out a per
// cent or two darker rather than lighter. Midnight was barely touched: by
// then the faces that look down see too little sky for the beam to matter.
//
// So as the sun goes under, every face's share of the beam comes down to the
// floor it already had facing away, over the first `NIGHT_FLAT_BY` of the
// sun's drop -- nothing at the moment of sunset, so the evening does not
// jump. **The floor, and not an average of the faces**: no face is given
// more light than it had, so the night cannot come out brighter anywhere,
// and what comes down is only what a sun under the world was lighting. What
// the night keeps -- the dusk's `sun.w`, the sky fill, the ambient floor --
// is untouched and the same from every direction, which is what light from
// a dark sky is.
//
// Rejected: mirroring the sun above the horizon as a moon. It keeps the
// shape a lit face gives -- and it is a directional light at night that was
// never there, which lights every top face with the full beam -- by this
// shader's own constants, open ground at midnight 1.6 times as bright as it
// is, for a player who has asked twice for the night to be darker.
const NIGHT_FLAT_BY: f32 = 0.1;

fn half_lambert(normal: vec3<f32>) -> f32 {
    let lit = max(dot(normal, -globals.sun.xyz), 0.0) * (1.0 - LAMBERT_FLOOR) + LAMBERT_FLOOR;
    // `sun.y` turns positive as the light starts travelling upward: the sun
    // is under the world.
    return mix(lit, LAMBERT_FLOOR, smoothstep(0.0, NIGHT_FLAT_BY, globals.sun.y));
}

// **How much of the ambient a point on a model gets, by the way it faces.**
//
// A chunk's vertices carry occlusion the mesher counted from the blocks
// round each corner. A model's carry three -- fully open -- on every vertex
// of every box, because there are no blocks to count: so an animal is a set
// of cuboids every face of which takes the same fill, its belly as bright as
// its back, and that is precisely what the word "пластиково" names. A toy,
// moulded in one colour, with the light painted on.
//
// What is missing is not detail, it is the *sky*. Ambient light comes from
// above: a surface turned up sees the whole of it, one turned down sees the
// ground and sees much less. That is one component of the normal the vertex
// already carries, and it rounds a box without bevelling it.
//
// **Handed through the occlusion slot** rather than added as a term of its
// own, because it is the same quantity -- how much of the ambient reaches
// here -- and the fragment already squares it, clamps it and scales it by
// the player's own ambient-occlusion setting. No interpolant, no uniform,
// and it follows a limb as it swings, because the normal does.
//
// Multiplied into whatever occlusion the caller had, so anything that ever
// learns to count its own keeps it.
fn model_openness(normal_y: f32, ao: f32) -> f32 {
    return ao * (1.0 - MODEL_SKY_FILL * (0.5 - 0.5 * normal_y));
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
    return terrain_vertex(in);
}

// The body of `vs_main`, as a function so the shadowed entry point and
// the shadow caster can reuse it rather than carry a second copy of the
// crop and tint decoding. Inlined by every compiler this runs through.
fn terrain_vertex(in: VertexInput) -> VertexOutput {
    var out: VertexOutput;
    let light = in.packed & LIGHT_MASK;

    let world = in.position + in.chunk_offset.xyz;
    // Cells, not corners: a merged rectangle four blocks wide hands
    // this a 4, and the sampler's `Repeat` mode tiles the picture
    // across it. An unmerged face hands it a 1 and comes out exactly as
    // it always did, which is what makes `MERGE_COPLANAR_FACES = false`
    // a real switch rather than a different-looking mode.
    var uv = vec2<f32>(
        f32(in.uv_cells & UV_MASK),
        f32((in.uv_cells >> V_SHIFT) & UV_MASK),
    );
    if ((in.uv_cells & FINE_UV_BIT) != 0u) {
        uv = vec2<f32>(
            f32(in.uv_cells & FINE_MASK),
            f32((in.uv_cells >> FINE_V_SHIFT) & FINE_MASK),
        ) / FINE_UNITS;
    }
    out.clip_position = globals.view_proj * vec4<f32>(world, 1.0);
    out.uv = uv;
    out.tex_layer = ((in.packed >> LAYER_SHIFT) & LAYER_MASK)
        | (((in.packed >> LAYER_HIGH_SHIFT) & 1u) << 8u)
        | (((in.uv_cells >> LAYER_TOP_SHIFT) & 3u) << 9u);
    out.view_distance = length(world - globals.camera_pos.xyz);
    out.translucent = light & TRANSLUCENT_BIT;
    // The byte is a foliage tint. It used to carry a texture crop above
    // 225 as well; a small face carries a real place in the picture now
    // (`FINE_UV_BIT` above), so the byte has one reading on a solid face.
    let raw_code = in.packed >> TINT_SHIFT;
    // A decal is untinted and nudged toward the eye (`DECAL_TINT`). Not for
    // water, whose byte is a depth.
    let decal = out.translucent == 0u && raw_code == DECAL_TINT;
    let code = select(raw_code, 0u, decal);
    if (decal) {
        // A decal lies on an upward face (`flat_block`), so the eye's height
        // over it is how edge-on it is seen. Never under a twentieth, or a
        // floor level with the eye -- a line on the screen -- would be pulled
        // through everything in front of it.
        let over = max(abs(globals.camera_pos.y - world.y), 0.05);
        out.clip_position.z = out.clip_position.z - (DECAL_DEPTH + DECAL_SLOPE / over) * out.clip_position.w;
    }
    if (out.translucent != 0u) {
        // The byte's second reading, and the translucent bit is what
        // picks it: water is not foliage, so the two cannot meet. See the
        // note beside `liquid_depth_below` in mesh.rs.
        out.water_depth = f32(code);
        out.tint = vec3<f32>(1.0);
    } else {
        out.tint = foliage_tint(code);
        out.water_depth = 0.0;
    }
    // Nought is "not foliage", which is most of the world; a surface tint
    // (soot, ash) is a code of its own and comes back negative. See
    // `foliage_tint`.
    let foliage = out.translucent == 0u && code != 0u && out.tint.x > 0.0;

    // See `half_lambert` for what a face's share of the sun is, and for
    // why it has no direction at night.
    let normal = face_normal((light >> 10u) & 7u);
    out.lambert = half_lambert(normal);
    // Under the speck hunt the flat slot carries the *face index*
    // instead, so the fragment can paint each direction its own colour.
    // See `speck_colour`.
    if (globals.texture_params.w > 0.5) {
        out.lambert = f32((light >> 10u) & 7u);
    }

    let sky = f32(light & 15u) / 15.0;
    let block = f32((light >> 4u) & 15u) / 15.0;
    let ao = f32((light >> 8u) & 3u) / 3.0;
    out.light_terms = vec3<f32>(sky, block, ao);

    // **Where this fragment will have to look to find its block.**
    //
    // Stepped half a block *into* the face, or the two blocks either
    // side of a shared plane would land in the same cell and a wall
    // would come out in stripes. Handed on relative to the frame's
    // origin; `mottled_shade` puts the origin back. See `shade_cell`.
    out.shade_cell = world - normal * 0.5;
    out.mottled = (in.packed & MOTTLED_BIT)
        | select(0u, UPWARD_BIT, normal.y > 0.5)
        | select(0u, FOLIAGE_BIT, foliage);
    // ...and whether it is the face a pick has just opened, in the same flat
    // slot: the word it rides in is the `uv` word, and only a fine
    // coordinate carries it. See `mesh::CHIPPED_BIT`.
    if ((in.uv_cells & FINE_UV_BIT) != 0u) {
        out.mottled = out.mottled | (in.uv_cells & CHIPPED_BIT);
    }
    return out;
}

// This block's own shade, a hair either side of white -- a colour and
// not a number, because what a heap of real stone varies in is
// temperature and not volume.
//
// `cell` arrives relative to the frame's origin, which is a whole
// number of blocks (see `FrameParams::render_origin`), so putting the
// origin back after the `floor` recovers the true cell exactly. Hashing
// the relative position instead would make the whole pattern crawl
// across the terrain every time the player walked far enough to move
// the origin.
//
// **What running it per pixel costs, measured.** Two hashes per
// fragment against two per vertex: the solid pass went from 0.179 ms to
// 0.248 ms at 1280x720 on a GTX 1050 Ti, in a frame whose whole GPU
// time is around 0.3 ms and whose length is decided elsewhere entirely.
// Sixty microseconds is the price of the merge being invisible, and the
// merge is worth 30-60% of the terrain's vertices.
fn mottled_shade(cell_relative: vec3<f32>) -> vec3<f32> {
    let cell = vec3<i32>(floor(cell_relative)) + vec3<i32>(floor(globals.render_origin.xyz));
    // **One hash, two numbers, and the reason is the phone.** This was
    // two independent hashes -- one for brightness, one for warmth --
    // which on a desktop card cost 0.07 ms and on the phone this game
    // is aimed at cost *three milliseconds*: measured at the same spot
    // in the same world, the terrain pass went 4.0 ms without any
    // mottling and 7.1 ms with two hashes, which is 119 frames a second
    // against 196. Three and a half million pixels and a mobile GPU's
    // integer units make a per-pixel hash a different purchase from the
    // one the desktop measurement priced.
    //
    // The mixing below is what makes one hash enough. Its whole job is
    // to leave no correlation between distant bits, so the top half of
    // the word and the bottom half are already two independent draws --
    // taking them separately costs nothing and asks for no second
    // avalanche. Twelve bits each is 4096 levels for a quantity that
    // moves the colour by five per cent.
    var h = u32(cell.x) * 73856093u ^ u32(cell.y) * 19349663u ^ u32(cell.z) * 83492791u;
    h = h ^ (h >> 13u);
    h = h * 1274126177u;
    h = h ^ (h >> 16u);
    let bright = f32(h & 0xfffu) / f32(0xfffu);
    let warm = f32((h >> 12u) & 0xfffu) / f32(0xfffu);
    let value = 1.0 + (bright - 0.5) * 2.0 * BLOCK_VARIATION;
    let warmth = (warm - 0.5) * 2.0 * BLOCK_TEMPERATURE;
    return vec3<f32>(value + warmth, value, value - warmth);
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
    return item_vertex(in);
}

// The body of `vs_item`, as a function for the reason `terrain_vertex` is
// one: `vs_item_shadowed` builds on it, and an entry point cannot be called.
fn item_vertex(in: ItemVertexInput) -> VertexOutput {
    var out: VertexOutput;
    let light = in.packed & LIGHT_MASK;

    out.clip_position = globals.view_proj * vec4<f32>(in.position, 1.0);
    out.uv = in.uv;
    out.tex_layer = in.packed >> 16u;
    out.view_distance = length(in.position - globals.camera_pos.xyz);
    out.translucent = 0u;
    out.water_depth = 0.0;
    out.tint = vec3<f32>(1.0);

    let normal = face_normal((light >> 10u) & 7u);
    out.lambert = half_lambert(normal);
    // Under the speck hunt the flat slot carries the *face index*
    // instead, so the fragment can paint each direction its own colour.
    // See `speck_colour`.
    if (globals.texture_params.w > 0.5) {
        out.lambert = f32((light >> 10u) & 7u);
    }

    let sky = f32(light & 15u) / 15.0;
    let block = f32((light >> 4u) & 15u) / 15.0;
    let ao = f32((light >> 8u) & 3u) / 3.0;
    // The underside of a barrel lying in the grass sees no sky and the lid
    // of it sees all of it. See `model_openness`.
    out.light_terms = vec3<f32>(sky, block, model_openness(normal.y, ao));
    // A dropped stack is one object rather than one of a hundred
    // identical ones, so there is nothing here for the mottling to fix --
    // but where it *is* still has to be right: the fog's glow and the air
    // between it and the eye are both taken from this, and a barrel eighty
    // blocks off with a `shade_cell` of nought was hazed as if it lay at the
    // frame's origin, which is under the player's feet.
    out.shade_cell = in.position;
    out.mottled = 0u;
    return out;
}

// `vs_item`, with where the item sits in the sun's shadow map, for
// `fs_cutout_shadowed`. A dropped stack in the shade of a tree was drawn in
// full sun, because the item pipeline had no shadowed twin. Looked up from a
// point pushed off the face, as `vs_main_shadowed` does and for its reason.
//
// **`ShadowedVertexOutput` and not `ModelVertexOutput`**: wgpu refuses a
// pipeline whose vertex stage hands on a location its fragment stage does not
// take, and `fs_cutout_shadowed` takes no colour at 11. See
// `every_pipeline_takes_in_its_fragment_stage_what_its_vertex_stage_hands_on`.
@vertex
fn vs_item_shadowed(in: ItemVertexInput) -> ShadowedVertexOutput {
    let normal = face_normal(((in.packed & LIGHT_MASK) >> 10u) & 7u);
    let lifted = in.position + normal * globals.shadow_bias.x;
    let v = item_vertex(in);
    var out: ShadowedVertexOutput;
    out.clip_position = v.clip_position;
    out.uv = v.uv;
    out.tex_layer = v.tex_layer;
    out.view_distance = v.view_distance;
    out.light_terms = v.light_terms;
    out.lambert = v.lambert;
    out.translucent = v.translucent;
    out.tint = v.tint;
    out.shade_cell = v.shade_cell;
    out.mottled = v.mottled;
    out.water_depth = v.water_depth;
    out.shadow_coord = (globals.shadow_view_proj * vec4<f32>(lifted, 1.0)).xyz;
    // **Set, not left to the zero a `var` starts at.** Nought is the render
    // origin, which is beside the player and inside the fires' volume, so a
    // dropped stack would have been lit by whatever a fire could see of the
    // player's feet.
    out.lamp_point = in.position + normal * LAMP_LIFT;
    return out;
}

// ---- other players and the view model, lit as the ground is ----
//
// **These entry points are in this file because the light is.** Other
// players had `actor.wgsl` and the hand had `hand.wgsl`, and each carried its
// own idea of how lit a thing is: one number -- the sun's strength on a
// half-Lambert, plus a floor -- multiplying the picture. That was the whole
// of the lighting when they were written. Then light became a colour
// (`shade_lit`: a warm key, a cool fill from the sky, the moonlit floor, the
// lighting step) and the sun began to cast shadows, all of it here, and
// neither copy followed. At sunset the ground went gold and blue and the
// figure standing on it stayed the grey of noon; in a tree's shade a player
// was in full sun; the barrel in the hand was lit by a lamp nobody else in the
// world could see. The report was "освещение не работает на 3д модели".
//
// Three ways were weighed:
//
// * **Port the colour into both files.** Two more copies of `shade_lit`,
//   each free to fall behind the next time the light changes -- which is
//   precisely how this happened.
// * **Splice this file's lighting into theirs at load.** WGSL has no
//   include, so it would be text surgery, a fourth thing the lighting step
//   rewrites, and a test of its own to keep it honest.
// * **Entry points here** (chosen). What differs is only the vertex stage --
//   a hand built in view space, a figure with a real normal and a skin -- and
//   what those stages hand on is a `VertexOutput`, so the fragment runs
//   `shade_lit` exactly as the ground's does: the same step, the same colours,
//   the same fog, and a shadowed variant for the price of an entry point. The
//   pipelines are built from the module the step compiles (`LookPipelines`),
//   so changing the step changes them with the terrain.

// `VertexOutput`, and two more: where the vertex is in the shadow map (10),
// read only by the `_shadowed` fragment stages, and the colour its picture is
// multiplied by (11) -- white on a skin, a garment's dye on a held shirt. One
// struct for both kinds of fragment stage; a stage that does not read a
// location simply does not.
struct ModelVertexOutput {
    @builtin(position) clip_position: vec4<f32>,
    @location(0) @interpolate(perspective, centroid) uv: vec2<f32>,
    @location(1) @interpolate(flat) tex_layer: u32,
    @location(2) view_distance: f32,
    @location(3) light_terms: vec3<f32>,
    @location(4) @interpolate(flat) lambert: f32,
    @location(5) @interpolate(flat) translucent: u32,
    @location(6) @interpolate(flat) tint: vec3<f32>,
    @location(7) shade_cell: vec3<f32>,
    @location(8) @interpolate(flat) mottled: u32,
    @location(9) @interpolate(flat) water_depth: f32,
    @location(10) shadow_coord: vec3<f32>,
    @location(11) @interpolate(flat) albedo: vec3<f32>,
};

fn modelled(v: VertexOutput, shadow_coord: vec3<f32>, albedo: vec3<f32>) -> ModelVertexOutput {
    var out: ModelVertexOutput;
    out.clip_position = v.clip_position;
    out.uv = v.uv;
    out.tex_layer = v.tex_layer;
    out.view_distance = v.view_distance;
    out.light_terms = v.light_terms;
    out.lambert = v.lambert;
    out.translucent = v.translucent;
    out.tint = v.tint;
    out.shade_cell = v.shade_cell;
    out.mottled = v.mottled;
    out.water_depth = v.water_depth;
    out.shadow_coord = shadow_coord;
    out.albedo = albedo;
    return out;
}

// Must match `hand::HandVertex`.
struct HeldVertexInput {
    @location(0) position: vec3<f32>,
    @location(1) uv: vec2<f32>,
    @location(2) packed: u32,
    @location(3) tint: vec4<f32>,
};

// Must match `hand::UNTEXTURED`.
const HELD_UNTEXTURED: u32 = 65535u;

// Where the light on the hand comes from.
//
// **A fixed direction in view space, not the sun**, and the decision
// outlived the file it was made in. The hand is the one object in the frame
// that never moves relative to the viewer, and a light that stays with it is
// what makes it read as held rather than as scenery; lit by the real sun, its
// shading swings as the player turns on the spot. What the sun gives it now
// is everything else: its colour, the sky's fill, the step, the night, the
// shade. Over the left shoulder, the usual key for a thing held before a
// camera.
const KEY_LIGHT: vec3<f32> = vec3<f32>(-0.45, 0.72, 0.53);

// How far below the eye the hand looks up its shadow: about where a held
// thing is. At the eye itself, a player whose head is just clear of a wall's
// shadow holds a lit barrel down in it.
const HELD_BELOW_EYE: f32 = 0.45;

@vertex
fn vs_held(in: HeldVertexInput) -> ModelVertexOutput {
    var v: VertexOutput;
    let light = in.packed & LIGHT_MASK;
    v.clip_position = globals.hand_view_proj * vec4<f32>(in.position, 1.0);
    v.uv = in.uv;
    v.tex_layer = in.packed >> 16u;
    // At the eye. The hand is nearer than anything the fog, the water's
    // absorption or the carried torch's fall-off measure, and nought is what
    // each of them says about the nearest thing there is.
    v.view_distance = 0.0;
    v.translucent = 0u;
    v.water_depth = 0.0;
    v.tint = vec3<f32>(1.0);
    let normal = face_normal((light >> 10u) & 7u);
    // ...and when the sun goes under, the key goes flat with every face of
    // the ground: `half_lambert`'s rule, for a direction of the hand's own.
    let keyed = max(dot(normal, KEY_LIGHT), 0.0) * (1.0 - LAMBERT_FLOOR) + LAMBERT_FLOOR;
    v.lambert = mix(keyed, LAMBERT_FLOOR, smoothstep(0.0, NIGHT_FLAT_BY, globals.sun.y));
    v.light_terms = vec3<f32>(
        f32(light & 15u) / 15.0,
        f32((light >> 4u) & 15u) / 15.0,
        // **In view space, with the key.** The hand's light is fixed
        // relative to the viewer for the reason `KEY_LIGHT` gives, and the
        // sky its ambient comes from has to be the same one -- an ambient
        // in world space would turn the shading over inside a held barrel
        // every time the player looked down. See `model_openness`.
        model_openness(normal.y, f32((light >> 8u) & 3u) / 3.0),
    );
    v.shade_cell = vec3<f32>(0.0);
    v.mottled = 0u;
    let at = globals.camera_pos.xyz - vec3<f32>(0.0, HELD_BELOW_EYE, 0.0);
    return modelled(v, (globals.shadow_view_proj * vec4<f32>(at, 1.0)).xyz, in.tint.rgb);
}

// What the held thing is made of before the light: its picture times its
// tint, or the tint alone where it has no picture. The picture is sampled
// either way, at a layer that exists: a sample that takes derivatives may not
// sit behind a branch on a varying.
fn held_surface(in: VertexOutput, albedo: vec3<f32>) -> vec4<f32> {
    let sampled = sample_block(in.uv, select(in.tex_layer, 0u, in.tex_layer == HELD_UNTEXTURED));
    if (in.tex_layer == HELD_UNTEXTURED) {
        return vec4<f32>(albedo, 1.0);
    }
    return vec4<f32>(sampled.rgb * albedo, sampled.a);
}

// `shadow_coord` is taken and not read. `vs_held` hands it on for the shadowed
// twin, and wgpu refuses a pipeline whose fragment stage leaves a location of
// the vertex stage's untaken -- the first draft did, and the game would not
// have started. See `every_pipeline_takes_in_its_fragment_stage_what_its_vertex_stage_hands_on`.
@fragment
fn fs_held(
    in: VertexOutput,
    @location(10) shadow_coord: vec3<f32>,
    @location(11) @interpolate(flat) albedo: vec3<f32>,
) -> @location(0) vec4<f32> {
    let surface = held_surface(in, albedo);
    // A tool is a sprite with holes in it: cut out, not blended, or its
    // corners come out as whatever the plate had behind it.
    if (surface.a < ALPHA_CUTOFF) {
        discard;
    }
    return shade_lit(in, surface, in.lambert);
}

@fragment
fn fs_held_shadowed(
    in: VertexOutput,
    @location(10) shadow_coord: vec3<f32>,
    @location(11) @interpolate(flat) albedo: vec3<f32>,
) -> @location(0) vec4<f32> {
    let surface = held_surface(in, albedo);
    if (surface.a < ALPHA_CUTOFF) {
        discard;
    }
    return shade_lit(in, surface, shadowed_lambert(in.lambert, in.light_terms.x, shadow_coord, in.view_distance, vec3<f32>(0.0), false));
}

// Must match `remote_players::ActorVertex`.
struct ActorVertexInput {
    @location(0) position: vec3<f32>,
    @location(1) color: vec3<f32>,
    @location(2) normal: vec3<f32>,
    @location(3) uv: vec2<f32>,
    @location(4) light: u32,
};

@vertex
fn vs_actor(in: ActorVertexInput) -> ModelVertexOutput {
    var v: VertexOutput;
    let light = in.light & LIGHT_MASK;
    v.clip_position = globals.view_proj * vec4<f32>(in.position, 1.0);
    v.uv = in.uv;
    v.tex_layer = 0u;
    v.view_distance = length(in.position - globals.camera_pos.xyz);
    v.translucent = 0u;
    v.water_depth = 0.0;
    v.tint = vec3<f32>(1.0);
    // The real normal and not one of six: a figure's limbs swing, and what
    // the CPU hands over is the cross product of the corners it emitted.
    // `half_lambert`, so a figure's beam goes flat when the sun goes under,
    // as the ground's does.
    v.lambert = half_lambert(in.normal);
    v.light_terms = vec3<f32>(
        f32(light & 15u) / 15.0,
        f32((light >> 4u) & 15u) / 15.0,
        // The sky is over a deer as it is over the grass it stands on. See
        // `model_openness`.
        model_openness(in.normal.y, f32((light >> 8u) & 3u) / 3.0),
    );
    // Only the fog's glow reads this, for which way the fragment lies.
    v.shade_cell = in.position;
    v.mottled = 0u;
    let lifted = in.position + in.normal * globals.shadow_bias.x;
    return modelled(v, (globals.shadow_view_proj * vec4<f32>(lifted, 1.0)).xyz, in.color);
}

// `crisp_uv` on a sheet that is not square, per axis, with the size read off
// the picture: a resource pack may hand over a skin drawn at twice the size,
// and the shader has no other way to find out.
fn crisp_sheet_uv(uv: vec2<f32>, size: vec2<f32>, ramp: vec2<f32>) -> vec2<f32> {
    let texel = uv * size;
    let seam = floor(texel + 0.5);
    let snapped = seam + clamp((texel - seam) / ramp, vec2<f32>(-0.5), vec2<f32>(0.5));
    return snapped / size;
}

fn actor_colour(in: VertexOutput, albedo: vec3<f32>, lambert: f32) -> vec4<f32> {
    // Taken before the branch: a derivative may only be taken in uniform
    // control flow, and a figure and an outline can share a quad of pixels.
    let size = vec2<f32>(textureDimensions(block_textures, 0));
    let ddx = dpdx(in.uv);
    let ddy = dpdy(in.uv);
    let ramp = max(sqrt(ddx * ddx + ddy * ddy) * size, vec2<f32>(1e-5));
    // The same two faults `sample_block` had and for the same reasons (see
    // there): `textureSample` took its level from the *snapped* coordinate,
    // flat inside a texel and near-vertical at a seam, so with filtering on
    // every seam of a figure fetched from its smallest level; and a ramp
    // over one texel squeezed a distant figure's samples onto the seams.
    let skin = textureSampleGrad(
        block_textures,
        block_sampler,
        crisp_sheet_uv(in.uv, size, min(ramp, vec2<f32>(1.0))),
        0,
        ddx,
        ddy,
    );
    if (in.uv.x < 0.0) {
        return outline_colour(in, albedo);
    }
    return shade_lit(in, vec4<f32>(skin.rgb * albedo, 1.0), lambert);
}

// The outline of the block under the crosshair, which rides the actor
// pipeline with no picture (`remote_players::UNTEXTURED`).
//
// **Not lit as the ground is, on purpose.** It is the one line in the world
// that says what a click will hit, and lit by `shade_lit` it would sink into
// the dark of the very cave it is most needed in. This is the light it always
// had, as `actor.wgsl` worked it out: the sun's strength on a half-Lambert,
// the ambient and a quarter of full brightness, then the fog.
fn outline_colour(in: VertexOutput, colour: vec3<f32>) -> vec4<f32> {
    let facing = (in.lambert - LAMBERT_FLOOR) / (1.0 - LAMBERT_FLOOR);
    let light = clamp(globals.sun.w * (facing * 0.6 + 0.4) + globals.fog_params.z + 0.25, 0.0, 1.3);
    var lit = colour * light;
    if (globals.extra.w > 0.5) {
        let fog_start = globals.fog_params.x;
        let fog_end = max(globals.fog_params.y, fog_start + 1.0);
        let t = clamp((in.view_distance - fog_start) / (fog_end - fog_start), 0.0, 1.0);
        lit = mix(lit, globals.fog_color.rgb, t * t);
    }
    return vec4<f32>(lit, 1.0);
}

// `shadow_coord` taken and not read, for the reason `fs_held` gives.
@fragment
fn fs_actor(
    in: VertexOutput,
    @location(10) shadow_coord: vec3<f32>,
    @location(11) @interpolate(flat) albedo: vec3<f32>,
) -> @location(0) vec4<f32> {
    return actor_colour(in, albedo, in.lambert);
}

@fragment
fn fs_actor_shadowed(
    in: VertexOutput,
    @location(10) shadow_coord: vec3<f32>,
    @location(11) @interpolate(flat) albedo: vec3<f32>,
) -> @location(0) vec4<f32> {
    return actor_colour(in, albedo, shadowed_lambert(in.lambert, in.light_terms.x, shadow_coord, in.view_distance, vec3<f32>(0.0), false));
}
// ---- end of the models ----

// Solid terrain: no `discard` anywhere in this entry point.
//
// That is the entire reason there are two. A fragment shader that can
// discard forces the GPU to run it before it knows whether the fragment
// survives, so hardware early-depth rejection is disabled for every draw
// using that shader. With one shared shader, the alpha cutout that makes
// leaves work was costing early-Z on all the terrain -- which is most of
// the triangles in the frame, and exactly what the near-to-far draw
// order exists to let the depth test throw away cheaply.
// **Six flat colours, one per face direction, for the speck hunt.**
//
// Flat black found the holes with sky behind them and was blind to the
// rest: a crack between a block's side and the block *behind* it shows
// that block, and black through black is black. Painted by direction,
// the side is one colour and whatever shows through the crack is
// another, so a hole is a pixel of the wrong colour among its
// neighbours whatever is behind it. Saturated and far apart so that no
// blend of two of them is a third. The sky stays the white clear.
fn speck_colour(face: f32) -> vec4<f32> {
    let f = u32(face + 0.5);
    if (f == 0u) { return vec4<f32>(1.0, 0.0, 0.0, 1.0); }
    if (f == 1u) { return vec4<f32>(0.0, 1.0, 0.0, 1.0); }
    if (f == 2u) { return vec4<f32>(0.0, 0.0, 1.0, 1.0); }
    if (f == 3u) { return vec4<f32>(1.0, 1.0, 0.0, 1.0); }
    if (f == 4u) { return vec4<f32>(1.0, 0.0, 1.0, 1.0); }
    return vec4<f32>(0.0, 1.0, 1.0, 1.0);
}

@fragment
fn fs_solid(in: VertexOutput) -> @location(0) vec4<f32> {
    // Flat black, so that what is *not* black is the answer. See
    // `fs_cutout` above and `FrameParams::speck_hunt`.
    if (globals.texture_params.w > 0.5) {
        return speck_colour(in.lambert);
    }
    let sampled = sample_block(in.uv, in.tex_layer);
    // **A hole drawn solid is not a hole -- it is what was behind it.**
    //
    // The complaint this fixes, in the player's words: *"при выключении
    // прозрачной листвы текстуры становятся будто 32 на 32"*. Turn the
    // cutout off and a canopy comes out a flat, pale green tile with no
    // detail in it at all -- which is exactly what a low-resolution
    // texture looks like, and exactly what was being drawn.
    //
    // The reason is one step upstream and is itself correct: every
    // invisible texel of a cutout picture is given the average colour
    // of its visible neighbours before the atlas is built (see
    // `texture::bleed_into_transparency`), so that mips and bilinear
    // taps average leaf colours rather than black. Drawn *solid*, that
    // smear is what shows: a third of the leaf tile is the mean of the
    // rest of it, and a picture that is one third its own average is a
    // picture with a third less detail.
    //
    // So the hole is shaded rather than shown. Behind a gap in a canopy
    // is the inside of the tree, which is darker than the leaf facing
    // the sun; multiplying by `HOLE_SHADE` puts that back and the
    // canopy reads as foliage again. It costs one compare and one
    // multiply, on the fragments of a picture that has holes at all --
    // every ordinary block texture is opaque and takes the `1.0` branch
    // without a memory access.
    //
    // **This is every canopy past the see-through line**, not only the
    // "solid everywhere" end of it: a chunk meshed past
    // `ClientSettings::transparent_leaves_chunks` sends its leaves through
    // this pass (`MeshBuffers::leaves_solid`, `renderer::cutout_range`),
    // so without the shade the same smear was on every distant canopy in
    // the default configuration.
    let solid_hole = step(sampled.a, ALPHA_CUTOFF);
    let filled = vec4<f32>(
        sampled.rgb * mix(1.0, HOLE_SHADE, solid_hole),
        sampled.a,
    );
    return shade(in, filled, cell_footprint(in.uv));
}

/// How dark a hole in a cutout picture is drawn when the picture is
/// drawn solid.
///
/// A little over half. Dark enough that the gaps read as depth rather
/// than as a lighter shade of the same green, light enough that a
/// canopy does not turn into a black lattice at noon -- which is what
/// the first attempt at 0.25 looked like.
const HOLE_SHADE: f32 = 0.55;

// Leaves. Same shading, plus the cutout -- and the early-Z cost, which
// is now paid only by the handful of chunks that contain a tree.
@fragment
fn fs_cutout(in: VertexOutput) -> @location(0) vec4<f32> {
    // **The speck hunt keeps the holes shut.** A leaf block is a
    // silhouette with a hundred gaps in it, every one of them showing
    // the sky, and counting those was how three earlier attempts at
    // this measured foliage and called it a crack. Here the cut-out is
    // switched off, so a canopy is one solid black shape and the only
    // white left in the frame is a pixel the mesh failed to cover. See
    // `FrameParams::speck_hunt`.
    if (globals.texture_params.w > 0.5) {
        return speck_colour(in.lambert);
    }
    let sampled = sample_cutout(in.uv, in.tex_layer);
    // Before the cut-out throws anything away: see `shade`.
    let cell_px = cell_footprint(in.uv);

    // Leaf textures carry alpha-0 texels, and without this they were
    // drawn as whatever colour happened to sit under them -- a solid
    // block of it, since the pass does not blend. This is the whole of
    // the fix for "trees are cubes".
    if (sampled.a < ALPHA_CUTOFF) {
        discard;
    }
    return shade(in, sampled, cell_px);
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

// **Told the footprint rather than taking it**: `fs_cutout` throws its
// holes away with `discard` before it gets here, and a derivative taken
// after that is one taken where some of the quad has gone.
fn shade(in: VertexOutput, sampled: vec4<f32>, cell_px: f32) -> vec4<f32> {
    return shade_lit_sky(in, sampled, in.lambert, in.light_terms.x, cell_px);
}

// `shade`, told how much of the beam this face catches rather than
// reading it off the vertex -- which is the one number a shadow changes.
// See `shadowed_lambert`.
//
// **The one the models use**, and so the one that may not take a
// derivative: `actor_colour` calls it past a branch that returns, which
// is not uniform control flow. It hands `NO_FOOTPRINT` down, and nothing
// reads it -- a model carries no `mottled` bit. See `cell_footprint`.
fn shade_lit(in: VertexOutput, sampled: vec4<f32>, lambert: f32) -> vec4<f32> {
    return shade_lit_sky(in, sampled, lambert, in.light_terms.x, NO_FOOTPRINT);
}

// How much of the world one screen pixel covers here, in blocks.
//
// **Measured on the texture coordinate, which costs nothing.** The
// terrain's `uv` is in *cells* -- a merged rectangle four blocks wide
// hands the vertex a four, and a small face hands it sixteenths (see
// `terrain_vertex`) -- so the screen derivative of `uv` is already blocks
// to the pixel, and `sample_block` takes that very derivative one line
// earlier to choose its filtering. Taken this way the two are the same
// expression and the compiler keeps one.
//
// It was the derivative of `shade_cell` instead, with a `length` on each:
// its own pair of derivatives and two square roots on every terrain
// fragment, which measured 0.2 ms of a 3.2 ms frame at 1920x1080 on the
// forest of `where_the_shadows_spend_the_frame` -- six per cent of the
// frame to fade a hash out. The same form as `ramp` in `sample_block`
// costs nothing that was not already being paid.
//
// **Only the terrain's entry points may call this**, and they call it
// before anything branches. A derivative is only defined in uniform
// control flow, and `shade_lit`'s other callers are past a branch.
fn cell_footprint(uv: vec2<f32>) -> f32 {
    let across = abs(dpdx(uv)) + abs(dpdy(uv));
    return max(across.x, across.y);
}

// What a caller with no footprint to give hands over: nought, which is a
// block filling the screen -- the answer that changes nothing for
// anything that has no `mottled` bit to read it.
const NO_FOOTPRINT: f32 = 0.0;

// **How much of the blocks' own shade survives at this size.**
//
// `mottled_shade` is a hash of the cell under the fragment: one value a
// block, with no mip chain and nothing to filter it. While a block is
// several pixels across that is exactly the point of it. Once a block is
// under two pixels it is being sampled below its own spacing, and the
// answer is a fresh lottery every time the sample point moves -- which on
// a far plain is every frame the eye moves at all. The report was "при
// прыжке на дистанции в плоскости видны искажения": a jump lifts the eye
// a block and a half straight up, the whole far ground slides at once,
// and the noise redraws itself across all of it.
//
// Measured over a flat sand plain at 1920x1080 with the player's own
// settings (`what_a_jump_does_to_a_distant_plane`), the hash put 3.3
// levels of 255 of roughness into the row just under the horizon and 1.9
// into the row at the player's feet: *more* where it cannot be made out
// than where it can, which is the signature of something sampled under
// its own spacing.
//
// So it is faded out over the last sizes at which it is legible, which is
// what a mip chain would do to it -- a cell smaller than a pixel averages
// to one, and one is where the fade ends. The fade begins at two pixels a
// block, which is the point past which there is nothing to see either
// way, and the hash is skipped entirely beyond it.
//
// **Rejected: fading by distance.** The fault is a *grazing* one. Ground
// seen nearly edge-on is a block to the pixel at thirty blocks, and the
// same ground looked at square on is eighteen pixels to the block there;
// a distance that was right for the horizon would have taken the shade
// off the wall a player is standing next to.
const MOTTLE_FADE_FROM: f32 = 0.5;
const MOTTLE_FADE_TO: f32 = 1.5;

// `shade_lit`, told what sky the sun's share is scaled by -- which is the
// flood fill's everywhere but where real shadows are cast (see `sun_sky`)
// -- and how big a block is here, for `MOTTLE_FADE_FROM`.
fn shade_lit_sky(in: VertexOutput, sampled: vec4<f32>, lambert: f32, sun_sky: f32, cell_px: f32) -> vec4<f32> {

    let sky_level = in.light_terms.x;
    let block_level = in.light_terms.y;
    let ao = in.light_terms.z;

    // **Light is a colour, not a number.**
    //
    // It used to be one scalar multiplying the albedo, and that is the
    // single biggest reason the world looked moulded rather than built:
    // scaling a colour keeps its hue and its saturation exactly, at
    // every brightness, so a face in shadow was the same colour as the
    // face beside it in sun, only quieter. Nothing real behaves that
    // way. Outdoors the direct light is warm and the light filling its
    // shadows comes from the sky and is blue, and the *difference*
    // between those two is what the eye reads a surface by.
    //
    // Both colours arrive at luminance one and the two shares sum to
    // one, so none of this changed how bright anything is. See
    // `Sky::sun_color`.
    let sun_term = sun_sky * globals.sun.w * lambert * SUN_SHARE;
    // **The torch in the player's own hand**, folded into the block
    // light rather than added after it -- see `carried_level`. It is a
    // lamp, so it goes where the lamps go, and the `max` two lines down
    // is then doing its job for a torch as well as for a hearth.
    let block_term = max(block_level, carried_level(in.view_distance)) * globals.extra.x;

    // The brightest source wins rather than the two adding -- the
    // standard voxel approximation, and the reason a torch in daylight
    // does not blow a wall out to white. Component by component, so a
    // warm lamp still warms a face the cool sky is already lighting.
    var direct = max(sun_term * globals.sun_color.rgb, block_term * FIRE_COLOR);
    var light_floor = AMBIENT_COLOR;

    // **Past the Simple step, the floor of the half-Lambert is the sky's.**
    //
    // Half-Lambert gives a face turned away from the sun a third of the
    // light a face toward it gets, and Simple paints that third in the
    // sun's colour -- so the north side of a block at sunset is a dim
    // orange, and a shadow is the lit ground at a lower volume. That
    // third is not sunlight. It stands for the light from the sky and
    // from the ground around, which is what reaches a surface the sun
    // cannot, and outdoors that is blue.
    //
    // So the sun's share is split where the floor is: what is above it
    // keeps the sun's colour, what is below it takes the fill's. **The
    // amount is exactly Simple's** -- the same `sun_term`, and a mix of
    // two colours both at luminance one -- so nothing is brighter or
    // darker than it was; a face in shade is cooler and a face in sun is
    // as warm as the key. It also keeps the torch rule: the brighter of
    // the sky and the fire still wins, component by component, as before.
    //
    // A shadow falls out of this for free. `shadowed_lambert` takes a
    // face down to the floor, the floor is the sky's colour, and the
    // shadow of a tree is the blue of the sky rather than a dark patch of
    // the ground's own colour.
    //
    // **The sky takes seven tenths of the floor, not all of it**, and the
    // pictures decided that. Handing it the whole floor was right for
    // shade and wrong for everything the sun actually lights: a face
    // only half turned to a golden-hour sun has a lambert near 0.55, the
    // floor is 0.35 of that, and a key that was two thirds sky blue came
    // out neutral green -- golden hour photographed with the sun at the
    // camera's back was the same picture at every step. At seven tenths
    // a shaded face keeps three tenths of the sun's hue and a lit one
    // about three quarters, which is warm light on cool shade rather than
    // cool light everywhere. The amount is untouched either way.
    //
    // Written multiplied out. The idea is `sun_term * mix(fill, sun,
    // 1 - SKY_OF_FLOOR * LAMBERT_FLOOR / lambert)`, and since `sun_term`
    // carries a `lambert` the division cancels exactly: what is left is
    // the sun's colour at `lambert` less `SKY_OF_FLOOR * LAMBERT_FLOOR` of
    // it handed to the sky. No divide and no clamp -- the mix factor
    // cannot leave 0..1, because `lambert` never falls below the floor --
    // on a line every terrain fragment runs.
    if (LIGHTING >= 1u) {
        let skylit = sun_sky * globals.sun.w * SUN_SHARE;
        // `min`, because a shadowed fragment's floor can be under
        // `LAMBERT_FLOOR` (`SHADOW_FLOOR`), and handing the sky more than
        // the beam there is would push the sun's colour negative -- black
        // shade with a coloured fringe. Everywhere else `lambert` is at or
        // over the floor and this is the constant it always was.
        let handed = SKY_OF_FLOOR * min(lambert, LAMBERT_FLOOR);
        let key_light = skylit * (lambert * globals.sun_color.rgb - handed * (globals.sun_color.rgb - globals.fill_color.rgb));
        direct = max(key_light, block_term * FIRE_COLOR_WARM);
        light_floor = mix(AMBIENT_COLOR, MOONLIT_FLOOR, sky_level);
    }

    // ...and the sky fills what the beam missed, in proportion to how
    // much sky the face can actually see. A cave has none, which is
    // exactly right.
    let fill = sky_level * globals.sun.w * SKY_SHARE * globals.fill_color.rgb;

    // **A floor, not a tax.** This used to add the ambient to whatever
    // the sun and the sky had already given, and by day that is
    // invisible -- noon is fifty times it. At midnight it was more than
    // half the light on an open meadow: measured on a lit stone wall
    // with the night floor at 0.03, the ground came out at 13.5 levels
    // of 255, of which 7 were this term. So a player asking for a
    // darker night was asking about a constant (`NIGHT_INTENSITY`) that
    // could only ever reach a third of what they could see, and the
    // rest was a number meant to keep caves navigable.
    //
    // Taking the larger of the two is what "floor" meant all along, and
    // it is the same rule the line above uses for the sun against a
    // torch. Nothing that was lit is dimmer than the floor, so nothing
    // that was navigable stops being: a cave still gets exactly this
    // number, because a cave has neither of the other two. What goes is
    // the *addition*, and the addition is only ever significant where
    // the real light is as faint as the floor -- which is night, which
    // is the place it was making bright.
    // **The moon's share of the night.** A moonless night takes the floor
    // down under open sky and a full moon lifts it, in proportion to the sky
    // a face can see, so a cave and the inside of a hut keep the floor they
    // had. See `MOONLESS_FLOOR` in `sky.rs`.
    light_floor = light_floor * (1.0 + globals.moon.w * sky_level);
    var light = max(direct + fill, globals.fog_params.z * light_floor);
    // Ambient occlusion darkens creases; strength is configurable so it
    // can be dialled down without touching the mesher.
    //
    // **Squared, so a seam is a shade and a corner is dark.** The mesher's
    // level counts how many of a corner's three neighbours are solid, and a
    // straight line of the darkening -- which this was -- took a third of
    // the strength off for one: every corner where a floor meets a wall, on
    // every block of every wall, came down by fifteen per cent at the
    // default strength and drew the grid of the world on the ground. The
    // player's word for it was "видны углы". Squared, one neighbour takes a
    // ninth of the strength (five per cent at the default), two take four
    // ninths, and a corner shut in on all sides still takes all of it --
    // the crease is kept where there is one. `mesh::corner_brightness`
    // weighs the corners the same way.
    let open = 1.0 - ao;
    let ao_factor = 1.0 - globals.extra.y * open * open;
    // ...and this block's own shade, so a wall of one texture is not a
    // hundred identical copies of it. See `BLOCK_VARIATION`.
    //
    // Worked out here rather than handed down from the vertex shader:
    // one quad is no longer one block, and a shade fetched per vertex
    // paints a whole merged rectangle in the colour of one of its
    // corners. See `shade_cell`.
    var variation = vec3<f32>(1.0);
    // Inside the flat branch, so nothing that is not a merged block pays
    // for the fade at all.
    if ((in.mottled & MOTTLED_BIT) != 0u) {
        let mottle_seen = 1.0 - smoothstep(MOTTLE_FADE_FROM, MOTTLE_FADE_TO, cell_px);
        if (mottle_seen > 0.0) {
            variation = mix(vec3<f32>(1.0), mottled_shade(in.shade_cell), mottle_seen);
        }
    }
    // ...and the marks of the pick on a face somebody is digging into. Off
    // the fine coordinate, which on a cut face is the place in the stone's
    // own picture: the marks sit on its texels. See `chip_shade`.
    if ((in.mottled & CHIPPED_BIT) != 0u) {
        variation = variation * chip_shade(in.uv);
    }
    light = clamp(light * ao_factor * variation, vec3<f32>(0.0), vec3<f32>(1.4));

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
    // A negative tint is a surface tint and covers the whole texel -- see
    // `foliage_tint`. The branch is a select, not an `if`: see the note
    // above on what a branch here was measured to cost.
    let surface = in.tint.x < 0.0;
    let tinted = select(mix(vec3<f32>(1.0), in.tint, greenness), -in.tint, surface);
    // **Rain darkens what it falls on**, and nothing in this world got wet.
    //
    // Scaled by the sky the face can see, so a floor under a roof and the
    // whole of a cave stay dry, and by how much of the shower has actually
    // reached the ground (`Sky::rain_arrived`) rather than by how overcast
    // it is -- the cloud closes a good minute before the first drop, and a
    // meadow soaked by a sky that is merely grey is the same lie the other
    // way round. See `WET_DARKEN`.
    let wet = globals.weather.x * sky_level;
    var color = sampled.rgb * tinted * light * mix(1.0, WET_DARKEN, wet);

    // **A leaf lit from behind glows**, and nothing in this world did.
    //
    // Free of a normal, which this fragment has not got: foliage glows when
    // the *eye* is down-sun of it -- the beam travelling along `sun.xyz` and
    // the view ray pointing the same way -- and in proportion to how little
    // of that beam the face itself catches, which is what a `lambert` at its
    // floor means. Both numbers are already here, and `sun_sky` is already
    // nought in the shadow of the trunk, so a leaf the sun cannot reach does
    // not light up.
    //
    // **Greenness and not a flag.** A leaf, a blade of grass and the turf on
    // top of a dirt block are the things this happens to and they have no bit
    // in common -- but the picture knows, the same way the foliage tint knows
    // (see the note above it). Stone's greenness is nought and it pays a
    // multiply. See `LEAF_GLOW`.
    if ((in.mottled & FOLIAGE_BIT) != 0u && greenness > 0.0) {
        let ray = in.shade_cell - globals.camera_pos.xyz;
        let through = max(dot(ray, globals.sun.xyz), 0.0) * inverseSqrt(dot(ray, ray) + 1e-8);
        let t2 = through * through;
        let t4 = t2 * t2;
        // How far this face is from facing the sun, 0 lit and 1 turned away.
        let turned = clamp((1.0 - lambert) / (1.0 - LAMBERT_FLOOR), 0.0, 1.0);
        let glow = LEAF_GLOW * greenness * t4 * t4 * turned * sun_sky * globals.sun.w;
        color = color + sampled.rgb * tinted * globals.sun_color.rgb * glow;
    }
    // Before the water and the fog, so it shapes the lit surface and
    // never the colour the world fades into: the fog's end has to be the
    // sky's colour exactly, and a shoulder over it would not be.
    if (LIGHTING >= 1u) {
        color = shoulder(color);
    }
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
        // than to black, or deep water reads as a cave. The fog colour
        // unscaled: it is what `fs_sky` paints under water and what the fog
        // below finishes on, and it took all three agreeing to stop the far
        // bed standing out of the water in bands (see `fog::UNDERWATER`).
        let murk = clamp(depth / 26.0, 0.0, 0.85);
        color = mix(color, globals.fog_color.rgb, murk);
    }

    // **A sheet of water with no highlight on it is a sheet of plastic.**
    //
    // Every surface in this game was a perfect diffuser -- the same colour
    // returned in every direction -- so a lake gave back exactly what a
    // painted floor of the same blue would. Water is the one material whose
    // look the eye knows without being told, and it is the loudest thing in
    // a screenshot of a meadow.
    //
    // Two terms, both off the one cosine the alpha at the end of this
    // function already needs:
    //
    // * **the sky, by Fresnel** -- a window looked into and a mirror looked
    //   along, so the pond at the player's feet keeps its bed and the far
    //   end of the lake goes the colour of the horizon, which is what the
    //   fog colour *is*;
    // * **the sun, as a glitter** -- a tight lobe round the mirror
    //   direction, laid over the reflected sky.
    //
    // **Only the lid**, and only from above it. A water *side* is a cut
    // through the column with no sky over it (a depth past one is the top --
    // see the alpha below), and from under the surface this would be a
    // reflection of a sky the swimmer is not on the same side of.
    //
    // Before the fog, so the far end of a lake still fades into the horizon
    // rather than shining out of it, and after the shoulder, because a
    // highlight is allowed to be the brightest thing in the frame.
    if (in.translucent != 0u && (in.mottled & UPWARD_BIT) != 0u && globals.extra.z < 0.5) {
        // `shade_cell` sits half a block under the face, as the alpha below
        // has to put back for the same reason.
        let ray = in.shade_cell + vec3<f32>(0.0, 0.5, 0.0) - globals.camera_pos.xyz;
        let steep = clamp(abs(ray.y) / max(in.view_distance, 0.001), 0.0, 1.0);
        // Schlick over water's own reflectance at normal incidence.
        let grazing = 1.0 - steep;
        let g2 = grazing * grazing;
        let mirrored = vec3<f32>(ray.x, -ray.y, ray.z);
        let sky_seen = glow_over(globals.fog_color.rgb, mirrored);
        let fresnel = (WATER_F0 + (1.0 - WATER_F0) * g2 * g2 * grazing) * WATER_SHEEN;
        color = mix(color, sky_seen, fresnel);
        // ...and the sun itself, where the mirrored ray points at it. Two
        // squarings and a cube rather than a `pow`, the way `glow_over`
        // shapes its halo.
        let facing = max(dot(mirrored, -globals.sun.xyz), 0.0) * inverseSqrt(dot(mirrored, mirrored) + 1e-8);
        let f2 = facing * facing;
        let f8 = f2 * f2 * f2 * f2;
        let f24 = f8 * f8 * f8;
        // **Through the same Fresnel as the sky.** The sun is part of what
        // the surface reflects, so it obeys the same rule: two per cent of
        // it straight down and nearly all of it along. Added on its own it
        // was a white disc on the water under the sun whatever the angle,
        // which reads as a lens artefact rather than as a sun path -- and a
        // pond looked straight into had a lamp in it.
        color = color + globals.sun_color.rgb * (globals.sun.w * WATER_GLITTER * fresnel * f24 * f24);
    }

    // **The air between here and the eye**, which used to be nothing at all
    // until the fog began. See `AIR_DEPTH`.
    //
    // Under water this is left out: `absorb` and `murk` above are the same
    // idea in the medium that is actually there, measured, and running both
    // would take the colour out of a reef twice.
    //
    // Luminance is kept exactly -- the mix is between a colour and a grey of
    // its own weight, tinted by the sky's hue -- so this moves no exposure
    // and nothing anybody argued over. `fill_color` is the sky at luminance
    // one, already on the uniform for the shadow fill.
    if (globals.extra.z < 0.5) {
        // **The fragment's own distance, squared, and not the vertex's.**
        //
        // `view_distance` is a `length` taken per vertex and interpolated
        // linearly, and linear interpolation of a distance sags in the
        // middle of a big quad -- by nothing anybody could see, until a term
        // that reads it applies to the near field. A merged rectangle
        // fourteen blocks across then shaded a few levels differently from
        // the same rectangle cut into its cells, which is precisely the
        // fault `a_block_wears_its_own_shade_however_many_blocks_the_quad_covers`
        // exists to catch, and it caught it.
        //
        // `shade_cell` is a *position*, interpolated perspective-correctly
        // and therefore exact across a planar quad however it is cut. Kept
        // squared so the curve needs no square root: the knee is still
        // `AIR_HALF`, and what changes is that the first twenty blocks take
        // less of the haze, which is what air does.
        let ray = in.shade_cell - globals.camera_pos.xyz;
        let far = dot(ray, ray);
        let luma = dot(color, vec3<f32>(0.2126, 0.7152, 0.0722));
        let air = AIR_DEPTH * far / (far + AIR_HALF * AIR_HALF);
        color = mix(color, luma * globals.fill_color.rgb, air);
    }

    if (globals.extra.w > 0.5) {
        let fog_start = globals.fog_params.x;
        let fog_end = max(globals.fog_params.y, fog_start + 1.0);
        // Squared falloff: closer to how real aerial perspective behaves
        // than a straight linear ramp, and it keeps the near field clear.
        let t = clamp((distance - fog_start) / (fog_end - fog_start), 0.0, 1.0);
        // **Toward the sunset, the distance is the sunset.** The same
        // `glow_over` the sky draws its horizon with, in the direction of
        // this fragment, so the far hills under a setting sun melt into
        // orange and the ones behind the player into dusk -- and the two
        // agree at the horizon in every direction.
        //
        // The direction comes from `shade_cell`, which is this fragment's
        // position half a block inside its face, already interpolated for
        // the mottling; half a block is nothing at the distances the fog
        // acts at, and a new interpolant would be paid for on every
        // fragment at every step. Skipped where the fog has not begun,
        // which is the whole near field.
        var fog_colour = globals.fog_color.rgb;
        if (LIGHTING >= 1u && t > 0.0) {
            fog_colour = glow_over(fog_colour, in.shade_cell - globals.camera_pos.xyz);
        }
        color = mix(color, fog_colour, t * t);
    }

    var alpha = sampled.a;
    if (in.translucent != 0u) {
        // Deeper water lets less of its bed through. `max(_, 0.0)`
        // rather than trusting the byte: a depth of zero on a face
        // flagged translucent would otherwise brighten the water
        // instead of darkening it.
        let depth = max(in.water_depth - 1.0, 0.0);
        // **How steeply the eye looks through the surface.** The only
        // face that carries a depth past one is the top of the water
        // (`only_the_top_of_water_is_faded_by_the_column_under_it`), and
        // `shade_cell` sits half a block under that face -- so it is put
        // back up there, or the lid right over a swimmer's head reads as
        // seen edge-on. A side (depth one) needs no lift and its
        // exponent is zero anyway.
        let lift = select(0.0, 0.5, in.water_depth > 1.0);
        let steep = clamp(
            abs(in.shade_cell.y + lift - globals.camera_pos.y) / max(in.view_distance, 0.001),
            WATER_GRAZE,
            1.0,
        );
        let window = mix(1.0 - WATER_ALPHA, 1.0 - WATER_ALPHA_OVERHEAD, steep * steep * steep * steep);
        alpha = 1.0 - window * exp(-WATER_DEPTH_FADE * depth / steep);
    }
    return vec4<f32>(color, alpha);
}

// ---------------------------------------------------------------------
// The sun's shadows. See `engine::shadow` for the design and for the
// two approaches this was chosen over.
//
// None of what follows is reached unless the player has turned shadows
// on *and* the sun is up: the game switches to these entry points
// rather than branching on a flag inside the ordinary ones, so the
// frame without shadows runs the code above and nothing else.
// ---------------------------------------------------------------------

// What half-Lambert leaves a face the sun cannot see. The shadow takes
// a lit face down to exactly this and no further, so a shaded top is as
// dark as the north side of the same block -- never darker, which is
// what "the sky still lights it" means.
const LAMBERT_FLOOR: f32 = 0.35;

// `VertexOutput`, plus where the fragment sits in the shadow map.
//
// A struct of its own because a vertex stage returns one struct and the
// ordinary one must not grow: an extra interpolant is paid for on every
// fragment of the frame, shadows or not.
struct ShadowedVertexOutput {
    @builtin(position) clip_position: vec4<f32>,
    @location(0) @interpolate(perspective, centroid) uv: vec2<f32>,
    @location(1) @interpolate(flat) tex_layer: u32,
    @location(2) view_distance: f32,
    @location(3) light_terms: vec3<f32>,
    @location(4) @interpolate(flat) lambert: f32,
    @location(5) @interpolate(flat) translucent: u32,
    @location(6) @interpolate(flat) tint: vec3<f32>,
    @location(7) shade_cell: vec3<f32>,
    @location(8) @interpolate(flat) mottled: u32,
    @location(9) @interpolate(flat) water_depth: f32,
    // Linear in the world, because the sun's camera is orthographic:
    // there is no divide to get wrong, and interpolating it across a
    // face gives exactly the point that fragment would project to.
    @location(10) shadow_coord: vec3<f32>,
    // Where the fires' shadows are looked up from: the fragment, just in
    // front of its face. See `LAMP_LIFT`.
    @location(11) lamp_point: vec3<f32>,
};

@vertex
fn vs_main_shadowed(in: VertexInput) -> ShadowedVertexOutput {
    let v = terrain_vertex(in);
    var out: ShadowedVertexOutput;
    out.clip_position = v.clip_position;
    out.uv = v.uv;
    out.tex_layer = v.tex_layer;
    out.view_distance = v.view_distance;
    out.light_terms = v.light_terms;
    out.lambert = v.lambert;
    out.translucent = v.translucent;
    out.tint = v.tint;
    out.shade_cell = v.shade_cell;
    out.mottled = v.mottled;
    out.water_depth = v.water_depth;

    // Looked up from a point pushed off the face along its normal. See
    // `shadow::NORMAL_OFFSET_TEXELS` for the seam along the foot of every
    // wall that this removes.
    let world = in.position + in.chunk_offset.xyz;
    let normal = face_normal(((in.packed & LIGHT_MASK) >> 10u) & 7u);
    let lifted = world + normal * globals.shadow_bias.x;
    out.shadow_coord = (globals.shadow_view_proj * vec4<f32>(lifted, 1.0)).xyz;
    out.lamp_point = world + normal * LAMP_LIFT;
    return out;
}

// How much of the sun reaches this point, 0..1, with the fade toward
// the map's edge and the day's strength already applied.
//
// Four hardware-filtered lookups a little apart, so sixteen texels vote:
// one alone gives a hard edge with a staircase a texel high, which up
// close is the grid of the map drawn on the ground.
// `textureSampleCompareLevel` rather than `textureSampleCompare`: it
// takes no derivatives, so it may sit behind the branches in
// `shadowed_lambert` that skip it for most of the screen.
fn sunlit_share(coord: vec3<f32>, view_distance: f32) -> f32 {
    let uv = coord.xy * vec2<f32>(0.5, -0.5) + vec2<f32>(0.5);
    let depth = coord.z - globals.shadow_bias.y;
    if (any(uv <= vec2<f32>(0.0)) || any(uv >= vec2<f32>(1.0)) || depth >= 1.0) {
        return 1.0;
    }
    let o = globals.shadow_params.w;
    var seen: f32;
    if (globals.shadow_bias.w > 0.5) {
        // **Hard shadows read one texel and compare it themselves**
        // (`shadow::Mode::Hard`): in or out, with nothing blended.
        //
        // It was the hardware's filtered compare of the four texels round
        // the point -- one call, and a *ramp* across two texels rather
        // than an edge. A texel is a tenth of a block, which sounds like
        // nothing until you look at ground ten blocks away at a grazing
        // angle, where a tenth of a block is eighteen screen pixels: the
        // report was "тени от листвы размытые", and measured across the
        // edge of a crown's shadow at 13:24 the filtered read crossed from
        // lit to shaded over 18 pixels where the voxel walk, which decides
        // the nearest thirty blocks, crossed in 1 (`what_a_canopy_casts`).
        // One tree therefore threw two different shadows in one frame.
        //
        // A read has a staircase where a filter does not, and the grid it
        // steps along is the thing that makes this affordable: the picture
        // is laid out in the world's own x and z (`LightView::around`), so
        // every edge a block has along either axis is a straight line of
        // whole texels at any height and any hour. What is left diagonal
        // is the upright edges of a block, and those end a shadow rather
        // than run along it.
        //
        // It is also the cheaper read: no comparison sampler, no blend.
        let size = vec2<f32>(textureDimensions(shadow_map, 0));
        let texel = clamp(vec2<i32>(uv * size), vec2<i32>(0), vec2<i32>(size) - vec2<i32>(1));
        seen = select(0.0, 1.0, depth <= textureLoad(shadow_map, texel, 0));
    } else {
        seen = (textureSampleCompareLevel(shadow_map, shadow_sampler, uv + vec2<f32>(-o, -o), depth)
            + textureSampleCompareLevel(shadow_map, shadow_sampler, uv + vec2<f32>(o, -o), depth)
            + textureSampleCompareLevel(shadow_map, shadow_sampler, uv + vec2<f32>(-o, o), depth)
            + textureSampleCompareLevel(shadow_map, shadow_sampler, uv + vec2<f32>(o, o), depth)) * 0.25;
        // **At the High lighting step, a second ring**: four more taps on the
        // axes, further out, averaged half and half with the first. Thirty-two
        // texels vote instead of sixteen, over a footprint nearly twice as
        // wide, so the edge of a tree's shadow is a soft penumbra rather than
        // a soft line -- for four more lookups on the fragments that reach
        // this far, which is what the step's cost table prices.
        if (LIGHTING >= 2u) {
            let r = o * 2.2;
            let ring = textureSampleCompareLevel(shadow_map, shadow_sampler, uv + vec2<f32>(r, 0.0), depth)
                + textureSampleCompareLevel(shadow_map, shadow_sampler, uv + vec2<f32>(-r, 0.0), depth)
                + textureSampleCompareLevel(shadow_map, shadow_sampler, uv + vec2<f32>(0.0, r), depth)
                + textureSampleCompareLevel(shadow_map, shadow_sampler, uv + vec2<f32>(0.0, -r), depth);
            seen = seen * 0.5 + ring * 0.125;
        }
    }
    // **Faded toward the picture's rim as well as toward the radius.** The
    // picture is laid out over the ground at the player's height
    // (`shadow::LightView::around`), and a receiver far above or below slides
    // across it -- at dawn a hillside up-sun can slide off. Faded over the
    // last twelfth of the square, that is a shadow thinning out on a far
    // slope rather than one cut off along a line.
    let rim = 1.0 - max(abs(coord.x), abs(coord.y));
    let edge = (1.0 - smoothstep(globals.shadow_params.y, globals.shadow_params.z, view_distance))
        * smoothstep(0.0, 0.08, rim);
    return 1.0 - (1.0 - seen) * globals.shadow_params.x * edge;
}

// What the beam's floor comes down to where the shadows reach.
//
// **The half-Lambert floor is a stand-in for shadows, and here there are
// real ones.** Without a shadow map, a third of the sun on every face
// turned away from it is what keeps the north side of a hill from going
// black -- light from the sky and the ground, faked as a little of the
// sun. With the setting on, that same third was the *bottom* of every
// shadow: a shaded top came down only as far as a north face, which was
// already bright, and a noon shadow under a tree took at most sixty-seven
// levels of a pixel's brightness (`what_the_shadows_look_like`). The
// player looked at the pictures and at the game and said the shadows were
// too weak to see. Where the map reaches, the floor comes down to this,
// shade and turned-away faces alike -- so the two still match, and the
// sky's own fill (`SKY_SHARE`) is what lights them, as it is outdoors.
//
// Fifteen hundredths rather than nothing: the sky fill is a sixth of the
// light, and shade that is a sixth of sunlit ground under a bright sky is
// darker than the eye expects of a day. The floor is still blended back to
// `LAMBERT_FLOOR` by `shadow_reach`, so the edge of the map and the dusk
// fade are not a line where the world changes brightness.
const SHADOW_FLOOR: f32 = 0.15;

// How fully the shadows act here, 0..1: the day's strength (fading in at
// dawn, out under cloud) times the fade toward the map's edge -- the same
// two things `sunlit_share` scales the shadow by.
fn shadow_reach(view_distance: f32) -> f32 {
    return globals.shadow_params.x
        * (1.0 - smoothstep(globals.shadow_params.y, globals.shadow_params.z, view_distance));
}

// The face's share of the beam once the shadow has had its say.
//
// Rebuilt from the cosine the vertex's half-Lambert was made of, so a face
// full in the sun still gets all of it and one turned away gets the floor,
// with the floor where the shadows reach at `SHADOW_FLOOR` and elsewhere
// at `LAMBERT_FLOOR` -- the ordinary pipelines' answer, exactly, where the
// reach is nothing.
//
// **Skipped where it cannot matter**: a face turned away from the sun
// takes the floor without sampling the map, and a fragment with no sky
// light gets no beam at all -- a cave, the inside of a house -- and keeps
// what it had.
//
// **`at` is where the terrain looks for the sun at the Hard step** (its
// `lamp_point`), and `voxels` whether to: the models' entry points have no
// point in the volume to walk from and read the map at every step.
fn shadowed_lambert(lambert: f32, sky_level: f32, coord: vec3<f32>, view_distance: f32, at: vec3<f32>, voxels: bool) -> f32 {
    let least = shadow_least(lambert, sky_level, view_distance);
    // **No sun casting here** -- no sky, night, a closed storm deck, past
    // the map's radius -- and `shadow_least` has answered `lambert` itself:
    // what the rest of this would work out to exactly, after a lookup into a
    // map that decides nothing. These entry points run at night now, for the
    // fires' shadows, so the lookup is skipped rather than paid on every
    // sky-lit pixel of a dark world.
    if (least >= lambert) {
        return least;
    }
    let beam = (lambert - LAMBERT_FLOOR) / (1.0 - LAMBERT_FLOOR);
    if (beam <= 1e-4) {
        return least;
    }
    var share: f32;
    if (voxels && globals.shadow_bias.w > 0.5) {
        share = sun_visibility(at, coord, view_distance);
    } else {
        share = sunlit_share(coord, view_distance);
    }
    return least + (1.0 - least) * beam * share;
}

// What `shadowed_lambert` keeps whatever the shadow says: the floor of the
// beam, which stands for the light off the sky and the ground rather than
// the sun (see `SHADOW_FLOOR`). `lambert` itself where no shadow is cast --
// no sky over the face, or no reach -- so that a caller can tell the part
// of a face's light the sun's beam gave it from the part it would have
// had anyway. `sun_sky` needs exactly that split.
fn shadow_least(lambert: f32, sky_level: f32, view_distance: f32) -> f32 {
    let reach = shadow_reach(view_distance);
    if (sky_level <= 0.0 || reach <= 0.0) {
        return lambert;
    }
    let beam = (lambert - LAMBERT_FLOOR) / (1.0 - LAMBERT_FLOOR);
    // A face turned away from the sun keeps `AWAY_FLOOR`; one the sun is
    // on, and something stands in front of, comes down to `SHADOW_FLOOR`.
    // The two meet smoothly over the first `AWAY_BLEND` of the beam, so a
    // wall the sun comes round onto does not step darker as it starts to
    // be lit.
    let kept = mix(AWAY_FLOOR, SHADOW_FLOOR, smoothstep(0.0, AWAY_BLEND, beam));
    return min(mix(LAMBERT_FLOOR, kept, reach), lambert);
}

// **What a face turned away from the sun keeps where the shadows reach.**
//
// It was `SHADOW_FLOOR`, the darkness of a shadow, and the report was the
// contrast: a sunlit top almost white against dark sides, on pale stone at
// 13:24. Measured on the step scene (`what_a_step_casts_and_a_pale_wall_is_lit`),
// a shaded side came out at 0.55 of its top with shadows on against 0.70
// without them -- turning shadows on made every block harder, not only the
// ground under a tree darker. The two are not the same light. A floor in a
// shadow has the thing casting it over it; a wall turned from the sun has
// half the sky in front of it and the lit ground at its foot throwing light
// back up. So the side keeps a quarter of the beam and a cast shadow still
// comes down to fifteen hundredths: the shadows are as dark as the player
// asked for, and a block is not.
const AWAY_FLOOR: f32 = 0.25;

// Over how much of the beam a face goes from `AWAY_FLOOR` to `SHADOW_FLOOR`
// as the sun comes round onto it. A step at nought would light a wall in
// shade darker the moment the sun touched it than a moment before.
const AWAY_BLEND: f32 = 0.3;

// ---------------------------------------------------------------------
// The fires' shadows. See `engine::lamp_shadow` for the design and what it
// was chosen over; `lamp_ray_clear` is a line-for-line copy of
// `Volume::ray_is_clear` there, and the tests of that are the tests of this.
// ---------------------------------------------------------------------

// Must match `lamp_shadow::SIDE` and `lamp_shadow::MAX_STEPS`
// (`the_shader_walks_the_grid_this_file_fills`).
const LAMP_SIDE: f32 = 64.0;
const LAMP_STEPS: i32 = 32;

// What a fire's light keeps on a face that cannot see the fire.
//
// **Not nought.** The flood fill has already walked the light round what is
// in the way, and what it brought stands for the light off the walls and the
// ground: the floor behind a pillar in a lit room is not black. Three tenths
// keeps a shadow a shadow at a glance -- about as dark against the lit floor
// as the sun's `SHADOW_FLOOR` leaves shade against a meadow -- without making
// the back of every wall in a camp a cave.
const LAMP_BLOCKED_SHARE: f32 = 0.3;

// How far in front of its face a fragment looks for fires from. A face lies
// exactly on a cell boundary and a fragment's cell is `floor` of where it is,
// so a point on the face itself is in the wall half the time; a twentieth
// out, it is always in the air the face looks into.
const LAMP_LIFT: f32 = 0.05;

// Soft shadows aim at four corners of a cube this far round the middle of
// the fire's cell -- **out into the cells beside it, and it was inside**.
//
// At 0.3 all four targets lay in the fire's own cell, and the walk
// (`lamp_ray_clear`) goes by cells: four rays ending in one cell cross almost
// the same cells on the way, so they agreed nearly everywhere and the "soft"
// shadow had the sharp one's edge ("тени от огня выглядят острыми, хотя
// выключены мягкие"). At 0.9 each ray ends in a different neighbouring cell
// and takes its own path past a post, so the edge is a penumbra about a block
// wide -- the width a camp fire's flames and glow actually throw. The sharp
// setting is still the one ray to the middle.
const LAMP_FLAME: f32 = 0.9;

// The two cuts a cell's byte is read through: over `LAMP_STOPS` it stops a
// fire's light (`lamp_shadow::STOPS_ALL`, a whole block), over `SUN_STOPS` it
// stops the sun (`STOPS_SUN` as well: leaves and thick wood).
const LAMP_STOPS: f32 = 0.75;
const SUN_STOPS: f32 = 0.25;
// ...and over `MAP_STOPS` and not over `SUN_STOPS` a model is in the cell
// (`lamp_shadow::ASKS_THE_MAP`): the walk cannot say what its boxes cover, and
// hands the ray to the map, which drew them.
const MAP_STOPS: f32 = 0.1;

// Must match `lamp_shadow::MAX_SUN_COLUMNS`.
const SUN_COLUMNS: i32 = 32;

// The row over the highest thing that casts in a column, counted from the
// volume's floor, and the row over the lowest; nought and nought for a
// column with nothing in it. See `lamp_shadow::Volume::lows`.
fn lamp_column(column: vec2<i32>) -> vec2<i32> {
    return vec2<i32>(round(textureLoad(lamp_heights, column, 0).rg * 255.0));
}

// Whether a cell, counted from the volume's corner, stops a fire's light.
// Nothing outside the volume does.
fn lamp_stops(cell: vec3<i32>) -> bool {
    if (any(cell < vec3<i32>(0)) || any(cell >= vec3<i32>(i32(LAMP_SIDE)))) {
        return false;
    }
    return textureLoad(lamp_cells, cell, 0).r > LAMP_STOPS;
}

// Where a ray from `start`, counted from the volume's corner, toward the sun
// ends: 0 in a cell that casts, 1 above everything in the volume that casts,
// 2 when it cannot tell -- out of a side, or out of steps. `Volume::sun_ray`,
// line for line, with its tests.
//
// **Wood does not shade the wood it grows from.** The bark of a post lies
// inside its own cell, so a ray from a trunk's side started in a cell that
// casts and went straight up into the next piece of the same trunk: every
// trunk was in its own shadow on the side facing the sun. While the ray is in
// its starting column, a cell short of a whole block is passed through if the
// start was one as well.
//
// **Walked a column at a time, not a cell at a time.** The first walk stepped
// into every cell the ray crossed and looked each one up, and an open meadow
// at noon is a ray rising twenty cells to clear the highest crown in the
// volume: twenty lookups on every sunlit pixel. Measured with the savanna
// scene of `what_the_lighting_costs` at 1920x1080, the Hard step's frame was
// 5.8 ms against the map's 2.9 at noon, and 10.8 against 4.0 with the sun low
// behind the camera. So the walk crosses columns, works out the rows the ray
// is at while it is over each one, and looks up only the rows under that
// column's own height (`lamp_heights`); a column the ray is above costs one
// lookup, and the walk ends as soon as the ray is above the whole volume.
fn sun_ray(start: vec3<f32>) -> u32 {
    let side = i32(LAMP_SIDE);
    let origin = vec3<i32>(floor(start));
    if (any(origin < vec3<i32>(0)) || any(origin >= vec3<i32>(side))) {
        return 2u;
    }
    let dir = -globals.sun.xyz;
    if (dir.y <= 0.0) {
        return 2u;
    }
    let own = textureLoad(lamp_cells, origin, 0).r;
    if (own > MAP_STOPS && own <= SUN_STOPS) {
        return 2u;
    }
    let from_caster = own > SUN_STOPS;
    let top = i32(lamps.grid.y) - 1;
    var column = origin.xz;
    let flat_dir = dir.xz;
    let stride = vec2<i32>(sign(flat_dir));
    let inverse = 1.0 / max(abs(flat_dir), vec2<f32>(1e-6));
    let base = floor(start.xz);
    var t_max = select(start.xz - base, base + 1.0 - start.xz, flat_dir > vec2<f32>(0.0)) * inverse;
    var t_enter = 0.0;
    for (var n = 0; n < SUN_COLUMNS; n = n + 1) {
        let t_exit = min(t_max.x, t_max.y);
        let y_enter = i32(floor(start.y + dir.y * t_enter));
        if (y_enter > top) {
            return 1u;
        }
        let y_exit = i32(floor(start.y + dir.y * t_exit));
        let own_column = all(column == origin.xz);
        let bounds = lamp_column(column);
        let last = min(min(y_exit, bounds.x - 1), side - 1);
        // Started at the lowest thing in the column, not at the row the ray
        // came in on: under a crown that is twenty cells of air looked up
        // one at a time, on every sunlit pixel of the ground in a wood.
        // `lamp_shadow::Volume::lows` and its copy of this walk.
        for (var y = max(max(y_enter, 0), bounds.y - 1); y <= last; y = y + 1) {
            if (own_column && y == origin.y) {
                continue;
            }
            let stops = textureLoad(lamp_cells, vec3<i32>(column.x, y, column.y), 0).r;
            if (stops > MAP_STOPS && stops <= SUN_STOPS) {
                return 2u;
            }
            if (stops > SUN_STOPS && !(from_caster && stops < LAMP_STOPS && own_column)) {
                return 0u;
            }
        }
        if (y_exit > top) {
            return 1u;
        }
        if (t_max.x < t_max.y) {
            column.x = column.x + stride.x;
            t_max.x = t_max.x + inverse.x;
        } else {
            column.y = column.y + stride.y;
            t_max.y = t_max.y + inverse.y;
        }
        t_enter = t_exit;
        if (any(column < vec2<i32>(0)) || any(column >= vec2<i32>(side))) {
            return 2u;
        }
    }
    return 2u;
}

// How much of the sun reaches a terrain fragment at the Hard step: the
// volume's walk where it can tell, and the map where it cannot. See
// `engine::lamp_shadow`, "The sun, at the Hard step".
fn sun_visibility(at: vec3<f32>, coord: vec3<f32>, view_distance: f32) -> f32 {
    if (lamps.grid.x > 0.5) {
        let ray = sun_ray(at - lamps.volume.xyz);
        if (ray == 0u) {
            return 1.0 - globals.shadow_params.x;
        }
        if (ray == 1u) {
            return 1.0;
        }
    }
    return sunlit_share(coord, view_distance);
}

// 1 if the line from `start` to `goal`, both counted from the volume's
// corner, reaches the cell `goal` is in without entering a cell that stops
// light; 0 if it does not. A grid walk -- see `Volume::ray_is_clear` for the
// rules at its ends and its ties, which have to be the same here. (`from` and
// `to` there; `from` is a word WGSL keeps for itself.)
fn lamp_ray_clear(start: vec3<f32>, goal: vec3<f32>) -> f32 {
    let dir = goal - start;
    var cell = vec3<i32>(floor(start));
    let end = vec3<i32>(floor(goal));
    let stride = vec3<i32>(sign(dir));
    let inverse = 1.0 / max(abs(dir), vec3<f32>(1e-6));
    let base = floor(start);
    var t_max = select(start - base, base + 1.0 - start, dir > vec3<f32>(0.0)) * inverse;
    for (var n = 0; n < LAMP_STEPS; n = n + 1) {
        if (all(cell == end)) {
            return 1.0;
        }
        var axis = 2;
        if (t_max.x < t_max.y && t_max.x < t_max.z) {
            axis = 0;
        } else if (t_max.y < t_max.z) {
            axis = 1;
        }
        if (t_max[axis] > 1.0) {
            return 1.0;
        }
        cell[axis] = cell[axis] + stride[axis];
        t_max[axis] = t_max[axis] + inverse[axis];
        if (all(cell == end)) {
            return 1.0;
        }
        if (lamp_stops(cell)) {
            return 0.0;
        }
    }
    return 1.0;
}

// Where a Soft ray aims at a fire: `LAMP_FLAME` off its middle, **unless that
// is inside something that stops light**, and then at the middle itself.
// `Volume::flame_aim`, line for line.
//
// A fire stands on a floor. Two of the four corners are below its middle, in
// the row under it -- the ground -- and a ray walked to a point inside the
// ground crosses the ground on the way: the floor round every hearth and the
// walls of the room it was in lost half the fire's light, in wedges, the
// moment the Soft step was chosen (`what_layers_plants_and_caves_cast`,
// `cave_room`). The same held for a fire against a wall and glowstone in a
// ceiling. A corner in rock is not a part of the flame anything can see.
fn flame_aim(flame: vec3<f32>, off: vec3<f32>) -> vec3<f32> {
    let aim = flame + off;
    if (lamp_stops(vec3<i32>(floor(aim)))) {
        return flame;
    }
    return aim;
}

// How much of its block light a point keeps once the fires round it have
// had their say: 1 where the brightest fire in reach is in view,
// `LAMP_BLOCKED_SHARE` where no fire is.
//
// A fire's weight here is what the flood fill's own rule leaves of it -- its
// level less the cells between, counted along the axes -- and the brightest
// wins, as it does in the flood fill: the share is the brightest fire in view
// against the brightest in reach. A fire that could not beat one already
// seen is not walked to at all, which is most of them in a camp of several.
fn lamp_visibility(at: vec3<f32>) -> f32 {
    let count = u32(lamps.volume.w);
    let local = at - lamps.volume.xyz;
    if (count == 0u || any(local < vec3<f32>(1.0)) || any(local >= vec3<f32>(LAMP_SIDE - 1.0))) {
        return 1.0;
    }
    let soft = globals.shadow_bias.w < 0.5;
    var reached = 0.0;
    var seen = 0.0;
    for (var i = 0u; i < count; i = i + 1u) {
        let lamp = lamps.lamps[i];
        let flame = lamp.xyz - lamps.volume.xyz;
        let apart = abs(floor(local) - floor(flame));
        let reach = lamp.w - (apart.x + apart.y + apart.z);
        reached = max(reached, reach);
        if (reach <= seen) {
            continue;
        }
        var clear: f32;
        if (soft) {
            clear = 0.25 * (lamp_ray_clear(local, flame_aim(flame, vec3<f32>(LAMP_FLAME, LAMP_FLAME, LAMP_FLAME)))
                + lamp_ray_clear(local, flame_aim(flame, vec3<f32>(LAMP_FLAME, -LAMP_FLAME, -LAMP_FLAME)))
                + lamp_ray_clear(local, flame_aim(flame, vec3<f32>(-LAMP_FLAME, LAMP_FLAME, -LAMP_FLAME)))
                + lamp_ray_clear(local, flame_aim(flame, vec3<f32>(-LAMP_FLAME, -LAMP_FLAME, LAMP_FLAME))));
        } else {
            clear = lamp_ray_clear(local, flame);
        }
        seen = max(seen, reach * clear);
    }
    if (reached <= 0.0) {
        return 1.0;
    }
    return mix(LAMP_BLOCKED_SHARE, 1.0, seen / reached);
}

// The sky the sun's share is scaled by, where real shadows are cast.
//
// **The flood fill's sky is a shadow of its own, and a second one.** Sky light
// is walked down from the top of the world and spread sideways a level a
// cell, so under a roof, beside a cliff or near a crown it fades in a soft
// patch straight *under* the thing -- and the sun's term was multiplied by
// it. With shadows on, a roof then darkened the sand twice: a soft patch
// under itself from the flood fill, and its real shadow off along the beam,
// and sand the sun did reach under the roof's edge still came out dim. The
// report was "освещение ванильное и новые тени конфликтуют"; the overhang in
// `what_uneven_sand_looks_like` is the picture. Where a shadow decides whether
// the sun arrives (`shadow_reach`), a face with any sky over it takes the
// whole beam the shadow lets through; the sky's own fill (`SKY_SHARE`) still
// follows the flood fill, which is what a roof really does to the light from
// the rest of the sky. Past the map's reach and at night it is the flood
// fill's sky exactly, so the ordinary pipelines and these agree there.
//
// **Only the beam, and not the floor under it.** `lambert` here is
// `shadowed_lambert`'s: the floor (`shadow_least`) and, over it, what the
// shadow let through. The floor is not sunlight -- it stands for the light
// off the sky and the ground, which is what a roof really does take away --
// and it was scaled by the whole sky with the beam. In a cave that is the
// difference between dark and lit: a room reached by a tunnel has a sky
// level of a few sixteenths, the walk finds rock over every fragment of it,
// and the floor of fifteen to twenty-five hundredths of the *full* sun then
// lit every wall of it, where the frame without shadows gave it that floor
// times its few sixteenths. Turning shadows on made caves brighter
// ("в пещерах проблемы с тенями"; `what_layers_plants_and_caves_cast`,
// `cave_dark`: the dead end of a gallery grey where the frame without shadows
// is black, and a room with no fire in it -- the first photograph -- 409
// thousand pixels of 922 lighter at dusk). So the floor takes the
// flood fill's sky, as the ordinary pipelines give it, and what the sun
// adds over it takes the whole sky wherever the sun gets through.
// `the_shadow_floor_of_a_cave_follows_its_sky_and_not_the_open_sky` holds it.
fn sun_sky(in: VertexOutput, lambert: f32) -> f32 {
    let sky = in.light_terms.x;
    let open = mix(sky, step(1e-4, sky), shadow_reach(in.view_distance));
    let least = shadow_least(in.lambert, sky, in.view_distance);
    return (sky * least + open * max(lambert - least, 0.0)) / max(lambert, 1e-4);
}

// `in` with its block light cut to what the fires round it can see, and
// untouched where it has none -- which is all of a day's screen away from a
// camp, and costs one comparison there.
fn lamp_lit(in: VertexOutput, at: vec3<f32>) -> VertexOutput {
    if (in.light_terms.y <= 0.0) {
        return in;
    }
    var lit = in;
    lit.light_terms.y = in.light_terms.y * lamp_visibility(at);
    return lit;
}

// `fs_solid`, shadowed. The hole shading is `fs_solid`'s, and the note
// on why it exists is there.
@fragment
fn fs_solid_shadowed(
    in: VertexOutput,
    @location(10) shadow_coord: vec3<f32>,
    @location(11) lamp_point: vec3<f32>,
) -> @location(0) vec4<f32> {
    let sampled = sample_block(in.uv, in.tex_layer);
    let solid_hole = step(sampled.a, ALPHA_CUTOFF);
    let filled = vec4<f32>(sampled.rgb * mix(1.0, HOLE_SHADE, solid_hole), sampled.a);
    let lambert = shadowed_lambert(in.lambert, in.light_terms.x, shadow_coord, in.view_distance, lamp_point, true);
    return shade_lit_sky(lamp_lit(in, lamp_point), filled, lambert, sun_sky(in, lambert), cell_footprint(in.uv));
}

// `fs_cutout`, shadowed: leaves, grass, and the animals.
@fragment
fn fs_cutout_shadowed(
    in: VertexOutput,
    @location(10) shadow_coord: vec3<f32>,
    @location(11) lamp_point: vec3<f32>,
) -> @location(0) vec4<f32> {
    let sampled = sample_cutout(in.uv, in.tex_layer);
    // Before the cut-out throws anything away: see `shade`.
    let cell_px = cell_footprint(in.uv);
    if (sampled.a < ALPHA_CUTOFF) {
        discard;
    }
    let lambert = shadowed_lambert(in.lambert, in.light_terms.x, shadow_coord, in.view_distance, lamp_point, true);
    return shade_lit_sky(lamp_lit(in, lamp_point), sampled, lambert, sun_sky(in, lambert), cell_px);
}

// The casters: the world drawn from the sun, depth only.
@vertex
fn vs_shadow(in: VertexInput) -> @builtin(position) vec4<f32> {
    return globals.shadow_view_proj * vec4<f32>(in.position + in.chunk_offset.xyz, 1.0);
}

struct ShadowCutoutOutput {
    @builtin(position) clip_position: vec4<f32>,
    @location(0) uv: vec2<f32>,
    @location(1) @interpolate(flat) tex_layer: u32,
};

// Leaves and animals, which need their picture to know where they have
// holes. The coordinate comes from `terrain_vertex` so a cropped model
// face is cut out by the same piece of picture it is drawn with.
@vertex
fn vs_shadow_cutout(in: VertexInput) -> ShadowCutoutOutput {
    let v = terrain_vertex(in);
    var out: ShadowCutoutOutput;
    out.clip_position = globals.shadow_view_proj * vec4<f32>(in.position + in.chunk_offset.xyz, 1.0);
    // **A face that looks at the sun is drawn behind itself.** These are
    // drawn from both sides -- `shadow::LEAF_CASTER_CULL` says why a canopy
    // let the light through when they were not -- and a face turned toward
    // the sun is a receiver as well: at its own depth it would be compared
    // against itself and shade itself in a moire. Pushed most of a cell
    // away from the sun (`shadow_bias.z`, already in depth, where an
    // orthographic depth is linear and a constant is a distance), it meets
    // the comparison a solid block's lit face meets against the block's far
    // side. Only the depth moves, so the shadow's outline does not.
    //
    // Against the true sun rather than the stepped one the picture is taken
    // along: the two are a twentieth of a degree apart, and a face whose
    // answer differs between them is edge-on to the beam and casts nothing.
    let normal = face_normal(((in.packed & LIGHT_MASK) >> 10u) & 7u);
    if (dot(normal, globals.sun.xyz) < 0.0) {
        out.clip_position.z = out.clip_position.z + globals.shadow_bias.z;
    }
    out.uv = v.uv;
    out.tex_layer = v.tex_layer;
    return out;
}

// Grass, flowers, ferns and crops, when the player asks for their shadows
// (`shadow::PlantShadows::All`).
//
// **Not `vs_shadow_cutout`.** That pushes a face that looks at the sun most of
// a cell behind itself, which is right for a leaf cell a block deep and wrong
// for a tuft: a sprite's two planes carry the up face whatever way they stand
// (`mesh::cross_block`), so at noon every one of them would be pushed three
// quarters of a block down -- under the grass it stands on -- and cast
// nothing. A tuft is a plane with nothing behind it, so it is pushed only as
// far as keeps a blade from being compared against its own depth.
//
// **How far that is depends on the sun, and a fixed push lifted every shadow
// off its plant.** The picture's depth is height (`shadow::LightView::around`),
// so a push is a drop in height, and a blade that stands less than the push
// above the ground (plus the receivers' own lift, `shadow_bias.x`, which the
// ground's lookup climbs by and the shadow's near end slides back by) casts
// nothing: the shadow began that height times `cot(elevation)` out from the
// foot. It was a sixth of the leaves' push, an eighth of a block, always --
// half a block of bare sand between a fireweed and its shadow at the golden
// hour and a whole block at dusk, and a tuft's shadow a scrap lying on its
// own well away from it (`what_layers_plants_and_caves_cast`, `sand_plot`).
//
// What a blade needs is set by the one texel the Hard step reads: a point
// lands up to half a texel's diagonal from the texel's middle, where its own
// plane was drawn, and across that a plane standing in the beam changes
// height by the distance over `cot(elevation)` -- a lot at noon, almost
// nothing with the sun low. So the push is the receivers' lift times
// `tan(elevation)`: the gap it leaves at the foot is then the same lift at
// every hour, a texel and a half, and at noon, where a steep sun makes the
// most of every texel's error, the push is larger than the old eighth.
// Capped at the leaves' push, which is what a sun straight overhead would
// otherwise run it past.
const LEAF_PUSH_BLOCKS: f32 = 0.75;
@vertex
fn vs_shadow_plant(in: VertexInput) -> ShadowCutoutOutput {
    let v = terrain_vertex(in);
    var out: ShadowCutoutOutput;
    out.clip_position = globals.shadow_view_proj * vec4<f32>(in.position + in.chunk_offset.xyz, 1.0);
    let rise = max(-globals.sun.y, 1e-3);
    let run = max(length(globals.sun.xz), 1e-3);
    let push = min(globals.shadow_bias.x * rise / run, LEAF_PUSH_BLOCKS);
    // `shadow_bias.z` is `LEAF_PUSH_BLOCKS` in depth: the ratio turns blocks
    // of height into depth.
    out.clip_position.z = out.clip_position.z + push * globals.shadow_bias.z / LEAF_PUSH_BLOCKS;
    out.uv = v.uv;
    out.tex_layer = v.tex_layer;
    return out;
}

// The sprites' range holds the flames as well (`mesh::flame_block`), and a
// fire throws light, not a shadow: the animated run is only ever the fire's
// frames (`animated`), so a layer in it is left out of the picture.
@fragment
fn fs_shadow_plant(in: ShadowCutoutOutput) {
    let first = u32(globals.anim.x);
    if (in.tex_layer >= first && in.tex_layer < first + u32(globals.anim.w)) {
        discard;
    }
    let alpha = textureSampleLevel(block_textures, block_sampler, in.uv, i32(in.tex_layer), 1.0).a;
    if (alpha < ALPHA_CUTOFF) {
        discard;
    }
}

// **The second level of the mip chain, not the first.** A leaf picture is
// sixteen texels a block and the shadow map about ten, so the full-size
// holes were sampled below their own spacing: each time the picture is
// taken at a new sub-texel phase -- a step of the sun, a new centre -- a
// different half of them survived, and the dapple under a tree changed
// pattern rather than position. Eight texels a block is under the map's
// density, so a hole is drawn as the same hole every time. The cutout keeps
// the same share of every level as of the full picture
// (`texture::held_to_coverage`), so the crown is no denser or thinner for
// it. A fixed level because the depth pass has no screen to take a
// derivative from.
@fragment
fn fs_shadow_cutout(in: ShadowCutoutOutput) {
    let alpha = textureSampleLevel(block_textures, block_sampler, in.uv, i32(animated(in.tex_layer)), 1.0).a;
    if (alpha < ALPHA_CUTOFF) {
        discard;
    }
}
