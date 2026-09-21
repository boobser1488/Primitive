//! Underground lakes and flooded passages: the water that stands in caves.
//!
//! ## What they are for
//!
//! **Clean water where there is no river.** A pond on the surface is a
//! gamble -- two mouthfuls in five make a player ill (`body::Water`,
//! `body::WATER_ILLNESS_CHANCE`) -- and the only water that never is runs
//! in a river. Water that has come down through the rock and stands in the
//! dark is what a well taps: it is drunk as `Water::Fresh`
//! (`WorldGen::cave_water_at`, and `water_kind` on the server's drinking
//! path). So a player camped far from a river in dry country has a choice
//! that was not there before: drink the pond by the camp and roll the dice,
//! boil it on a hearth for the fuel and the wait that costs, or go down with
//! a light and a jug to the lake a cave leads to. The lake is
//! far, dark and under a roof of dripstone (`dripstone::SPIKE_FROM_BLOCKS`);
//! the pond is right here. Neither answer is always right, which is what
//! makes it a decision.
//!
//! The flooded passages -- siphons, a tunnel that dips under the water to
//! its roof and comes up again on the other side -- are the same water seen
//! from a tunnel. Swimming one is a held breath in the dark (`body` air); the
//! other answer is to dig round, which is slow, or to cut the lake a channel
//! lower down and let it go, which empties the clean water the lake was for.
//!
//! Three other uses were weighed and put down:
//!
//! * *Ore or flint laid under the water, to be dived for.* In a world that
//!   can be dug, nothing is out of reach behind a lake: a pick goes round
//!   it in a minute. A reward that is really a detour is a chore with a
//!   swim in it.
//! * *Stream tin on a cave lake's bed*, the placer the surface rivers
//!   already carry (`types::BLOCK_STREAM_TIN`). A second place to find the
//!   scarcest metal weakens the reason tin country is worth travelling to,
//!   and a pebble under water needs a waterlogged form nothing else here
//!   has.
//! * *Fish.* Water that light never reaches holds almost nothing, and a
//!   cave pool that bit at the river's rate would be the best fishing in the
//!   world. So the fishing path keeps calling the pool standing water (see
//!   the server's `water_kind`): only drinking is told it is clean.
//!
//! ## Why a pool is filled rather than drawn at a level
//!
//! **Water here is conserved and flows** (`primitive_server::logic::water`).
//! Generated water is at rest until something near it changes, and then a
//! cell with air beside it at its own level runs out -- so a lake whose edge
//! stood as a wall against an open passage would empty the moment a torch
//! went up beside it, and one that crossed a chunk seam with a different
//! answer on each side would leave a wall of water standing in the seam.
//!
//! Three ways of deciding which cave cells are water were weighed:
//!
//! * *Everything carved below a water table.* One number, no search -- and
//!   wrong at every place the table changes: two regions with different
//!   tables meet in a tunnel as a standing wall, and a single table for the
//!   whole world floods every deep cave there is.
//! * *A cell is water when its own neighbours hold it.* Local, and it is not
//!   a rule: whether a cell is held depends on whether the cell beside it is,
//!   and that is a flood fill whether it is written as one or not.
//! * **A bounded fill from a seed, remembered per site (chosen).** A grid
//!   cell of `CELL` blocks offers one seed in the rock; the fill starts on
//!   the cave floor under it and raises the water one level at a time,
//!   taking in every carved cell at or below the level that the water
//!   already touches. The level stops rising the moment the pool at the
//!   next level would touch open ground, reach past `REACH`, or grow past
//!   `MAX_CELLS` -- the spill point, found the way water finds it. Every
//!   four-neighbour and the cell under every cell of the pool is then either
//!   the pool or rock, **which is the closure, by construction**. The answer
//!   is a pure function of the seed and the site, so every chunk the pool
//!   reaches into asks the same question and writes its own share of the
//!   same answer, in any order, on any thread.
//!
//! What is "carved" here is exactly what `fill_column` carves: the column's
//! own height and seal (`Column::seal`) and `is_cave`, read through the
//! column tiles. The one thing the fill is stricter about is the surface:
//! anything within `SKIN` of the ground counts as open, so a pool never
//! comes up under a hillside's soil, a ruin's floor or a river's bed.

use std::rc::Rc;

use super::{Column, TileStore, WorldGen, BEDROCK_TOP, SEA_LEVEL};
use crate::types::{Chunk, BLOCK_AIR, BLOCK_LIMESTONE, BLOCK_WATER, CHUNK_SIZE_X, CHUNK_SIZE_Z};

