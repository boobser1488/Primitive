//! The wind in the grass: plants and leaves bend with the world's wind, in
//! the vertex shader.
//!
//! ## Where the bending is done
//!
//! Three places were weighed:
//!
//! * **Rebuilding the plants' vertices on the CPU each frame.** Exact and
//!   shader-free, and it is the one thing a chunk mesh is built never to
//!   do: the meshes are made once on a worker and live in one arena
//!   (`engine::arena`), and a meadow is the densest geometry in the world.
//!   A frame that rewrote it would be a frame spent uploading grass.
//! * **Moving the picture instead of the geometry** -- a scroll of the
//!   texture coordinate on a plant's upper texels. Free of any geometry at
//!   all, and wrong in the way that matters: the silhouette stays still, so
//!   a meadow in a gale is a meadow of motionless blades with something
//!   crawling inside each one.
//! * **An offset in the vertex shader (chosen).** The mesher writes, on
//!   every vertex of a plant, how far above its root that vertex stands
//!   (`Vertex::swaying`); the shader bends it by that much along the wind.
//!   A root has nought and stays exactly where the mesher put it -- which is
//!   the one property this whole module is built around, and the one the
//!   tests hold: **the base of a stem never moves**. The cost is a branch
//!   every vertex of the terrain takes and nearly every vertex fails, and a
//!   dozen multiplies on the few that pass. **Measured**: a GTX 1050 Ti at
//!   1920x1080 in a storm, the meadow at its thickest -- the cut-out pass
//!   1.968 ms still and 1.978 in the full wind, the solid pass 0.663 and
//!   0.665. A hundredth of a millisecond, inside the noise.
//!
//! ## Why the push is a function of the world position and nothing else
//!
//! A leaf block's faces are merged into rectangles and share corners with
//! their neighbours. If a vertex's bend depended on anything but where it
//! is -- which cell it came from, which face it is on -- two faces meeting
//! at one corner would bend it two ways and the canopy would open cracks
//! with the sky showing through. A bend that is a smooth function of the
//! world position gives every copy of a corner the same answer. What makes
//! two tufts a block apart *not* swing in step is that the function is not
//! slow: the flutter's two waves are a few blocks long, so a block's
//! difference in position is a large difference in phase.
//!
//! ## What the wind is made of
//!
//! * **A steady lean** in the direction the world's wind blows
//!   (`raft::wind`, the wind the rain slants in, the clouds run with and the
//!   sail fills with), as strong as it blows.
//! * **A gust**, a long wave running down-wind across the meadow at a walk:
//!   the lean deepens under it and eases behind it, so a squall is seen
//!   *coming* over the grass before it arrives. This is the picture the
//!   owner asked for ("порыв проходит по лугу волной").
//! * **A flutter**, two short waves at angles to each other, which is what
//!   keeps neighbouring tufts from moving as one sheet.
//!
//! All three are phases a frame hands the shader (`Breeze::uniform`), worked
//! out here in double precision from the render origin and the clock. The
//! shader only ever adds a small local position to them, so a meadow a
//! million blocks from spawn sways as smoothly as one at it, and no GPU is
//! asked for the sine of a six-digit number.
//!
//! ## Rejected
//!
//! * **A wind of the renderer's own**, a time-based wobble that ignores the
//!   weather. It would be a screensaver: grass thrashing on a still morning
//!   says nothing, and the owner's words were that the strength comes from
//!   the world's wind. Here a calm is a calm.
//! * **Leaves bent as hard as grass.** A leaf block is a branch's worth of
//!   leaves on a trunk that does not move; bent a quarter of a block the
//!   crowns wobbled like jelly and parted from their logs. They get a
//!   shiver (`LEAF_REACH`), which is what a canopy in wind looks like from
//!   the ground.

use primitive_shared::raft::Wind;

/// How much a player has asked the wind to move things.
///
/// **Three steps, and the top one is the world as it blows.** There is no
/// step past the world's wind: a setting that exaggerated it would be a
/// setting that lied about the weather, and the wind here is information
/// -- which way the fire will run, which way to sail. `Gentle` is for a
/// player who finds a moving meadow tiring to look at, and `Off` is the
/// picture the game had before and the frame it cost.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Sway {
    Off,
    Gentle,
    #[default]
    Full,
}

