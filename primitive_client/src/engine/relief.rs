//! **A stone lying on the ground has a thickness**: the shape a pebble,
//! a stick, a flake of flint or a nugget of copper is drawn with in the
//! world.
//!
//! ## What it replaces, and why
//!
//! Every loose thing on the ground used to be one quad a fiftieth of a
//! block over the grass (`mesh::flat_block`), wearing its picture seen
//! from above. From above that is honest. From where a player stands --
//! eye a block and a half up, looking a few blocks ahead -- it was a
//! sticker: a grey shape with no edge, no side in shadow, and no way to
//! tell a stone from a stain. "сделай палки камни и прочее 3д моделями а
//! не просто наложениями". The same picture carried in the hand had been
//! a solid for a long time already (`item_model`), which made the
//! difference obvious the moment one was dropped.
//!
//! ## The shape: the picture's own silhouette, stood up in two steps
//!
//! The picture is read as a height map with two levels:
//!
//! * every opaque texel is a slab one texel tall;
//! * every opaque texel whose four neighbours are opaque too gets a second
//!   texel on top of it.
//!
//! So a round pebble is a low dome -- a rim one texel high and a crown two
//! -- a flake of flint is a ridge, and a stick, which is a stroke two
//! texels wide, is a rod one texel thick. **The texel is the unit on every
//! axis**: a step is exactly as tall as a texel of the picture is wide, so
//! the side of a stone wears its colour at the same density as its top and
//! nothing is stretched (the rule `mesh::FINE_UV_BIT` exists for).
//!
//! Compared, before choosing:
//!
//! * **A hand-made low-poly lump per block** (a box, or two stacked boxes).
//!   The cheapest -- five quads a stone -- and wrong for the pictures this
//!   game has: `pebble.png` is *four* stones, `flint.png` two, `stream_tin`
//!   a nodule and a grain. A box cannot follow any of those, the picture on
//!   its top would be cut off at the box's edge or show transparent texels
//!   as holes in a solid, and every new loose item would need a model
//!   written by hand in Rust. Rejected.
//! * **One slab, the silhouette extruded a texel or two** -- what a dropped
//!   item is (`item_model`). Right silhouette, no authoring. But a slab two
//!   texels thick reads as a coin or a tile, not as a stone: flat top, sheer
//!   sides. It is the first tier of what was chosen.
//! * **The silhouette in two tiers (chosen).** One more tier costs the
//!   eroded core's tops and edges, which on these pictures is a quarter of
//!   the model, and it is the difference between a coin and a lump. A third
//!   tier was tried on paper: at sixteen texels the second erosion of a
//!   five-texel pebble is a single texel, a spike rather than a crown.
//! * **Stacked copies of the flat quad** ("shells", one per height step,
//!   each discarding its own transparent texels) -- two or three quads a
//!   stone, and no sides at all: from the player's eye the steps come apart
//!   into parallel slices with grass between them, and every slice faces up,
//!   so nothing on the stone turns away from the sun. Rejected.
//!
//! ## What it costs, and why nothing about its shape is left to the cut-out
//!
//! **A stone is drawn as its exact surface**: a top over each rectangle of
//! texels of one height, a side along each run of edge, nothing standing
//! anywhere a texel is transparent. Thirty-five to a hundred quads a stone.
//!
//! It shipped cheaper, and the saving was the seam. Two tricks leaned on the
//! cut-out to draw the silhouette instead of the geometry:
//!
//! * *The rim's top was one quad, the whole picture*, discarded where the
//!   picture is transparent -- two triangles where a pebble's rectangles are
//!   a dozen.
//! * *A side was one quad per line of texels*, carried from the first edge
//!   on that line to the last across whatever lay between -- discarded over
//!   transparent texels, hidden inside the stone over opaque ones.
//!
//! Both put an edge of the stone where the discard decides rather than where
//! a triangle ends, beside an edge that is a triangle's, and the two do not
//! agree about the pixel on it. With several samples a pixel the discard is
//! decided once, at the pixel's middle, for all of them: where the middle
//! fell past a top's edge, the samples inside the stone lost their top and
//! showed the ground under it -- a line along every top edge. At one sample,
//! a slit of grass down a flank where a carried side met the next. "на
//! стыках 3д камней, веток и т.д. есть стыки". Taken apart in `relief_repro`'s
//! `where_the_seams_on_a_stone_come_from`, one change at a time: exact tops
//! took the line away and not the slit, exact sides the slit.
//!
//! The bill, triangles a stone, shipped against exact: stick 84 / 116,
//! pebble 82 / 136, flint 114 / 216, flake 86 / 142, copper 40 / 70. Paid
//! only inside the player's relief distance (`ClientSettings::relief_chunks`,
//! four chunks by default); past it a stone is the flat quad either way.
//!
//! ## What it turned out to cost, and the tier the far band gives up
//!
//! **That bill was the largest in the frame and nobody had added it up.**
//! With the player's line at its furthest stop -- eight chunks -- the loose
//! stones were 0.86 ms of a 3.06 ms GPU frame on a GTX 1050 Ti: 48% of the
//! whole cut-out pass, more than the leaves (0.58) and all the grass past
//! forty blocks (0.20) together. The pass is neither fragment- nor fill-bound
//! -- it is triangles, and these are the only things in the world that cost a
//! hundred of them each. `lod::StoneDetail` carries the ablation.
//!
//! So past `lod::RELIEF_CHUNKS` -- four chunks, the default, and the line at
//! which a rim was measured to be under a pixel and a half -- a stone keeps
//! its silhouette and gives up its crown: [`Relief::slab`], one tier, 47%
//! fewer triangles. Under that line nothing changed, so a player on the
//! default settings is looking at the same pixels as before.
//!
//! The breaking cracks were the reason `Relief::exact` existed -- a crack is
//! multiplied onto whatever is behind it and discards nothing -- and now it
//! is also what is drawn, so the two fields hold one surface. They are kept
//! apart because they answer two questions, and the day a cheaper drawn
//! surface is found that does not leak, the cracks must not follow it.

use image::RgbaImage;

use crate::engine::mesh::{cell_hash, pack_light, Vertex};
use primitive_shared::types::BlockId;

/// Texels on a side of the height map, whatever the picture's resolution.
///
/// **Sixteen, and not the atlas resolution**, because the atlas can be
/// anything a resource pack asks for and the geometry grows with the square
/// of it: a 64-texel pebble would be sixteen times the sides for a stone
/// twenty centimetres across. The picture is sampled at the middle of each
/// sixteenth, which on the shipped art is every texel exactly.
pub const GRID: usize = 16;

/// A texel counts as part of the shape at or above this alpha: the
/// shader's cut-out, so what is modelled and what is drawn agree.
const ALPHA_CUTOFF: u8 = 128;

/// How far into its texel a side's picture coordinate starts, in pictures.
///
/// A side stands exactly on the boundary between its own texel and the one
/// beside it, and that one is -- by definition of an edge -- transparent.
/// Sampled on the boundary itself, the filter takes half of each and the
/// cut-out throws away a hairline down the whole side. One fine step
/// (`mesh::FINE_UNITS`), a sixteenth of a texel, is the least the vertex
/// can say.
const INTO_TEXEL: f32 = 1.0 / 256.0;

/// Whether a block lying flat is drawn with a thickness.
///
/// **Coatings are not**: ash and a layer of snow cover their cell edge to
/// edge and *are* the ground there (`types::is_covering_flat`); a slab of
/// them a texel thick would be a step in the floor at every cell. **Nor is a
/// lily pad**, which floats on the water's surface -- a leaf is thinner than
/// a texel, and anything standing up from it would stand out of the water.
pub fn has_relief(block: BlockId) -> bool {
    use primitive_shared::types as t;
    t::is_flat(block) && !t::is_covering_flat(block) && t::block_kind(block) != t::BLOCK_LILY_PAD
}

/// One rectangle of the model, in the model's own space: x and z in texels
/// of the picture (0..16, x along the picture's columns and z down its
/// rows), y in texels up from the ground.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Facet {
    pub corners: [[f32; 3]; 4],
    /// Where each corner is in the picture, in pictures.
    pub uv: [[f32; 2]; 4],
    /// The mesher's face index, 0 = +Y, 2..5 = +X, -X, +Z, -Z. Nothing
    /// faces down: the model lies on the ground.
    pub face: u8,
}

