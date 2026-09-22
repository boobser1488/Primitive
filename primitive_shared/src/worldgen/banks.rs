//! The edges of rivers.
//!
//! ## What was asked
//!
//! "Берега рек слишком аккуратные." A river here is a contour of a slow
//! noise field (`scale::RiverOrder`), and everything about its edge used to
//! be a constant read off that contour:
//!
//! * **One width.** The channel was `half_width` either side of the line for
//!   the whole length of every river of an order, so a brook was three blocks
//!   of water from the hills to the sea, and a river a canal.
//! * **One cross-section.** Both banks came down the same smoothstep, so
//!   every bank of every river was the same shelving ramp -- no bank you had
//!   to climb out over and no bar you could wade onto.
//! * **One level.** The valley pulled the country toward a floodplain three
//!   blocks over the water everywhere, so the top of every bank stood at the
//!   same height, a kerb.
//! * **One material.** Under the water `surface_for` reads depth alone, so
//!   every river's shallows were a ring of sand exactly as wide as three
//!   blocks of depth, and above it the turf ran to the edge except where the
//!   deposit field dropped a patch of clay or gravel.
//! * **One density of reeds.** Each waterline column rolled its plants on
//!   its own hash, which is an even sprinkle along every bank in the world.
//!
//! ## What a real bank does, and what is done here
//!
//! **The width wanders**, on a field a few widths long, and each bank on its
//! own field: one side pushes out into a bay while the other stands, and the
//! river is narrow in one reach and broad in the next (`RiverBank::edge`).
//!
//! **A river cuts the outside of a bend and builds the inside.** Water
//! swinging round a bend is thrown against the outer bank and undercuts it,
//! and slows on the inner side and drops its load there as a bar. So each
//! bank is *steep* or *shelving*: the curvature of the channel says which way
//! a bend leans, and a slower field adds the cut banks a straight reach has
//! anyway (`RiverBank::steep`). A cut bank keeps its ground to the water's
//! edge and drops as a wall one to three blocks tall into water that is deep
//! at its foot (`cut_bank`); a shelving bank is the old ramp, with the
//! shallows wide.
//!
//! **The floodplain rises and falls** by a block or so either way
//! (`FLOODPLAIN_SWING`), so a wall is three blocks in one place and one in
//! the next, and a low reach of plain comes down to the water as a flat.
//!
//! **The margin is made of what the water left there** (`river_margin`):
//! sand, gravel, mud and clay in patches along the shallows and the low bank,
//! turf to the water's edge between them, and on a cut bank the turf lip
//! with bare earth or stones showing where it has fallen in. The bed further
//! out is gravel where it runs and silt and mud where it does not.
//!
//! **Reeds stand in beds** with open bank between them (`waterline_stand`),
//! and **a rapid has stones standing in it** (`WorldGen::place_rapid_stones`).
//!
//! ## Why none of it can come apart at a chunk seam
//!
//! Every number here is a function of the seed and the column's own planet
//! coordinates -- noise and the river field, sampled where the column is --
//! which is the property that already lets the rivers be generated a chunk
//! at a time in any order. The one thing that looks at neighbours, whether a
//! dry column has river water beside it, reads the heights `build_column_tile`
//! already has a ring of, and the water is still the sea's level over every
//! column cut below it: a bank lowered under the waterline is flooded, and
//! one left above it is dry, so no water is ever left standing against air.
//!
//! ## Rejected
//!
//! * **An overhang: a real undercut, ground over a hollow of water.** It is
//!   what a cut bank is in life, and it is a column that is not a height --
//!   every rule from the slope to the cave seal to the reeds reads a column
//!   as one number. A wall that stands straight up out of deep water says
//!   "the river is eating this bank" to a player just as clearly.
//! * **Width and shape per river, by a hash of which river it is.** A river
//!   here is a contour with no identity to hash, and a whole river one width
//!   is the complaint moved from the world to each river in it.
//! * **The curvature alone for the shape.** A brook's field bends over
//!   hundreds of blocks and would have had shelving banks from end to end;
//!   the noise is what gives a straight reach its cut banks.

