//! Rats: where they come from, and what they take.
//!
//! The rules are `primitive_shared::vermin` -- both sides read those --
//! and the map of where a player actually lives is
//! `primitive_shared::haunt`. This is the server's half: the clock, the
//! spawner and the raid.
//!
//! ## The shape, and why it is a pass rather than an animal's mind
//!
//! A rat is an ordinary animal while it is walking about: it spawns
//! through `logic::animals::Animals::spawn`, it is drawn and hit and
//! killed like a hare, and it flees a player because `awareness` says so.
//! What is *not* in the animal is the stealing, and that is deliberate.
//!
//! The obvious design is a rat that walks to a chest, opens it and takes
//! something -- a state machine with a target cell, a path and a
//! gesture. It was rejected for a reason that shows up immediately in
//! the test: **a chest is inside a room, and the rat is a collider that
//! cannot open a door.** Pathing vermin into a sealed storeroom is
//! either impossible (and then the mechanic never fires and nobody ever
//! sees it) or a hole in the collider (and then rats walk through walls,
//! which is the bug report). The honest reading is the real one: a rat
//! got in through a gap you did not know about, and what you see in the
//! morning is what is missing.
//!
//! So the raid is a **pass over the containers near a rat**, on the slow
//! clock, exactly like `rot`'s pass over the chests. What the rat's body
//! is for is being *seen* -- a shape along a skirting board at night,
//! and something to swing at.
//!
//! What stops a player simply walling everything in and forgetting about
//! it is that the pass does not care about walls either: it cares about
//! [`REACH`], and about the light. A storeroom with a torch in it is
//! safe while the torch burns.
//!
//! ## The clock
//!
//! One pass every [`PASS_SECONDS`]. Slow on purpose: a rat that took
//! something every tick would empty a chest in a minute, and the loss has
//! to be small enough per night that a player who checks their stores in
//! the morning is annoyed rather than ruined.

use std::collections::HashSet;
use std::path::{Path, PathBuf};

use primitive_shared::animals::Species;
use primitive_shared::haunt::{self, Haunts};
use primitive_shared::vermin;

use crate::logic::containers::{ChestPos, Chests};

/// How near a rat has to be to a container to get into it, in blocks.
///
/// Three. Enough that a rat in the corridor reaches the chest in the
/// storeroom next to it -- which is the case the module note is about --
/// and not so far that one rat in a hall raids a whole house. It is also
/// about as far as a player can see a rat at night, so the two
/// half-glimpses a player gets, the shape and the missing meat, are
/// about the same place.
pub const REACH: f32 = 3.0;

/// How long between raids, in seconds.
///
/// Twenty. Over a ten-minute night that is thirty chances, and one rat
/// takes one thing each time it fires -- so a night with two rats in the
/// house costs a few items and one rung off a store, not a chest. **The
/// number was forty-five first and it was wrong the other way**: nothing
/// was ever missing, and a mechanic nobody notices is a mechanic that is
/// not there.
pub const PASS_SECONDS: f32 = 20.0;

/// How often the spawner looks for somewhere to put a rat, in seconds.
///
/// Slower than the raid, because a spawn is the expensive half: it walks
/// the lived-in cells and asks the world about the light in each.
pub const SPAWN_SECONDS: f32 = 30.0;

/// How often the map of where players live is aged, in seconds.
pub const DECAY_SECONDS: f32 = 10.0;

/// Bumped whenever [`SaveFile`] changes shape. A file of another version
/// loads as an empty map (see [`Vermin::load`]) rather than as garbage.
const SAVE_FORMAT_VERSION: u32 = 1;

/// `haunts.bin`: the version, then every remembered cell and its warmth.
#[derive(serde::Serialize, serde::Deserialize)]
struct SaveFile {
    version: u32,
    cells: Vec<((i32, i32), f32)>,
}

/// The server's memory of its players' habits, and the two clocks that
/// run off it.
#[derive(Debug, Default)]
pub struct Vermin {
    haunts: Haunts,
    since_raid: f32,
    since_spawn: f32,
    since_decay: f32,
}

