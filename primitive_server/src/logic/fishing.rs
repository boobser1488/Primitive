//! Fishing, on the server: which cells are traps and what goes into them,
//! and who has a line in the water.
//!
//! The rules both sides agree on -- what water holds a fish, what a trap
//! and a rod each want of it, how the hour and the sky move a bite -- are in
//! `primitive_shared::fishing`, with the argument for having exactly these
//! two ways. This is the part only the authority has: the clock, the rolls
//! and the list of where the traps are.
//!
//! ## Where a trap's fish are, and where the trap is
//!
//! **The fish are in the block**, in the variant (`types::trap_catch`), so a
//! full trap is full on the wire, in the save and after a crash with no file
//! of its own for the count. **Which cells are traps is here**, in a set
//! saved to `traps.bin`, because the clock has to find them and the world
//! cannot be asked: a scan of the loaded chunks for traps four times a day is
//! the hundred million cells `logic::carrion` already declined to walk to
//! find, usually, nothing.
//!
//! A trap joins the way a carcass does -- through `notify_mechanics`, so
//! setting one is the edit that lists it -- and a trap this server has never
//! heard of (one from a world saved before the file, or one set by a mod)
//! joins the first time anything happens in its cell.
//!
//! ## A trap nobody is near still fishes
//!
//! **Unlike a carcass, which keeps where nobody has loaded it.** Rot that
//! stops out of sight is a favour; a trap that stops out of sight is a trap
//! that only works while you stand beside it, which is the one thing a trap
//! is for not doing. So a step that finds a trap's chunk unloaded is *owed*
//! ([`OWED_STEPS_MAX`]), and rolled the next time the chunk is there -- in
//! the water it is in then, which is the only water there is to ask.
//!
//! ## A line in the water
//!
//! A cast is held here, by player, and is not saved: a line does not outlive
//! the session of the hand holding it. It lands its fish on its own when the
//! roll comes up, and it ends when the fisher lets go of it -- another slot
//! in the hand, a walk away past `fishing::CAST_HOLDS`, the float's cell no
//! longer water, the player gone. Each of those is a rule the client
//! mirrors to stop drawing the float, from the same shared function.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::time::Instant;

use primitive_shared::fishing::{Strike, TRAP_HOLDS};
use primitive_shared::protocol::{BlockChange, PlayerId};
use primitive_shared::types::{block_kind, trap_catch, trap_holding, BlockId, BLOCK_FISH_TRAP};
use primitive_shared::{saltpan, snare};
use serde::{Deserialize, Serialize};

use crate::logic::rng::Rng;

/// What the place a set thing stands in is like, for one step of the clock:
/// the three things this list steps, each asking its own question of the
/// world (the tick's `fill_traps` asks it).
///
/// **One list for all three, and not a list each.** A snare and a salt pan
/// are set and walked away from exactly as a fish trap is -- they fill on the
/// rot clock, they must go on filling in a chunk nobody has loaded, and they
/// join the list when they are set -- and the list, its file and its owed
/// steps are already that. A second and third `traps.bin` would be the same
/// code twice more with a different block in it.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Setting {
    /// A fish trap: the chance of a fish this step (`fishing::trap_chance`).
    Water(f32),
    /// A snare: the chance of a hare this step (`snare::catch_chance`).
    Ground(f32),
    /// A salt pan: the sky over it this step (`saltpan::step`).
    Sky { rained_on: bool, roofed: bool, air_c: f32 },
}

/// Is this something the list steps: a fish trap, a snare, a salt pan?
pub fn is_listed(block: BlockId) -> bool {
    block_kind(block) == BLOCK_FISH_TRAP || snare::is_snare(block) || saltpan::is_pan(block)
}

/// Its own version, independent of the world's.
const SAVE_FORMAT_VERSION: u32 = 1;

/// Where a trap is, in world cells.
pub type TrapPos = (i32, i32, i32);

/// **How many steps a trap out of sight is owed, at most**: three days.
///
/// Enough that a trap left by a camp a player walked away from for a long
/// evening is as full on their return as if they had stayed. Not unbounded,
/// because an owed count is a promise kept in a file, and a trap on a server
/// nobody visited for a year owes nothing a cap of three days would not
/// already have filled.
pub const OWED_STEPS_MAX: u8 = 12;