use noise::NoiseFn;

use crate::types::{
    BlockId, Chunk, BLOCK_AIR, BLOCK_CLAY, BLOCK_COBBLESTONE, BLOCK_DIRT, BLOCK_GRANITE, BLOCK_GRASS, BLOCK_GRAVEL, BLOCK_ICE, BLOCK_MUD,
    BLOCK_SAND, BLOCK_WATER, CHUNK_SIZE_X, CHUNK_SIZE_Y, CHUNK_SIZE_Z,
};

use super::scale::RiverOrder;
use super::{fbm, hash2, smoothstep, spline, Biome, ColumnCache, Surface, WorldGen, CONTINENT_SPLINE, RAPID_SPEED, SEA_LEVEL};

/// One stone per this many shallow columns of a river's bed, of those the
/// water runs over as a rapid. Rolled before the current is asked, which
/// costs as much as three columns of ground.
const RAPID_STONE_SPACING: u32 = 9;

/// How far a bank wanders from the order's `half_width`, as a share of it,
/// either way. A third: a brook of four blocks of water is three in one reach
/// and five in the next, which is a difference seen from the bank, and never
/// wide enough to break `a_brook_a_river_and_a_great_river_are_each_as_wide_as_their_order`.
const WANDER: f64 = 0.35;

/// How much wider the outside of the tightest bend is cut, and how much
/// narrower the inside, as a share of the width.
const BEND_WIDENS: f64 = 0.15;

/// The furthest a channel's cut ever reaches, in half widths: past this a
/// column's bank is never asked about. `WANDER` and `BEND_WIDENS` at their
/// widest together, and a hair over.
pub(super) const WIDEST: f64 = (1.0 + WANDER) * (1.0 + BEND_WIDENS) + 0.01;

/// The curvature, in half widths, at which a bend counts as tight: a channel
/// turning on a radius of about eight of its own half widths. Measured
/// against the orders' meanders -- a river's loops come round on radii of
/// two hundred blocks, and a great river's on eight hundred -- so a real
/// bend of either reaches it and a straight reach's wobble does not.
const BEND_TIGHT: f64 = 0.12;

/// Where the water meets a cut bank, as a share of the bank's `edge`: the
/// shelving bank's water ends a little over half way out, and a cut bank's
/// a little further, because the ground there does not come down to meet it.
const CUT_BANK_WATER: f64 = 0.6;

/// How far over the water the top of a cut bank may stand before the ground
/// behind it is eased down to that height, in blocks. The wall is the ground
/// the river cut into, so it is as tall as the floodplain is high, and this is
/// the most of it that is left as a wall: three blocks is a bank a player
/// climbs out over with a jump and a step, found by walking along it, and not
/// a gorge.
const CUT_BANK_TOP: f64 = 2.6;

/// How far the floodplain swings either side of `WorldGen::FLOODPLAIN`, in
/// blocks, beside the channel. A block and a half and more: the plain at a
/// river's edge is one to four blocks over its water rather than three
/// everywhere, which is
/// the difference between a bank you step down off and one you jump, and
/// never low enough to lie level with the water, where a floodplain would be
/// a lake.
///
/// **Only beside the channel**, fading out by a width past its edge
/// (`FLOODPLAIN_REACH`). Swung across the whole valley -- two kilometres of a
/// great river's -- it moved the height of a floodplain's every column by a
/// block, which moved where its swamps and its woods are, and a sample of six
/// forest chunks picked by the biome at their middles came back a different
/// six, half as closed (`the_floor_under_a_wood_is_darker_than_the_open_ground_beside_it`).
/// The bank is what was asked about; the country behind it stays as it was.
const FLOODPLAIN_SWING: f64 = 1.7;
/// The lowest the floodplain ever is: under this the valley has nothing to
/// pull toward, and the noise is not sampled.
pub(super) const FLOODPLAIN_LOWEST: f64 = WorldGen::FLOODPLAIN - FLOODPLAIN_SWING;
/// How far from the middle line the floodplain's swing reaches, in half
/// widths: all of it out to the first, none past the second.
const FLOODPLAIN_REACH: (f64, f64) = (1.2, 2.2);

