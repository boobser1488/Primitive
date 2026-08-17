//! Dropped stacks lying in the world.
//!
//! ## Why they exist
//!
//! Breaking a block used to credit the breaker's inventory directly.
//! That is simpler, and it quietly rules a lot out: you cannot see what
//! someone dropped, cannot give anything away, cannot mine with a full
//! pack and come back for the rest, and cannot throw anything out. An
//! item in the world is the object all of those need.
//!
//! ## Shape of the simulation
//!
//! Deliberately small. An item falls, lands, waits, and is absorbed by
//! the first player close enough. There is no item-to-item collision, no
//! stacking of nearby drops, and no bouncing: each of those costs a
//! per-pair pass over everything on the ground, and none of them is
//! visible next to a cube the size of a fist.
//!
//! Two bounds keep it finite: items expire, and there is a hard cap on
//! how many can exist at once. Without them a player mining continuously
//! into a full inventory is an unbounded allocation with a network cost
//! attached.

use std::collections::{HashMap, HashSet};
use std::sync::Arc;
use std::time::{Duration, Instant};

use primitive_shared::protocol::{EntityId, EntityKind, EntityState};
use primitive_shared::inventory::MAX_STACK;
use primitive_shared::types::{collision_height, BlockId, Chunk, ChunkPos, CHUNK_SIZE_Y};

use crate::logic::world::World;

/// How long a dropped stack waits before it disappears.
///
/// Long enough to mine out a seam and come back for it, short enough
/// that a busy server is not carrying yesterday's litter.
pub const LIFETIME: Duration = Duration::from_secs(300);

/// How close a player has to be to pick something up, in blocks.
///
/// Measured to the nearest point of the player's *body*, not to a single
/// point on it. Measuring from chest height made the range depend on
/// where the item happened to sit vertically: a block broken underfoot
/// leaves its drop about two metres below the chest, which was outside
/// a range that felt generous at eye level. Clamping to the collider
/// first means an item beside your knee is as collectable as one beside
/// your head, and the number below means what it says.
const PICKUP_RANGE: f32 = 1.4;

/// Nothing at all can be picked up until it has existed this long.
///
/// A drop is a thing in the world, and a thing in the world has to be
/// *there* before it can be taken: mining a seam used to fill the pack
/// with blocks that never visibly existed, because the drop spawned and
/// was absorbed inside the same tick the block broke. Half a second is
/// long enough for the cube to pop out of the hole and land, which is
/// what makes mining read as picking things up rather than as blocks
/// teleporting into a counter.
///
/// It also gives every drop a moment in which it can be *seen* -- and,
/// standing over a full inventory, a moment in which the player can tell
/// that it was left behind.
pub const PICKUP_ARM_DELAY: Duration = Duration::from_millis(500);

/// A newly dropped stack ignores the player who threw it for this long.
///
/// Longer than the arming delay, and for a different reason: without it,
/// throwing something out is instantly undone, because the item spawns
/// on top of the player who dropped it and is picked straight back up.
const PICKUP_DELAY: Duration = Duration::from_millis(1200);

/// The most items that may exist at once.
///
/// A cap rather than a queue: past this, drops are simply not spawned.
/// Losing a block that a player already could not carry is a better
/// failure than an unbounded entity list on a busy server.
pub const MAX_ITEMS: usize = 2048;

const GRAVITY: f32 = -22.0;
const TERMINAL_VELOCITY: f32 = -30.0;
/// Horizontal drag per second while sliding after a landing.
const GROUND_DRAG_PER_SEC: f32 = 0.02;
/// How much speed a bounce keeps. Low: a dropped block should settle,
/// not skitter across the floor.
const BOUNCE: f32 = 0.25;
/// How many cells an embedded item will climb looking for open space.
const UNSTICK_LIMIT: i32 = 4;

/// Side of an item's collider, in blocks. Matches what the client draws
/// -- a point-sized item visibly clips into walls its centre is clear
/// of, which is most of what made drops look wrong.
pub const ITEM_SIZE: f32 = 0.3;
const HALF: f32 = ITEM_SIZE / 2.0;

/// Side of a pickup-grid cell, in blocks.
///
/// The grid exists for the same reason `InterestGrid` does, one layer
/// down: `collect_near` used to walk every item in the world for every
/// player, every tick -- O(players x items) distance tests that nearly
/// all said no. Bucketing items by x/z means a pickup query reads the
/// 3x3 cells around the player and nothing else, and that window is only
/// exact while a cell is at least `PICKUP_RANGE` across -- an item
/// further than one cell away is then further than the range. A test
/// below pins the relationship.
///
/// No y axis, again like `InterestGrid`: the pickup range is under two
/// blocks, so a vertical dimension would triple the bookkeeping to skip
/// items the 3D distance test rejects for free.
const PICKUP_CELL: f32 = 2.0;

// Checked where it cannot be forgotten: if someone widens the pickup
// range past the cell, this is what says why drops would have stopped
// being collectable at the edge of the range.
const _: () = assert!(PICKUP_CELL >= PICKUP_RANGE);