/// Side of the grid a pool's seed is dropped on, in blocks: three chunks.
///
/// About one site in a hundred and fifty metres of walking, most of which
/// find no cave under them or a cave that will not hold water. Wider would
/// make the lake a rumour; narrower would put water in every cave, and a
/// cave with a lake in it is supposed to be the one you remember.
const CELL: i32 = 48;

/// How far a pool may reach from its seed, either way along x or z.
///
/// Also what bounds the chunks that have to ask about a site
/// (`flood_caves`). Past this the water is taken to have somewhere to go
/// and the level stops rising, which is the honest reading of a tunnel
/// that runs on out of sight below the waterline.
const REACH: i32 = 24;

/// The most cells one pool may hold before the level under it is called
/// the spill point. A chamber and a few tunnel lengths, a few thousand
/// cubic metres; a room bigger than this is not a lake but a flooded
/// underground, which this is not trying to make.
const MAX_CELLS: usize = 3_000;

/// How far over the lowest floor the water may stand. Deep enough that a
/// tunnel which dips further than it is tall drowns to its roof -- which is
/// what a siphon is -- and shallow enough that a pool is a lake in a cave
/// rather than a cave that is a lake.
const MAX_RISE: i32 = 16;

/// No water stands higher than this. Under `dripstone`'s and the cave
/// scatter's ceiling (`SEA_LEVEL - 4`) with room to spare, so the shallow
/// "caves" that are really hollows in a hill never hold a lake.
pub(super) const MAX_LEVEL: i32 = SEA_LEVEL - 10;

/// Cells within this many of the ground count as open, not rock. See the
/// module note: it is what keeps a pool from surfacing under soil, or
/// against anything a surface pass digs.
pub(super) const SKIN: i32 = 6;

/// The fewest cells a pool must hold to be written at all -- a puddle in a
/// crack is not a find.
const MIN_CELLS: usize = 8;

/// A sky this rainy counts as wet. The same line `grow_dripstone` draws, for
/// the same reason: the wetter half of the land.
const WET_SKY: f64 = 0.15;

/// One pool, found and closed. Cells sorted, so a point query is a search.
pub(super) struct CavePool {
    /// The surface of the water: no cell of the pool is above it.
    pub(super) level: i32,
    /// Every water cell, as planet (x, y, z), sorted.
    pub(super) cells: Vec<(i32, i32, i32)>,
    /// Bounds of `cells` on x and z, inclusive, so a chunk that does not
    /// overlap the pool costs a compare.
    min: (i32, i32),
    max: (i32, i32),
}

thread_local! {
    // Pool judgements, keyed by site cell. `Rc` because a pool is a few
    // thousand cells and the store hands out a borrow it cannot outlive.
    static CAVE_POOLS: std::cell::RefCell<TileStore<Option<Rc<CavePool>>>> =
        std::cell::RefCell::new(TileStore::new(CAVE_POOLS_KEPT));
}

/// How many sites one thread remembers: a band of the world a dozen sites
/// wide, which is several times the working set of a walk.
const CAVE_POOLS_KEPT: usize = 256;

/// What a cell is, as far as holding water goes.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Class {
    Rock,
    Cave,
    /// Open ground, or cave out of reach: water that touches it leaves.
    Open,
}

impl WorldGen {
    /// Fills the chunk's share of every cave pool that reaches into it.
    ///
    /// Into air only: the pools are exactly the carved cells, and nothing
    /// before this pass writes that deep, so a cell that is not air here is
    /// one a surface pass took -- left as it is, which is rock as far as the
    /// water is concerned and keeps the closure.
    pub(super) fn flood_caves(
        &self,
        blocks: &mut [crate::types::BlockId],
        origin_x: i32,
        origin_z: i32,
    ) {
        let (end_x, end_z) = (
            origin_x + CHUNK_SIZE_X as i32 - 1,
            origin_z + CHUNK_SIZE_Z as i32 - 1,
        );
        // A seed lies in its own cell and a pool within `REACH` of it, so
        // these are exactly the cells whose pools can overlap the chunk.
        for cell_z in (origin_z - REACH).div_euclid(CELL)..=(end_z + REACH).div_euclid(CELL) {
            for cell_x in (origin_x - REACH).div_euclid(CELL)..=(end_x + REACH).div_euclid(CELL) {
                let Some(pool) = self.cave_pool(cell_x, cell_z) else {
                    continue;
                };
                if pool.max.0 < origin_x
                    || pool.min.0 > end_x
                    || pool.max.1 < origin_z
                    || pool.min.1 > end_z
                {
                    continue;
                }
                for &(x, y, z) in &pool.cells {
                    if x < origin_x || x > end_x || z < origin_z || z > end_z {
                        continue;
                    }
                    let index =
                        Chunk::index((x - origin_x) as usize, y as usize, (z - origin_z) as usize);
                    if blocks[index] == BLOCK_AIR {
                        blocks[index] = BLOCK_WATER;
                    }
                }
            }
        }
    }