/// One bank of one order of river at one column.
#[derive(Clone, Copy, Debug)]
pub(super) struct RiverBank {
    /// How far the column is from the channel's middle line, in blocks.
    pub distance: f64,
    /// Where the cut on this side of the line ends, in blocks: the order's
    /// half width, wandered and bent.
    pub edge: f64,
    /// 0 for a shelving bank, 1 for a cut bank.
    pub steep: f64,
    /// How hard the channel bends toward this bank: 1 on the outside of the
    /// tightest bend, -1 on the inside, 0 on a straight reach.
    ///
    /// Kept although nothing in the generator reads it after `steep` is
    /// worked out, because it is the one number that says *why* a bank is
    /// steep, and the test that the outside of a bend is the steep side
    /// (`the_outside_of_a_bend_is_steeper_than_the_inside`) has nothing else
    /// to ask.
    #[cfg_attr(not(test), expect(dead_code, reason = "read by the bank tests"))]
    pub bend: f64,
}

impl RiverBank {
    /// A column past the furthest any bank of the order reaches: no cut, and
    /// nothing sampled to say so.
    pub(super) fn far(order: &RiverOrder, distance: f64) -> Self {
        Self { distance, edge: order.half_width, steep: 0.0, bend: 0.0 }
    }
}

/// What one order of river does to one column: `WorldGen::river`'s answer.
#[derive(Clone, Copy, Debug)]
pub(super) struct RiverCut {
    /// 1 on the middle line, easing to 0 at the bank's edge.
    pub channel: f64,
    /// The same for the valley the channel lies in.
    pub valley: f64,
    /// How much of a river this country is at all: nothing up a hill or out
    /// at sea, all of one on a lowland. `channel` and `valley` carry it
    /// already; the cut bank needs it on its own. See `cut_bank`.
    pub mask: f64,
    pub bank: RiverBank,
}

impl RiverCut {
    /// No river of the order anywhere near.
    pub(super) const NONE: Self = Self {
        channel: 0.0,
        valley: 0.0,
        mask: 0.0,
        bank: RiverBank { distance: f64::INFINITY, edge: 1.0, steep: 0.0, bend: 0.0 },
    };
}

/// The curvature of a field's contour through a column, in one over blocks,
/// from the column's sample, the four `step` away along the axes (east,
/// west, south, north) and the one diagonal to the south-east.
///
/// The divergence of the unit normal, `(fxx fz² - 2 fx fz fxz + fzz fx²) /
/// |∇f|³`: positive where the contour bends away from the side the field is
/// positive on, so the outside of a bend is the side whose field has the
/// curvature's sign.
pub(super) fn curvature(field: f64, around: [f64; 5], step: f64) -> f64 {
    let [east, west, south, north, corner] = around;
    let fx = (east - west) / (2.0 * step);
    let fz = (south - north) / (2.0 * step);
    let fxx = (east - 2.0 * field + west) / (step * step);
    let fzz = (south - 2.0 * field + north) / (step * step);
    let fxz = (corner - east - south + field) / (step * step);
    let g2 = fx * fx + fz * fz;
    if g2 <= f64::EPSILON * f64::EPSILON {
        return 0.0;
    }
    (fxx * fz * fz - 2.0 * fx * fz * fxz + fzz * fx * fx) / (g2 * g2.sqrt())
}

