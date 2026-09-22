//! Where a player actually lives, as opposed to where they have been.
//!
//! Rats do not come to the middle of a wood. They come to the place with
//! the chests in it, and they come there because somebody keeps walking
//! past. So something has to be able to answer "is this a lived-in
//! place?" about an arbitrary cell of the world, cheaply, every few
//! seconds, for ever.
//!
//! ## Why a decayed map and not a history
//!
//! The first shape anybody reaches for is a list of where the player has
//! been. It is wrong twice over:
//!
//! * **It grows without bound.** A player walking for an hour is an hour
//!   of positions, in the save, on every autosave, for the life of the
//!   world -- and the interesting part of it is three cells wide.
//! * **It answers the wrong question.** A history says *where you were at
//!   four o'clock*. What vermin want to know is *where you keep coming
//!   back to*, and getting that out of a history means scanning the whole
//!   of it and weighting it by age -- which is this, computed the
//!   expensive way, every time it is asked.
//!
//! So: a small map of coarse cells to a number, the number goes up while
//! somebody stands in the cell and falls off exponentially the rest of
//! the time. A camp that is used every night stays hot. A camp that is
//! abandoned cools to nothing in a few days and its entry is dropped --
//! which is exactly the behaviour wanted, because **rats leave when you
//! do**, and a player who moves house should not find the old one still
//! infested.
//!
//! Rejected, and written down because it is the cheaper-looking option:
//! marking the *blocks* instead -- a chest is a lived-in place, a bed is
//! a lived-in place. That makes an untouched storeroom at the far end of
//! a tunnel as attractive as the kitchen, and worse, it makes a chest
//! placed in a wood summon rats to a wood. It is the walking that says
//! somebody lives here, and a map of walking is what this is.
//!
//! ## What it costs
//!
//! One `HashMap` of at most [`MAX_CELLS`] entries per world -- a few
//! hundred bytes -- one hash lookup per player per tick to warm a cell,
//! and a sweep of the whole map on [`Haunts::decay`], which is called on
//! the same slow clock everything else in the server's minute is on. The
//! cap is enforced by dropping the coldest entry, so a player who walks
//! across a continent leaves the map exactly as large as a player who
//! never leaves their hut.

use std::collections::HashMap;

/// How wide a cell is, in blocks.
///
/// Eight, which is a hut. Smaller and a player pottering about their own
/// kitchen spreads their warmth over nine cells and none of them gets hot;
/// larger and "where you live" starts to include the field outside, so
/// rats would appear in the open where any light at all drives them off
/// and nothing would ever come of it.
pub const CELL: i32 = 8;

/// The most cells one world remembers.
///
/// A player has one home, or two, or a home and a mine. Two hundred
/// eight-block cells is a good deal more than that and still nothing at
/// all to sweep. When it is full the coldest entry goes, which is always
/// somewhere walked through once.
pub const MAX_CELLS: usize = 200;

/// How long a cell takes to lose half its warmth with nobody in it, in
/// seconds.
///
/// Twenty minutes: two full days at the default day length. **Long enough
/// that a night out hunting does not cost you your infestation** -- which
/// matters, because a player who leaves at dusk and comes back at dawn is
/// exactly the player this is supposed to catch -- and short enough that
/// the camp you abandoned a week ago is not still a nest.
pub const HALF_LIFE_SECONDS: f32 = 20.0 * 60.0;

/// How much warmth one second of standing in a cell is worth.
///
/// Scaled so that [`LIVED_IN`] is reached after a few minutes of actually
/// being somewhere, and so that a player who merely walks through -- a
/// second or two in the cell -- leaves almost nothing behind.
pub const PER_SECOND: f32 = 1.0 / 60.0;

/// The warmth at which somewhere counts as lived in.
///
/// Three: three minutes of standing about, or several nights of walking
/// through. A first camp reaches it during the first evening spent
/// building it, which is the intent -- **the rats arrive on the night the
/// hut becomes a home**, not on the night it is finished.
pub const LIVED_IN: f32 = 3.0;

