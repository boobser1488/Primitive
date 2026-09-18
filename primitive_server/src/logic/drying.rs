//! Hides on racks, and the weather they cure in.
//!
//! ## Why this is a mechanic and not a recipe
//!
//! Every other transformation in this game is instantaneous: the craft
//! menu is a promise that if you are holding these things and standing
//! in the right place, you get that thing. That is the right shape for
//! tying fibre round a stick and for pouring an ingot, and it is the
//! wrong shape for curing a skin -- which is days of *doing nothing in
//! particular*, in weather that is neither wet nor freezing, somewhere
//! you can come back to.
//!
//! Making it a recipe would flatten the one process in the world whose
//! whole content is waiting somewhere suitable. So instead:
//!
//! - skins go on a rack (see [`primitive_shared::rack`], which is where
//!   the two slots live);
//! - the rack watches the sky;
//! - leather is what is in the tray when you come back.
//!
//! ## What "suitable" means
//!
//! Two conditions, and each of them is a real one rather than a number
//! chosen to make the wait longer:
//!
//! - **Rain stops it.** A skin left out in the wet does not dry, it
//!   rots -- and here the progress simply pauses, because a mechanic
//!   that silently destroyed several hours of a player's time for going
//!   inside during a storm would be a mechanic nobody uses twice.
//! - **Cold slows it.** Below freezing nothing moves at all; the
//!   warmer it is the faster it goes, up to a cap. That is what makes a
//!   tannery a thing you site rather than a block you place: the sunny
//!   bank by the river genuinely cures faster than the tundra.
//!
//! Both are read off the same climate sample the player's own
//! temperature comes from ([`crate::climate`]), so there is one answer
//! to "what is the weather like here" and the rack and the player agree
//! about it. Both also *reach the player now*: they ride along with the
//! rack's screen as a [`primitive_shared::protocol::RackState`], which
//! is the difference between a process that has rules and a process
//! whose rules anybody can find out about.
//!
//! ## Why the skins are not stored here
//!
//! They are in the container store with the chests
//! ([`crate::containers`]), and only the *progress* is here -- the same
//! split [`crate::smelting`] makes, for the same reason. A rack is a
//! container now, so opening it, moving a stack in or out, spilling when
//! the block breaks and being saved are all the chest's code, unchanged
//! and already tested. What is left over is one float per rack, which is
//! what this module is.
//!
//! Unlike the hearth's, that float **is** saved: a batch in a kiln is
//! fourteen seconds and starting it again is nothing, and twelve minutes
//! of drying thrown away by a server restart is an evening.
//!
//! ## Cost
//!
//! **Nothing per tick per rack.** Racks are stepped on an interval
//! ([`STEP_INTERVAL_SECS`]) rather than every tick, and a step is one
//! climate sample and one addition per *loaded* rack -- a rack in a
//! chunk nobody has is not stepped at all, and picks up where it left
//! off when somebody comes back. That last part is deliberate and it is
//! also honest: drying is something that happens at a camp, and a camp
//! is somewhere a player is.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use serde::{Deserialize, Serialize};

use primitive_shared::rack::{self, CURE_SECONDS};
use primitive_shared::types::BlockId;
use primitive_shared::weather::Weather;

use crate::logic::climate::Ambient;
use crate::logic::containers::Chests;
use crate::logic::fire::Fires;
use crate::logic::world::World;

/// Where a rack is, in global block coordinates.
pub type RackPos = (i32, i32, i32);

pub use primitive_shared::rack::{cures_into, is_rack};

/// How often the racks are stepped.
///
/// Two seconds. Coarse on purpose: the process takes twelve minutes, so
/// this is three hundred and sixty steps over its life, and a finer
/// interval would be spending ticks to make a number that nobody can see
/// move smoother.
pub const STEP_INTERVAL_SECS: f32 = 2.0;

/// Below this, in the degrees `body` measures in, nothing cures at all.
pub const NO_CURE_BELOW_C: f32 = 0.0;
/// ...and at or above this it goes at full speed.
pub const FULL_CURE_AT_C: f32 = 24.0;

