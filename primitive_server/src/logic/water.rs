//! Water that flows.
//!
//! The rules are in `primitive_shared::fluid` and were written down
//! before this existed, for the reason that module gives: a client that
//! draws a surface at one height and a server that thinks the level is
//! another produce a player swimming through a wall of water that is not
//! where it is drawn. This is the half that owns the world -- it looks
//! cells up, moves level between them and reports what changed. It
//! decides nothing about *how much* moves; `fluid::settle` and
//! `fluid::sideways` decide that, and they do it without a world so they
//! can be tested exhaustively.
//!
//! ## The shape
//!
//! A `CellMechanic` like any other (see `logic::simulation`): a queue of
//! coordinates to re-examine, a bounded number of them per pass, and
//! changes handed back for the caller to batch and broadcast. Sand
//! taught the shape; water is the second thing written to it, which is
//! the first evidence that the shape was worth writing down.
//!
//! Two things it does that sand does not:
//!
//! * **It dedupes the queue.** Every cell that changes pushes six
//!   neighbours, and a flowing front changes hundreds of cells a second
//!   -- so the same coordinate is offered dozens of times before it is
//!   ever looked at. Sand can afford the duplicates because a block
//!   edit pushes two cells; water cannot, and a queue full of the same
//!   dozen coordinates is a queue that is not draining.
//! * **It runs slower than the tick.** Not for the physics -- the
//!   simulation is stepwise and has no timestep in it -- but because
//!   water that spreads a block every 50 ms crosses a valley faster than
//!   a player can walk out of it, and because a step that runs a fifth
//!   as often costs a fifth as much. See `FLOW_INTERVAL`.
//!
//! ## A full cell is a source
//!
//! **This was strict conservation, and strict conservation ate the
//! oceans.** Every move took level out of one cell and put exactly that
//! much into another, which is the honest model and reads beautifully in
//! a test. What it does in a world is this: a player digs one hole at
//! the shore, water runs in, the cells behind it run in after, and the
//! wavefront walks out across the sea taking an eighth off everything it
//! passes. The ocean does not drain away -- there is far too much of it
//! -- it goes *lumpy*, permanently, in every direction from the hole.
//! Water at eight different depths in one bay, each cell a step, with a
//! wall drawn at every step. That is what "the water is broken" looked
//! like, and no amount of work in the mesher could have fixed it,
//! because the world really was that shape.
//!
//! So a cell that is **full** gives water away without losing any. It is
//! the sea, or a lake, or a river; it is not a bucket. Everything below
//! full is flowing water and still conserves exactly, so a stream
//! thins out as it spreads and comes to a stop -- which is the part that
//! has to terminate, and does.
//!
//! Two things follow, and both are wanted:
//!
//! * **Still water stays still.** Every cell worldgen makes is full, so
//!   an undisturbed sea is flat and never has an eighth taken out of it.
//! * **A basin that fills up settles.** Flowing water that accumulates
//!   to full becomes a source in its turn, so a hole under a lake fills
//!   and then stops being interesting, rather than staying a permanent
//!   dimple in the surface above it.

use std::collections::{HashSet, VecDeque};

use primitive_shared::fluid;
use primitive_shared::protocol::BlockChange;
use primitive_shared::types::{
    can_be_displaced_by_falling, is_liquid, with_layers, BlockId, BLOCK_AIR, BLOCK_WATER,
    CHUNK_SIZE_Y, LAYERS_PER_BLOCK,
};

use crate::logic::falling::BlockWorld;
use crate::logic::simulation::CellMechanic;

/// How long between flow steps, in seconds.
///
/// A quarter of a second is five ticks at the stock rate, and it is the
/// number that decides how fast water moves: a step spreads a front by
/// one cell, so this is four cells a second -- a brisk walk, slow enough
/// to run away from and fast enough that a channel fills while you are
/// still looking at it.
///
/// It is also four fifths of the cost gone. The simulation does not care
/// how often it is stepped (there is no timestep in `settle`), so the
/// only thing a per-tick step would buy is water that moves twenty cells
/// a second, which is the thing everyone regrets.
const FLOW_INTERVAL: f32 = 0.25;

/// Hard cap on the queue, as for falling blocks: a pathological edit
/// pattern degrades into "some water is slow" rather than into unbounded
/// memory.
const MAX_QUEUE: usize = 64 * 1024;

/// The four cells beside one, in a fixed order.
const SIDES: [(i32, i32); 4] = [(1, 0), (-1, 0), (0, 1), (0, -1)];

