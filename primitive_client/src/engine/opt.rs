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

/// The line in `shader.wgsl` the bias replaces. Written there exactly as
/// it is here, and `specialise` fails loudly if it ever stops matching.
const MIP_BIAS_SWITCH: &str = "const MIP_BIAS: f32 = 0.0;";

/// The terrain shader with [`mip_bias`] compiled into it.
///
/// Chained after `lighting::specialise` and `AtlasSplit::specialise`, and
/// like them it hands the source straight back when there is nothing to
/// change -- so a run without the switch compiles the file's own text.
pub fn specialise(source: std::borrow::Cow<'_, str>) -> std::borrow::Cow<'_, str> {
    specialise_at(mip_bias(), source)
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
    fn the_shader_still_carries_the_line_the_mip_bias_replaces() {
        assert!(
            include_str!("shader.wgsl").contains(MIP_BIAS_SWITCH),
            "shader.wgsl must carry {MIP_BIAS_SWITCH:?} for engine::opt::specialise"
        );
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
