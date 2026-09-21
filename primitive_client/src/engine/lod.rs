//! Distant chunks, meshed out of bigger blocks.
//!
//! ## Why this exists, and what it is not
//!
//! The solid pass is bound by **triangle count**, not by pixels. The
//! measurement that decided this feature: at 1280x720 the pass sends
//! 1.35 million triangles behind 0.9 million pixels, and putting *ten
//! times* the pixels in front of it costs 19% more. The average terrain
//! triangle is smaller than a pixel and is billed at the fixed 2x2
//! fragment quad the rasteriser works in, so the only lever that moves
//! the number is fewer and larger triangles at range.
//!
//! This is therefore not a fidelity setting with a speed side effect. It
//! is the one remaining lever on the pass that costs the most.
//!
//! ## The mechanism, in one sentence
//!
//! A distant chunk's *neighbourhood* -- the padded block-and-light box
//! the mesher reads -- is rewritten so that every 2x2 column of cells
//! holds one material and one light level, and then the **ordinary
//! mesher runs on it unchanged**.
//!
//! **That is the whole design, and the reason for it is risk.** The
//! alternative was a second mesher that emits big quads directly, and it
//! would have had to re-earn everything `mesh.rs` already gets right:
//! winding (`FrontFace::Ccw`, and a box wound inside out draws its far
//! side through its near one), the face index that carries the lighting
//! normal, the five-bit tiling UV, the greedy merge, the T-junction
//! closing, the direction groups `renderer::solid_ranges_facing` culls
//! by. Coarsening the *input* instead means none of those can be got
//! wrong here: whatever comes out is a correct mesh of a blockier world.
//!
//! The reduction then comes from the greedy merge (see
//! `mesh::MERGE_COPLANAR_FACES`) having far less to disagree about: four
//! neighbouring columns that used to differ by a block of height, a
//! material or a light level now hold exactly the same thing, so one
//! rectangle covers what four used to.
//!
//! **Coarsening the blocks is only half of it**, and finding that out
//! was the whole of this feature's difficulty. The merge joins two faces
//! only when all four of their corners agree about light *and* ambient
//! occlusion, and occlusion is worked out per cell from the eight cells
//! around it. On natural ground almost every top face has a step
//! somewhere on its edge, so a chunk of meadow merges 256 top faces into
//! about 240 quads however simple its blocks are -- measured, at 7% off
//! the solid pass for the coarsening alone. The other half is that a
//! coarse chunk is **lit flat**: see `Neighbourhood::mark_coarse` and
//! `flat_lit` in `build_mesh`. With both, the same band gives up nearly
//! half its triangles.
//!
//! ## Seams: why there is no hole, ever
//!
//! Two rules, and between them they make the boundary a non-event.
//!
//! 1. **A coarse cell is opaque if *any* of its cells was.** So the
//!    volume a coarse chunk draws is a *superset* of the real solid
//!    volume it stands for. Never a subset -- that is the direction that
//!    opens holes, and it is the reason this is not "take the majority",
//!    which looks better on a slope and lets you see through the world
//!    at the boundary.
//!
//! 2. **Only the chunk's own cells are coarsened; the one-block ring of
//!    padding around them stays real.** A face at the seam is therefore
//!    culled exactly when the *real* neighbouring block is opaque -- and
//!    a real opaque block is inside its own chunk's coarse cell by rule
//!    1, whatever level that chunk is meshed at. So the far side is
//!    always drawing something there.
//!
//! Put together: chunk A hides a face only where chunk B has solid
//! matter, and chunk B's solid matter is never less than the world's. A
//! chunk's mesh depends on nothing but its own level, so **changing one
//! chunk's level never dirties its neighbours** and there is no cascade
//! to schedule.
//!
//! What the player does see at the boundary is the ground being up to a
//! block fatter on the far side of it -- a low terrace, not a gap. That
//! is the price of rule 1, and it is the right one to pay: at ten chunks
//! a block is four pixels.
//!
//! ## Why the vertical axis is left alone
//!
//! The obvious form of this is 2x2x2, and it is not what runs. Two
//! reasons, in order of weight:
//!
//! * **A step in height is what a player sees.** Coarsening x and z
//!   makes a distant hillside blockier from above; coarsening y moves
//!   the *skyline*, and a whole ring of the world rising a block as you
//!   walk toward it is exactly the artefact this feature has to not
//!   have. Horizontally, flat ground -- which is most of a plains world
//!   -- comes out at precisely the height it really is.
//! * The neighbourhood is only filled up to the chunk's own skyline plus
//!   one plane (`Neighbourhood::fill`), so a cell that grew upward out
//!   of that band would be meshed against whatever the previous chunk to
//!   use the pooled buffer left there.
//!
//! The vertical factor is still in the table, and `coarsen` honours it,
//! because "we tried it and it costs more than it buys" is worth being
//! able to re-measure rather than re-derive -- and it does cost more:
//! 2x2x2 came out at 268k solid triangles where 2x1x2 came out at 236k,
//! for the reason under `CELL`.

use primitive_shared::types::{
    block_kind, is_branch, is_cross, is_cutout, is_flat, is_liquid, is_opaque, BlockId, BLOCK_AIR,
    BLOCK_BIRCH_LOG, BLOCK_LOG, BLOCK_STRIPPED_LOG, CHUNK_SIZE_X, CHUNK_SIZE_Y, CHUNK_SIZE_Z,
};

use primitive_shared::worldgen::SEA_LEVEL;

use crate::engine::mesh::Neighbourhood;

/// The coarsest level there is. Two bands.
pub const MAX_LEVEL: u8 = 2;

/// Blocks per coarse cell at each level: (horizontal, vertical).
///
/// **Both coarse bands are 2x2, and that is a measurement rather than an
/// oversight.** Over 361 chunks of the world `far` around the shore the
/// benchmark stands on (see `what_a_coarse_chunk_costs`), solid
/// triangles came out:
///
/// ```text
/// full detail   449_621
/// 2x2           235_860     -48%
/// 4x4           278_724     -38%   <- worse than 2x2
/// 2x2x2         268_364     -40%   <- worse than 2x2
/// ```
///
/// **4x4 goes backwards, and the reason is relief.** A coarse column
/// stands as tall as the *tallest* of the columns it swallows (it has
/// to; see the module note on why nothing may get shorter), and the
/// maximum of sixteen samples overshoots much further than the maximum
/// of four. So the ground gains height variation rather than losing it:
/// the tops do get cheaper, and the walls between the plateaus more than
/// spend it -- +X, -X, +Z and -Z each grew by a fifth going from 2x2 to
/// 4x4. It also fattens every thin upright thing of rock -- a spire --
/// into a four-block block, which is an artefact a player can point at.
/// Trunks are not fattened at any cell size: they are carried through
/// whole, because a fattened trunk was the one a player *did* point at
/// (see `carried_whole`).
///
/// Coarsening y loses for the same reason and a second one; see the
/// module note.
///
/// So the far band is not coarser geometry. What it is instead is the
/// band where the grass stops being meshed at all -- see
/// `keeps_sprites`, which is where its saving comes from.
const CELL: [(usize, usize); MAX_LEVEL as usize + 1] = [(1, 1), (2, 1), (2, 1)];

/// How far a chunk has to travel past a threshold before its level
/// changes, in chunks.
///
/// **Without this a chunk sitting on the line rebuilds every time the
/// player steps across a block boundary**, because the distance is a
/// float and the threshold is a number it wanders either side of. That
/// is a remesh, an arena upload and a visible pop, several times a
/// second, for a chunk at the edge of sight. A whole chunk of margin
/// means crossing it takes sixteen blocks of walking in one direction.
const HYSTERESIS: f32 = 1.0;