type Cell = (i32, i32, i32);

#[derive(Default)]
pub struct Water {
    queue: VecDeque<Cell>,
    /// Exactly the contents of `queue`. See the note above on why water
    /// needs this and sand does not.
    queued: HashSet<Cell>,
    /// Seconds since the last flow step, so the mechanic can run slower
    /// than the tick that drives it.
    since_step: f32,
    /// Cells that have finished moving and have nothing feeding them.
    ///
    /// **A second, much slower queue, and it has to be separate.** The
    /// flow queue is for water that is *doing* something; drying is for
    /// water that has stopped, and the flow queue empties itself of
    /// exactly those. A cell that has levelled off is dropped from the
    /// flow queue and never looked at again -- which is correct for
    /// flowing and is why a stranded puddle used to sit there for ever.
    ///
    /// Kept in insertion order like the other one so a big flood dries
    /// from its oldest edge rather than in whatever order a hash gives.
    drying: VecDeque<Cell>,
    drying_set: HashSet<Cell>,
    /// Flow steps since the last drying pass. See `fluid::DRY_AFTER_STEPS`.
    steps_since_dry: u32,
}

impl Water {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn pending(&self) -> usize {
        self.queue.len()
    }

    /// Queue a cell, unless it is outside the world or already waiting.
    fn push(&mut self, (x, y, z): Cell) {
        if y < 0 || y >= CHUNK_SIZE_Y as i32 || self.queue.len() >= MAX_QUEUE {
            return;
        }
        if self.queued.insert((x, y, z)) {
            self.queue.push_back((x, y, z));
        }
    }

    /// Everything a change at this cell can set in motion: the cell
    /// itself (it may be water that can now move), the cell above it (it
    /// may now have somewhere to fall) and the four beside it (they may
    /// now have somewhere to spread).
    ///
    /// Not the cell below: what a cell of water does depends on what is
    /// under it and beside it, never on what is over it.
    fn push_around(&mut self, (x, y, z): Cell) {
        self.push((x, y, z));
        self.push((x, y + 1, z));
        for (dx, dz) in SIDES {
            self.push((x + dx, y, z + dz));
        }
    }

    /// How much level a cell would accept: the whole cell if water can
    /// enter it at all, whatever is left of it if it already holds
    /// water, and none of it otherwise.
    ///
    /// `None` -- an unloaded chunk -- is no room at all. Water does not
    /// flow into terrain nobody has generated yet: the cell would be
    /// written and then overwritten the moment the chunk arrives.
    fn room(world: &dyn BlockWorld, (x, y, z): Cell) -> u8 {
        if y < 0 || y >= CHUNK_SIZE_Y as i32 {
            return 0;
        }
        match world.block(x, y, z) {
            Some(block) if is_liquid(block) => LAYERS_PER_BLOCK - fluid::level(block),
            // Plants are washed away rather than holding water back, on
            // the same list that decides what falling sand buries.
            Some(block) if can_be_displaced_by_falling(block) => LAYERS_PER_BLOCK,
            _ => 0,
        }
    }

    /// The level already in a cell water may enter, or `None` for one it
    /// may not.
    fn enterable_level(world: &dyn BlockWorld, cell: Cell) -> Option<u8> {
        match world.block(cell.0, cell.1, cell.2) {
            Some(block) if is_liquid(block) => Some(fluid::level(block)),
            Some(block) if can_be_displaced_by_falling(block) => Some(0),
            _ => None,
        }
    }

    /// Writes a cell and records it, both for the broadcast and for the
    /// queue -- a cell whose level changed is a cell that may now move
    /// again, and so are its neighbours.
    fn set(&mut self, world: &dyn BlockWorld, cell: Cell, level: u8, changes: &mut Vec<BlockChange>) {
        let block = water_at(level);
        let before = world.block(cell.0, cell.1, cell.2);
        world.set(cell.0, cell.1, cell.2, block);
        // **Only tell anyone when the change can be seen.**
        //
        // The simulation moves water in eighths, but `fluid::surface_height`
        // returns the same height for every level there is -- deliberately,
        // so a lake is one flat sheet rather than a staircase. So a cell
        // going from three eighths to five is a `BlockId` change that
        // draws *identically*, and sending it costs a packet, a chunk
        // remesh on every client that can see it (~0.8 ms of their frame
        // budget), and a permanent row in the world's edit overlay.
        //
        // What a client actually needs from this mechanic is where the
        // water is, not how much: air becoming water and water becoming
        // air. Those are the changes that alter geometry, light and what
        // you can swim in, and they are a small fraction of the traffic
        // a flood used to generate.
        if visibly_different(before, block) {
            changes.push(BlockChange {
                global_x: cell.0,
                global_y: cell.1,
                global_z: cell.2,
                block_id: block,
            });
        }
        self.push_around(cell);
    }