/// Where a line is in its life.
///
/// **A line has four states and the player can only act in two of them**,
/// which is what makes the strike a decision rather than a key to hold:
/// striking in `Waiting` is striking at nothing and ends the cast, and
/// reeling means something only in `Fighting`.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Phase {
    /// The float is still landing. Seconds left of it.
    Settling(f32),
    /// On the water, waiting. `Cast::bite_in` is counting down.
    Waiting,
    /// **Under.** Seconds left to strike (`fishing::STRIKE_SECONDS`).
    Dipping(f32),
    /// On, and being fought.
    Fighting(primitive_shared::fishing::Fight),
}

impl Phase {
    /// The byte the wire carries (`protocol::ServerMessage::Line`).
    ///
    /// A number rather than the enum itself, because the enum holds a
    /// `Fight` -- five floats of server-side state that no client has any
    /// business seeing, and that would pin the fight's shape to the protocol
    /// version the day anybody tunes it.
    pub fn code(self) -> u8 {
        match self {
            Phase::Settling(_) => 0,
            Phase::Waiting => 1,
            Phase::Dipping(_) => 2,
            Phase::Fighting(_) => 3,
        }
    }
}

/// A line in the water.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Cast {
    /// The cell of water the float is on.
    pub float: (i32, i32, i32),
    /// The pack slot the rod was in when it was cast. Another slot selected
    /// is the rod put down.
    pub slot: usize,
    /// Seconds until a fish takes it.
    pub bite_in: f32,
    /// The average wait this water gives (`fishing::rod_wait_seconds`), kept
    /// so a bite that was missed can be rolled again without surveying the
    /// water a second time -- and so the water is judged as it was when the
    /// line went in, which is the hour the player chose.
    pub mean: f32,
    /// What is on the hook, and the slot it came out of. `None` is a bare
    /// hook, which still fishes (`fishing::BARE_HOOK`).
    pub bait: Option<(usize, primitive_shared::fishing::Bait)>,
    /// Which fish is on it, rolled when the bite comes rather than at the
    /// cast: the roll has to see the bait that is on the hook *now*, and a
    /// cast can outlive several baits.
    pub hooked: Option<primitive_shared::animals::Species>,
    pub phase: Phase,
    /// Is the hand pulling? `protocol::ClientMessage::Reel`.
    pub reeling: bool,
    /// How well this water fishes, for the client to twitch the float by.
    pub liveliness: u8,
    /// When the fisher's connection began. A player id is a connection
    /// number handed out again once it is free, and a cast must not be
    /// inherited by whoever joins next with the same number.
    pub joined_at: Instant,
}

/// What happened to a line in one step, for the server to act on.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum CastEvent {
    /// The float went under: a fish has the bait and the strike window is
    /// open.
    Dipped,
    /// The window closed with no strike. **The bait is gone** -- something
    /// ate it -- and the line goes back to waiting.
    Missed,
    /// The fish is at the bank.
    Landed(primitive_shared::animals::Species),
    /// The line parted or the hook pulled out.
    Lost,
    /// The fisher let go of it: another slot, a walk away, a drained pond.
    Ended,
    /// A fight is on and the strain has moved.
    Straining,
}

/// Every trap and every line.
pub struct Fishing {
    /// Trap -> steps of the clock it is owed from while it was not loaded.
    traps: HashMap<TrapPos, u8>,
    /// Cells that changed and have not been looked at. The `CellMechanic`
    /// contract: queued here, never a world read.
    pending: Vec<TrapPos>,
    casts: HashMap<PlayerId, Cast>,
    /// How hard each spot has been fished lately: the count of fish taken
    /// out of it, and when that count was last touched
    /// (`fishing::pressure_factor`).
    ///
    /// **Not saved, and that is a decision.** A spot forgives a fish every
    /// twenty minutes (`fishing::SPOT_RECOVERS_SECONDS`), so the whole life
    /// of an entry is shorter than an evening; a file for it would be a
    /// second save format, a second version number and a second migration,
    /// to make a restart of the server stop forgiving a pool it was going to
    /// forgive anyway. The one thing it buys somebody determined enough to
    /// restart a world between casts is two minutes of fishing.
    pressure: HashMap<TrapPos, (f32, Instant)>,
    dirty: bool,
    rng: Rng,
}

impl Default for Fishing {
    fn default() -> Self {
        Self::new()
    }
}

impl Fishing {
    pub fn new() -> Self {
        Self {
            traps: HashMap::new(),
            pending: Vec::new(),
            casts: HashMap::new(),
            pressure: HashMap::new(),
            dirty: false,
            rng: Rng::from_clock(),
        }
    }

    /// The same, with a stated seed, for a test that wants the same fish
    /// twice.
    pub fn seeded(seed: u64) -> Self {
        Self {
            rng: Rng::seeded(seed),
            ..Self::new()
        }
    }