/// How close two drops of the same block have to be to become one.
///
/// Merging is not a nicety. Mining a seam of sixty blocks made sixty
/// separate entities, each drawn as its own cube and each sent to every
/// nearby player twenty times a second -- a heap that looked like litter
/// and cost like a crowd.
const MERGE_RANGE: f32 = 0.9;

pub struct Item {
    pub id: EntityId,
    pub block: BlockId,
    pub count: u32,
    pub position: (f32, f32, f32),
    velocity: (f32, f32, f32),
    spawned_at: Instant,
    /// Who dropped it, if anyone. See `PICKUP_DELAY`.
    dropped_by: Option<u64>,
    resting: bool,
}

impl Item {
    pub fn state(&self) -> EntityState {
        EntityState {
            id: self.id,
            kind: EntityKind::Item {
                block: self.block,
                count: self.count,
            },
            x: self.position.0,
            y: self.position.1,
            z: self.position.2,
        }
    }

    fn can_be_picked_up_by(&self, player: u64, now: Instant) -> bool {
        let age = now.duration_since(self.spawned_at);
        let wait = match self.dropped_by {
            // Yours for longer than anyone else's, so a throw is not
            // undone by the act of walking away from it.
            Some(owner) if owner == player => PICKUP_DELAY,
            _ => PICKUP_ARM_DELAY,
        };
        age >= wait
    }

    /// Whether this drop is too new to take part in anything.
    ///
    /// Merging has to respect the delays as well as pickup does: a stack
    /// folded into an older pile inherits that pile's age and owner, and
    /// can be picked straight back up -- which is exactly what the two
    /// delays above exist to stop. A drop this new is also still in the
    /// air, so there is nothing for it to pile onto yet.
    fn is_settling(&self, now: Instant) -> bool {
        let age = now.duration_since(self.spawned_at);
        age < PICKUP_ARM_DELAY || (self.dropped_by.is_some() && age < PICKUP_DELAY)
    }
}

#[derive(Default)]
pub struct Items {
    items: Vec<Item>,
    next_id: u64,
    spawned: u64,
    collected: u64,
    expired: u64,
    /// Whether last tick still had settling items. Kept one tick so the
    /// merge pass runs once more *after* the last settle delay expires --
    /// that transition tick is exactly when a pile becomes mergeable, and
    /// gating on the current tick alone would skip it forever.
    had_settling: bool,
    /// cell -> indices into `items`, at `PICKUP_CELL` resolution. See
    /// that constant for why. Rebuilt whenever indices could have gone
    /// stale -- after the physics step, and after a pickup that emptied
    /// a stack -- and appended to on spawn, which is the one mutation
    /// that never moves anything already indexed.
    cells: HashMap<(i32, i32), Vec<u32>>,
}

/// The pickup-grid cell under a point.
#[inline]
fn pickup_cell(x: f32, z: f32) -> (i32, i32) {
    (
        (x / PICKUP_CELL).floor() as i32,
        (z / PICKUP_CELL).floor() as i32,
    )
}

impl Items {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn len(&self) -> usize {
        self.items.len()
    }

    pub fn is_empty(&self) -> bool {
        self.items.is_empty()
    }

    pub fn stats(&self) -> (u64, u64, u64) {
        (self.spawned, self.collected, self.expired)
    }

    pub fn iter(&self) -> impl Iterator<Item = &Item> {
        self.items.iter()
    }

    pub fn states(&self) -> Vec<EntityState> {
        self.items.iter().map(|item| item.state()).collect()
    }

    /// Drops a stack, `at` being the **centre** of the item.
    ///
    /// Returns false if the cap refused it.
    pub fn spawn(
        &mut self,
        block: BlockId,
        count: u32,
        at: (f32, f32, f32),
        direction: (f32, f32, f32),
        dropped_by: Option<u64>,
        now: Instant,
    ) -> bool {
        if count == 0 || self.items.len() >= MAX_ITEMS {
            return false;
        }
        self.next_id += 1;
        self.spawned += 1;
        self.items.push(Item {
            id: self.next_id,
            block,
            count,
            position: at,
            // A gentle lob. The first numbers threw everything several
            // blocks, so a broken block sailed off the ledge you mined
            // it from and a thrown stack landed out of reach.
            velocity: (direction.0 * 2.2, direction.1 * 2.2 + 1.4, direction.2 * 2.2),
            spawned_at: now,
            dropped_by,
            resting: false,
        });
        // Indexed straight away rather than waiting for the next step:
        // spawns happen from connection tasks between steps, and the
        // cell snapshot the tick loop filters players by must not be
        // blind to them.
        self.cells
            .entry(pickup_cell(at.0, at.2))
            .or_default()
            .push((self.items.len() - 1) as u32);
        true
    }

    /// Advances the whole simulation one tick.
    pub fn step(&mut self, world: &World, dt: f32, now: Instant) {
        let mut any_moving = false;
        for item in &mut self.items {
            step_one(item, world, dt);
            any_moving |= !item.resting;
        }
        // A world of settled piles is the common case, and rebuilding
        // the merge buckets for it every tick found nothing every time:
        // resting items past their settle delay were all offered to each
        // other the tick that delay expired. Merging can only become
        // possible again when something moves, lands, or finishes
        // settling.
        let settling_now = self.items.iter().any(|item| item.is_settling(now));
        if any_moving || settling_now || self.had_settling {
            self.merge_nearby(now);
        }
        self.had_settling = settling_now;
        let before = self.items.len();
        self.items
            .retain(|item| now.duration_since(item.spawned_at) < LIFETIME);
        self.expired += (before - self.items.len()) as u64;
        // Everything above may have moved items or shifted indices;
        // one rebuild covers all of it. O(items), once per tick --
        // which is the budget the whole grid exists to enforce.
        self.rebuild_cells();
    }

