//! When a bolt falls and where it lands: the server's half of
//! `primitive_shared::lightning`, which holds the rules and the reasons
//! for every number here.
//!
//! ## What is here
//!
//! A countdown, the same shape as the sky's own (`logic::weather::Sky`):
//! while a storm is overhead it runs down, and when it reaches nothing a
//! bolt is due. It is **not** stepped when the sky is anything else, so
//! a world that never storms never does any of this.
//!
//! And the draw: a handful of columns near a player, their top blocks
//! found, the best one taken. Everything that decides "best" is in the
//! rules (`lightning::attraction`); what is here is the looking, which
//! is the part that costs block reads and therefore the part that has to
//! be bounded.
//!
//! ## What is not here
//!
//! What a strike *does* -- the fire, the scorched turf, the blow to
//! whoever was standing there, the message every client draws a flash
//! from. That is the tick loop's, for the reason a mechanic's block
//! changes always are: it takes locks this file does not hold and it
//! broadcasts, and a simulation that broadcasts is one that cannot be
//! tested without a socket.
//!
//! ## What is not saved
//!
//! The countdown. A storm that was half way to its next bolt when the
//! server stopped starts counting again when it comes back, which is a
//! difference of under two minutes in a thing that is random anyway --
//! and a saved file for it would be a file to version.

use primitive_shared::lightning::{
    attraction, CANDIDATES, NEAREST, REACH, SEARCH_ABOVE, SEARCH_BELOW, STRIKE_GAP_SECONDS,
};
use primitive_shared::types::{is_air, BlockId};
use primitive_shared::weather::Weather;
use primitive_shared::wildfire::fuel;

use crate::logic::falling::BlockWorld;
use crate::logic::rng::Rng;

/// A cell, in global block coordinates. The wildfire's.
pub type Cell = (i32, i32, i32);

/// The storm's clock.
pub struct Storm {
    /// Seconds until the next bolt. Meaningless while the sky is not a
    /// storm -- it is not stepped then.
    remaining: f32,
    rng: Rng,
}

impl Default for Storm {
    fn default() -> Self {
        Self::new()
    }
}

impl Storm {
    pub fn new() -> Self {
        let mut rng = Rng::from_clock();
        let first = rng.range(STRIKE_GAP_SECONDS.start, STRIKE_GAP_SECONDS.end);
        Self { remaining: first, rng }
    }

    /// The same, repeatable. For tests, and for a server whose weather is
    /// the same every run (`weather::Sky::seeded`).
    pub fn seeded(seed: u64) -> Self {
        let mut storm = Self::new();
        storm.rng = Rng::seeded(seed);
        storm.remaining = storm.rng.range(STRIKE_GAP_SECONDS.start, STRIKE_GAP_SECONDS.end);
        storm
    }

    /// Seconds until the next bolt, for `/weather` and the debug line.
    pub fn remaining(&self) -> f32 {
        self.remaining
    }

    /// One tick. `true` means a bolt is due now.
    ///
    /// **Anything but a storm holds the clock where it is** rather than
    /// resetting it: a squall that passes over twice in ten minutes is
    /// one storm as far as a player standing under it is concerned, and a
    /// clock that started again from a fresh roll every time the sky
    /// flickered would put a bolt at the start of each.
    pub fn step(&mut self, weather: Weather, dt: f32) -> bool {
        if weather != Weather::Storm || dt <= 0.0 {
            return false;
        }
        self.remaining -= dt;
        if self.remaining > 0.0 {
            return false;
        }
        self.remaining = self.rng.range(STRIKE_GAP_SECONDS.start, STRIKE_GAP_SECONDS.end);
        true
    }

    /// Which of the players online this bolt is near.
    ///
    /// One bolt near one of them rather than one each: a storm whose
    /// danger scaled with how many people were logged in would be a
    /// storm that punishes a busy server.
    pub fn pick(&mut self, players: usize) -> usize {
        if players <= 1 {
            return 0;
        }
        self.rng.below(players as u32) as usize
    }