    /// How long this cast waits for its bite: an exponential round `mean`
    /// seconds, never under `fishing::ROD_SHORTEST_SECONDS`. **Memoryless on
    /// purpose** -- see `cast` -- and rolled here so the line and the trap
    /// draw from one stream.
    pub fn roll_wait(&mut self, mean: f32) -> f32 {
        // 1 - u is in (0, 1], so the logarithm is finite.
        let u = 1.0 - self.rng.next_f32();
        (-u.ln() * mean).max(primitive_shared::fishing::ROD_SHORTEST_SECONDS)
    }

    /// A roll in `0..n`.
    pub fn roll_below(&mut self, n: u32) -> u32 {
        self.rng.below(n)
    }

    /// A roll in `0..1`, for which fish took the bait
    /// (`fishing::rod_species`).
    pub fn roll_unit(&mut self) -> f32 {
        self.rng.next_f32()
    }

    /// How many traps this server knows of. Reported in `/stats`-shaped
    /// places and read by the tests.
    pub fn traps(&self) -> usize {
        self.traps.len()
    }

    pub fn is_dirty(&self) -> bool {
        self.dirty
    }

    /// A cell changed. Queued; the world is not ours to read here.
    pub fn on_block_changed(&mut self, gx: i32, gy: i32, gz: i32) {
        self.pending.push((gx, gy, gz));
    }

    /// Reconciles the queue against the world: a trap joins, and a cell that
    /// stopped being one -- lifted, broken, built over -- leaves.
    ///
    /// Every tick, on a budget, as the carrion's is: the queue is every edit
    /// in the world and nearly none of them are traps.
    pub fn reconcile(&mut self, budget: usize, mut block_at: impl FnMut(TrapPos) -> Option<BlockId>) {
        let batch: Vec<TrapPos> = self.pending.drain(..budget.min(self.pending.len())).collect();
        for at in batch {
            let Some(block) = block_at(at) else { continue };
            match (is_listed(block), self.traps.contains_key(&at)) {
                (true, false) => {
                    self.traps.insert(at, 0);
                    self.dirty = true;
                }
                (false, true) => {
                    self.traps.remove(&at);
                    self.dirty = true;
                }
                _ => {}
            }
        }
    }

    /// One step of the rot clock over every trap. Hands back the traps that
    /// changed, as the blocks to write.
    ///
    /// `read` answers what is in the cell and what the place is like for
    /// whatever is set there ([`Setting`]): `None` for a chunk nobody has
    /// loaded, which is owed the step rather than skipped -- a snare and a
    /// salt pan on the same terms as a fish trap, because each is a thing
    /// set and walked away from.
    pub fn step_traps(&mut self, mut read: impl FnMut(TrapPos) -> Option<(BlockId, Setting)>) -> Vec<BlockChange> {
        let mut changes = Vec::new();
        let mut gone = Vec::new();
        let rng = &mut self.rng;
        for (&at, owed) in self.traps.iter_mut() {
            let Some((block, setting)) = read(at) else {
                if *owed < OWED_STEPS_MAX {
                    *owed += 1;
                    self.dirty = true;
                }
                continue;
            };
            if !is_listed(block) {
                gone.push(at);
                continue;
            }
            let steps = 1 + u32::from(*owed);
            if *owed > 0 {
                *owed = 0;
                self.dirty = true;
            }
            let mut now = block;
            for _ in 0..steps {
                now = match (block_kind(now), setting) {
                    (BLOCK_FISH_TRAP, Setting::Water(chance)) => {
                        let fish = trap_catch(now);
                        if fish < TRAP_HOLDS && rng.chance(chance) {
                            trap_holding(fish + 1)
                        } else {
                            now
                        }
                    }
                    (_, Setting::Ground(chance)) if snare::is_snare(now) => snare::step(now, chance, rng.next_f32()),
                    (_, Setting::Sky { rained_on, roofed, air_c }) if saltpan::is_pan(now) => {
                        saltpan::step(now, rained_on, roofed, air_c)
                    }
                    // A setting that does not fit the thing -- the world
                    // changed between the read and here -- changes nothing.
                    _ => now,
                };
            }
            if now != block {
                changes.push(BlockChange {
                    global_x: at.0,
                    global_y: at.1,
                    global_z: at.2,
                    block_id: now,
                });
            }
        }
        for at in gone {
            self.traps.remove(&at);
            self.dirty = true;
        }
        changes
    }