    /// Reindexes every item into its pickup cell.
    fn rebuild_cells(&mut self) {
        self.cells.clear();
        for (index, item) in self.items.iter().enumerate() {
            self.cells
                .entry(pickup_cell(item.position.0, item.position.2))
                .or_default()
                .push(index as u32);
        }
    }

    /// The set of cells that currently hold any item at all.
    ///
    /// Taken once per tick by the tick loop, so the per-player pickup
    /// pass can skip the items mutex -- and the player's own state lock
    /// -- for everyone standing nowhere near a drop, which on a large
    /// server is nearly everyone.
    pub fn occupied_cells(&self) -> HashSet<(i32, i32)> {
        self.cells.keys().copied().collect()
    }

    /// Whether a player standing at `feet` is close enough to any of
    /// these cells for a pickup to be possible. The 3x3 window around
    /// the player's cell, i.e. the same cells `collect_near` would read.
    pub fn any_within_reach(feet: (f32, f32, f32), occupied: &HashSet<(i32, i32)>) -> bool {
        let (cx, cz) = pickup_cell(feet.0, feet.2);
        for dz in -1..=1 {
            for dx in -1..=1 {
                if occupied.contains(&(cx + dx, cz + dz)) {
                    return true;
                }
            }
        }
        false
    }

    /// Folds drops of the same block that have come to rest together.
    ///
    /// Mining a seam of sixty blocks otherwise leaves sixty entities:
    /// sixty cubes drawn, sixty states sent to every nearby player
    /// twenty times a second, and a heap that reads as litter rather
    /// than as a pile of stone.
    ///
    /// Bucketed by position rather than compared pairwise. The pairwise
    /// version is the obvious one and it is quadratic, which is exactly
    /// the wrong shape for the case that matters -- a lot of items in
    /// one place.
    fn merge_nearby(&mut self, now: Instant) {
        if self.items.len() < 2 {
            return;
        }
        use std::collections::HashMap;
        // One bucket per merge-range cube. Two items in the same bucket
        // are close enough to consider; anything further apart cannot be
        // in it.
        let mut buckets: HashMap<(i32, i32, i32, BlockId), usize> = HashMap::new();
        let mut merged_any = false;

        for index in 0..self.items.len() {
            let item = &self.items[index];
            // A stack that is still settling takes no part: merging it
            // into an older pile would hand it the older pile's age, and
            // the delays are the whole point.
            if item.count == 0 || item.is_settling(now) {
                continue;
            }
            let key = (
                (item.position.0 / MERGE_RANGE).floor() as i32,
                (item.position.1 / MERGE_RANGE).floor() as i32,
                (item.position.2 / MERGE_RANGE).floor() as i32,
                item.block,
            );
            match buckets.get(&key).copied() {
                Some(into) if into != index => {
                    let room = MAX_STACK.saturating_sub(self.items[into].count);
                    let moved = room.min(self.items[index].count);
                    if moved > 0 {
                        self.items[into].count += moved;
                        self.items[index].count -= moved;
                        merged_any = true;
                    }
                    // A stack that filled up stops being the target, so
                    // the next one starts a fresh pile instead of
                    // silently failing to merge forever.
                    if self.items[into].count >= MAX_STACK {
                        buckets.insert(key, index);
                    }
                }
                _ => {
                    buckets.insert(key, index);
                }
            }
        }

        if merged_any {
            self.items.retain(|item| item.count > 0);
        }
    }

    /// Offers everything near a player to a callback, which returns how
    /// many of the stack it took.
    ///
    /// Written as a callback rather than returning a list because the
    /// caller holds the player's inventory lock: handing back items to
    /// be collected in a second pass means either holding that lock
    /// across the whole sweep or doing the distance work twice.
    pub fn collect_near<F>(
        &mut self,
        player: u64,
        feet: (f32, f32, f32),
        now: Instant,
        mut take: F,
    ) where
        F: FnMut(BlockId, u32) -> u32,
    {
        let range_sq = PICKUP_RANGE * PICKUP_RANGE;
        // Only the 3x3 cells around the player, not the whole world --
        // see `PICKUP_CELL` for why that window loses nothing. The
        // distance test inside is unchanged.
        let mut emptied_any = false;
        let (cx, cz) = pickup_cell(feet.0, feet.2);
        for dz in -1..=1 {
            for dx in -1..=1 {
                let Some(bucket) = self.cells.get(&(cx + dx, cz + dz)) else {
                    continue;
                };
                for index in bucket {
                    let item = &mut self.items[*index as usize];
                    if item.count == 0 || !item.can_be_picked_up_by(player, now) {
                        continue;
                    }
                    // Nearest point of the collider, then the distance
                    // to that.
                    let nearest_y = item
                        .position
                        .1
                        .clamp(feet.1, feet.1 + primitive_shared::geometry::PLAYER_HEIGHT);
                    let (dx, dy, dz) = (
                        item.position.0 - feet.0,
                        item.position.1 - nearest_y,
                        item.position.2 - feet.2,
                    );
                    if dx * dx + dy * dy + dz * dz > range_sq {
                        continue;
                    }
                    let taken = take(item.block, item.count).min(item.count);
                    item.count -= taken;
                    if taken > 0 {
                        self.collected += 1;
                    }
                    emptied_any |= item.count == 0;
                }
            }
        }
        // A partly collected stack stays; an emptied one goes -- and
        // removal shifts indices, so the grid is rebuilt with it. Only
        // when something was actually emptied: the common case is a
        // player walking past drops they cannot take, and that must
        // stay free.
        if emptied_any {
            self.items.retain(|item| item.count > 0);
            self.rebuild_cells();
        }
    }
}