    /// Where the next bolt goes, near this player, or `None` if there was
    /// nothing worth striking -- which is what a player deep underground
    /// or out at sea in unloaded country gets.
    ///
    /// `metal` is whether they are carrying metal (`types::tool_tier`);
    /// the caller has already asked whether the sky can see them, because
    /// "I was in a cave and lightning hit me" is the report that makes a
    /// mechanic feel arbitrary.
    ///
    /// **Bounded by construction**: `CANDIDATES` columns, each scanned
    /// over a fixed window of height. A storm therefore costs a few
    /// hundred block reads a minute and nothing between bolts. See the
    /// module note in `primitive_shared::lightning` on the charge map
    /// this is instead of.
    pub fn draw(&mut self, world: &dyn BlockWorld, feet: (f32, f32, f32), metal: bool) -> Option<Cell> {
        let (px, py, pz) = (feet.0.floor() as i32, feet.1.floor() as i32, feet.2.floor() as i32);
        let mut best: Option<(f32, Cell)> = None;
        // The player's own column first, and only when they are carrying
        // metal in the open: an empty-handed player in a field is not a
        // target, which is what keeps a storm a risk rather than a
        // punishment for being outdoors. See `lightning::METAL`.
        let consider = |score: f32, at: Cell, best: &mut Option<(f32, Cell)>| {
            if best.is_none_or(|(had, _)| score > had) {
                *best = Some((score, at));
            }
        };
        // ...and only when they are the top of their own column, which
        // is what "in the open" means and what makes this safe to ask
        // without a sky test of its own: a player in a cave or under a
        // roof has a hill or a ceiling above them, and the bolt that
        // lands on it is not theirs.
        if metal {
            if let Some(top) = top_of(world, px, pz, py) {
                if top.1 <= py {
                    consider(attraction(top.1, py, false, true), top, &mut best);
                }
            }
        }
        for _ in 0..CANDIDATES {
            // A ring rather than a disc: a bolt is a thing that happens
            // over there. The player's own column gets in only by the
            // branch above, and on its own terms.
            let (dx, dz) = self.offset();
            let (gx, gz) = (px + dx, pz + dz);
            let Some(top) = top_of(world, gx, gz, py) else {
                continue;
            };
            let burns = world
                .block(top.0, top.1, top.2)
                .is_some_and(|block| fuel(block).is_some());
            consider(attraction(top.1, py, burns, false), top, &mut best);
        }
        best.map(|(_, at)| at)
    }

    /// Where one candidate is, relative to the player: anywhere within
    /// reach that is not right on top of them.
    ///
    /// **The hole in the middle is round, and it was square.** Drawing
    /// each axis in `NEAREST..REACH` separately excludes the whole cross
    /// through the player -- so the tree eight blocks due east of them,
    /// the one thing a storm ought to hit, could not be drawn at all
    /// while a tree eight north *and* eight east could. A round hole is
    /// the rule as it was meant: a bolt falls somewhere near, but not on
    /// a player who has given it no reason to.
    ///
    /// Bounded tries rather than a loop until it lands: four rejections
    /// in a row is a chance in a thousand, and the fallback is a point on
    /// the ring, which is a perfectly good candidate.
    fn offset(&mut self) -> (i32, i32) {
        for _ in 0..4 {
            let dx = self.rng.range(-(REACH as f32), REACH as f32) as i32;
            let dz = self.rng.range(-(REACH as f32), REACH as f32) as i32;
            if dx * dx + dz * dz >= NEAREST * NEAREST {
                return (dx, dz);
            }
        }
        (REACH, 0)
    }
}

/// The top block of a column: the highest cell in the search window that
/// is not air, with its height. `None` where the chunk is not loaded or
/// the whole window is empty.
///
/// The window is a band round the player's own height (`SEARCH_ABOVE`,
/// `SEARCH_BELOW`) rather than the whole column, which is what keeps this
/// a fixed cost per candidate. A bolt therefore cannot find the roof of a
/// mountain a player is standing at the foot of -- correctly: what they
/// can see is the country they are in.
fn top_of(world: &dyn BlockWorld, gx: i32, gz: i32, feet: i32) -> Option<Cell> {
    let top = (feet + SEARCH_ABOVE).min(primitive_shared::types::CHUNK_SIZE_Y as i32 - 1);
    let bottom = (feet - SEARCH_BELOW).max(0);
    (bottom..=top)
        .rev()
        .find(|&y| world.block(gx, y, gz).is_some_and(|block: BlockId| !is_air(block)))
        .map(|y| (gx, y, gz))
}

#[cfg(test)]
mod tests {
    use super::*;
    use primitive_shared::types::{BLOCK_AIR, BLOCK_GRASS, BLOCK_LEAVES, BLOCK_LOG, BLOCK_STONE};
    use std::collections::HashMap;
    use std::sync::Mutex;

    /// A world that is a floor at y=10 and whatever is put on it.
    #[derive(Default)]
    struct Field {
        put: Mutex<HashMap<Cell, BlockId>>,
    }

    impl BlockWorld for Field {
        fn block(&self, gx: i32, gy: i32, gz: i32) -> Option<BlockId> {
            if let Some(&block) = self.put.lock().unwrap().get(&(gx, gy, gz)) {
                return Some(block);
            }
            Some(if gy <= 10 { BLOCK_GRASS } else { BLOCK_AIR })
        }
        fn set(&self, gx: i32, gy: i32, gz: i32, block: BlockId) {
            self.put.lock().unwrap().insert((gx, gy, gz), block);
        }
    }

    impl Field {
        /// A tree: a trunk with a crown over it, the shape the generator
        /// grows (`worldgen::MAX_CANOPY_RADIUS`). The crown matters to
        /// the draw -- a tree is two dozen columns, not one, which is
        /// most of why a handful of samples finds one at all.
        fn tree(&self, gx: i32, gz: i32, height: i32) {
            for y in 11..=(10 + height) {
                self.set(gx, y, gz, BLOCK_LOG);
            }
            for dx in -2i32..=2 {
                for dz in -2i32..=2 {
                    if dx.abs() == 2 && dz.abs() == 2 {
                        continue;
                    }
                    self.set(gx + dx, 10 + height, gz + dz, BLOCK_LEAVES);
                    self.set(gx + dx, 11 + height, gz + dz, BLOCK_LEAVES);
                }
            }
        }

