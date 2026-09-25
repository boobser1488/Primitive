//! How much the light is allowed to do: the lighting quality setting.
//!
//! **Not the light map.** What a block is lit by -- the baked sky and
//! block levels -- is `primitive_shared::lighting`, and it is the same at
//! every step of this setting. This module decides what *colour* that
//! light is and how the frame is graded, which is a question only the
//! client has and only the renderer answers.
//!
//! ## The three steps
//!
//! * **Simple** is the game as it looked before the setting existed:
//!   the same shader text, the same pipelines, the same sky. A player who
//!   picks it pays for nothing and sees nothing new.
//! * **Balanced** is colour, which costs next to nothing because nearly
//!   all of it is decided once a frame on the CPU. The sun is golden low
//!   down and only faintly warm at noon; the part of the light that does
//!   not come straight from the sun comes from the sky and is cool, so a
//!   face turned away from the sun is bluer rather than merely darker; the
//!   sky and the distance glow toward the sun while it is low and stay the
//!   flat colour the player asked for (`sky.wgsl`, "one colour, not a
//!   gradient") the rest of the day; open ground at night sits under a
//!   moonlit blue floor while a cave keeps its own; fire is a deeper
//!   orange; and a soft shoulder keeps a sunlit face from clipping its hue
//!   away. Weather takes all of it back toward grey.
//! * **High** is Balanced with the touches that are paid for per pixel
//!   in every frame: a wide halo of scattered light around the sun that
//!   also hazes the distance in that direction, and, when REALISTIC
//!   SHADOWS is on, a softer shadow edge from twice the filter taps.
//!
//! What each step costs is measured in `renderer::lighting_tools`
//! (`what_the_lighting_costs`), and what each looks like is photographed
//! there (`what_the_lighting_looks_like`).
//!
//! ## How a step reaches the GPU
//!
//! **A constant in the shader source, set before it is compiled.** Both
//! `shader.wgsl` and `sky.wgsl` declare `const LIGHTING: u32 = 0u;`, and
//! every addition past Simple sits behind `if (LIGHTING >= 1u)` or
//! `if (LIGHTING >= 2u)`. [`Quality::specialise`] rewrites that one line,
//! the renderer compiles the result, and a branch on a compile-time
//! constant is folded away by every shader compiler this game meets --
//! naga's own constant evaluator first, then FXC, DXC, the SPIR-V driver
//! or the phone's GLSL compiler. Simple does not even rewrite the line: it
//! compiles the text as it is, which is the shader the game already had
//! with some dead code in it.
//!
//! Three ways to do it were weighed:
//!
//! * **A uniform, branched on per pixel.** The cheapest to write and the
//!   one the shadow setting already turned down for the reason that
//!   applies here too: "costs nothing on Simple" would then be a
//!   measurement to repeat on every GPU, and on the phone this game is
//!   aimed at, arithmetic per pixel is precisely what was measured to be
//!   dear (`shader.wgsl`, `mottled_shade`: one extra hash was three
//!   milliseconds).
//! * **More entry points**, the way the shadows did it
//!   (`fs_solid_shadowed`). Right for one on/off switch with one reader;
//!   here it is three steps times solid, cutout, water, items and the two
//!   shadowed receivers, twelve names for what is one idea -- and each of
//!   them still has to pass the step down into the shared `shade_lit`,
//!   where it is a function argument that only *probably* gets folded.
//! * **Pipeline-overridable constants** (`override` in WGSL), which are
//!   exactly this, done by the API. wgpu 0.19 does not expose them, and
//!   the version is not free to move: it is chained to winit and
//!   android-activity (see CLAUDE.md on GameActivity).
//!
//! The price of the chosen way is that changing the setting rebuilds the
//! terrain and sky pipelines -- two shader compiles, once, on the frame
//! the row is pressed -- which is the same bargain `set_shadows` makes.

use std::borrow::Cow;

/// How much the lighting does. See the module note for each step.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Quality {
    Simple,
    Balanced,
    High,
}

/// The line both shaders carry, exactly as written there.
///
/// Matched whole rather than by pattern, so that a shader edit which
/// reformats it fails `every_shader_carries_the_lighting_switch_exactly_once`
/// instead of silently compiling every step as Simple.
const SWITCH: &str = "const LIGHTING: u32 = 0u;";

impl Quality {
    /// Every step, worst to best -- the order the settings row walks, so
    /// that pressing *right* makes the picture better, as on every other
    /// row.
    pub const ALL: [Quality; 3] = [Quality::Simple, Quality::Balanced, Quality::High];

    /// The number the shader's `LIGHTING` constant is set to.
    pub fn level(self) -> u32 {
        match self {
            Quality::Simple => 0,
            Quality::Balanced => 1,
            Quality::High => 2,
        }
    }

    /// Whether anything past the old look is switched on.
    pub fn is_simple(self) -> bool {
        self == Quality::Simple
    }