/// How much of a rack's progress a fire beside it is worth.
///
/// A smoking rack is the oldest way of curing a skin there is, and it is
/// the reason a tannery and a hearth are the same camp. Applied as a
/// floor on the temperature term rather than as a multiplier, so a fire
/// makes a tundra workable rather than making a summer meadow twice as
/// fast.
pub const FIRE_FLOOR: f32 = 0.75;

/// Its own file beside the chests', on exactly the terms `containers`
/// gives: a world saved before racks existed has no such file, which
/// reads as "no racks", and nothing has to be migrated.
///
/// **v2 is the container.** v1 kept the skin here as well as the
/// progress, because a rack was not a container yet; a v1 file is read
/// and the skins in it handed back to the caller to be put where they
/// live now -- see [`Drying::load`]. Refusing it would be refusing to
/// start; ignoring it would be quietly eating whatever was on the racks.
const SAVE_FORMAT_VERSION: u32 = 2;

#[derive(Serialize, Deserialize)]
struct SaveFile {
    version: u32,
    /// How far along each rack is, 0..1. A rack with nothing on it has
    /// no entry.
    racks: Vec<(RackPos, f32)>,
}

// ---- what a rack file written before racks were containers looks like ----

/// One rack, in the format that kept the skin as well as the progress.
#[derive(Deserialize)]
struct RackV1 {
    raw: BlockId,
    progress: f32,
}

#[derive(Deserialize)]
struct SaveFileV1 {
    /// Read past rather than used: the version has already been decoded
    /// on its own to decide which shape this is, and bincode has to be
    /// told about every field in the order they were written.
    #[allow(dead_code)]
    version: u32,
    racks: Vec<(RackPos, RackV1)>,
}

/// How far along each rack is, as it goes to and comes off the disk.
type SavedProgress = Vec<(RackPos, f32)>;
/// The skins a rack file written before racks were containers had on
/// its frames, and where they were. See [`Drying::load`].
pub type SavedSkins = Vec<(RackPos, BlockId)>;

/// What one step of the racks changed.
///
/// Two lists rather than one, because two different things want to hear
/// about a rack and they want to hear about different racks. A screen
/// standing open at one has to be told **every** time the bar moves or
/// the reason it is not moving changes -- otherwise it shows one
/// photograph for twelve minutes. A plugin hook is about the *event*,
/// and the event is a skin becoming leather.
#[derive(Debug, Default, PartialEq)]
pub struct Stepped {
    /// Every rack that was looked at and had something curing on it:
    /// what a watching screen needs, whether it moved or the rain
    /// stopped it.
    pub advanced: Vec<RackPos>,
    /// ...and the ones that finished a skin. A subset of `advanced`.
    pub finished: Vec<RackPos>,
}

/// How far along every rack that is part way through a skin is.
#[derive(Default)]
pub struct Drying {
    progress: HashMap<RackPos, f32>,
    dirty: bool,
    /// Seconds since the last step. See [`STEP_INTERVAL_SECS`].
    since_step: f32,
    /// How many hides have finished here since the server started.
    /// Not saved and not printed anywhere yet: it is what a test asks
    /// to tell "a skin was banked" from "the numbers moved".
    cured: u64,
}

impl Drying {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn len(&self) -> usize {
        self.progress.len()
    }

    pub fn is_empty(&self) -> bool {
        self.progress.is_empty()
    }

    pub fn cured_total(&self) -> u64 {
        self.cured
    }

    pub fn is_dirty(&self) -> bool {
        self.dirty
    }

    /// How far through the skin on the rack at `at`, 0..1.
    ///
    /// A rack nothing has been laid on has no entry and reads as zero,
    /// which is what an empty frame is: not started.
    pub fn progress_at(&self, at: RackPos) -> f32 {
        self.progress.get(&at).copied().unwrap_or(0.0)
    }

    /// Sets how far along a rack is.
    ///
    /// Two callers and both of them are outside the game: the test world,
    /// which arrives with skins part way through so that the *end* of the
    /// process is a minute away rather than twelve, and the test hook
    /// that finishes one on demand. Nothing a player does comes through
    /// here -- laying a skin on a frame starts it at nothing, which is
    /// what laying a skin on a frame does.
    pub fn set_progress(&mut self, at: RackPos, progress: f32) {
        let progress = if progress.is_finite() {
            progress.clamp(0.0, 1.0)
        } else {
            0.0
        };
        self.progress.insert(at, progress);
        self.dirty = true;
    }

