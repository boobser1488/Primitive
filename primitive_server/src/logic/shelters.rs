//! The rooms players stand in, remembered, and the warmth each room holds.
//!
//! The rules are `primitive_shared::shelter`'s. What is here is the two
//! things rules cannot keep: a cache of which room a cell is in, so the
//! walk is not paid for twice a second per player, and the warmth a room
//! has soaked up from a fire, so a room can still be warm after the fire
//! has gone out.
//!
//! ## Why a cache, and why this one
//!
//! A room is a flood fill of up to `wildfire::ROOM_MAX_CELLS` cells, each
//! of which asks up to six cells above it whether there is a ceiling: a few
//! thousand block reads, each through a shard lock. The warmth is sampled
//! every `climate::SAMPLE_INTERVAL_SECS` per player, and a player standing
//! in their hut stands in the same cell for minutes. So the answer is kept
//! per cell, with the world's edit count at the time, and thrown away when
//! an edit lands near the room (`World::edited_since`) or when it is older
//! than [`ROOM_TTL`].
//!
//! Rejected: invalidating by listening for block changes. Every mechanic
//! that edits the world would have to say so, and one that forgot -- the
//! fire eating a wall, sand pouring through a roof -- would leave a room
//! sealed that is open. The world's own journal cannot forget.
//!
//! Rejected: surveying on the comfort's slower clock (`comfort::SURVEY_SECONDS`)
//! and no cache. A door shut is warmth the player should feel within the
//! half second everything else about warmth is felt in; four seconds of a
//! draught through a door already shut is a bug report.
//!
//! ## Why the warmth is kept here and not on the player
//!
//! It belongs to the room. A player who lights a fire, goes out for wood
//! and comes back should find the room as warm as the fire and the walls
//! left it -- not as warm as they last felt it, and not cold because they
//! were not there to watch it. So it is kept by room, with the world day it
//! was last true, and decays when it is next asked about (`shelter::afterglow`)
//! rather than on a tick nobody needs.
//!
//! Not saved. It is at most a few minutes of warmth, and a restart that
//! forgets it costs a player a cold quarter of an hour in a room whose fire
//! is out anyway.

use std::collections::HashMap;
use std::sync::Arc;
use std::time::{Duration, Instant};

use primitive_shared::raft::Wind;
use primitive_shared::shelter::{afterglow, survey, Room};
use primitive_shared::wildfire::ROOM_MAX_RISE;

use crate::logic::world::World;

type Cell = (i32, i32, i32);
type Key = ((i32, i32, i32), u32, u16, u16);

/// How long a cached room is trusted without being looked at again, edits
/// or no edits.
///
/// Half a minute. The edit journal covers every change inside the box the
/// room was found in; this covers the room that was *not* found -- a hall
/// too big to be a room, whose far end is outside any box worth keeping --
/// and anything the journal has forgotten. One walk per player per half
/// minute is nothing.
pub const ROOM_TTL: Duration = Duration::from_secs(30);

/// How far round a cell that is not in a room an edit still matters, in
/// blocks. A roof laid within this of where somebody stands can make the
/// open air they were in a room.
const OPEN_AIR_REACH: i32 = 8;

/// The most cells remembered at once. Past it the cache starts again, which
/// costs one walk per player standing in a room.
const MOST_ROOMS: usize = 4096;

/// The most rooms whose warmth is remembered. Past it the cold ones go.
const MOST_WARM_ROOMS: usize = 1024;

/// Warmth under this is no warmth, and is forgotten.
const COLD: f32 = 0.005;

struct Cached {
    room: Option<Arc<Room>>,
    serial: u64,
    low: Cell,
    high: Cell,
    at: Instant,
}

#[derive(Default)]
pub struct Shelters {
    rooms: HashMap<Cell, Cached>,
    /// Room -> (the share of a hearth's warmth it held, the world day it
    /// held it, how long it holds it).
    warmth: HashMap<Key, (f32, f64, f32)>,
    /// Walks taken, for the tests and the stats: how often the cache failed.
    pub walks: u64,
}

impl Shelters {
    pub fn new() -> Self {
        Self::default()
    }