        /// Is this column part of a tree?
        fn wooded(&self, at: Cell) -> bool {
            matches!(self.block(at.0, at.1, at.2), Some(BLOCK_LOG) | Some(BLOCK_LEAVES))
        }
    }

    #[test]
    fn nothing_is_struck_out_of_a_clear_sky() {
        let mut storm = Storm::seeded(1);
        for _ in 0..10_000 {
            assert!(!storm.step(Weather::Clear, 1.0));
            assert!(!storm.step(Weather::Rain, 1.0));
        }
        // ...and the clock it was holding is the one the storm gets.
        assert!(storm.remaining() > 0.0);
    }

    #[test]
    fn a_storm_throws_a_bolt_about_once_a_minute() {
        let mut storm = Storm::seeded(7);
        let mut bolts = 0;
        for _ in 0..(20 * 60) {
            if storm.step(Weather::Storm, 1.0) {
                bolts += 1;
            }
        }
        // Twenty minutes of storm. Near enough one a minute, and the
        // test states the two ends rather than the number: fireworks at
        // one end, a storm nobody notices at the other.
        assert!(bolts >= 8, "twenty minutes of storm and {bolts} bolts");
        assert!(bolts <= 35, "twenty minutes of storm and {bolts} bolts");
    }

    #[test]
    fn the_bolt_goes_to_the_wood_rather_than_to_the_meadow_beside_it() {
        // Half the field is trees. If the draw were a dart at the map
        // the wood would take half the bolts; because it is the tallest
        // thing under the cloud, it takes nearly all of them.
        let world = Field::default();
        for x in (-40..0).step_by(8) {
            for z in (-40..40).step_by(8) {
                world.tree(x, z, 5);
            }
        }
        let mut storm = Storm::seeded(4242);
        let mut wood = 0;
        let mut bolts = 0;
        for _ in 0..200 {
            let Some(at) = storm.draw(&world, (0.5, 11.0, 0.5), false) else {
                continue;
            };
            bolts += 1;
            if world.wooded(at) {
                wood += 1;
            }
        }
        assert!(bolts > 150, "only {bolts} of two hundred bolts landed anywhere");
        assert!(
            wood * 10 > bolts * 8,
            "the wood took {wood} of {bolts} bolts, which is what a dart at the map would do"
        );
    }

    #[test]
    fn an_empty_handed_player_in_a_field_is_not_a_target_and_one_with_an_axe_is() {
        let world = Field::default();
        let at_feet = |metal| {
            let mut storm = Storm::seeded(11);
            (0..60)
                .filter(|_| storm.draw(&world, (0.5, 11.0, 0.5), metal) == Some((0, 10, 0)))
                .count()
        };
        assert_eq!(at_feet(false), 0, "a bolt came down on an empty-handed player in a flat field");
        assert!(at_feet(true) > 50, "the metal made no difference: {} of sixty", at_feet(true));
        // ...and at the edge of a wood the trees take most of them even
        // with the axe in hand. That is the decision the mechanic asks
        // for: get under something taller, or put the axe down -- and
        // the open field with the axe is the one place both answers are
        // missing.
        for x in (-40..40).step_by(8) {
            for z in (10..40).step_by(8) {
                world.tree(x, z, 5);
            }
        }
        let mut storm = Storm::seeded(11);
        let elsewhere = (0..60)
            .filter_map(|_| storm.draw(&world, (0.5, 11.0, 0.5), true))
            .filter(|&at| at != (0, 10, 0))
            .count();
        assert!(elsewhere > 30, "the axe outdrew a whole wood: {elsewhere} of sixty went elsewhere");
    }

    #[test]
    fn a_bolt_finds_nothing_where_the_world_is_not_loaded() {
        struct Nowhere;
        impl BlockWorld for Nowhere {
            fn block(&self, _: i32, _: i32, _: i32) -> Option<BlockId> {
                None
            }
            fn set(&self, _: i32, _: i32, _: i32, _: BlockId) {}
        }
        let mut storm = Storm::seeded(3);
        assert_eq!(storm.draw(&Nowhere, (0.0, 64.0, 0.0), true), None);
    }

    #[test]
    fn a_bolt_lands_where_the_sky_can_see_and_never_inside_the_hill() {
        // Every candidate is the top of its column, so nothing under a
        // roof is ever drawn -- which is the whole of why a player in a
        // cave is safe without the caller testing anything.
        let world = Field::default();
        world.set(5, 11, 5, BLOCK_STONE);
        world.set(5, 12, 5, BLOCK_STONE);
        let mut storm = Storm::seeded(88);
        for _ in 0..50 {
            if let Some(at) = storm.draw(&world, (0.5, 11.0, 0.5), false) {
                assert!(
                    world.block(at.0, at.1 + 1, at.2) == Some(BLOCK_AIR),
                    "a bolt landed under something at {at:?}"
                );
            }
        }
    }
}