    /// One cell's worth of flow. `budget` counts these, not the lookups
    /// inside them.
    fn flow_one(&mut self, world: &dyn BlockWorld, cell: Cell, changes: &mut Vec<BlockChange>) {
        let (x, y, z) = cell;
        let Some(block) = world.block(x, y, z) else {
            return; // not loaded: leave it alone rather than guess
        };
        if !is_liquid(block) {
            return;
        }
        let here = fluid::level(block);
        if here == 0 {
            return;
        }

        let below = (x, y - 1, z);
        let below_room = Self::room(world, below);
        let sides = SIDES.map(|(dx, dz)| Self::enterable_level(world, (x + dx, y, z + dz)));
        let lower = sides
            .iter()
            .filter(|level| matches!(level, Some(level) if *level < here))
            .count() as u8;
        // Read before anything moves, and used twice: `settle` needs it
        // to know not to empty a cell something is falling into (see the
        // note there), and `stranded` needs it below to know whether a
        // cell that did not move has anything behind it. Nothing in this
        // function writes to the cell above, so one lookup does for both.
        let fed_from_above = matches!(
            world.block(x, y + 1, z),
            Some(above) if is_liquid(above) && fluid::level(above) > 0
        );

        let flow = fluid::settle(here, below_room, lower, fed_from_above);
        // A full cell is the sea rather than a bucket: it feeds what is
        // below and beside it and does not go down. See the note at the
        // top of this file for what strict conservation did instead.
        let source = here >= LAYERS_PER_BLOCK;
        let mut left = here;

        // Down first, and never both -- `settle` guarantees a cell with
        // somewhere to fall does not also spread, and the whole reason
        // that rule is written down once is so this can rely on it.
        if flow.down > 0 {
            let filled = fluid::level(world.block(below.0, below.1, below.2).unwrap_or(BLOCK_AIR));
            self.set(world, below, filled + flow.down, changes);
            if !source {
                left -= flow.down;
            }
        } else if flow.sideways > 0 {
            for ((dx, dz), there) in SIDES.iter().zip(sides) {
                let Some(there) = there else { continue };
                // Against what is left *now*, not against what was here
                // when the pass started: two neighbours filled in the
                // same step would otherwise both be measured against a
                // level this cell no longer has.
                let moved = fluid::sideways(left, there, flow.sideways);
                if moved == 0 {
                    continue;
                }
                self.set(world, (x + dx, y, z + dz), there + moved, changes);
                if !source {
                    left -= moved;
                }
            }
        }

        if left != here {
            self.set(world, cell, left, changes);
            return;
        }

        // Nothing moved. Either this cell is at rest among its equals --
        // in which case it is a puddle, and a puddle with nothing
        // running into it is going to dry -- or it is full, and a full
        // cell is a source and is never touched. `stranded` draws that
        // line; what is above was read at the top of this function.
        let highest_side = sides.iter().filter_map(|level| *level).max().unwrap_or(0);
        if fluid::stranded(here, fed_from_above, highest_side) {
            self.mark_drying(cell);
        }
    }

    /// Puts a cell on the slow queue, if it is not already on it.
    fn mark_drying(&mut self, cell: Cell) {
        if self.drying.len() >= MAX_QUEUE {
            return;
        }
        if self.drying_set.insert(cell) {
            self.drying.push_back(cell);
        }
    }

    /// One cell's worth of drying: an eighth off, or the last of it.
    ///
    /// Re-tested before it is drained, because everything that put it
    /// here may have changed in the seconds since. The sea reaching a
    /// stranded puddle is the ordinary case, not the exception: a player
    /// digs a channel to it and the puddle stops being stranded a step
    /// later.
    fn dry_one(&mut self, world: &dyn BlockWorld, cell: Cell, changes: &mut Vec<BlockChange>) {
        let (x, y, z) = cell;
        let Some(block) = world.block(x, y, z) else {
            return;
        };
        if !is_liquid(block) {
            return;
        }
        let here = fluid::level(block);
        let fed_from_above = matches!(
            world.block(x, y + 1, z),
            Some(above) if is_liquid(above) && fluid::level(above) > 0
        );
        let highest_side = SIDES
            .iter()
            .filter_map(|(dx, dz)| Self::enterable_level(world, (x + dx, y, z + dz)))
            .max()
            .unwrap_or(0);
        if !fluid::stranded(here, fed_from_above, highest_side) {
            return; // something feeds it now
        }
        self.set(world, cell, here - 1, changes);
        // Still stranded, still shrinking: back on the queue, because
        // `set` only wakes the *flow* queue and flowing is precisely
        // what this cell is not doing.
        if here - 1 > 0 {
            self.mark_drying(cell);
        }
    }
}