    /// The room the cell `feet` is in, if it is in one, walked again only
    /// when it has to be.
    pub fn room_at(&mut self, world: &World, feet: Cell) -> Option<Arc<Room>> {
        if let Some(cached) = self.rooms.get(&feet) {
            if cached.at.elapsed() < ROOM_TTL && !world.edited_since(cached.serial, cached.low, cached.high) {
                return cached.room.clone();
            }
        }
        // Taken before the walk, so an edit that lands during it makes the
        // answer stale rather than slipping in under it.
        let serial = world.edit_serial();
        let unloaded = std::cell::Cell::new(false);
        let room = survey(
            |x, y, z| {
                let block = world.cached_block(x, y, z);
                if block.is_none() {
                    unloaded.set(true);
                }
                block
            },
            feet,
        );
        self.walks += 1;
        // **A room that was not found because the world round it was not
        // loaded is not remembered**: it is the answer to a question about
        // blocks nobody has yet, and the edit journal cannot say when they
        // arrive -- loading a chunk is not an edit.
        if unloaded.get() {
            return room.map(Arc::new);
        }
        let top = feet.1 - 1 + ROOM_MAX_RISE + 1;
        let (low, high) = match &room {
            // Two past the walls: the walk looks at the neighbours of what
            // is beyond each gap (`shelter::survey`), and a block laid there
            // changes whether that gap is a smoke hole.
            Some(room) => {
                let (low, high) = room.bounds;
                ((low.0 - 2, feet.1 - 1, low.2 - 2), (high.0 + 2, top, high.2 + 2))
            }
            None => (
                (feet.0 - OPEN_AIR_REACH, feet.1 - 1, feet.2 - OPEN_AIR_REACH),
                (feet.0 + OPEN_AIR_REACH, top, feet.2 + OPEN_AIR_REACH),
            ),
        };
        if self.rooms.len() >= MOST_ROOMS {
            self.rooms.clear();
        }
        let room = room.map(Arc::new);
        self.rooms.insert(feet, Cached { room: room.clone(), serial, low, high, at: Instant::now() });
        room
    }

    /// The share of a hearth's warmth this room still holds on world day
    /// `now`: nought for a room nobody has warmed.
    pub fn warmth(&self, room: &Room, now: f64) -> f32 {
        self.warmth
            .get(&room.key)
            .map(|&(share, then, holds)| afterglow(share, (now - then).max(0.0) as f32, holds))
            .unwrap_or(0.0)
    }