impl Vermin {
    pub fn new() -> Vermin {
        Vermin::default()
    }

    /// The map itself, for the save and for `/stats`.
    pub fn haunts(&self) -> &Haunts {
        &self.haunts
    }

    /// Replaces the map, on load.
    pub fn set_haunts(&mut self, haunts: Haunts) {
        self.haunts = haunts;
    }

    fn save_path(dir: &Path) -> PathBuf {
        dir.join("haunts.bin")
    }

    /// Writes the map of habits, atomically, the way the carrion is
    /// written. Always, and not only when "dirty": it changes every tick
    /// somebody stands anywhere, and it is two hundred entries at most.
    ///
    /// **Only the map.** The three clocks start again at nothing, which
    /// costs at most one raid's worth of waiting; saving them would be a
    /// format that has to change every time a clock is retuned.
    ///
    /// Nothing cools while the server is down, and that is on purpose: the
    /// world's own time stops with it (the fires, the rot, the drying rack
    /// all wait), so a house left for a night of real time has been left
    /// for no time at all.
    pub fn save(&self, dir: &Path) -> std::io::Result<usize> {
        std::fs::create_dir_all(dir)?;
        let cells = self.haunts.cells();
        let count = cells.len();
        let bytes = bincode::serialize(&SaveFile {
            version: SAVE_FORMAT_VERSION,
            cells,
        })
        .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))?;
        let final_path = Self::save_path(dir);
        let tmp_path = final_path.with_extension("bin.tmp");
        std::fs::write(&tmp_path, &bytes)?;
        std::fs::rename(&tmp_path, &final_path)?;
        Ok(count)
    }

    /// Reads the map back. A world saved before the map was (no file), or
    /// a file this version cannot read, is a world nobody has lived in yet
    /// -- the carrion's bargain: the cost is a few minutes before the rats
    /// find the house again, and refusing to start is a world nobody can
    /// play.
    pub fn load(&mut self, dir: &Path) -> std::io::Result<usize> {
        let bytes = match std::fs::read(Self::save_path(dir)) {
            Ok(bytes) => bytes,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(0),
            Err(e) => return Err(e),
        };
        let Ok(save) = bincode::deserialize::<SaveFile>(&bytes) else {
            return Ok(0);
        };
        if save.version != SAVE_FORMAT_VERSION {
            return Ok(0);
        }
        self.haunts = Haunts::from_cells(save.cells);
        Ok(self.haunts.len())
    }

    /// Notes that these players spent `dt` where they are standing, and
    /// ages the map when it is due.
    ///
    /// `dt` is clamped to a second apiece, as every interval on this
    /// server clamps its own: a frame that took five seconds must not
    /// warm a cell by five seconds of living in it. A player who alt-tabs
    /// for an hour must not come back to a mansion in the map.
    pub fn tick(&mut self, standing: &[(f32, f32)], dt: f32) {
        let dt = dt.clamp(0.0, 1.0);
        for &(x, z) in standing {
            self.haunts.visit(x, z, dt);
        }
        self.since_decay += dt;
        if self.since_decay >= DECAY_SECONDS {
            self.haunts.decay(self.since_decay);
            self.since_decay = 0.0;
        }
    }

    /// Is a raid due?
    pub fn raid_due(&mut self, dt: f32) -> bool {
        Self::due(&mut self.since_raid, dt, PASS_SECONDS)
    }

    /// ...and a look for somewhere to put one?
    pub fn spawn_due(&mut self, dt: f32) -> bool {
        Self::due(&mut self.since_spawn, dt, SPAWN_SECONDS)
    }

    fn due(since: &mut f32, dt: f32, every: f32) -> bool {
        *since += dt.clamp(0.0, 1.0);
        if *since < every {
            return false;
        }
        // The remainder is kept rather than zeroed, for `rot::Rot::due`'s
        // reason: a server a hair over its tick budget would drift slow
        // for ever.
        *since = (*since - every).min(every);
        true
    }

    /// Where a rat could come out, given where a player has been living
    /// and what the world looks like there.
    ///
    /// `dark_floor_in` is asked, for one cell of the map, for a standing
    /// place inside it that is dark enough -- `None` when there is none,
    /// which is what a lit storeroom answers. It is a closure because
    /// this module must not know what a chunk is; the world is the
    /// caller's.
    ///
    /// Warmest cell first ([`Haunts::lived_in`]), and the first answer
    /// wins: **the rats come out of the room you spend most time in.**
    /// Picking at random among the lived-in cells was the first cut and
    /// it put them in the corridor as often as the kitchen, which reads
    /// as "rats spawn near you" rather than as "rats are in your pantry".
    pub fn somewhere_to_come_out(
        &self,
        night: bool,
        mut dark_floor_in: impl FnMut((i32, i32)) -> Option<(f32, f32, f32)>,
    ) -> Option<(f32, f32, f32)> {
        if !night {
            return None;
        }
        self.haunts
            .lived_in()
            .into_iter()
            .find_map(|(cell, _)| dark_floor_in(cell))
    }

    /// How many more rats this world will take.
    ///
    /// Against [`vermin::MOST_RATS`] and counted off the live animals
    /// rather than off a tally of our own, for the reason every other
    /// cap here is counted that way: a number we keep is a number that
    /// goes wrong when something else kills one.
    pub fn room_for_more(living: usize) -> usize {
        vermin::MOST_RATS.saturating_sub(living)
    }
}