/// Would a client draw these two cells differently?
///
/// Two cells of water are drawn identically whatever their levels, so
/// the only visible change water can make is arriving somewhere or
/// leaving it. Kept next to `water_at`, and next to the reasoning in
/// `set` that depends on it: the day levels are drawn at their own
/// heights, this is the one function that has to change with
/// `fluid::surface_height`.
#[inline]
fn visibly_different(before: Option<BlockId>, after: BlockId) -> bool {
    match before {
        // Nothing known about the cell -- say yes and let the client
        // sort it out when the chunk lands.
        None => true,
        Some(before) => is_liquid(before) != is_liquid(after),
    }
}

/// The block id for a cell holding this much water. Nothing at all is
/// air, not a cell of water with nothing in it.
#[inline]
fn water_at(level: u8) -> BlockId {
    if level == 0 {
        BLOCK_AIR
    } else {
        with_layers(BLOCK_WATER, level.min(LAYERS_PER_BLOCK))
    }
}

impl CellMechanic for Water {
    fn name(&self) -> &'static str {
        "water"
    }

    fn on_block_changed(&mut self, gx: i32, gy: i32, gz: i32) {
        self.push_around((gx, gy, gz));
    }

    fn step(&mut self, world: &dyn BlockWorld, dt: f32, budget: usize) -> Vec<BlockChange> {
        self.since_step += dt;
        if self.since_step < FLOW_INTERVAL {
            return Vec::new();
        }
        // Not zeroed: a server running a hair over its tick budget would
        // otherwise lose the remainder every time and drift slow.
        // Clamped, because a server that has stalled for a second must
        // not then run five steps in five ticks to catch up -- the water
        // would visibly lurch.
        self.since_step = (self.since_step - FLOW_INTERVAL).min(FLOW_INTERVAL);

        let mut changes = Vec::new();
        for _ in 0..budget.min(self.queue.len()) {
            let Some(cell) = self.queue.pop_front() else {
                break;
            };
            self.queued.remove(&cell);
            self.flow_one(world, cell, &mut changes);
        }

        // ...and, far less often, the water that has stopped moving.
        //
        // The two are deliberately not the same pass. Flowing is what
        // the budget is for and it runs every step; drying runs once in
        // `DRY_AFTER_STEPS` of them, which is what keeps a pit being
        // filled from the sea from drying faster than it fills. See
        // `fluid::DRY_AFTER_STEPS`.
        self.steps_since_dry += 1;
        if self.steps_since_dry >= fluid::DRY_AFTER_STEPS {
            self.steps_since_dry = 0;
            for _ in 0..budget.min(self.drying.len()) {
                let Some(cell) = self.drying.pop_front() else {
                    break;
                };
                self.drying_set.remove(&cell);
                self.dry_one(world, cell, &mut changes);
            }
        }
        changes
    }

    fn pending(&self) -> usize {
        Water::pending(self)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::logic::falling::tests::TestWorld;
    use primitive_shared::types::{block_kind, BLOCK_STONE, BLOCK_TALL_GRASS};

    const TICK: f32 = 1.0 / 20.0;

    /// Ticks until the water stops moving, or until `limit` runs out.
    fn settle(sim: &mut Water, world: &TestWorld, limit: usize) -> Vec<BlockChange> {
        let mut all = Vec::new();
        for _ in 0..limit {
            if sim.pending() == 0 {
                break;
            }
            all.extend(sim.step(world, TICK, 4096));
        }
        all
    }

    /// How far the fixtures reach. Comfortably further than the water in
    /// any of them can spread, so a test that says "it all ended up
    /// somewhere" has looked everywhere it could have gone.
    const REACH: i32 = 16;

    /// A floor of stone at y = 0 across the whole area a test uses.
    fn floored() -> TestWorld {
        let world = TestWorld::default();
        for x in -REACH..=REACH {
            for z in -REACH..=REACH {
                world.put(x, 0, z, BLOCK_STONE);
            }
        }
        world
    }

    /// Every eighth of water in the world, wherever it is.
    fn total_water(world: &TestWorld) -> u32 {
        let mut total = 0;
        for x in -REACH..=REACH {
            for z in -REACH..=REACH {
                for y in 0..12 {
                    total += fluid::level(world.get(x, y, z)) as u32;
                }
            }
        }
        total
    }

    #[test]
    fn water_falls_before_it_spreads() {
        // The one rule, through the world rather than through `settle`:
        // a cell with a hole under it empties downward and leaves its
        // neighbours alone.
        let world = floored();
        world.put(0, 5, 0, BLOCK_WATER);

        let mut sim = Water::new();
        sim.on_block_changed(0, 5, 0);
        settle(&mut sim, &world, 400);

        assert!(fluid::level(world.get(0, 1, 0)) > 0, "it never reached the floor");
        // ...and the cell it fell from is still full, because a full
        // cell is a source. A waterfall that empties its own top is a
        // waterfall that stops.
        assert_eq!(
            fluid::level(world.get(0, 5, 0)),
            LAYERS_PER_BLOCK,
            "the source drained itself"
        );
        for (dx, dz) in SIDES {
            assert_eq!(
                fluid::level(world.get(dx, 5, dz)),
                0,
                "it spread while it had room to fall"
            );
        }
    }

    #[test]
    fn a_waterfall_does_not_flicker_the_cells_it_falls_through() {
        // **The oscillation.** A cell in the middle of a falling column
        // took two eighths from the source above and gave two eighths to
        // the cell below, which left nothing -- and nothing is not a
        // thinner cell of water, it is `BLOCK_AIR`. So the cell emptied
        // to air, the source filled it again, and the two of them did
        // that four times a second for as long as the water fell.
        //
        // Counted in *broadcast* changes rather than in world reads,
        // because that is where every part of the cost lands: a packet
        // to every client that can see it, a chunk remesh on each of
        // them, and a row in the world's edit overlay -- which is what
        // made the autosave rewrite the whole of `edits.bin` every
        // interval while any water anywhere was falling.
        let world = floored();
        world.put(0, 8, 0, BLOCK_WATER);

        let mut sim = Water::new();
        sim.on_block_changed(0, 8, 0);
        let mut changes = Vec::new();
        // Long enough for two hundred flow steps, which is fifty
        // seconds of a waterfall that is still running at the end: the
        // floor is far wider than the water can fill, so the column
        // never turns into a settled full column and stops being
        // interesting.
        for _ in 0..1_000 {
            changes.extend(sim.step(&world, TICK, 4096));
        }

        let middle = (0, 5, 0);
        assert!(
            fluid::level(world.get(middle.0, middle.1, middle.2)) > 0,
            "the middle of the column ran dry, so there was no waterfall to test"
        );
        let churn = changes
            .iter()
            .filter(|c| (c.global_x, c.global_y, c.global_z) == middle)
            .count();
        // Once: the water arrived. Anything more is the cell being
        // emptied and refilled.
        assert!(
            churn <= 1,
            "the middle of the waterfall was reported {churn} times"
        );
    }

    #[test]
    fn water_on_a_floor_spreads_and_then_stops() {
        let world = floored();
        world.put(0, 1, 0, BLOCK_WATER);

        let mut sim = Water::new();
        sim.on_block_changed(0, 1, 0);
        let changes = settle(&mut sim, &world, 2000);

        assert!(!changes.is_empty(), "it never moved at all");
        assert!(
            fluid::level(world.get(1, 1, 0)) > 0,
            "nothing reached the cell beside it"
        );
        // ...and it came to rest. A simulation that never goes quiet is
        // a block update per tick for every puddle in the world.
        assert_eq!(sim.pending(), 0, "the queue never drained");
    }

    #[test]
    fn flowing_water_is_neither_created_nor_destroyed() {
        // Conservation still holds for everything below full, which is
        // all the water that actually moves. A *source* is exempt by
        // definition -- that is what makes it a source -- so this uses
        // half-full cells, which are ordinary flowing water.
        //
        // Every move is a chance to lose or invent an eighth, and a
        // stream that quietly gains one never stops spreading.
        use primitive_shared::types::with_layers;
        let world = floored();
        let half = with_layers(BLOCK_WATER, 4);
        for x in -1..=1 {
            for z in -1..=1 {
                world.put(x, 1, z, half);
            }
        }

        let mut sim = Water::new();
        let before = total_water(&world);
        for x in -1..=1 {
            for z in -1..=1 {
                sim.on_block_changed(x, 1, z);
            }
        }
        settle(&mut sim, &world, 20_000);

        assert_eq!(
            total_water(&world),
            before,
            "the flowing water in the world changed while it was moving about"
        );
    }

    #[test]
    fn digging_at_the_shore_does_not_make_the_sea_lumpy() {
        // **The bug this model exists for.** With strict conservation,
        // one hole at the edge of a sea sent a wavefront out across it
        // taking an eighth off every cell it passed: water at eight
        // different depths in one bay, permanently, each step of it
        // drawn with a wall. The sea does not drain -- there is far too
        // much of it -- it goes lumpy, and no work in the mesher could
        // have fixed that, because the world really was that shape.
        let world = floored();
        for x in -REACH..=REACH {
            for z in -REACH..=REACH {
                world.put(x, 1, z, BLOCK_WATER);
            }
        }
        // Somebody digs one cell out of the floor at the edge.
        world.put(REACH - 1, 0, REACH - 1, BLOCK_AIR);

        let mut sim = Water::new();
        sim.on_block_changed(REACH - 1, 0, REACH - 1);
        settle(&mut sim, &world, 20_000);

        // Every cell of the sea is still full. Not "nearly all of
        // them": one dimple is the whole complaint.
        for x in -REACH..=REACH {
            for z in -REACH..=REACH {
                let level = fluid::level(world.get(x, 1, z));
                assert_eq!(
                    level,
                    LAYERS_PER_BLOCK,
                    "the sea is {level}/8 deep at ({x},{z}) after one hole was dug"
                );
            }
        }
        // ...and the hole itself filled, rather than the sea ignoring it.
        assert_eq!(
            fluid::level(world.get(REACH - 1, 0, REACH - 1)),
            LAYERS_PER_BLOCK,
            "the hole never filled"
        );
    }

    #[test]
    fn a_lake_that_is_already_flat_does_not_move() {
        // The oscillation test. Two cells trading the same eighth back
        // and forth for ever is a block update per tick per pair, and it
        // is invisible until a server is at 40% CPU doing nothing.
        //
        // Walled, because a puddle with a dry edge is *supposed* to
        // spill off it -- what must not move is water that has nowhere
        // left to go.
        let world = floored();
        for x in -3i32..=3 {
            for z in -3i32..=3 {
                let wall = x.abs() == 3 || z.abs() == 3;
                world.put(x, 1, z, if wall { BLOCK_STONE } else { BLOCK_WATER });
            }
        }

        let mut sim = Water::new();
        for x in -2..=2 {
            for z in -2..=2 {
                sim.on_block_changed(x, 1, z);
            }
        }
        let changes = settle(&mut sim, &world, 2000);

        assert!(
            changes.is_empty(),
            "a flat lake moved {} times",
            changes.len()
        );
    }

    #[test]
    fn water_does_not_flow_into_solid_ground_or_unloaded_chunks() {
        let world = floored();
        world.put(1, 1, 0, BLOCK_STONE);
        world.missing_cell(-1, 1, 0);
        world.put(0, 1, 0, BLOCK_WATER);

        let mut sim = Water::new();
        sim.on_block_changed(0, 1, 0);
        settle(&mut sim, &world, 2000);

        assert_eq!(world.get(1, 1, 0), BLOCK_STONE, "it went through the wall");
        assert_eq!(
            fluid::level(world.get(-1, 1, 0)),
            0,
            "it flowed into a chunk nobody has generated"
        );
    }

    #[test]
    fn water_washes_plants_away() {
        // Same list that decides what falling sand buries: a tuft of
        // grass does not hold back a flood.
        let world = floored();
        world.put(1, 1, 0, BLOCK_TALL_GRASS);
        world.put(0, 1, 0, BLOCK_WATER);

        let mut sim = Water::new();
        sim.on_block_changed(0, 1, 0);
        settle(&mut sim, &world, 2000);

        assert_ne!(
            block_kind(world.get(1, 1, 0)),
            block_kind(BLOCK_TALL_GRASS),
            "the tuft held the water back"
        );
    }

    #[test]
    fn it_runs_slower_than_the_tick_that_drives_it() {
        let world = floored();
        world.put(0, 5, 0, BLOCK_WATER);
        let mut sim = Water::new();
        sim.on_block_changed(0, 5, 0);

        // Four ticks at 20 Hz is under the interval: nothing yet.
        for _ in 0..4 {
            assert!(sim.step(&world, TICK, 512).is_empty());
        }
        assert!(!sim.step(&world, TICK, 512).is_empty(), "it never stepped");
    }

    #[test]
    fn the_queue_dedupes_and_stays_bounded() {
        let mut sim = Water::new();
        for _ in 0..50 {
            sim.on_block_changed(0, 5, 0);
        }
        assert_eq!(
            sim.pending(),
            6,
            "the same cell was queued once per notification"
        );

        for i in 0..(MAX_QUEUE as i32 + 5_000) {
            sim.on_block_changed(i, 30, 0);
        }
        assert!(sim.pending() <= MAX_QUEUE);
    }

    #[test]
    fn it_flows_through_the_real_world_and_the_real_registry() {
        // The unit tests above prove the rules. This proves the wiring:
        // the server's own sharded, overlay-backed `World` as the
        // `BlockWorld`, and the mechanic reached through `Mechanics`
        // exactly as the tick loop reaches it. Everything between the
        // two -- the trait impl, the registration, the budget split --
        // is code no unit test here touches.
        use crate::logic::simulation::Mechanics;
        use crate::logic::world::World;

        let world = World::new(4242, 64);
        let chunk = world.generate(primitive_shared::types::ChunkPos::new(0, 0));
        world.insert(chunk);

        // A shelf of stone with a cell of water in the air over it.
        for y in 36..=41 {
            for x in 3..=5 {
                for z in 3..=5 {
                    world.set_block(x, y, z, BLOCK_AIR);
                }
            }
        }
        for x in 3..=5 {
            for z in 3..=5 {
                world.set_block(x, 36, z, BLOCK_STONE);
            }
        }
        world.set_block(4, 40, 4, BLOCK_WATER);

        let mut mechanics = Mechanics::new();
        mechanics.register(Box::new(Water::new()));
        mechanics.on_block_changed(4, 40, 4);
        assert_eq!(mechanics.pending(), vec![("water", 6)]);

        let mut changes = Vec::new();
        for _ in 0..400 {
            changes.extend(mechanics.step(&world, TICK));
        }

        assert!(!changes.is_empty(), "nothing was reported to the clients");
        assert!(
            fluid::level(world.cached_block(4, 37, 4).unwrap()) > 0,
            "it never reached the shelf"
        );
    }

    /// Ticks long enough for the slow drying pass to run `passes` times.
    fn wait_for_drying(sim: &mut Water, world: &TestWorld, passes: u32) -> Vec<BlockChange> {
        let mut all = Vec::new();
        let steps = fluid::DRY_AFTER_STEPS * passes + fluid::DRY_AFTER_STEPS;
        // Five ticks to a flow step, and every flow step counts toward
        // the drying interval whether or not anything was flowing.
        for _ in 0..(steps * 5 + 10) {
            all.extend(sim.step(world, TICK, 4096));
        }
        all
    }

    #[test]
    fn water_with_nothing_feeding_it_dries_up() {
        // **What happens when the source is taken away.** Cut the
        // channel that fed a pool and the water in it used to stay
        // exactly where it was for ever -- standing water with nothing
        // running into it, which is the one thing water never does.
        let world = floored();
        let mut sim = Water::new();
        // A puddle four eighths deep, walled in on every side, with
        // nothing above it.
        for (x, z) in [(0, 0), (1, 0), (0, 1), (1, 1)] {
            world.put(x, 1, z, with_layers(BLOCK_WATER, 4));
            sim.on_block_changed(x, 1, z);
        }
        for x in -1..=2 {
            for z in -1..=2 {
                if !(0..=1).contains(&x) || !(0..=1).contains(&z) {
                    world.put(x, 1, z, BLOCK_STONE);
                }
            }
        }

        settle(&mut sim, &world, 400);
        assert!(total_water(&world) > 0, "the puddle went before drying began");

        wait_for_drying(&mut sim, &world, LAYERS_PER_BLOCK as u32 + 2);
        assert_eq!(
            total_water(&world),
            0,
            "a puddle with no source left standing water behind"
        );
    }

    #[test]
    fn the_sea_does_not_evaporate() {
        // The other half, and the one that matters more: a full cell is
        // a source, and drying must not touch one. Otherwise every lake
        // in the world quietly empties from the top down.
        let world = floored();
        let mut sim = Water::new();
        for x in -3..=3 {
            for z in -3..=3 {
                world.put(x, 1, z, BLOCK_WATER);
                world.put(x, 2, z, BLOCK_WATER);
                sim.on_block_changed(x, 1, z);
            }
        }
        // Walls, so nothing runs off and the lake is genuinely at rest.
        for x in -4i32..=4 {
            for z in -4i32..=4 {
                if x.abs() == 4 || z.abs() == 4 {
                    world.put(x, 1, z, BLOCK_STONE);
                    world.put(x, 2, z, BLOCK_STONE);
                }
            }
        }
        let before = total_water(&world);
        settle(&mut sim, &world, 400);
        wait_for_drying(&mut sim, &world, 12);
        assert_eq!(total_water(&world), before, "the lake evaporated");
    }

    #[test]
    fn a_pit_being_filled_from_the_sea_still_fills() {
        // **The race drying could have lost.** A hole halfway through
        // filling is full of cells whose neighbours are no higher than
        // they are -- indistinguishable, for a moment, from a puddle
        // with nothing behind it. Drying has to be slow enough that the
        // sea wins every time; see `fluid::DRY_AFTER_STEPS`.
        let world = floored();
        let mut sim = Water::new();
        // A body of full water to the left, an empty trench to the
        // right, joined at one cell.
        for x in -8..=0 {
            for z in -1..=1 {
                world.put(x, 1, z, BLOCK_WATER);
                world.put(x, 2, z, BLOCK_WATER);
            }
        }
        for x in 1..=6 {
            for z in -2..=2 {
                world.put(x, 0, z, BLOCK_STONE);
            }
        }
        sim.on_block_changed(0, 1, 0);

        // Run long enough that several drying passes happen *while* the
        // trench is filling.
        for _ in 0..1_200 {
            sim.step(&world, TICK, 4096);
        }
        let reached = (1..=4)
            .filter(|&x| fluid::level(world.get(x, 1, 0)) > 0)
            .count();
        assert!(reached >= 3, "only {reached} cells of the trench took water");
    }

    #[test]
    fn only_visible_changes_are_broadcast() {
        // A cell filling from three eighths to five draws identically --
        // `fluid::surface_height` is the same for every level -- so
        // sending it costs a packet and a chunk remesh on every client
        // in range to change nothing at all. What a client needs is
        // where the water is, not how much.
        let world = floored();
        let mut sim = Water::new();
        // A source pouring into a cell that already holds water: the
        // neighbour's level will climb, but it was water before and it
        // is water after.
        world.put(0, 1, 0, BLOCK_WATER);
        world.put(1, 1, 0, with_layers(BLOCK_WATER, 2));
        for x in [-1, 2] {
            world.put(x, 1, 0, BLOCK_STONE);
        }
        for z in [-1, 1] {
            for x in 0..=1 {
                world.put(x, 1, z, BLOCK_STONE);
            }
        }
        sim.on_block_changed(0, 1, 0);
        let changes = settle(&mut sim, &world, 200);

        // It really did move -- otherwise this proves nothing.
        assert!(
            fluid::level(world.get(1, 1, 0)) > 2,
            "the neighbour never filled, so there was nothing to report"
        );
        // ...and none of what it reported was an invisible level change.
        for change in &changes {
            let cell = (change.global_x, change.global_y, change.global_z);
            assert_ne!(
                cell,
                (1, 1, 0),
                "an invisible level change was broadcast anyway"
            );
        }
    }

    #[test]
    fn arriving_and_leaving_are_still_broadcast() {
        // The other half: the changes that *are* visible must survive
        // the filter, or water spreads on the server and nowhere else.
        let world = floored();
        let mut sim = Water::new();
        world.put(0, 1, 0, BLOCK_WATER);
        sim.on_block_changed(0, 1, 0);
        let changes = settle(&mut sim, &world, 200);
        assert!(
            changes.iter().any(|c| is_liquid(c.block_id)),
            "water spread on the server and told nobody"
        );
    }

    #[test]
    fn work_per_pass_is_bounded() {
        let world = floored();
        let mut sim = Water::new();
        for i in 0..2_000 {
            world.put(i, 5, 0, BLOCK_WATER);
            sim.on_block_changed(i, 5, 0);
        }
        let before = sim.pending();
        // Enough ticks for one step, and one step only.
        for _ in 0..5 {
            sim.step(&world, TICK, 64);
        }
        assert!(sim.pending() > before - 512, "a single pass drained far too much");
    }
}