    /// A sample found `hearth` of a fire's warmth in this room: remember
    /// whichever is more, that or what the walls were still holding.
    pub fn warmed(&mut self, room: &Room, wind: Wind, hearth: f32, now: f64) {
        let held = self.warmth(room, now).max(if hearth.is_finite() { hearth.clamp(0.0, 1.0) } else { 0.0 });
        if held < COLD {
            self.warmth.remove(&room.key);
            return;
        }
        if self.warmth.len() >= MOST_WARM_ROOMS && !self.warmth.contains_key(&room.key) {
            self.warmth.retain(|_, &mut (share, then, holds)| afterglow(share, (now - then).max(0.0) as f32, holds) >= COLD);
            if self.warmth.len() >= MOST_WARM_ROOMS {
                return;
            }
        }
        self.warmth.insert(room.key, (held, now, room.holds_days(wind)));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use primitive_shared::types::{BLOCK_AIR, BLOCK_STONE};

    fn world() -> Arc<World> {
        let world = Arc::new(World::with_preset(7, primitive_shared::worldgen::Preset::Test, 1024));
        for cx in -1..=1 {
            for cz in -1..=1 {
                let chunk = world.generate(primitive_shared::types::ChunkPos::new(cx, cz));
                world.insert(chunk);
            }
        }
        let ground = primitive_shared::showcase::GROUND_Y;
        for x in -6..=12 {
            for z in -6..=12 {
                for y in (ground + 1)..(ground + 14) {
                    world.set_block(x, y, z, BLOCK_AIR);
                }
            }
        }
        world
    }

    /// A shut stone hut, inside x and z 0..=3, air three high.
    fn hut(world: &World) -> Cell {
        let y = primitive_shared::showcase::GROUND_Y + 1;
        for x in -1..=4 {
            for z in -1..=4 {
                world.set_block(x, y + 3, z, BLOCK_STONE);
                if x == -1 || x == 4 || z == -1 || z == 4 {
                    for dy in 0..3 {
                        world.set_block(x, y + dy, z, BLOCK_STONE);
                    }
                }
            }
        }
        (1, y, 1)
    }

    #[test]
    fn a_room_is_walked_once_and_again_only_when_a_wall_changes() {
        let world = world();
        let feet = hut(&world);
        let mut shelters = Shelters::new();
        let started = Instant::now();
        let first = shelters.room_at(&world, feet).expect("the hut is not a room");
        let walked = started.elapsed();
        assert!(first.side.is_empty());
        let started = Instant::now();
        for _ in 0..10 {
            shelters.room_at(&world, feet);
        }
        eprintln!("a walk: {walked:?}; ten cached answers: {:?}", started.elapsed());
        assert_eq!(shelters.walks, 1, "a room nobody touched was walked again");

        // An edit far away is not this room's business.
        world.set_block(40, feet.1, 40, BLOCK_STONE);
        shelters.room_at(&world, feet);
        assert_eq!(shelters.walks, 1, "an edit across the map made the hut stale");

        // A doorway is.
        world.set_block(4, feet.1, 1, BLOCK_AIR);
        world.set_block(4, feet.1 + 1, 1, BLOCK_AIR);
        let opened = shelters.room_at(&world, feet).expect("a doorway unmade the hut");
        assert_eq!(shelters.walks, 2);
        assert_eq!(opened.side.len(), 2, "the cached hut did not see its new door");
    }

    #[test]
    fn a_room_keeps_its_fire_after_the_fire_and_a_room_opened_up_does_not() {
        let world = world();
        let feet = hut(&world);
        let mut shelters = Shelters::new();
        let room = shelters.room_at(&world, feet).expect("room");
        shelters.warmed(&room, Wind::CALM, 0.55, 10.0);
        let an_hour = 1.0 / 24.0;
        let later = shelters.warmth(&room, 10.0 + an_hour);
        assert!(later > 0.3 && later < 0.55, "a stone hut an hour after its fire held {later}");
        // A later sample with no fire does not wipe the walls' warmth out.
        shelters.warmed(&room, Wind::CALM, 0.0, 10.0 + an_hour);
        assert!((shelters.warmth(&room, 10.0 + an_hour) - later).abs() < 1e-4);

        world.set_block(1, feet.1 + 3, 1, BLOCK_AIR);
        let holed = shelters.room_at(&world, feet).expect("a smoke hole unmade the hut");
        assert_eq!(shelters.warmth(&holed, 10.0 + an_hour), 0.0, "the warmth stayed in through the new hole");
    }

    /// **The measurement the cache is for.** Reads counted, not time: a
    /// debug build's clock says nothing about a release server's, and the
    /// count is the same on every machine.
    #[test]
    fn the_biggest_room_there_is_costs_a_bounded_number_of_reads_and_a_cached_one_costs_none() {
        let reads = std::cell::Cell::new(0u32);
        // A hall at the size limit: a stone box, air everywhere inside.
        let side = 14;
        let look = |x: i32, y: i32, z: i32| {
            reads.set(reads.get() + 1);
            let inside = (0..side).contains(&x) && (0..side).contains(&z) && (1..=2).contains(&y);
            Some(if inside { BLOCK_AIR } else { BLOCK_STONE })
        };
        let started = Instant::now();
        let hall = survey(look, (1, 1, 1));
        let took = started.elapsed();
        assert!(hall.is_some(), "a hall of {} cells was not a room", side * side * 2);
        let cells = (side * side * 2) as u32;
        // Five neighbours a cell, each asking up to ROOM_MAX_RISE cells
        // whether it has a ceiling, and the gap sort after.
        let bound = cells * 5 * (ROOM_MAX_RISE as u32 + 1) + 256;
        eprintln!("a {cells}-cell room: {} reads, {took:?} in this build", reads.get());
        assert!(reads.get() <= bound, "{} reads for {cells} cells", reads.get());
    }
}