/// One raid: every container within [`REACH`] of a rat loses one thing.
///
/// Answers which containers changed, so whoever has one open is told --
/// the same contract `rot::Rot::age_chests` has, and for the same reason:
/// a chest that changes while a player is looking at it must not go on
/// showing what used to be in it.
///
/// **One container per rat per pass, and the nearest.** Letting each rat
/// work every chest in reach is how one rat empties a storeroom in a
/// night; letting each container be worked by every rat is the same thing
/// wearing a different loop. A rat is one animal and it takes one thing.
///
/// **A thing set down on the floor is one of these containers**, and that is
/// how a haunch left on the floor of a dark room goes in the night. It keeps
/// itself in the same store (`set_down_item`), so this pass reaches it with
/// no second list of floor cells to remember -- and `vermin::wants` is what
/// leaves a knife or a stone lying there. What comes out of the edit is told
/// to every client by `broadcast_chest_state`, which ends in `tell_set_down`:
/// a spoiled strip is drawn as what it now is, and the last piece eaten
/// clears its cell. Rejected: a pass of its own over the set-down cells,
/// which would be a second rule for the same mouth, with its own reach and
/// its own light, to drift from this one.
pub fn raid(
    chests: &mut Chests,
    rats: &[(f32, f32, f32)],
    mut light_at: impl FnMut(ChestPos) -> u8,
) -> Vec<ChestPos> {
    if rats.is_empty() {
        return Vec::new();
    }
    let positions = chests.positions();
    let mut changed = Vec::new();
    // A container already robbed this pass is not robbed again, whichever
    // rat is nearest it.
    let mut done: HashSet<ChestPos> = HashSet::new();
    for &rat in rats {
        let Some(at) = nearest_worth_robbing(&positions, chests, rat, &done, &mut light_at) else {
            continue;
        };
        let robbed = chests.edit(at, |contents| {
            let Some(slot) = vermin::target_slot(contents.slots()) else {
                return false;
            };
            let Some(stack) = contents.take_slot(slot) else {
                return false;
            };
            // Back into the slot it came out of, like `rot` does: a store
            // that jumps across the chest when something gets at it is a
            // store the player cannot keep an eye on. `gnaw` answering
            // `None` is the last of it eaten, and the slot stays empty.
            if let Some(left) = vermin::gnaw(stack) {
                if let Some(spill) = contents.put_in_slot(slot, left) {
                    contents.add_worn(spill.block, spill.count, spill.damage);
                }
            }
            true
        });
        if robbed {
            done.insert(at);
            changed.push(at);
        }
    }
    changed
}