/// Below this a cell is dropped from the map outright.
///
/// **It has to be well under what one decay interval of standing still
/// adds, and the first cut was not.** That version dropped
/// anything under a tenth of [`LIVED_IN`] -- 0.3 -- and the server ages
/// the map every ten seconds, which is 0.167 of warmth. So a cell was
/// dropped every time it was aged, a player could stand in one room for
/// ten minutes and the map stayed empty, and nowhere in any world was
/// ever lived in. A twentieth of a point survives a single ten-second
/// interval of standing and does not survive a walk-through, which is
/// exactly the line this is for.
pub const COLD: f32 = 0.05;

/// The most warmth a cell may hold.
///
/// Without it a player who lives in one room for a week has a cell worth
/// ten thousand, and then leaving for a month would not cool it: the decay
/// is proportional, so an unbounded top makes an unbounded memory. Twelve
/// is four times [`LIVED_IN`], so a long-lived home is meaningfully warmer
/// than a camp and still forgets inside a week.
pub const MAX_HEAT: f32 = 12.0;

/// Which cell a position is in.
pub fn cell_of(x: f32, z: f32) -> (i32, i32) {
    (
        (x.floor() as i32).div_euclid(CELL),
        (z.floor() as i32).div_euclid(CELL),
    )
}

/// The middle of a cell, in blocks. What a spawner aims at.
pub fn cell_centre(cell: (i32, i32)) -> (f32, f32) {
    (
        (cell.0 * CELL) as f32 + CELL as f32 / 2.0,
        (cell.1 * CELL) as f32 + CELL as f32 / 2.0,
    )
}

/// The world's memory of where its players spend their time.
///
/// Saved with the world (`haunts.bin`, the server's `logic::vermin`),
/// because an infestation that reset every time the server restarted
/// would be a mechanic nobody could plan around -- and the whole point of
/// it is that it is a consequence of how you have been living. For a
/// whole release this line said so and nothing wrote it: after every
/// restart the house was a stranger's for the minutes it took to warm
/// up again, and rats came back late for no reason a player could see.
#[derive(Debug, Clone, Default, serde::Serialize, serde::Deserialize)]
pub struct Haunts {
    cells: HashMap<(i32, i32), f32>,
}

impl Haunts {
    pub fn new() -> Haunts {
        Haunts::default()
    }

    /// Somebody spent `seconds` at this position.
    ///
    /// **The cell they are in and nothing else.** An earlier cut warmed
    /// the neighbours too, on the grounds that a hut straddles a cell
    /// boundary, and what it produced was a nine-cell smear in which the
    /// warmest cell was wherever the player happened to have paused --
    /// so the rats came out of the wall of the field rather than out of
    /// the pantry. One cell is coarse enough already; that is what
    /// [`CELL`] is for.
    pub fn visit(&mut self, x: f32, z: f32, seconds: f32) {
        if seconds <= 0.0 {
            return;
        }
        let cell = cell_of(x, z);
        let added = seconds * PER_SECOND;
        match self.cells.get_mut(&cell) {
            Some(heat) => *heat = (*heat + added).min(MAX_HEAT),
            None => {
                self.make_room();
                self.cells.insert(cell, added.min(MAX_HEAT));
            }
        }
    }

    /// Time passes and everywhere cools.
    ///
    /// Exponential, off [`HALF_LIFE_SECONDS`], and cells that have fallen
    /// below [`COLD`] are dropped outright -- a number that small will
    /// never come back to matter, and leaving it in is how a map of two
    /// hundred entries becomes a map of two hundred entries that are all
    /// noise.
    pub fn decay(&mut self, seconds: f32) {
        if seconds <= 0.0 {
            return;
        }
        let factor = 0.5f32.powf(seconds / HALF_LIFE_SECONDS);
        self.cells.retain(|_, heat| {
            *heat *= factor;
            *heat > COLD
        });
    }