    /// Forgets a rack, because the block holding it is gone.
    ///
    /// What was *on* it is the container store's business -- breaking
    /// the block spills it, the way breaking a chest does. This is the
    /// float.
    pub fn forget(&mut self, at: RackPos) {
        if self.progress.remove(&at).is_some() {
            self.dirty = true;
        }
    }

    /// How fast a rack in these conditions cures, as a multiple of
    /// ideal.
    ///
    /// Pure, and takes the ambient rather than a world, because every
    /// interesting case is a *kind of weather* rather than anything the
    /// server does next -- which is what lets the rules be checked
    /// without a world, a fire map or a chunk.
    pub fn rate(ambient: &Ambient, weather: Weather) -> f32 {
        // Rain stops it dead. Checked first and separately from the
        // temperature, because a warm shower is still a shower.
        if ambient.getting_wet || (weather.is_wet() && !ambient.sheltered) {
            return 0.0;
        }
        let warmth = ((ambient.temperature_c - NO_CURE_BELOW_C)
            / (FULL_CURE_AT_C - NO_CURE_BELOW_C))
            .clamp(0.0, 1.0);
        if ambient.near_fire {
            warmth.max(FIRE_FLOOR)
        } else {
            warmth
        }
    }

    /// One interval's worth of drying, for every rack in a loaded chunk.
    ///
    /// Returns what changed, in two lists -- see [`Stepped`]. This is
    /// the only place that knows a rack moved at all, which is why the
    /// screens are fed from here rather than from the container store's
    /// own idea of "something changed": nothing in the store changes for
    /// twelve minutes.
    ///
    /// A rack that is not in a loaded chunk is skipped entirely and keeps
    /// its progress -- see the note at the top of the module.
    ///
    /// The world is read through `cached_block` and only through it, for
    /// the reason every periodic pass on this server does: a background
    /// step that could trigger terrain generation would be a way to make
    /// the server generate chunks by leaving something somewhere.
    pub fn step(
        &mut self,
        world: &Arc<World>,
        fires: &Fires,
        chests: &mut Chests,
        weather: Weather,
        time_of_day: f32,
        dt: f32,
    ) -> Stepped {
        self.since_step += dt.clamp(0.0, 1.0);
        if self.since_step < STEP_INTERVAL_SECS {
            return Stepped::default();
        }
        let elapsed = self.since_step;
        self.since_step = 0.0;

        let mut stepped = Stepped::default();
        // The container store's own list, filtered to the racks in it.
        // Asking the chests rather than keeping a set of rack positions
        // here is what makes this impossible to get out of step: a skin
        // that reached a frame by *any* route -- a click, a shift-click,
        // a plugin, a world that was loaded off a disk -- is in that
        // store, and there is no second place to remember to update.
        for at in chests.positions() {
            match world.cached_block(at.0, at.1, at.2) {
                None => continue, // nobody has this chunk; leave it be
                Some(block) if is_rack(block) => {}
                Some(_) => {
                    // A chest, or a rack that has been replaced by
                    // something that is not one. Either way there is
                    // nothing here to advance.
                    self.forget(at);
                    continue;
                }
            }
            let contents = chests.contents(at);
            if contents
                .block_in(rack::HIDE_SLOT)
                .and_then(cures_into)
                .is_none()
            {
                // Nothing on the frame. The progress goes back to
                // nothing rather than waiting: a frame that kept half a
                // skin's worth of drying and applied it to whatever was
                // laid on it next would be handing out leather for a
                // wait that never happened.
                self.forget(at);
                continue;
            }
            if rack::curing(&contents).is_none() {
                // A skin on the frame and no room in the tray. **The
                // progress is kept**, which is the whole difference
                // between a rack that waits for you and a rack that
                // punishes you for not being there: coming back to a
                // full tray must cost the trip, not the twelve minutes.
                // Reported anyway, so the screen can say what is wrong.
                stepped.advanced.push(at);
                continue;
            }
            let ambient = Ambient::of(
                world,
                fires,
                (at.0 as f32 + 0.5, at.1 as f32, at.2 as f32 + 0.5),
                time_of_day,
                weather,
            );
            let rate = Self::rate(&ambient, weather);
            // Reported whether it moved or not: "the rain has stopped
            // it" is news to a player standing at the screen, and a rack
            // that only spoke up when it was working would go quiet at
            // exactly the moment there was something to say.
            stepped.advanced.push(at);
            let done = {
                let progress = self.progress.entry(at).or_insert(0.0);
                if rate > 0.0 {
                    // Per thing on the frame: a bundle of grass is a
                    // minute and a skin is twelve (`rack::cure_seconds`).
                    let seconds = contents
                        .block_in(rack::HIDE_SLOT)
                        .map_or(CURE_SECONDS, rack::cure_seconds);
                    *progress = (*progress + rate * elapsed / seconds).min(1.0);
                    self.dirty = true;
                }
                // Checked whatever the weather is: a skin that finished
                // in the sun and then had rain fall on it is finished.
                *progress >= 1.0
            };
            if !done {
                continue;
            }
            // Re-run inside the edit, because `curing` was asked of a
            // copy and a player at the screen may have emptied the frame
            // between then and now.
            if chests.edit(at, rack::complete) {
                self.cured += 1;
                self.progress.insert(at, 0.0);
                self.dirty = true;
                stepped.finished.push(at);
            }
        }
        // Anything whose contents are gone entirely -- taken off by
        // hand, or spilled by the block breaking -- is no longer drying
        // anywhere. Done against the store rather than against the loop
        // above, because the loop only sees loaded chunks and this is
        // true whether anybody is standing there or not.
        let before = self.progress.len();
        self.progress.retain(|at, _| chests.holds_anything(*at));
        if self.progress.len() != before {
            self.dirty = true;
        }
        stepped
    }

