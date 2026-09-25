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
//! is what this module is: one switch per hypothesis, default off, so that
//! a run with it and a run without it differ in exactly one thing.
//!
//! ## The switches
//!
//! | variable | what it does |
//! |---|---|
//! | `PRIMITIVE_OPT_DEPTH_DISCARD=1` | the main pass stops storing its depth buffer |
//! | `PRIMITIVE_OPT_DEPTH_PREPASS=1` | the solid terrain lays depth down first, then shades |
//! | `PRIMITIVE_OPT_TRILINEAR=1` | minification filters and mips blend, while magnification stays nearest |
//! | `PRIMITIVE_OPT_MIP_BIAS=-0.5` | the terrain fetch asks for a sharper mip level |
//! | `PRIMITIVE_OPT_ANISO=4` | overrides the anisotropy setting |
//! | `PRIMITIVE_OPT_RESOLUTION=50` | overrides the resolution scale, in per cent |
//! | `PRIMITIVE_OPT_LOD=4` | overrides where the coarse bands start, in chunks |
//!
//! The last three change nothing about how the game draws -- they are the
//! three settings the frame is most sensitive to, made reachable without a
//! menu. The question "is this pass paying for pixels or for triangles?"
//! is answered by halving the pixels and reading the stage line, and then
//! by halving the triangles and reading it again. `lod.rs` answered it
//! that way on a desktop and got "triangles"; whether a phone agrees is
//! not a thing to assume, and until now it could not be asked.
//!
//! ## Read once
//!
//! Every value is behind a `OnceLock`. A switch read per frame is an
//! environment lookup per frame, and on Android the environment is a
//! lock; read once, a switch costs an atomic load. It also means a
//! switch cannot change under a running pipeline, which is what the
//! pipelines built from them assume.

use std::sync::OnceLock;