/// The nearest container in reach that has something a rat wants and is
/// dark enough to be got at.
fn nearest_worth_robbing(
    positions: &[ChestPos],
    chests: &Chests,
    rat: (f32, f32, f32),
    done: &HashSet<ChestPos>,
    light_at: &mut impl FnMut(ChestPos) -> u8,
) -> Option<ChestPos> {
    let mut best: Option<(f32, ChestPos)> = None;
    for &at in positions {
        if done.contains(&at) {
            continue;
        }
        let d2 = distance2(rat, at);
        if d2 > REACH * REACH {
            continue;
        }
        if best.is_some_and(|(so_far, _)| d2 > so_far) {
            continue;
        }
        // The light is asked before the contents, because most chests in
        // most worlds hold stone and the cheap test should come first --
        // and because a lit chest is not robbed however full it is.
        if light_at(at) >= vermin::LIGHT_KEEPS_THEM_OUT {
            continue;
        }
        if vermin::target_slot(chests.contents(at).slots()).is_none() {
            continue;
        }
        // Ties broken by position, so two servers running the same world
        // rob the same chest: `positions` comes off a map and its order
        // is not the same twice.
        match best {
            Some((so_far, held)) if (d2 - so_far).abs() < 1e-6 && held < at => {}
            _ => best = Some((d2, at)),
        }
    }
    best.map(|(_, at)| at)
}

fn distance2(from: (f32, f32, f32), to: ChestPos) -> f32 {
    // The container's middle, not its corner: a chest at (0,0,0) fills the
    // cell, and measuring to the corner makes the far side of it half a
    // block further away than the near side for no reason a player could
    // see.
    let dx = from.0 - (to.0 as f32 + 0.5);
    let dy = from.1 - (to.1 as f32 + 0.5);
    let dz = from.2 - (to.2 as f32 + 0.5);
    dx * dx + dy * dy + dz * dz
}

/// Is this animal one of ours?
pub fn is_rat(species: Species) -> bool {
    species == Species::Rat
}

/// How lived-in the cell at a position is, for the debug panel and the
/// `/stats` line.
pub fn heat_at(haunts: &Haunts, x: f32, z: f32) -> f32 {
    haunts.heat_at(x, z)
}