    fn save_path(dir: &Path) -> PathBuf {
        dir.join("racks.bin")
    }

    /// Writes them out. Atomic, like the chests: a temp file and a
    /// rename, so a crash mid-write cannot leave a truncated one.
    pub fn save(&mut self, dir: &Path) -> std::io::Result<usize> {
        std::fs::create_dir_all(dir)?;
        let mut racks: Vec<(RackPos, f32)> = self
            .progress
            .iter()
            .map(|(&at, &progress)| (at, progress))
            .collect();
        // Stable bytes for the same world, so a save with nothing
        // changed produces an identical file.
        racks.sort_by_key(|&(at, _)| at);
        let count = racks.len();
        let bytes = bincode::serialize(&SaveFile {
            version: SAVE_FORMAT_VERSION,
            racks,
        })
        .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))?;
        let final_path = Self::save_path(dir);
        let tmp_path = final_path.with_extension("bin.tmp");
        std::fs::write(&tmp_path, &bytes)?;
        std::fs::rename(&tmp_path, &final_path)?;
        self.dirty = false;
        Ok(count)
    }

    /// Reads them back. A missing file is a world from before racks
    /// existed, which reads as "nothing is drying".
    ///
    /// Answers with the skins that have to be **put back into the
    /// container store**: a v1 file kept them here, and a v1 world
    /// opened by this build would otherwise start with every rack
    /// mysteriously bare. Empty for a v2 file, which is every file this
    /// build writes.
    pub fn load(&mut self, dir: &Path) -> std::io::Result<SavedSkins> {
        let bytes = match std::fs::read(Self::save_path(dir)) {
            Ok(bytes) => bytes,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
            Err(e) => return Err(e),
        };
        // The version is the first field of both shapes, so it can be
        // read before deciding which shape this is.
        let version: u32 = bincode::deserialize(&bytes)
            .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))?;
        let (racks, skins): (SavedProgress, SavedSkins) = match version {
            SAVE_FORMAT_VERSION => {
                let save: SaveFile = bincode::deserialize(&bytes)
                    .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))?;
                (save.racks, Vec::new())
            }
            1 => {
                let old: SaveFileV1 = bincode::deserialize(&bytes)
                    .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))?;
                let mut racks = Vec::new();
                let mut skins = Vec::new();
                for (at, rack) in old.racks {
                    if cures_into(rack.raw).is_none() {
                        continue; // a stone on a rack; it was never drying
                    }
                    racks.push((at, rack.progress));
                    skins.push((at, rack.raw));
                }
                (racks, skins)
            }
            other => {
                return Err(std::io::Error::new(
                    std::io::ErrorKind::InvalidData,
                    format!(
                        "racks.bin is version {other}, this build reads {SAVE_FORMAT_VERSION}"
                    ),
                ))
            }
        };
        self.progress.clear();
        for (at, progress) in racks {
            // Repaired rather than trusted: this comes off a file an
            // operator can edit, and a rack at a progress of NaN is a
            // rack that never finishes. The conservative reading costs a
            // player the wait; the generous one hands them free leather
            // for a corrupt file.
            let progress = if progress.is_finite() {
                progress.clamp(0.0, 1.0)
            } else {
                0.0
            };
            self.progress.insert(at, progress);
        }
        // **A migrated file has to be written back.** Nothing else marks
        // this dirty at load, and a rack in an unloaded chunk is never
        // stepped -- so without this the v1 file stays on the disk and
        // the *next* start migrates it again, laying a second skin on
        // every frame over whatever the player had put there. Found by
        // opening a real v1 world and looking at what got saved.
        self.dirty = !skins.is_empty();
        Ok(skins)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use primitive_shared::inventory::Stack;
    use primitive_shared::rack::{HIDE_SLOT, LEATHER_SLOT};
    use primitive_shared::types::{BLOCK_HIDE, BLOCK_LEATHER};

    fn fair() -> Ambient {
        Ambient {
            temperature_c: FULL_CURE_AT_C,
            getting_wet: false,
            near_fire: false,
            sheltered: false,
            in_water: false,
            // The sun and how fast a wet player dries: nothing a rack
            // reads, so whatever a calm, mild day says.
            ..Ambient::default()
        }
    }

    fn a_frame_with(hides: u32) -> Chests {
        let mut chests = Chests::new();
        chests.edit(AT, |contents| {
            contents.put_in_slot(HIDE_SLOT, Stack::new(BLOCK_HIDE, hides));
        });
        chests
    }

    const AT: RackPos = (4, 12, -7);

    #[test]
    fn rain_stops_it_and_does_not_spoil_it() {
        // Both halves. A shower must pause the process rather than
        // destroying several hours of somebody's evening, and it must
        // actually pause it rather than being decoration.
        let mut wet = fair();
        wet.getting_wet = true;
        assert_eq!(Drying::rate(&wet, Weather::Rain), 0.0);
        assert!(Drying::rate(&fair(), Weather::Clear) > 0.0);
        // ...and under a roof the rain is somebody else's.
        let mut inside = fair();
        inside.sheltered = true;
        assert!(Drying::rate(&inside, Weather::Rain) > 0.0);
    }

    #[test]
    fn the_cold_slows_it_and_a_fire_answers_the_cold() {
        let mut tundra = fair();
        tundra.temperature_c = -10.0;
        assert_eq!(Drying::rate(&tundra, Weather::Clear), 0.0);

        let mut smoked = tundra;
        smoked.near_fire = true;
        let rate = Drying::rate(&smoked, Weather::Clear);
        assert!(rate >= FIRE_FLOOR, "a fire in a tundra bought nothing: {rate}");

        // ...and a fire in a warm meadow is not a second speed-up: the
        // floor is a floor rather than a bonus.
        let mut warm_and_smoked = fair();
        warm_and_smoked.near_fire = true;
        assert!(Drying::rate(&warm_and_smoked, Weather::Clear) <= 1.0);
    }

    #[test]
    fn the_rate_is_monotonic_in_warmth() {
        let mut last = -1.0;
        for degrees in [-5.0f32, 0.0, 6.0, 12.0, 18.0, 24.0, 40.0] {
            let mut ambient = fair();
            ambient.temperature_c = degrees;
            let rate = Drying::rate(&ambient, Weather::Clear);
            assert!(rate >= last, "{degrees} degrees cured slower than the step below it");
            assert!((0.0..=1.0).contains(&rate));
            last = rate;
        }
    }

    #[test]
    fn a_finished_frame_swaps_one_skin_for_one_piece_of_leather() {
        // The whole of the mechanic, without a world: the progress is
        // pushed to the end the way the test hook does it, and the next
        // step is what banks it.
        let mut chests = a_frame_with(3);
        let mut drying = Drying::new();
        drying.set_progress(AT, 1.0);

        let contents = chests.contents(AT);
        assert!(rack::curing(&contents).is_some());
        assert!(chests.edit(AT, rack::complete));

        let after = chests.contents(AT);
        assert_eq!(after.count_in(HIDE_SLOT), 2, "it took more than one skin");
        assert_eq!(after.count_in(LEATHER_SLOT), 1);
        assert_eq!(after.count(BLOCK_LEATHER), 1);
    }

    #[test]
    fn a_frame_in_fair_weather_finishes_a_skin_and_starts_the_next() {
        // The loop itself, without a socket: a rack in a loaded chunk,
        // stepped until the skin at the end of its cure is banked. The
        // integration test in `tests/body.rs` covers the wiring; this
        // covers what the wiring is wrapped around.
        let mut chests = a_frame_with(2);
        let mut drying = Drying::new();
        drying.set_progress(AT, 0.999);
        let world = a_world_with_a_rack_at(AT);
        let fires = Fires::new();

        let mut finished = Vec::new();
        for _ in 0..8 {
            let stepped = drying.step(&world, &fires, &mut chests, Weather::Clear, 0.5, 1.0);
            finished.extend(stepped.finished);
        }
        assert_eq!(finished, vec![AT], "the skin at the end of its cure never finished");
        let after = chests.contents(AT);
        assert_eq!(after.count_in(HIDE_SLOT), 1, "it took the wrong number of skins");
        assert_eq!(after.count_in(LEATHER_SLOT), 1, "no leather reached the tray");
        // ...and the next skin starts from nothing rather than from the
        // last one's leftovers.
        assert!(drying.progress_at(AT) < 0.5, "the second skin started part way through");
        assert_eq!(drying.cured_total(), 1);
    }

    #[test]
    fn a_full_tray_holds_the_wait_rather_than_spending_it() {
        // The failure this is here for: a skin that finishes while the
        // tray is full used to have its progress dropped on the next
        // step, so emptying the tray started the twelve minutes again.
        let mut chests = a_frame_with(2);
        chests.edit(AT, |contents| {
            contents.put_in_slot(
                LEATHER_SLOT,
                Stack::new(BLOCK_LEATHER, primitive_shared::inventory::MAX_STACK),
            );
        });
        let mut drying = Drying::new();
        drying.set_progress(AT, 0.95);

        let world = a_world_with_a_rack_at(AT);
        let fires = Fires::new();
        // Twice, because one call is worth at most a second however
        // long the frame was -- see the clamp at the top of `step`, which
        // is what stops a stalled server from curing a hide in one tick.
        let mut stepped = Stepped::default();
        for _ in 0..STEP_INTERVAL_SECS.ceil() as usize + 1 {
            stepped = drying.step(&world, &fires, &mut chests, Weather::Clear, 0.5, 1.0);
            if !stepped.advanced.is_empty() {
                break;
            }
        }
        assert_eq!(stepped.advanced, vec![AT], "nobody at the rack was told it had stopped");
        assert!(stepped.finished.is_empty(), "it made leather with no room for it");
        assert!(
            drying.progress_at(AT) >= 0.95,
            "a full tray threw away the wait: {}",
            drying.progress_at(AT)
        );
    }

    #[test]
    fn a_hide_frame_from_an_old_lone_rack_keeps_drying_its_hide() {
        // The frame is the old lone rack under its own id (`World::load`
        // rewrites one into the other in the same cell), and the drying is
        // keyed by the cell: the skin and its progress are where they were,
        // and the next step carries on from there.
        let mut chests = a_frame_with(1);
        let mut drying = Drying::new();
        drying.set_progress(AT, 0.3);
        let world = a_world_with_a_rack_at(AT);
        world.set_block(
            AT.0,
            AT.1,
            AT.2,
            primitive_shared::types::rack_with_hide(primitive_shared::types::BLOCK_HIDE_FRAME, true),
        );
        let fires = Fires::new();
        for _ in 0..STEP_INTERVAL_SECS.ceil() as usize + 1 {
            drying.step(&world, &fires, &mut chests, Weather::Clear, 0.5, 1.0);
        }
        assert!(drying.progress_at(AT) > 0.3, "the frame stopped drying the hide: {}", drying.progress_at(AT));
        assert_eq!(chests.contents(AT).count_in(HIDE_SLOT), 1, "the hide left the frame");
    }

    #[test]
    fn progress_survives_a_save_and_a_reload() {
        let dir = temp_dir("progress");
        let mut drying = Drying::new();
        drying.set_progress(AT, 0.4);
        drying.save(&dir).expect("save");

        let mut read = Drying::new();
        assert!(read.load(&dir).expect("load").is_empty(), "a v2 file had skins in it");
        assert!((read.progress_at(AT) - 0.4).abs() < 1e-6);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_file_written_by_a_meddler_is_repaired_rather_than_believed() {
        let dir = temp_dir("meddler");
        let bytes = bincode::serialize(&SaveFile {
            version: SAVE_FORMAT_VERSION,
            racks: vec![(AT, f32::NAN), ((1, 1, 1), 40.0)],
        })
        .expect("serialise");
        std::fs::write(Drying::save_path(&dir), bytes).expect("write");

        let mut read = Drying::new();
        read.load(&dir).expect("load");
        assert_eq!(read.progress_at(AT), 0.0, "a progress of NaN was believed");
        assert_eq!(read.progress_at((1, 1, 1)), 1.0, "a progress of 40 was believed");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_rack_file_from_before_the_screen_hands_its_skins_back() {
        // **The one thing a format change here must not do is eat a
        // hide.** A v1 file kept the skin beside the progress; this
        // build keeps it in the container store, and the migration is
        // the caller's to finish -- so `load` has to say what was on the
        // frames.
        let dir = temp_dir("v1");
        #[derive(Serialize)]
        struct OldRack {
            raw: BlockId,
            progress: f32,
        }
        #[derive(Serialize)]
        struct OldFile {
            version: u32,
            racks: Vec<(RackPos, OldRack)>,
        }
        let bytes = bincode::serialize(&OldFile {
            version: 1,
            racks: vec![
                (AT, OldRack { raw: BLOCK_HIDE, progress: 0.75 }),
                // ...and a rack with a stone on it, which was never
                // drying and must not come back as one.
                ((0, 0, 0), OldRack { raw: primitive_shared::types::BLOCK_STONE, progress: 0.5 }),
            ],
        })
        .expect("serialise");
        std::fs::write(Drying::save_path(&dir), bytes).expect("write");

        let mut read = Drying::new();
        let skins = read.load(&dir).expect("load");
        assert_eq!(skins, vec![(AT, BLOCK_HIDE)], "the skin on the frame was lost");
        assert!((read.progress_at(AT) - 0.75).abs() < 1e-6);
        assert_eq!(read.progress_at((0, 0, 0)), 0.0, "a stone was left drying");
        // ...and it must be written back, or the *next* start reads the
        // v1 file again and lays a second skin on every frame. A rack in
        // an unloaded chunk is never stepped, so nothing else would ever
        // mark this dirty.
        assert!(read.is_dirty(), "a migrated rack file would never be rewritten");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_world_with_no_rack_file_has_no_racks() {
        let dir = temp_dir("missing");
        let _ = std::fs::remove_dir_all(&dir);
        let mut drying = Drying::new();
        assert_eq!(drying.load(&dir).ok(), Some(Vec::new()));
        assert!(drying.is_empty());
    }

    /// A loaded chunk with a rack standing in it, which is the only
    /// thing `step` asks the world for.
    fn a_world_with_a_rack_at(at: RackPos) -> Arc<World> {
        let world = Arc::new(World::with_preset(
            7,
            primitive_shared::worldgen::Preset::Test,
            64,
        ));
        let pos = primitive_shared::types::ChunkPos::from_global(at.0, at.2).0;
        let chunk = world.generate(pos);
        world.insert(chunk);
        world.set_block(at.0, at.1, at.2, primitive_shared::types::BLOCK_DRYING_RACK);
        world
    }

    fn temp_dir(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "primitive_racks_{name}_{}_{:?}",
            std::process::id(),
            std::thread::current().id()
        ));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).expect("temp dir");
        dir
    }
}
