//! Falling blocks (sand).
//!
//! Runs on the server, because the world is the server's: a client
//! simulating its own sand would disagree with everyone else's within a
//! second, and any client could then claim sand had landed wherever it
//! liked.
//!
//! ## Shape of the simulation
//!
//! Not a per-block entity with a position and a velocity -- just a work
//! queue of coordinates to re-examine. A block edit pushes the cells
//! that could newly be unsupported; each tick pops a bounded number of
//! them, and any that turn out to be floating sand move down one block.
//! Moving a block pushes its own new position (it may keep falling) and
//! the cell above it (whatever was resting on it is now unsupported
//! too), so a whole column collapses over several ticks without any of
//! it being tracked as an object.
//!
//! Two properties this shape buys:
//!
//! * **Bounded work per tick.** A player hollowing out a desert can't
//!   stall the tick loop; the queue just drains over more ticks.
//! * **No lost updates.** The queue is the only state, so a chunk being
//!   evicted from the cache mid-fall doesn't leave a half-fallen tower.
//!
//! ## Falling blocks are entities
//!
//! A block that starts falling is removed from the world and becomes a
//! **falling-block entity**: a position and a velocity, integrated every
//! tick and replicated to nearby clients, which draw it smoothly between
//! ticks. When it lands it turns back into a block.
//!
//! The earlier version teleported the block down one cell every few
//! ticks. That reads as stuttering, and it also meant the block existed
//! in the world grid the whole way down, so it briefly blocked whatever
//! was below it. An entity has neither problem: the grid cell is empty
//! while it's in the air, and the client interpolates the motion.

use std::collections::VecDeque;

use primitive_shared::protocol::{BlockChange, EntityId, EntityKind, EntityState};
use primitive_shared::types::{
    can_be_displaced_by_falling, is_affected_by_gravity, BlockId, BLOCK_AIR, CHUNK_SIZE_Y,
};

/// Cells examined per pass. Generous -- the check is a couple of cached
/// block lookups -- but finite.
pub const MAX_CHECKS_PER_PASS: usize = 512;
/// Gravity for falling entities, blocks per second squared.
const GRAVITY: f32 = -24.0;
/// Terminal speed, so a block dropped from the sky doesn't tunnel
/// through the ground between ticks.
const MAX_FALL_SPEED: f32 = -18.0;
/// A block never falls further than this in one tick, whatever the
/// timestep -- the guarantee that the landing check can't be skipped.
const MAX_STEP: f32 = 0.9;
/// Hard cap on the queue. A pathological edit pattern should degrade
/// into "some sand doesn't fall" rather than into unbounded memory.
const MAX_QUEUE: usize = 64 * 1024;
/// How far above its blocking cell a landing block will look for
/// somewhere to come to rest.
///
/// It needs to look at all because a column of falling sand lands as
/// several separate entities: the lowest one fills the cell the next one
/// was about to occupy. Bounded, because "search the whole column" turns
/// a tall tower landing into O(height²) work, and because a block that
/// can't find a home within a few cells is in a pocket the player has
/// deliberately sealed.
const MAX_LANDING_SEARCH: i32 = 8;

/// The world operations the simulation needs. A trait so the logic can
/// be tested against a plain HashMap instead of a live sharded world.
pub trait BlockWorld {
    /// `None` means "not loaded" -- the simulation then leaves the cell
    /// alone rather than generating terrain to answer.
    fn block(&self, gx: i32, gy: i32, gz: i32) -> Option<BlockId>;
    fn set(&self, gx: i32, gy: i32, gz: i32, block: BlockId);
}

/// A block in mid-air.
#[derive(Debug, Clone, Copy)]
pub struct FallingEntity {
    pub id: EntityId,
    pub block: BlockId,
    /// Position of the block's minimum corner.
    pub x: f32,
    pub y: f32,
    pub z: f32,
    pub velocity_y: f32,
}

impl FallingEntity {
    pub fn state(&self) -> EntityState {
        EntityState {
            id: self.id,
            kind: EntityKind::FallingBlock { block: self.block },
            x: self.x,
            y: self.y,
            z: self.z,
        }
    }
}

#[derive(Default)]
pub struct FallingBlocks {
    queue: VecDeque<(i32, i32, i32)>,
    entities: Vec<FallingEntity>,
    next_id: EntityId,
    dropped: u64,
}