/// The two surfaces of one picture's model.
#[derive(Debug, Clone, Default)]
pub struct Relief {
    /// What the mesher draws: the exact surface. See the module note for the
    /// cheaper one that leaned on the cut-out, and the seam it left.
    pub drawn: Vec<Facet>,
    /// The same surface without relying on it: tops only over opaque
    /// texels, sides only along edges. For the cracks.
    pub exact: Vec<Facet>,
    /// The same silhouette in **one** tier: every opaque texel a slab a
    /// texel tall, no crown and no step up to it. For the chunks past
    /// `lod::RELIEF_CHUNKS` that the player's `relief_chunks` still
    /// reaches. See `Relief::surface`.
    pub slab: Vec<Facet>,
}

/// A height map: 0 empty, 1 the rim, 2 the crown.
type Heights = [[u8; GRID]; GRID];

impl Relief {
    /// Reads a picture's alpha as the height map and builds both surfaces.
    pub fn from_image(image: &RgbaImage) -> Self {
        Self::from_mask(&mask_of(image))
    }

    /// The same from a bare mask, rows first. Split out so the geometry can
    /// be tested on shapes drawn in the test.
    pub fn from_mask(solid: &[[bool; GRID]; GRID]) -> Self {
        let heights = heights_of(solid);
        if !heights.iter().flatten().any(|&h| h > 0) {
            return Self::default();
        }

        let exact = exact_surface(&heights);
        // **The crown is the half of the model the far band gives up.**
        //
        // Flattening every texel to the rim is not a different shape: the
        // silhouette, the inset, the turn and every side's picture are the
        // same, and what goes is the second texel of height and the step up
        // to it. On the shipped pictures that is a little over half the
        // quads -- pebble 68 -> 40, flint 108 -> 60, shell 59 -> 26,
        // 775 -> 413 over the ten loose things, 47% fewer triangles --
        // because a crown pays for its own tops *and* a ring of one-texel
        // sides around them, and the rim's tops merge into far larger
        // rectangles once nothing is cut out of the middle of them.
        //
        // What it costs is a texel of height at four chunks and beyond,
        // where a whole stone is 0.74 of a block: at sixty-four blocks that
        // texel is a quarter of a pixel at 720p. The line it is given up at
        // is the default of the setting itself (`lod::RELIEF_CHUNKS`, four
        // chunks, "a stone's rim is under a pixel and a half at forty
        // blocks") -- so a player on the default sees exactly what they saw,
        // and the two stops past it buy the silhouette further out rather
        // than a thickness nobody can resolve.
        //
        // Measured, world `night` at noon, 1280x720, render distance 13,
        // MSAA 4 on a GTX 1050 Ti, with `relief_chunks = 8`: the stones were
        // 0.86 ms of a 3.06 ms frame -- 48% of the whole cut-out pass, more
        // than the leaves and the grass together. See the note over
        // `lod::StoneDetail` for the ablation that found them.
        let slab = exact_surface(&heights.map(|line| line.map(|h| h.min(1))));
        Self { drawn: exact.clone(), exact, slab }
    }

    /// The surface to draw at this distance's detail. `Flat` has none: the
    /// mesher draws `mesh::flat_block`'s quad instead and never asks.
    pub fn surface(&self, detail: crate::engine::lod::StoneDetail) -> &[Facet] {
        match detail {
            crate::engine::lod::StoneDetail::Full => &self.drawn,
            _ => &self.slab,
        }
    }

    /// This model as it shipped before the seams: the rim topped by the whole
    /// picture, sides carried across a line. Rebuilt from the exact tops, so
    /// it needs no picture. For the photographs that show the seam.
    #[cfg(test)]
    pub fn as_it_shipped(&self) -> Self {
        let mut heights = [[0u8; GRID]; GRID];
        for facet in self.exact.iter().filter(|f| f.face == 0) {
            let (x0, z0) = (facet.corners[0][0] as usize, facet.corners[0][2] as usize);
            let (x1, z1) = (facet.corners[2][0] as usize, facet.corners[2][2] as usize);
            for line in heights.iter_mut().take(z1).skip(z0) {
                for h in line.iter_mut().take(x1).skip(x0) {
                    *h = facet.corners[0][1] as u8;
                }
            }
        }
        if !heights.iter().flatten().any(|&h| h > 0) {
            return self.clone();
        }
        let at = |x: i32, z: i32| height_in(&heights, x, z);
        let mut drawn = vec![top([0, 0, GRID, GRID], 1.0)];
        for rect in rectangles(&heights, |h| h == 2) {
            drawn.push(top(rect, 2.0));
        }
        for tier in [Tier::Rim, Tier::Crown] {
            for side in Side::ALL {
                for line in 0..GRID as i32 {
                    let exposed = |b: i32| {
                        let (x, z) = side.texel(line, b);
                        let (nx, nz) = side.beyond(x, z);
                        at(x, z) >= tier.level() && at(nx, nz) < tier.level()
                    };
                    let blocks = |b: i32| {
                        let (x, z) = side.texel(line, b);
                        tier.blocks_a_span(at(x, z))
                    };
                    for (from, to) in runs(exposed, |b| !blocks(b)) {
                        drawn.push(side.facet(line, from, to, tier));
                    }
                }
            }
        }
        Self { drawn, exact: self.exact.clone(), slab: self.slab.clone() }
    }

    /// How many triangles the mesher spends on one of these.
    #[cfg(test)]
    pub fn triangles(&self) -> usize {
        self.drawn.len() * 2
    }

    /// `facets` placed in a cell: laid over the middle of it the way the
    /// flat quad was, turned by `turn` quarter turns about the vertical
    /// through the cell's middle, and scaled so a texel of height is a
    /// texel of width.
    ///
    /// **The face index turns with the corners.** The shader makes a
    /// normal out of it; left in model space, every stone turned a quarter
    /// would be lit on its east side as if that side faced south -- the
    /// fault that stayed in the animals for a whole version because nothing
    /// disappears, it is only shaded wrong. Checked against the emitted
    /// corners, not against this table: see
    /// `every_side_of_a_stone_points_out_of_the_stone_at_every_turn`.
    pub fn placed(facet: &Facet, at: [f32; 3], inset: f32, turn: u32) -> ([[f32; 3]; 4], u8) {
        let texel = (1.0 - 2.0 * inset) / GRID as f32;
        let corners = facet.corners.map(|[x, y, z]| {
            // About the middle of the cell, in blocks.
            let (mut dx, mut dz) = (inset + x * texel - 0.5, inset + z * texel - 0.5);
            for _ in 0..turn {
                (dx, dz) = (dz, -dx);
            }
            [at[0] + 0.5 + dx, at[1] + y * texel, at[2] + 0.5 + dz]
        });
        let mut face = facet.face;
        for _ in 0..turn {
            // (dx, dz) -> (dz, -dx) carries +X to -Z, -Z to -X, -X to +Z and
            // +Z to +X: `mesh::push_box`'s step, `[0, 1, 5, 4, 2, 3]`, and the
            // direction the flat quad turned its picture.
            face = match face {
                2 => 5,
                5 => 3,
                3 => 4,
                4 => 2,
                up_or_down => up_or_down,
            };
        }
        (corners, face)
    }

    /// The quarter turn a stone in this cell lies at: the same hash the
    /// flat quad turned its picture by, so a stone keeps the orientation
    /// it had.
    pub fn turn_of(cell: [i32; 3]) -> u32 {
        cell_hash(cell[0], cell[1], cell[2]) & 3
    }

