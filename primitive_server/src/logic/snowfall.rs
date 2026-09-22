//! Snow that lies: a snowfall covering the ground round the players, and a
//! thaw taking it back.
//!
//! ## What this is for
//!
//! "Сделай снежную погоду." It already snowed -- the rain turns to flakes
//! wherever the season has pushed the snow line past the ground
//! (`season::falls_as_snow_with_swing`) -- and none of it ever reached the
//! ground: a winter storm over a meadow was a white screen over green grass,
//! and the morning after was the same meadow. Winter was a number on the
//! thermometer and a particle effect. Now a snowfall whitens the ground a
//! column at a time, and a thaw takes it back the same way.
//!
//! ## What it lays, and where
//!
//! **A cover, not a drift** (`BLOCK_SNOW_COVER`): a white sheet on the floor
//! of the air cell over the ground, drawn like ash -- corner to corner, hiding
//! the turf under it. Snow used to come in eighths of a block and does not any
//! more (only liquids carry a depth), and the other thing a snowfall could lay
//! is a whole block of snow, which is a metre of it: a night of that would
//! wall a camp in. A cover is walked through, and a sweep of the hand clears a
//! path.
//!
//! On a level top under open sky only: a whole one, or one lowered a
//! quarter at a time (a lip on a generated slope, a floor dug down), where
//! the cover lies on the real top (`types::coating_rests_at`). Not where a
//! plant or a stone lies (the snow would have to delete it), not on leaves
//! (a crown of white squares), not under a roof, not on water, which is the
//! frost's (`water::Frost`).
//!
//! ## What thaws
//!
//! The cover, and only the cover. The generator's snow on a peak is whole
//! blocks, where the line never moves above, and a snow block a player built
//! with is theirs.
//!
//! ## What it costs
//!
//! The frost's shape and for the frost's reasons: a pass a second, a fixed
//! handful of random columns within a radius of each player, each a walk down
//! from above until the sky is blocked. Random columns rather than a sweep, so
//! the snow creeps over a field in the first minutes of a storm rather than
//! arriving on it in one tick.

use primitive_shared::protocol::BlockChange;
use primitive_shared::season;
use primitive_shared::types::{
    block_kind, blocks_the_sky, coating_rests_at, is_cross, is_flat, is_leafy, is_liquid, BlockId, BLOCK_AIR,
    BLOCK_SNOW_COVER, CHUNK_SIZE_Y,
};

use crate::logic::falling::BlockWorld;
use crate::logic::rng::Rng;

/// Seconds between passes.
const INTERVAL: f32 = 1.0;

/// Columns each player's neighbourhood is offered a pass.
///
/// **Sixty-four**, half the frost's. The square of [`RADIUS`] is nine
/// thousand columns, so an open field is half white after about two minutes
/// of snowfall and all but covered after ten -- the snow is seen arriving
/// rather than switched on.
const COLUMNS_PER_PLAYER: usize = 64;

/// The most columns one player is offered a pass at any reach: enough for
/// twenty-four chunks of view at the near pace.
const MAX_COLUMNS_PER_PLAYER: usize = 4096;

/// How far from a player snow lies and thaws, in blocks. Less than the
/// frost's ninety-six: a cover of snow is a small thing, not seen across a
/// valley the way a frozen lake is.
const RADIUS: i32 = 48;

/// How far above and below a player a surface is looked for.
const ABOVE: i32 = 32;
const BELOW: i32 = 32;

/// Snow that thaws has to be this far on the warm side of the line, on the
/// generator's 0..1 climate scale -- the frost's margin, for the frost's
/// reason: a column on the line must not lay and melt every second.
const THAW_MARGIN: f32 = 0.01;

type Cell = (i32, i32, i32);

pub struct Snowfall {
    since: f32,
    rng: Rng,
    /// How far from a player snow lies: `RADIUS` until the server says how
    /// far its players see (`reach_view`).
    radius: i32,
}

impl Default for Snowfall {
    fn default() -> Self {
        Self::new()
    }
}

/// What a column's top is to the snow.
enum Top {
    /// Ground a cover could lie on: this cell over it is air.
    Bare(Cell),
    /// A cover already lying, in this cell.
    Covered(Cell),
}

impl Snowfall {
    pub fn new() -> Self {
        Self { since: 0.0, rng: Rng::from_clock(), radius: RADIUS }
    }

    /// Reaches as far as a player is sent chunks, at the pace the near ground
    /// is covered -- the frost's fix (`water::Frost::reach_view`) for the
    /// frost's fault: forty-eight blocks is three chunks, so a snowfall laid
    /// a white square round the player with green ground past its edge.
    pub fn reach_view(&mut self, view_chunks: i32) {
        self.radius = (view_chunks.max(1) * primitive_shared::types::CHUNK_SIZE_X as i32).max(RADIUS);
    }

    /// Columns per pass at this reach: `COLUMNS_PER_PLAYER` per `RADIUS`
    /// square of area, capped.
    fn columns_for(radius: i32) -> usize {
        let scale = (f64::from(radius) / f64::from(RADIUS)).powi(2);
        ((COLUMNS_PER_PLAYER as f64 * scale) as usize).clamp(COLUMNS_PER_PLAYER, MAX_COLUMNS_PER_PLAYER)
    }