/// The ground a cut bank leaves, blended over the shelving cut by how much of
/// a cut bank this is.
///
/// `before` is the column before this order's channel was cut into it, and
/// `gentle` after: the ramp every bank used to be.
///
/// **A wall where the water meets the ground.** Out from `CUT_BANK_WATER` of
/// the edge the ground is left as it was, up to `CUT_BANK_TOP` over the water
/// and rising a block a block behind that; in from it the bed drops to a
/// block under the water at once and to the channel's floor within a few
/// blocks. Blended by weight rather than switched, so a bank between the two
/// kinds is a wall a block or two tall rather than a choice of three or none.
///
/// The weight is taken away in three places, each of which was a way for a
/// wall to go somewhere it must not:
/// * **toward the middle line**, where the other bank's weight meets this
///   one's: the two sides must agree about the column on the line;
/// * **toward the edge**, where this order's cut stops and the country
///   carries on untouched;
/// * **on high ground and where the river fades out**, where the ground
///   behind the wall is far over the water: a brook up a hillside cut this
///   way was a trench with walls taller than a player can see over.
pub(super) fn cut_bank(order: &RiverOrder, before: f64, gentle: f64, cut: &RiverCut) -> f64 {
    let RiverBank { distance, edge, steep, .. } = cut.bank;
    let sea = f64::from(SEA_LEVEL);
    let bed = (sea - f64::from(order.depth)).min(sea - 1.0);
    let water_edge = CUT_BANK_WATER * edge;
    let beyond = distance - water_edge;
    let wall = if beyond < 0.0 {
        let deep = smoothstep(0.0, (0.3 * water_edge).max(1.5), -beyond);
        (sea - 1.0) * (1.0 - deep) + bed * deep
    } else {
        sea + CUT_BANK_TOP + beyond
    }
    .min(before);
    let weight = steep
        * cut.mask
        * smoothstep(0.0, 0.5 * water_edge, distance)
        * (1.0 - smoothstep(0.8 * edge, edge, distance))
        * smoothstep(sea + 7.0, sea + 4.5, before);
    gentle + (wall - gentle) * weight
}

impl WorldGen {
    /// The width and the kind of the bank on this side of an order's line.
    ///
    /// `field` is the order's river field at the column, whose sign is the
    /// side; `curvature` is the line's, from `curvature`.
    ///
    /// Each side reads its fields at its own offset, so the two banks of one
    /// river wander and steepen independently -- a bay on the left does not
    /// come with a matching one on the right, which is the look of a canal
    /// with a bulge in it.
    pub(super) fn river_bank(&self, order: &RiverOrder, x: f64, z: f64, field: f64, distance: f64, curvature: f64) -> RiverBank {
        let side = if field >= 0.0 { 1.0 } else { -1.0 };
        let half = order.half_width;
        // A few widths from one bay to the next, and two octaves so a long
        // narrow reach has a small bay in it.
        let wander = fbm(&self.detail_noise, x + side * 40_013.0, z - 20_011.0, 1.0 / (half * 5.0), 2);
        // Positive on the outside of a bend.
        let bend = (side * curvature * half / BEND_TIGHT).clamp(-1.0, 1.0);
        let edge = half * (1.0 + WANDER * (wander * 1.8).clamp(-1.0, 1.0)) * (1.0 + BEND_WIDENS * bend);
        // One octave, a little shorter than the wander: a cut bank is a
        // reach of wall a few widths long, not a patchwork.
        let temper = self.detail_noise.get([
            (x + side * 60_017.0) / (half * 3.0),
            (z + 30_011.0) / (half * 3.0),
        ]);
        let steep = smoothstep(-0.15, 0.25, temper + 0.35 * bend);
        RiverBank { distance, edge, steep, bend }
    }