    /// Whether the generator put cave water in this cell of *this world*.
    ///
    /// What the server asks when a player drinks deep underground; see the
    /// module note for why that water is clean. A question about where the
    /// world laid a lake, not about what is in the cell now: a pool drained
    /// and poured full again from a jug is still the same hollow in the
    /// rock, and that is close enough to be the rule.
    pub fn cave_water_at(&self, gx: i32, gy: i32, gz: i32) -> bool {
        if self.preset == super::Preset::Test || gy > MAX_LEVEL || gy <= BEDROCK_TOP {
            return false;
        }
        let (px, pz) = self.on_planet(gx, gz);
        let (lo_x, hi_x) = ((px - REACH).div_euclid(CELL), (px + REACH).div_euclid(CELL));
        let (lo_z, hi_z) = ((pz - REACH).div_euclid(CELL), (pz + REACH).div_euclid(CELL));
        (lo_z..=hi_z).any(|cz| {
            (lo_x..=hi_x).any(|cx| {
                self.cave_pool(cx, cz)
                    .is_some_and(|pool| gy <= pool.level && pool.cells.binary_search(&(px, gy, pz)).is_ok())
            })
        })
    }

    /// The pool at a site, remembered per thread. See `CAVE_POOLS`.
    pub(super) fn cave_pool(&self, cell_x: i32, cell_z: i32) -> Option<Rc<CavePool>> {
        let key = (self.key(), cell_x, cell_z);
        let known = CAVE_POOLS.with(|store| store.borrow().tiles.get(&key).cloned());
        if let Some(pool) = known {
            return pool;
        }
        // Built outside the borrow, for `lake_site`'s reason: the fill reads
        // column tiles, and a builder under the borrow is one refactor from
        // re-entering it.
        let pool = self.find_cave_pool(cell_x, cell_z).map(Rc::new);
        CAVE_POOLS.with(|store| store.borrow_mut().get_or_insert(key, || pool).clone())
    }

    /// The uncached half of `cave_pool`: the roll, the seed and the fill.
    fn find_cave_pool(&self, cell_x: i32, cell_z: i32) -> Option<CavePool> {
        let roll = super::hash2(cell_x, cell_z, self.seed ^ 0xCA7E_1A4E);
        let seed_x = cell_x * CELL + (roll % CELL as u32) as i32;
        let seed_z = cell_z * CELL + ((roll >> 8) % CELL as u32) as i32;
        let column = self.column_anywhere(seed_x, seed_z);
        // **Wet country floods its caves more often**, on the rule
        // `grow_dripstone` reads: limestone is the rock water dissolves its
        // way through, and water standing over the rock or rain falling on
        // it is water coming down. Two sites in three there, one in four
        // anywhere else. The humidity field is only read when the rock and
        // the column have not already answered.
        let offered = (roll >> 16) % 12;
        let wet = column.rock == BLOCK_LIMESTONE
            || column.water > column.height
            || ((3..8).contains(&offered) && self.humidity(seed_x, seed_z) > WET_SKY);
        if offered >= if wet { 8 } else { 3 } {
            return None;
        }
        // A cave cell somewhere under the seed: down from a height rolled
        // between the floor of the world and the highest a pool may stand.
        let top = MAX_LEVEL.min(column.height - SKIN);
        if top <= BEDROCK_TOP + 2 {
            return None;
        }
        let start = BEDROCK_TOP + 2 + ((roll >> 20) % (top - BEDROCK_TOP - 1) as u32) as i32;
        let mut fill = Fill::new(self, seed_x, seed_z);
        let mut y = start;
        while y > BEDROCK_TOP && fill.class(seed_x, y, seed_z) != Class::Cave {
            y -= 1;
        }
        if y <= BEDROCK_TOP {
            return None;
        }
        // ...and down to the floor of that cave, where water would gather.
        while fill.class(seed_x, y - 1, seed_z) == Class::Cave {
            y -= 1;
        }
        fill.run((seed_x, y, seed_z))
    }
}