impl Sway {
    /// What the world's wind is multiplied by.
    pub fn amplitude(self) -> f32 {
        match self {
            Sway::Off => 0.0,
            Sway::Gentle => 0.45,
            Sway::Full => 1.0,
        }
    }

    /// One step along, held at both ends, for the reason
    /// `Setting::Lighting` walks rather than wraps.
    pub fn step(self, delta: i32) -> Sway {
        const ALL: [Sway; 3] = [Sway::Off, Sway::Gentle, Sway::Full];
        let at = ALL.iter().position(|s| *s == self).unwrap_or(2) as i32;
        ALL[(at + delta).clamp(0, 2) as usize]
    }
}

/// Where a vertex's reach sits in `mesh::Vertex::uv`, and how many steps it
/// has. **Bits 15..18**: a plain cell coordinate leaves bits 15..28 spare, a
/// fine one uses them (so a model's box never sways), and water reads the
/// word its own way (so the shader asks about water first). Must match
/// `SWAY_SHIFT` and `SWAY_STEPS` in shader.wgsl.
pub const SWAY_SHIFT: u32 = 15;
pub const SWAY_STEPS: u32 = 15;
/// The bit that says a vertex belongs to the turf's low blades
/// (`engine::meadow`), which shrink into the ground toward the edge of the
/// ring they are drawn in rather than stopping at it. Must match
/// `COVER_BIT` in shader.wgsl.
pub const COVER_BIT: u32 = 1 << 19;

/// How far a vertex one whole block above its root is pushed at full
/// strength, in blocks, before the gust and the flutter. A tuft is a
/// quarter to a whole block tall, so a meadow's blades lean a few
/// hundredths to a quarter of a block: seen, and never lying flat.
#[cfg_attr(not(test), allow(dead_code))]
pub const REACH: f32 = 0.22;
/// ...and what a leaf block's corners are given in place of a height: a
/// shiver, not a bend. See the module note.
pub const LEAF_REACH: f32 = 2.0 / 15.0;

/// How much of the push is the steady lean and how much comes with a gust.
#[cfg_attr(not(test), allow(dead_code))]
const LEAN: f32 = 0.35;
#[cfg_attr(not(test), allow(dead_code))]
const GUST: f32 = 0.65;
/// How long a gust is from crest to crest, in blocks, and how fast it runs
/// down-wind, in blocks a second -- a walk, so a squall crossing a meadow
/// can be watched.
const GUST_LENGTH: f64 = 22.0;
const GUST_SPEED: f64 = 6.0;
/// The two flutter waves: which way each runs across the world, how long it
/// is, and how fast it turns over. Neither is along an axis, so rows of
/// tufts on the block grid do not flicker as rows.
const FLUTTER_A: (f64, f64, f64, f64) = (0.8, 0.6, 3.3, 4.1);
const FLUTTER_B: (f64, f64, f64, f64) = (-0.447, 0.894, 4.8, 2.9);
/// How much the flutter changes the push along the wind, and how far it
/// swings a tip across it.
#[cfg_attr(not(test), allow(dead_code))]
const FLUTTER_ALONG: f32 = 0.3;
#[cfg_attr(not(test), allow(dead_code))]
const FLUTTER_ACROSS: f32 = 0.12;
/// How far a bent tip sinks, as a share of the square of how far it leant:
/// a blade bends on an arc, not along a rail.
#[cfg_attr(not(test), allow(dead_code))]
const SINK: f32 = 0.35;

/// The reach a vertex this far above its root carries, in `SWAY_STEPS`.
///
/// Capped at a block: the upper half of a tall plant is one plant with its
/// lower half, and its top bends as far as a tuft's does -- the whole block
/// between them is stalk.
pub fn reach_bits(height_above_root: f32) -> u32 {
    let steps = (height_above_root.clamp(0.0, 1.0) * SWAY_STEPS as f32).round() as u32;
    steps << SWAY_SHIFT
}

