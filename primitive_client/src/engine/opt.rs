//! Frame-cost experiments, each behind its own environment switch.
//!
//! ## Why they are switches and not decisions
//!
//! Every line in here exists because of one measurement from a real
//! phone -- a Redmi Note 13 Pro 5G, Adreno 710, Vulkan, the `bench`
//! world, the player standing still:
//!
//! ```text
//! fps=60  frame avg/p95/p99 = 16.7/17.4/18.5 ms  still=100%
//! chunks loaded=441 draws=8 tris=240k/329k cut=13k culled=298
//! frame sim/encode/wait/present = 0.57/0.93/14.54/0.55 ms   gpu=11.9 ms
//! gpu stages: solid 11.34  cutout 0.040  sky 0.46  ui 0.008  shadow 0.000
//! 1898x854 (1.6 Mpx) at 70% resolution, msaa=1x, aniso=1x, present=mailbox
//! ```
//!
//! The processor is asleep and the whole frame is one pass: **11.34 ms of
//! 11.9 is the solid terrain**. What nobody can say from a desktop is
//! *why*, because the two machines fail differently -- a tile-based GPU
//! bills a triangle for binning as well as for pixels, and its depth and
//! colour never leave the tile unless something asks them to.
//!
//! **A phone cannot be driven** (see CLAUDE.md): MIUI refuses synthetic
//! input, so every menu tap on a device under test is a tap a human has to
//! make. Anything that has to be compared on a device therefore has to be
//! reachable from the environment, or it cannot be compared at all. That
//! is what this module is: one switch per hypothesis, so that a run with
//! one and a run without it differ in exactly one thing. Most are off
//! until asked for; the one that has already won a comparison is on, and
//! turns off (see `flag_or`).
//!
//! ## The switches
//!
//! | variable | what it does |
//! |---|---|
//! | `PRIMITIVE_OPT_TRILINEAR=0` | back to the point-sampled minification the game had before the device answered |
//! | `PRIMITIVE_OPT_MIP_BIAS=-0.5` | the terrain fetch asks for a sharper mip level |
//! | `PRIMITIVE_OPT_ANISO=4` | overrides the anisotropy setting |
//! | `PRIMITIVE_OPT_RESOLUTION=50` | overrides the resolution scale, in per cent |
//! | `PRIMITIVE_OPT_LOD=4` | overrides where the coarse bands start, in chunks |
//! | `PRIMITIVE_OPT_VIEW=8` | overrides the render distance, in chunks |
//! | `PRIMITIVE_OPT_NO_TEXTURE=1` | the terrain fetch, ablated |
//! | `PRIMITIVE_OPT_NO_MOTTLE=1` | the per-block shade and its derivative, ablated |
//! | `PRIMITIVE_OPT_NO_LIGHT=1` | the light assembly, ablated |
//! | `PRIMITIVE_OPT_NO_FOG=1` | the fog and the aerial perspective, ablated |
//! | `PRIMITIVE_OPT_DEPTH16=1` | a 16-bit depth buffer instead of a 32-bit float one |
//! | `PRIMITIVE_OPT_CHEAP_LIGHT=1` | the light's arithmetic, ablated; its varyings still read |
//! | `PRIMITIVE_OPT_BLOCK_SHADE=1` | the blocks' own shade on (or `=0` off) whatever the settings say |
//!
//! The last three change nothing about how the game draws -- they are the
//! three settings the frame is most sensitive to, made reachable without a
//! menu. The question "is this pass paying for pixels or for triangles?"
//! is answered by halving the pixels and reading the stage line, and then
//! by halving the triangles and reading it again.
//!
//! ## What the device has already answered
//!
//! Fifty-five seconds a mode, restarted between them, the last `[F3]` line
//! taken:
//!
//! ```text
//! mode                fps  frame avg/p95/p99   gpu     solid   tris
//! stock                54  18.7/19.8/20.4 ms   14.99   14.40   255k/340k
//! PRIMITIVE_OPT_LOD=4  54  18.6/19.5/19.9 ms   14.78   14.19   230k/309k
//! OPT_RESOLUTION=50    66  15.2/15.9/16.2 ms   11.80   11.16   255k/340k
//! OPT_DEPTH_DISCARD=1  54  18.5/19.7/20.3 ms   14.81   14.20   255k/340k
//! OPT_DEPTH_PREPASS=1  47  21.4/22.1/22.4 ms   17.61   17.04   255k/340k
//! OPT_TRILINEAR=1      53  18.7/19.9/20.6 ms   15.00   14.37   255k/340k
//! LOD=4 + RES=30       74  13.5/14.6/15.0 ms    8.88    8.14   230k/309k
//! ```
//!
//! Four of those switches have done their job and are gone:
//!
//! * **Trilinear costs nothing** -- 14.37 against 14.40, inside the noise --
//!   so it is what the game does now and the switch only turns it off.
//! * **The depth prepass loses on the phone too**, 17.04 against 14.40,
//!   the same way it lost on a desktop. Taken out; see the rejected note
//!   beside the main pass in `renderer.rs`.
//! * **Not storing the depth buffer is free and harmless** -- 14.20
//!   against 14.40, which is noise -- so the frame just does it, and the
//!   branch is gone.
//! * **The frame is not purely triangle-bound after all.** Half the
//!   resolution is a quarter of the pixels and took the solid pass down
//!   22%, at the same 255k triangles. So roughly a fifth to a quarter of
//!   the pass is fill, and the rest is geometry.
//!
//! ## The second round, and where the frame actually goes
//!
//! ```text
//! mode                   fps  gpu      solid   tris        detail
//! stock                   55  14.682   14.078  255k/340k   247/162/32
//! OPT_VIEW=8              58  13.739   13.168  151k/194k   155/42/0
//! OPT_VIEW=8 + OPT_LOD=4  59  13.543   12.955  126k/163k   69/128/0
//! OPT_LOD=4               56  14.421   13.817  230k/309k   69/180/192
//! PRIMITIVE_SPECKS=1      65  10.820    5.873  255k/340k   247/162/32
//! OPT_MIP_BIAS=-0.5       55  14.688   14.116  255k/340k   247/162/32
//! ```
//!
//! **Shading is 58% of the solid pass.** The speck hunt replaces this
//! fragment shader with one flat colour a face -- no fetch, no light, no
//! fog -- and the pass falls from 14.08 ms to 5.87 at the same 255
//! thousand triangles. Meanwhile *halving* the triangles (the render
//! distance at 8: 255k to 151k) buys 0.9 ms. So geometry is about 5.9 ms
//! and per-fragment work about 8.2, and every lever this module started
//! with was pulling on the smaller half.
//!
//! The mip bias costs nothing measurable (14.116 against 14.078), so the
//! sharpening is free and the only question left about it is what it looks
//! like.
//!
//! ## Where the 8.2 ms is: four ablations
//!
//! `PRIMITIVE_SPECKS` is the ceiling and says nothing about the parts. So
//! each part of the fragment shader has a switch that takes it away, and
//! each is a constant compiled into the shader rather than a uniform, so
//! an unset build is the shader that was there before:
//!
//! | variable | what stops being computed |
//! |---|---|
//! | `PRIMITIVE_OPT_NO_TEXTURE=1` | the atlas fetch; a flat albedo instead |
//! | `PRIMITIVE_OPT_NO_MOTTLE=1` | the per-block hash *and* the screen derivative that fades it |
//! | `PRIMITIVE_OPT_NO_LIGHT=1` | the beam, the fill, the floor, the occlusion |
//! | `PRIMITIVE_OPT_NO_FOG=1` | the fog ramp and the aerial perspective |
//!
//! Each makes the picture wrong on purpose; they are instruments. The four
//! should roughly add up to the gap between a stock run and the speck
//! hunt, and where they do not is itself the answer -- the remainder is
//! the tint, the greenness and the interpolation of ten varyings, none of
//! which can be switched off without changing what is drawn.
//!
//! **And what they came back as.** Same world, same seat, 45 s a mode:
//!
//! ```text
//! mode                 solid    against stock
//! stock                14.104
//! OPT_NO_MOTTLE=1      12.236   -1.87
//! OPT_NO_TEXTURE=1     11.921   -2.18
//! OPT_NO_LIGHT=1       11.474   -2.63
//! OPT_NO_FOG=1         13.045   -1.06
//! OPT_DEPTH16=1        13.798   -0.31
//! SPECKS=1              5.796   -8.31
//! SPECKS + RES=50       6.340
//! ```
//!
//! Three things fell out of that, and they set what happens next.
//!
//! **The parts add up.** 1.87 + 2.18 + 2.63 + 1.06 is 7.74 of the 8.31 the
//! speck hunt takes away. Nothing large is hiding, the work is spread, and
//! there is no single change that wins the pass back -- it comes off a
//! piece at a time.
//!
//! **There is no overdraw to speak of.** With the fragment shader reduced
//! to a flat colour, a quarter of the pixels cost 6.34 ms against the full
//! frame's 5.80 -- which is to say the difference is noise and the 5.8 ms
//! is geometry, not fill. So the near-to-far order and the facing rule are
//! doing their job and nothing is to be won by sorting harder.
//!
//! **`shader f16: not offered by this adapter`**, so half precision is not
//! a lever on this device at all. Closed.
//!
//! ## What is being done about it
//!
//! The blocks' own shade is now a setting (`block_shade`), off by default
//! on Android and on everywhere else -- 1.87 ms for a five per cent wobble
//! in a block's colour is worth it on a desktop that pays 0.07 for it and
//! not on a phone that pays 13% of its solid pass.
//!
//! ## Where it stopped, and why
//!
//! With the shade off by default, the device's stock build:
//!
//! ```text
//! mode                 fps  gpu      solid
//! stock                 60  12.731   12.191
//! OPT_CHEAP_LIGHT=1     62  12.370   11.815
//! OPT_NO_LIGHT=1        63  12.190   11.654
//! OPT_TRILINEAR=0       60  12.749   12.207
//! ```
//!
//! Over the whole of this work the solid pass went 14.10 to 12.19 ms and
//! the frame 18.7 to 16.7, which is 53 fps to 60.
//!
//! **Cheap light is within noise of no light** (11.82 against 11.65), so
//! what the light costs is arithmetic and not the interpolation of the
//! values it reads. That was the question `CHEAP_LIGHT` was built to
//! answer, and the answer closes the file on packing ten `@location`s into
//! six: it would be 104 edits in the shader for a ceiling that is now
//! fractions of a millisecond.
//!
//! **And the light is no longer worth folding either.** The whole of it is
//! 0.54 ms now where it was 2.63 -- because `SKIP_LIGHT` used to be
//! measured with the blocks' shade *on*, and what it was really taking
//! away was the screen-space derivative that fed the shade's fade. Turn
//! the shade off, as a phone now does, and the light is a rounding error.
//! One setting collected most of what four ablations had been pointing at.
//!
//! **Trilinear filtering is free**, finally and for the third time: 12.207
//! against 12.191.
//!
//! So the fragment shader is done. What is left of the pass is the atlas
//! fetch and about 5.8 ms of geometry, and the geometry is the next
//! subject rather than this one.
//!
//! And one did not do its job: `PRIMITIVE_OPT_LOD=4` took 10% of the
//! triangles away where a coarse chunk sheds 56% of its own
//! (`lod::CELL`). `lod_bands_repro` reproduces the device's frame to
//! within a third of a per cent and says where the rest went: **a frame is
//! not the world.** The fog cull takes the far ring and the frustum takes
//! five sixths of what is left, and what survives is weighted towards the
//! near chunks -- which are the ones no coarsening may touch. In-view solid
//! triangles at that seat:
//!
//! ```text
//!                     lod 10      lod 4
//! render distance 12  339_339    258_846   -24%
//! render distance  8  185_891    149_785
//! ```
//!
//! So the ceiling on moving the bands in is about a quarter, not four
//! fifths, and the render distance is the stronger of the two levers --
//! which is why `PRIMITIVE_OPT_VIEW` now exists beside it. The device
//! measured less than a quarter, so the `[F3]` line also carries
//! `detail=fine/coarse/coarser` now: a setting that reached the mesher and
//! one that did not looked identical from outside the phone.
//!
//! ## Read once
//!
//! Every value is behind a `OnceLock`. A switch read per frame is an
//! environment lookup per frame, and on Android the environment is a
//! lock; read once, a switch costs an atomic load. It also means a
//! switch cannot change under a running pipeline, which is what the
//! pipelines built from them assume.

