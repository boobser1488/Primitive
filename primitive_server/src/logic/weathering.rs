//! Boards in the rain, on the server: the clock that greys and rots what
//! is left out. The rules -- what weathers, what is wet, how long a stage
//! takes and what rotten does -- are `primitive_shared::weathering`, with
//! the argument for each; this is where and how often the rain is asked.
//!
//! ## Where it looks
//!
//! **The snow's shape** (`logic::snowfall`): a pass a second, a handful of
//! random columns round each player, each walked down from above to the
//! first thing that stops the sky. If that is a board that still weathers,
//! it rolls for its next stage. Nothing is kept per board, and a dry sky
//! costs one comparison.
//!
//! ## Only near somebody, and why that is not the fish trap's debt
//!
//! A fish trap out of sight is owed its steps (`logic::fishing`), because
//! a trap is *for* working while nobody is there. Rot is the other kind of
//! clock -- the carrion's (`logic::carrion`): a cost, and a cost that is
//! only paid where somebody is looking is a favour, not a cheat. The debt
//! would also need a list of where the boards are, and the boards are
//! everything anybody ever built; a trap is one cell a player chose to set.
//! So a house rots while its owner lives in it, which is the house the
//! owner is deciding about, and a camp abandoned on the other side of the
//! world is as it was left.
//!
//! ## Two players at one house
//!
//! Each player's square is sampled on its own, so a board two players both
//! stand near is looked at twice as often. The chance each look is given is
//! divided by how many squares hold the column, so a house with a guest in
//! it does not rot at twice the rate of the same house alone.

use primitive_shared::protocol::BlockChange;
use primitive_shared::types::{blocks_the_sky, BlockId, CHUNK_SIZE_Y};
use primitive_shared::weathering::{is_wet, stage_chance, weathered, weathering, weathers};

use crate::logic::falling::BlockWorld;
use crate::logic::rng::Rng;

/// Seconds between passes.
const INTERVAL: f32 = 1.0;

/// Columns each player's neighbourhood is looked at, a pass. The snow's
/// number: a roof is looked at every couple of minutes, and a stage is a
/// matter of months, so looking more often would buy nothing but reads.
const COLUMNS_PER_PLAYER: usize = 64;

/// How far from a player boards weather, in blocks: the snow's, which is
/// already about a camp and the fields round it.
const RADIUS: i32 = 48;

/// How far above and below a player a roof is looked for.
const ABOVE: i32 = 32;
const BELOW: i32 = 32;

type Cell = (i32, i32, i32);

pub struct Weathering {
    since: f32,
    /// Seconds since anything last fell. Starts dry: a world loaded in fine
    /// weather has no wet boards to remember.
    dry_for: f32,
    rng: Rng,
}

impl Default for Weathering {
    fn default() -> Self {
        Self::new()
    }
}

impl Weathering {
    pub fn new() -> Self {
        Self { since: 0.0, dry_for: f32::INFINITY, rng: Rng::from_clock() }
    }

    /// The same, repeatable, for tests.
    pub fn seeded(seed: u64) -> Self {
        Self { since: 0.0, dry_for: f32::INFINITY, rng: Rng::seeded(seed) }
    }

    /// Notes the sky for this tick, and says whether a pass is due.
    ///
    /// Every tick, not only on a pass, so the boards start drying the tick
    /// the rain stops rather than up to a second later.
    pub fn due(&mut self, dt: f32, falling: bool) -> bool {
        self.dry_for = if falling { 0.0 } else { self.dry_for + dt };
        self.since += dt;
        if self.since < INTERVAL {
            return false;
        }
        self.since = (self.since - INTERVAL).min(INTERVAL);
        true
    }

