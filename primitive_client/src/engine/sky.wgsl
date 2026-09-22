// The sky: gradient, sun, moon, stars and cloud.
//
// ## No geometry
//
// One triangle covering the screen, and everything else worked out per
// fragment from the direction that pixel is looking in. A skybox would
// need a cube, six textures and a seam at every edge of it; a dome would
// need geometry that has to be tessellated finely enough that the sun is
// round. Here the sun *is* round, because it is a distance test against
// a direction rather than a shape someone had to build.
//
// ## No overdraw
//
// Drawn after the terrain with the depth test on and the sky's own depth
// pinned to the far plane, so it survives only where nothing was drawn.
// The alternative -- sky first, terrain over it -- shades every pixel of
// the sky and then throws most of it away, and standing in a cave that
// is the whole screen wasted.
//
// ## What the time of day drives
//
// Everything. The sun rides the same circle the lighting uses, the moon
// sits opposite it, the stars are fixed to a sphere that turns with
// them, and each fades in and out with how much daylight is left. There
// is one clock, it comes from the server, and nothing here has an
// animation of its own -- so every player sees the same sky at the same
// moment, which is the whole reason the clock is server-side.

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
    // Turns a clip-space point back into a world-space direction. The
    // whole pass rests on this: without it a fragment knows where it is
    // on the screen and nothing about where it is looking.
    inv_view_proj: mat4x4<f32>,
    // x: time of day 0..1, y: how cloudy, z: seconds since start,
    // w: how overcast the weather has made it
    sky_params: vec4<f32>,
    // Where the frame's origin is in the world. Only the cloud layer
    // wants it: everything else here is happier measured from the eye,
    // and a cloud has to stay over the same field as the origin moves
    // under it.
    render_origin: vec4<f32>,
    // Declared and unread, down to the two this shader wants: a uniform
    // block is an offset table, and leaving a field out shifts every one
    // after it. See `shader.wgsl`, which reads them.
    hand_view_proj: mat4x4<f32>,
    anim: vec4<f32>,
    sun_color: vec4<f32>,
    fill_color: vec4<f32>,
    shadow_view_proj: mat4x4<f32>,
    shadow_params: vec4<f32>,
    shadow_bias: vec4<f32>,
    // The sunset and the halo. See `glow_over` below.
    horizon_glow: vec4<f32>,
    sun_haze: vec4<f32>,
    // The sun's compass bearing, flat and unit length, in x and z. See
    // `glow_lobe`. y and w are the cloud deck's drift -- see the cloud below.
    glow_dir: vec4<f32>,
    // The moon: xyz the direction its light travels, w the night floor's
    // scale less one. See the moon below, and `Sky::moon_uniform`.
    moon: vec4<f32>,
};

// Which lighting step this module was compiled for. See the same line in
// `shader.wgsl`; the two are rewritten together.
const LIGHTING: u32 = 0u;

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

@group(0) @binding(0)
var<uniform> globals: Globals;

// The cloud field, as a picture. See `texture::CloudTexture` and
// `gen_placeholder_textures::generate_clouds`.
@group(1) @binding(0)
var cloud_texture: texture_2d<f32>;
@group(1) @binding(1)
var cloud_sampler: sampler;

struct SkyOut {
    @builtin(position) clip_position: vec4<f32>,
    // This pixel's point on the far plane, still homogeneous, worked
    // out **once per corner** instead of once per pixel.
    //
    // The fragment shader used to be handed the screen position and ask
    // the inverse matrix which way it was looking -- two 4x4 multiplies
    // and two divides, on every pixel of sky. Both are avoidable, and
    // for different reasons.
    //
    // The near-plane point was avoidable because it was never needed:
    // the eye is on the same ray, and `camera_pos` is already a uniform,
    // so the direction is `far - eye` and the near plane never comes
    // into it.
    //
    // The matrix itself is avoidable because unprojecting the far plane
    // is *affine* in the screen position, and this triangle is drawn
    // with a constant clip w -- so what the rasterizer interpolates
    // between the three corners is exactly what the multiply would have
    // produced. The perspective divide stays per-pixel, because that
    // part is not affine.
    @location(0) far_point: vec4<f32>,
};