    /// Puts a line in the water for `player`, replacing any they had.
    ///
    /// A second cast is a new roll, and that costs nothing and buys nothing:
    /// the wait is memoryless (`fishing::rod_wait_seconds`), so recasting
    /// does not hurry a bite and sitting on a cast does not slow one.
    pub fn cast(&mut self, player: PlayerId, cast: Cast) {
        self.casts.insert(player, cast);
    }

    /// Takes the line out of the water, if there was one.
    pub fn reel_in(&mut self, player: PlayerId) -> Option<Cast> {
        self.casts.remove(&player)
    }

    /// Everyone with a line in the water.
    pub fn fishers(&self) -> Vec<PlayerId> {
        self.casts.keys().copied().collect()
    }

    pub fn cast_of(&self, player: PlayerId) -> Option<Cast> {
        self.casts.get(&player).copied()
    }

    /// How much has been taken out of the spot a float is in, with the time
    /// since it was last fished forgiven (`fishing::pressure_recovered`).
    pub fn pressure_at(&self, float: (i32, i32, i32), now: Instant) -> f32 {
        let key = primitive_shared::fishing::spot_key(float);
        self.pressure.get(&key).map_or(0.0, |&(taken, when)| {
            primitive_shared::fishing::pressure_recovered(taken, now.saturating_duration_since(when).as_secs_f32())
        })
    }

    /// One more fish out of this spot.
    ///
    /// **Counted where the float was and not where the player stood**, so a
    /// fisher who casts from one rock into three different bays tires three
    /// bays, and one who stands in three places casting into the same hole
    /// tires the hole.
    pub fn fish_taken(&mut self, float: (i32, i32, i32), now: Instant) {
        let key = primitive_shared::fishing::spot_key(float);
        let taken = (self.pressure_at(float, now) + 1.0).min(primitive_shared::fishing::SPOT_HOLDS);
        self.pressure.insert(key, (taken, now));
        // A spot that has come all the way back is forgotten, so an hour of
        // somebody walking a coastline does not leave a map entry a cell.
        self.pressure.retain(|_, &mut (taken, when)| {
            primitive_shared::fishing::pressure_recovered(taken, now.saturating_duration_since(when).as_secs_f32()) > 0.0
        });
    }

    /// How many spots are being rested. For the tests and for `/stats`.
    pub fn tired_spots(&self) -> usize {
        self.pressure.len()
    }

    /// **The strike.** What it does depends on the phase, and refusing is
    /// half of what it is for -- see `fishing::Strike`.
    pub fn strike(&mut self, player: PlayerId) -> Strike {
        let Some(cast) = self.casts.get_mut(&player) else {
            return Strike::NotFishing;
        };
        match cast.phase {
            // Under: the hook goes in, and what is on it was decided when it
            // took the bait.
            Phase::Dipping(_) => {
                let Some(hooked) = cast.hooked else {
                    cast.phase = Phase::Waiting;
                    return Strike::TooEarly;
                };
                cast.phase = Phase::Fighting(primitive_shared::fishing::Fight::new(hooked));
                cast.reeling = false;
                Strike::Hooked
            }
            // Striking at nothing: the float comes out of the water and that
            // cast is over. This is the cost that makes it a decision.
            Phase::Waiting | Phase::Settling(_) => {
                self.casts.remove(&player);
                Strike::TooEarly
            }
            // Already on: a second tap is not a second strike.
            Phase::Fighting(_) => Strike::NotFishing,
        }
    }

    /// Which way the hand is, while a fish is on.
    pub fn reel(&mut self, player: PlayerId, pulling: bool) {
        if let Some(cast) = self.casts.get_mut(&player) {
            cast.reeling = pulling;
        }
    }