/// How far from the player, in chunks, a stone lying on the ground keeps its
/// thickness (`engine::relief`); further out it is the flat quad again.
///
/// **Why there has to be a line at all.** A stone with a thickness is forty
/// quads where the flat one was one, and there are a great many of them: 27
/// a chunk on the plains, 41 in an oak wood, 48 in a birch wood or a taiga
/// (`relief::cost::what_stones_cost_the_mesher`). Given to every chunk of
/// the benchmark world at render distance 24, the world went from 6.17 to
/// 8.83 million triangles and the arena from 290 to 422 MB; kept out of the
/// coarse bands only, 6.99 million and 331 MB; inside this line, 6.33
/// million and 298 MB, of which the chunks within three of the player hold
/// 83 thousand triangles more than they did flat. All that -- for thickness
/// nobody can see: a stone's rim is a texel of a picture laid 0.74 of a
/// block wide, 0.046 of a block, which at 1920x1080 and a field of view of
/// ninety is a pixel at twenty-five blocks and half of one at fifty.
///
/// **Why at the mesher and not the draw.** Keeping the sides in a range of
/// their own and not drawing it past the line would have saved the GPU the
/// triangles and kept every megabyte, and memory was the larger half of the
/// bill. The price of deciding it here is one more ring of chunks rebuilt
/// as the player walks -- the chunks crossing *this* line, which is a ring a
/// quarter the circumference of the first coarse band's -- rebuilt by the
/// same machinery, `restripe_detail_levels`, that the bands already use.
///
/// **Four**, with `HYSTERESIS` either side: a chunk gains its stones'
/// thickness once it is within three chunks -- nearest block about forty
/// blocks off, where the rim is under a pixel and a half at 1080p -- and
/// gives it up past five. Nearer than that the change would be visible as
/// the stones in front of a walking player popping up out of the ground.
///
/// **The default of a setting now** (`ClientSettings::relief_chunks`), not
/// the line itself: a weak machine gives the thickness up nearer or
/// altogether, and zero is exactly the flat quads the game drew before
/// stones had any.
pub const RELIEF_CHUNKS: i32 = 4;

/// Whether a chunk `distance_chunks` away is inside a line `line_chunks`
/// out, given whether it was built as inside. A Schmitt trigger for
/// `level_at`'s reason (see `HYSTERESIS`), and a line at zero or below is
/// no line: nothing is inside it.
///
/// One function for the two lines that are the player's to move -- the
/// stones' thickness and the see-through canopy -- so the two cannot come
/// to disagree about what "a chunk past the line" means.
fn inside_line(distance_chunks: f32, line_chunks: i32, current: bool) -> bool {
    if line_chunks <= 0 {
        return false;
    }
    let line = line_chunks as f32;
    if current {
        distance_chunks < line + HYSTERESIS
    } else {
        distance_chunks < line - HYSTERESIS
    }
}

/// How much of a stone's thickness a chunk gets.
///
/// **The stones were the most expensive thing in the frame, and nobody
/// knew.** World `night` at noon, 1280x720, render distance 13, MSAA 4 on a
/// GTX 1050 Ti, with the player's own `relief_chunks = 8`: the whole cut-out
/// pass took 1.79 ms of a 3.06 ms GPU frame, and drawing the loose stones as
/// flat quads (`relief_chunks = 0`) took it to 0.93 -- **0.86 ms, 48% of the
/// pass and 28% of the frame, for pebbles**. The leaves were 0.58 of it and
/// all the grass past forty blocks 0.20. Measured by ablation because the
/// pass is not fragment-bound and not fill-bound either: the speck hunt,
/// which replaces the whole fragment shader with a flat colour, takes it
/// only to 1.56, and a quarter of the pixels only to 1.36. It is triangles,
/// and a stone is 116 to 216 of them (`engine::relief`).
///
/// The cost is all in the stops past the default: 0.14 ms at four chunks,
/// 0.48 at six, 0.86 at eight. Which is the shape of the thing -- area grows
/// with the square of the line -- and also the shape of the argument, since
/// a stone at six chunks is four pixels across and its crown is a quarter of
/// one.
///
/// So the line the setting moves stays where the player put it, and what it
/// carries out there is the silhouette in one tier rather than two
/// (`Relief::slab`, 47% fewer triangles). Full relief ends at
/// [`RELIEF_CHUNKS`] -- the default, and the distance that was measured as
/// the last at which a rim is more than a pixel.
///
/// Rejected, and why:
///
/// * **Dropping the stops past four.** Honest about the cost and a loss to
///   the player: at eight chunks a slab still reads as a stone lying in the
///   grass where a flat quad reads as a stain, which is the whole complaint
///   the relief was written for.
/// * **Back-face culling the relief, or filing its facets under the six
///   face directions the way the terrain is** (`renderer::solid_ranges_facing`).
///   Both take away triangles the rasteriser was going to reject anyway, and
///   this repository has already measured that as worth nothing: the facing
///   rule cut 15% of the solid pass's triangles for 0.005 ms.
/// * **The model as it shipped before the seams** -- one quad for the rim's
///   whole top, sides carried across a line. Counted on the real pictures it
///   is 97 quads to the exact surface's 68 for a pebble at the second tier:
///   the saving was the top, and the tops are not where the quads are.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum StoneDetail {
    /// The flat quad the game drew before stones had a thickness.
    #[default]
    Flat,
    /// The silhouette one texel thick: `Relief::slab`.
    Slab,
    /// The two-tier surface: `Relief::drawn`.
    Full,
}

/// How much thickness a chunk `distance_chunks` away should be meshed with,
/// given the player's line and what it was meshed with before.
///
/// Two lines, each with `inside_line`'s hysteresis, so neither can flicker
/// as a player walks the boundary. A setting nearer than [`RELIEF_CHUNKS`]
/// is the whole of the answer -- there is no band left for the slab, and a
/// player who moved the line in asked for less, not for a cheaper more.
///
/// **What the second line costs is a second ring of remeshes**, and that is
/// written down rather than dressed up: a player who has moved the setting
/// out now crosses two boundaries instead of one, so the ring at four chunks
/// rebuilds where it used to sit still. It goes through the ordinary dirty
/// queue on the mesher's own budget, like the ring at the player's line that
/// was always there, and it is the chunks at four chunks rather than at
/// eight -- a quarter of the ring, since a ring's length grows with its
/// radius.
pub fn stones_at(distance_chunks: f32, relief_chunks: i32, current: StoneDetail) -> StoneDetail {
    if !inside_line(distance_chunks, relief_chunks, current != StoneDetail::Flat) {
        return StoneDetail::Flat;
    }
    let full_line = relief_chunks.min(RELIEF_CHUNKS);
    if inside_line(distance_chunks, full_line, current == StoneDetail::Full) {
        StoneDetail::Full
    } else {
        StoneDetail::Slab
    }
}

/// The setting's value for "see-through at any distance": further than any
/// render distance reaches, so it needs no case of its own anywhere a
/// distance is compared with it.
pub const LEAVES_SEE_THROUGH_EVERYWHERE: i32 = 1000;

/// Whether a chunk `distance_chunks` away keeps its canopy see-through --
/// cut out, with the insides of its crowns -- or has it meshed as a solid
/// shell drawn in the opaque pass. `leaf_chunks` is
/// `ClientSettings::transparent_leaves_chunks`: zero is solid everywhere,
/// `LEAVES_SEE_THROUGH_EVERYWHERE` see-through everywhere.
///
/// **Decided at the mesher, with a chunk of hysteresis, and not in the
/// shader or the draw.** Three places the line could be:
///
/// * *In the shader, by distance* -- no `discard` past the line, a filled
///   hole instead. One pipeline and no remesh, and the worst of both: a
///   shader that *can* discard loses early-Z on every fragment it runs,
///   near or far, so the far canopy saves nothing, and the crowns keep
///   their insides.
/// * *In the draw, by chunk distance* -- what the renderer did before this
///   was a setting: a hidden line at 0.45 of the view distance past which
///   a chunk's leaf range went through the opaque pipeline. Instant and
///   free to move, and it could not touch the geometry: a solid crown
///   still carried every face between two of its own leaf cells, which a
///   solid crown never shows -- half the canopy's triangles in the
///   benchmark wood (see the CHANGELOG).
/// * **At the mesher (chosen).** Past the line a chunk is rebuilt with its
///   crowns as shells -- the rule a coarse chunk has always used
///   (`mesh::face_visible`) -- and the mesh itself says its leaves are
///   solid, so the renderer draws what was built and a shell is never cut
///   out (a cut-out shell shows sky through its holes, into a hollow tree).
///   The price is the stones' price: a ring of chunks rebuilt as the player
///   walks, through `restripe_detail_levels`, which the hysteresis keeps
///   from being one chunk on the line rebuilt at every step.
pub fn leaves_see_through_at(distance_chunks: f32, leaf_chunks: i32, current: bool) -> bool {
    leaf_chunks >= LEAVES_SEE_THROUGH_EVERYWHERE || inside_line(distance_chunks, leaf_chunks, current)
}

/// Blocks per coarse cell at `level`, horizontally and vertically.
pub fn cell_size(level: u8) -> (usize, usize) {
    CELL[(level as usize).min(MAX_LEVEL as usize)]
}