/// Whether a switch is on. Anything but `0`, `false`, `off`, `no` and the
/// empty string counts as on, so `=1` and `=yes` and a bare `=` that the
/// env file wrote as an empty value all read the way they look.
fn flag(name: &str) -> bool {
    match std::env::var(name) {
        Ok(value) => !matches!(
            value.trim().to_ascii_lowercase().as_str(),
            "" | "0" | "false" | "off" | "no"
        ),
        Err(_) => false,
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

/// **The main pass stops storing its depth buffer.**
///
/// The depth attachment is written with `StoreOp::Store` and nothing
/// reads it: the scene blit samples colour, the screenshot reads the
/// swapchain, and the next frame clears depth before it draws. On a
/// desktop that store is a no-op the driver may not even honour. On a
/// tile-based GPU it is a full copy of the depth buffer out of tile
/// memory and into main memory, every frame -- at the phone's 1329x598
/// scene that is 3.2 MB a frame, 190 MB a second, on a bus the whole
/// device shares.
///
/// **The picture cannot change.** A store op decides what happens to the
/// attachment *after* the pass; every test inside the pass has already
/// run. The only way this could show is if something downstream sampled
/// the depth texture, and nothing does -- `depth_view` appears in this
/// crate as a render attachment and nowhere else.
pub fn depth_store_discard() -> bool {
    static ON: OnceLock<bool> = OnceLock::new();
    *ON.get_or_init(|| flag("PRIMITIVE_OPT_DEPTH_DISCARD"))
}

/// **The solid terrain draws twice: depth only, then shaded.**
///
/// The classic cure for an opaque pass that shades pixels it then paints
/// over. The first draw writes nothing but depth -- no varyings, no
/// texture fetch, every colour channel masked off; the second draws the
/// same triangles with `LessEqual`, and every fragment that lost is
/// rejected before its shader runs.
///
/// **What it is expected to cost rather than save, and why it is a switch
/// rather than a change.** This repository has already measured the same
/// idea twice and had it come out negative both times:
///
/// * The cut-out pass (see the note beside it in `renderer.rs`): a
///   quarter of the pixels took it from 1.79 ms to 1.36, and a flat
///   fragment shader to 1.56, so pixels were not what it was spending.
/// * `lod.rs`: the solid pass sends 1.35 million triangles behind 0.9
///   million pixels and ten times the pixels costs 19% more. The average
///   terrain triangle is smaller than a pixel.
///
/// A prepass sends every triangle a second time, and a pass whose bill is
/// triangles pays twice for a saving measured in pixels. On the phone's
/// own numbers a triangle covers three pixels, which is worse than the
/// desktop's ratio, not better.
///
/// So why is it here? Because a tile-based GPU has a thing a desktop does
/// not: Adreno's low-resolution depth buffer is built during binning, and
/// a depth-only draw is exactly what primes it. The honest answer is that
/// nobody in this repository knows which way it goes on an Adreno, and
/// one run with the switch and one without is a cheaper way to find out
/// than an argument.
///
/// **On a desktop it is a rout, and the number is here so nobody has to
/// re-derive it.** `prepass_repro::what_a_depth_prepass_costs`, the shore
/// of the benchmark world at 1280x720, 113 chunks and 483 thousand solid
/// triangles, on a GTX 1050 Ti:
///
/// ```text
/// one pass:  4.085 ms a frame
/// prepass:  19.634 ms a frame
/// ```
///
/// Nearly five times, which is more than doubling the triangles can
/// explain on its own -- a depth pass that still carries a colour
/// attachment, masked or not, does not get the driver's double-rate
/// depth-only path either. The saving it was buying is zero here, and the
/// near-to-far sort is why: by the time a far chunk is drawn the depth
/// buffer in front of it is already written, so there was no shading left
/// to skip.
///
/// **Only where the device has multi-draw**, which is every Vulkan and
/// D3D12 device and no GLES one. The prepass reuses the solid pass's own
/// indirect sub-draws -- the same buffer, the same ranges, the same
/// order -- and building a second per-chunk loop for the devices that
/// cannot would be a second copy of the thing being measured.
pub fn depth_prepass() -> bool {
    static ON: OnceLock<bool> = OnceLock::new();
    *ON.get_or_init(|| flag("PRIMITIVE_OPT_DEPTH_PREPASS"))
}

/// **Minification filters, magnification stays nearest.**
///
/// At anisotropy 1 the block sampler is `Nearest` in all three modes --
/// magnification, minification and the choice of mip (see
/// `texture::build_sampler`). The magnification half of that is
/// deliberate and right: this is 16x16 pixel art and a linear filter
/// smears it. The other two halves are not a decision, they are what
/// falls out of building the sampler from one mode.
///
/// What they do to the picture is what the phone photographed: ground
/// running away from the camera is sampled at one point from one mip
/// level chosen by the *longer* of the two derivatives, so a surface seen
/// edge-on reads a level far coarser than it needs across its short axis,
/// and there is no blend between levels to hide where one ends. That is
/// "мыльная картинка" and it is free to fix -- trilinear minification is
/// not a measurable cost on any GPU made this decade, and it changes
/// nothing a player stands next to, because a magnified fragment never
/// reaches the minification filter.
///
/// wgpu only refuses mixed modes when anisotropy is above 1, and this is
/// for the case where it is 1.
pub fn trilinear_minification() -> bool {
    static ON: OnceLock<bool> = OnceLock::new();
    *ON.get_or_init(|| flag("PRIMITIVE_OPT_TRILINEAR"))
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
/// **The lever the phone's own numbers point at.** `lod.rs` measured the
/// solid pass as bound by triangle count and not by pixels, and the
/// device's stage line agrees: 240 thousand triangles behind 0.79 million
/// pixels, which is a triangle every three pixels and every one of them
/// billed at the 2x2 quad the rasteriser works in. The default is ten
/// chunks inside a render distance of twelve, so only the outermost ring
/// of the world is coarsened at all -- and moving the line in is the one
/// change that takes triangles away rather than moving their cost around.
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

/// One line for the log, so that a measurement taken from a device says
/// on its face which build it came from. Printed once at startup and only
/// when something is actually switched on -- a stock run stays silent.
pub fn announce() {
    let mut on: Vec<String> = Vec::new();
    if depth_store_discard() {
        on.push("depth-discard".into());
    }
    if depth_prepass() {
        on.push("depth-prepass".into());
    }
    if trilinear_minification() {
        on.push("trilinear".into());
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