    /// `source` with its `LIGHTING` constant set to this step.
    ///
    /// **Simple hands the text back untouched**, borrowed: the pipelines
    /// it builds are compiled from the very string they were compiled
    /// from before this setting existed.
    pub fn specialise(self, source: &str) -> Cow<'_, str> {
        if self.is_simple() {
            return Cow::Borrowed(source);
        }
        Cow::Owned(source.replacen(
            SWITCH,
            &format!("const LIGHTING: u32 = {}u;", self.level()),
            1,
        ))
    }
}

/// Simple, everywhere.
///
/// **The rule was Balanced by default if it measured within the noise of
/// Simple, and it does not.** `what_the_lighting_costs`, the savanna at
/// 1920x1080 with four samples on a GTX 1050 Ti: looking into the sunset
/// Balanced is 0.16-0.17 ms over Simple, about five per cent; with the sun
/// behind the camera and a crown of acacia filling the lower half of the
/// frame it is 0.59-0.68 ms, fifteen per cent, and that is not noise.
/// Simple itself is the shader from before the setting to within noise
/// (3.159 against 3.133 ms, 3.950 against 4.029). So a player who never
/// opens the settings pays nothing, and the row is where the warmer light
/// is chosen -- the way the shadows are.
///
/// **Where the cost is, measured rather than guessed**
/// (`LIGHTING_ABLATE`). Taking the whole terrain side of Balanced out
/// brings the frame back to Simple, so it is per-fragment work, not the
/// sky and not the CPU's colours. But taking out any one addition -- the
/// light split, the shoulder, the glow in the fog -- saves at most a tenth
/// of a millisecond of the seven tenths: the price does not add up from
/// its parts. Making each part cheaper (no divide in the split, no `exp`
/// in the shoulder or the glow, the sun's bearing worked out once a frame)
/// moved nothing measurable. Written down so the next attempt starts from
/// the compiler's side -- what `shade_lit`, called from four entry points,
/// becomes once it grows -- rather than from the arithmetic again.
///
/// A phone was never going to start anywhere else: nobody has measured
/// it there, and the phone is where arithmetic per pixel was already
/// found to be dear.
///
/// **Two steps by the clock, three by eye** (re-measured while the
/// graphics presets were being laid out, because a preset that spends a
/// step has to know what the step costs). `what_the_lighting_costs` over
/// the forest at 1920x1080 with four samples on a GTX 1050 Ti, the median
/// frame of each row:
///
/// ```text
///                        simple   balanced   high
/// into the sunset, noon  13.542   14.381     14.408
/// into the sunset, dusk  13.514   14.405     14.402
/// sun behind, noon       13.407   14.186     14.221
/// sun behind, dusk       13.376   14.200     14.206
/// ...the same, shadowed  22.684   23.097     23.213
/// ```
///
/// Simple to Balanced is 0.78-0.89 ms, six per cent, in every view and at
/// every hour -- a step, and the reason this default is Simple. **Balanced
/// to High is not a step by the clock**: 0.03 ms at most and negative in
/// one of the four, which is noise on a 13.4 ms frame. With shadows on it
/// is 0.1-0.2 ms, which is the extra filter taps and is still under one
/// per cent. What High buys is the halo round a low sun and a softer
/// shadow edge, and those are looked at rather than timed
/// (`what_the_lighting_looks_like`). Written down so that nobody prices
/// the top step again from the note above, which was taken over a savanna
/// on a different day and says 0.16-0.68 ms for the *first* step.
pub fn default_quality() -> Quality {
    Quality::Simple
}