/// What level a chunk `distance` chunks away should be meshed at, given
/// the level it is currently built at.
///
/// `lod_chunks` is the setting: the distance at which the first coarse
/// band starts, with each further band one multiple further out. Zero
/// turns the whole thing off, and that is a real answer rather than a
/// disabled feature -- a small render distance never reaches the first
/// threshold either.
///
/// `current` is what the chunk is built at now, and it is what makes
/// this a Schmitt trigger rather than a comparison: a chunk only becomes
/// coarser once it is a chunk *past* the threshold, and only becomes
/// finer once it is a chunk inside it. See `HYSTERESIS`.
pub fn level_at(distance_chunks: f32, lod_chunks: i32, current: u8) -> u8 {
    if lod_chunks <= 0 {
        return 0;
    }
    let mut level = 0;
    for step in 1..=MAX_LEVEL {
        let threshold = lod_chunks as f32 * step as f32;
        // Coming from inside, the bar is higher; from outside, lower.
        let bar = if current >= step {
            threshold - HYSTERESIS
        } else {
            threshold + HYSTERESIS
        };
        if distance_chunks < bar {
            break;
        }
        level = step;
    }
    level
}

/// The skyline at and above which a chunk counts as mountain for
/// `band_start`: thirty-two blocks over the sea.
///
/// Leaves are in the skyline, so a forest on the plain has to stay under
/// it: plains ground stands at most seventeen over the sea and a crown
/// a dozen over that. Around the benchmark's shore 8 of the 373 nearest
/// chunks are over the line; around the highest ground near spawn, 312.
pub const TALL_SKYLINE: i32 = SEA_LEVEL + 32;

/// How much of the setting a mountain's first band starts at.
const TALL_SHARE: f32 = 0.5;

/// The nearest a mountain's first band may start, in chunks, unless the
/// setting itself is nearer. Sixty-four blocks: past the reach of
/// anything a player is doing, and the smallest value the setting row
/// offers, so no mountain goes coarser than a player could have asked for.
const TALL_FLOOR: i32 = 4;

/// Where a chunk's first coarse band starts, given the setting and the
/// chunk's skyline (`Neighbourhood::ceiling`).
///
/// **A mountain leaves full detail nearer than a meadow does**, because
/// that is where the triangles are. With the world 256 tall and the
/// ranges ninety blocks over the sea, the same render distance centred
/// on the highest ground near spawn (seed 4242) meshes 6.1 million
/// triangles where the benchmark's shore meshes 3.5
/// (`arena::world_cost::what_the_mountains_cost_band_by_band`). Of the
/// solid pass there the full-detail band is a third, and it is where a
/// hillside costs most for least: a chunk of slope is 3.0k triangles at
/// full detail and 1.2k at 2x2, a bare peak 3.1k and 1.0k -- where a
/// chunk of meadow goes from 2.2k to 0.9k.
///
/// **The cell does not get coarser, only nearer**, and that is measured
/// too. The same mountain, every chunk of the first coarse band meshed at
/// every cell, solid triangles in thousands:
///
/// ```text
///                     2x2    4x4   2x2x2   4x4x2    8x8
/// slopes  (sea+32..64) 1007   1214    1008    1269   1410
/// peaks   (sea+64 up)   100     70      78      69     67
/// ```
///
/// A slope is walls, and a bigger cell stands as tall as the tallest
/// column it swallows (see `CELL`), so it grows walls faster than it
/// saves tops -- the meadow's finding, made worse by relief. Only bare
/// peaks gain from a bigger cell, and there are few enough of them that
/// giving them 4x4 buys about 1% of the pass. Not worth a second rule.
///
/// **What the two numbers were chosen by**, solid triangles over the same
/// 1793 chunks, the bands where the setting puts them against the bands a
/// mountain gets:
///
/// ```text
///                              highest ground     benchmark shore
/// setting alone                3347k              2144k
/// sea+32, half the setting     2968k  -11%        2130k  -0.7%   <- shipped
/// sea+24, 0.4 of the setting   2867k  -14%        2084k  -2.8%
/// sea+42, half the setting     2990k  -11%        2136k
/// ```
///
/// Twenty-four over the sea saves a little more in the mountains, and it
/// does it by catching forests on the plain -- the shore loses three
/// times as much, which is a meadow's crowns going blocky seventy blocks
/// from a player who never went near a hill. Thirty-two leaves the shore
/// all but untouched; forty-two gives up part of the mountain for nothing.
///
/// **The skyline rather than the relief**, because the skyline is free:
/// `Neighbourhood::fill` works it out for its own reasons, and the relief
/// would need every column's surface found. What that costs is a high
/// plateau being coarsened sooner than it strictly needs to -- a little
/// picture on the rare flat top, and nothing else.
///
/// **Seams need nothing new.** A chunk's mesh depends only on its own
/// level, and a coarse chunk beside a fine one is hole-free whatever the
/// two levels are (see the module note), so a mountain going coarse
/// before the valley beside it is one more pair of levels meeting.
pub fn band_start(lod_chunks: i32, skyline: i32) -> i32 {
    band_start_with(lod_chunks, skyline, TALL_SKYLINE, TALL_SHARE)
}

/// `band_start` with its two numbers given, so the measurement that chose
/// them can walk the alternatives over the same chunks.
pub(crate) fn band_start_with(lod_chunks: i32, skyline: i32, tall_from: i32, share: f32) -> i32 {
    if lod_chunks <= 0 || skyline < tall_from {
        return lod_chunks;
    }
    let nearer = (lod_chunks as f32 * share).round() as i32;
    nearer.max(lod_chunks.min(TALL_FLOOR))
}

/// Whether tufts of grass, flowers and loose stones are meshed at all at
/// this level.
///
/// **This is the whole of what the second band is**, since both bands
/// coarsen the ground by the same 2x2 (see `CELL`).
///
/// **They are not simplified, they are dropped**, and the reason is that
/// there is nothing to simplify: a tuft is two crossed quads with a
/// mostly-empty texture, and half a tuft is not a cheaper tuft. A field
/// is a tuft every three columns, which makes them the densest thing in
/// the world by count and the least worth having at range.
///
/// Kept in the first coarse band and dropped in the second, on purpose:
/// with the stock settings the second band starts twenty chunks out --
/// three hundred and twenty blocks, where a tuft is two pixels tall and,
/// at a render distance of 24, the fog has taken a third to a half of the
/// colour (it begins at three quarters of the streamed reach; see
/// `ClientSettings::fog_range`). The first band starts at ten
/// chunks, where a tuft is still four pixels and its disappearance would
/// be a line moving through the grass in front of the player.
///
/// This is a floor and not a replacement for `detail_distance`, which
/// the player sets and which can be nearer than this.
pub fn keeps_sprites(level: u8, quality: Quality) -> bool {
    match quality {
        // Nothing is dropped: the far band differs from the near one
        // only in that there is nothing left to differ in. A player who
        // set this asked for the picture.
        Quality::Fine => true,
        Quality::Normal => level < 2,
        // Dropped from the first band as well, which is where the grass
        // actually costs something: a field is a tuft every third
        // column, and ten chunks out that is thousands of quads for two
        // pixels each.
        Quality::Coarse => level < 1,
    }
}

/// How much a coarse chunk is allowed to give up.
///
/// **Three steps rather than a number**, because the decision a player
/// is making is "I would rather have the picture" or "I would rather
/// have the frames", and 0.7 is not that sentence.
///
/// What each step is made of is measured rather than invented -- see
/// `CELL` for the geometry and `Neighbourhood::mark_coarse` for the
/// light:
///
/// * `Fine` keeps the **real lighting** on coarse chunks. That is the
///   expensive half to give up: flat light is what lets the greedy
///   mesher weld neighbouring faces at all, and without it coarsening
///   the blocks alone measured 7% of the solid pass instead of 48%. It
///   is also the half a player can *see* -- a gradient across a distant
///   hillside -- so it is the one the top setting keeps.
/// * `Normal` is what the feature was built as: flat light, and the
///   grass stops at the second band.
/// * `Coarse` drops the grass at the first band too. Nothing else is
///   made coarser, because everything else that could be was measured
///   and came out *worse* (4x4 and 2x2x2, both under `CELL`).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Quality {
    Fine,
    /// What the feature was built as, and what a player who never opens
    /// the settings gets.
    #[default]
    Normal,
    Coarse,
}