/// What one frame hands the shader about the wind.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Breeze {
    /// The direction the wind blows toward, flat and unit length (x, z).
    pub toward: [f32; 2],
    /// How hard, 0..1, with the player's step already applied.
    pub strength: f32,
    /// The gust's and the two flutters' phases at the render origin, in
    /// radians and wrapped.
    pub phases: [f32; 3],
}

impl Breeze {
    pub const CALM: Breeze = Breeze { toward: [1.0, 0.0], strength: 0.0, phases: [0.0; 3] };

    /// The wind as this frame bends the grass with it.
    ///
    /// `origin` is the render origin (whole blocks), `seconds` a clock that
    /// only counts up (`FrameParams::elapsed_seconds`).
    pub fn at(wind: Wind, sway: Sway, origin: [f64; 3], seconds: f64) -> Breeze {
        let (sin, cos) = wind.toward.sin_cos();
        let toward = [cos, sin];
        let strength = (wind.strength * sway.amplitude()).clamp(0.0, 1.0);
        let tau = std::f64::consts::TAU;
        let wave = |dir: (f64, f64), length: f64, turn: f64| -> f32 {
            let k = tau / length;
            (k * (origin[0] * dir.0 + origin[2] * dir.1) - turn * seconds).rem_euclid(tau) as f32
        };
        let gust_turn = tau / GUST_LENGTH * GUST_SPEED;
        Breeze {
            toward,
            strength,
            phases: [
                wave((f64::from(toward[0]), f64::from(toward[1])), GUST_LENGTH, gust_turn),
                wave((FLUTTER_A.0, FLUTTER_A.1), FLUTTER_A.2, FLUTTER_A.3),
                wave((FLUTTER_B.0, FLUTTER_B.1), FLUTTER_B.2, FLUTTER_B.3),
            ],
        }
    }

    /// The two rows of the globals block the shader reads this from.
    ///
    /// `cover_line` rides in the spare slot: how far out, in blocks, the
    /// turf's low blades are drawn (`meadow::Density::fade_line`).
    pub fn uniform(&self, cover_line: f32) -> [[f32; 4]; 2] {
        [
            [self.toward[0], self.toward[1], self.strength, cover_line],
            [self.phases[0], self.phases[1], self.phases[2], 0.0],
        ]
    }

    /// Where a vertex ends up, **the shader's arithmetic done here** so the
    /// tests can hold it to its promises without a GPU. `local` is the
    /// vertex relative to the render origin, `reach` its step count as the
    /// mesher wrote it. Must stay the same sums as `sway_offset` in
    /// shader.wgsl; `the_shader_bends_by_the_numbers_this_file_names` checks
    /// the constants.
    #[cfg_attr(not(test), allow(dead_code))]
    pub fn offset(&self, local: [f32; 3], reach: u32) -> [f32; 3] {
        if reach == 0 || self.strength <= 0.0 {
            return [0.0; 3];
        }
        let reach = reach as f32 / SWAY_STEPS as f32 * REACH;
        let d = self.toward;
        let gust = self.gust(local);
        let wave = |w: (f64, f64, f64, f64), phase: f32| {
            let k = std::f32::consts::TAU / w.2 as f32;
            ((local[0] * w.0 as f32 + local[2] * w.1 as f32) * k + phase).sin()
        };
        let flutter = 0.6 * wave(FLUTTER_A, self.phases[1]) + 0.4 * wave(FLUTTER_B, self.phases[2]);
        let push = self.strength * (LEAN + GUST * gust);
        let along = push * (1.0 + FLUTTER_ALONG * flutter);
        let across = self.strength * FLUTTER_ACROSS * flutter;
        [
            reach * (d[0] * along - d[1] * across),
            -reach * SINK * along * along,
            reach * (d[1] * along + d[0] * across),
        ]
    }