    /// Appends the drawn surface as terrain vertices.
    #[allow(clippy::too_many_arguments)]
    pub fn append(
        &self,
        cell: [i32; 3],
        at: [f32; 3],
        inset: f32,
        layer: u32,
        light: u8,
        detail: crate::engine::lod::StoneDetail,
        vertices: &mut Vec<Vertex>,
        indices: &mut Vec<u32>,
    ) {
        let (sky, block_light) = (light & 0x0F, (light >> 4) & 0x0F);
        let turn = Self::turn_of(cell);
        for facet in self.surface(detail) {
            let (corners, face) = Self::placed(facet, at, inset, turn);
            // Unoccluded, as the flat quad was: the sides are a texel or
            // two tall, and the corner sampling that darkens a wall's foot
            // would read the cells around a stone, not the stone.
            let packed = pack_light(sky, block_light, 3, face);
            let base = vertices.len() as u32;
            for (corner, uv) in corners.into_iter().zip(facet.uv) {
                vertices.push(Vertex::new(corner, [0.0, 0.0], layer, packed).with_fine_uv(uv));
            }
            indices.extend_from_slice(&[base, base + 1, base + 2, base, base + 2, base + 3]);
        }
    }
}

/// Which texels of a picture are part of its shape, rows first, on the
/// sixteen-texel grid whatever the picture's resolution.
fn mask_of(image: &RgbaImage) -> [[bool; GRID]; GRID] {
    let (width, height) = image.dimensions();
    let mut solid = [[false; GRID]; GRID];
    if width == 0 || height == 0 {
        return solid;
    }
    for (row, line) in solid.iter_mut().enumerate() {
        for (column, texel) in line.iter_mut().enumerate() {
            // The middle of the sixteenth, not its corner: at sixteen
            // texels that is the texel itself, and at any other size it
            // is the texel the eye would say is there.
            let x = ((column as u32 * 2 + 1) * width / (GRID as u32 * 2)).min(width - 1);
            let y = ((row as u32 * 2 + 1) * height / (GRID as u32 * 2)).min(height - 1);
            *texel = image.get_pixel(x, y).0[3] >= ALPHA_CUTOFF;
        }
    }
    solid
}

/// A thing hung on a drying rack, read off its own picture once, at load.
///
/// **"в сушилке пусть будет не плоская хрень, а рыба или мясо".** What
/// hung under a rack's ridge was one slab wearing the carried picture --
/// a sticker of a fish, with the picture's transparent corners cut out of
/// a flat board. The rack now hangs the things themselves
/// (`mesh::hang_goods`), and two of the three ways it draws them need
/// something only the picture knows:
///
/// * **A fish or a frond is its own silhouette**, a [`Relief`] stood on
///   end and mirrored into a solid -- the shape a pebble on the ground is,
///   and near enough what a held fish already is (`item_model`). It hangs
///   by the end the picture draws it held by: see `tail` and `down`.
/// * **A strip of jerky or a sod of peat is a box**, and a box face
///   wearing an icon shows the icon's transparent margin as holes. It
///   wears `swatch` instead: the largest rectangle of the picture with no
///   hole in it, which is the thing's own colour and grain. Found rather
///   than written down, so a resource pack that redraws dried meat
///   changes the colour of the strips with it.
///
/// Compared, before choosing: boxes wearing a colour written in Rust
/// (rejected -- the atlas already holds the colour, and a number in code
/// drifts from the art the first time the art changes), and every good as
/// an extruded icon (rejected for meat and peat -- a steak-shaped icon
/// hung up by a corner is a steak on a hook, not strips drying, and a sod
/// is a brick however its icon is drawn).
#[derive(Debug, Clone, Default)]
pub struct Hung {
    /// The silhouette, lying down as a pebble would: see `Relief`.
    pub relief: Relief,
    /// `[u0, v0, u1, v1]` in pictures: the largest hole-free rectangle,
    /// pulled half a texel in from every side so the filter never reaches
    /// the transparent texel beyond it. All zero for a picture with no
    /// opaque texel at all.
    pub swatch: [f32; 4],
    /// Where the thing is held, in texels of the picture (x across, y
    /// down): the end of its long axis nearer the picture's lower left.
    pub tail: [f32; 2],
    /// From `tail` toward the other end, unit length, in the same axes.
    pub down: [f32; 2],
}

impl Hung {
    pub fn from_image(image: &RgbaImage) -> Self {
        Self::from_mask(&mask_of(image))
    }

    pub fn from_mask(solid: &[[bool; GRID]; GRID]) -> Self {
        let texels: Vec<[f32; 2]> = (0..GRID)
            .flat_map(|row| (0..GRID).map(move |column| (row, column)))
            .filter(|&(row, column)| solid[row][column])
            .map(|(row, column)| [column as f32 + 0.5, row as f32 + 0.5])
            .collect();
        if texels.is_empty() {
            return Self { down: [0.0, 1.0], ..Self::default() };
        }
        // The long axis: the principal direction of the opaque texels. A
        // diagonal icon -- every fish and frond in the pack -- is drawn at
        // whatever slant fitted the square, and hung at that slant it would
        // stick out sideways from its own cord.
        let n = texels.len() as f32;
        let mean = texels.iter().fold([0.0, 0.0], |m, t| [m[0] + t[0] / n, m[1] + t[1] / n]);
        let (mut xx, mut xy, mut yy) = (0.0f32, 0.0f32, 0.0f32);
        for t in &texels {
            let (dx, dy) = (t[0] - mean[0], t[1] - mean[1]);
            xx += dx * dx;
            xy += dx * dy;
            yy += dy * dy;
        }
        let angle = 0.5 * (2.0 * xy).atan2(xx - yy);
        let mut down = [angle.cos(), angle.sin()];
        // **Held by the lower-left end**: where a diagonal icon puts the
        // handle of a thing -- a knife's grip, a fish's tail, a frond's
        // stem. So `down` points toward the upper right, away from the
        // corner at (0, 16).
        if down[0] - down[1] < 0.0 {
            down = [-down[0], -down[1]];
        }
        let reach = texels
            .iter()
            .map(|t| (t[0] - mean[0]) * down[0] + (t[1] - mean[1]) * down[1])
            .fold(0.0f32, f32::min)
            // To the outer edge of the last texel, not its middle.
            - 0.5;
        let tail = [mean[0] + down[0] * reach, mean[1] + down[1] * reach];
        let swatch = largest_rectangle(solid).map_or([0.0; 4], |[x0, y0, x1, y1]| {
            let g = GRID as f32;
            [(x0 as f32 + 0.5) / g, (y0 as f32 + 0.5) / g, (x1 as f32 - 0.5) / g, (y1 as f32 - 0.5) / g]
        });
        Self { relief: Relief::from_mask(solid), swatch, tail, down }
    }
}

/// The largest rectangle of opaque texels, `[x0, y0, x1, y1]` with the far
/// edges exclusive; of two the same size, the squarer, since a box of any
/// proportion samples it. Brute force: a sixteen-texel picture is a few
/// thousand rectangles, once a picture, at load.
fn largest_rectangle(solid: &[[bool; GRID]; GRID]) -> Option<[usize; 4]> {
    let mut best: Option<([usize; 4], (usize, usize))> = None;
    for y0 in 0..GRID {
        for x0 in 0..GRID {
            // The widest run from here on each row down, narrowing as it goes.
            let mut width = GRID - x0;
            for y1 in y0 + 1..=GRID {
                let run = (x0..x0 + width).take_while(|&x| solid[y1 - 1][x]).count();
                width = width.min(run);
                if width == 0 {
                    break;
                }
                let score = (width * (y1 - y0), width.min(y1 - y0));
                if best.is_none_or(|(_, kept)| score > kept) {
                    best = Some(([x0, y0, x0 + width, y1], score));
                }
            }
        }
    }
    best.map(|(rectangle, _)| rectangle)
}

/// A texel's height, nought off the picture.
fn height_in(heights: &Heights, x: i32, z: i32) -> u8 {
    if (0..GRID as i32).contains(&x) && (0..GRID as i32).contains(&z) {
        heights[z as usize][x as usize]
    } else {
        0
    }
}