    /// One pass: every board it finds wet under the sky rolls for a stage.
    ///
    /// `day_seconds` is the length of the world's day, because a stage is
    /// counted in days (`weathering::WET_DAYS_PER_STAGE`) and a server can
    /// set its own day.
    pub fn pass(&mut self, world: &dyn BlockWorld, around: &[Cell], day_seconds: f32) -> Vec<BlockChange> {
        let mut changed = Vec::new();
        if around.is_empty() || day_seconds.is_nan() || day_seconds <= 0.0 || !is_wet(false, self.dry_for / day_seconds) {
            return changed;
        }
        let span = (RADIUS * 2 + 1) as u32;
        // How much wet time one look at a column stands for: a column is
        // looked at `COLUMNS_PER_PLAYER` times in `span * span` a pass, so
        // on average once every this many passes.
        let look_days = INTERVAL * (span * span) as f32 / COLUMNS_PER_PLAYER as f32 / day_seconds;
        for &(px, py, pz) in around {
            for _ in 0..COLUMNS_PER_PLAYER {
                let x = px + self.rng.below(span) as i32 - RADIUS;
                let z = pz + self.rng.below(span) as i32 - RADIUS;
                let Some((at, board)) = Self::rained_on(world, x, py, z) else {
                    continue;
                };
                if !weathers(board) {
                    continue;
                }
                let watchers = around
                    .iter()
                    .filter(|&&(ox, _, oz)| (x - ox).abs() <= RADIUS && (z - oz).abs() <= RADIUS)
                    .count()
                    .max(1);
                if !self.rng.chance(stage_chance(look_days / watchers as f32)) {
                    continue;
                }
                let aged = weathered(board, weathering(board) + 1);
                world.set(at.0, at.1, at.2, aged);
                changed.push(BlockChange { global_x: at.0, global_y: at.1, global_z: at.2, block_id: aged });
            }
        }
        changed
    }