fn step_one(item: &mut Item, world: &World, dt: f32) {
    if item.resting && item.velocity.1 == 0.0 {
        // Still settled, and the ground has not moved. Skipping the
        // whole integration here is what keeps a field of dropped blocks
        // from costing anything per tick.
        if supported(world, item.position) && !blocked(world, item.position) {
            return;
        }
        item.resting = false;
    }

    // An item that has ended up inside something -- a block placed over
    // it, a landing that clipped a corner -- climbs out rather than
    // staying there forever.
    //
    // Without this a stuck drop is unreachable *and* immortal for its
    // whole five-minute lifetime, and since every one of them is sent to
    // every nearby player twenty times a second, a shaft full of them is
    // a real cost as well as a visible fault.
    if blocked(world, item.position) {
        unstick(item, world);
        return;
    }

    item.velocity.1 = (item.velocity.1 + GRAVITY * dt).max(TERMINAL_VELOCITY);

    // Axis at a time against the item's real box.
    //
    // Testing a single point was the earlier version, and it is wrong in
    // a way that is easy to see: the item is drawn as a cube three
    // tenths of a block across, so its centre can be clear of a wall
    // while a third of it is buried in one. Sweeping the box means what
    // collides is what is drawn.
    let step = (
        item.velocity.0 * dt,
        item.velocity.1 * dt,
        item.velocity.2 * dt,
    );

    let try_x = (item.position.0 + step.0, item.position.1, item.position.2);
    if blocked(world, try_x) {
        item.velocity.0 = -item.velocity.0 * BOUNCE;
    } else {
        item.position = try_x;
    }

    let try_z = (item.position.0, item.position.1, item.position.2 + step.2);
    if blocked(world, try_z) {
        item.velocity.2 = -item.velocity.2 * BOUNCE;
    } else {
        item.position = try_z;
    }

    let try_y = (item.position.0, item.position.1 + step.1, item.position.2);
    if blocked(world, try_y) {
        if item.velocity.1 <= 0.0 {
            // Landed. Sit exactly on top of whatever stopped it, so the
            // cube rests on the surface rather than half through it.
            item.position.1 = (try_y.1 - HALF).floor() + 1.0 + HALF;
            if item.velocity.1 < -3.0 {
                // A real drop bounces once rather than stopping dead.
                item.velocity.1 *= -BOUNCE;
            } else {
                item.velocity = (0.0, 0.0, 0.0);
                item.resting = true;
            }
        } else {
            item.velocity.1 = 0.0; // clipped a ceiling
        }
    } else {
        item.position = try_y;
    }

    let drag = GROUND_DRAG_PER_SEC.powf(dt);
    item.velocity.0 *= drag;
    item.velocity.2 *= drag;
}

/// Lifts an item out of the block it is inside.
///
/// Upwards, and only a few cells: straight up is the direction with
/// open sky at the end of it, and a bounded search means a drop sealed
/// into bedrock stops costing anything rather than looping.
fn unstick(item: &mut Item, world: &World) {
    item.velocity = (0.0, 0.0, 0.0);
    for _ in 0..UNSTICK_LIMIT {
        item.position.1 += 1.0;
        if !solid_at(world, item.position) {
            item.resting = false;
            return;
        }
    }
    // Buried past reach. Leave it where it is; the lifetime will clear
    // it, and it is not worth more work than that.
    item.resting = true;
}

fn solid_at(world: &World, at: (f32, f32, f32)) -> bool {
    let y = at.1.floor() as i32;
    match world.cached_block(at.0.floor() as i32, y, at.2.floor() as i32) {
        // Inside the block, not merely in its cell: a layer of soil
        // fills part of one, and a point above the layer is in the air.
        Some(block) => at.1 < y as f32 + collision_height(block),
        None => false,
    }
}