/// One fill in progress. See the module note.
struct Fill<'a> {
    gen: &'a WorldGen,
    seed_x: i32,
    seed_z: i32,
    /// Height and seal of every column the fill has looked at: a tile
    /// lookup is a hash and a borrow, and the fill asks the same column for
    /// every cell of it.
    columns: std::collections::HashMap<(i32, i32), (i32, (i32, i32))>,
    /// What each cell the fill has looked at is, and so whether it has.
    seen: std::collections::HashMap<(i32, i32, i32), Class>,
}

impl<'a> Fill<'a> {
    fn new(gen: &'a WorldGen, seed_x: i32, seed_z: i32) -> Self {
        Self {
            gen,
            seed_x,
            seed_z,
            columns: std::collections::HashMap::new(),
            seen: std::collections::HashMap::new(),
        }
    }

    /// Carved exactly as `fill_column` carves, and open within `SKIN` of the
    /// ground or past `REACH` of the seed.
    fn class(&mut self, x: i32, y: i32, z: i32) -> Class {
        if y <= BEDROCK_TOP {
            return Class::Rock;
        }
        let gen = self.gen;
        let (height, (seal_floor, seal_roof)) = *self.columns.entry((x, z)).or_insert_with(|| {
            let Column { height, seal, .. } = gen.column_anywhere(x, z);
            (height, seal)
        });
        if y > height - SKIN {
            return Class::Open;
        }
        if (seal_floor..=seal_roof).contains(&y) || !gen.is_cave(x, y, z) {
            return Class::Rock;
        }
        if (x - self.seed_x).abs() > REACH || (z - self.seed_z).abs() > REACH {
            // Cave past the reach: the water has somewhere to go.
            return Class::Open;
        }
        Class::Cave
    }

    /// Raises the water from `floor` until the next level would spill, and
    /// answers with the pool at the last level that held.
    fn run(mut self, floor: (i32, i32, i32)) -> Option<CavePool> {
        let mut level = floor.1;
        if level > MAX_LEVEL {
            return None;
        }
        // Every cell of the pool, in the order it went in; the pool at a
        // level is a prefix of this.
        let mut pool: Vec<(i32, i32, i32)> = Vec::new();
        let mut queue = vec![floor];
        self.seen.insert(floor, Class::Cave);
        // Cave cells one over the level, touching the pool: they go in when
        // it rises. And whether open ground touches the pool there, which
        // stops it rising at all.
        let mut above: Vec<(i32, i32, i32)> = Vec::new();
        let mut spills_above = false;
        let mut held: Option<(i32, usize)> = None;
        loop {
            while let Some(cell) = queue.pop() {
                pool.push(cell);
                if pool.len() > MAX_CELLS {
                    return self.finish(pool, held);
                }
                let (x, y, z) = cell;
                for n in [
                    (x + 1, y, z),
                    (x - 1, y, z),
                    (x, y, z + 1),
                    (x, y, z - 1),
                    (x, y - 1, z),
                    (x, y + 1, z),
                ] {
                    if self.seen.contains_key(&n) {
                        continue;
                    }
                    let class = self.class(n.0, n.1, n.2);
                    self.seen.insert(n, class);
                    match class {
                        Class::Rock => {}
                        Class::Open if n.1 <= level => return self.finish(pool, held),
                        Class::Open => spills_above = true,
                        Class::Cave if n.1 <= level => queue.push(n),
                        Class::Cave => above.push(n),
                    }
                }
            }
            // Nothing at this level touches open ground: it holds.
            held = Some((level, pool.len()));
            if spills_above || level + 1 > MAX_LEVEL || level + 1 > floor.1 + MAX_RISE {
                return self.finish(pool, held);
            }
            level += 1;
            // Every cell in `above` is at the new level: a pool cell is at or
            // under the old one, so what touches it from over the level is
            // exactly one higher.
            queue.append(&mut above);
        }
    }

    fn finish(
        self,
        mut pool: Vec<(i32, i32, i32)>,
        held: Option<(i32, usize)>,
    ) -> Option<CavePool> {
        let (level, count) = held?;
        pool.truncate(count);
        let lowest = pool.iter().map(|c| c.1).min()?;
        // A puddle in a crack, or a film one cell deep, is not a lake.
        if pool.len() < MIN_CELLS || level - lowest < 1 {
            return None;
        }
        pool.sort_unstable();
        let min = pool
            .iter()
            .fold((i32::MAX, i32::MAX), |m, c| (m.0.min(c.0), m.1.min(c.2)));
        let max = pool
            .iter()
            .fold((i32::MIN, i32::MIN), |m, c| (m.0.max(c.0), m.1.max(c.2)));
        Some(CavePool {
            level,
            cells: pool,
            min,
            max,
        })
    }
}