    /// The height a river's valley pulls the country toward here, `distance`
    /// from an order's middle line. See `FLOODPLAIN_SWING`.
    pub(super) fn floodplain_at(&self, order: &RiverOrder, gx: i32, gz: i32, distance: f64) -> f64 {
        let near = 1.0 - smoothstep(FLOODPLAIN_REACH.0 * order.half_width, FLOODPLAIN_REACH.1 * order.half_width, distance);
        if near <= 0.0 {
            return WorldGen::FLOODPLAIN;
        }
        let swing = (self.detail_noise.get([f64::from(gx) / 64.0 + 71.3, f64::from(gz) / 64.0 - 43.7]) * 2.0).clamp(-1.0, 1.0);
        WorldGen::FLOODPLAIN + FLOODPLAIN_SWING * swing * near
    }

    /// What the margin of a river is made of at this column, or `None` to
    /// leave what the climate and the slope made it.
    ///
    /// `rise` is the column's height over the river's water: negative under
    /// it. Asked of the river's bed and of the dry columns with river water
    /// beside them, and of nothing else (`build_column_tile`).
    ///
    /// **One patchy field, read in bands**, so the materials come in reaches
    /// a dozen blocks long and meet each other in the order a river sorts
    /// them: gravel where the water ran, sand, then the mud and the clay of
    /// slack water, with turf coming down to the water between. The field is
    /// the deposit field at its own offset and a finer scale, because the
    /// deposit field's own patches are sixty blocks across -- a whole reach
    /// of bank the same -- and its clay and gravel still lie on the flat
    /// ground behind.
    ///
    /// Rejected: a hash per column. It is the speckle the reeds were, in
    /// stone.
    pub(super) fn river_margin(&self, gx: i32, gz: i32, rise: i32, biome: Biome) -> Option<Surface> {
        // Not on the sea's shore: a beach is the sea's, and a sea cliff with
        // gravel on its lip is a river bank a hundred blocks from any river.
        // The same test `biome_from` makes, asked only of the dry margin.
        if rise >= 0 && spline(CONTINENT_SPLINE, self.continent(gx, gz)) <= f64::from(SEA_LEVEL) + 2.5 {
            return None;
        }
        let m = (fbm(&self.deposit_noise, f64::from(gx) + 5_003.0, f64::from(gz) - 3_001.0, 1.0 / 15.0, 2) * 1.8).clamp(-1.0, 1.0);
        let lay = |top, filler, soil| Some(Surface { top, filler, soil });
        match rise {
            // The bed: gravel where the water runs, silt over sand where it
            // does not, and mud in the slack.
            ..=-3 => match m {
                m if m < -0.15 => lay(BLOCK_GRAVEL, BLOCK_GRAVEL, 2),
                m if m > 0.4 => lay(BLOCK_MUD, BLOCK_DIRT, 3),
                _ => lay(BLOCK_DIRT, BLOCK_SAND, 3),
            },
            // The shallows, where a player wades and looks down.
            -2..=-1 => match m {
                m if m < -0.45 => lay(BLOCK_GRAVEL, BLOCK_GRAVEL, 2),
                m if m < -0.05 => lay(BLOCK_SAND, BLOCK_SAND, 3),
                m if m < 0.3 => lay(BLOCK_MUD, BLOCK_DIRT, 3),
                m if m < 0.55 => lay(BLOCK_CLAY, BLOCK_CLAY, 3),
                _ => lay(BLOCK_SAND, BLOCK_SAND, 3),
            },
            // The low bank, level with the water or a block over it.
            0..=1 => match m {
                m if m < -0.5 => lay(BLOCK_GRAVEL, BLOCK_GRAVEL, 2),
                m if m < -0.2 => lay(BLOCK_SAND, BLOCK_SAND, 3),
                m if m < 0.15 => None,
                m if m < 0.45 => lay(BLOCK_MUD, BLOCK_DIRT, 3),
                _ => lay(BLOCK_CLAY, BLOCK_CLAY, 3),
            },
            // **The lip of a cut bank.** The slope rules strip a column
            // standing two blocks or more over the water beside it to bare
            // rock, which put a line of cobble along every cut bank in the
            // world. Turf holds on its roots over a fresh cut; where it has
            // fallen in the earth shows, and here and there the stones the
            // river is washing out of it. Four of soil, so the face under
            // the lip is earth the river cut through and not rock.
            _ => match m {
                // Bare earth only where the turf is green: the savanna's and
                // the desert's own ground is already the bare kind, and dirt
                // on their banks grew the meadow's dry grass on it
                // (`dry_grass_grows_on_the_savannas_dry_ground_and_nowhere_else`).
                m if m < -0.35 && biome.surface().top == BLOCK_GRASS => lay(BLOCK_DIRT, BLOCK_DIRT, 4),
                m if m > 0.55 => lay(BLOCK_COBBLESTONE, BLOCK_DIRT, 4),
                _ => {
                    let turf = biome.surface();
                    lay(turf.top, turf.filler, 4)
                }
            },
        }
    }