/// Whether an item centred here would overlap anything solid.
///
/// Every cell the box touches, not just the one its centre is in.
fn blocked(world: &World, centre: (f32, f32, f32)) -> bool {
    // A hair inside the faces, so an item resting exactly on a surface
    // does not read as intersecting it.
    let e = HALF - 1e-3;
    let (x0, y0, z0) = (
        (centre.0 - e).floor() as i32,
        (centre.1 - e).floor() as i32,
        (centre.2 - e).floor() as i32,
    );
    let (x1, y1, z1) = (
        (centre.0 + e).floor() as i32,
        (centre.1 + e).floor() as i32,
        (centre.2 + e).floor() as i32,
    );
    // Chunk resolved once per column rather than per cell: the eight
    // cells of the box are almost always in one chunk, and going through
    // `cached_block` for each was eight shard locks and hash lookups per
    // call -- for every resting item, every tick.
    let mut cache: Option<(ChunkPos, Option<Arc<Chunk>>)> = None;
    for z in z0..=z1 {
        for x in x0..=x1 {
            let (pos, lx, lz) = ChunkPos::from_global(x, z);
            if !matches!(&cache, Some((cached, _)) if *cached == pos) {
                cache = Some((pos, world.cached(pos)));
            }
            let Some((_, Some(chunk))) = &cache else {
                continue;
            };
            for y in y0..=y1 {
                if y < 0 || y as usize >= CHUNK_SIZE_Y {
                    continue;
                }
                // How much of the cell the block fills is part of the
                // answer now that loose material comes in layers: an
                // item dropped on a dusting of snow should lie *on* it,
                // not float at the top of its cell.
                let top = y as f32 + collision_height(chunk.get(lx, y as usize, lz));
                if top > y as f32 && centre.1 - e < top {
                    return true;
                }
            }
        }
    }
    false
}

/// Whether there is something directly under a resting item.
fn supported(world: &World, at: (f32, f32, f32)) -> bool {
    blocked(world, (at.0, at.1 - 0.05, at.2))
}

#[cfg(test)]
mod tests {
    use super::*;
    use primitive_shared::types::{BLOCK_DIRT, BLOCK_STONE};

    fn now() -> Instant {
        Instant::now()
    }

    /// A spawn timestamp old enough that the drop is collectable by
    /// anyone who did not throw it.
    ///
    /// Most of these tests are about collision, merging or collection
    /// rather than about the delay, and a drop that spawned this instant
    /// is not collectable by anybody -- see `PICKUP_ARM_DELAY`.
    fn settled() -> Instant {
        Instant::now() - PICKUP_ARM_DELAY - Duration::from_millis(50)
    }

    /// A world with nothing in it; items fall forever.
    fn empty_world() -> World {
        World::new(1, 256)
    }

    /// A world with a solid floor at y = 0..=9 and a wall at x >= 8.
    fn walled_world() -> World {
        use primitive_shared::types::{Chunk, ChunkPos, BLOCK_AIR, BLOCK_STONE, CHUNK_VOLUME};
        let world = World::new(1, 256);
        let mut blocks = vec![BLOCK_AIR; CHUNK_VOLUME];
        for y in 0..10 {
            for z in 0..16 {
                for x in 0..16 {
                    blocks[Chunk::index(x, y, z)] = BLOCK_STONE;
                }
            }
        }
        for y in 10..16 {
            for z in 0..16 {
                for x in 8..16 {
                    blocks[Chunk::index(x, y, z)] = BLOCK_STONE;
                }
            }
        }
        world.insert(Chunk {
            pos: ChunkPos::new(0, 0),
            blocks,
        });
        world
    }

    #[test]
    fn a_dropped_stack_appears_in_the_world() {
        let mut items = Items::new();
        assert!(items.is_empty());
        assert!(items.spawn(BLOCK_STONE, 3, (0.0, 20.0, 0.0), (0.0, 0.0, 0.0), None, now()));
        assert_eq!(items.len(), 1);

        let states = items.states();
        assert_eq!(states.len(), 1);
        assert_eq!(
            states[0].kind,
            EntityKind::Item {
                block: BLOCK_STONE,
                count: 3
            }
        );
    }

    #[test]
    fn dropping_nothing_is_not_a_drop() {
        let mut items = Items::new();
        assert!(!items.spawn(BLOCK_STONE, 0, (0.0, 0.0, 0.0), (0.0, 0.0, 0.0), None, now()));
        assert!(items.is_empty());
    }

    #[test]
    fn items_fall() {
        let world = empty_world();
        let mut items = Items::new();
        items.spawn(BLOCK_STONE, 1, (0.0, 40.0, 0.0), (0.0, 0.0, 0.0), None, now());
        let start = items.iter().next().unwrap().position.1;
        for _ in 0..30 {
            items.step(&world, 1.0 / 20.0, now());
        }
        assert!(
            items.iter().next().unwrap().position.1 < start,
            "the drop hung in the air"
        );
    }

    #[test]
    fn a_walking_player_picks_things_up() {
        let mut items = Items::new();
        items.spawn(BLOCK_STONE, 4, (0.0, 1.0, 0.0), (0.0, 0.0, 0.0), None, settled());

        let mut got = 0;
        items.collect_near(1, (0.0, 0.5, 0.0), now(), |_, count| {
            got += count;
            count
        });
        assert_eq!(got, 4);
        assert!(items.is_empty(), "a fully collected stack stayed in the world");
    }