/// Tops over every rectangle of one height, sides along every run of edge:
/// the stone and nothing the cut-out has to take away. See the module note.
fn exact_surface(heights: &Heights) -> Vec<Facet> {
    let at = |x: i32, z: i32| height_in(heights, x, z);
    let mut exact = Vec::new();
    for rect in rectangles(heights, |h| h == 2) {
        exact.push(top(rect, 2.0));
    }
    for rect in rectangles(heights, |h| h == 1) {
        exact.push(top(rect, 1.0));
    }
    for tier in [Tier::Rim, Tier::Crown] {
        for side in Side::ALL {
            for line in 0..GRID as i32 {
                let exposed = |b: i32| {
                    let (x, z) = side.texel(line, b);
                    let (nx, nz) = side.beyond(x, z);
                    at(x, z) >= tier.level() && at(nx, nz) < tier.level()
                };
                for (from, to) in runs(exposed, |_| false) {
                    exact.push(side.facet(line, from, to, tier));
                }
            }
        }
    }
    exact
}

fn heights_of(solid: &[[bool; GRID]; GRID]) -> Heights {
    let mut heights = [[0u8; GRID]; GRID];
    let opaque = |x: i32, z: i32| {
        (0..GRID as i32).contains(&x) && (0..GRID as i32).contains(&z) && solid[z as usize][x as usize]
    };
    for (z, line) in heights.iter_mut().enumerate() {
        for (x, h) in line.iter_mut().enumerate() {
            let (x, z) = (x as i32, z as i32);
            if !opaque(x, z) {
                continue;
            }
            // Four neighbours, not eight: with eight, a five-texel round
            // pebble keeps a crown of one texel -- a pimple -- and with four
            // it keeps a plus of five, which reads as the top of a stone.
            let core = opaque(x - 1, z) && opaque(x + 1, z) && opaque(x, z - 1) && opaque(x, z + 1);
            *h = if core { 2 } else { 1 };
        }
    }
    heights
}

/// Greedy rectangles over the texels `wanted` accepts: `[x0, z0, x1, z1]`,
/// the far corner exclusive. The standard sweep: run right, then down.
fn rectangles(heights: &Heights, wanted: impl Fn(u8) -> bool) -> Vec<[usize; 4]> {
    let mut covered = [[false; GRID]; GRID];
    let mut out = Vec::new();
    for z0 in 0..GRID {
        for x0 in 0..GRID {
            if covered[z0][x0] || !wanted(heights[z0][x0]) {
                continue;
            }
            let mut x1 = x0 + 1;
            while x1 < GRID && !covered[z0][x1] && wanted(heights[z0][x1]) {
                x1 += 1;
            }
            let mut z1 = z0 + 1;
            while z1 < GRID && (x0..x1).all(|x| !covered[z1][x] && wanted(heights[z1][x])) {
                z1 += 1;
            }
            for line in covered.iter_mut().take(z1).skip(z0) {
                for texel in line.iter_mut().take(x1).skip(x0) {
                    *texel = true;
                }
            }
            out.push([x0, z0, x1, z1]);
        }
    }
    out
}