    /// How lived-in this position is.
    pub fn heat_at(&self, x: f32, z: f32) -> f32 {
        self.heat_in(cell_of(x, z))
    }

    /// The same, for a cell already worked out.
    pub fn heat_in(&self, cell: (i32, i32)) -> f32 {
        self.cells.get(&cell).copied().unwrap_or(0.0)
    }

    /// Does somebody live here?
    pub fn is_lived_in(&self, x: f32, z: f32) -> bool {
        self.heat_at(x, z) >= LIVED_IN
    }

    /// Every lived-in cell, warmest first.
    ///
    /// What the spawner walks: it wants the kitchen before the corridor,
    /// and it wants to skip the ninety cells that are somebody's walk to
    /// the river.
    pub fn lived_in(&self) -> Vec<((i32, i32), f32)> {
        let mut hot: Vec<_> = self
            .cells
            .iter()
            .filter(|(_, &heat)| heat >= LIVED_IN)
            .map(|(&cell, &heat)| (cell, heat))
            .collect();
        // By heat, and by cell when two are equal -- a `HashMap`'s order is
        // not the same twice, and a spawner that picked differently on two
        // servers running the same world would be a desync nobody could
        // reproduce.
        hot.sort_by(|a, b| b.1.total_cmp(&a.1).then(a.0.cmp(&b.0)));
        hot
    }

    /// Every remembered cell, in cell order: what the save writes. Sorted
    /// so a save of an unchanged world is the same bytes twice.
    pub fn cells(&self) -> Vec<((i32, i32), f32)> {
        let mut all: Vec<_> = self.cells.iter().map(|(&cell, &heat)| (cell, heat)).collect();
        all.sort_by_key(|&(cell, _)| cell);
        all
    }

    /// A map rebuilt from [`Haunts::cells`], on the map's own terms: heat
    /// clamped to [`MAX_HEAT`], anything at or under [`COLD`] (or not a
    /// number at all) dropped, and no more than [`MAX_CELLS`] kept. A file
    /// is not trusted to have been written by this version of the rules.
    pub fn from_cells(cells: impl IntoIterator<Item = ((i32, i32), f32)>) -> Haunts {
        let mut haunts = Haunts::new();
        for (cell, heat) in cells {
            if heat.is_nan() || heat <= COLD {
                continue;
            }
            haunts.make_room();
            haunts.cells.insert(cell, heat.min(MAX_HEAT));
        }
        haunts
    }

    /// How many cells are remembered. For the debug panel and the tests.
    pub fn len(&self) -> usize {
        self.cells.len()
    }

    pub fn is_empty(&self) -> bool {
        self.cells.is_empty()
    }