    #[test]
    fn nothing_can_be_picked_up_the_instant_it_appears() {
        // Mining a seam used to fill the pack with blocks that never
        // visibly existed: the drop spawned and was absorbed on the same
        // tick the block broke. A drop has to be *there* first.
        let mut items = Items::new();
        let dropped = now();
        items.spawn(BLOCK_STONE, 4, (0.0, 1.0, 0.0), (0.0, 0.0, 0.0), None, dropped);

        let mut got = 0;
        let collect = |items: &mut Items, at: Instant, got: &mut u32| {
            items.collect_near(1, (0.0, 0.5, 0.0), at, |_, count| {
                *got += count;
                count
            });
        };
        collect(&mut items, dropped, &mut got);
        assert_eq!(got, 0, "a drop was absorbed the moment it appeared");
        collect(&mut items, dropped + PICKUP_ARM_DELAY / 2, &mut got);
        assert_eq!(got, 0, "the delay ran out early");

        collect(
            &mut items,
            dropped + PICKUP_ARM_DELAY + Duration::from_millis(1),
            &mut got,
        );
        assert_eq!(got, 4, "the drop never became collectable");
    }

    #[test]
    fn a_distant_player_picks_up_nothing() {
        let mut items = Items::new();
        items.spawn(BLOCK_STONE, 4, (0.0, 1.0, 0.0), (0.0, 0.0, 0.0), None, now());
        let mut got = 0;
        items.collect_near(1, (40.0, 1.0, 40.0), now(), |_, count| {
            got += count;
            count
        });
        assert_eq!(got, 0);
        assert_eq!(items.len(), 1);
    }

    #[test]
    fn a_full_pack_leaves_the_rest_on_the_ground() {
        // The whole reason `collect_near` asks how much was taken: an
        // inventory that could only fit two of a stack of five must
        // leave three lying there rather than eating them.
        let mut items = Items::new();
        items.spawn(BLOCK_STONE, 5, (0.0, 1.0, 0.0), (0.0, 0.0, 0.0), None, settled());
        items.collect_near(1, (0.0, 0.5, 0.0), now(), |_, _| 2);
        assert_eq!(items.len(), 1, "the remainder vanished");
        assert_eq!(items.iter().next().unwrap().count, 3);
    }

    #[test]
    fn you_do_not_instantly_pick_up_what_you_just_threw() {
        // Otherwise throwing something out is undone on the same tick:
        // it spawns on top of the thrower.
        let mut items = Items::new();
        let spawned = now();
        items.spawn(BLOCK_DIRT, 1, (0.0, 1.0, 0.0), (0.0, 0.0, 0.0), Some(7), spawned);

        // Long enough for anyone else, not long enough for the thrower.
        let later = spawned + PICKUP_ARM_DELAY + Duration::from_millis(10);
        assert!(later < spawned + PICKUP_DELAY, "the two delays should differ");

        let mut got = 0;
        items.collect_near(7, (0.0, 0.5, 0.0), later, |_, c| {
            got += c;
            c
        });
        assert_eq!(got, 0, "the thrower picked it straight back up");

        items.collect_near(9, (0.0, 0.5, 0.0), later, |_, c| {
            got += c;
            c
        });
        assert_eq!(got, 1, "another player could not pick up a settled drop");
    }

    #[test]
    fn the_thrower_can_take_it_back_after_a_moment() {
        let mut items = Items::new();
        let spawned = Instant::now() - PICKUP_DELAY - Duration::from_millis(50);
        items.spawn(BLOCK_DIRT, 1, (0.0, 1.0, 0.0), (0.0, 0.0, 0.0), Some(7), spawned);
        let mut got = 0;
        items.collect_near(7, (0.0, 0.5, 0.0), Instant::now(), |_, c| {
            got += c;
            c
        });
        assert_eq!(got, 1);
    }

    #[test]
    fn litter_expires() {
        let world = empty_world();
        let mut items = Items::new();
        let long_ago = Instant::now() - LIFETIME - Duration::from_secs(1);
        items.spawn(BLOCK_STONE, 1, (0.0, 1.0, 0.0), (0.0, 0.0, 0.0), None, long_ago);
        items.step(&world, 1.0 / 20.0, Instant::now());
        assert!(items.is_empty(), "an ancient drop is still there");
        assert_eq!(items.stats().2, 1);
    }

    #[test]
    fn an_item_thrown_at_a_wall_stays_out_of_it() {
        // The first version only resolved the vertical axis, so anything
        // thrown horizontally walked straight into the wall and settled
        // inside it -- unreachable, and alive for its whole lifetime.
        let world = walled_world();
        let mut items = Items::new();
        items.spawn(
            BLOCK_STONE,
            1,
            (4.0, 11.0, 4.0),
            (1.0, 0.0, 0.0),
            None,
            now(),
        );
        for _ in 0..200 {
            items.step(&world, 1.0 / 20.0, now());
        }
        let item = items.iter().next().expect("the drop vanished");
        assert!(
            !blocked(&world, item.position),
            "the drop ended up inside a block at {:?}",
            item.position
        );
    }

    #[test]
    fn drops_of_the_same_block_pile_into_one() {
        // Sixty entities for a mined seam is both litter on screen and a
        // crowd on the wire.
        let world = walled_world();
        let mut items = Items::new();
        for i in 0..30 {
            items.spawn(
                BLOCK_STONE,
                1,
                (4.0 + (i % 3) as f32 * 0.1, 11.0, 4.0),
                (0.0, 0.0, 0.0),
                None,
                settled(),
            );
        }
        assert_eq!(items.len(), 30);
        for _ in 0..60 {
            items.step(&world, 1.0 / 20.0, now());
        }
        assert!(items.len() < 5, "still {} separate drops", items.len());
        let total: u32 = items.iter().map(|i| i.count).sum();
        assert_eq!(total, 30, "merging lost or invented blocks");
    }