    /// Counts every line down. `holds` is asked of each first -- is the
    /// fisher still there, still holding the rod, still near the float, is
    /// the float still on water -- and a line it says no to is gone.
    ///
    /// `takes` is asked when a wait runs out: which fish took the bait, out
    /// of the water as it is *now* (`fishing::rod_species`). `None` there
    /// means nothing in this water wants what is on the hook, and the wait
    /// starts again -- the float sits, which is the honest picture of a
    /// berry on a hook in a sea of cod.
    ///
    /// Answers what happened to each line, for the caller to act on.
    pub fn step_casts(
        &mut self,
        dt: f32,
        mut holds: impl FnMut(PlayerId, &Cast) -> bool,
        mut takes: impl FnMut(PlayerId, &Cast, f32) -> Option<primitive_shared::animals::Species>,
    ) -> Vec<(PlayerId, Cast, CastEvent)> {
        use primitive_shared::fishing::{Fought, STRIKE_SECONDS};
        let mut events = Vec::new();
        // The waits a step might need, rolled before the map is walked: the
        // random stream cannot be reached from inside `retain`, and one roll
        // per line in a fixed order is a stream that does not depend on what
        // order a hash map happens to be in.
        //
        // The same goes for the roll that decides *which* fish takes the
        // bait: the caller answers that from the world (`rod_species`), and
        // it cannot reach into this rng while this is borrowed.
        let mut waits: HashMap<PlayerId, (f32, f32)> = HashMap::new();
        let means: Vec<(PlayerId, f32)> = self.casts.iter().map(|(&player, cast)| (player, cast.mean)).collect();
        for (player, mean) in means {
            let wait = self.roll_wait(mean);
            let which = self.roll_unit();
            waits.insert(player, (wait, which));
        }
        self.casts.retain(|&player, cast| {
            if !holds(player, cast) {
                events.push((player, *cast, CastEvent::Ended));
                return false;
            }
            match cast.phase {
                Phase::Settling(left) => {
                    let left = left - dt;
                    cast.phase = if left > 0.0 { Phase::Settling(left) } else { Phase::Waiting };
                    true
                }
                Phase::Waiting => {
                    cast.bite_in -= dt;
                    if cast.bite_in > 0.0 {
                        return true;
                    }
                    // Something has the bait. Which fish is decided now, and
                    // the float only goes under if something will take it.
                    let (wait, which) = waits.get(&player).copied().unwrap_or((cast.mean, 0.5));
                    cast.hooked = takes(player, cast, which);
                    if cast.hooked.is_none() {
                        cast.bite_in = wait;
                        return true;
                    }
                    cast.phase = Phase::Dipping(STRIKE_SECONDS);
                    events.push((player, *cast, CastEvent::Dipped));
                    true
                }
                Phase::Dipping(left) => {
                    let left = left - dt;
                    if left > 0.0 {
                        cast.phase = Phase::Dipping(left);
                        return true;
                    }
                    // Too slow: it ate the bait and went. The line stays in
                    // the water and waits again.
                    cast.phase = Phase::Waiting;
                    cast.hooked = None;
                    cast.bite_in = waits.get(&player).copied().map_or(cast.mean, |(wait, _)| wait);
                    events.push((player, *cast, CastEvent::Missed));
                    true
                }
                Phase::Fighting(mut fight) => {
                    let fought = fight.step(dt, cast.reeling);
                    cast.phase = Phase::Fighting(fight);
                    match fought {
                        Fought::On => {
                            events.push((player, *cast, CastEvent::Straining));
                            true
                        }
                        Fought::Landed => {
                            let species = cast.hooked.unwrap_or(primitive_shared::animals::Species::Fish);
                            events.push((player, *cast, CastEvent::Landed(species)));
                            false
                        }
                        Fought::Lost => {
                            events.push((player, *cast, CastEvent::Lost));
                            false
                        }
                    }
                }
            }
        });
        events
    }

    fn save_path(dir: &Path) -> PathBuf {
        dir.join("traps.bin")
    }

    /// Writes the traps out, atomically, the way the carrion is written.
    pub fn save(&mut self, dir: &Path) -> std::io::Result<usize> {
        std::fs::create_dir_all(dir)?;
        let mut traps: Vec<(TrapPos, u8)> = self.traps.iter().map(|(&at, &owed)| (at, owed)).collect();
        // Stable bytes for the same world.
        traps.sort_by_key(|&(at, _)| at);
        let count = traps.len();
        let bytes = bincode::serialize(&SaveFile {
            version: SAVE_FORMAT_VERSION,
            traps,
        })
        .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))?;
        let final_path = Self::save_path(dir);
        let tmp_path = final_path.with_extension("bin.tmp");
        std::fs::write(&tmp_path, &bytes)?;
        std::fs::rename(&tmp_path, &final_path)?;
        self.dirty = false;
        Ok(count)
    }

    /// Reads them back. **A missing or unreadable file lists no traps**, and
    /// the fish already in them stay in them -- they are in the world -- so
    /// what is lost is only the clock until something next happens in each
    /// cell. The carrion's bargain: a smaller catch, never a world that will
    /// not start.
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
        self.traps = save
            .traps
            .into_iter()
            .map(|(at, owed)| (at, owed.min(OWED_STEPS_MAX)))
            .collect();
        self.dirty = false;
        Ok(self.traps.len())
    }
}

#[derive(Serialize, Deserialize)]
struct SaveFile {
    version: u32,
    traps: Vec<(TrapPos, u8)>,
}

#[cfg(test)]
mod tests {
    use super::*;
    use primitive_shared::animals::Species;
    use primitive_shared::types::{BLOCK_AIR, BLOCK_STONE};