impl FallingBlocks {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn pending(&self) -> usize {
        self.queue.len()
    }

    /// Total blocks that have finished falling, for `/stats`.
    pub fn dropped(&self) -> u64 {
        self.dropped
    }

    pub fn entities(&self) -> &[FallingEntity] {
        &self.entities
    }

    pub fn entity_count(&self) -> usize {
        self.entities.len()
    }

    /// Entity snapshots within `radius` blocks of a point -- the same
    /// interest filtering players get, so a distant sandslide costs a
    /// player nothing.
    pub fn nearby_states(&self, centre: (f32, f32, f32), radius: f32) -> Vec<EntityState> {
        let radius_squared = radius * radius;
        self.entities
            .iter()
            .filter(|e| {
                let (dx, dy, dz) = (e.x - centre.0, e.y - centre.1, e.z - centre.2);
                dx * dx + dy * dy + dz * dz <= radius_squared
            })
            .map(|e| e.state())
            .collect()
    }

    /// Call after any block change. Queues the cell itself (it may be
    /// sand that can now fall) and the cell above it (whatever was
    /// resting on the old block may now be unsupported).
    pub fn on_block_changed(&mut self, gx: i32, gy: i32, gz: i32) {
        self.push(gx, gy, gz);
        self.push(gx, gy + 1, gz);
    }

    fn push(&mut self, gx: i32, gy: i32, gz: i32) {
        if gy < 0 || gy >= CHUNK_SIZE_Y as i32 || self.queue.len() >= MAX_QUEUE {
            return;
        }
        self.queue.push_back((gx, gy, gz));
    }

    /// Advances the simulation by `dt` seconds.
    ///
    /// Two halves: cells to re-examine (does anything start falling?)
    /// and entities already in the air (do they land?).
    pub fn step<W: BlockWorld>(&mut self, world: &W, dt: f32) -> Vec<BlockChange> {
        let mut changes = self.spawn_new_falls(world);
        changes.extend(self.advance_entities(world, dt));
        changes
    }

    /// Looks at queued cells and turns unsupported blocks into entities.
    fn spawn_new_falls<W: BlockWorld>(&mut self, world: &W) -> Vec<BlockChange> {
        let mut changes = Vec::new();
        let checks = self.queue.len().min(MAX_CHECKS_PER_PASS);

        for _ in 0..checks {
            let Some((gx, gy, gz)) = self.queue.pop_front() else {
                break;
            };

            let Some(block) = world.block(gx, gy, gz) else {
                continue; // chunk not loaded; forget it rather than guess
            };
            if !is_affected_by_gravity(block) {
                continue;
            }
            if gy == 0 {
                continue; // bedrock floor
            }

            let Some(below) = world.block(gx, gy - 1, gz) else {
                continue;
            };
            if !can_be_displaced_by_falling(below) {
                continue; // supported
            }

            // Leave the grid and become an entity.
            world.set(gx, gy, gz, BLOCK_AIR);
            changes.push(BlockChange {
                global_x: gx,
                global_y: gy,
                global_z: gz,
                block_id: BLOCK_AIR,
            });

            self.next_id += 1;
            self.entities.push(FallingEntity {
                id: self.next_id,
                block,
                x: gx as f32,
                y: gy as f32,
                z: gz as f32,
                velocity_y: 0.0,
            });

            // Whatever was resting on it is now unsupported.
            self.push(gx, gy + 1, gz);
        }

        changes
    }