    /// Drops the coldest cell if there is no room for a new one.
    fn make_room(&mut self) {
        if self.cells.len() < MAX_CELLS {
            return;
        }
        let coldest = self
            .cells
            .iter()
            .min_by(|a, b| a.1.total_cmp(b.1).then(a.0.cmp(b.0)))
            .map(|(&cell, _)| cell);
        if let Some(cell) = coldest {
            self.cells.remove(&cell);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Standing still for this long, in seconds.
    fn stand(haunts: &mut Haunts, at: (f32, f32), minutes: f32) {
        haunts.visit(at.0, at.1, minutes * 60.0);
    }

    #[test]
    fn a_place_somebody_keeps_coming_back_to_becomes_lived_in() {
        let mut haunts = Haunts::new();
        assert!(!haunts.is_lived_in(0.0, 0.0), "an empty map has a home in it");
        stand(&mut haunts, (2.0, 2.0), 4.0);
        assert!(haunts.is_lived_in(2.0, 2.0), "four minutes in one room is not living there");
        // ...and the rest of the world is not.
        assert!(!haunts.is_lived_in(200.0, 200.0));
    }

    #[test]
    fn walking_through_somewhere_does_not_make_it_a_home() {
        let mut haunts = Haunts::new();
        // A hundred cells, two seconds each: an hour's walk in a straight
        // line.
        for step in 0..100 {
            haunts.visit((step * CELL) as f32 + 1.0, 0.0, 2.0);
        }
        assert!(
            haunts.lived_in().is_empty(),
            "a walk left {} homes behind it",
            haunts.lived_in().len()
        );
    }

    #[test]
    fn a_camp_left_behind_is_forgotten_and_a_camp_slept_in_is_not() {
        let mut haunts = Haunts::new();
        stand(&mut haunts, (2.0, 2.0), 10.0);
        assert!(haunts.is_lived_in(2.0, 2.0));
        // A night out: gone from dusk to dawn, which is the case this
        // must *not* forget.
        haunts.decay(10.0 * 60.0);
        assert!(
            haunts.is_lived_in(2.0, 2.0),
            "a night's hunting cost the player their home"
        );
        // A week away, and it is somebody else's wood again.
        haunts.decay(7.0 * 24.0 * 60.0 * 60.0);
        assert!(!haunts.is_lived_in(2.0, 2.0), "the abandoned camp is still a home");
        assert!(haunts.is_empty(), "the cold cell is still taking up room");
    }

    #[test]
    fn the_map_never_grows_past_its_cap_however_far_a_player_walks() {
        let mut haunts = Haunts::new();
        for step in 0..MAX_CELLS as i32 * 10 {
            haunts.visit((step * CELL) as f32, (step * CELL) as f32, 1.0);
            assert!(
                haunts.len() <= MAX_CELLS,
                "the map reached {} cells",
                haunts.len()
            );
        }
        assert_eq!(haunts.len(), MAX_CELLS);
    }

    #[test]
    fn the_home_survives_a_long_walk_that_fills_the_map() {
        let mut haunts = Haunts::new();
        stand(&mut haunts, (2.0, 2.0), 20.0);
        // The cap is reached by walking, and what is dropped is always
        // the coldest -- so the one warm cell is the last thing to go.
        for step in 1..MAX_CELLS as i32 * 4 {
            haunts.visit((step * CELL) as f32, 0.0, 1.0);
        }
        assert!(
            haunts.is_lived_in(2.0, 2.0),
            "a long walk evicted the player's own house"
        );
    }

    #[test]
    fn the_warmest_cell_is_the_one_most_lived_in_and_the_order_is_the_same_twice() {
        let mut haunts = Haunts::new();
        stand(&mut haunts, (2.0, 2.0), 4.0);
        stand(&mut haunts, (2.0 + CELL as f32, 2.0), 12.0);
        let hot = haunts.lived_in();
        assert_eq!(hot.len(), 2);
        assert_eq!(hot[0].0, cell_of(2.0 + CELL as f32, 2.0), "the corridor beat the kitchen");
        assert_eq!(haunts.lived_in(), hot, "two readings of one map disagreed");
    }

    #[test]
    fn a_cell_is_the_same_cell_on_both_sides_of_the_origin() {
        // `div_euclid` and not `/`: integer division rounds toward zero,
        // so -1 and 1 would share a cell and a house built west of the
        // spawn would be half in the cell east of it.
        assert_ne!(cell_of(-1.0, 0.0), cell_of(1.0, 0.0));
        assert_eq!(cell_of(-1.0, 0.0), cell_of(-CELL as f32, 0.0));
        let cell = cell_of(-20.0, -20.0);
        let (x, z) = cell_centre(cell);
        assert_eq!(cell_of(x, z), cell, "a cell's middle is in another cell");
    }

    #[test]
    fn the_memory_of_a_world_survives_being_written_down() {
        let mut haunts = Haunts::new();
        stand(&mut haunts, (2.0, 2.0), 10.0);
        let bytes = bincode::serialize(&haunts).expect("a map that will not save");
        let back: Haunts = bincode::deserialize(&bytes).expect("a map that will not load");
        assert!(back.is_lived_in(2.0, 2.0));
        assert_eq!(back.len(), haunts.len());
    }
}