    const AT: TrapPos = (3, 40, -9);

    fn listed(fishing: &mut Fishing, block: BlockId) {
        fishing.on_block_changed(AT.0, AT.1, AT.2);
        fishing.reconcile(64, |_| Some(block));
    }

    #[test]
    fn setting_a_trap_lists_it_and_lifting_it_forgets_it() {
        let mut fishing = Fishing::seeded(5);
        listed(&mut fishing, BLOCK_STONE);
        assert_eq!(fishing.traps(), 0, "a stone was listed as a trap");
        listed(&mut fishing, BLOCK_FISH_TRAP);
        assert_eq!(fishing.traps(), 1);
        listed(&mut fishing, BLOCK_AIR);
        assert_eq!(fishing.traps(), 0, "a lifted trap is still fished");
    }

    #[test]
    fn a_snare_and_a_salt_pan_are_listed_and_stepped_as_a_trap_is() {
        use primitive_shared::types::{BLOCK_SALT_PAN_SALT, BLOCK_SNARE, BLOCK_SNARE_SPRUNG};
        let mut fishing = Fishing::seeded(9);
        listed(&mut fishing, BLOCK_SNARE);
        assert_eq!(fishing.traps(), 1, "a snare was not listed");
        // A certain catch, then a day in the noose: robbed.
        let mut block = BLOCK_SNARE;
        for _ in 0..8 {
            for change in fishing.step_traps(|_| Some((block, Setting::Ground(1.0)))) {
                block = change.block_id;
            }
        }
        assert_eq!(block, BLOCK_SNARE_SPRUNG, "a hare left out for two days was still in the snare");
        // A pan of the sea under a warm dry sky is a crust in a day, and owed
        // steps count while nobody has the shore loaded.
        let mut pans = Fishing::seeded(9);
        let brine = saltpan::brine(0);
        listed(&mut pans, brine);
        for _ in 0..3 {
            assert!(pans.step_traps(|_| None).is_empty());
        }
        let sky = Setting::Sky { rained_on: false, roofed: false, air_c: 28.0 };
        let changes = pans.step_traps(|_| Some((brine, sky)));
        assert_eq!(changes.first().map(|c| c.block_id), Some(BLOCK_SALT_PAN_SALT), "the owed sun dried nothing");
    }

    #[test]
    fn a_trap_in_good_water_fills_and_stops_at_what_it_holds() {
        let mut fishing = Fishing::seeded(5);
        listed(&mut fishing, BLOCK_FISH_TRAP);
        let mut block = BLOCK_FISH_TRAP;
        for _ in 0..100 {
            for change in fishing.step_traps(|_| Some((block, Setting::Water(1.0)))) {
                block = change.block_id;
            }
        }
        assert_eq!(trap_catch(block), TRAP_HOLDS, "a certain catch never filled the trap");
        assert!(primitive_shared::types::is_known_block(block), "a full trap is an invented id");
    }

    #[test]
    fn a_trap_in_water_that_holds_no_fish_stays_empty() {
        let mut fishing = Fishing::seeded(5);
        listed(&mut fishing, BLOCK_FISH_TRAP);
        for _ in 0..400 {
            assert!(fishing.step_traps(|_| Some((BLOCK_FISH_TRAP, Setting::Water(0.0)))).is_empty(), "a fish in a puddle");
        }
    }

    #[test]
    fn a_trap_nobody_has_loaded_is_owed_its_steps_and_fills_when_it_is_seen_again() {
        let mut fishing = Fishing::seeded(5);
        listed(&mut fishing, BLOCK_FISH_TRAP);
        for _ in 0..OWED_STEPS_MAX + 5 {
            assert!(fishing.step_traps(|_| None).is_empty());
        }
        let changes = fishing.step_traps(|_| Some((BLOCK_FISH_TRAP, Setting::Water(1.0))));
        assert_eq!(changes.len(), 1, "a trap out of sight for three days came back empty");
        assert_eq!(trap_catch(changes[0].block_id), TRAP_HOLDS);
        // ...and what it was owed is paid once, not again.
        let again = fishing.step_traps(|_| Some((BLOCK_FISH_TRAP, Setting::Water(0.0))));
        assert!(again.is_empty(), "the owed steps were paid twice");
    }

    /// A line in the water with `bait` on it, already settled.
    fn a_line(bait: Option<primitive_shared::fishing::Bait>, bite_in: f32) -> Cast {
        Cast {
            float: (0, 20, 0),
            slot: 2,
            bite_in,
            mean: 30.0,
            bait: bait.map(|bait| (3, bait)),
            hooked: None,
            phase: Phase::Waiting,
            reeling: false,
            liveliness: 128,
            joined_at: Instant::now(),
        }
    }