    /// Integrates entities and lands the ones that hit something.
    fn advance_entities<W: BlockWorld>(&mut self, world: &W, dt: f32) -> Vec<BlockChange> {
        let mut changes = Vec::new();
        // (index, the cell that stopped it). Keeping the blocking cell
        // rather than re-deriving it later is what makes the resting
        // position exact -- see `land`.
        let mut landed: Vec<(usize, i32)> = Vec::new();

        for (index, entity) in self.entities.iter_mut().enumerate() {
            entity.velocity_y = (entity.velocity_y + GRAVITY * dt).max(MAX_FALL_SPEED);
            // Clamping the step is what makes the landing check sound:
            // without it a fast block could pass through a floor between
            // two ticks and keep going.
            let step = (entity.velocity_y * dt).max(-MAX_STEP);
            let next_y = entity.y + step;

            let cell_below = next_y.floor() as i32;
            let blocked = cell_below < 0
                || world
                    .block(entity.x.floor() as i32, cell_below, entity.z.floor() as i32)
                    .map(|id| !can_be_displaced_by_falling(id))
                    .unwrap_or(true); // unloaded: stop rather than fall into the unknown

            if blocked {
                landed.push((index, cell_below));
            } else {
                entity.y = next_y;
            }
        }

        // Take them out back-to-front so the indices stay valid, then
        // settle them lowest first.
        //
        // The order matters. A collapsing column becomes several
        // entities, and they can be blocked on the same tick; if the
        // upper one is placed first it takes the cell the lower one was
        // going to occupy, and the lower one then has to search *up*
        // past it. Settling from the bottom means each block lands where
        // it actually fell to, and the stack keeps its original order.
        let mut settling: Vec<(FallingEntity, i32)> = landed
            .into_iter()
            .rev()
            .map(|(index, cell_below)| (self.entities.swap_remove(index), cell_below))
            .collect();
        settling.sort_by(|a, b| a.0.y.total_cmp(&b.0.y));

        for (entity, cell_below) in settling {
            if let Some(change) = self.land(world, &entity, cell_below) {
                changes.push(change);
            }
        }

        changes
    }