impl Quality {
    /// Every step, in the order the settings row walks them.
    pub const ALL: [Quality; 3] = [Quality::Fine, Quality::Normal, Quality::Coarse];

    /// Whether a coarse chunk keeps the light it really has.
    ///
    /// See the note on the enum: this is the half of the saving that
    /// matters and the half that shows, which is why it is what the top
    /// step buys back.
    pub fn flat_light(self) -> bool {
        !matches!(self, Quality::Fine)
    }

    /// What it is called in a message.
    ///
    /// **Only tests read it**: the settings file writes the name serde
    /// derives, and the interface looks up a translated phrase by the
    /// variant (`Msg::LodFine` and friends). It is here because a test
    /// that fails saying "false is not true" is worse than one that
    /// names the step it was asking about.
    #[cfg(test)]
    pub fn name(self) -> &'static str {
        match self {
            Quality::Fine => "fine",
            Quality::Normal => "normal",
            Quality::Coarse => "coarse",
        }
    }
}

/// Whether a block is a tree's wood, which coarsening carries through at
/// full resolution instead of folding it into its cell.
///
/// **A trunk was the one thing a coarse cell turned into a post.** A log
/// counted toward the cell's material like rock, so in every layer of a
/// 2x2 cell where the trunk was the only solid block, the other three
/// cells became log as well: a one-block trunk stood as a 2x2 column, and
/// every pocket of air a tapering crown leaves beside its trunk filled
/// with bark, so brown cubes stood out of the top and sides of the
/// leaves. Ten chunks out that was behind the fog. Once a mountain left
/// full detail at half the setting (`band_start`), it was drawn on every
/// fir on a slope ninety blocks from the player -- the likeliest reading
/// of the report's bare brown posts standing on the rock.
/// `renderer::lod_repro` photographs it.
///
/// **Carried, like the leaves, and not counted.** Carried, a trunk is
/// exactly as thick as it is. Not counted, the air beside it stays air
/// unless something else solid shares the cell. Carried but still
/// counted would keep the trunk and fill the air round it with bark
/// anyway -- that half is the post.
///
/// The seam does not notice. Its rule is that no solid block the chunk
/// next door culls against may disappear (see the module note), and a
/// carried log is the real log.
///
/// Branch pieces for the same reason and a worse symptom: they are not
/// cubes, so they fell into "everything else" and became the cell's rock,
/// or air -- a far tree with no limbs.
fn carried_whole(id: BlockId) -> bool {
    primitive_shared::wood::is_log(id) || matches!(block_kind(id), BLOCK_LOG | BLOCK_BIRCH_LOG | BLOCK_STRIPPED_LOG) || is_branch(id)
}

/// Rewrites a chunk's own cells so that each `cell_size` block of them
/// holds one material and one light level.
///
/// Runs on a mesher worker, between the fill and `build_mesh`, on the
/// pooled neighbourhood -- so it is allowed to destroy what it is given.
///
/// **The padding ring is deliberately untouched.** See the module note:
/// it is what makes a seam between two levels hole-free without either
/// chunk knowing what the other was meshed at.
pub fn coarsen(cache: &mut Neighbourhood, level: u8, quality: Quality) {
    coarsen_cells(cache, cell_size(level), keeps_sprites(level, quality), quality.flat_light());
}

/// `coarsen` with the cell given outright rather than looked up by level.
///
/// Split out so a measurement can put any cell over the same chunk --
/// "would 4x4 pay on a mountain" is a question about this loop, and
/// answering it by editing `CELL` and rebuilding is how a number stops
/// being re-measured.
pub(crate) fn coarsen_cells(
    cache: &mut Neighbourhood,
    (horizontal, vertical): (usize, usize),
    sprites: bool,
    flat_light: bool,
) {
    if horizontal <= 1 && vertical <= 1 {
        return;
    }
    // Lit flat from here on -- unless the player asked for the picture.
    // Not a side effect of coarsening but half of it: see `flat_lit` in
    // `build_mesh` for the measurement that says the blocks alone are
    // worth 7% where the pair is worth 48%.
    if flat_light {
        cache.mark_coarse();
    }
    // Above the chunk's own skyline there is nothing to coarsen, and --
    // more to the point -- nothing that has been *filled*: `fill` copies
    // planes up to the ceiling and one more, and leaves the rest holding
    // whatever the last chunk to borrow this buffer put there.
    let ceiling = cache.ceiling().clamp(0, CHUNK_SIZE_Y as i32) as usize;

    // Tallies for one coarse cell: a handful of entries at the sizes in
    // `CELL`, so a linear scan of a tiny vector beats a hash map by a
    // wide margin -- and it is cleared rather than rebuilt, so a chunk
    // costs no allocation at all.
    let mut tally: Vec<(BlockId, u32)> = Vec::with_capacity(16);

    let mut y0 = 0;
    while y0 < ceiling {
        let y1 = (y0 + vertical).min(ceiling);
        for z0 in (0..CHUNK_SIZE_Z).step_by(horizontal) {
            let z1 = (z0 + horizontal).min(CHUNK_SIZE_Z);
            for x0 in (0..CHUNK_SIZE_X).step_by(horizontal) {
                let x1 = (x0 + horizontal).min(CHUNK_SIZE_X);

                tally.clear();
                // Light is taken at its **brightest**, per channel. The
                // cell that decides how a coarse face is lit is the open
                // one in front of it; averaging in the dark rock behind
                // would put a shadow on daylit ground, and -- because
                // the merge only joins faces that agree exactly -- a
                // gradient across a coarse cell is also a merge that
                // does not happen, which is the whole point of this.
                let (mut sky, mut glow) = (0u8, 0u8);
                for y in y0..y1 {
                    for z in z0..z1 {
                        for x in x0..x1 {
                            let (id, light) = cache.own_cell(x, y, z);
                            sky = sky.max(light & 0x0F);
                            glow = glow.max(light >> 4);
                            // Wood does not vote: a trunk alone in its
                            // layer would win the cell and grow into a
                            // post. See `carried_whole`.
                            if !is_opaque(id) || carried_whole(id) {
                                continue;
                            }
                            match tally.iter_mut().find(|(seen, _)| *seen == id) {
                                Some((_, count)) => *count += 1,
                                None => tally.push((id, 1)),
                            }
                        }
                    }
                }
                let light = sky | (glow << 4);
                // The commonest material. `max_by_key` keeps the last
                // of equal maxima, and the scan order is fixed, so a
                // tie is broken the same way every time the chunk is
                // remeshed -- which is what stops a distant hillside
                // changing colour whenever something near it is
                // dirtied.
                let fill = tally
                    .iter()
                    .max_by_key(|(_, count)| *count)
                    .map(|(id, _)| *id);

                for y in y0..y1 {
                    for z in z0..z1 {
                        for x in x0..x1 {
                            let (id, _) = cache.own_cell(x, y, z);
                            // Tufts and loose stones first, because a
                            // tuft is a cutout too and the test below
                            // would keep it for the wrong reason.
                            let replacement = if is_cross(id) || is_flat(id) {
                                // A tuft, a flower, a loose stone.
                                if sprites {
                                    id
                                } else if primitive_shared::types::stands_in_water(id) {
                                    // ...and a stem of kelp, which is the
                                    // sea as well as a plant
                                    // (`types::BLOCK_KELP`). Dropped to air
                                    // it was a dry shaft down through the
                                    // distant sea, with the surface drawn
                                    // round the inside of it.
                                    primitive_shared::types::BLOCK_WATER
                                } else {
                                    BLOCK_AIR
                                }
                            } else if carried_whole(id) {
                                // A trunk, a limb: as thick as it is.
                                id
                            } else if is_liquid(id) || is_cutout(id) {
                                // **Water and leaves are carried
                                // through untouched**, at full
                                // resolution, and this is the "coarsen
                                // only the solid pass" half of the
                                // decision. A sea whose surface moved a
                                // block would show a step across the
                                // whole horizon; a canopy flattened into
                                // cubes emits *more* faces than it
                                // saves, because leaves against
                                // identical leaves are drawn once each
                                // rather than culled (see
                                // `mesh::face_visible`), so a solid blob
                                // of them is a blob with its insides
                                // drawn.
                                id
                            } else {
                                // Everything else becomes the coarse
                                // cell's material -- including the air
                                // inside it, which is what makes the
                                // drawn volume a superset of the real
                                // one and the seam hole-free.
                                //
                                // With nothing opaque in the cell that
                                // means air: a lone campfire or chest
                                // three hundred blocks out is a couple
                                // of pixels and a dozen quads.
                                fill.unwrap_or(BLOCK_AIR)
                            };
                            cache.set_own_cell(x, y, z, replacement, light);
                        }
                    }
                }
            }
        }
        y0 += vertical;
    }
}