    /// Steps a line, with the fisher holding on and a trout on offer.
    fn step(fishing: &mut Fishing, dt: f32) -> Vec<(PlayerId, Cast, CastEvent)> {
        fishing.step_casts(dt, |_, _| true, |_, _, _| Some(Species::Trout))
    }

    #[test]
    fn a_float_goes_under_when_the_wait_is_up_and_not_before() {
        let mut fishing = Fishing::seeded(5);
        fishing.cast(9, a_line(None, 10.0));
        assert!(step(&mut fishing, 9.0).is_empty(), "a bite before its time");
        let events = step(&mut fishing, 2.0);
        assert_eq!(events.len(), 1);
        assert!(matches!(events[0].2, CastEvent::Dipped), "the wait ran out and nothing happened");
        assert!(fishing.cast_of(9).is_some(), "the line came out of the water on a bite");
    }

    #[test]
    fn a_line_let_go_of_lands_nothing() {
        let mut fishing = Fishing::seeded(5);
        fishing.cast(4, a_line(None, 1.0));
        let events = fishing.step_casts(5.0, |_, _| false, |_, _, _| Some(Species::Fish));
        assert!(
            events.iter().all(|(_, _, event)| matches!(event, CastEvent::Ended)),
            "a fish came up on a rod nobody held"
        );
        assert!(fishing.cast_of(4).is_none());
    }

    #[test]
    fn the_server_refuses_a_strike_before_the_float_goes_under() {
        let mut fishing = Fishing::seeded(5);
        fishing.cast(1, a_line(None, 10.0));
        assert_eq!(fishing.strike(1), Strike::TooEarly, "a strike into open water was allowed");
        assert!(fishing.cast_of(1).is_none(), "striking at nothing left the line in the water");
        // ...and a strike with no line at all is neither a catch nor a
        // scolding.
        assert_eq!(fishing.strike(1), Strike::NotFishing);
        // A float still landing is not a float that has been taken.
        let mut settling = a_line(None, 10.0);
        settling.phase = Phase::Settling(primitive_shared::fishing::SETTLE_SECONDS);
        fishing.cast(2, settling);
        assert_eq!(fishing.strike(2), Strike::TooEarly, "a strike at the splash of the cast hooked something");
    }

    #[test]
    fn a_strike_inside_the_dip_hooks_the_fish_and_one_after_it_does_not() {
        let mut fishing = Fishing::seeded(5);
        fishing.cast(1, a_line(Some(primitive_shared::fishing::Bait::Worm), 0.5));
        assert!(matches!(step(&mut fishing, 1.0)[0].2, CastEvent::Dipped));
        assert_eq!(fishing.strike(1), Strike::Hooked);
        assert!(matches!(fishing.cast_of(1).expect("on").phase, Phase::Fighting(_)));

        // The same line, struck a second too late: the fish has the bait
        // and the float is back on the water.
        let mut late = Fishing::seeded(5);
        late.cast(1, a_line(Some(primitive_shared::fishing::Bait::Worm), 0.5));
        assert!(matches!(step(&mut late, 1.0)[0].2, CastEvent::Dipped));
        let missed = step(&mut late, primitive_shared::fishing::STRIKE_SECONDS + 0.1);
        assert!(matches!(missed[0].2, CastEvent::Missed), "the dip never closed");
        assert_eq!(late.strike(1), Strike::TooEarly, "a strike after the dip hooked a fish");
    }

    #[test]
    fn bait_is_spent_on_a_bite_and_never_on_a_cast_that_came_to_nothing() {
        // The events are what spends it (`crate::spend_bait`), so this is
        // the test that the events only say so when a fish took it.
        let mut fishing = Fishing::seeded(5);
        fishing.cast(1, a_line(Some(primitive_shared::fishing::Bait::Worm), 30.0));
        let quiet: Vec<CastEvent> = (0..60).flat_map(|_| step(&mut fishing, 0.25)).map(|(_, _, e)| e).collect();
        assert!(
            !quiet.iter().any(|e| matches!(e, CastEvent::Missed | CastEvent::Landed(_) | CastEvent::Lost)),
            "a worm went missing off a line nothing had touched: {quiet:?}"
        );
        // ...and a line nothing in the water wants waits for ever without
        // ever losing its bait.
        let mut nobody = Fishing::seeded(7);
        nobody.cast(1, a_line(Some(primitive_shared::fishing::Bait::Berry), 0.1));
        let events: Vec<CastEvent> = (0..40)
            .flat_map(|_| nobody.step_casts(0.5, |_, _| true, |_, _, _| None))
            .map(|(_, _, e)| e)
            .collect();
        assert!(events.is_empty(), "a berry in a sea of cod cost the player something: {events:?}");
    }