impl Default for Quality {
    fn default() -> Self {
        default_quality()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn simple_compiles_the_shader_it_always_compiled() {
        // Not a copy that happens to match: the same borrowed text, so
        // there is nothing a later edit to `specialise` could make differ.
        let source = include_str!("shader.wgsl");
        assert!(matches!(Quality::Simple.specialise(source), Cow::Borrowed(s) if std::ptr::eq(s, source)));
    }

    /// The rewrite is a text match. A shader edit that reformats the line
    /// would leave every step compiling as Simple with nothing failing --
    /// the setting would still move, and nothing would change.
    #[test]
    fn every_shader_carries_the_lighting_switch_exactly_once() {
        for (name, source) in [("shader.wgsl", include_str!("shader.wgsl")), ("sky.wgsl", include_str!("sky.wgsl"))] {
            assert_eq!(source.matches(SWITCH).count(), 1, "{name} does not carry `{SWITCH}` exactly once");
            for quality in [Quality::Balanced, Quality::High] {
                let compiled = quality.specialise(source);
                assert!(!compiled.contains(SWITCH), "{name} at {quality:?} still says Simple");
                let wanted = format!("const LIGHTING: u32 = {}u;", quality.level());
                assert_eq!(compiled.matches(wanted.as_str()).count(), 1, "{name} at {quality:?}");
                assert_eq!(compiled.len(), source.len(), "{name} at {quality:?} changed more than one digit");
            }
        }
    }

    /// The terrain fades into the horizon the sky draws, so the two
    /// shaders must work the horizon out identically -- in every
    /// direction, sunset included -- or the edge of the world is a line.
    #[test]
    fn the_sky_and_the_terrain_draw_the_same_horizon() {
        const FROM: &str = "// ---- the horizon, shared with sky.wgsl ----";
        const TO: &str = "// ---- end of the shared horizon ----";
        let block = |name: &str, source: &str| -> String {
            let start = source.find(FROM).unwrap_or_else(|| panic!("{name} has no shared horizon"));
            let end = source[start..].find(TO).unwrap_or_else(|| panic!("{name}'s shared horizon never ends")) + start;
            source[start..end].replace("\r\n", "\n")
        };
        let terrain = block("shader.wgsl", include_str!("shader.wgsl"));
        let sky = block("sky.wgsl", include_str!("sky.wgsl"));
        assert!(terrain.contains("fn glow_over("), "the shared block lost its function");
        assert_eq!(terrain, sky, "the sky and the terrain disagree about the horizon");
    }

    /// **Every face takes its share of the sun through `half_lambert`**,
    /// because that function is where the night takes the beam's direction
    /// away. A lambert worked out raw from the sun's direction anywhere else
    /// in the terrain shader lights the world from a sun under it: after
    /// sunset the walls facing where it went down were as bright as the open
    /// ground, and in a forest more than twice as bright as the floor under
    /// the crowns, every block edge between the two a hard line.
    #[test]
    fn every_face_takes_its_share_of_the_sun_through_the_one_function_that_knows_it_has_set() {
        let source = include_str!("shader.wgsl");
        let raw = "dot(normal, -globals.sun.xyz)";
        assert_eq!(source.matches(raw).count(), 1, "the sun's direction is dotted with a face normal outside `half_lambert`");
        let start = source.find("fn half_lambert(").expect("shader.wgsl has no `half_lambert`");
        let end = source[start..].find("\n}").expect("`half_lambert` never ends") + start;
        let body = &source[start..end];
        assert!(body.contains(raw), "`half_lambert` does not work out the lambert itself");
        assert!(body.contains("globals.sun.y"), "`half_lambert` no longer asks whether the sun is under the world");
    }

    /// **The blocks' own shade is never taken at a size it cannot be seen
    /// at.**
    ///
    /// `mottled_shade` is a hash of the cell under the fragment with no mip
    /// chain: at two pixels a block it is a texture, and under one pixel a
    /// block it is noise that redraws itself the moment the eye moves --
    /// "при прыжке на дистанции в плоскости видны искажения", a far plain
    /// boiling as a jump lifts the eye. The cure is a fade over the last
    /// sizes at which it reads (`MOTTLE_FADE_FROM`), and the way to lose it
    /// again is to add a second call to the hash that does not go through
    /// the fade.
    ///
    /// Stated on the text of the shader because that is where the property
    /// lives: there is no CPU copy of this to test, and a picture of a
    /// plain that happens not to boil is a picture and not a rule.
    #[test]
    fn the_blocks_own_shade_is_only_taken_where_a_block_is_bigger_than_a_pixel() {
        let source = include_str!("shader.wgsl").replace("\r\n", "\n");
        let calls: Vec<&str> = source.lines().filter(|line| line.contains("mottled_shade(") && !line.contains("fn ")).collect();
        assert_eq!(calls.len(), 1, "the blocks' own shade is worked out in more than one place: {calls:?}");
        assert!(
            calls[0].contains("mix(vec3<f32>(1.0), mottled_shade(in.shade_cell), mottle_seen)"),
            "the hash no longer fades out with the cell's size: {}",
            calls[0]
        );
        assert!(
            source.contains("let mottle_seen = 1.0 - smoothstep(MOTTLE_FADE_FROM, MOTTLE_FADE_TO, cell_px);"),
            "the fade is no longer worked out from the fragment's footprint"
        );
        // Faded out before a block is down to one pixel, and not before it
        // is down to two -- the first is where there is nothing left to
        // alias and the second is where there is still something to see.
        let value = |name: &str| {
            let at = source.find(&format!("const {name}: f32 = ")).unwrap_or_else(|| panic!("shader.wgsl has no {name}"));
            let rest = &source[at + format!("const {name}: f32 = ").len()..];
            rest[..rest.find(';').expect("a constant ends")].parse::<f32>().expect("a number")
        };
        let (from, to) = (value("MOTTLE_FADE_FROM"), value("MOTTLE_FADE_TO"));
        assert!((0.4..=0.6).contains(&from), "the fade starts at {from} blocks a pixel, not at about two pixels a block");
        assert!(to > from && to <= 2.0, "the fade ends at {to} blocks a pixel");
    }

    #[test]
    fn the_steps_run_worst_to_best_and_each_is_its_own_number() {
        let levels: Vec<u32> = Quality::ALL.iter().map(|q| q.level()).collect();
        assert_eq!(levels, vec![0, 1, 2]);
    }

    #[test]
    fn a_step_is_written_to_the_file_by_name_and_read_back() {
        #[derive(serde::Serialize, serde::Deserialize)]
        struct Row {
            lighting: Quality,
        }
        for quality in Quality::ALL {
            let text = toml::to_string(&Row { lighting: quality }).expect("serialise");
            assert!(!text.contains(char::is_numeric), "the file says a number rather than a name: {text}");
            let back: Row = toml::from_str(&text).expect("parse");
            assert_eq!(back.lighting, quality);
        }
    }
}