    /// The same, repeatable, for tests.
    pub fn seeded(seed: u64) -> Self {
        Self { since: 0.0, rng: Rng::seeded(seed), radius: RADIUS }
    }

    /// Has enough time passed for another pass? See `water::Frost::due`.
    pub fn due(&mut self, dt: f32) -> bool {
        self.since += dt;
        if self.since < INTERVAL {
            return false;
        }
        self.since = (self.since - INTERVAL).min(INTERVAL);
        true
    }

    /// One pass: covers the ground if snow is falling, clears it if warm.
    ///
    /// `precipitating` is the sky's own word (rain or storm); whether that is
    /// snow is decided per column, from the column's climate, the season and
    /// the latitude -- the questions `water::Frost::pass` asks, handed in the
    /// same way and for the same reasons.
    pub fn pass(
        &mut self,
        world: &dyn BlockWorld,
        around: &[Cell],
        precipitating: bool,
        world_time: f32,
        climate: impl Fn(i32, i32, i32) -> f32,
        latitude: impl Fn(i32) -> Option<f32>,
    ) -> Vec<BlockChange> {
        let mut changed = Vec::new();
        let radius = self.radius;
        let span = (radius * 2 + 1) as u32;
        let columns = Self::columns_for(radius);
        for &(px, py, pz) in around {
            for _ in 0..columns {
                let x = px + self.rng.below(span) as i32 - radius;
                let z = pz + self.rng.below(span) as i32 - radius;
                let Some(top) = Self::top(world, x, py, z) else {
                    continue;
                };
                let swing = season::seasonal_swing(latitude(z));
                let (at, id) = match top {
                    Top::Bare(cell) => {
                        let warmth = climate(cell.0, cell.1, cell.2);
                        if !(precipitating && season::falls_as_snow_with_swing(warmth, world_time, swing)) {
                            continue;
                        }
                        (cell, BLOCK_SNOW_COVER)
                    }
                    Top::Covered(cell) => {
                        let warmth = climate(cell.0, cell.1, cell.2);
                        // Not while it is still coming down, and not while the
                        // air would still make snow of it.
                        if precipitating || season::falls_as_snow_with_swing(warmth - THAW_MARGIN, world_time, swing) {
                            continue;
                        }
                        (cell, BLOCK_AIR)
                    }
                };
                world.set(at.0, at.1, at.2, id);
                changed.push(BlockChange { global_x: at.0, global_y: at.1, global_z: at.2, block_id: id });
            }
        }
        changed
    }