// One triangle rather than two, so there is no diagonal seam down the
// middle where the two halves meet and no fragment is shaded twice.
@vertex
fn vs_sky(@builtin(vertex_index) index: u32) -> SkyOut {
    var out: SkyOut;
    let x = f32((index << 1u) & 2u) * 2.0 - 1.0;
    let y = f32(index & 2u) * 2.0 - 1.0;
    out.far_point = globals.inv_view_proj * vec4<f32>(x, y, 1.0, 1.0);
    // z = 1: the far plane, so the depth test keeps this only where
    // nothing nearer was drawn.
    out.clip_position = vec4<f32>(x, y, 1.0, 1.0);
    return out;
}

// A hash good enough for stars: uncorrelated, cheap, and the same every
// frame so a star does not twinkle by accident.
fn hash3(p: vec3<f32>) -> f32 {
    let q = fract(p * vec3<f32>(0.1031, 0.1030, 0.0973));
    let r = q + dot(q, q.yxz + 33.33);
    return fract((r.x + r.y) * r.z);
}

/// **The sine stays, and it was measured.**
///
/// This is the hash the cloud layer is built out of, and up to
/// forty-one of them are taken per pixel of cloud -- the warp is two
/// `noise2`, the density a `fbm2`, the self-shadowing another, and each
/// `noise2` is four hashes. That count is exactly the argument for
/// replacing `fract(sin(...))` with one of the sine-free mixes, which
/// is received wisdom old enough to be quoted without checking.
///
/// It was checked. The replacement -- the same multiply-dot-fract trick
/// `hash3` above uses -- moved the sky stage from 0.83 ms to **1.08**
/// at an identical viewpoint (`solid` 0.145 against 0.143, the same two
/// hundred draws). Thirty per cent slower.
///
/// The reason is the hardware rather than the arithmetic. A sine here
/// is one instruction on a unit that exists to do nothing else, and it
/// runs alongside the arithmetic pipeline rather than in it; the
/// "cheap" hash is fifteen-odd ordinary operations that queue up behind
/// everything else the shader is already doing. Trading one special
/// instruction for fifteen general ones is only a win where the special
/// one is emulated, and on this card it is not.
///
/// The sky is still the most expensive thing in the frame. What it
/// needs is fewer pixels, not cheaper ones.
fn hash2(p: vec2<f32>) -> f32 {
    return fract(sin(dot(p, vec2<f32>(127.1, 311.7))) * 43758.5453);
}

/// How wide one cloud pixel is, in the units the cloud layer is sampled
/// in (the layer scales world metres by 0.0022, so this is about twelve
/// blocks square). Big enough to read as a deliberate pixel from the
/// ground, small enough that a cloud is still made of several.
const CLOUD_PIXEL: f32 = 0.026;

/// How much of this layer's coordinate space one tile of the cloud
/// picture covers. Matches `CLOUD_TILE` in the generator: get the two
/// out of step and a cloud is the wrong size.
const CLOUD_TILE: f32 = 6.0;

/// The cloud field.
///
/// **Was four octaves of value noise computed here**, plus a second
/// stack for the warp and a third for the self-shadowing: about forty
/// hashes on every pixel of sky, in the one pass that runs for every
/// pixel the terrain did not cover. It is one fetch, and the field is a
/// file somebody can open and repaint.
///
/// `.r` is the density the threshold below cuts; `.g` and `.b` are the
/// two fields that bend a cloud's outline. One fetch brings back all
/// three, because a fetch brings back four numbers whether they are
/// wanted or not.
///
/// The picture carries the same distribution the noise had -- see the
/// generator, which explains at length why normalising it would ruin
/// the sky -- so every threshold below is the number it always was.
fn cloud_field(p: vec2<f32>) -> vec3<f32> {
    return textureSampleLevel(cloud_texture, cloud_sampler, p / CLOUD_TILE, 0.0).rgb;
}