    /// The first thing in a column, walking down from above, that stops the
    /// sky -- which is the thing the rain lands on -- and where it is.
    ///
    /// `None` for a column with nothing in reach, and for one that runs into
    /// a chunk nobody has loaded: unloaded is left alone, as everywhere a
    /// pass reads.
    fn rained_on(world: &dyn BlockWorld, x: i32, from_y: i32, z: i32) -> Option<(Cell, BlockId)> {
        let top = (from_y + ABOVE).min(CHUNK_SIZE_Y as i32 - 1);
        let bottom = (from_y - BELOW).max(0);
        for y in (bottom..=top).rev() {
            let block = world.block(x, y, z)?;
            if blocks_the_sky(block) {
                return Some(((x, y, z), block));
            }
        }
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::logic::falling::tests::TestWorld;
    use primitive_shared::types::{BLOCK_GRASS, BLOCK_PEGGED_PLANKS, BLOCK_PLANKS, BLOCK_STONE};
    use primitive_shared::weathering::{ROTTEN, WET_DAYS_PER_STAGE};
    use primitive_shared::wildfire::with_soot;

    /// A short day, so a test can rain for years in a few thousand passes.
    /// A look then stands for days rather than a tenth of one, and since a
    /// look takes one stage at most (`stage_chance`) the boards here age a
    /// little slower than a real world's -- which only makes "rotten after
    /// years" harder to pass, not easier.
    const DAY: f32 = 30.0;

    /// A floor of boards round the player at y = 20, open to the sky.
    fn deck(size: i32, board: BlockId) -> TestWorld {
        let world = TestWorld::default();
        for x in -size..=size {
            for z in -size..=size {
                world.put(x, 20, z, board);
            }
        }
        world
    }

    /// Rains on `world` for this many days, one pass a second.
    fn rain(weather: &mut Weathering, world: &TestWorld, days: f32) {
        let passes = (days * DAY / INTERVAL) as usize;
        for _ in 0..passes {
            if weather.due(INTERVAL, true) {
                weather.pass(world, &[(0, 21, 0)], DAY);
            }
        }
    }

    fn stages(world: &TestWorld, size: i32, y: i32) -> Vec<u8> {
        (-size..=size)
            .flat_map(|x| (-size..=size).map(move |z| (x, z)))
            .map(|(x, z)| weathering(world.get(x, y, z)))
            .collect()
    }

    #[test]
    fn boards_open_to_the_rain_rot_through_their_stages_over_years_and_not_in_a_season() {
        let world = deck(6, BLOCK_PEGGED_PLANKS);
        let mut weather = Weathering::seeded(11);

        // A season's rain -- a quarter of a stage -- leaves most boards new
        // and none of them rotten.
        rain(&mut weather, &world, WET_DAYS_PER_STAGE / 4.0);
        let early = stages(&world, 6, 20);
        assert!(early.iter().all(|&s| s < ROTTEN), "a board rotted in a season");
        let touched = early.iter().filter(|&&s| s > 0).count();
        assert!(touched * 2 < early.len(), "{touched} of {} boards greyed in a season", early.len());

        // Twice the four stages' worth of rain rots nearly all of them, and
        // on the way the roof went through every stage rather than jumping.
        rain(&mut weather, &world, WET_DAYS_PER_STAGE * 8.0);
        let late = stages(&world, 6, 20);
        let rotten = late.iter().filter(|&&s| s == ROTTEN).count();
        assert!(rotten * 10 >= late.len() * 8, "only {rotten} of {} boards rotten after years of rain", late.len());
        assert!(late.iter().all(|&s| s <= ROTTEN));
    }

    #[test]
    fn a_board_under_a_roof_stays_sound_while_the_roof_over_it_rots() {
        let world = deck(6, BLOCK_PLANKS);
        for x in -6..=6 {
            for z in -6..=6 {
                world.put(x, 24, z, BLOCK_STONE);
            }
        }
        // ...and one board out from under the roof, beside it, on grass.
        world.put(9, 20, 9, BLOCK_PLANKS);
        world.put(9, 19, 9, BLOCK_GRASS);
        let mut weather = Weathering::seeded(12);
        rain(&mut weather, &world, WET_DAYS_PER_STAGE * 8.0);
        assert!(stages(&world, 6, 20).iter().all(|&s| s == 0), "a board under a stone roof weathered");
        assert!(weathering(world.get(9, 20, 9)) > 0, "the board out in the rain did not weather");
        assert_eq!(world.get(9, 19, 9), BLOCK_GRASS, "the rain aged the ground under a board");
    }

    #[test]
    fn a_dry_sky_ages_nothing_and_boards_dry_off_an_afternoon_after_the_rain() {
        let world = deck(6, BLOCK_PLANKS);
        let mut weather = Weathering::seeded(13);
        let passes = (WET_DAYS_PER_STAGE * 8.0 * DAY) as usize;
        for _ in 0..passes {
            if weather.due(INTERVAL, false) {
                weather.pass(&world, &[(0, 21, 0)], DAY);
            }
        }
        assert!(stages(&world, 6, 20).iter().all(|&s| s == 0), "boards weathered under a clear sky");

        // Rain stops: still wet for the afternoon, dry after it.
        weather.due(INTERVAL, true);
        let mut wet_after = 0.0;
        for _ in 0..(DAY as usize * 4) {
            weather.due(INTERVAL, false);
            if is_wet(false, weather.dry_for / DAY) {
                wet_after += INTERVAL;
            }
        }
        let expected = primitive_shared::weathering::DRIES_IN_DAYS * DAY;
        assert!((wet_after - expected).abs() <= INTERVAL, "boards stayed wet {wet_after}s, not {expected}s");
    }

    #[test]
    fn a_sooted_board_is_smoke_cured_and_a_rotten_one_goes_no_further() {
        let world = deck(3, with_soot(BLOCK_PLANKS, 1));
        world.put(0, 20, 0, weathered(BLOCK_PLANKS, ROTTEN));
        let mut weather = Weathering::seeded(14);
        rain(&mut weather, &world, WET_DAYS_PER_STAGE * 8.0);
        assert_eq!(world.get(0, 20, 0), weathered(BLOCK_PLANKS, ROTTEN), "rot went past rotten");
        assert_eq!(world.get(1, 20, 1), with_soot(BLOCK_PLANKS, 1), "a smoked board rotted");
    }

    #[test]
    fn a_guest_does_not_make_a_house_rot_faster() {
        // The same deck, rained on with one player and with two standing on
        // it: the fraction of stages taken should be the same, within what
        // chance allows, and not double.
        let taken = |players: &[Cell], seed: u64| {
            let world = deck(6, BLOCK_PLANKS);
            let mut weather = Weathering::seeded(seed);
            let passes = (WET_DAYS_PER_STAGE * DAY) as usize;
            for _ in 0..passes {
                if weather.due(INTERVAL, true) {
                    weather.pass(&world, players, DAY);
                }
            }
            stages(&world, 6, 20).iter().map(|&s| u32::from(s)).sum::<u32>()
        };
        let alone: u32 = (0..4).map(|seed| taken(&[(0, 21, 0)], 100 + seed)).sum();
        let with_guest: u32 = (0..4).map(|seed| taken(&[(0, 21, 0), (1, 21, 1)], 200 + seed)).sum();
        assert!(
            (with_guest as f32) < alone as f32 * 1.4,
            "two players at a house aged it {with_guest} stages against {alone} alone"
        );
    }
}
