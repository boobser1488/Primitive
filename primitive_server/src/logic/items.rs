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

use primitive_shared::protocol::{entity_id, EntityId, EntityKind, EntitySource, EntityState};
use primitive_shared::packed::PackedChunk;
use primitive_shared::types::{BlockId, ChunkPos, CHUNK_SIZE_Y};

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
/// How much of its speed a thing in water keeps, per second.
///
/// High, because water is thick and the alternative is a floating log
/// that oscillates about the waterline until it despawns. Together with
/// the buoyancy above this settles a cork in about a second.
const WATER_DRAG_PER_SEC: f32 = 4.0;
/// Extra vertical damping, per second, while an item is only partly under.
///
/// The lift at the waterline is a spring (see `step_one`): a log pushed a
/// finger's width down gets back about eleven radians a second, and the
/// water's own drag alone -- scaled to the wet six tenths -- is a fifth of
/// what it takes to stop that ringing. It bobbed a dozen times before it
/// lay still. This makes it a little over half critical: one dip and a
/// rise, settled in half a second, which reads as a thing landing on water
/// rather than as a thing on a spring.
const SURFACE_DRAG_PER_SEC: f32 = 10.0;

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
    /// How worn it is, in the units `inventory::Stack::damage` counts.
    ///
    /// **Wear is a fact about the object, and the ground is not a
    /// repair bench.** A drop used to carry only a block and a count, so
    /// a half-spent axe thrown down and picked straight back up came
    /// back new -- a free repair for two clicks, and the same one
    /// through a broken chest, which spills its slots down this path.
    /// The inventory has fought exactly this three times already (see
    /// `Inventory::split_into`, `quick_move` and `tidy`); this is the
    /// fourth place an object could change on the way past.
    ///
    /// Not on the wire: `EntityKind::Item` carries what to draw, and a
    /// stack lying in the grass is drawn as its block whatever state it
    /// is in.
    pub damage: u32,
    pub position: (f64, f64, f64),
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
    /// How many stacks this simulation has ever put on the ground.
    /// **Not the id they are replicated under** -- see
    /// `protocol::EntitySource`.
    next_ordinal: u64,
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
fn pickup_cell(x: f64, z: f64) -> (i32, i32) {
    (
        (x / f64::from(PICKUP_CELL)).floor() as i32,
        (z / f64::from(PICKUP_CELL)).floor() as i32,
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
        at: (f64, f64, f64),
        direction: (f32, f32, f32),
        dropped_by: Option<u64>,
        now: Instant,
    ) -> bool {
        self.spawn_worn(block, count, 0, at, direction, dropped_by, now)
    }

    /// The same, for something that has already been used.
    ///
    /// Every caller that has a whole `Stack` in its hand must come
    /// through here rather than through `spawn` -- the pack's `add_worn`
    /// exists for exactly the same reason and says exactly the same
    /// thing. `spawn` is for what the *world* produces, which is always
    /// new: a broken block, a felled tree, a killed animal.
    #[allow(clippy::too_many_arguments)]
    pub fn spawn_worn(
        &mut self,
        block: BlockId,
        count: u32,
        damage: u32,
        at: (f64, f64, f64),
        direction: (f32, f32, f32),
        dropped_by: Option<u64>,
        now: Instant,
    ) -> bool {
        if count == 0 || self.items.len() >= MAX_ITEMS {
            return false;
        }
        self.next_ordinal += 1;
        self.spawned += 1;
        self.items.push(Item {
            id: entity_id(EntitySource::Item, self.next_ordinal),
            block,
            count,
            damage,
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

    /// Destroys every stack within `radius` of a point and says how many
    /// stacks went.
    ///
    /// **Destroys, not collects.** The counterpart of `collect_near`,
    /// and the reason it is a separate call rather than a flag on that
    /// one: the two do opposite things to a player's afternoon, and a
    /// single function with a bool would be a function whose name does
    /// not say which. Added for `ItemsApi::clear_near`, which is the
    /// tidy-up a mod does after a fight rather than a way to take loot.
    ///
    /// A full scan rather than the 3x3 pickup window, because a mod may
    /// name any radius it likes and the window is sized for a player's
    /// arms. Not on any hot path: nothing in the game calls this.
    pub fn clear_near(&mut self, at: (f64, f64, f64), radius: f32) -> u32 {
        if !radius.is_finite() || radius < 0.0 {
            return 0;
        }
        let range_sq = f64::from(radius) * f64::from(radius);
        let before = self.items.len();
        self.items.retain(|item| {
            let (dx, dy, dz) = (
                item.position.0 - at.0,
                item.position.1 - at.1,
                item.position.2 - at.2,
            );
            dx * dx + dy * dy + dz * dz > range_sq
        });
        let gone = before - self.items.len();
        if gone > 0 {
            // Removal shifts indices, so the grid has to go with them --
            // the same rule `collect_near` follows when a stack empties.
            self.rebuild_cells();
        }
        gone as u32
    }

    /// Changes *what* each stack is, without touching where it is.
    ///
    /// `rewrite` is shown every item and answers the block it should
    /// now be, or `None` to leave it. Answers how many changed. Exists
    /// for the rot pass (`logic::rot`), which ages the meat lying in
    /// the grass by the same clock as the meat in a pack -- a drop that
    /// kept for ever would make the ground the best larder in the game.
    ///
    /// Deliberately **not** an `iter_mut` over the items. The pickup
    /// grid is indexed by position and rebuilt only when the step moves
    /// something; a caller handed `&mut Item` could move one between
    /// rebuilds and leave the grid pointing at the wrong cell. The
    /// block is the one field nothing else indexes on.
    pub fn rewrite_stacks(&mut self, mut rewrite: impl FnMut(&Item) -> Option<BlockId>) -> usize {
        let mut changed = 0;
        for item in &mut self.items {
            if let Some(block) = rewrite(item) {
                if block != item.block {
                    item.block = block;
                    changed += 1;
                }
            }
        }
        changed
    }

    /// Whether a player standing at `feet` is close enough to any of
    /// these cells for a pickup to be possible. The 3x3 window around
    /// the player's cell, i.e. the same cells `collect_near` would read.
    pub fn any_within_reach(feet: (f64, f64, f64), occupied: &HashSet<(i32, i32)>) -> bool {
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
        // Keyed by wear as well as by block, on the same rule
        // `Inventory::tidy` groups by: two objects that differ in how
        // spent they are are two objects. Nothing durable stacks past
        // one, so this never splits a pile that used to merge -- for
        // everything that does stack the damage is zero.
        let mut buckets: HashMap<(i32, i32, i32, BlockId, u32), usize> = HashMap::new();
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
                (item.position.0 / f64::from(MERGE_RANGE)).floor() as i32,
                (item.position.1 / f64::from(MERGE_RANGE)).floor() as i32,
                (item.position.2 / f64::from(MERGE_RANGE)).floor() as i32,
                item.block,
                item.damage,
            );
            match buckets.get(&key).copied() {
                Some(into) if into != index => {
                    // What fits is the *block's* limit, not the global
                    // one: two dropped axes lying together are two axes,
                    // and a pile of them on the ground would be picked up
                    // as a slot full of axes -- see
                    // `blocks::BlockDef::stack`.
                    let limit = primitive_shared::types::stack_limit(item.block);
                    let room = limit.saturating_sub(self.items[into].count);
                    let moved = room.min(self.items[index].count);
                    if moved > 0 {
                        self.items[into].count += moved;
                        self.items[index].count -= moved;
                        // **A pile is as old as the newest thing in it.** The
                        // count went onto the older stack and its clock
                        // stayed, so stone mined onto a heap near the end
                        // of its five minutes expired with the heap seconds
                        // later.
                        let newer = self.items[index].spawned_at;
                        if newer > self.items[into].spawned_at {
                            self.items[into].spawned_at = newer;
                        }
                        merged_any = true;
                    }
                    // A stack that filled up stops being the target, so
                    // the next one starts a fresh pile instead of
                    // silently failing to merge forever.
                    if self.items[into].count >= limit {
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
    /// `take` is handed the block, how many, and how worn -- and answers
    /// how many it took. The wear travels with the object: a pack that
    /// took a half-spent axe has to put a half-spent axe in a slot.
    pub fn take_lying_in(&mut self, cell: (i32, i32, i32), mut wanted: impl FnMut(BlockId) -> bool, count: u32) -> u32 {
        // Only what has come to rest in that cell: a stick still in the air
        // over a fire is not yet laid in it.
        let mut taken = 0;
        for item in self.items.iter_mut() {
            if taken == count {
                break;
            }
            let at = (
                item.position.0.floor() as i32,
                item.position.1.floor() as i32,
                item.position.2.floor() as i32,
            );
            if at != cell || item.count == 0 || !wanted(item.block) {
                continue;
            }
            let take = item.count.min(count - taken);
            item.count -= take;
            taken += take;
        }
        if taken > 0 {
            self.items.retain(|item| item.count > 0);
            self.rebuild_cells();
        }
        taken
    }

    /// How many of what `wanted` accepts are lying in a cell.
    pub fn count_lying_in(&self, cell: (i32, i32, i32), wanted: impl Fn(BlockId) -> bool) -> u32 {
        self.items
            .iter()
            .filter(|item| {
                wanted(item.block)
                    && (
                        item.position.0.floor() as i32,
                        item.position.1.floor() as i32,
                        item.position.2.floor() as i32,
                    ) == cell
            })
            .map(|item| item.count)
            .sum()
    }

    /// Takes up to `count` of what `wanted` accepts out of the stacks lying
    /// in one block cell, and answers how many it took.
    ///
    /// **What a firepit is laid from** (`primitive_shared::pit`):
    /// TerraFirmaCraft's fire is three sticks and a log *thrown on the
    /// ground* and struck, and the ground is this store. Taken only
    /// together -- the server counts first with `count_lying_in` and takes
    /// only when all of it is there -- so a strike short of a log costs
    /// nothing.
    ///
    /// Declared after its two helpers, above, for the reader's sake: this
    /// note is about all three.
    pub fn collect_near<F>(
        &mut self,
        player: u64,
        feet: (f64, f64, f64),
        now: Instant,
        mut take: F,
    ) where
        F: FnMut(BlockId, u32, u32) -> u32,
    {
        let range_sq = f64::from(PICKUP_RANGE * PICKUP_RANGE);
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
                        .clamp(feet.1, feet.1 + f64::from(primitive_shared::geometry::PLAYER_HEIGHT));
                    let (dx, dy, dz) = (
                        item.position.0 - feet.0,
                        item.position.1 - nearest_y,
                        item.position.2 - feet.2,
                    );
                    if dx * dx + dy * dy + dz * dz > range_sq {
                        continue;
                    }
                    let taken = take(item.block, item.count, item.damage).min(item.count);
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
        //
        // **Unless the water lifts it harder than it weighs.** Resting
        // skipped the buoyancy along with everything else, so timber that
        // touched the bed of a pool a hand deep lay on the bottom for its
        // whole life -- the one picture `step_one` exists to prevent.
        // Asked only of things that float, so a field of stone on the
        // ground still costs a density lookup and nothing else.
        let lifted = primitive_shared::types::floats(item.block)
            && submerged(world, item.position)
                > primitive_shared::types::density(item.block) / primitive_shared::types::WATER_DENSITY;
        if supported(world, item.position) && !blocked(world, item.position) && !lifted {
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

    // **In water, weight stops being the whole story.**
    //
    // A dropped thing used to fall through a lake exactly as it falls
    // through air and lie on the bed: a log at the bottom of a river is
    // the one thing everybody knows is wrong. What decides it is
    // *density* -- see `types::density`, which is the material's, not
    // the item's weight -- and the arithmetic is Archimedes with the
    // volumes cancelled: the acceleration is gravity scaled by how much
    // heavier than water the thing is. Stone at 2.6 sinks at three
    // fifths of a fall; oak at 0.6 rises at two thirds of one.
    //
    // **On the part of the box that is under, not on the cell the middle
    // is in.** The lift used to be all or nothing: full buoyancy while the
    // centre was in a water cell, a full fall with no drag the moment it
    // left. A log arriving at the top rose out of its cell, dropped back
    // in at a whole g, was thrown up again, and did that twenty times a
    // second for its five minutes of life -- every floating thing in the
    // world twitching on the surface, which is what a player reported
    // ("предметы дергаются при всплытии"). Lift in proportion to the wet
    // fraction is a spring with its rest at the waterline instead of a
    // switch across it: timber rides six tenths under, fat nine, and
    // nothing chatters because nothing changes discontinuously.
    let wet = submerged(world, item.position);
    // **What lies in the water is wet** (`wet`), as the pack of a swimmer
    // is. A bundle of kindling thrown across the river, or a log floated
    // down it, used to come out of the water as dry as it went in -- the
    // crossing the wet pack makes a plan of was one throw.
    if wet > 0.0 {
        item.block = primitive_shared::wet::wetted(item.block);
    }
    if wet > 0.0 {
        let ratio = primitive_shared::types::density(item.block)
            / primitive_shared::types::WATER_DENSITY;
        item.velocity.1 += GRAVITY * (1.0 - wet / ratio.max(0.05)) * dt;
        // Water is thick. Without this a log leaving the bottom of a
        // lake arrives at the surface like a cork out of a bottle and
        // then falls back, for ever: the drag is what turns buoyancy
        // into a rise that settles. Scaled by how much is wet, so the
        // drag arrives with the water rather than all at once on a
        // corner.
        let damp = (1.0 - WATER_DRAG_PER_SEC * wet * dt).clamp(0.0, 1.0);
        // **Against the water, not the ground**: a river's current
        // (`WorldGen::river_current`) is what the drag pulls a floating
        // thing toward, so a log dropped in a river goes down it at the
        // river's speed -- the way a player loses a stick they dropped
        // swimming, and the way timber was moved before anybody had a
        // cart. Only what floats: a stone on the bed stays where it sank,
        // and asking the generator for a field of sunk stone would be the
        // one cost here that grows with the number of drops.
        //
        // **And the water the simulation is moving**, which the generator
        // knows nothing about: a log in a pond somebody has just cut into
        // goes out through the cut with the water, and one on a settled
        // pond stays where it is (`fluid::running`). Four reads, and only
        // for a floating thing that is in water at all.
        let (flow_x, flow_z) = if primitive_shared::types::floats(item.block) {
            let (rx, rz) =
                world.generator().river_current(item.position.0 as f32, item.position.1 as f32, item.position.2 as f32);
            let cell = (item.position.0.floor() as i32, item.position.1.floor() as i32, item.position.2.floor() as i32);
            let (sx, sz) = primitive_shared::fluid::running_velocity(primitive_shared::fluid::running(
                cell.0,
                cell.1,
                cell.2,
                &|x, y, z| world.cached_block(x, y, z),
            ));
            (rx + sx, rz + sz)
        } else {
            (0.0, 0.0)
        };
        item.velocity.0 = flow_x + (item.velocity.0 - flow_x) * damp;
        item.velocity.1 *= damp;
        item.velocity.2 = flow_z + (item.velocity.2 - flow_z) * damp;
        if wet < 1.0 {
            item.velocity.1 *= (-SURFACE_DRAG_PER_SEC * dt).exp();
        }
    } else {
        item.velocity.1 = (item.velocity.1 + GRAVITY * dt).max(TERMINAL_VELOCITY);
    }

    // Axis at a time against the item's real box.
    //
    // Testing a single point was the earlier version, and it is wrong in
    // a way that is easy to see: the item is drawn as a cube three
    // tenths of a block across, so its centre can be clear of a wall
    // while a third of it is buried in one. Sweeping the box means what
    // collides is what is drawn.
    // **The step in `f64`, and the collision too.** A stack a million
    // blocks out moved in sixteenths while its position was an `f32` --
    // a slow slide down a slope was a stack that stood still and then
    // jumped -- and at ten million a whole block a tick, which is a stone
    // that falls into the ground it should have landed on. See
    // `geometry::for_each_block_box_f64` for the boxes' half of it.
    let step = (
        f64::from(item.velocity.0 * dt),
        f64::from(item.velocity.1 * dt),
        f64::from(item.velocity.2 * dt),
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

    // Whether this tick ended with the item in contact with a surface
    // it could rub against. Read by the friction at the bottom, which
    // is the only thing that has any business knowing it.
    let mut on_ground = false;

    let try_y = (item.position.0, item.position.1 + step.1, item.position.2);
    if let Some(top) = overlap_top(world, try_y) {
        if item.velocity.1 <= 0.0 {
            on_ground = true;
            // Landed. Sit exactly on top of whatever stopped it, so the
            // cube rests on the surface rather than half through it.
            //
            // **On the surface, not on the ceiling of its cell.** This
            // was `(try_y.1 - HALF).floor() + 1.0 + HALF`, which is the
            // same number for anything filling its cell and three
            // quarters of a block wrong over a campfire -- see
            // `overlap_top` for the bounce that produced. It is also
            // right for a drop that fell far enough in one tick to have
            // its box in two cells at once, where the old line settled
            // it inside the floor.
            item.position.1 = top + f64::from(HALF);
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

    // **Friction is what the ground does, and it was being done by the
    // air.** This ran every tick, flying included, at ninety-eight per
    // cent of the horizontal speed per second -- so a throw arrived at
    // the ground with eighteen per cent of what it left with, and a
    // gentle lob became a drop at the player's feet.
    //
    // What that cost is not a number in a file. The thrower's eye is
    // 1.62 up; a stack landing 0.96 blocks in front of them is 57 degrees
    // below the horizon, and the bottom edge of the frame is half the
    // field of view -- 35 degrees at the default 70. The item lands
    // *just past the bottom of the screen*, which is what "выкидываю
    // предметы -- их не видно" is. Without the drag in flight the same
    // throw carries about 1.36 blocks, some 20 degrees down, well inside
    // the frame.
    //
    // The fix is not a bigger throw. A faster lob with the same air
    // drag would still arrive dead and would sail off every ledge it
    // was mined from -- see the note on `spawn_worn`'s velocity, which
    // is where that was already tried once. What was wrong was applying
    // a *ground* number in the air, and the constant has said so since
    // it was written.
    //
    // The slide it is named for is untouched: an item still in contact
    // with a surface -- which is every tick of a bounce, and the moment
    // of a landing -- is scrubbed exactly as before.
    if on_ground {
        let drag = GROUND_DRAG_PER_SEC.powf(dt);
        item.velocity.0 *= drag;
        item.velocity.2 *= drag;
    }
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

fn solid_at(world: &World, at: (f64, f64, f64)) -> bool {
    let (x, y, z) = (
        at.0.floor() as i32,
        at.1.floor() as i32,
        at.2.floor() as i32,
    );
    match world.cached_block(x, y, z) {
        // Inside the block, not merely in its cell -- and the block is
        // not always the cell on the flat either. This used to be a
        // height test, which is right for a layer of soil and wrong for
        // a drying rack: a frame of poles is two and a half sixteenths
        // deep in the middle of its cell, so a drop lying beside one
        // read as buried in it and `unstick` shot it a metre into the
        // air. `geometry::block_box` is the same shape the player's
        // collider walks into.
        // ...and a leaning palm's slices, asked with the palm round them, so
        // a coconut shaken down beside the trunk is buried in the bark a
        // player walks into and not in the cell it leans out of.
        Some(block) => {
            let mut inside = false;
            let near = |dx: i32, dy: i32, dz: i32| {
                world.cached_block(x + dx, y + dy, z + dz).unwrap_or(primitive_shared::types::BLOCK_AIR)
            };
            primitive_shared::geometry::for_each_block_box_f64(block, x, y, z, near, |min, max| {
                inside |= (min[0]..max[0]).contains(&at.0)
                    && (min[1]..max[1]).contains(&at.1)
                    && (min[2]..max[2]).contains(&at.2);
            });
            inside
        }
        None => false,
    }
}

/// How much of an item's box is under water, 0 (dry) to 1 (sunk).
///
/// **A fraction, where it used to be a yes or no about the middle.** The
/// yes or no was chosen so a corner dipping would not flip the buoyancy
/// on and off -- and then the middle flipped it instead, at exactly the
/// waterline, which is where a floating thing spends its whole life. A
/// fraction has nothing to flip. See `step_one`.
///
/// Measured to the water's drawn surface in each cell
/// (`fluid::surface_height_with_above`) rather than to the cell's
/// ceiling: the top cell of a pool stops short of the top of its cell,
/// and measuring to the ceiling floated everything that far above the
/// water it was drawn on.
fn submerged(world: &World, centre: (f64, f64, f64)) -> f32 {
    use primitive_shared::types::{is_liquid, BLOCK_AIR};
    let (x, z) = (centre.0.floor() as i32, centre.2.floor() as i32);
    // Height never leaves a few hundred, so this half stays in `f32`.
    let (bottom, top) = (centre.1 as f32 - HALF, centre.1 as f32 + HALF);
    let mut wet = 0.0;
    for y in bottom.floor() as i32..=top.floor() as i32 {
        let Some(block) = world.cached_block(x, y, z) else {
            continue;
        };
        if !is_liquid(block) {
            continue;
        }
        let above = world.cached_block(x, y + 1, z).unwrap_or(BLOCK_AIR);
        let surface = y as f32 + primitive_shared::fluid::surface_height_with_above(block, above);
        wet += (top.min(surface) - bottom.max(y as f32)).max(0.0);
    }
    (wet / ITEM_SIZE).clamp(0.0, 1.0)
}

/// Whether an item centred here would overlap anything solid.
///
/// Every cell the box touches, not just the one its centre is in.
fn blocked(world: &World, centre: (f64, f64, f64)) -> bool {
    overlap_top(world, centre).is_some()
}

/// The top of the highest block box an item centred here overlaps, or
/// `None` if it is clear of everything.
///
/// **The height, not merely the fact.** A landing used to be settled
/// with `(bottom).floor() + 1.0`, which is the top of the *cell* -- and
/// that is only the top of the block while every block fills its cell.
/// A campfire fills a quarter of one and a backpack half, so a drop that
/// landed on either was lifted to the ceiling of the cell, found nothing
/// under it on the next tick, fell back to the real surface, and was
/// lifted again: **"на полу блоках те начинают прыгать"**, at twenty
/// ticks a second, for the whole five minutes a drop lives.
///
/// Every cell the box touches is scanned rather than stopping at the
/// first hit, because the first hit is not necessarily the highest --
/// a drop straddling a campfire and the full block beside it has to
/// rest on the block. It is at most eight comparisons, inside the
/// column lookup that already dominates this.
fn overlap_top(world: &World, centre: (f64, f64, f64)) -> Option<f64> {
    // A hair inside the faces, so an item resting exactly on a surface
    // does not read as intersecting it.
    let e = f64::from(HALF) - 1e-3;
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
    let mut cache: Option<(ChunkPos, Option<Arc<PackedChunk>>)> = None;
    let mut highest: Option<f64> = None;
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
                // answer, because some blocks do not fill theirs: an
                // item dropped on a campfire should lie *on* it, not
                // float at the top of its cell.
                //
                // **And across, not only up.** A drying rack is a full
                // cell tall and two and a half sixteenths deep, so a
                // height alone said a drop anywhere in the rack's cell
                // rests at the rack's top -- an item hanging in the air
                // beside the frame, at head height, with nothing under
                // it. The horizontal test is why this asks
                // `geometry::block_box` rather than `collision_height`:
                // it is the same box the player's collider walks into,
                // which is what stops a drop resting where a player
                // cannot reach it.
                let block = chunk.get(lx, y as usize, lz);
                // **A drop falls through a canopy.** Leaves are a solid
                // cell for a player -- you can stand on a treetop, and
                // that stays true -- but for something falling out of
                // one they are branches with gaps in them. An apple
                // picked out of an orchard used to land *on* the leaves
                // it grew in and sit there, out of reach unless the
                // player climbed after it, which is a bug that looks
                // exactly like a bug. Everything a tree gives up --
                // apples, sticks, a robbed nest, the fruit knocked out
                // of a canopy by a felled trunk -- now reaches the
                // ground, which is where a player is standing.
                //
                // It is the item's rule and not the world's: nothing
                // about the block changes, so the mesher, the collider
                // and the lighting are untouched. See
                // `types::is_canopy`.
                if primitive_shared::types::is_leafy(block) {
                    continue;
                }
                // A leaning palm's slices are asked with the palm round
                // them (`geometry::for_each_block_box`): a coconut shaken
                // down against the trunk rests on the bark it can see.
                let near = |dx: i32, dy: i32, dz: i32| {
                    world.cached_block(x + dx, y + dy, z + dz).unwrap_or(primitive_shared::types::BLOCK_AIR)
                };
                primitive_shared::geometry::for_each_block_box_f64(block, x, y, z, near, |bmin, bmax| {
                    if centre.0 - e < bmax[0]
                        && centre.0 + e > bmin[0]
                        && centre.2 - e < bmax[2]
                        && centre.2 + e > bmin[2]
                        && centre.1 - e < bmax[1]
                    {
                        highest = Some(highest.map_or(bmax[1], |best: f64| best.max(bmax[1])));
                    }
                });
            }
        }
    }
    highest
}

/// Whether there is something directly under a resting item.
fn supported(world: &World, at: (f64, f64, f64)) -> bool {
    blocked(world, (at.0, at.1 - 0.05, at.2))
}

#[cfg(test)]
mod tests {


    /// **A log does not lie on the bottom of a river.**
    ///
    /// Dropped things fell through water exactly as they fall through
    /// air, which put timber, torches and wool on the lake bed -- the
    /// one thing every player knows is wrong. What decides it is
    /// density, and this checks the three cases that matter: something
    /// lighter than water comes up, something heavier goes down, and the
    /// one that is *nearly* water settles instead of shooting out.
    #[test]
    fn what_floats_comes_up_and_what_sinks_goes_down() {
        use primitive_shared::types::{
            density, floats, BLOCK_FAT, BLOCK_IRON_INGOT, BLOCK_LOG, BLOCK_STONE, BLOCK_WOOL,
            WATER_DENSITY,
        };
        assert!(floats(BLOCK_LOG), "timber should float");
        assert!(floats(BLOCK_WOOL), "a fleece should float");
        assert!(floats(BLOCK_FAT), "tallow is skimmed off the top of the pot");
        assert!(!floats(BLOCK_STONE), "rock should not");
        assert!(!floats(BLOCK_IRON_INGOT), "iron least of all");

        // Fat is the near case: it floats, and only just, so the rise
        // has to be gentle or it behaves like cork.
        let gentle = (1.0 - WATER_DENSITY / density(BLOCK_FAT)).abs();
        let cork = (1.0 - WATER_DENSITY / density(BLOCK_LOG)).abs();
        assert!(
            gentle * 3.0 < cork,
            "tallow rises at {gentle:.2} of gravity and timber at {cork:.2}"
        );
        // ...and nothing anywhere is denser than iron or lighter than
        // the grass family, which is the range the arithmetic assumes.
        for id in primitive_shared::types::ALL_BLOCK_IDS.iter().map(|(id, _)| *id) {
            let d = density(id);
            assert!(
                (100.0..=9000.0).contains(&d),
                "{} has a density of {d}",
                primitive_shared::types::block_name(id)
            );
        }
    }

    use primitive_shared::inventory::MAX_STACK;
    use super::*;
    use primitive_shared::types::{collision_height, BLOCK_DIRT, BLOCK_STONE};

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
    fn an_apple_falls_through_the_canopy_it_grew_in() {
        // **What a player sees when this breaks**: they knock an apple
        // out of a tree, it lands on the leaves a block below and sits
        // there at head height in the middle of a canopy, out of reach
        // unless they climb after it. Leaves are still solid for
        // everything else -- a player may stand on the same treetop --
        // so this is the item's rule and nothing more. See
        // `types::is_canopy`.
        use primitive_shared::types::{Chunk, ChunkPos, BLOCK_AIR, BLOCK_LEAVES, CHUNK_VOLUME};
        let world = World::new(1, 256);
        let mut blocks = vec![BLOCK_AIR; CHUNK_VOLUME];
        for z in 0..16 {
            for x in 0..16 {
                // A floor...
                blocks[Chunk::index(x, 9, z)] = BLOCK_STONE;
                // ...and a canopy three blocks over it.
                blocks[Chunk::index(x, 13, z)] = BLOCK_LEAVES;
            }
        }
        world.insert(Chunk {
            pos: ChunkPos::new(0, 0),
            blocks,
        });

        let mut items = Items::new();
        assert!(items.spawn(
            primitive_shared::types::BLOCK_APPLE,
            1,
            (8.5, 16.0, 8.5),
            (0.0, 0.0, 0.0),
            None,
            settled(),
        ));
        for _ in 0..200 {
            items.step(&world, 1.0 / 20.0, now());
        }
        let resting = items.iter().next().expect("the apple vanished").position.1;
        assert!(
            resting < 13.0,
            "the apple came to rest at {resting}, which is in the canopy rather than under it"
        );
        // ...and it did stop, on the floor, rather than falling out of
        // the world: a rule that let items through everything would
        // pass this test by deleting the game.
        assert!(resting > 9.0, "the apple fell through the floor as well");
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
        items.collect_near(1, (0.0, 0.5, 0.0), now(), |_, count, _| {
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
            items.collect_near(1, (0.0, 0.5, 0.0), at, |_, count, _| {
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
        items.collect_near(1, (40.0, 1.0, 40.0), now(), |_, count, _| {
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
        items.collect_near(1, (0.0, 0.5, 0.0), now(), |_, _, _| 2);
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
        items.collect_near(7, (0.0, 0.5, 0.0), later, |_, c, _| {
            got += c;
            c
        });
        assert_eq!(got, 0, "the thrower picked it straight back up");

        items.collect_near(9, (0.0, 0.5, 0.0), later, |_, c, _| {
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
        items.collect_near(7, (0.0, 0.5, 0.0), Instant::now(), |_, c, _| {
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
                (f64::from(4.0 + (i % 3) as f32 * 0.1), 11.0, 4.0),
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
        items.collect_near(7, (4.0, 10.0, 4.0), now, |_, count, _| {
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

    /// A pile is as old as the newest thing in it.
    ///
    /// Merging moved the count onto the *older* stack and kept that
    /// stack's clock, so thirty stone mined onto a heap four minutes and
    /// fifty seconds old were gone ten seconds later -- from a player who
    /// had been told a drop waits five minutes.
    #[test]
    fn a_fresh_drop_that_joins_an_old_pile_keeps_its_own_five_minutes() {
        let world = walled_world();
        let mut items = Items::new();
        let start = Instant::now();
        items.spawn(
            BLOCK_STONE,
            3,
            (4.0, 11.0, 4.0),
            (0.0, 0.0, 0.0),
            None,
            start - LIFETIME + Duration::from_secs(20),
        );
        items.spawn(BLOCK_STONE, 5, (4.0, 11.0, 4.0), (0.0, 0.0, 0.0), None, settled());
        for _ in 0..60 {
            items.step(&world, 1.0 / 20.0, start);
        }
        assert_eq!(items.len(), 1, "the two stacks never piled together");

        // Half a minute on: the old pile's time is up, the new stack's is not.
        items.step(&world, 1.0 / 20.0, start + Duration::from_secs(30));
        let left: u32 = items.iter().map(|item| item.count).sum();
        assert!(left >= 5, "the stone mined a moment ago expired with the old pile ({left} left)");
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
        let bottom = item.position.1 - f64::from(ITEM_SIZE / 2.0);
        assert!(
            (bottom - 10.0).abs() < 0.01,
            "its underside rested at {bottom} instead of on the floor"
        );
        assert!(!blocked(&world, item.position));
    }

    /// The floor of `walled_world` with `block` laid on top of it at
    /// (4, 10, 4), which is where these tests drop things.
    fn floor_topped_with(block: BlockId) -> World {
        use primitive_shared::types::{Chunk, ChunkPos, BLOCK_AIR, CHUNK_VOLUME};
        let world = World::new(1, 256);
        let mut blocks = vec![BLOCK_AIR; CHUNK_VOLUME];
        for y in 0..10 {
            for z in 0..16 {
                for x in 0..16 {
                    blocks[Chunk::index(x, y, z)] = BLOCK_STONE;
                }
            }
        }
        for z in 0..16 {
            for x in 0..16 {
                blocks[Chunk::index(x, 10, z)] = block;
            }
        }
        // Nine chunks, not one. A drop thrown hard leaves the middle
        // chunk inside a second, and an absent chunk is nothing to land
        // on -- so a one-chunk fixture is a floor with a cliff eight
        // blocks from the middle, and a test that throws anything ends
        // up measuring the fall out of the world instead.
        for cx in -1..=1 {
            for cz in -1..=1 {
                world.insert(Chunk {
                    pos: ChunkPos::new(cx, cz),
                    blocks: blocks.clone(),
                });
            }
        }
        world
    }

    /// A stone floor at y = 0..=9 with one `block` standing at
    /// (4, 10, 4) and open air all round it.
    ///
    /// `floor_topped_with` fills the whole layer, which is the right
    /// fixture for "does a drop land *on* this" and the wrong one for
    /// "does a drop land *beside* it": with every cell filled there is
    /// no beside to land in.
    fn floor_beside(block: BlockId) -> World {
        use primitive_shared::types::BLOCK_AIR;
        let world = floor_topped_with(BLOCK_AIR);
        assert!(world.set_block(4, 10, 4, block), "the fixture is not loaded");
        world
    }

    #[test]
    fn a_log_dropped_on_a_pond_comes_to_rest_on_the_surface_instead_of_hopping_on_it() {
        // **The twitch a player reported, as motion.** The lift used to
        // switch on the cell the centre was in, so a floating log crossed
        // the top of its cell, fell back at a whole g, and was thrown up
        // again every tick. Two seconds is long enough for any honest
        // landing to have settled; after that, a second of stepping may
        // not move it by more than float noise.
        //
        // One cell of water on a stone bed, and dropped from above it, so
        // the log also hits the bottom on the way in -- which is the other
        // half: a resting drop used to skip the buoyancy and stay there.
        use primitive_shared::types::{BLOCK_AIR, BLOCK_LOG, BLOCK_WATER};
        let world = floor_topped_with(BLOCK_WATER);
        let mut items = Items::new();
        items.spawn(BLOCK_LOG, 1, (4.5, 12.5, 4.5), (0.0, 0.0, 0.0), None, now());
        let height = |items: &Items| items.iter().next().expect("the log vanished").position.1;

        for _ in 0..40 {
            items.step(&world, 1.0 / 20.0, now());
        }
        let (mut lowest, mut highest) = (height(&items), height(&items));
        for _ in 0..20 {
            items.step(&world, 1.0 / 20.0, now());
            lowest = lowest.min(height(&items));
            highest = highest.max(height(&items));
        }
        assert!(
            highest - lowest < 0.002,
            "a log on still water moved {:.3} blocks in a second after it landed: \
             it is hopping, not floating",
            highest - lowest
        );
        let surface = 10.0 + primitive_shared::fluid::surface_height_with_above(BLOCK_WATER, BLOCK_AIR);
        assert!(
            lowest - f64::from(HALF) < f64::from(surface) && highest + f64::from(HALF) > f64::from(surface),
            "the log is at {lowest:.3}, and the water's surface at {surface:.3} is not \
             between its bottom and its top"
        );
    }

    #[test]
    fn kindling_thrown_into_a_pond_comes_out_wet_and_a_stone_comes_out_a_stone() {
        use primitive_shared::types::{BLOCK_LOG, BLOCK_WATER};
        let world = floor_topped_with(BLOCK_WATER);
        let mut items = Items::new();
        items.spawn(BLOCK_LOG, 1, (4.5, 12.5, 4.5), (0.0, 0.0, 0.0), None, now());
        items.spawn(BLOCK_STONE, 1, (6.5, 12.5, 6.5), (0.0, 0.0, 0.0), None, now());
        for _ in 0..40 {
            items.step(&world, 1.0 / 20.0, now());
        }
        let log = items.iter().find(|i| primitive_shared::types::block_kind(i.block) == BLOCK_LOG).expect("the log");
        assert!(primitive_shared::wet::is_wet(log.block), "a log that floated in a pond is dry");
        assert!(items.iter().any(|i| i.block == BLOCK_STONE), "a stone came out of the water as something else");
    }

    /// **A drop beside a rack lies on the ground, not on the rack.**
    ///
    /// A drying rack is a full cell tall and two and a half sixteenths
    /// deep, and this scan asked `collision_height` -- a number with no
    /// opinion about width. So every drop anywhere in a rack's cell was
    /// held up at the rack's top: an item hanging at head height beside
    /// the frame, with a metre of daylight under it, unreachable from
    /// the ground it looked like it was lying on.
    ///
    /// Asserted on the *underside* rather than the centre, because that
    /// is the surface the fault was about.
    #[test]
    fn a_drop_beside_a_drying_rack_lies_on_the_floor_and_not_on_the_frame() {
        use primitive_shared::types::{faced, Facing, BLOCK_DRYING_RACK};

        let world = floor_beside(faced(BLOCK_DRYING_RACK, Facing::North));
        let mut items = Items::new();
        // Inside the rack's own cell and clear of the poles, which
        // stand at 4.4375..4.59375 on z. The drop is 0.3 across.
        items.spawn(BLOCK_STONE, 1, (4.2, 14.0, 4.2), (0.0, 0.0, 0.0), None, now());
        for _ in 0..200 {
            items.step(&world, 1.0 / 20.0, now());
        }
        let item = items.iter().next().expect("the drop vanished");
        let bottom = item.position.1 - f64::from(HALF);
        assert!(
            (bottom - 10.0).abs() < 0.01,
            "the drop's underside rested at {bottom} rather than on the floor at 10.0",
        );

        // ...and walking into the frame is still walking into
        // something: a drop that lands *on* the poles rests on them.
        let mut items = Items::new();
        items.spawn(BLOCK_STONE, 1, (4.5, 14.0, 4.5), (0.0, 0.0, 0.0), None, now());
        for _ in 0..200 {
            items.step(&world, 1.0 / 20.0, now());
        }
        let bottom = items.iter().next().expect("the drop vanished").position.1 - f64::from(HALF);
        assert!(
            (bottom - 11.0).abs() < 0.01,
            "a drop dropped onto the frame fell through it to {bottom}",
        );
    }

    /// Every block a drop can land on that does not fill its cell.
    ///
    /// Read off the block table for the reason the client's collider
    /// tests read their copy off it: what broke here is a property of
    /// anything shorter than its cell, and a list written by hand would
    /// not grow when the table does.
    fn part_height_blocks() -> Vec<(BlockId, &'static str, f32)> {
        let found: Vec<_> = primitive_shared::blocks::BLOCKS
            .iter()
            .map(|def| (def.id, def.name, collision_height(def.id)))
            .filter(|&(_, _, height)| height > 0.0 && height < 1.0)
            .collect();
        assert!(!found.is_empty(), "nothing is shorter than its cell any more");
        found
    }

    #[test]
    fn a_drop_lands_on_top_of_a_block_that_does_not_fill_its_cell() {
        // **"На полу блоках те начинают прыгать."** A landing settled
        // the drop at the top of the *cell* rather than the top of the
        // block, so on a campfire or a pack it was put three quarters of
        // a block into the air, fell back to the real surface, and was
        // put there again -- twenty times a second, for the five minutes
        // a drop lives. See `overlap_top`.
        for (block, name, height) in part_height_blocks() {
            let world = floor_topped_with(block);
            let mut items = Items::new();
            items.spawn(BLOCK_STONE, 1, (4.5, 14.0, 4.5), (0.0, 0.0, 0.0), None, now());
            for _ in 0..200 {
                items.step(&world, 1.0 / 20.0, now());
            }
            let item = items.iter().next().expect("the drop vanished");
            let bottom = item.position.1 - f64::from(HALF);
            assert!(
                (bottom - f64::from(10.0 + height)).abs() < 0.01,
                "on a {name} the drop's underside rested at {bottom} rather than on {}",
                10.0 + height
            );

            // ...and it stays there. The bounce was never in the
            // landing, it was in the tick *after* it: the drop was
            // above its own support, so nothing held it up.
            let settled = item.position.1;
            for _ in 0..100 {
                items.step(&world, 1.0 / 20.0, now());
                let now_at = items.iter().next().unwrap().position.1;
                assert!(
                    (now_at - settled).abs() < 0.001,
                    "a drop on a {name} moved to {now_at} after settling at {settled}"
                );
            }
        }
    }

    #[test]
    fn horizontal_speed_is_lost_on_the_ground_and_kept_in_the_air() {
        // **Both halves of what `GROUND_DRAG_PER_SEC` is for, stated as
        // motion rather than as its value.** It ran every tick, flight
        // included, which is a ground number being applied by the air:
        // a throw arrived with eighteen per cent of the speed it left
        // with and landed at the thrower's feet, out of frame. Taking it
        // out of the flight must not take it out of the slide it is
        // named for -- a drop still has to stop rather than skitter --
        // so the property has to be asserted from both ends or the fix
        // is one line away from becoming the opposite fault.
        //
        // A distance would not do it: a hard throw bounces, contact
        // ticks are few, and with the friction taken out altogether the
        // stack settles only four tenths of a block further on. What
        // separates the two worlds is not where it stops, it is whether
        // a tick in the air costs it anything.
        let world = floor_topped_with(BLOCK_STONE);
        let floor_top = 11.0;
        let mut items = Items::new();
        // Dropped from high up so that it is still moving fast when it
        // arrives, and therefore bounces: a landing gentle enough to
        // settle zeroes every axis at once, which would prove nothing
        // about friction.
        items.spawn(BLOCK_STONE, 1, (4.5, 16.0, 4.5), (5.0, 0.0, 0.0), None, now());

        let speed = |items: &Items| items.iter().next().expect("the drop vanished").velocity.0;
        let rising = |items: &Items| items.iter().next().unwrap().velocity.1 >= 0.0;
        let height = |items: &Items| items.iter().next().unwrap().position.1;

        let thrown_at = speed(&items);
        assert!(thrown_at > 1.0, "the fixture threw nothing: {thrown_at}");

        // The fall, stopping well clear of the floor: every tick of it
        // has to cost the throw nothing at all.
        let mut ticks_in_the_air = 0;
        while height(&items) > floor_top + 2.0 {
            let before = speed(&items);
            items.step(&world, 1.0 / 20.0, now());
            ticks_in_the_air += 1;
            assert_eq!(
                speed(&items),
                before,
                "a tick in mid-air cost the drop speed, {ticks_in_the_air} ticks in"
            );
            assert!(ticks_in_the_air < 400, "it never came down");
        }
        assert!(ticks_in_the_air > 3, "it started on the floor, so nothing flew");

        // ...and then the tick it touches down on, which is the one that
        // turns the fall around. That tick has to cost it some, and not
        // all: a landing gentle enough to zero every axis at once is a
        // settle, and proves nothing about a slide.
        for _ in 0..40 {
            let before = speed(&items);
            items.step(&world, 1.0 / 20.0, now());
            if !rising(&items) && !items.iter().next().unwrap().resting {
                assert_eq!(before, speed(&items), "still falling, and it cost speed");
                continue;
            }
            assert!(
                speed(&items) < before,
                "touching down cost the drop nothing: {before} then {}",
                speed(&items)
            );
            assert!(
                speed(&items) > 0.0,
                "the landing stopped it dead rather than scrubbing it, so this                  is a settle and not the slide the friction is named for"
            );
            return;
        }
        panic!("it never touched down");
    }

    #[test]
    fn an_item_collides_as_the_cube_it_is_drawn_as() {
        // A point-sized collider lets the visible cube bury a third of
        // itself in a wall its centre is clear of, which is most of what
        // made drops look wrong.
        let world = walled_world();
        // Centre just clear of the wall at x = 8, but the box is not.
        let centre = (8.0 - f64::from(ITEM_SIZE) / 4.0, 12.0, 4.0);
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
            assert!(items.spawn(BLOCK_STONE, 1, primitive_shared::geometry::wide(at), (0.0, 0.0, 0.0), None, settled()));
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
        items.collect_near(1, primitive_shared::geometry::wide(feet), now(), |_, count, _| {
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

    /// The stone floor and wall of `walled_world`, in the chunk `(cx, cz)`.
    fn walled_world_at(cx: i32, cz: i32) -> World {
        use primitive_shared::types::{Chunk, ChunkPos, BLOCK_AIR, BLOCK_STONE, CHUNK_VOLUME};
        let world = World::new(1, 256);
        let mut blocks = vec![BLOCK_AIR; CHUNK_VOLUME];
        for z in 0..16 {
            for x in 0..16 {
                for y in 0..10 {
                    blocks[Chunk::index(x, y, z)] = BLOCK_STONE;
                }
                if x >= 8 {
                    for y in 10..16 {
                        blocks[Chunk::index(x, y, z)] = BLOCK_STONE;
                    }
                }
            }
        }
        world.insert(Chunk { pos: ChunkPos::new(cx, cz), blocks });
        world
    }

    /// **A stack thrown a long way out lands where it lands at home.** Its
    /// position was an `f32`: a million blocks out a slide along the floor
    /// was a stack that stood still and then jumped a sixteenth, and ten
    /// million out a stone thrown at a wall moved a whole block a tick and
    /// was put down inside the stone (`for_each_block_box_f64` is the other
    /// half of it -- the wall's face was a rounding too). Thrown at the wall
    /// from the same place in the same chunk, it has to come to rest at the
    /// same spot to the micron, on the floor and short of the wall.
    #[test]
    fn a_stack_thrown_far_from_zero_lands_where_it_lands_at_home() {
        let throw = |cx: i32, cz: i32| {
            let world = walled_world_at(cx, cz);
            let corner = (f64::from(cx) * 16.0, 0.0, f64::from(cz) * 16.0);
            let mut items = Items::new();
            assert!(items.spawn(BLOCK_STONE, 1, (corner.0 + 5.3, 12.0, corner.2 + 4.7), (1.0, 0.0, 0.2), None, settled()));
            let mut path = Vec::new();
            for _ in 0..200 {
                items.step(&world, 1.0 / 20.0, now());
                let at = items.iter().next().expect("the stack vanished").position;
                path.push((at.0 - corner.0, at.1, at.2 - corner.2));
            }
            path
        };
        let home = throw(0, 0);
        let rest = *home.last().unwrap();
        assert!((rest.1 - (10.0 + f64::from(HALF))).abs() < 1e-3, "at home the stone came to rest at {rest:?}");
        assert!(rest.0 <= 8.0 - f64::from(HALF) + 1e-3 && rest.0 > 6.0, "at home the stone is not at the wall: {rest:?}");
        for (cx, cz) in [(62_500, 62_500), (-62_500, 62_500), (625_000, -625_000), (-625_000, -625_000)] {
            let far = throw(cx, cz);
            for (tick, (a, b)) in home.iter().zip(&far).enumerate() {
                assert!(
                    (a.0 - b.0).abs() < 1e-5 && (a.1 - b.1).abs() < 1e-5 && (a.2 - b.2).abs() < 1e-5,
                    "chunk ({cx}, {cz}), tick {tick}: at home the stone was at {a:?} from the corner, out there at {b:?}"
                );
            }
        }
    }
}