use std::sync::OnceLock;

/// Whether a switch is on, given what it is when nobody sets it. Anything
/// but `0`, `false`, `off`, `no` and the empty string counts as on, so
/// `=1` and `=yes` and a bare `=` that the env file wrote as an empty
/// value all read the way they look.
///
/// **The default is a parameter because a switch that wins becomes the
/// game.** Trilinear filtering was measured on a device, cost nothing and
/// is now what the sampler does -- and the comparison that decided it has
/// to stay available, or the next device cannot re-take it.
/// `PRIMITIVE_OPT_TRILINEAR=0` is the whole reason for the argument.
fn flag_or(name: &str, default: bool) -> bool {
    match std::env::var(name) {
        Ok(value) => !matches!(
            value.trim().to_ascii_lowercase().as_str(),
            "" | "0" | "false" | "off" | "no"
        ),
        Err(_) => default,
    }
}

/// A number from the environment, or `None` where the variable is absent
/// or is not a number. **Not a panic and not a default**: a typo in a
/// file on a phone would otherwise either kill the game at startup or
/// silently measure the wrong thing, and neither is readable from a
/// logcat line. It is said out loud instead.
fn number<T: std::str::FromStr>(name: &str) -> Option<T> {
    let raw = std::env::var(name).ok()?;
    match raw.trim().parse() {
        Ok(value) => Some(value),
        Err(_) => {
            println!("[opt] {name}={raw:?} is not a number, ignored");
            None
        }
    }
}