/// Where in a cell a rat could stand, given a look at the world.
///
/// Walks the cell's columns and answers the first dark one with air over
/// solid ground. **Along the walls first**: the columns are visited in
/// ring order from the cell's edge inward, because a rat coming out in
/// the middle of a room reads as a rat falling out of the ceiling, and
/// one that appears against a wall reads as one that was behind it.
pub fn dark_floor_in(
    cell: (i32, i32),
    night: bool,
    mut column: impl FnMut(i32, i32) -> Option<(f32, u8)>,
) -> Option<(f32, f32, f32)> {
    let base = (cell.0 * haunt::CELL, cell.1 * haunt::CELL);
    let mut candidates: Vec<(i32, i32, i32)> = Vec::new();
    for dx in 0..haunt::CELL {
        for dz in 0..haunt::CELL {
            // How far in from the cell's edge this column is. Zero is the
            // wall side.
            let inset = dx
                .min(haunt::CELL - 1 - dx)
                .min(dz)
                .min(haunt::CELL - 1 - dz);
            candidates.push((inset, dx, dz));
        }
    }
    candidates.sort();
    for (_, dx, dz) in candidates {
        let (x, z) = (base.0 + dx, base.1 + dz);
        let Some((y, light)) = column(x, z) else {
            continue;
        };
        if !vermin::may_appear(night, light, f32::INFINITY) {
            continue;
        }
        return Some((x as f32 + 0.5, y, z as f32 + 0.5));
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use primitive_shared::inventory::Stack;
    use primitive_shared::types::{BLOCK_COBBLESTONE, BLOCK_DRIED_MEAT, BLOCK_HIDE};

    fn chest_with(at: ChestPos, stack: Stack) -> Chests {
        let mut chests = Chests::new();
        chests.edit(at, |contents| {
            contents.put_in_slot(0, stack);
        });
        chests
    }

    #[test]
    fn rats_come_at_night_where_a_player_has_been_living() {
        let mut vermin = Vermin::new();
        // Ten minutes in one room, which is an evening's building.
        for _ in 0..600 {
            vermin.tick(&[(4.0, 4.0)], 1.0);
        }
        let dark_floor = |cell: (i32, i32)| dark_floor_in(cell, true, |_, _| Some((64.0, 0)));
        assert!(
            vermin.somewhere_to_come_out(true, dark_floor).is_some(),
            "nothing came to a dark house at night"
        );
        // ...and not by day, whatever the map says.
        assert!(vermin.somewhere_to_come_out(false, dark_floor).is_none());
    }

    #[test]
    fn nothing_comes_out_in_a_wilderness_a_player_has_only_walked_through() {
        let mut vermin = Vermin::new();
        // An hour's walk: a second in each of a hundred cells, twice over.
        for _ in 0..2 {
            for step in 0..100 {
                vermin.tick(&[((step * haunt::CELL) as f32, 0.0)], 1.0);
            }
        }
        let anywhere = |cell: (i32, i32)| {
            let (x, z) = haunt::cell_centre(cell);
            Some((x, 64.0, z))
        };
        assert!(
            vermin.somewhere_to_come_out(true, anywhere).is_none(),
            "a walk through a wood summoned rats to it"
        );
    }

    #[test]
    fn a_lamp_in_the_storeroom_is_the_whole_of_the_defence() {
        let mut vermin = Vermin::new();
        for _ in 0..600 {
            vermin.tick(&[(4.0, 4.0)], 1.0);
        }
        let lit = |cell: (i32, i32)| {
            dark_floor_in(cell, true, |_, _| Some((64.0, vermin::LIGHT_KEEPS_THEM_OUT)))
        };
        assert!(
            vermin.somewhere_to_come_out(true, lit).is_none(),
            "a lit room is still a nest"
        );
    }

    #[test]
    fn a_rat_spoils_what_is_on_a_rack_beside_it() {
        // A rack is a container (`primitive_shared::rack`), so this is the
        // same pass that robs a chest -- which is the whole reason the
        // raid is written against containers and not against chests.
        let at = (10, 64, 10);
        let mut chests = chest_with(at, Stack::new(BLOCK_HIDE, 2));
        let rat = (10.5, 64.5, 11.0);
        let changed = raid(&mut chests, &[rat], |_| 0);
        assert_eq!(changed, vec![at], "the rack was left alone");
        let left = chests.contents(at).slots()[0].expect("the rack was stripped");
        assert_eq!(left.count, 1, "the rat took {} skins", 2 - left.count);
    }

    #[test]
    fn a_rat_on_the_other_side_of_the_house_takes_nothing() {
        let at = (10, 64, 10);
        let mut chests = chest_with(at, Stack::new(BLOCK_DRIED_MEAT, 8));
        let far = (10.5, 64.5, 10.5 + REACH + 1.0);
        assert!(raid(&mut chests, &[far], |_| 0).is_empty());
        assert_eq!(chests.contents(at).slots()[0].unwrap().count, 8);
    }

    #[test]
    fn a_lit_chest_is_not_robbed_however_full_it_is() {
        let at = (10, 64, 10);
        let mut chests = chest_with(at, Stack::new(BLOCK_DRIED_MEAT, 8));
        let rat = (10.5, 64.5, 11.0);
        assert!(raid(&mut chests, &[rat], |_| vermin::LIGHT_KEEPS_THEM_OUT).is_empty());
        assert_eq!(chests.contents(at).slots()[0].unwrap().block, BLOCK_DRIED_MEAT);
    }

    #[test]
    fn a_box_of_rubble_is_not_worth_the_trip() {
        let at = (10, 64, 10);
        let mut chests = chest_with(at, Stack::new(BLOCK_COBBLESTONE, 40));
        let rat = (10.5, 64.5, 11.0);
        assert!(raid(&mut chests, &[rat], |_| 0).is_empty());
    }

    #[test]
    fn one_rat_takes_one_thing_however_many_chests_are_in_reach() {
        let mut chests = Chests::new();
        for z in 9..=11 {
            chests.edit((10, 64, z), |contents| {
                contents.put_in_slot(0, Stack::new(BLOCK_DRIED_MEAT, 8));
            });
        }
        let changed = raid(&mut chests, &[(10.5, 64.5, 10.5)], |_| 0);
        assert_eq!(changed.len(), 1, "one rat robbed {} chests", changed.len());
    }

    #[test]
    fn the_store_comes_apart_a_rung_a_night_rather_than_all_at_once() {
        use primitive_shared::types::BLOCK_SALTED_MEAT;
        let at = (0, 64, 0);
        let mut chests = chest_with(at, Stack::new(BLOCK_DRIED_MEAT, 6));
        let rat = (0.5, 64.5, 1.0);
        raid(&mut chests, &[rat], |_| 0);
        let after = chests.contents(at).slots()[0].expect("the store went in one pass");
        assert_ne!(after.block, BLOCK_SALTED_MEAT, "the ladder went the wrong way");
        assert_eq!(after.count, 5, "the rat ate more than one");
        assert!(
            primitive_shared::vermin::spoiled(BLOCK_DRIED_MEAT).is_some(),
            "dried meat has no rung below it"
        );
    }

    #[test]
    fn the_house_is_only_ever_so_infested() {
        assert_eq!(Vermin::room_for_more(0), vermin::MOST_RATS);
        assert_eq!(Vermin::room_for_more(vermin::MOST_RATS), 0);
        assert_eq!(Vermin::room_for_more(vermin::MOST_RATS + 5), 0);
    }

    #[test]
    fn a_rat_comes_out_against_a_wall_and_not_in_the_middle_of_the_floor() {
        // Every column in the cell is a dark floor, so what decides is the
        // order -- and the order has to start at the cell's edge.
        let at = dark_floor_in((0, 0), true, |_, _| Some((64.0, 0))).expect("nowhere to stand");
        let edge = at.0.floor() as i32 == 0
            || at.2.floor() as i32 == 0
            || at.0.floor() as i32 == haunt::CELL - 1
            || at.2.floor() as i32 == haunt::CELL - 1;
        assert!(edge, "a rat came out at {at:?}, in the middle of the room");
    }

    fn scratch_dir(name: &str) -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!("primitive-haunts-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        dir
    }

    #[test]
    fn the_house_is_still_lived_in_after_the_server_restarts() {
        let dir = scratch_dir("roundtrip");
        let mut vermin = Vermin::new();
        for _ in 0..240 {
            vermin.tick(&[(3.0, 3.0), (100.0, -40.0)], 1.0);
        }
        assert!(vermin.haunts().is_lived_in(3.0, 3.0));
        let saved = vermin.save(&dir).expect("saved");
        assert_eq!(saved, vermin.haunts().len());
        let mut again = Vermin::new();
        assert_eq!(again.load(&dir).expect("loaded"), saved);
        assert_eq!(again.haunts().cells(), vermin.haunts().cells(), "the map came back different");
        assert!(again.haunts().is_lived_in(3.0, 3.0), "a restart forgot where the player lives");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_world_saved_before_the_map_was_loads_as_nobody_living_anywhere() {
        let dir = scratch_dir("old");
        std::fs::create_dir_all(&dir).expect("dir");
        let mut vermin = Vermin::new();
        assert_eq!(vermin.load(&dir).expect("an old save refused to load"), 0);
        assert!(vermin.haunts().is_empty());
        // ...and a file from some other version of the format is the same
        // empty map, not garbage and not a refusal to start.
        std::fs::write(dir.join("haunts.bin"), [9u8, 0, 0, 0, 1, 2, 3]).expect("written");
        assert_eq!(vermin.load(&dir).expect("a strange file refused to load"), 0);
        assert!(vermin.haunts().is_empty());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_map_of_habits_is_not_a_map_of_one_long_frame() {
        // A server that hung for an hour must not come back to a mansion:
        // one tick is worth at most one second of living somewhere.
        let mut vermin = Vermin::new();
        vermin.tick(&[(0.0, 0.0)], 3600.0);
        assert!(
            !vermin.haunts().is_lived_in(0.0, 0.0),
            "one frame built a home"
        );
    }
}