#[cfg(test)]
mod tests {

    use super::*;
    use primitive_shared::types::{BLOCK_GRASS, BLOCK_STONE, BLOCK_WATER};

    #[test]
    fn each_step_of_the_quality_setting_gives_up_more_than_the_one_above_it() {
        // **The three steps have to be three different bargains**, or
        // the row is a decoration. What each gives up is written out in
        // `Quality`; this is the statement that they differ, in the
        // direction the words promise.
        //
        // The gentle one keeps the light, which is where the saving
        // actually comes from (see `Neighbourhood::mark_coarse`) -- so
        // it is the step that costs frames and buys the picture back.
        assert!(!Quality::Fine.flat_light());
        assert!(Quality::Normal.flat_light());
        assert!(Quality::Coarse.flat_light());

        // ...and the grass goes further out with every step down.
        assert!(keeps_sprites(1, Quality::Fine));
        assert!(keeps_sprites(2, Quality::Fine));
        assert!(keeps_sprites(1, Quality::Normal));
        assert!(!keeps_sprites(2, Quality::Normal));
        assert!(!keeps_sprites(1, Quality::Coarse));
        assert!(!keeps_sprites(2, Quality::Coarse));

        // Full detail is full detail whatever the quality says: the
        // setting is about coarse chunks, and a chunk at level zero is
        // not one.
        for quality in Quality::ALL {
            assert!(keeps_sprites(0, quality), "{} dropped the grass under the player's feet", quality.name());
        }
    }

    #[test]
    fn the_gentle_step_leaves_a_coarse_chunk_its_own_lighting() {
        // The one thing `Quality::Fine` is *for*, checked where it
        // happens rather than through the frame: the blocks are still
        // merged, and the light is still the light the world computed.
        // A chunk that came back flat-lit here would be the setting
        // doing nothing at all, which is exactly the failure a row
        // three words wide invites.
        let meadow = |x: usize, y: usize, z: usize| {
            let _ = (x, z);
            if y < 20 { BLOCK_STONE } else { BLOCK_AIR }
        };
        let mut fine = box_of(meadow);
        coarsen(&mut fine, 1, Quality::Fine);
        assert!(!fine.is_coarse(), "the gentle step flattened the light");

        let mut normal = box_of(meadow);
        coarsen(&mut normal, 1, Quality::Normal);
        assert!(normal.is_coarse(), "the normal step kept the real light");
    }

    /// A neighbourhood whose own cells are set by a closure, with the
    /// ceiling worked out the way `fill` would.
    fn box_of(mut cell: impl FnMut(usize, usize, usize) -> BlockId) -> Neighbourhood {
        let mut cache = Neighbourhood::default();
        for y in 0..CHUNK_SIZE_Y {
            for z in 0..CHUNK_SIZE_Z {
                for x in 0..CHUNK_SIZE_X {
                    cache.set_own_cell(x, y, z, cell(x, y, z), 0x0F);
                }
            }
        }
        cache.recompute_ceiling();
        cache
    }

    #[test]
    fn a_coarse_cell_is_solid_wherever_any_of_its_blocks_was() {
        // **The rule the seam rests on.** If a coarse cell could be
        // emptier than the world it stands for, the chunk next door --
        // which culls its own faces against the *real* blocks here --
        // would hide a face with nothing behind it, and the player would
        // see through the world along the boundary.
        let mut cache = box_of(|x, y, z| {
            // One solid block in each 2x2 column of the floor.
            if y == 0 && x % 2 == 0 && z % 2 == 0 {
                BLOCK_STONE
            } else {
                BLOCK_AIR
            }
        });
        coarsen(&mut cache, 1, Quality::Normal);
        for z in 0..CHUNK_SIZE_Z {
            for x in 0..CHUNK_SIZE_X {
                assert_eq!(
                    cache.own_cell(x, 0, z).0,
                    BLOCK_STONE,
                    "({x}, 0, {z}) emptied out where the world had stone in its cell"
                );
            }
        }
    }

    #[test]
    fn coarsening_never_grows_a_trunk_into_the_air_beside_it() {
        // **The bare brown posts on the mountain.** A log counted toward
        // its cell's material like rock, so a trunk alone in its layer
        // came out a 2x2 column, and the air pockets beside a trunk inside
        // a crown became bark standing out of the leaves -- ninety blocks
        // from a player once a mountain coarsened at half the setting.
        // See `carried_whole`.
        //
        // Stated as the property rather than the picture: after
        // coarsening, at every level, the wood and the leaves are exactly
        // where they were, and no other cell has become wood. A slope runs
        // under the trees so that some trunk cells share their layer with
        // rock, which is the case where the trunk used to turn into stone.
        use primitive_shared::types::{block_name, BLOCK_BIRCH_LOG, BLOCK_LEAVES, BLOCK_LOG};
        let world = |x: usize, y: usize, z: usize| {
            let ground = 10 + (x + z) / 3;
            if y <= ground {
                BLOCK_STONE
            } else if (x, z) == (5, 5) && y <= 22 {
                BLOCK_LOG
            } else if (x, z) == (10, 9) && y <= 26 {
                BLOCK_BIRCH_LOG
            } else if (18..=22).contains(&y) && x.abs_diff(5) <= 2 && z.abs_diff(5) <= 2 && !(x + y + z).is_multiple_of(3) {
                // A crown with pockets of air in it, as a tapering one has.
                BLOCK_LEAVES
            } else {
                BLOCK_AIR
            }
        };
        let is_wood = |id: BlockId| matches!(id, BLOCK_LOG | BLOCK_BIRCH_LOG);
        // `block_name` has no row for air, and "which was ?" says nothing.
        let block_name = |id: BlockId| if id == BLOCK_AIR { "air" } else { block_name(id) };
        for level in 1..=MAX_LEVEL {
            let before = box_of(world);
            let mut after = box_of(world);
            coarsen(&mut after, level, Quality::Normal);
            let mut grown_rock = 0;
            for y in 0..CHUNK_SIZE_Y {
                for z in 0..CHUNK_SIZE_Z {
                    for x in 0..CHUNK_SIZE_X {
                        let (was, now) = (before.own_cell(x, y, z).0, after.own_cell(x, y, z).0);
                        if is_wood(was) || was == BLOCK_LEAVES {
                            assert_eq!(
                                now,
                                was,
                                "level {level} turned the {} at ({x}, {y}, {z}) into {}",
                                block_name(was),
                                block_name(now)
                            );
                        }
                        assert!(
                            !is_wood(now) || now == was,
                            "level {level} grew {} into ({x}, {y}, {z}), which was {}",
                            block_name(now),
                            block_name(was)
                        );
                        grown_rock += usize::from(was == BLOCK_AIR && now == BLOCK_STONE);
                    }
                }
            }
            assert!(
                grown_rock > 0,
                "level {level}: the slope did not coarsen at all, so the fixture tests nothing"
            );
        }
    }

    #[test]
    fn coarsening_never_removes_solid_matter() {
        // The general form of the rule above, over a whole hillside: for
        // every cell that was opaque, the coarse world is opaque there
        // too. Stated as an inclusion rather than as a shape, because
        // that inclusion is exactly the seam argument.
        let height = |x: usize, z: usize| 8 + (x * 7 + z * 3) % 11;
        let before = box_of(|x, y, z| {
            if y <= height(x, z) {
                BLOCK_STONE
            } else {
                BLOCK_AIR
            }
        });
        for level in 1..=MAX_LEVEL {
            let mut after = box_of(|x, y, z| {
                if y <= height(x, z) {
                    BLOCK_STONE
                } else {
                    BLOCK_AIR
                }
            });
            coarsen(&mut after, level, Quality::Normal);
            for y in 0..CHUNK_SIZE_Y {
                for z in 0..CHUNK_SIZE_Z {
                    for x in 0..CHUNK_SIZE_X {
                        if is_opaque(before.own_cell(x, y, z).0) {
                            assert!(
                                is_opaque(after.own_cell(x, y, z).0),
                                "level {level} lost the solid block at ({x}, {y}, {z})"
                            );
                        }
                    }
                }
            }
        }
    }