/// How many squares across one face of the sky is.
///
/// **What this is worth in pixels, worked out rather than asserted.**
/// One face runs -1..1 in the projected coordinate, so it holds `2 *
/// STAR_GRID` cells; at the middle of a face a cell is `1 / STAR_GRID`
/// radians across, and toward its corner the same cell is squeezed to
/// about 0.47 of that, because a cube face is not equal-angle. A screen
/// `h` pixels tall at field of view `f` puts `(h / 2) / tan(f / 2)`
/// pixels in a radian. At 150, 1080 and 70 degrees that is a star of
/// 5.1 screen pixels in the middle of a face and 2.4 at its corner.
///
/// This comment used to claim a dozen-odd pixels, which was never true
/// of any size anybody plays at -- and the number matters, because the
/// whole reason the stars were rebuilt on cube faces was that a star
/// narrower than a pixel crawls between them when the camera turns. Two
/// pixels is the floor; `every_star_is_wide_enough_to_hold_still` in
/// `engine::renderer` does the arithmetic above and fails if a change
/// here takes a star under it.
///
/// It is a floor and not a comfortable margin. At 720p and a 95 degree
/// field of view the corner cell is 1.6 pixels and stars do crawl;
/// curing that means about 62 here, which is 2.4 times the star and a
/// fifth as many of them -- a different night sky, not a bug fix, so it
/// is written down rather than done.
const STAR_GRID: f32 = 150.0;

/// Which square of the sky a direction falls in.
///
/// **A cube face, not a lattice.** The stars used to be indexed by
/// `floor(dir * 90)`, which cuts direction space into cubes and then
/// asks where the unit sphere crosses them -- and the sphere clips some
/// of those cubes almost tangentially, so the cells it makes are wildly
/// unequal and some of them are slivers. A sliver is a star narrower
/// than a pixel, and a star narrower than a pixel crawls between them
/// whenever the camera turns.
///
/// Projecting onto the dominant axis first is the standard fix: every
/// cell is a square on one of six faces, they are all about the same
/// size on screen, and none of them is a sliver.
///
/// Returns the cell's two coordinates and which face it is on, which
/// together are what the hash is taken of.
fn star_cell(dir: vec3<f32>, grid: f32) -> vec3<f32> {
    let a = abs(dir);
    if (a.x >= a.y && a.x >= a.z) {
        let uv = dir.yz / max(a.x, 1e-6);
        return vec3<f32>(floor(uv * grid), select(1.0, 0.0, dir.x > 0.0));
    }
    if (a.y >= a.z) {
        let uv = dir.xz / max(a.y, 1e-6);
        return vec3<f32>(floor(uv * grid), select(3.0, 2.0, dir.y > 0.0));
    }
    let uv = dir.xy / max(a.z, 1e-6);
    return vec3<f32>(floor(uv * grid), select(5.0, 4.0, dir.z > 0.0));
}