    #[test]
    fn a_stack_just_thrown_does_not_merge_into_an_older_pile() {
        // Otherwise throwing something out where drops of the same block
        // already lie undoes itself: the thrown stack folds into a pile
        // that nobody owns, and is picked straight back up on the next
        // tick.
        let world = walled_world();
        let mut items = Items::new();
        let now = Instant::now();
        items.spawn(BLOCK_STONE, 4, (4.0, 11.0, 4.0), (0.0, 0.0, 0.0), None, settled());
        for _ in 0..40 {
            items.step(&world, 1.0 / 20.0, now);
        }
        // ...and now a player throws more of the same onto it.
        items.spawn(BLOCK_STONE, 2, (4.0, 11.0, 4.0), (0.0, 0.0, 0.0), Some(7), now);
        items.step(&world, 1.0 / 20.0, now);

        let mut got = 0;
        items.collect_near(7, (4.0, 10.0, 4.0), now, |_, count| {
            got += count;
            count
        });
        assert_eq!(got, 4, "the thrower picked up what they had just thrown");
        assert_eq!(
            items.iter().map(|i| i.count).sum::<u32>(),
            2,
            "the thrown stack was not left behind"
        );
    }

    #[test]
    fn different_blocks_never_merge() {
        let world = walled_world();
        let mut items = Items::new();
        items.spawn(BLOCK_STONE, 5, (4.0, 11.0, 4.0), (0.0, 0.0, 0.0), None, settled());
        items.spawn(BLOCK_DIRT, 5, (4.0, 11.0, 4.0), (0.0, 0.0, 0.0), None, settled());
        for _ in 0..60 {
            items.step(&world, 1.0 / 20.0, now());
        }
        assert_eq!(items.len(), 2, "two kinds of block became one stack");
    }

    #[test]
    fn merging_respects_the_stack_limit() {
        let world = walled_world();
        let mut items = Items::new();
        for _ in 0..4 {
            items.spawn(
                BLOCK_STONE,
                MAX_STACK / 2,
                (4.0, 11.0, 4.0),
                (0.0, 0.0, 0.0),
                None,
                settled(),
            );
        }
        for _ in 0..60 {
            items.step(&world, 1.0 / 20.0, now());
        }
        let total: u32 = items.iter().map(|i| i.count).sum();
        assert_eq!(total, MAX_STACK * 2, "merging lost blocks");
        for item in items.iter() {
            assert!(item.count <= MAX_STACK, "a pile grew past a stack");
        }
    }

    #[test]
    fn an_item_that_ends_up_inside_a_block_climbs_out() {
        // However it got there -- a block placed over it, a landing that
        // clipped a corner -- it must not stay: a stuck drop is both
        // unreachable and, since every one is sent to every nearby
        // player twenty times a second, not free.
        let world = walled_world();
        let mut items = Items::new();
        items.spawn(BLOCK_STONE, 1, (10.0, 12.0, 4.0), (0.0, 0.0, 0.0), None, now());
        assert!(solid_at(&world, items.iter().next().unwrap().position));

        for _ in 0..40 {
            items.step(&world, 1.0 / 20.0, now());
        }
        let item = items.iter().next().expect("the drop vanished");
        assert!(
            !solid_at(&world, item.position),
            "still buried at {:?}",
            item.position
        );
    }

    #[test]
    fn a_buried_item_gives_up_rather_than_looping() {
        // Sealed in solid rock with no open cell within reach. It must
        // stop costing anything and wait out its lifetime.
        use primitive_shared::types::{Chunk, ChunkPos, BLOCK_STONE as ROCK, CHUNK_VOLUME};
        let world = World::new(1, 256);
        world.insert(Chunk {
            pos: ChunkPos::new(0, 0),
            blocks: vec![ROCK; CHUNK_VOLUME],
        });
        let mut items = Items::new();
        items.spawn(BLOCK_STONE, 1, (4.0, 20.0, 4.0), (0.0, 0.0, 0.0), None, now());
        for _ in 0..100 {
            items.step(&world, 1.0 / 20.0, now());
        }
        assert_eq!(items.len(), 1, "it should still exist, just stuck");
    }

    #[test]
    fn a_landed_item_comes_to_rest_on_top_of_the_ground() {
        let world = walled_world();
        let mut items = Items::new();
        items.spawn(BLOCK_STONE, 1, (4.0, 14.0, 4.0), (0.0, 0.0, 0.0), None, now());
        for _ in 0..200 {
            items.step(&world, 1.0 / 20.0, now());
        }
        let item = items.iter().next().expect("the drop vanished");
        // `position` is the centre of the cube, so resting on a floor at
        // y = 10 puts the centre half an item above it.
        let bottom = item.position.1 - ITEM_SIZE / 2.0;
        assert!(
            (bottom - 10.0).abs() < 0.01,
            "its underside rested at {bottom} instead of on the floor"
        );
        assert!(!blocked(&world, item.position));
    }