    /// How far into a gust a point is, 0 (between gusts) to 1 (under its
    /// crest): the long wave that runs down-wind across a meadow.
    #[cfg_attr(not(test), allow(dead_code))]
    pub fn gust(&self, local: [f32; 3]) -> f32 {
        let along = local[0] * self.toward[0] + local[2] * self.toward[1];
        let k = std::f32::consts::TAU / GUST_LENGTH as f32;
        let crest = 0.5 + 0.5 * (along * k + self.phases[0]).sin();
        crest * crest * (3.0 - 2.0 * crest)
    }

    /// The constants the shader has to agree with, by the names it gives
    /// them. See `the_shader_bends_by_the_numbers_this_file_names`.
    #[cfg(test)]
    fn named_constants() -> Vec<(&'static str, f64)> {
        vec![
            ("SWAY_SHIFT", f64::from(SWAY_SHIFT)),
            ("SWAY_STEPS", f64::from(SWAY_STEPS)),
            ("COVER_BIT", f64::from(COVER_BIT)),
            ("SWAY_REACH", f64::from(REACH)),
            ("SWAY_LEAN", f64::from(LEAN)),
            ("SWAY_GUST", f64::from(GUST)),
            ("SWAY_GUST_LENGTH", GUST_LENGTH),
            ("SWAY_FLUTTER_ALONG", f64::from(FLUTTER_ALONG)),
            ("SWAY_FLUTTER_ACROSS", f64::from(FLUTTER_ACROSS)),
            ("SWAY_SINK", f64::from(SINK)),
            ("SWAY_A_X", FLUTTER_A.0),
            ("SWAY_A_Z", FLUTTER_A.1),
            ("SWAY_A_LENGTH", FLUTTER_A.2),
            ("SWAY_B_X", FLUTTER_B.0),
            ("SWAY_B_Z", FLUTTER_B.1),
            ("SWAY_B_LENGTH", FLUTTER_B.2),
        ]
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn gale() -> Breeze {
        Breeze::at(Wind { toward: 0.7, strength: 1.0 }, Sway::Full, [1_000_000.0, 0.0, -2_000_000.0], 12_345.6)
    }

    #[test]
    fn the_root_of_a_stem_never_moves_in_any_wind() {
        for sway in [Sway::Off, Sway::Gentle, Sway::Full] {
            let breeze = Breeze::at(Wind { toward: 2.1, strength: 1.0 }, sway, [5.0, 0.0, 9.0], 77.0);
            for x in -20..20 {
                for z in -20..20 {
                    let at = [x as f32 * 0.37, 64.0, z as f32 * 0.53];
                    assert_eq!(breeze.offset(at, 0), [0.0; 3], "a root at {at:?} moved");
                }
            }
        }
    }

    #[test]
    fn the_tip_of_a_tuft_leans_the_way_the_wind_blows() {
        let breeze = gale();
        let tip = breeze.offset([3.0, 60.0, 4.0], SWAY_STEPS);
        let along = tip[0] * breeze.toward[0] + tip[2] * breeze.toward[1];
        assert!(along > 0.05, "a tip in a gale leant {along} blocks down-wind");
        assert!(along < 0.4, "a tip in a gale leant {along} blocks -- flattened, not bent");
        assert!(tip[1] < 0.0, "a bent tip did not sink on its arc");
    }

    #[test]
    fn a_calm_is_a_calm_and_off_is_off() {
        let still = Breeze::at(Wind { toward: 1.0, strength: 0.0 }, Sway::Full, [0.0; 3], 10.0);
        assert_eq!(still.offset([1.0, 1.0, 1.0], SWAY_STEPS), [0.0; 3]);
        let off = Breeze::at(Wind { toward: 1.0, strength: 1.0 }, Sway::Off, [0.0; 3], 10.0);
        assert_eq!(off.offset([1.0, 1.0, 1.0], SWAY_STEPS), [0.0; 3]);
        let gentle = Breeze::at(Wind { toward: 1.0, strength: 1.0 }, Sway::Gentle, [0.0; 3], 10.0);
        let full = Breeze::at(Wind { toward: 1.0, strength: 1.0 }, Sway::Full, [0.0; 3], 10.0);
        let size = |o: [f32; 3]| (o[0] * o[0] + o[2] * o[2]).sqrt();
        assert!(size(gentle.offset([1.0, 1.0, 1.0], SWAY_STEPS)) < size(full.offset([1.0, 1.0, 1.0], SWAY_STEPS)));
    }

    #[test]
    fn neighbouring_tufts_do_not_swing_in_step() {
        // Sixteen tufts in a row, one a block, watched over a second: if
        // they moved as one sheet every pair would have the same offset at
        // every moment.
        let mut differs = 0;
        for frame in 0..30 {
            let breeze = Breeze::at(Wind { toward: 0.3, strength: 0.8 }, Sway::Full, [0.0; 3], frame as f64 / 30.0);
            let tips: Vec<[f32; 3]> = (0..16).map(|x| breeze.offset([x as f32 + 0.5, 70.0, 0.5], 8)).collect();
            differs += tips.windows(2).filter(|w| (w[0][0] - w[1][0]).abs() + (w[0][2] - w[1][2]).abs() > 0.004).count();
        }
        assert!(differs > 30 * 15 / 2, "only {differs} of {} neighbouring pairs differed", 30 * 15);
    }

    #[test]
    fn a_gust_runs_down_wind() {
        // Where the gust is deepest along a line down-wind, a moment later
        // is further down-wind.
        let crest = |seconds: f64| {
            let breeze = Breeze::at(Wind { toward: 0.0, strength: 1.0 }, Sway::Full, [0.0; 3], seconds);
            (0..220).map(|i| i as f32 * 0.1).max_by(|a, b| breeze.gust([*a, 70.0, 0.0]).total_cmp(&breeze.gust([*b, 70.0, 0.0]))).unwrap()
        };
        let (now, later) = (crest(10.0), crest(10.5));
        let moved = (later - now).rem_euclid(GUST_LENGTH as f32);
        assert!((1.0..6.0).contains(&moved), "the gust's crest moved {moved} blocks in half a second");
    }

    #[test]
    fn moving_the_render_origin_does_not_move_the_grass() {
        // The same world point, drawn round two origins sixteen blocks
        // apart: the bend must be the same, or every step the origin takes
        // would jerk the whole meadow.
        let wind = Wind { toward: 1.3, strength: 0.9 };
        let a = Breeze::at(wind, Sway::Full, [1_000_000.0, 0.0, 500_000.0], 99.0);
        let b = Breeze::at(wind, Sway::Full, [1_000_016.0, 0.0, 500_000.0], 99.0);
        let here = a.offset([20.5, 70.0, 3.25], SWAY_STEPS);
        let there = b.offset([4.5, 70.0, 3.25], SWAY_STEPS);
        for i in 0..3 {
            assert!((here[i] - there[i]).abs() < 1e-3, "{here:?} against {there:?}");
        }
    }

    #[test]
    fn the_shader_bends_by_the_numbers_this_file_names() {
        let shader = include_str!("shader.wgsl");
        for (name, value) in Breeze::named_constants() {
            let line = shader
                .lines()
                .find(|l| l.trim_start().starts_with(&format!("const {name}:")))
                .unwrap_or_else(|| panic!("shader.wgsl has no `const {name}`"));
            let number: f64 = line
                .split('=')
                .nth(1)
                .and_then(|v| v.trim().trim_end_matches(';').trim_end_matches('u').parse().ok())
                .unwrap_or_else(|| panic!("could not read the number in `{line}`"));
            assert!((number - value).abs() < 1e-4, "{name} is {number} in the shader and {value} here");
        }
    }

    #[test]
    fn a_reach_fits_its_bits_and_leaves_the_layer_alone() {
        assert_eq!(reach_bits(0.0), 0);
        assert_eq!(reach_bits(5.0), SWAY_STEPS << SWAY_SHIFT);
        let top = crate::engine::mesh::LAYER_TOP_SHIFT;
        assert!(SWAY_SHIFT + 4 <= 19 && 19 < top, "the reach and the cover bit must sit below the layer's top bits");
        assert_eq!(reach_bits(1.0) & COVER_BIT, 0);
    }
}