/// **Minification filters, magnification stays nearest.** On unless the
/// environment says otherwise, which is what `PRIMITIVE_OPT_TRILINEAR=0`
/// is for.
///
/// At anisotropy 1 the block sampler used to be `Nearest` in all three
/// modes -- magnification, minification and the choice of mip (see
/// `texture::build_sampler`). The magnification half of that is
/// deliberate and right: this is 16x16 pixel art and a linear filter
/// smears it. The other two halves were not a decision, they were what
/// fell out of building the sampler from one mode.
///
/// What they did to the picture is what the phone photographed: ground
/// running away from the camera sampled at one point from one mip level
/// chosen by the *longer* of the two derivatives, so a surface seen
/// edge-on read a level far coarser than it needed across its short axis,
/// with no blend between levels to hide where one ended. That is "мыльная
/// картинка".
///
/// **It is on because the device says it is free**: the solid pass came
/// out at 14.37 ms against 14.40 stock, which is noise on a run whose p99
/// moves by half a millisecond. Nothing a player stands next to changes
/// either, because a magnified fragment never reaches the minification
/// filter. The switch stays so the comparison can be taken again on
/// another device rather than believed.
///
/// wgpu only refuses mixed modes when anisotropy is above 1; above 1
/// every mode is already Linear and this changes nothing.
pub fn trilinear_minification() -> bool {
    static ON: OnceLock<bool> = OnceLock::new();
    *ON.get_or_init(|| flag_or("PRIMITIVE_OPT_TRILINEAR", true))
}