/// Runs of `exposed` texels along a line, each `(first, last)` inclusive,
/// joined across whatever `bridges` lets them.
///
/// A run is closed at the first texel it may not cross, and ends at the last
/// exposed texel before that -- never on a bridged one, which would be a
/// side sticking out past the edge it belongs to.
fn runs(exposed: impl Fn(i32) -> bool, bridges: impl Fn(i32) -> bool) -> Vec<(i32, i32)> {
    let mut out = Vec::new();
    let mut open: Option<(i32, i32)> = None;
    for b in 0..GRID as i32 {
        if exposed(b) {
            open = Some(match open {
                Some((first, _)) => (first, b),
                None => (b, b),
            });
        } else if !bridges(b) {
            if let Some(run) = open.take() {
                out.push(run);
            }
        }
    }
    out.extend(open);
    out
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Tier {
    /// The slab every opaque texel stands in: ground to one texel.
    Rim,
    /// The core on top of it: one texel to two.
    Crown,
}

impl Tier {
    fn level(self) -> u8 {
        match self {
            Tier::Rim => 1,
            Tier::Crown => 2,
        }
    }

    fn heights(self) -> (f32, f32) {
        match self {
            Tier::Rim => (0.0, 1.0),
            Tier::Crown => (1.0, 2.0),
        }
    }

    /// Whether a side of this tier may not be carried across a texel of
    /// height `h` in its own column.
    ///
    /// **A rim side crosses anything**: an empty texel is discarded, and an
    /// opaque one is under the rim's top. **A crown side crosses only the
    /// crown or nothing**: over a rim texel it would stand a texel tall in
    /// the open, on top of the rim, wearing that texel's colour.
    #[cfg(test)]
    fn blocks_a_span(self, h: u8) -> bool {
        match self {
            Tier::Rim => false,
            Tier::Crown => h == 1,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Side {
    /// Facing -X: the left edge of a column.
    West,
    /// Facing +X.
    East,
    /// Facing -Z: the top edge of a row of the picture.
    North,
    /// Facing +Z.
    South,
}

impl Side {
    const ALL: [Side; 4] = [Side::West, Side::East, Side::North, Side::South];

    /// The texel at step `b` along `line`: lines are columns for West and
    /// East, rows for North and South.
    fn texel(self, line: i32, b: i32) -> (i32, i32) {
        match self {
            Side::West | Side::East => (line, b),
            Side::North | Side::South => (b, line),
        }
    }

    /// The texel across the edge this side stands on.
    fn beyond(self, x: i32, z: i32) -> (i32, i32) {
        match self {
            Side::West => (x - 1, z),
            Side::East => (x + 1, z),
            Side::North => (x, z - 1),
            Side::South => (x, z + 1),
        }
    }

    /// The side of texels `from..=to` along `line`, for `tier`.
    ///
    /// **Wound anticlockwise seen from outside**, which is the only
    /// statement a quad makes about where it faces. The picture on it is the
    /// texel it stands on: along the run, the run's own texels; across the
    /// height, that one texel's width, so a side a texel high shows a
    /// texel's worth of colour -- see `INTO_TEXEL` for the margin.
    fn facet(self, line: i32, from: i32, to: i32, tier: Tier) -> Facet {
        let (y0, y1) = tier.heights();
        let g = GRID as f32;
        let (a0, a1) = (from as f32, (to + 1) as f32);
        let (l0, l1) = (line as f32, (line + 1) as f32);
        // Across the texel, inset from both of its edges.
        let (c0, c1) = (l0 / g + INTO_TEXEL, l1 / g - INTO_TEXEL);
        // ...and along the run, inset at its two ends: where a side ends at a
        // stone's corner the texel past the end is transparent, for the same
        // reason as across. **Not the pale one-pixel fringe along a stone's
        // top edges** that the loupe shows (`relief_repro`, `turn_s` and
        // `turn_s_one_sample`): this inset was tried against it and changed
        // nothing, it is there at one sample a pixel, and the flat quad had
        // it along its silhouette before any stone had a side. It is the
        // cut-out's edge, not the relief's.
        let (e0, e1) = (a0 / g + INTO_TEXEL, a1 / g - INTO_TEXEL);
        match self {
            Side::West => Facet {
                corners: [[l0, y0, a0], [l0, y0, a1], [l0, y1, a1], [l0, y1, a0]],
                uv: [[c0, e0], [c0, e1], [c1, e1], [c1, e0]],
                face: 3,
            },
            Side::East => Facet {
                corners: [[l1, y0, a1], [l1, y0, a0], [l1, y1, a0], [l1, y1, a1]],
                uv: [[c1, e1], [c1, e0], [c0, e0], [c0, e1]],
                face: 2,
            },
            Side::North => Facet {
                corners: [[a1, y0, l0], [a0, y0, l0], [a0, y1, l0], [a1, y1, l0]],
                uv: [[e1, c0], [e0, c0], [e0, c1], [e1, c1]],
                face: 5,
            },
            Side::South => Facet {
                corners: [[a0, y0, l1], [a1, y0, l1], [a1, y1, l1], [a0, y1, l1]],
                uv: [[e0, c1], [e1, c1], [e1, c0], [e0, c0]],
                face: 4,
            },
        }
    }
}

/// A top over texels `[x0, z0, x1, z1)` at height `y`, wound to face up.
fn top([x0, z0, x1, z1]: [usize; 4], y: f32) -> Facet {
    let g = GRID as f32;
    let (x0, z0, x1, z1) = (x0 as f32, z0 as f32, x1 as f32, z1 as f32);
    Facet {
        corners: [[x0, y, z0], [x0, y, z1], [x1, y, z1], [x1, y, z0]],
        uv: [[x0 / g, z0 / g], [x0 / g, z1 / g], [x1 / g, z1 / g], [x1 / g, z0 / g]],
        face: 0,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::engine::texture::FaceLayers;
    use primitive_shared::types::{
        BLOCK_ASH, BLOCK_FLINT, BLOCK_FLINT_FLAKE, BLOCK_LILY_PAD, BLOCK_NATIVE_COPPER, BLOCK_PEBBLE,
        BLOCK_RUSTY_STONE, BLOCK_SHELL, BLOCK_SNOW_COVER, BLOCK_STICK, BLOCK_STREAM_TIN,
    };

    const STONES: [BlockId; 8] = [
        BLOCK_STICK,
        BLOCK_PEBBLE,
        BLOCK_FLINT,
        BLOCK_FLINT_FLAKE,
        BLOCK_NATIVE_COPPER,
        BLOCK_RUSTY_STONE,
        BLOCK_STREAM_TIN,
        BLOCK_SHELL,
    ];

    fn name(block: BlockId) -> &'static str {
        primitive_shared::blocks::definition(block).name
    }

    /// A shape drawn in the test, `#` opaque, for what the shipped pictures
    /// happen not to have: a crown interrupted by a notch of rim, a hole in
    /// a stone, a stroke running off the picture's edge.
    fn awkward() -> Relief {
        let rows = [
            "#######.........",
            "#######.........",
            "#######....###..",
            "###.###....###..",
            "#######....###..",
            "##.####.........",
            "#######.........",
            "................",
            "....#########...",
            "....#########...",
            "....####.####...",
            "....#########...",
            "....#########...",
            "...............#",
            "..............##",
            ".............##.",
        ];
        let mut solid = [[false; GRID]; GRID];
        for (z, row) in rows.iter().enumerate() {
            for (x, c) in row.chars().enumerate() {
                solid[z][x] = c == '#';
            }
        }
        Relief::from_mask(&solid)
    }

    /// The texels a surface's tops cover, and the height they cover them at.
    fn footprint(facets: &[Facet]) -> [[u8; GRID]; GRID] {
        let mut covered = [[0u8; GRID]; GRID];
        for facet in facets.iter().filter(|f| f.face == 0) {
            let (x0, z0) = (facet.corners[0][0] as usize, facet.corners[0][2] as usize);
            let (x1, z1) = (facet.corners[2][0] as usize, facet.corners[2][2] as usize);
            for line in covered.iter_mut().take(z1).skip(z0) {
                for texel in line.iter_mut().take(x1).skip(x0) {
                    *texel = facet.corners[0][1] as u8;
                }
            }
        }
        covered
    }

    /// **The far band gives up the crown and nothing else.** A slab that
    /// covered a different set of texels would be a different stone seen
    /// from above, and the line it takes over at is four chunks away --
    /// near enough that a stone is still a shape rather than a dot.
    #[test]
    fn a_stone_past_the_default_line_keeps_its_outline_and_loses_its_crown() {
        for (name, relief) in every_model() {
            let whole = footprint(&relief.drawn);
            let slab = footprint(&relief.slab);
            for (z, (line, flat)) in whole.iter().zip(slab.iter()).enumerate() {
                for (x, (&tall, &low)) in line.iter().zip(flat.iter()).enumerate() {
                    assert_eq!(
                        tall > 0,
                        low > 0,
                        "{name}: texel {x},{z} is in one silhouette and not the other"
                    );
                    assert!(low <= 1, "{name}: texel {x},{z} kept a crown in the slab");
                }
            }
            assert!(
                relief.slab.len() < relief.drawn.len(),
                "{name}: the slab is {} quads against {} -- no band is worth a remesh for that",
                relief.slab.len(),
                relief.drawn.len()
            );
            assert!(relief.slab.iter().all(|f| f.corners.iter().all(|c| c[1] <= 1.0)));
        }
    }

    /// **What the far band is worth, over the things a player walks past.**
    /// A stick is a stroke two texels wide and has almost no core to lose
    /// (46 quads against 58); a pebble, a flake and a shell are half or
    /// better. The number that matters is the sum, because it is what the
    /// chunks out there actually send.
    #[test]
    fn one_tier_is_about_half_the_triangles_of_two() {
        let (mut whole, mut flat) = (0, 0);
        for (_, relief) in every_model() {
            whole += relief.drawn.len();
            flat += relief.slab.len();
        }
        assert!(
            flat * 10 <= whole * 6,
            "the slab band saves too little to be worth its remesh: {flat} quads against {whole}"
        );
    }

    fn every_model() -> Vec<(String, Relief)> {
        let layers = FaceLayers::empty_for_test();
        let mut models: Vec<(String, Relief)> = STONES
            .iter()
            .map(|&block| {
                let relief = layers.relief(block).expect("a stone lying on the ground has a thickness");
                (name(block).to_string(), relief.clone())
            })
            .collect();
        models.push(("awkward".into(), awkward()));
        models
    }

    /// The height map a model was built from, read back off its exact tops.
    fn model_heights(relief: &Relief) -> Heights {
        let mut heights = [[0u8; GRID]; GRID];
        for facet in relief.exact.iter().filter(|f| f.face == 0) {
            let (x0, z0) = (facet.corners[0][0] as usize, facet.corners[0][2] as usize);
            let (x1, z1) = (facet.corners[2][0] as usize, facet.corners[2][2] as usize);
            for line in heights.iter_mut().take(z1).skip(z0) {
                for h in line.iter_mut().take(x1).skip(x0) {
                    *h = facet.corners[0][1] as u8;
                }
            }
        }
        heights
    }

    fn height_at(heights: &Heights, x: f32, z: f32) -> u8 {
        let (x, z) = (x.floor() as i32, z.floor() as i32);
        if (0..GRID as i32).contains(&x) && (0..GRID as i32).contains(&z) {
            heights[z as usize][x as usize]
        } else {
            0
        }
    }

    fn normal_of(corners: &[[f32; 3]; 4]) -> [f32; 3] {
        let e1: [f32; 3] = std::array::from_fn(|a| corners[1][a] - corners[0][a]);
        let e2: [f32; 3] = std::array::from_fn(|a| corners[2][a] - corners[1][a]);
        [
            e1[1] * e2[2] - e1[2] * e2[1],
            e1[2] * e2[0] - e1[0] * e2[2],
            e1[0] * e2[1] - e1[1] * e2[0],
        ]
    }

    /// The mesher's six directions, in face order.
    const NORMALS: [[f32; 3]; 6] =
        [[0.0, 1.0, 0.0], [0.0, -1.0, 0.0], [1.0, 0.0, 0.0], [-1.0, 0.0, 0.0], [0.0, 0.0, 1.0], [0.0, 0.0, -1.0]];

    /// A texel behind a side and the one in front of it, in picture texels.
    type TexelPair = ([f32; 2], [f32; 2]);

    /// Where a side stands and which texel it wears: the level it is the
    /// side of, the axis it runs along (0 = x, 1 = z), its extent along it,
    /// and for each texel step the texel behind it and the one in front.
    fn side_texels(facet: &Facet) -> (u8, Vec<TexelPair>) {
        let n = NORMALS[facet.face as usize];
        let bottom = facet.corners.iter().map(|c| c[1]).fold(f32::MAX, f32::min);
        let level = bottom as u8 + 1;
        let lo: [f32; 2] = [0, 2].map(|a| facet.corners.iter().map(|c| c[a]).fold(f32::MAX, f32::min));
        let hi: [f32; 2] = [0, 2].map(|a| facet.corners.iter().map(|c| c[a]).fold(f32::MIN, f32::max));
        let along = if n[0] != 0.0 { 1 } else { 0 };
        let texels = (lo[along] as i32..hi[along] as i32)
            .map(|step| {
                let mut behind = [lo[0] - 0.5 * n[0], lo[1] - 0.5 * n[2]];
                behind[along] = step as f32 + 0.5;
                (behind, [behind[0] + n[0], behind[1] + n[2]])
            })
            .collect();
        (level, texels)
    }

    #[test]
    fn every_side_of_a_stone_points_out_of_the_stone_at_every_turn() {
        // **Two bugs this family has already had, both invisible to a check
        // of coordinates.** `push_box` wound +Z and -Z inside out, and the
        // drying rack showed its far poles through its near ones; animals
        // wrote a face index before turning, and were lit welded to their own
        // bodies. So this reads the *emitted vertices* -- the corners the
        // rasteriser gets and the face index the shader makes a normal of --
        // and holds both to the stone itself: the winding's normal is the
        // face index's, and a step along it from the middle of an exact face
        // leaves the stone while a step against it goes in.
        const INSET: f32 = 0.13;
        let texel = (1.0 - 2.0 * INSET) / GRID as f32;
        for (name, relief) in every_model() {
            for cell_x in 0..16 {
                let cell = [cell_x, 7, 3];
                let turn = Relief::turn_of(cell);
                let (mut vertices, mut indices) = (Vec::new(), Vec::new());
                relief.append(cell, [0.0; 3], INSET, 5, 0xFF, crate::engine::lod::StoneDetail::Full, &mut vertices, &mut indices);
                assert_eq!(vertices.len(), relief.drawn.len() * 4, "{name}");
                assert_eq!(indices.len(), relief.drawn.len() * 6, "{name}");
                for quad in vertices.chunks_exact(4) {
                    let face = (quad[0].light() >> 10) & 7;
                    assert!(quad.iter().all(|v| (v.light() >> 10) & 7 == face), "{name}: a quad of two faces");
                    let n = normal_of(&[0, 1, 2, 3].map(|k| quad[k].position));
                    let length = n.iter().map(|c| c * c).sum::<f32>().sqrt();
                    let along: f32 = (0..3).map(|a| n[a] * NORMALS[face as usize][a]).sum();
                    assert!(
                        length > 0.0 && (along - length).abs() <= 1e-3 * length,
                        "{name} at turn {turn}: a quad wound towards {n:?} says it faces {face}"
                    );
                }
            }

            // Inside and outside, on the exact surface at every turn,
            // taken back into the picture by the inverse of the turn.
            let heights = model_heights(&relief);
            for turn in 0..4 {
                for facet in &relief.exact {
                    let (corners, face) = Relief::placed(facet, [0.0; 3], INSET, turn);
                    let middle: [f32; 3] = std::array::from_fn(|a| corners.iter().map(|c| c[a]).sum::<f32>() / 4.0);
                    let stone_at = |sign: f32| {
                        let p: [f32; 3] =
                            std::array::from_fn(|a| middle[a] + sign * 0.25 * texel * NORMALS[face as usize][a]);
                        let (mut dx, mut dz) = (p[0] - 0.5, p[2] - 0.5);
                        for _ in 0..turn {
                            (dx, dz) = (-dz, dx);
                        }
                        let (x, z) = ((dx + 0.5 - INSET) / texel, (dz + 0.5 - INSET) / texel);
                        p[1] / texel < height_at(&heights, x, z) as f32
                    };
                    assert!(stone_at(-1.0), "{name} at turn {turn}: behind a face towards {face} is not stone");
                    assert!(!stone_at(1.0), "{name} at turn {turn}: in front of a face towards {face} is stone");
                }
            }
        }
    }

    #[test]
    fn no_side_of_a_stone_leaves_its_silhouette_to_the_cut_out() {
        // **The slit of grass down a stone's flank.** A side carried across a
        // line from the first edge to the last -- discarded over transparent
        // texels, hidden inside the stone over opaque ones -- ends where the
        // discard decides beside a side that ends where its triangle does,
        // and the ground shows between them. So every texel a drawn side
        // stands on is an edge of its level: the stone opaque behind it, lower
        // in front. Checked texel by texel against the height map.
        //
        // ...and the other way: every edge the height map has is under a drawn
        // side, or the stone has a hole in its flank.
        for (name, relief) in every_model() {
            let heights = model_heights(&relief);
            let mut shown = std::collections::HashSet::new();
            for facet in relief.drawn.iter().filter(|f| f.face != 0) {
                let (level, texels) = side_texels(facet);
                for (behind, front) in texels {
                    let own = height_at(&heights, behind[0], behind[1]);
                    let beyond = height_at(&heights, front[0], front[1]);
                    assert!(
                        own >= level && beyond < level,
                        "{name}: a side at level {level} stands over texel {behind:?} \
                         (height {own}, beyond it {beyond}), which is not an edge"
                    );
                    assert!(
                        shown.insert((facet.face, level, behind[0] as i32, behind[1] as i32)),
                        "{name}: two sides stand on the edge of texel {behind:?}"
                    );
                }
            }
            for (z, row) in heights.iter().enumerate() {
                for (x, &own) in row.iter().enumerate() {
                    for level in 1..=own {
                        for (face, dx, dz) in [(2u8, 1i32, 0i32), (3, -1, 0), (4, 0, 1), (5, 0, -1)] {
                            let beyond = height_at(&heights, x as f32 + 0.5 + dx as f32, z as f32 + 0.5 + dz as f32);
                            if beyond < level {
                                assert!(
                                    shown.contains(&(face, level, x as i32, z as i32)),
                                    "{name}: the edge of texel ({x}, {z}) facing {face} at level {level} is not drawn"
                                );
                            }
                        }
                    }
                }
            }
        }
    }

    #[test]
    fn no_top_of_a_stone_leaves_its_silhouette_to_the_cut_out() {
        // **The seam along a stone's top edges.** A top drawn over texels it
        // does not stand on -- the whole picture, as the rim's used to be --
        // has its edge wherever the discard decides, once per pixel, while
        // the side under that edge is geometry decided per sample; the ground
        // shows between the two. So every drawn top covers exactly texels of
        // its own level, every texel of every level is under exactly one top,
        // and nothing about a top's outline is left to the alpha.
        for (name, relief) in every_model() {
            let heights = model_heights(&relief);
            let mut covered = [[0u8; GRID]; GRID];
            for facet in relief.drawn.iter().filter(|f| f.face == 0) {
                let level = facet.corners[0][1] as u8;
                let (x0, z0) = (facet.corners[0][0] as usize, facet.corners[0][2] as usize);
                let (x1, z1) = (facet.corners[2][0] as usize, facet.corners[2][2] as usize);
                for (z, line) in covered.iter_mut().enumerate().take(z1).skip(z0) {
                    for (x, count) in line.iter_mut().enumerate().take(x1).skip(x0) {
                        assert_eq!(
                            heights[z][x], level,
                            "{name}: a top at level {level} is drawn over texel ({x}, {z}) of height {}",
                            heights[z][x]
                        );
                        *count += 1;
                    }
                }
            }
            for z in 0..GRID {
                for x in 0..GRID {
                    let wanted = u8::from(heights[z][x] > 0);
                    assert_eq!(covered[z][x], wanted, "{name}: texel ({x}, {z}) is under {} tops", covered[z][x]);
                }
            }
        }
    }

    #[test]
    fn a_side_wears_the_texel_it_stands_on() {
        // Along the run, the run's own texels; across the height, inside the
        // one texel and never on its boundary, where the filter takes half of
        // the transparent texel beside it and the cut-out shaves the side.
        for (name, relief) in every_model() {
            for facet in relief.drawn.iter().chain(&relief.exact).filter(|f| f.face != 0) {
                let n = NORMALS[facet.face as usize];
                let across = if n[0] != 0.0 { 0 } else { 1 };
                let us: Vec<f32> = facet.uv.iter().map(|uv| uv[across] * GRID as f32).collect();
                let column = us[0].floor();
                assert!(
                    us.iter().all(|&u| u > column && u < column + 1.0),
                    "{name}: a side's picture leaves its texel: {us:?}"
                );
                let (_, texels) = side_texels(facet);
                let behind = texels[0].0[across].floor();
                assert_eq!(column, behind, "{name}: a side wears a texel it does not stand on");
                // Along: the run's own texels, and never the boundary at
                // either end, which is where the pale corner hairline was.
                let along = 1 - across;
                let first = texels[0].0[along].floor();
                let last = texels[texels.len() - 1].0[along].floor() + 1.0;
                for uv in facet.uv {
                    let a = uv[along] * GRID as f32;
                    assert!(a > first && a < last, "{name}: a side's picture reaches past its run: {a} outside {first}..{last}");
                }
            }
        }
    }

    #[test]
    fn a_stone_stands_up_and_a_coating_does_not() {
        for block in STONES {
            assert!(has_relief(block), "{} lies flat", name(block));
        }
        // Ash and snow *are* the floor there, and a lily pad floats.
        for block in [BLOCK_ASH, BLOCK_SNOW_COVER, BLOCK_LILY_PAD] {
            assert!(!has_relief(block), "{} got a thickness", name(block));
        }
        // A pebble is a lump, not a coin: some of it is two texels tall.
        let layers = FaceLayers::empty_for_test();
        let pebble = layers.relief(BLOCK_PEBBLE).unwrap();
        assert!(pebble.drawn.iter().any(|f| f.face == 0 && f.corners[0][1] == 2.0), "a pebble has no crown");
    }

    #[test]
    fn what_the_things_on_the_ground_cost() {
        // Bounded so a picture redrawn with a ragged edge cannot quietly make
        // every stone in the world a hundred triangles: the flat quad was two.
        // 240 since the exact surface is what is drawn -- the flint, two
        // flakes with a notch each, is 216 -- where it was 200 for the
        // surface that leaned on the cut-out (see the module note).
        for (name, relief) in every_model() {
            println!(
                "{name}: {} triangles drawn, {} as it shipped",
                relief.triangles(),
                relief.as_it_shipped().triangles()
            );
            if name != "awkward" {
                assert!(relief.triangles() <= 240, "{name} costs {} triangles", relief.triangles());
            }
        }
    }

    /// A mask from rows of `#` and `.`, rows first.
    fn mask(rows: [&str; GRID]) -> [[bool; GRID]; GRID] {
        rows.map(|row| {
            let mut line = [false; GRID];
            for (texel, c) in line.iter_mut().zip(row.chars()) {
                *texel = c == '#';
            }
            line
        })
    }

    #[test]
    fn a_hung_goods_swatch_holds_no_transparent_texel_of_its_picture() {
        // A strip of jerky is a box, and a box wearing a picture with holes
        // in it shows the holes: the swatch is what makes the strip solid
        // (`Hung`). Half a texel in from every side, so the filter never
        // reaches past it either -- checked by widening it back out and
        // finding only opaque texels under it.
        let fish = mask([
            "................",
            "...........###..",
            ".........######.",
            "........#######.",
            ".......########.",
            "......#######...",
            ".....#######....",
            "....#######.....",
            "...#######......",
            "..######........",
            ".#####..........",
            "###.#...........",
            "##..............",
            "................",
            "................",
            "................",
        ]);
        let hung = Hung::from_mask(&fish);
        let [u0, v0, u1, v1] = hung.swatch.map(|c| c * GRID as f32);
        assert!(u1 > u0 && v1 > v0, "a fish has a piece with no hole in it: {:?}", hung.swatch);
        let (rows, columns) = ((v0 - 0.5).round() as usize..(v1 + 0.5).round() as usize, (u0 - 0.5).round() as usize..(u1 + 0.5).round() as usize);
        for (y, line) in fish.iter().enumerate().take(rows.end).skip(rows.start) {
            for (x, &opaque) in line.iter().enumerate().take(columns.end).skip(columns.start) {
                assert!(opaque, "the swatch {:?} covers the hole at ({x}, {y})", hung.swatch);
            }
        }
    }

    #[test]
    fn a_diagonal_picture_hangs_by_its_lower_left_end_along_its_own_slant() {
        // A fish is drawn corner to corner, tail at the lower left, as a
        // knife is drawn grip there. Hung by the other end it hangs head up;
        // hung plumb along the picture's edge instead of its own slant it
        // sticks out sideways from its cord.
        let mut stroke = [[false; GRID]; GRID];
        for k in 1..15 {
            stroke[15 - k][k] = true;
            stroke[15 - k][k - 1] = true;
        }
        let hung = Hung::from_mask(&stroke);
        assert!(hung.tail[0] < 3.0 && hung.tail[1] > 13.0, "held at {:?}, not the lower left", hung.tail);
        let slant = std::f32::consts::FRAC_1_SQRT_2;
        assert!(
            (hung.down[0] - slant).abs() < 0.05 && (hung.down[1] + slant).abs() < 0.05,
            "hangs along {:?}, not the stroke's own slant toward the upper right",
            hung.down
        );
    }

    #[test]
    fn every_good_a_rack_hangs_has_a_picture_with_a_shape_and_a_swatch() {
        // The shipped pictures, as the mesher gets them: a good whose
        // picture yielded no shape would hang nothing, and one whose swatch
        // is a single texel would be a strip of one colour stretched along
        // a box -- the "texture that did not load" look.
        let layers = FaceLayers::empty_for_test();
        for item in primitive_shared::rack::HANGING.iter().flatten().copied().chain([primitive_shared::types::BLOCK_CORD]) {
            let hung = layers.hung(item).unwrap_or_else(|| panic!("{}: a rack hangs it and has no shape for it", name(item)));
            assert!(!hung.relief.drawn.is_empty(), "{}: no silhouette", name(item));
        }
        // And the ones dressed in their swatch (`mesh::Dress`): a frond is
        // a stroke a texel wide and never wears one.
        use primitive_shared::types as t;
        for item in [t::BLOCK_LEATHER, t::BLOCK_DRIED_MEAT, t::BLOCK_SALTED_MEAT, t::BLOCK_DRIED_SALTED_MEAT, t::BLOCK_DRIED_PEAT, t::BLOCK_CORD] {
            let hung = layers.hung(item).expect("checked above");
            let [u0, v0, u1, v1] = hung.swatch.map(|c| c * GRID as f32);
            assert!(u1 - u0 >= 1.0 && v1 - v0 >= 1.0, "{}: swatch {:?} is under two texels a side", name(item), hung.swatch);
        }
    }
}

/// **What the thickness costs the mesher and the GPU**, measured in one
/// binary: the same chunks meshed with the reliefs and with the table that
/// has none (`FaceLayers::without_reliefs`), interleaved, best of seven.
///
/// ```text
/// cargo test --release -p primitive_client --lib what_stones_cost_the_mesher \
///     -- --ignored --nocapture
/// ```
///
/// Each country is the three-by-three at the middle of the nearest patch of
/// it, with a ring of real neighbours, as `arena::world_cost` measures a
/// country. The benchmark world (seed 4242) is then streamed whole at render
/// distance 24 with the stock LOD bands, for what the arena has to hold.
#[cfg(test)]
mod cost {
    use crate::engine::mesh::{build_mesh, MeshBuffers, Neighbourhood};
    use crate::engine::texture::FaceLayers;
    use crate::logic::chunk_manager::ChunkManager;
    use primitive_shared::lighting::{compute_isolated, LightMap};
    use primitive_shared::types::{is_flat, BlockId, Chunk, ChunkPos};
    use primitive_shared::worldgen::{Biome, Preset, WorldGen, Zone};

    /// A name, a picture table, and which chunks (by distance) get reliefs.
    type Variant<'a> = (&'a str, &'a FaceLayers, &'a dyn Fn(f32) -> bool);

    struct Sample {
        triangles: usize,
        sprite_triangles: usize,
        vertices: usize,
        seconds: f64,
    }

    fn mesh_all(
        positions: &[ChunkPos],
        chunks: &ChunkManager,
        light: &LightMap,
        layers: &FaceLayers,
        generator: &WorldGen,
    ) -> Sample {
        let mut cache = Box::<Neighbourhood>::default();
        let mut out = Box::<MeshBuffers>::default();
        let mut sample = Sample { triangles: 0, sprite_triangles: 0, vertices: 0, seconds: 0.0 };
        for pos in positions {
            cache.fill(*pos, chunks, light);
            out.clear();
            let started = std::time::Instant::now();
            build_mesh(*pos, &cache, layers, generator, &mut out);
            sample.seconds += started.elapsed().as_secs_f64();
            sample.triangles += out.indices.len() / 3;
            sample.sprite_triangles += (out.sprite_end - out.leaf_end) as usize / 3;
            sample.vertices += out.vertices.len();
        }
        sample
    }

    #[test]
    #[ignore = "a measurement, not an assertion -- run it explicitly, in release"]
    fn what_stones_cost_the_mesher() {
        let after = FaceLayers::numbered_for_test();
        let before = FaceLayers::numbered_for_test().without_reliefs();
        let places = [
            ("temperate plains", Zone::Temperate, Biome::Plains),
            ("temperate forest", Zone::Temperate, Biome::Forest),
            ("birch forest", Zone::Temperate, Biome::BirchForest),
            ("dead forest", Zone::Temperate, Biome::DeadForest),
            ("taiga", Zone::North, Biome::Taiga),
            ("tropical savanna", Zone::Tropics, Biome::Savanna),
            ("dry-belt savanna", Zone::DryBelt, Biome::Savanna),
            ("tropical beach", Zone::Tropics, Biome::Beach),
        ];
        for (name, zone, wanted) in places {
            let generator = WorldGen::with_zone(1234, Preset::Normal, zone);
            let mut nearest: Option<(i64, ChunkPos)> = None;
            for gz in (-3008..3008).step_by(64) {
                for gx in (-3008..3008).step_by(64) {
                    let distance = i64::from(gx) * i64::from(gx) + i64::from(gz) * i64::from(gz);
                    if nearest.is_some_and(|(best, _)| best <= distance) || generator.biome_at(gx + 8, gz + 8) != wanted {
                        continue;
                    }
                    nearest = Some((distance, ChunkPos::from_global(gx, gz).0));
                }
            }
            let Some((_, centre)) = nearest else {
                println!("[stones] {name:17} none within three thousand blocks");
                continue;
            };
            let around: Vec<ChunkPos> = (-2..=2)
                .flat_map(|dz| (-2..=2).map(move |dx| ChunkPos::new(centre.x + dx, centre.z + dz)))
                .collect();
            let generated: Vec<Chunk> = around.iter().map(|&pos| generator.generate_chunk(pos)).collect();
            let inner: Vec<ChunkPos> = around
                .iter()
                .copied()
                .filter(|pos| (pos.x - centre.x).abs() <= 1 && (pos.z - centre.z).abs() <= 1)
                .collect();
            let count = |wanted: &dyn Fn(BlockId) -> bool| -> usize {
                generated
                    .iter()
                    .filter(|chunk| inner.contains(&chunk.pos))
                    .map(|chunk| chunk.blocks.iter().filter(|&&b| wanted(b)).count())
                    .sum()
            };
            let lying = count(&super::has_relief);
            let coatings = count(&|b| is_flat(b) && !super::has_relief(b));
            let isolated: Vec<Vec<u8>> = generated.iter().map(|chunk| compute_isolated(&chunk.blocks)).collect();
            let mut chunks = ChunkManager::new(4);
            for chunk in generated {
                chunks.insert(chunk);
            }
            let mut light = LightMap::new();
            for (pos, data) in around.iter().zip(isolated) {
                light.insert_precomputed(&chunks, *pos, data);
            }
            let (mut was, mut now) = (f64::MAX, f64::MAX);
            let (mut old, mut new) = (None, None);
            for _ in 0..7 {
                let a = mesh_all(&inner, &chunks, &light, &before, &generator);
                let b = mesh_all(&inner, &chunks, &light, &after, &generator);
                was = was.min(a.seconds);
                now = now.min(b.seconds);
                (old, new) = (Some(a), Some(b));
            }
            let (old, new) = (old.unwrap(), new.unwrap());
            let n = inner.len() as f64;
            println!(
                "[stones] {name:17} {:.1} things lying, {:.1} coatings a chunk | triangles {:.0} -> {:.0} a chunk \
                 (sprites {:.0} -> {:.0}) | vertices {:.0} -> {:.0} | mesh {:.3} -> {:.3} ms/chunk",
                lying as f64 / n,
                coatings as f64 / n,
                old.triangles as f64 / n,
                new.triangles as f64 / n,
                old.sprite_triangles as f64 / n,
                new.sprite_triangles as f64 / n,
                old.vertices as f64 / n,
                new.vertices as f64 / n,
                was * 1e3 / n,
                now * 1e3 / n,
            );
        }

        // The benchmark world, streamed whole.
        const RADIUS: i32 = 24;
        let settings = crate::settings::ClientSettings::default();
        let generator = WorldGen::new(4242);
        let (sx, sz) = generator.spawn_column();
        let centre = ChunkPos::from_global(sx, sz).0;
        let probe = ChunkManager::new(RADIUS);
        let mut positions = Vec::new();
        for dx in -RADIUS..=RADIUS {
            for dz in -RADIUS..=RADIUS {
                if probe.inside(dx, dz) {
                    positions.push(ChunkPos::new(centre.x + dx, centre.z + dz));
                }
            }
        }
        let distance = |p: &ChunkPos| (((p.x - centre.x).pow(2) + (p.z - centre.z).pow(2)) as f32).sqrt();
        let generated: Vec<Chunk> = positions.iter().map(|&pos| generator.generate_chunk(pos)).collect();
        let isolated: Vec<Vec<u8>> = generated.iter().map(|chunk| compute_isolated(&chunk.blocks)).collect();
        let mut chunks = ChunkManager::new(RADIUS);
        for chunk in generated {
            chunks.insert(chunk);
        }
        let mut light = LightMap::new();
        for (pos, data) in positions.iter().zip(isolated) {
            light.insert_precomputed(&chunks, *pos, data);
        }
        let stride = std::mem::size_of::<crate::engine::mesh::Vertex>();
        // Flat everywhere (before), solid everywhere (what was measured first,
        // and why there is a line), and solid inside `lod::RELIEF_CHUNKS` as
        // the game meshes it.
        let everywhere = |_: f32| true;
        let ring = |d: f32| {
            crate::engine::lod::stones_at(d, crate::engine::lod::RELIEF_CHUNKS, crate::engine::lod::StoneDetail::Full)
                != crate::engine::lod::StoneDetail::Flat
        };
        let variants: [Variant; 3] =
            [("flat", &before, &everywhere), ("solid everywhere", &after, &everywhere), ("solid near", &after, &ring)];
        for (label, layers, near) in variants {
            let mut cache = Box::<Neighbourhood>::default();
            let mut out = Box::<MeshBuffers>::default();
            let (mut vertices, mut indices, mut sprites, mut close) = (0usize, 0usize, 0usize, 0usize);
            let mut seconds = 0.0;
            for pos in &positions {
                cache.fill(*pos, &chunks, &light);
                let start = crate::engine::lod::band_start(settings.lod_distance_chunks, cache.ceiling());
                let level = crate::engine::lod::level_at(distance(pos), start, 0);
                crate::engine::lod::coarsen(&mut cache, level, settings.lod_quality);
                cache.lay_stones(if near(distance(pos)) { crate::engine::lod::StoneDetail::Full } else { crate::engine::lod::StoneDetail::Flat });
                out.clear();
                let started = std::time::Instant::now();
                build_mesh(*pos, &cache, layers, &generator, &mut out);
                seconds += started.elapsed().as_secs_f64();
                vertices += out.vertices.len();
                indices += out.indices.len();
                let s = (out.sprite_end - out.leaf_end) as usize / 3;
                sprites += s;
                if distance(pos) <= 3.0 {
                    close += s;
                }
            }
            println!(
                "[stones] world 4242 r{RADIUS} {label:16}: {} chunks, {:.2} M triangles ({:.0} k in sprites, {:.0} k of \
                 them within three chunks), arena {:.1} MB, mesh {:.0} ms one thread",
                positions.len(),
                indices as f64 / 3.0 / 1e6,
                sprites as f64 / 1e3,
                close as f64 / 1e3,
                (vertices * stride + indices * 4) as f64 / (1024.0 * 1024.0),
                seconds * 1e3
            );
        }
    }
}