    #[test]
    fn an_item_collides_as_the_cube_it_is_drawn_as() {
        // A point-sized collider lets the visible cube bury a third of
        // itself in a wall its centre is clear of, which is most of what
        // made drops look wrong.
        let world = walled_world();
        // Centre just clear of the wall at x = 8, but the box is not.
        let centre = (8.0 - ITEM_SIZE / 4.0, 12.0, 4.0);
        assert!(!solid_at(&world, centre), "the centre is outside the wall");
        assert!(blocked(&world, centre), "but the box overlaps it");
    }

    #[test]
    fn the_item_count_is_bounded() {
        // A player mining into a full pack must not be an unbounded
        // allocation with a network cost attached.
        let mut items = Items::new();
        for _ in 0..(MAX_ITEMS + 100) {
            items.spawn(BLOCK_STONE, 1, (0.0, 1.0, 0.0), (0.0, 0.0, 0.0), None, now());
        }
        assert_eq!(items.len(), MAX_ITEMS);
    }

    #[test]
    fn the_pickup_grid_agrees_with_the_linear_scan_it_replaced() {
        // The grid is an optimisation over a scan of every item, so any
        // disagreement is a bug in the optimisation. The positions are
        // chosen to sit just inside and just outside the range, across
        // cell seams, on negative coordinates, and above the player's
        // head where the collider clamp decides the answer.
        let feet = (1.95f32, 10.0f32, -0.05f32); // next to two cell seams
        let positions: Vec<(f32, f32, f32)> = vec![
            (1.95, 10.0, -0.05),  // underfoot
            (2.05, 10.0, -0.05),  // across the x seam, well inside
            (1.95, 10.0, 0.05),   // across the z seam, well inside
            (3.30, 10.0, -0.05),  // just inside, one cell over
            (3.40, 10.0, -0.05),  // just outside, same cell as the last
            (0.60, 10.0, -0.05),  // just inside on the other side
            (0.50, 10.0, -0.05),  // just outside
            (1.95, 11.0, -0.05),  // beside the body, clamped vertically
            (1.95, 14.0, -0.05),  // above the head, out of reach
            (-2.60, 10.0, -2.60), // a diagonal cell away, far outside
            (30.0, 10.0, 30.0),   // nowhere near
        ];
        let mut items = Items::new();
        for &at in &positions {
            assert!(items.spawn(BLOCK_STONE, 1, at, (0.0, 0.0, 0.0), None, settled()));
        }

        // What the old linear scan would have taken: the same distance
        // test, applied to everything.
        let range_sq = PICKUP_RANGE * PICKUP_RANGE;
        let expected = positions
            .iter()
            .filter(|p| {
                let nearest_y = p.1.clamp(feet.1, feet.1 + primitive_shared::geometry::PLAYER_HEIGHT);
                let (dx, dy, dz) = (p.0 - feet.0, p.1 - nearest_y, p.2 - feet.2);
                dx * dx + dy * dy + dz * dz <= range_sq
            })
            .count();
        assert!(expected > 0, "the test lost its teeth: nothing is in range");
        assert!(
            expected < positions.len(),
            "the test lost its teeth: everything is in range"
        );

        let mut got = 0usize;
        items.collect_near(1, feet, now(), |_, count| {
            got += count as usize;
            count
        });
        assert_eq!(got, expected, "the grid and the scan disagree on what is in range");
        assert_eq!(
            items.len(),
            positions.len() - expected,
            "the leftovers disagree too"
        );
    }

    #[test]
    fn the_cell_snapshot_never_hides_a_collectable_drop() {
        // The tick loop skips the items mutex for players whose 3x3
        // neighbourhood holds no cells at all, so the snapshot has to
        // say yes for anyone `collect_near` could possibly serve --
        // including for a drop spawned this tick, before any step has
        // rebuilt the index.
        let mut items = Items::new();
        items.spawn(BLOCK_STONE, 1, (0.0, 1.0, 0.0), (0.0, 0.0, 0.0), None, settled());
        let occupied = items.occupied_cells();
        assert!(Items::any_within_reach((0.5, 0.5, 0.5), &occupied));
        assert!(Items::any_within_reach((-1.0, 0.5, 1.0), &occupied));
        assert!(
            !Items::any_within_reach((40.0, 0.5, 40.0), &occupied),
            "a player nowhere near a drop should be filtered out"
        );

        // ...and after the physics step has reindexed everything.
        let world = empty_world();
        items.step(&world, 1.0 / 20.0, now());
        let occupied = items.occupied_cells();
        assert!(!occupied.is_empty(), "the step lost the index");
    }

    #[test]
    fn every_item_has_its_own_id() {
        let mut items = Items::new();
        for _ in 0..50 {
            items.spawn(BLOCK_STONE, 1, (0.0, 1.0, 0.0), (0.0, 0.0, 0.0), None, now());
        }
        let mut ids: Vec<EntityId> = items.iter().map(|i| i.id).collect();
        ids.sort_unstable();
        let before = ids.len();
        ids.dedup();
        assert_eq!(ids.len(), before, "two items share an id");
    }
}