    #[test]
    fn the_skyline_of_a_coarse_chunk_is_the_highest_of_the_columns_it_covers() {
        // The vertical factor is 1, so the coarse ground stands at a
        // real height and not at a rounded one -- which is why flat
        // country shows no terrace at the boundary at all. What it
        // *does* do is take the tallest of the columns it merges.
        let mut cache = box_of(|x, _y, _z| if x == 0 { BLOCK_STONE } else { BLOCK_AIR });
        // Only x == 0 is solid, so column 1 is empty and the pair must
        // come out solid together.
        coarsen(&mut cache, 1, Quality::Normal);
        assert_eq!(cache.own_cell(1, 0, 0).0, BLOCK_STONE);
        assert_eq!(cache.own_cell(1, 30, 0).0, BLOCK_STONE);
        assert_eq!(
            cache.own_cell(2, 0, 0).0,
            BLOCK_AIR,
            "the next coarse cell along had nothing solid in it and must stay empty"
        );
    }

    #[test]
    fn water_keeps_its_own_level_through_every_band() {
        // A sea whose surface rose a block at the boundary would show a
        // step across the whole horizon, which is the one artefact a
        // shoreline cannot hide.
        let sea = 20;
        for level in 1..=MAX_LEVEL {
            let mut cache = box_of(|_x, y, _z| {
                if y < 4 {
                    BLOCK_STONE
                } else if y <= sea {
                    BLOCK_WATER
                } else {
                    BLOCK_AIR
                }
            });
            coarsen(&mut cache, level, Quality::Normal);
            for z in 0..CHUNK_SIZE_Z {
                for x in 0..CHUNK_SIZE_X {
                    assert_eq!(
                        cache.own_cell(x, sea, z).0,
                        BLOCK_WATER,
                        "level {level} moved the surface at ({x}, {z})"
                    );
                    assert_eq!(
                        cache.own_cell(x, sea + 1, z).0,
                        BLOCK_AIR,
                        "level {level} raised the sea at ({x}, {z})"
                    );
                }
            }
        }
    }

    #[test]
    fn a_coarse_cell_takes_the_material_most_of_it_was_made_of() {
        // Grass over dirt: the surface has to stay grass, or a distant
        // meadow turns the colour of what is under it.
        let mut cache = box_of(|x, y, _z| {
            if y > 0 {
                BLOCK_AIR
            } else if x < 3 {
                BLOCK_GRASS
            } else {
                BLOCK_STONE
            }
        });
        coarsen(&mut cache, 1, Quality::Normal);
        assert_eq!(cache.own_cell(0, 0, 0).0, BLOCK_GRASS);
        assert_eq!(cache.own_cell(1, 0, 0).0, BLOCK_GRASS);
        // x = 2 is grass and x = 3 is stone, one each: the tie goes to
        // whichever the scan met first, which is a fixed order, so the
        // chunk looks the same after every remesh.
        let tied = cache.own_cell(2, 0, 0).0;
        assert_eq!(tied, cache.own_cell(3, 0, 0).0);
    }

    /// Terrain jagged enough that every column differs from its
    /// neighbours: the worst case for a boundary between two levels,
    /// and the only kind of terrain where a hole would show.
    fn seam_height(gx: i32, gz: i32) -> i32 {
        8 + (gx * 5 + gz * 11).rem_euclid(13)
    }

    /// Nine chunks of it, addressable the way the mesher addresses the
    /// world.
    struct SeamWorld {
        chunks: std::collections::HashMap<primitive_shared::types::ChunkPos, Vec<BlockId>>,
    }

    impl SeamWorld {
        fn new() -> Self {
            use primitive_shared::types::{Chunk, CHUNK_VOLUME};
            let mut chunks = std::collections::HashMap::new();
            for cx in -1..=2 {
                for cz in -1..=1 {
                    let mut blocks = vec![BLOCK_AIR; CHUNK_VOLUME];
                    for lz in 0..CHUNK_SIZE_Z {
                        for lx in 0..CHUNK_SIZE_X {
                            let gx = cx * CHUNK_SIZE_X as i32 + lx as i32;
                            let gz = cz * CHUNK_SIZE_Z as i32 + lz as i32;
                            let top = seam_height(gx, gz) as usize;
                            for y in 0..=top {
                                blocks[Chunk::index(lx, y, lz)] = if y == top {
                                    BLOCK_GRASS
                                } else {
                                    BLOCK_STONE
                                };
                            }
                        }
                    }
                    chunks.insert(primitive_shared::types::ChunkPos::new(cx, cz), blocks);
                }
            }
            Self { chunks }
        }
    }

    impl primitive_shared::lighting::BlockSource for SeamWorld {
        fn block_at(&self, gx: i32, gy: i32, gz: i32) -> Option<BlockId> {
            use primitive_shared::types::Chunk;
            if gy < 0 || gy >= CHUNK_SIZE_Y as i32 {
                return Some(BLOCK_AIR);
            }
            let pos = primitive_shared::types::ChunkPos::new(
                gx.div_euclid(CHUNK_SIZE_X as i32),
                gz.div_euclid(CHUNK_SIZE_Z as i32),
            );
            let lx = gx.rem_euclid(CHUNK_SIZE_X as i32) as usize;
            let lz = gz.rem_euclid(CHUNK_SIZE_Z as i32) as usize;
            self.chunks
                .get(&pos)
                .map(|blocks| blocks[Chunk::index(lx, gy as usize, lz)])
        }

        fn chunk_data(&self, pos: primitive_shared::types::ChunkPos) -> Option<&[BlockId]> {
            self.chunks.get(&pos).map(|blocks| blocks.as_slice())
        }
    }

    /// Which cells of one plane of a chunk's mesh have anything drawn
    /// over them, as a `[y][z]` grid.
    ///
    /// Rasterised from the triangles rather than counted as quads,
    /// because the greedy merge means a quad can be any size and the
    /// T-junction pass can give it more than four corners. What the
    /// question is really asking is "is this square of the seam covered
    /// by anything at all", and that is a coverage test.
    fn plane_coverage(
        buffers: &crate::engine::mesh::MeshBuffers,
        face: usize,
        plane: f32,
    ) -> Vec<Vec<bool>> {
        let mut covered = vec![vec![false; CHUNK_SIZE_Z]; CHUNK_SIZE_Y];
        let range = buffers.solid_groups[face] as usize..buffers.solid_groups[face + 1] as usize;
        for triangle in buffers.indices[range].chunks_exact(3) {
            let corners: Vec<[f32; 3]> = triangle
                .iter()
                .map(|i| buffers.vertices[*i as usize].position)
                .collect();
            if corners.iter().any(|c| (c[0] - plane).abs() > 1e-4) {
                continue;
            }
            let flat: Vec<(f32, f32)> = corners.iter().map(|c| (c[1], c[2])).collect();
            let (mut y0, mut y1) = (f32::MAX, f32::MIN);
            let (mut z0, mut z1) = (f32::MAX, f32::MIN);
            for (y, z) in &flat {
                y0 = y0.min(*y);
                y1 = y1.max(*y);
                z0 = z0.min(*z);
                z1 = z1.max(*z);
            }
            let rows = covered
                .iter_mut()
                .enumerate()
                .take((y1.ceil() as usize).min(CHUNK_SIZE_Y))
                .skip(y0.floor().max(0.0) as usize);
            for (y, row) in rows {
                let cells = row
                    .iter_mut()
                    .enumerate()
                    .take((z1.ceil() as usize).min(CHUNK_SIZE_Z))
                    .skip(z0.floor().max(0.0) as usize);
                for (z, cell) in cells {
                    let point = (y as f32 + 0.5, z as f32 + 0.5);
                    // Sign of the cross product against each edge, with
                    // a point on an edge counting as inside: a rectangle
                    // is two triangles and its diagonal runs straight
                    // through the centres of its cells.
                    let side = |a: (f32, f32), b: (f32, f32)| {
                        (point.0 - b.0) * (a.1 - b.1) - (a.0 - b.0) * (point.1 - b.1)
                    };
                    let d = [
                        side(flat[0], flat[1]),
                        side(flat[1], flat[2]),
                        side(flat[2], flat[0]),
                    ];
                    let outside = d.iter().any(|v| *v < -1e-4) && d.iter().any(|v| *v > 1e-4);
                    if !outside {
                        *cell = true;
                    }
                }
            }
        }
        covered
    }