    /// The top of a column as the sky sees it, walking down from above.
    ///
    /// A plant or a stone lying in a cell stops the walk and answers nothing,
    /// which is what keeps the snow off it rather than deleting it. Water
    /// answers nothing: that surface is the frost's. So does anything the sky
    /// shows through without being a floor -- a fence, a crown of leaves.
    fn top(world: &dyn BlockWorld, x: i32, from_y: i32, z: i32) -> Option<Top> {
        let top = (from_y + ABOVE).min(CHUNK_SIZE_Y as i32 - 2);
        let bottom = (from_y - BELOW).max(1);
        for y in (bottom..=top).rev() {
            // Unloaded is left alone, as it is everywhere a pass reads.
            let block: BlockId = world.block(x, y, z)?;
            if block == BLOCK_AIR {
                continue;
            }
            if block_kind(block) == BLOCK_SNOW_COVER {
                return Some(Top::Covered((x, y, z)));
            }
            if is_liquid(block) || is_cross(block) || is_flat(block) || is_leafy(block) {
                return None;
            }
            // The cell over it was air, or the walk would have stopped there.
            //
            // **A lowered top takes snow like a whole one** -- a lip on a
            // hillside, a floor dug down (`types::coating_rests_at`). The
            // cover goes in the cell over it as it always did and is drawn
            // on the lip's real top (`types::rest_drop`). Asked for a whole
            // top, every rise of a winter meadow stayed green.
            return (blocks_the_sky(block) && coating_rests_at(block).is_some() && y < top)
                .then_some(Top::Bare((x, y + 1, z)));
        }
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::logic::falling::tests::TestWorld;
    use primitive_shared::types::{BLOCK_GRASS, BLOCK_LEAVES, BLOCK_SNOW, BLOCK_STONE, BLOCK_TALL_GRASS};

    /// Cold enough to snow in any season, and warm enough to thaw in any.
    const FREEZING: f32 = 0.0;
    const WARM: f32 = 0.9;

    fn field(size: i32) -> TestWorld {
        let world = TestWorld::default();
        for x in -size..=size {
            for z in -size..=size {
                world.put(x, 20, z, BLOCK_GRASS);
            }
        }
        world
    }

    fn run(snow: &mut Snowfall, world: &TestWorld, falling: bool, warmth: f32, passes: usize) {
        for _ in 0..passes {
            snow.pass(world, &[(0, 21, 0)], falling, 0.0, |_, _, _| warmth, |_| Some(45.0));
        }
    }

    #[test]
    fn a_long_snowfall_covers_a_field_and_never_stacks_snow_on_snow() {
        let world = field(RADIUS);
        let mut snow = Snowfall::seeded(3);
        run(&mut snow, &world, true, FREEZING, 1500);
        let (mut covered, mut columns) = (0, 0);
        for x in -10..=10 {
            for z in -10..=10 {
                columns += 1;
                covered += usize::from(world.get(x, 21, z) == BLOCK_SNOW_COVER);
                assert_eq!(world.get(x, 22, z), BLOCK_AIR, "snow stacked a second cell high at {x},{z}");
                assert_eq!(world.get(x, 20, z), BLOCK_GRASS, "the snow replaced the ground at {x},{z}");
            }
        }
        assert!(covered * 10 >= columns * 9, "only {covered} of {columns} columns under snow after a long fall");
    }

    #[test]
    fn a_thaw_takes_the_snow_back_and_leaves_the_ground_it_lay_on() {
        let world = field(RADIUS);
        let mut snow = Snowfall::seeded(4);
        run(&mut snow, &world, true, FREEZING, 1500);
        run(&mut snow, &world, false, WARM, 1500);
        for x in -10..=10 {
            for z in -10..=10 {
                assert_eq!(world.get(x, 21, z), BLOCK_AIR, "snow outlived a thaw at {x},{z}");
                assert_eq!(world.get(x, 20, z), BLOCK_GRASS, "the thaw took the ground at {x},{z}");
            }
        }
    }

    #[test]
    fn snow_keeps_off_plants_crowns_roofs_and_a_warm_sky() {
        let world = field(4);
        world.put(0, 21, 0, BLOCK_TALL_GRASS);
        world.put(2, 21, 2, BLOCK_LEAVES);
        world.put(-2, 24, -2, BLOCK_STONE);
        let mut snow = Snowfall::seeded(5);
        run(&mut snow, &world, true, FREEZING, 2000);
        assert_eq!(world.get(0, 21, 0), BLOCK_TALL_GRASS, "the snow buried a tuft of grass");
        assert_eq!(world.get(2, 22, 2), BLOCK_AIR, "snow lay on a crown of leaves");
        assert_eq!(world.get(-2, 21, -2), BLOCK_AIR, "snow fell through a roof");
        assert_eq!(world.get(-2, 25, -2), BLOCK_SNOW_COVER, "the roof itself took no snow");

        let warm = field(4);
        let mut rain = Snowfall::seeded(6);
        run(&mut rain, &warm, true, WARM, 1000);
        assert_eq!(warm.get(1, 21, 1), BLOCK_AIR, "rain in warm air laid snow");
    }

    #[test]
    fn a_hillside_of_lips_is_snowed_on_to_the_last_lip_and_thawed_back_to_its_turf() {
        // A lip on every column but a few, a quarter, a half and three
        // quarters of a block of turf (`worldgen::lips`), and the earth a
        // spade leaves under one: the snow covers them as it covers a field
        // and the thaw takes it back, leaving each lip the lip it was.
        use primitive_shared::dig::{self, Side};
        let world = field(RADIUS);
        let ground_at = |x: i32, z: i32| {
            let quarters = x.rem_euclid(4) as u8;
            let lip = dig::lowered(BLOCK_GRASS, quarters);
            if z.rem_euclid(5) == 0 {
                dig::next_bite(lip, Side::PosY).expect("the sod comes off a lip")
            } else {
                lip
            }
        };
        for x in -RADIUS..=RADIUS {
            for z in -RADIUS..=RADIUS {
                world.put(x, 20, z, ground_at(x, z));
            }
        }
        let mut snow = Snowfall::seeded(8);
        run(&mut snow, &world, true, FREEZING, 1500);
        let (mut covered, mut lips) = (0, 0);
        for x in -10..=10 {
            for z in -10..=10 {
                assert_eq!(world.get(x, 20, z), ground_at(x, z), "the snow changed the ground at {x},{z}");
                if dig::is_dug(ground_at(x, z)) {
                    lips += 1;
                    covered += usize::from(world.get(x, 21, z) == BLOCK_SNOW_COVER);
                }
            }
        }
        assert!(covered * 10 >= lips * 9, "only {covered} of {lips} lips under snow after a long fall");

        run(&mut snow, &world, false, WARM, 1500);
        for x in -10..=10 {
            for z in -10..=10 {
                assert_eq!(world.get(x, 21, z), BLOCK_AIR, "snow outlived a thaw on the lip at {x},{z}");
                assert_eq!(world.get(x, 20, z), ground_at(x, z), "the thaw changed the lip at {x},{z}");
            }
        }
    }

    #[test]
    fn the_generators_whole_blocks_of_snow_never_thaw() {
        let world = TestWorld::default();
        world.put(0, 20, 0, BLOCK_STONE);
        world.put(0, 21, 0, BLOCK_SNOW);
        let mut snow = Snowfall::seeded(7);
        run(&mut snow, &world, false, WARM, 3000);
        assert_eq!(world.get(0, 21, 0), BLOCK_SNOW, "a whole block of snow melted");
    }
}