/// **What the terrain's mip level is shifted by**, negative for sharper.
///
/// Compiled into the shader as a constant rather than handed over in the
/// globals, the way the lighting step is (see `lighting::specialise`): it
/// cannot change while the game runs, and a uniform read per fragment to
/// carry a number that is the same all run is a cost for nothing.
///
/// Zero is the shader's own text and no substitution happens at all.
///
/// **A dial, not a fix.** Half a level sharper is roughly what the
/// missing anisotropy costs a surface at 45 degrees; a whole level is
/// what it costs at a steeper angle, and past that a distant plain starts
/// to sparkle as the sampling falls under the texture's own spacing. The
/// point of putting it in the environment is that the right number is a
/// thing to look at on the device, not to derive.
pub fn mip_bias() -> f32 {
    static BIAS: OnceLock<f32> = OnceLock::new();
    *BIAS.get_or_init(|| {
        let bias: f32 = number("PRIMITIVE_OPT_MIP_BIAS").unwrap_or(0.0);
        // A bias outside this is not a sharpening, it is a different
        // texture; clamped rather than refused so a stray minus sign
        // cannot make the world read from a 1x1 mip.
        bias.clamp(-4.0, 4.0)
    })
}

/// The lines in `shader.wgsl` the switches replace, each written there
/// exactly as it is here.
///
/// **A table rather than a call per constant**, because the failure mode
/// is one line renamed in the shader and a substitution that then silently
/// does nothing: one list means one test
/// (`the_shader_still_carries_every_line_a_switch_replaces`) covers all of
/// them.
const SWITCH_LINES: [&str; 7] = [
    "const MIP_BIAS: f32 = 0.0;",
    "const SKIP_TEXTURE: bool = false;",
    "const SKIP_MOTTLE: bool = false;",
    "const SKIP_LIGHT: bool = false;",
    "const SKIP_FOG: bool = false;",
    "const DECAL_SCALE: f32 = 1.0;",
    "const CHEAP_LIGHT: bool = false;",
];

/// The line in `shader.wgsl` the bias replaces.
const MIP_BIAS_SWITCH: &str = SWITCH_LINES[0];

/// The terrain shader with [`mip_bias`] compiled into it.
///
/// Chained after `lighting::specialise` and `AtlasSplit::specialise`, and
/// like them it hands the source straight back when there is nothing to
/// change -- so a run without the switch compiles the file's own text.
pub fn specialise(source: std::borrow::Cow<'_, str>) -> std::borrow::Cow<'_, str> {
    let mut out = specialise_at(mip_bias(), source);
    // **The decal nudge goes with the depth format**, not with a switch of
    // its own: they are one decision, and a nudge scaled without the buffer
    // being made smaller would push every decal off its surface for
    // nothing.
    if depth16() {
        out = rewrite(out, SWITCH_LINES[5], "const DECAL_SCALE: f32 = 32.0;");
    }
    for (on, line, replacement) in [
        (no_texture(), SWITCH_LINES[1], "const SKIP_TEXTURE: bool = true;"),
        (no_light(), SWITCH_LINES[3], "const SKIP_LIGHT: bool = true;"),
        (no_fog(), SWITCH_LINES[4], "const SKIP_FOG: bool = true;"),
        (cheap_light(), SWITCH_LINES[6], "const CHEAP_LIGHT: bool = true;"),
        // **The player's own setting, in the same list as the
        // instruments.** It is the one line here that a settings file
        // decides rather than an environment variable, and it is
        // substituted the same way because the answer has to be a constant
        // in the shader either way. See `block_shade`.
        (!block_shade(), SWITCH_LINES[2], "const SKIP_MOTTLE: bool = true;"),
    ] {
        if on {
            out = rewrite(out, line, replacement);
        }
    }
    out
}

/// One line of the shader swapped for another, borrowing until it has to
/// own. `debug_assert` rather than a silent pass, for the reason
/// `SWITCH_LINES` exists.
fn rewrite<'a>(
    source: std::borrow::Cow<'a, str>,
    line: &str,
    replacement: &str,
) -> std::borrow::Cow<'a, str> {
    debug_assert!(source.contains(line), "shader.wgsl no longer carries {line:?}");
    std::borrow::Cow::Owned(source.replacen(line, replacement, 1))
}