    /// What a band of the world actually costs, level by level.
    ///
    /// ```text
    /// cargo test -p primitive_client --lib what_a_coarse_chunk_costs \
    ///     -- --ignored --nocapture
    /// ```
    ///
    /// A measurement, not an assertion: the number that decides whether
    /// this feature is worth its complexity is how many triangles a
    /// chunk of *real* terrain loses, and that is a property of the
    /// world generator rather than of anything here.
    #[test]
    #[ignore = "a measurement, not an assertion -- run it explicitly"]
    fn what_a_coarse_chunk_costs() {
        use crate::engine::mesh::{build_mesh, MeshBuffers, Neighbourhood};
        use primitive_shared::lighting::LightMap;
        use primitive_shared::types::{Chunk, ChunkPos};

        struct Generated {
            chunks: std::collections::HashMap<ChunkPos, Chunk>,
        }
        impl primitive_shared::lighting::BlockSource for Generated {
            fn block_at(&self, gx: i32, gy: i32, gz: i32) -> Option<BlockId> {
                if gy < 0 || gy >= CHUNK_SIZE_Y as i32 {
                    return Some(BLOCK_AIR);
                }
                let pos = ChunkPos::new(
                    gx.div_euclid(CHUNK_SIZE_X as i32),
                    gz.div_euclid(CHUNK_SIZE_Z as i32),
                );
                self.chunks.get(&pos).map(|chunk| {
                    chunk.get(
                        gx.rem_euclid(CHUNK_SIZE_X as i32) as usize,
                        gy as usize,
                        gz.rem_euclid(CHUNK_SIZE_Z as i32) as usize,
                    )
                })
            }
            fn chunk_data(&self, pos: ChunkPos) -> Option<&[BlockId]> {
                self.chunks.get(&pos).map(|chunk| chunk.blocks.as_slice())
            }
        }

        // The world the benchmark is run in.
        let generator = primitive_shared::worldgen::WorldGen::new(4242);
        // Centred where the benchmark stands in the world `far`: the
        // shore at (-15.5, 24, 16.5), which is chunk (-1, 1). A patch
        // round the origin would have been three quarters open sea, and
        // the sea is the one thing this feature deliberately does not
        // touch.
        let centre = ChunkPos::new(-1, 1);
        let radius = 10;
        let mut world = Generated {
            chunks: std::collections::HashMap::new(),
        };
        for cx in centre.x - radius..=centre.x + radius {
            for cz in centre.z - radius..=centre.z + radius {
                let pos = ChunkPos::new(cx, cz);
                world.chunks.insert(pos, generator.generate_chunk(pos));
            }
        }
        let mut light = LightMap::new();
        for cx in centre.x - radius..=centre.x + radius {
            for cz in centre.z - radius..=centre.z + radius {
                light.load_chunk(&world, ChunkPos::new(cx, cz));
            }
        }

        // How much variety the merge has to swallow, before any of it
        // is meshed: distinct tint bytes and distinct surface heights
        // over one chunk of meadow.
        {
            use std::collections::HashSet;
            let mut tints: HashSet<u32> = HashSet::new();
            let mut heights: HashSet<i32> = HashSet::new();
            let mut surfaces = 0;
            let chunk = &world.chunks[&centre];
            for lz in 0..CHUNK_SIZE_Z {
                for lx in 0..CHUNK_SIZE_X {
                    for y in (0..CHUNK_SIZE_Y).rev() {
                        let id = chunk.get(lx, y, lz);
                        if id == BLOCK_AIR {
                            continue;
                        }
                        surfaces += 1;
                        heights.insert(y as i32);
                        if primitive_shared::types::is_foliage(id) {
                            let (t, h) = generator.climate_column(
                                centre.x * CHUNK_SIZE_X as i32 + lx as i32,
                                centre.z * CHUNK_SIZE_Z as i32 + lz as i32,
                            );
                            tints.insert(crate::engine::mesh::pack_tint(
                                primitive_shared::worldgen::cooled_by_altitude(t, y as i32),
                                h,
                            ));
                        }
                        break;
                    }
                }
            }
            println!(
                "one chunk: {surfaces} surface columns, {} distinct heights, {} distinct tints",
                heights.len(),
                tints.len()
            );
        }

        let layers = crate::engine::texture::FaceLayers::empty_for_test();
        println!();
        println!(
            "level  solid tris  leaf tris  sprite tris  water tris  vertices               [+Y -Y +X -X +Z -Z]"
        );
        for level in 0..=MAX_LEVEL {
            let (mut solid, mut leaf, mut sprite, mut water, mut vertices) = (0, 0, 0, 0, 0);
            let mut by_face = [0u32; 6];
            for cx in centre.x - radius + 1..=centre.x + radius - 1 {
                for cz in centre.z - radius + 1..=centre.z + radius - 1 {
                    let pos = ChunkPos::new(cx, cz);
                    let mut cache = Neighbourhood::default();
                    cache.fill(pos, &world, &light);
                    coarsen(&mut cache, level, Quality::Normal);
                    let mut out = MeshBuffers::default();
                    build_mesh(pos, &cache, &layers, &generator, &mut out);
                    solid += out.solid_index_count / 3;
                    leaf += (out.leaf_end - out.solid_index_count) / 3;
                    sprite += (out.sprite_end - out.leaf_end) / 3;
                    water += (out.indices.len() as u32 - out.sprite_end) / 3;
                    vertices += out.vertices.len() as u32;
                    for (face, total) in by_face.iter_mut().enumerate() {
                        *total += (out.solid_groups[face + 1] - out.solid_groups[face]) / 3;
                    }
                }
            }
            println!(
                "{level:5}  {solid:10}  {leaf:9}  {sprite:11}  {water:10}  {vertices:8}  {by_face:?}"
            );
        }
    }

    #[test]
    fn a_coarse_chunk_beside_a_fine_one_leaves_no_hole_in_the_seam() {
        // **The artefact this whole design is arranged around.** The
        // chunk at full detail culls its faces against the *real* blocks
        // next door; the coarse chunk draws a blockier version of them.
        // If the coarse version could ever hold less matter than the
        // real one, the fine chunk would hide a face with nothing behind
        // it and the player would see through the ground along a chunk
        // boundary -- from ten chunks away, as a moving line.
        //
        // So: mesh the pair, and check that every square of the plane
        // between them where one side is solid and the other is not has
        // something drawn over it, by one of the two.
        use crate::engine::mesh::{build_mesh, MeshBuffers, Neighbourhood};
        use primitive_shared::lighting::LightMap;
        use primitive_shared::types::{Chunk, ChunkPos};

        let world = SeamWorld::new();
        let mut light = LightMap::new();
        for cx in -1..=2 {
            for cz in -1..=1 {
                light.load_chunk(&world, ChunkPos::new(cx, cz));
            }
        }
        let layers = crate::engine::texture::FaceLayers::empty_for_test();
        let generator = primitive_shared::worldgen::WorldGen::new(0);

        for level in 1..=MAX_LEVEL {
            let fine_pos = ChunkPos::new(0, 0);
            let mut fine_cache = Neighbourhood::default();
            fine_cache.fill(fine_pos, &world, &light);
            let mut fine = MeshBuffers::default();
            build_mesh(fine_pos, &fine_cache, &layers, &generator, &mut fine);

            let coarse_pos = ChunkPos::new(1, 0);
            let mut coarse_cache = Neighbourhood::default();
            coarse_cache.fill(coarse_pos, &world, &light);
            coarsen(&mut coarse_cache, level, Quality::Normal);
            let mut coarse = MeshBuffers::default();
            build_mesh(coarse_pos, &coarse_cache, &layers, &generator, &mut coarse);

            // +X faces of the fine chunk sit on its own x = 16;
            // -X faces of the coarse chunk sit on its x = 0. The same
            // plane of the world. See `mesh::faces()` for the order.
            let from_fine = plane_coverage(&fine, 2, CHUNK_SIZE_X as f32);
            let from_coarse = plane_coverage(&coarse, 3, 0.0);

            let mut mismatches = 0;
            for y in 0..CHUNK_SIZE_Y {
                for z in 0..CHUNK_SIZE_Z {
                    let near =
                        is_opaque(world.chunks[&fine_pos][Chunk::index(CHUNK_SIZE_X - 1, y, z)]);
                    let far = is_opaque(coarse_cache.own_cell(0, y, z).0);
                    if near == far {
                        continue; // both solid, or both open: nothing to close
                    }
                    mismatches += 1;
                    assert!(
                        from_fine[y][z] || from_coarse[y][z],
                        "level {level}: nothing drawn over ({y}, {z}) of the seam, \
                         where one side is solid and the other is not"
                    );
                }
            }
            assert!(
                mismatches > 20,
                "level {level}: only {mismatches} squares of the seam disagreed -- \
                 the fixture is too smooth to be testing anything"
            );
        }
    }