@fragment
fn fs_sky(in: SkyOut) -> @location(0) vec4<f32> {
    // The direction this pixel is looking, in world space: the far
    // plane point the rasterizer handed over, less the eye. See
    // `SkyOut` for why neither the matrix nor the near plane is here.
    // Both of these are measured from the point the frame is drawn
    // around, and both are therefore small. **They used to be absolute
    // world coordinates**, and the subtraction of two seven-digit
    // numbers whose difference is a few hundred is where most of the
    // precision in this shader went: the reconstructed direction wobbled
    // by a fraction of a degree that changed every frame, which is the
    // sky visibly trembling.
    let dir = normalize(in.far_point.xyz / in.far_point.w - globals.camera_pos.xyz);

    let daylight = globals.sun.w;
    // The sun's *position*, which is the opposite of the direction its
    // light travels.
    let to_sun = -globals.sun.xyz;

    // How far the weather has come in, 0..1, eased on the CPU -- see
    // `Sky::overcast`. Read once here because four separate things in
    // this function want it: the stars go out behind cloud, the sun and
    // moon stop being discs, the deck spreads, and its colour drops
    // toward slate.
    let overcast = clamp(globals.sky_params.w, 0.0, 1.0);
    // What is left of a body in the sky once there is weather in front
    // of it. Not zero at the top: even a storm has a bright patch where
    // the sun is, and taking it to nothing makes a storm sky perfectly
    // flat -- which is the one thing an actual storm sky never is.
    let through_cloud = 1.0 - overcast * 0.88;

    // ---- the sky colour ----
    //
    // **One colour, not a gradient.** It used to run from the fog colour
    // at the horizon to a deeper blue at the zenith along `pow(up,
    // 0.65)`, so the sky's tint changed with how high in it you looked
    // -- and the player asked for that to stop: a sky that is one shade
    // above the horizon, the way the rest of this world is flat colour
    // with hard edges. What is kept is the band at the horizon, where
    // the sky still becomes the fog colour exactly, because that is what
    // the terrain fades into: any difference between the two shows up
    // as a visible line where the world ends. The band is narrow --
    // about seven degrees -- so it reads as haze on the horizon rather
    // than as the gradient coming back.
    let height = clamp(dir.y, -1.0, 1.0);
    let up = clamp(height, 0.0, 1.0);
    let sky_blue = globals.fog_color.rgb * vec3<f32>(0.62, 0.74, 1.06);
    var colour = mix(globals.fog_color.rgb, sky_blue, smoothstep(0.0, 0.12, up));
    // **The one gradient this sky has, and only while the sun is low.**
    // The flat colour above is what the player asked for and it is what
    // noon still is; the glow is zero there (`Sky::horizon_glow`). Under
    // a low sun it rises out of the horizon toward the sun's bearing and
    // is gone forty degrees up, so what reads is a sunset rather than the
    // old zenith gradient come back. At the horizon `colour` is exactly
    // the fog colour, so this is exactly what the terrain fades into.
    colour = glow_over(colour, dir);

    // Below the horizon there is no sky, only the haze the ground fades
    // into -- otherwise flying up and looking down shows a second sun.
    let above = smoothstep(-0.06, 0.02, height);

    // ---- stars ----
    //
    // Fixed to a sphere that turns with the day, so they rise and set
    // together with the sun instead of hanging in place.
    //
    // Sampled on a coarse grid of directions: one cell in twenty or so
    // holds a star, which is what keeps them points rather than noise.
    let spin = globals.sky_params.x * 6.28318;
    let cs = cos(spin);
    let sn = sin(spin);
    let turned = vec3<f32>(
        dir.x * cs - dir.y * sn,
        dir.x * sn + dir.y * cs,
        dir.z,
    );
    // **The cell is the star.** It used to be a soft point a fraction
    // of a cell wide, which at any resolution is smaller than a pixel
    // -- and a sub-pixel point is the one thing on a screen that cannot
    // hold still. It shimmered, and drawing the sky at half size and
    // stretching it doubled that.
    //
    // Now a star fills its square outright, hard-edged, every one the
    // same size. It matches the rest of the game, which is made of
    // squares, and it is stable for the reason a square is: it either
    // covers a pixel or it does not.
    let cell = star_cell(turned, STAR_GRID);
    let star_seed = hash3(cell);
    var stars = 0.0;
    if (star_seed > 0.9915) {
        // Brightness varies per star, and twinkles on the world clock
        // so everyone sees the same sky. Quantised to four steps: a
        // smooth fade on a hard-edged square is the one place the pixel
        // would have shown as an accident.
        let bright = 0.4 + 0.6 * hash3(cell + 7.0);
        let twinkle = 0.75 + 0.25 * sin(globals.sky_params.x * 900.0 + star_seed * 60.0);
        stars = floor(bright * twinkle * 4.0 + 0.5) / 4.0;
    }
    // Only at night, and never below the horizon.
    let night = clamp(1.0 - daylight * 2.2, 0.0, 1.0);
    // Cloud is what a starless night actually is. The deck below is
    // drawn *over* the stars, but only where it is thick enough to
    // survive the threshold, so without this a storm night is a field
    // of stars with holes punched in it.
    colour = colour + vec3<f32>(stars * night * above * through_cloud);

    // ---- the moon ----
    //
    // **Where the moon really is, lit from where the sun really is.** It used
    // to stand opposite the sun every night with its "phase" a second disc
    // swung across it on the hour -- `sin(time_of_day * 0.5)`, so the moon
    // changed shape during a night and was full on no night in particular. It
    // is placed by its month now (`Sky::moon_direction`,
    // `primitive_shared::moon`): full opposite the sun, new beside it, rising
    // later every night. The lit part is worked out rather than carved: each
    // pixel of the disc is a point on a sphere, lit where that point faces the
    // sun, so a crescent always points at the sun that lights it. The dark part
    // keeps a sixteenth of the light -- earthshine -- so a thin moon is still
    // a whole disc against the stars.
    var to_moon = -globals.moon.xyz;
    if (dot(to_moon, to_moon) < 0.5) {
        // A tool that filled the globals itself has no moon: opposite the
        // sun, where it always was.
        to_moon = globals.sun.xyz;
    }
    let moon_cos = dot(dir, to_moon);
    if (moon_cos > 0.995) {
        let d = acos(clamp(moon_cos, -1.0, 1.0));
        let radius = 0.055;
        let disc = smoothstep(radius, radius * 0.86, d);
        let across = normalize(cross(to_moon, vec3<f32>(0.0, 1.0, 0.0)));
        let upward = cross(across, to_moon);
        let offset = (dir - to_moon * moon_cos) / radius;
        let u = dot(offset, across);
        let v = dot(offset, upward);
        let facing = sqrt(max(1.0 - u * u - v * v, 0.0));
        let surface = across * u + upward * v - to_moon * facing;
        let sunward = smoothstep(-0.08, 0.08, dot(surface, to_sun));
        let lit = disc * mix(0.06, 1.0, sunward);
        colour = colour + vec3<f32>(0.86, 0.88, 0.95) * lit * night * above * through_cloud;
    }

    // ---- the sun ----
    //
    // A disc with a glow around it. The glow is what sells a sunset:
    // without it the sun is a sticker, and with it the whole quarter of
    // the sky it is in warms up.
    let sun_cos = dot(dir, to_sun);
    let glow = pow(clamp(sun_cos, 0.0, 1.0), 220.0);
    // The glow keeps rather more of itself than the disc does, because
    // that is what a sun behind weather looks like: no edge to it at
    // all, and a wide bright smear where it stands.
    let glow_through = 1.0 - overcast * 0.55;
    colour = colour + vec3<f32>(1.0, 0.72, 0.42) * glow * daylight * 0.8 * above * glow_through;
    if (sun_cos > 0.9975) {
        let d = acos(clamp(sun_cos, -1.0, 1.0));
        let disc = smoothstep(0.042, 0.032, d);
        colour = colour + vec3<f32>(1.0, 0.96, 0.86) * disc * above * through_cloud;
    }

    // ---- cloud ----
    //
    // A layer at a fixed height, sampled where the view ray crosses it.
    // Three things about it were wrong, and each of them was visible:
    //
    // * **It drifted on the world clock**, which wraps. Every midnight
    //   the whole sky snapped back to the pattern it had at the previous
    //   one. It now drifts on a clock that only counts up.
    // * **It ignored where the camera was.** The crossing point was
    //   worked out as though the eye were always at y = 0, so the layer
    //   hung the same distance overhead whether you were in a valley or
    //   on a peak, and climbing a mountain moved you no closer to it.
    //   Now it is a plane at a height in the world, and the ray is
    //   intersected with it properly -- which also means that from above
    //   it you look *down* on to it.
    // * **It was one octave of noise through a threshold**, so the edges
    //   were the edges of the noise: a smooth grey blob, thickest where
    //   it happened to peak. Real cloud has a base that is flat and a
    //   top that is not, and reads as depth mostly through the shadow
    //   one part of it casts on another. The density is now warped by a
    //   second field (which is what gives an edge its curl) and shaded
    //   by sampling it again a little way toward the sun -- if there is
    //   more cloud between this point and the light, this point is
    //   darker.
    // In *world* height and world x/z, which is what the origin is added
    // back for: a cloud belongs to the sky over a place, and a deck that
    // was measured from a moving origin would slide across the world
    // every time the origin caught up with the player.
    let eye = globals.camera_pos.xyz + globals.render_origin.xyz;
    let layer_height = 190.0;
    let below = eye.y < layer_height;
    // Toward the layer: up if it is over us, down if we have climbed
    // above it. Rays going the other way never reach it at all.
    let toward = select(-dir.y, dir.y, below);
    if (toward > 0.015) {
        let travel = abs(layer_height - eye.y) / max(toward, 1e-4);
        let at = (eye.xz + dir.xz * travel) * 0.0022;
        // **Carried by the wind, not by the clock** (`Sky::blow_the_clouds`):
        // it was `seconds` times one fixed slant, so a storm deck crawled
        // toward the same corner as a fair one whatever the rain below it
        // was slanting in. The CPU integrates the world's wind into this, so
        // a gale runs the deck across the sky and a calm nearly stops it.
        // The warp takes three times the drift, so the shapes deform as they
        // move rather than sliding past as a rigid sheet.
        let drift = vec2<f32>(globals.glow_dir.y, globals.glow_dir.w);

        // The warp is **one octave**, not a stack of them.
        //
        // Its whole job is to bend the outline of a cloud so the edge
        // curls instead of running smooth, and an octave at a sixteenth
        // of the amplitude cannot be seen through a threshold at all --
        // it moves the sample by less than the cut is wide. This is the
        // sky pass, which runs for every pixel the terrain did not
        // cover, so it is the one shader in the game where six noise
        // samples are worth counting: dropping them measured as a third
        // off the cost of the pass.
        let warp = cloud_field(at * 2.1 + drift * 3.0 + 11.7).gb;
        let smooth_at = at + drift + (warp - 0.5) * 0.55;

        // ---- the cloud is drawn on a grid ----
        //
        // Everything else in this world is made of cubes a metre across,
        // and the sky was the one surface with no scale to it at all: a
        // soft airbrushed smear over a world of hard edges, which is the
        // one thing that reads as *not belonging to the game* rather
        // than as weather.
        //
        // So the field is sampled on a grid and held constant across
        // each cell. The cell is a fixed size in the same units the
        // layer is sampled in, which makes it a fixed size in *metres*
        // -- roughly a dozen blocks square, the size that reads as a
        // deliberate pixel from the ground rather than as noise.
        //
        // The drift is applied before the snap, not after, so the whole
        // grid slides across the sky a cell at a time instead of the
        // clouds sliding through a stationary grid: the pattern moves,
        // its pixels stay square.
        let sample_at = (floor(smooth_at / CLOUD_PIXEL) + 0.5) * CLOUD_PIXEL;
        let density = cloud_field(sample_at).r;

        // `cloudiness` decides where the cut is: at 0 nothing survives
        // the threshold, at 1 most of it does.
        //
        // The cut is nearly hard now. It was a wide fade, which is what
        // a smooth field wants -- but across a grid the fade only ever
        // lands *within* one cell, so all it does is make some pixels
        // half-transparent. Sharp edges are the whole point of drawing
        // this on a grid.
        //
        // Weather is the *other* half of where it lands. A player's
        // cloud setting says what a fair sky looks like; the front says
        // how far it has been overrun. So the two are combined rather
        // than one winning: rain takes a sparse sky to nearly solid, and
        // a player who turned clouds down still gets weather, because a
        // downpour out of an empty sky is worse than either setting.
        let cover = mix(globals.sky_params.y, 0.94, overcast);
        let floor_edge = 0.60 - cover * 0.40;
        let amount = smoothstep(floor_edge, floor_edge + 0.03, density);

        // Self-shadowing: the same field again, a step toward the sun.
        // More cloud that way means this part is in the shade of it.
        //
        // Skipped where there is no cloud to shade, which is most of the
        // sky most of the time: the second fetch is a fetch for a colour
        // that is about to be multiplied by an `amount` of zero. The branch is on a value the whole
        // neighbourhood agrees about almost everywhere -- clouds are
        // large -- so it costs nothing where it does not save anything.
        var shadow = 0.0;
        var mottle = 0.0;
        if (amount > 0.001) {
            // A whole cell toward the sun, so the shading lands on the
            // grid as well: a pixel is lit or it is not, and the step
            // between two of them falls on a cell wall like every other
            // edge here.
            let sun_step = normalize(vec2<f32>(to_sun.x, to_sun.z) + vec2<f32>(1e-4, 0.0))
                * CLOUD_PIXEL * 2.0;
            let toward_sun = cloud_field(sample_at + sun_step).r;
            shadow = clamp((toward_sun - density) * 2.6, 0.0, 1.0);
            // Three tones rather than a gradient. Two flat greys with a
            // step between them is how a cloud is drawn in pixel art,
            // and it is also all the shape information there is at this
            // resolution.
            shadow = floor(shadow * 3.0 + 0.5) / 3.0;
            // A little per-cell variation on top, from the cell itself,
            // so a bank of cloud is mottled rather than poster-flat --
            // the same trick the block textures use, at the size of a
            // cloud pixel.
            mottle = (hash2(floor(smooth_at / CLOUD_PIXEL)) - 0.5) * 0.06;
        }
        // ---- what colour a cloud is, which is what the weather is ----
        //
        // A fair-weather cloud is a white thing with a grey underside.
        // A rain cloud is neither: it is dark all over, blue-grey rather
        // than white-grey, and it has *less* contrast across it, not
        // more -- the shape stops reading because there is no gap for
        // the light to come through. All three of those are here, and
        // together they are most of what makes rain look like rain from
        // inside it.
        let sunlit = mix(vec3<f32>(1.02, 0.99, 0.95), vec3<f32>(0.40, 0.42, 0.48), overcast);
        let shaded = mix(vec3<f32>(0.48, 0.52, 0.62), vec3<f32>(0.20, 0.21, 0.26), overcast);
        // Flatter under weather: the self-shadow is a sun that is no
        // longer getting through.
        let relief = shadow * mix(0.85, 0.45, overcast);
        var cloud = mix(sunlit, shaded, relief) + mottle;
        // Warmed by a low sun, like everything else in the sky is.
        cloud = cloud * mix(vec3<f32>(0.30, 0.33, 0.42), vec3<f32>(1.0), daylight);
        // **Past the Simple step, the deck catches the sunset.** Cloud on
        // the sun's side of a low sky is lit from underneath by light that
        // has crossed the most air, and that underside is the colour of
        // the glow, only brighter than the sky beside it -- which is the
        // one sight that makes a sunset worth stopping for. Its own
        // brightness is kept (the mix is toward the glow at the cloud's
        // luminance) so a grey rain cloud is not turned into a lamp.
        if (LIGHTING >= 1u) {
            let luma = dot(cloud, vec3<f32>(0.2126, 0.7152, 0.0722));
            let lit_from_below = globals.horizon_glow.rgb * luma * 1.7;
            cloud = mix(cloud, lit_from_below, clamp(glow_lobe(vec3<f32>(dir.x, 0.0, dir.z)) * 0.9, 0.0, 1.0));
        }
        // The bright rim where the sun stands behind the deck. Fades
        // with the weather along with the glow it comes from -- a storm
        // has no sunlit edges.
        cloud = cloud + vec3<f32>(0.35, 0.16, 0.06) * glow * (1.0 - shadow) * glow_through;

        // Two fades, and they are different questions. The first is
        // geometric: near the horizon the layer is edge-on and a long
        // way off, so it thins out instead of ending in a line. The
        // second is aerial perspective -- cloud thirty kilometres away
        // is the colour of the air between, and without it the layer
        // reads as a ceiling painted directly overhead.
        let edge_on = smoothstep(0.015, 0.30, toward);
        let haze = clamp(travel / 26000.0, 0.0, 0.75);
        // Toward the air's colour in *this* direction, sunset included,
        // or a distant cloud on the sun's side would fade to lavender
        // against an orange horizon.
        cloud = mix(cloud, glow_over(globals.fog_color.rgb, dir), haze);
        colour = mix(colour, cloud, amount * edge_on * 0.96);
    }

    // Under water the sky is not the sky: it is the surface seen from
    // below, and the terrain shader already tints everything toward the
    // water colour. Matching it here keeps the two from disagreeing at
    // the waterline -- and it is the fog colour *exactly*, because that is
    // what the terrain finishes on at the fog's end. At 55% of it, which is
    // what this was, everything past eighteen blocks was brighter than the
    // sky behind it and a swimmer saw the far bed and the lid in bands.
    // `fog::UNDERWATER` holds the old 55% now.
    if (globals.extra.z > 0.5) {
        colour = globals.fog_color.rgb;
    }

    return vec4<f32>(colour, 1.0);
}