/// `specialise`, told the bias rather than reading it, so that a test can
/// compile the shader at a bias the process was not started with -- a
/// `OnceLock` cannot be set from inside a test run, and a substitution
/// nobody ever validates is a shader that fails to compile on a phone and
/// nowhere else.
fn specialise_at(bias: f32, source: std::borrow::Cow<'_, str>) -> std::borrow::Cow<'_, str> {
    if bias == 0.0 {
        return source;
    }
    debug_assert!(
        source.contains(MIP_BIAS_SWITCH),
        "shader.wgsl no longer carries {MIP_BIAS_SWITCH:?}"
    );
    // `{:?}` on an f32 always writes a decimal point, which WGSL needs to
    // read the literal as a float rather than an integer.
    std::borrow::Cow::Owned(source.replacen(
        MIP_BIAS_SWITCH,
        &format!("const MIP_BIAS: f32 = {bias:?};"),
        1,
    ))
}

/// The anisotropy the renderer should use, given what the settings say.
///
/// The setting wins unless the environment names a number, which is the
/// rule every switch here follows: a player who never sets one of these
/// gets their own settings, and a device under test gets the same build
/// asked two different questions.
pub fn anisotropy(setting: u16) -> u16 {
    static OVERRIDE: OnceLock<Option<u16>> = OnceLock::new();
    match *OVERRIDE.get_or_init(|| number("PRIMITIVE_OPT_ANISO")) {
        Some(value) => value.clamp(1, 16),
        None => setting,
    }
}

/// The resolution scale the renderer should use, given what the settings
/// say. In per cent, so `PRIMITIVE_OPT_RESOLUTION=50` reads the way the
/// settings screen writes it.
///
/// **This is the measurement `lod.rs` is built on**, made reachable from a
/// phone: run the same seat at 100 and at 50 and compare the solid stage.
/// Half the scale is a quarter of the pixels; a pass that is paying for
/// pixels comes down by something near that, and a pass that is paying
/// for triangles barely moves.
pub fn resolution_scale(setting: Option<f32>) -> Option<f32> {
    static OVERRIDE: OnceLock<Option<f32>> = OnceLock::new();
    match *OVERRIDE.get_or_init(|| number::<f32>("PRIMITIVE_OPT_RESOLUTION")) {
        // The same range the settings slider has. A scale of zero is a
        // surface of no pixels, which is a validation error rather than a
        // fast frame.
        Some(percent) => Some((percent / 100.0).clamp(0.1, 1.0)),
        None => setting,
    }
}

/// How far out the coarse bands start, given what the settings say. In
/// chunks; zero is the simplification off.
///
/// **Measured, and smaller than it looks.** The default is ten chunks
/// inside a render distance of twelve, so only the outermost ring of the
/// world is coarsened at all -- which reads like a lever with most of its
/// travel unused. It is not: `lod_bands_repro` puts moving the line from
/// ten to four at -24% of the triangles in view, and an Adreno 710 at -10%
/// of them, because the near band a player is standing in is a third of
/// the frame and cannot be coarsened at any setting.
///
/// It is a setting and a slider already. It is here because reaching that
/// slider is a menu tap, and a phone cannot be driven.
pub fn lod_distance(setting: i32) -> i32 {
    static OVERRIDE: OnceLock<Option<i32>> = OnceLock::new();
    match *OVERRIDE.get_or_init(|| number("PRIMITIVE_OPT_LOD")) {
        // Not clamped here: `ClientSettings::sanitise` does it, right
        // after this, with the same bounds a settings file gets -- and a
        // second clamp with its own numbers is a second set of numbers to
        // drift.
        Some(chunks) => chunks,
        None => setting,
    }
}

/// The render distance, in chunks, given what the settings say.
///
/// **The other way to send fewer triangles, and the device has to say
/// which is cheaper.** Coarsening keeps the world its own size and makes
/// the far half of it blockier; a shorter render distance keeps every
/// block where it is and stops the world sooner. They cost the player
/// different things -- one takes detail off the hills, the other takes the
/// hills away -- and they cost the GPU different things too: coarsening
/// only removes triangles, while a shorter distance removes triangles, the
/// chunks they live in, the memory that holds them and the fog they were
/// fading into.
///
/// So this is here beside `lod_distance` to be measured against it rather
/// than argued about. It is the same setting the RENDER DISTANCE row
/// moves, and `ClientSettings::sanitise` clamps it by the same rule.
pub fn view_distance(setting: i32) -> i32 {
    static OVERRIDE: OnceLock<Option<i32>> = OnceLock::new();
    match *OVERRIDE.get_or_init(|| number("PRIMITIVE_OPT_VIEW")) {
        Some(chunks) => chunks,
        None => setting,
    }
}

/// **The atlas fetch, taken away.** Every terrain fragment gets
/// `FLAT_ALBEDO` and the shading runs on it unchanged.
///
/// What it prices is one `textureSample` from an array 804 layers deep,
/// with trilinear minification, over a bus a phone shares with everything
/// else. If this is most of the 8.2 ms the answer is about texture
/// residency -- a smaller atlas, fewer mip levels in flight, or blocks
/// that share pictures -- and not about arithmetic.
pub fn no_texture() -> bool {
    static ON: OnceLock<bool> = OnceLock::new();
    *ON.get_or_init(|| flag_or("PRIMITIVE_OPT_NO_TEXTURE", false))
}

/// **The block-to-block shade, taken away** -- the hash in
/// `mottled_shade`, and with it the screen-space derivative in
/// `cell_footprint` that exists only to fade the hash out at range.
///
/// **Both halves, or the measurement would be half a measurement.** The
/// derivative is taken on every terrain fragment in the frame whether the
/// block wears a shade or not, because a derivative has to be taken in
/// uniform control flow; leaving it in would price the hash and not what
/// the feature costs.
///
/// shader.wgsl already records what this class of GPU thinks of a
/// per-pixel hash: two of them took a phone's terrain pass from 4.0 ms to
/// 7.1 where a desktop paid 0.07. It is one hash now. This says what that
/// one still costs.
pub fn no_mottle() -> bool {
    static ON: OnceLock<bool> = OnceLock::new();
    *ON.get_or_init(|| flag_or("PRIMITIVE_OPT_NO_MOTTLE", false))
}

/// **The light, taken away**: the sun's beam, the sky fill, the ambient
/// floor, the ambient occlusion and the clamp over the lot. Every fragment
/// is lit at one.
///
/// Most of this is a handful of multiplies over values the vertex shader
/// already interpolated, so a large number here would be a surprise and a
/// useful one -- it would mean the cost is in the vector work rather than
/// in the fetch, which is exactly the case half precision (`f16`) is made
/// for.
pub fn no_light() -> bool {
    static ON: OnceLock<bool> = OnceLock::new();
    *ON.get_or_init(|| flag_or("PRIMITIVE_OPT_NO_LIGHT", false))
}

/// **The distance haze, taken away**: the fog ramp and the aerial
/// perspective that greys the far ground before it.
///
/// The cheapest-looking of the four on paper -- a `mix` and a ratio -- and
/// the one that runs on literally every fragment with no bit to gate it.
/// If it prices high, the fix is real and easy: the ramp is a function of
/// `view_distance`, which is already a varying, so it can move to the
/// vertex shader for a quad small enough that the ramp is linear across
/// it.
pub fn no_fog() -> bool {
    static ON: OnceLock<bool> = OnceLock::new();
    *ON.get_or_init(|| flag_or("PRIMITIVE_OPT_NO_FOG", false))
}

/// **The light's arithmetic taken away while its varyings stay.** See
/// `CHEAP_LIGHT` in shader.wgsl for what the pair of readings separates.
pub fn cheap_light() -> bool {
    static ON: OnceLock<bool> = OnceLock::new();
    *ON.get_or_init(|| flag_or("PRIMITIVE_OPT_CHEAP_LIGHT", false))
}

/// **Whether blocks wear their own shade** -- the one thing in this module
/// the player owns rather than a person taking a measurement.
///
/// It is here, beside the instruments, because it has to be a constant in
/// the shader and this is where shader constants are decided. What makes
/// it different is that it is written as well as read: `ClientSettings`
/// hands it over at startup and again whenever the row moves, and
/// `GraphicsState::set_block_shade` rebuilds the pipelines around it the
/// way the lighting step does.
///
/// **Off by default on Android, on everywhere else**, and the number is
/// why: an Adreno 710 drew the solid pass in 12.24 ms without the shade
/// and 14.10 with it -- 1.87 ms, 13% of the pass, for a five per cent
/// wobble in the colour of a block. A desktop measured the same feature at
/// 0.07 ms. It is the clearest case in this repository of a thing that is
/// worth its price on one machine and not on another, which is the
/// definition of a quality setting.
///
/// `PRIMITIVE_OPT_NO_MOTTLE=1` still forces it off, so the ablation stays
/// available on a machine whose settings say otherwise, and
/// `PRIMITIVE_OPT_BLOCK_SHADE=1` forces it on, which is how a phone is
/// shown the picture its default takes away.
pub fn block_shade() -> bool {
    if no_mottle() {
        return false;
    }
    match *BLOCK_SHADE_OVERRIDE.get_or_init(|| tristate("PRIMITIVE_OPT_BLOCK_SHADE")) {
        Some(forced) => forced,
        None => BLOCK_SHADE.load(std::sync::atomic::Ordering::Relaxed),
    }
}

/// What `PRIMITIVE_OPT_BLOCK_SHADE` says, if it says anything.
///
/// **A switch that only turned something off was half a switch.** The
/// setting is off by default on a phone, so `PRIMITIVE_OPT_NO_MOTTLE`
/// there asks for what was already happening -- and there was no way at
/// all to ask for the other side, which is the comparison a device needs
/// to make. `=1` puts the shade back on a phone, `=0` takes it off a
/// desktop, and unset leaves the settings file in charge.
static BLOCK_SHADE_OVERRIDE: OnceLock<Option<bool>> = OnceLock::new();

/// A switch with three answers: on, off, and "nobody said".
///
/// `flag_or` cannot express the third, because it takes the default *for*
/// the caller and so an unset variable and a variable set to the default
/// are the same thing to it. Here they are not: one means the settings
/// file decides and the other means it does not.
fn tristate(name: &str) -> Option<bool> {
    let value = std::env::var(name).ok()?;
    Some(!matches!(
        value.trim().to_ascii_lowercase().as_str(),
        "" | "0" | "false" | "off" | "no"
    ))
}

/// What the settings say, until they say otherwise.
///
/// **An atomic rather than a `OnceLock`**, because unlike every other
/// value in this module it can change while the game runs -- and a plain
/// `static mut` would be the same thing with none of the guarantees. The
/// ordering is `Relaxed` because the only reader is the pipeline build
/// that the same thread has just asked for.
static BLOCK_SHADE: std::sync::atomic::AtomicBool =
    std::sync::atomic::AtomicBool::new(!cfg!(target_os = "android"));

/// Hands the setting over. Returns whether the shader would come out
/// different, so the caller knows whether the pipelines have to be built
/// again.
///
/// **What changed, and not what was stored.** With
/// `PRIMITIVE_OPT_BLOCK_SHADE` or `PRIMITIVE_OPT_NO_MOTTLE` set, the
/// settings file is overruled -- so a player opening the settings screen
/// during a measurement would otherwise rebuild nine pipelines to produce
/// exactly the shader that was already running.
pub fn set_block_shade(on: bool) -> bool {
    let before = block_shade();
    BLOCK_SHADE.store(on, std::sync::atomic::Ordering::Relaxed);
    block_shade() != before
}

/// **A 16-bit depth buffer in place of the 32-bit float one.**
///
/// Colour and depth are 8 bytes a pixel of tile memory on a tile-based
/// GPU; at two bytes of depth they are 6, which is a third more pixels in
/// a bin and a third fewer bins for the geometry to be sorted into and
/// re-read for. That is a saving on the *geometry* half of the pass, which
/// the device puts at 5.9 ms, and on the bandwidth the whole frame shares.
///
/// **What it costs, said plainly.** Two things, and they are why this is a
/// switch:
///
/// * A decal's depth nudge is 9.5e-7 of clip depth and a 16-bit step is
///   about 1.5e-5, so the nudge is multiplied by `DECAL_SCALE` (see
///   shader.wgsl) and a decal then stands further off the surface it is
///   painted on -- visible at a grazing angle, if at all.
/// * The sun's shadow map is made from the same format, so it loses
///   precision too. The device this is for has shadows off (`shadow 0.000`
///   on its stage line), but a run with them on is not measuring the same
///   picture and the number would not be comparable.
///
/// `Depth24Plus` is not offered as a middle step because it is not one:
/// every implementation this runs on stores it in four bytes, so it saves
/// exactly nothing of what this is trying to save.
///
/// **Measured, and left off.** An Adreno 710 drew the solid pass in 13.80
/// ms against 14.10, so the third of the tile memory bought 0.31 ms --
/// two per cent of the pass, for a decal that stands off its surface and a
/// shadow map that loses half its bits. The switch stays because the
/// number is worth re-taking on a device with less tile memory, where the
/// same third is a larger share; the default stays as it was because on
/// this one it does not pay for what it costs.
pub fn depth16() -> bool {
    static ON: OnceLock<bool> = OnceLock::new();
    *ON.get_or_init(|| flag_or("PRIMITIVE_OPT_DEPTH16", false))
}

/// The depth format the frame and every pipeline that draws into it use.
///
/// **One function, and every caller goes through it.** A pipeline built
/// for one format and a pass begun with another is a validation error at
/// the first frame -- loud, but loud on whichever machine ran it, and this
/// switch exists to be flipped on a phone. There is no second copy of the
/// answer to get wrong.
pub fn depth_format() -> wgpu::TextureFormat {
    if depth16() {
        wgpu::TextureFormat::Depth16Unorm
    } else {
        wgpu::TextureFormat::Depth32Float
    }
}

/// One line for the log, so that a measurement taken from a device says
/// on its face which build it came from. Printed once at startup and only
/// when something is actually switched on -- a stock run stays silent.
pub fn announce() {
    let mut on: Vec<String> = Vec::new();
    // Named when it is *off*, which is the state nobody asked for: a line
    // that said "trilinear" on every stock run would be noise, and a run
    // with the filtering turned back off is exactly the one whose numbers
    // would otherwise be unexplainable a week later.
    if !trilinear_minification() {
        on.push("trilinear off".into());
    }
    if mip_bias() != 0.0 {
        on.push(format!("mip-bias {}", mip_bias()));
    }
    if anisotropy(1) != 1 {
        on.push(format!("aniso {}", anisotropy(1)));
    }
    if let Some(scale) = resolution_scale(None) {
        on.push(format!("resolution {:.0}%", scale * 100.0));
    }
    // A sentinel no settings file holds, so "the environment named a
    // number" and "the number happens to be the default" cannot be
    // confused in the log.
    if lod_distance(i32::MIN) != i32::MIN {
        on.push(format!("lod {} chunks", lod_distance(i32::MIN)));
    }
    if view_distance(i32::MIN) != i32::MIN {
        on.push(format!("view {} chunks", view_distance(i32::MIN)));
    }
    for (set, name) in [
        (no_texture(), "no-texture"),
        (no_mottle(), "no-mottle"),
        (no_light(), "no-light"),
        (cheap_light(), "cheap-light"),
        (no_fog(), "no-fog"),
        (depth16(), "depth16"),
        // Said either way, because on a desktop "off" is the unusual
        // state and on a phone "on" is -- and a reader of a measurement
        // needs whichever of the two this run was.
        (!block_shade(), "block-shade off"),
        (block_shade() && cfg!(target_os = "android"), "block-shade on"),
    ] {
        if set {
            on.push(name.into());
        }
    }
    if !on.is_empty() {
        println!("[opt] {}", on.join(", "));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The substitution is a text match, and a rename in the shader that
    /// left this behind would silently draw at the stock mip level while
    /// the log said otherwise.
    #[test]
    fn the_shader_still_carries_every_line_a_switch_replaces() {
        let source = include_str!("shader.wgsl");
        for line in SWITCH_LINES {
            assert_eq!(
                source.matches(line).count(),
                1,
                "shader.wgsl must carry {line:?} exactly once for engine::opt::specialise"
            );
        }
    }

    /// Every ablation has to produce a shader that compiles, and only a
    /// build with the switch set would ever find out -- on a phone, which
    /// is the one place nobody can attach a debugger to. So each one is
    /// substituted, parsed and validated here, whole and split.
    #[test]
    fn every_ablation_still_compiles() {
        use naga::valid::{Capabilities, ValidationFlags, Validator};
        let source = include_str!("shader.wgsl");
        for line in SWITCH_LINES {
            let flipped = line
                .replace("bool = false", "bool = true")
                .replace("f32 = 1.0", "f32 = 32.0")
                .replace("f32 = 0.0", "f32 = -0.5");
            assert_ne!(flipped, line, "{line:?} has no other value to take");
            let text = source.replacen(line, &flipped, 1);
            for arrays in [1u32, 4] {
                let split = crate::engine::texture::AtlasSplit {
                    per_array: crate::engine::texture::MIN_PER_ARRAY,
                    arrays,
                };
                let text = split.specialise(std::borrow::Cow::Borrowed(text.as_str()));
                let module = naga::front::wgsl::parse_str(&text).unwrap_or_else(|e| {
                    panic!("{flipped} in {arrays} array(s) failed to parse:\n{}", e.emit_to_string(&text))
                });
                Validator::new(ValidationFlags::all(), Capabilities::all())
                    .validate(&module)
                    .unwrap_or_else(|e| panic!("{flipped} in {arrays} array(s) failed validation: {e:?}"));
            }
        }
    }

    /// **The biased shader has to compile, and only a build with the
    /// switch on would ever find out.** The bias turns one `textureSample`
    /// into a `textureSampleBias`, which the atlas split then has to
    /// rewrite as well (`texture::AtlasSplit::specialise`) -- and the
    /// device that splits is a GLES phone, which is the one place nobody
    /// can attach a debugger to. So the substituted source is parsed and
    /// validated here, whole and split, at a bias the process was not
    /// started with.
    #[test]
    fn the_shader_still_compiles_with_a_mip_bias_in_it() {
        use naga::valid::{Capabilities, ValidationFlags, Validator};
        let source = include_str!("shader.wgsl");
        for arrays in [1u32, 4] {
            let split = crate::engine::texture::AtlasSplit {
                per_array: crate::engine::texture::MIN_PER_ARRAY,
                arrays,
            };
            let text = split.specialise(specialise_at(-0.5, std::borrow::Cow::Borrowed(source)));
            assert!(text.contains("-0.5"), "the bias was not written in");
            let module = naga::front::wgsl::parse_str(&text).unwrap_or_else(|e| {
                panic!("a biased shader in {arrays} array(s) failed to parse:
{}", e.emit_to_string(&text))
            });
            Validator::new(ValidationFlags::all(), Capabilities::all())
                .validate(&module)
                .unwrap_or_else(|e| panic!("a biased shader in {arrays} array(s) failed validation: {e:?}"));
        }
    }

    /// **Nothing here changes what a desktop draws.**
    ///
    /// Every switch in this module is off, or at its default, unless
    /// somebody says otherwise -- and on a machine that is not a phone the
    /// blocks' shade is on, which is what the game did before any of this
    /// existed. So `specialise` must hand the source straight back,
    /// borrowed and unrewritten. A `Cow::Owned` here means some switch has
    /// acquired a default that rewrites the shader, and the first anybody
    /// would know of it is a screenshot that no longer matches.
    #[test]
    fn a_machine_nobody_is_measuring_compiles_the_shader_as_written() {
        if cfg!(target_os = "android") {
            return; // there the shade is off by default, and on purpose
        }
        let source = include_str!("shader.wgsl");
        let out = specialise(std::borrow::Cow::Borrowed(source));
        // Which line moved, rather than "they differ": the shader is
        // three thousand lines and the answer is one of seven.
        let moved: Vec<&str> = SWITCH_LINES
            .iter()
            .copied()
            .filter(|line| !out.contains(line))
            .collect();
        assert!(
            moved.is_empty(),
            "a switch now rewrites the shader by default: {moved:?}"
        );
        assert!(
            matches!(out, std::borrow::Cow::Borrowed(_)),
            "the shader was copied without being changed"
        );
    }

    /// `{:?}` is what makes `-0.5` come out as `-0.5` and `-1` as `-1.0`;
    /// WGSL reads the second as an integer and refuses to assign it to an
    /// `f32`, which would be a shader that fails to compile on a phone
    /// and nowhere else.
    #[test]
    fn a_whole_number_of_mip_levels_is_written_as_a_float() {
        let written = format!("const MIP_BIAS: f32 = {:?};", -1.0f32);
        assert_eq!(written, "const MIP_BIAS: f32 = -1.0;");
    }
}