    /// Turns a stopped entity back into a block.
    ///
    /// The resting cell is the one *above* whatever stopped it, which is
    /// not always the cell the entity's own position is in: between the
    /// two, `cell_below + 1` is the authority, because a block may have
    /// been placed under the entity (or another falling block may have
    /// landed there) since its position was last validated.
    ///
    /// If that cell is occupied the search continues upward. That case is
    /// the whole reason this isn't a one-liner: without it, a column of
    /// sand collapsing loses every block but the first, because each one
    /// lands into the cell the one below it just filled and is silently
    /// discarded.
    fn land<W: BlockWorld>(
        &mut self,
        world: &W,
        entity: &FallingEntity,
        cell_below: i32,
    ) -> Option<BlockChange> {
        let gx = entity.x.floor() as i32;
        let gz = entity.z.floor() as i32;
        let start = (cell_below + 1).max(entity.y.floor() as i32).max(0);

        for gy in start..(start + MAX_LANDING_SEARCH).min(CHUNK_SIZE_Y as i32) {
            let free = world
                .block(gx, gy, gz)
                .map(can_be_displaced_by_falling)
                .unwrap_or(false);
            if !free {
                continue;
            }

            world.set(gx, gy, gz, entity.block);
            self.dropped += 1;
            // It may itself be unsupported (a block was mined out from
            // under it mid-fall), and whatever is above it now has
            // something to rest on.
            self.push(gx, gy, gz);
            self.push(gx, gy + 1, gz);
            return Some(BlockChange {
                global_x: gx,
                global_y: gy,
                global_z: gz,
                block_id: entity.block,
            });
        }

        // Nowhere to put it within reach: the block is lost. The
        // alternative -- searching further, or sideways -- ends up
        // placing sand inside sealed rooms a long way from where it fell.
        None
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use primitive_shared::types::{BLOCK_SAND, BLOCK_STONE, BLOCK_WATER};
    use std::cell::RefCell;
    use std::collections::HashMap;

    #[derive(Default)]
    pub struct TestWorld {
        blocks: RefCell<HashMap<(i32, i32, i32), BlockId>>,
        /// Coordinates that read as "not loaded".
        missing: RefCell<Vec<(i32, i32, i32)>>,
    }

    impl TestWorld {
        pub fn put(&self, gx: i32, gy: i32, gz: i32, id: BlockId) {
            self.blocks.borrow_mut().insert((gx, gy, gz), id);
        }
        pub fn get(&self, gx: i32, gy: i32, gz: i32) -> BlockId {
            self.blocks
                .borrow()
                .get(&(gx, gy, gz))
                .copied()
                .unwrap_or(BLOCK_AIR)
        }
    }

    impl BlockWorld for TestWorld {
        fn block(&self, gx: i32, gy: i32, gz: i32) -> Option<BlockId> {
            if self.missing.borrow().contains(&(gx, gy, gz)) {
                return None;
            }
            Some(self.get(gx, gy, gz))
        }
        fn set(&self, gx: i32, gy: i32, gz: i32, block: BlockId) {
            self.put(gx, gy, gz, block);
        }
    }

    const TICK: f32 = 1.0 / 20.0;

    /// Runs until nothing is queued and nothing is in the air.
    fn settle(sim: &mut FallingBlocks, world: &TestWorld, limit: usize) -> Vec<BlockChange> {
        let mut all = Vec::new();
        for _ in 0..limit {
            if sim.pending() == 0 && sim.entity_count() == 0 {
                break;
            }
            all.extend(sim.step(world, TICK));
        }
        all
    }

    #[test]
    fn unsupported_sand_falls_until_it_lands() {
        let world = TestWorld::default();
        world.put(0, 0, 0, BLOCK_STONE);
        world.put(0, 10, 0, BLOCK_SAND);

        let mut sim = FallingBlocks::new();
        sim.on_block_changed(0, 10, 0);
        settle(&mut sim, &world, 100);

        assert_eq!(world.get(0, 1, 0), BLOCK_SAND, "should rest on the stone");
        assert_eq!(world.get(0, 10, 0), BLOCK_AIR, "should have left the top");
    }

    #[test]
    fn supported_sand_stays_put() {
        let world = TestWorld::default();
        world.put(0, 5, 0, BLOCK_STONE);
        world.put(0, 6, 0, BLOCK_SAND);

        let mut sim = FallingBlocks::new();
        sim.on_block_changed(0, 6, 0);
        let changes = settle(&mut sim, &world, 20);

        assert!(changes.is_empty(), "nothing should have moved");
        assert_eq!(world.get(0, 6, 0), BLOCK_SAND);
    }

    #[test]
    fn removing_the_support_makes_the_whole_column_collapse() {
        // This is the case the "push the cell above" rule exists for: a
        // stack of sand has to come down, not just its bottom block.
        let world = TestWorld::default();
        world.put(0, 0, 0, BLOCK_STONE);
        world.put(0, 1, 0, BLOCK_STONE); // the support, about to be mined
        for y in 2..=6 {
            world.put(0, y, 0, BLOCK_SAND);
        }

        let mut sim = FallingBlocks::new();
        world.set(0, 1, 0, BLOCK_AIR); // player mines it
        sim.on_block_changed(0, 1, 0);
        settle(&mut sim, &world, 200);

        for y in 1..=5 {
            assert_eq!(world.get(0, y, 0), BLOCK_SAND, "sand missing at y={y}");
        }
        assert_eq!(world.get(0, 6, 0), BLOCK_AIR, "the column should have dropped");
    }

    #[test]
    fn sand_sinks_through_water() {
        let world = TestWorld::default();
        world.put(0, 0, 0, BLOCK_STONE);
        for y in 1..=5 {
            world.put(0, y, 0, BLOCK_WATER);
        }
        world.put(0, 6, 0, BLOCK_SAND);

        let mut sim = FallingBlocks::new();
        sim.on_block_changed(0, 6, 0);
        settle(&mut sim, &world, 100);

        assert_eq!(world.get(0, 1, 0), BLOCK_SAND, "sand should reach the bottom");
    }

    #[test]
    fn other_blocks_are_left_alone() {
        let world = TestWorld::default();
        world.put(0, 10, 0, BLOCK_STONE); // floating stone stays floating

        let mut sim = FallingBlocks::new();
        sim.on_block_changed(0, 10, 0);
        let changes = settle(&mut sim, &world, 20);

        assert!(changes.is_empty());
        assert_eq!(world.get(0, 10, 0), BLOCK_STONE);
    }

    #[test]
    fn sand_does_not_fall_out_of_the_world() {
        let world = TestWorld::default();
        world.put(0, 0, 0, BLOCK_SAND);

        let mut sim = FallingBlocks::new();
        sim.on_block_changed(0, 0, 0);
        settle(&mut sim, &world, 20);

        assert_eq!(world.get(0, 0, 0), BLOCK_SAND, "y=0 is the floor");
    }

    #[test]
    fn an_unloaded_chunk_is_skipped_rather_than_guessed() {
        let world = TestWorld::default();
        world.put(0, 10, 0, BLOCK_SAND);
        world.missing.borrow_mut().push((0, 9, 0));

        let mut sim = FallingBlocks::new();
        sim.on_block_changed(0, 10, 0);
        let changes = settle(&mut sim, &world, 20);

        assert!(changes.is_empty(), "must not move sand into unloaded space");
        assert_eq!(world.get(0, 10, 0), BLOCK_SAND);
    }

    #[test]
    fn a_falling_block_becomes_an_entity_and_lands_as_a_block() {
        let world = TestWorld::default();
        world.put(0, 0, 0, BLOCK_STONE);
        world.put(0, 10, 0, BLOCK_SAND);

        let mut sim = FallingBlocks::new();
        sim.on_block_changed(0, 10, 0);

        // First step: it leaves the grid and enters the air.
        sim.step(&world, TICK);
        assert_eq!(sim.entity_count(), 1, "should be airborne");
        assert_eq!(world.get(0, 10, 0), BLOCK_AIR, "grid cell must be empty");

        settle(&mut sim, &world, 500);
        assert_eq!(sim.entity_count(), 0, "should have landed");
        assert_eq!(world.get(0, 1, 0), BLOCK_SAND);
    }

    #[test]
    fn a_falling_entity_accelerates_instead_of_moving_at_a_fixed_rate() {
        let world = TestWorld::default();
        world.put(0, 0, 0, BLOCK_STONE);
        world.put(0, 40, 0, BLOCK_SAND);

        let mut sim = FallingBlocks::new();
        sim.on_block_changed(0, 40, 0);
        sim.step(&world, TICK);

        let mut heights = Vec::new();
        for _ in 0..6 {
            sim.step(&world, TICK);
            heights.push(sim.entities()[0].y);
        }
        let first_drop = heights[0] - heights[1];
        let later_drop = heights[4] - heights[5];
        assert!(
            later_drop > first_drop,
            "gravity should accelerate it ({first_drop} then {later_drop})"
        );
    }

    #[test]
    fn a_fast_block_cannot_tunnel_through_the_floor() {
        // The per-tick step is clamped precisely so the landing check
        // can't be skipped over.
        let world = TestWorld::default();
        world.put(0, 0, 0, BLOCK_STONE);
        world.put(0, 60, 0, BLOCK_SAND);

        let mut sim = FallingBlocks::new();
        sim.on_block_changed(0, 60, 0);
        // A deliberately huge timestep, the sort a stalled server
        // produces.
        for _ in 0..400 {
            sim.step(&world, 0.5);
            if sim.entity_count() == 0 {
                break;
            }
        }
        assert_eq!(world.get(0, 1, 0), BLOCK_SAND, "it fell through the floor");
    }

    #[test]
    fn entities_are_reported_only_to_players_near_them() {
        let world = TestWorld::default();
        world.put(0, 0, 0, BLOCK_STONE);
        world.put(0, 30, 0, BLOCK_SAND);
        let mut sim = FallingBlocks::new();
        sim.on_block_changed(0, 30, 0);
        sim.step(&world, TICK);

        assert_eq!(sim.nearby_states((0.0, 30.0, 0.0), 16.0).len(), 1);
        assert!(
            sim.nearby_states((500.0, 30.0, 500.0), 16.0).is_empty(),
            "a distant player should not be told about it"
        );
    }

    #[test]
    fn a_collapsing_column_keeps_every_block() {
        // Regression. A column becomes one entity per block, and they
        // fall independently; the lowest lands first and fills the cell
        // the next one is heading for. The landing code used to give up
        // at that point and drop the block on the floor of the
        // simulation -- so mining under a five-high sand tower left one
        // block of sand and four that had simply ceased to exist.
        let world = TestWorld::default();
        world.put(0, 0, 0, BLOCK_STONE);
        world.put(0, 1, 0, BLOCK_STONE); // support, about to be mined
        for y in 2..=6 {
            world.put(0, y, 0, BLOCK_SAND);
        }

        let mut sim = FallingBlocks::new();
        world.set(0, 1, 0, BLOCK_AIR);
        sim.on_block_changed(0, 1, 0);
        settle(&mut sim, &world, 500);

        let recovered = (1..=6).filter(|&y| world.get(0, y, 0) == BLOCK_SAND).count();
        assert_eq!(recovered, 5, "all five blocks should still exist");
        assert_eq!(sim.dropped(), 5, "every block should be reported as landed");
        for y in 1..=5 {
            assert_eq!(world.get(0, y, 0), BLOCK_SAND, "gap at y={y}");
        }
    }

    #[test]
    fn a_tall_column_lands_in_its_original_order() {
        // Ten blocks, so several are in the air at once and some are
        // blocked on the same tick. They must stack, not interleave with
        // gaps or overwrite each other.
        let world = TestWorld::default();
        world.put(0, 0, 0, BLOCK_STONE);
        for y in 20..30 {
            world.put(0, y, 0, BLOCK_SAND);
        }

        let mut sim = FallingBlocks::new();
        sim.on_block_changed(0, 20, 0);
        settle(&mut sim, &world, 2000);

        for y in 1..=10 {
            assert_eq!(world.get(0, y, 0), BLOCK_SAND, "gap at y={y}");
        }
        assert_eq!(world.get(0, 11, 0), BLOCK_AIR, "the stack is exactly 10 high");
    }

    #[test]
    fn a_block_placed_under_a_falling_one_mid_flight_does_not_swallow_it() {
        // The cell the entity is *in* was free when it was last checked;
        // by the time it lands someone may have built there. Resting on
        // top of the new block is right; vanishing into it is not.
        let world = TestWorld::default();
        world.put(0, 0, 0, BLOCK_STONE);
        world.put(0, 12, 0, BLOCK_SAND);

        let mut sim = FallingBlocks::new();
        sim.on_block_changed(0, 12, 0);
        for _ in 0..4 {
            sim.step(&world, TICK);
        }
        assert_eq!(sim.entity_count(), 1, "should still be airborne");

        // Build a floor right under it, where it is now.
        let occupied = sim.entities()[0].y.floor() as i32;
        world.put(0, occupied, 0, BLOCK_STONE);
        settle(&mut sim, &world, 500);

        assert_eq!(
            world.get(0, occupied + 1, 0),
            BLOCK_SAND,
            "should have come to rest on top of the new block"
        );
    }

    #[test]
    fn a_block_that_cannot_land_anywhere_is_dropped_rather_than_forced_in() {
        // A sealed pocket: nothing within reach is free. Losing the
        // block is the documented behaviour -- what must not happen is
        // overwriting whatever the player built.
        let world = TestWorld::default();
        for y in 0..(MAX_LANDING_SEARCH + 4) {
            world.put(0, y, 0, BLOCK_STONE);
        }
        let mut sim = FallingBlocks::new();
        sim.entities.push(FallingEntity {
            id: 1,
            block: BLOCK_SAND,
            x: 0.0,
            y: 2.0,
            z: 0.0,
            velocity_y: 0.0,
        });
        sim.step(&world, TICK);

        assert_eq!(sim.entity_count(), 0, "it should have stopped");
        for y in 0..(MAX_LANDING_SEARCH + 4) {
            assert_eq!(world.get(0, y, 0), BLOCK_STONE, "stone at y={y} was overwritten");
        }
    }

    #[test]
    fn work_per_pass_is_bounded() {
        // A big excavation must not turn one tick into an unbounded loop.
        let world = TestWorld::default();
        let mut sim = FallingBlocks::new();
        for i in 0..(MAX_CHECKS_PER_PASS as i32 * 3) {
            world.put(i, 20, 0, BLOCK_SAND);
            sim.on_block_changed(i, 20, 0);
        }
        let before = sim.pending();
        sim.step(&world, TICK);
        assert!(
            sim.pending() > 0,
            "a single pass should not have drained a queue this big"
        );
        assert!(sim.pending() < before + MAX_CHECKS_PER_PASS);
    }

    #[test]
    fn the_queue_cannot_grow_without_limit() {
        let mut sim = FallingBlocks::new();
        for i in 0..(MAX_QUEUE as i32 + 5000) {
            sim.on_block_changed(i, 30, 0);
        }
        assert!(sim.pending() <= MAX_QUEUE);
    }

    #[test]
    fn leaving_the_grid_and_landing_are_both_reported() {
        // Clients need the vacated cell and, later, the filled one --
        // otherwise the block appears to duplicate or vanish.
        let world = TestWorld::default();
        world.put(0, 0, 0, BLOCK_STONE);
        world.put(0, 3, 0, BLOCK_SAND);

        let mut sim = FallingBlocks::new();
        sim.on_block_changed(0, 3, 0);

        let leaving = sim.step(&world, TICK);
        assert_eq!(leaving.len(), 1);
        assert_eq!(leaving[0].block_id, BLOCK_AIR);
        assert_eq!(leaving[0].global_y, 3);

        let rest = settle(&mut sim, &world, 500);
        let landing = rest
            .iter()
            .find(|c| c.block_id == BLOCK_SAND)
            .expect("no landing reported");
        assert_eq!(landing.global_y, 1);
    }
}