    #[test]
    fn a_chunk_on_the_threshold_does_not_flicker_between_two_levels() {
        // **The dithering this exists to stop.** The distance is a float
        // that wanders either side of the line as the player breathes,
        // and every crossing is a remesh and an upload.
        let lod = 10;
        // Standing exactly on the line, a fine chunk stays fine...
        assert_eq!(level_at(10.0, lod, 0), 0);
        // ...and a coarse one stays coarse.
        assert_eq!(level_at(10.0, lod, 1), 1);
        // It takes a whole chunk past the line either way.
        assert_eq!(level_at(11.0, lod, 0), 1);
        assert_eq!(level_at(8.9, lod, 1), 0);
    }

    #[test]
    fn a_stone_keeps_its_thickness_near_and_does_not_flicker_on_the_line() {
        let line = RELIEF_CHUNKS;
        let thick = |distance: f32, current| stones_at(distance, line, current) != StoneDetail::Flat;
        // Near: solid, whatever it was.
        assert!(thick(0.0, StoneDetail::Flat) && thick(0.0, StoneDetail::Full));
        // On the line it stays what it was built as...
        assert!(thick(line as f32, StoneDetail::Full));
        assert!(!thick(line as f32, StoneDetail::Flat));
        // ...and it takes a chunk past the line either way to change.
        assert!(!thick(line as f32 + HYSTERESIS, StoneDetail::Full));
        assert!(thick(line as f32 - HYSTERESIS - 0.1, StoneDetail::Flat));
        // Far: flat.
        assert!(!thick(24.0, StoneDetail::Full) && !thick(24.0, StoneDetail::Flat));
    }

    #[test]
    fn a_relief_distance_of_zero_lays_every_stone_flat() {
        // Zero is the setting's "off": the flat quads the game drew before
        // stones had a thickness, under the player's own feet included.
        for distance in [0.0, 1.0, 3.0, 50.0] {
            assert_eq!(
                stones_at(distance, 0, StoneDetail::Full),
                StoneDetail::Flat,
                "a stone {distance} chunks off kept its thickness at zero"
            );
            assert_eq!(stones_at(distance, 0, StoneDetail::Flat), StoneDetail::Flat);
        }
    }

    #[test]
    fn the_see_through_canopy_line_does_not_flicker_as_the_player_walks_along_it() {
        // **The remesh this exists to stop.** A chunk's distance is a float
        // that wanders either side of the line with every step, and each
        // flip is a rebuild and an upload of a chunk of forest. Walk a chunk
        // back and forth across the line in tenths and count the flips:
        // once out and once back per trip, never more.
        let line = 6;
        let mut built = true;
        let mut flips = 0;
        let mut walk: Vec<f32> = Vec::new();
        for _ in 0..3 {
            walk.extend((0..=40).map(|i| 4.5 + i as f32 * 0.1)); // out past 7
            walk.extend((0..=40).map(|i| 8.5 - i as f32 * 0.1)); // back inside 5
        }
        for distance in walk {
            let now = leaves_see_through_at(distance, line, built);
            flips += usize::from(now != built);
            built = now;
        }
        assert_eq!(flips, 6, "three trips across the line flipped the canopy {flips} times");
        // Wandering a chunk either side of the line flips nothing.
        for distance in [5.1, 6.0, 6.9, 6.0, 5.1] {
            assert!(leaves_see_through_at(distance, line, true));
            assert!(!leaves_see_through_at(distance, line, false));
        }
    }

    #[test]
    fn a_stone_keeps_both_tiers_to_the_default_line_and_one_past_it() {
        // The player's row at its furthest stop: full detail to the default
        // line, the silhouette alone from there to eight chunks, the flat
        // quad past it.
        let line = 8;
        assert_eq!(stones_at(0.5, line, StoneDetail::Full), StoneDetail::Full);
        assert_eq!(stones_at(RELIEF_CHUNKS as f32 - 1.0, line, StoneDetail::Full), StoneDetail::Full);
        assert_eq!(stones_at(RELIEF_CHUNKS as f32 + 1.0, line, StoneDetail::Full), StoneDetail::Slab);
        assert_eq!(stones_at(line as f32 + 1.0, line, StoneDetail::Slab), StoneDetail::Flat);
    }

    #[test]
    fn a_line_at_or_inside_the_default_has_no_band_of_slabs_in_it() {
        // A player who moved the line *in* asked for less thickness, not for
        // a cheaper kind of it: every chunk that has any has both tiers.
        for line in 1..=RELIEF_CHUNKS {
            for distance in [0.0, 0.5, 1.5, 2.5, 3.5, 7.5] {
                assert_ne!(
                    stones_at(distance, line, StoneDetail::Full),
                    StoneDetail::Slab,
                    "a line at {line} chunks grew a slab band at {distance}"
                );
            }
        }
    }

    #[test]
    fn neither_stone_line_flickers_for_a_player_walking_across_it() {
        // Both lines carry `inside_line`'s hysteresis, so a chunk standing
        // exactly on one keeps what it was built with.
        let line = 8;
        assert_eq!(stones_at(line as f32, line, StoneDetail::Slab), StoneDetail::Slab);
        assert_eq!(stones_at(line as f32, line, StoneDetail::Flat), StoneDetail::Flat);
        let full = RELIEF_CHUNKS as f32;
        assert_eq!(stones_at(full, line, StoneDetail::Full), StoneDetail::Full);
        assert_eq!(stones_at(full, line, StoneDetail::Slab), StoneDetail::Slab);
    }

    #[test]
    fn the_stones_go_flat_at_zero_whatever_they_were() {
        for current in [StoneDetail::Flat, StoneDetail::Slab, StoneDetail::Full] {
            assert_eq!(stones_at(0.0, 0, current), StoneDetail::Flat);
        }
    }

    #[test]
    fn the_two_ends_of_the_leaf_distance_mean_everywhere_and_nowhere() {
        for distance in [0.0, 1.0, 9.0, 40.0, 900.0] {
            for built in [false, true] {
                assert!(leaves_see_through_at(distance, LEAVES_SEE_THROUGH_EVERYWHERE, built));
                assert!(!leaves_see_through_at(distance, 0, built));
            }
        }
    }

    #[test]
    fn turning_the_setting_off_leaves_every_chunk_at_full_detail() {
        for distance in [0.0, 5.0, 50.0, 500.0] {
            assert_eq!(level_at(distance, 0, 0), 0);
            // Even a chunk that is currently coarse comes back.
            assert_eq!(level_at(distance, 0, 2), 0);
        }
    }

    #[test]
    fn the_bands_start_where_the_setting_says_and_stack_outward() {
        let lod = 8;
        assert_eq!(level_at(0.0, lod, 0), 0);
        assert_eq!(level_at(9.0, lod, 0), 1);
        assert_eq!(level_at(17.0, lod, 0), 2);
        // There is no third band: the coarsest is as coarse as it gets.
        assert_eq!(level_at(400.0, lod, 0), MAX_LEVEL);
    }

    #[test]
    fn a_mountain_leaves_full_detail_nearer_than_a_meadow_does() {
        let lod = 10;
        let (meadow, mountain) = (SEA_LEVEL + 12, SEA_LEVEL + 80);
        assert_eq!(band_start(lod, meadow), lod, "a meadow's bands moved");
        assert!(band_start(lod, mountain) < lod, "a mountain starts where a meadow does");
        // Seven chunks out: the meadow is still fine, the slope is not.
        assert_eq!(level_at(7.0, band_start(lod, meadow), 0), 0);
        assert_eq!(level_at(7.0, band_start(lod, mountain), 0), 1);
    }

    #[test]
    fn no_mountain_goes_coarse_nearer_than_the_setting_could_have_asked_for() {
        // The setting row stops at four chunks, and so does a mountain --
        // unless the player set it nearer than that themselves.
        for lod in 1..=64 {
            let start = band_start(lod, CHUNK_SIZE_Y as i32);
            assert!(start >= lod.min(TALL_FLOOR), "lod {lod}: a mountain starts at {start}");
            assert!(start <= lod, "lod {lod}: a mountain starts further out than a meadow");
        }
        // And off is off, however tall the ground.
        assert_eq!(band_start(0, CHUNK_SIZE_Y as i32), 0);
    }
}