    #[test]
    fn a_fish_fought_carelessly_breaks_the_line_and_one_fought_well_comes_in() {
        // Reeling without ever letting go: the strain gets away on the
        // first run.
        let mut hard = Fishing::seeded(5);
        let mut cast = a_line(Some(primitive_shared::fishing::Bait::Scrap), 0.0);
        cast.hooked = Some(Species::Pike);
        cast.phase = Phase::Fighting(primitive_shared::fishing::Fight::new(Species::Pike));
        cast.reeling = true;
        hard.cast(1, cast);
        let mut ending = None;
        for _ in 0..400 {
            for (_, _, event) in hard.step_casts(0.05, |_, _| true, |_, _, _| None) {
                if matches!(event, CastEvent::Lost | CastEvent::Landed(_)) {
                    ending = Some(event);
                }
            }
        }
        assert!(matches!(ending, Some(CastEvent::Lost)), "a pike hauled at without pause came in: {ending:?}");

        // The same pike, given line whenever it runs.
        let mut careful = Fishing::seeded(5);
        let mut cast = a_line(Some(primitive_shared::fishing::Bait::Scrap), 0.0);
        cast.hooked = Some(Species::Pike);
        cast.phase = Phase::Fighting(primitive_shared::fishing::Fight::new(Species::Pike));
        careful.cast(1, cast);
        let mut ending = None;
        for _ in 0..1200 {
            let pulling = match careful.cast_of(1).map(|cast| cast.phase) {
                Some(Phase::Fighting(fight)) => fight.running() < 0.45 && fight.strain < 0.7,
                _ => false,
            };
            careful.reel(1, pulling);
            for (_, _, event) in careful.step_casts(0.05, |_, _| true, |_, _, _| None) {
                if matches!(event, CastEvent::Lost | CastEvent::Landed(_)) {
                    ending = Some(event);
                }
            }
        }
        assert!(
            matches!(ending, Some(CastEvent::Landed(Species::Pike))),
            "a pike played properly still got away: {ending:?}"
        );
    }

    #[test]
    fn the_same_spot_fishes_out_and_comes_back() {
        use primitive_shared::fishing::{pressure_factor, SPOT_HOLDS, SPOT_RECOVERS_SECONDS};
        let mut fishing = Fishing::seeded(5);
        let hole = (40, 20, -12);
        let start = Instant::now();
        assert_eq!(fishing.pressure_at(hole, start), 0.0, "an untouched pool was already tired");
        for _ in 0..SPOT_HOLDS as u32 {
            fishing.fish_taken(hole, start);
        }
        let spent = fishing.pressure_at(hole, start);
        assert!(spent >= SPOT_HOLDS - 0.01, "six fish out of one hole left it at {spent}");
        assert!(pressure_factor(spent) < 0.25, "a hole fished out still fishes at {}", pressure_factor(spent));
        // ...and the water twenty paces along is untouched, which is the
        // whole point of a spot being a place.
        assert_eq!(fishing.pressure_at((40 + 20, 20, -12), start), 0.0, "the next bay was tired too");
        // An hour later it is itself again, and forgotten.
        let later = start + std::time::Duration::from_secs_f32(SPOT_RECOVERS_SECONDS * SPOT_HOLDS + 60.0);
        assert_eq!(fishing.pressure_at(hole, later), 0.0, "the pool never came back");
        fishing.fish_taken((900, 20, 900), later);
        assert_eq!(fishing.tired_spots(), 1, "a pool that had come back was still being remembered");
    }

    #[test]
    fn the_traps_and_what_they_are_owed_survive_a_restart() {
        let dir = std::env::temp_dir().join(format!("primitive-traps-{}", std::process::id()));
        let mut fishing = Fishing::seeded(5);
        listed(&mut fishing, BLOCK_FISH_TRAP);
        fishing.step_traps(|_| None);
        assert!(fishing.is_dirty());
        assert_eq!(fishing.save(&dir).expect("saved"), 1);
        let mut back = Fishing::new();
        assert_eq!(back.load(&dir).expect("loaded"), 1);
        assert_eq!(back.traps.get(&AT), Some(&1), "the owed step was lost across a restart");
        let _ = std::fs::remove_dir_all(&dir);
    }
}