    /// How thick the plants of the waterline stand here: `None` for an open
    /// reach of bank with nothing growing at the edge, or the number the
    /// plant's own spacing is divided by -- one for the ordinary scatter,
    /// three in a bed.
    ///
    /// **A reed bed is a bed.** Each waterline column rolling its own plant
    /// is an even sprinkle along every bank in the world, and that is what a
    /// bank drawn by a rule looks like. A slow field decides where the beds
    /// are, a third of the waterline is open, and in the beds the plants
    /// stand three times as thick -- so there are about as many reeds as
    /// before, all of them somewhere.
    pub(super) fn waterline_stand(&self, gx: i32, gz: i32) -> Option<u32> {
        let bed = self.deposit_noise.get([f64::from(gx) / 22.0 - 811.3, f64::from(gz) / 22.0 + 4_441.7]);
        if bed < -0.12 {
            None
        } else if bed > 0.15 {
            Some(3)
        } else {
            Some(1)
        }
    }

    /// **Stones standing in a rapid**, their tops at the water or a block
    /// over it.
    ///
    /// A river runs white where its bed is broken, and the stones it broke
    /// on are what a player sees of that from the bank: a line of them is
    /// where the water is fast, and where the water is fast is where a
    /// swimmer is taken (`river_current`). A calm reach has none, so a reach
    /// with stones in it is a reach read before anyone is in it.
    ///
    /// Only in the shallows -- three blocks of water or less -- because a
    /// stone under the surface of deep water is nothing anyone sees, and only
    /// over the chunk's own columns: a stone is one column, and its column is
    /// in exactly one chunk. The die before the current, which reads the
    /// country up and down the channel.
    pub(super) fn place_rapid_stones(&self, blocks: &mut [BlockId], origin_x: i32, origin_z: i32, columns: &ColumnCache) {
        for lz in 0..CHUNK_SIZE_Z as i32 {
            for lx in 0..CHUNK_SIZE_X as i32 {
                let column = columns.at(lx, lz);
                let depth = SEA_LEVEL - column.height;
                if column.biome != Biome::River || column.water != SEA_LEVEL || !(1..=3).contains(&depth) {
                    continue;
                }
                let (gx, gz) = (origin_x + lx, origin_z + lz);
                let roll = hash2(gx, gz, self.seed.wrapping_add(0x5A91D));
                if !roll.is_multiple_of(RAPID_STONE_SPACING) {
                    continue;
                }
                let (vx, vz) = self.column_current(gx, gz);
                if vx.hypot(vz) < RAPID_SPEED * 0.8 {
                    continue;
                }
                let stone = if column.rock == BLOCK_GRANITE { BLOCK_GRANITE } else { BLOCK_COBBLESTONE };
                // Awash or a block proud of the water, one in two.
                let top = SEA_LEVEL + ((roll >> 8) & 1) as i32;
                for y in column.height + 1..=top.min(CHUNK_SIZE_Y as i32 - 1) {
                    let index = Chunk::index(lx as usize, y as usize, lz as usize);
                    if matches!(blocks[index], BLOCK_WATER | BLOCK_ICE | BLOCK_AIR) {
                        blocks[index] = stone;
                    }
                }
            }
        }
    }
}
