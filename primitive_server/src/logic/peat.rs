//! Peat drying on the ground, where it was set down.
//!
//! "сделай возможность сушить торф на земле просто под солнцем и в
//! зависимости от погоды" -- and then, to be exact about it, "я про то, если
//! положить торф через шифт". A sod cut out of a bog and **set down** with
//! the modifier (`types::can_be_set_down`, `set_down_item`) lies on the grass
//! and dries; a sod put down plainly is a block of peat and stays one. That
//! is how peat has always been made into fuel: cut, laid out in rows on the
//! bank, turned, and carried home dry. The rack used to be the only way, and
//! the rack is the larder now (`rack::Trade`).
//!
//! ## What the weather does to it
//!
//! [`rate`] is the whole of the rules, pure, so every case below is a test
//! and not a hope:
//!
//! - **Sun, warmth and wind dry it.** Warmth is the rack's term -- nothing
//!   below freezing, full at 24 degrees -- and it multiplies a sum of what
//!   the open sky gives: a little for being out at all, most of it for the
//!   sun's height, the rest for the wind (`raft::wind`, the one wind both
//!   sides already agree on). A calm grey noon dries; a bright windy one
//!   dries fastest; a still night dries a little.
//! - **Shade and a roof slow it.** Under a roof there is no sun and no wind,
//!   only the air: it still dries, at [`OPEN_AIR`] of the rate -- a shed is a
//!   place peat keeps dry, not a place it dries.
//! - **Snow stops it** and so does frost: a sod under a fall of snow is
//!   neither drying nor soaking.
//! - **Rain undoes it.** Not a pause, as the rack's is: a cut sod is a
//!   sponge, and one caught out in a downpour is wetter at the end of it.
//!   It gives back [`SOAK`] of the fair-weather rate per unit of the rain's
//!   intensity, so a shower costs a little and a storm costs the day. A sod
//!   that goes back past the middle is a wet sod again, and looks it.
//!
//! **Why rain undoes peat and only pauses a skin.** The rack's note is right
//! for the rack: a mechanic that silently destroyed hours of a player's
//! time for going inside during a storm would be one nobody uses twice. A
//! skin is on a frame the player built, often under a roof, and finishing it
//! is the whole reward. Peat is the opposite trade -- free to lay out, a
//! field of it at once, and the decision it exists to create is *when* to
//! cut and whether to carry it in when the sky turns. If rain only paused
//! it, the answer would always be "leave it", and a mechanic with one
//! correct answer is not one yet. What it does **not** do is undo a brick:
//! a sod that finished is fuel and stays fuel (it is taken off this list the
//! moment it turns), the rack's "finished is finished" rule, so a player is
//! never punished for coming back late -- only for leaving it half done in
//! bad weather.
//!
//! ## Where the state lives
//!
//! **The stage is the thing in the store.** Wet is `BLOCK_PEAT`, half way is
//! `BLOCK_DRYING_PEAT`, done is `BLOCK_DRIED_PEAT`, and the set-down cell's
//! ordinary message (`ServerMessage::SetDownItem`) draws whichever it is --
//! so a field of peat reads at a glance with nothing new on the wire, and a
//! sod picked up half dried carries its half with it. What is here is the
//! progress inside a stage: one float per cell, on the racks' terms --
//! stepped on a slow clock ([`STEP_INTERVAL_SECS`]), only in loaded chunks,
//! read through `cached_block` so a sod can never make the server generate
//! terrain, and saved in its own file beside the racks'.
//!
//! Rejected: stepping every set-down store and asking each whether it holds
//! peat. It is the container store's whole list every two seconds for a few
//! sods, and a list of the cells that are drying is what the pit kilns and
//! the racks keep.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use serde::{Deserialize, Serialize};

use primitive_shared::types::{block_kind, is_set_down, BlockId, BLOCK_DRIED_PEAT, BLOCK_DRYING_PEAT, BLOCK_PEAT};
use primitive_shared::weather::Weather;

use crate::logic::climate::Ambient;
use crate::logic::containers::Chests;
use crate::logic::fire::Fires;
use crate::logic::world::World;

/// Where a sod lies, in global block coordinates.
pub type SodPos = (i32, i32, i32);

/// How often the sods are stepped. The racks' interval, for the racks'
/// reason: a quarter of an hour of drying does not need a finer clock.
pub const STEP_INTERVAL_SECS: f32 = 2.0;

/// How long a sod takes from cut to brick in the best weather there is --
/// warm, bright and windy -- in seconds.
///
/// Fifteen minutes, a little longer than a skin on a frame, because the best
/// weather is rare: a fair day's average is about half of it, so a row laid
/// out in the morning is fuel by the next, and a wet week is a week without.
pub const DRY_SECONDS: f32 = 900.0;

/// Where the wet sod turns into the half-dried one, and back.
pub const HALF: f32 = 0.5;

/// How much of the best rate the open air gives with no sun and no wind:
/// what a sod under a roof dries at, and what a still night adds.
pub const OPEN_AIR: f32 = 0.3;
/// ...how much more the sun gives at its height, in the open.
pub const SUN: f32 = 0.5;
/// ...and how much more a full wind gives, in the open.
pub const WIND: f32 = 0.2;

/// How fast rain gives the drying back, as a share of the best rate, per
/// unit of the rain's intensity (`Weather::intensity`): a shower a third,
/// a storm six tenths.
pub const SOAK: f32 = 0.6;

/// Below this nothing dries, and at or above [`FULL_AT_C`] warmth is no
/// longer the limit. The rack's two numbers (`drying`), for the rack's
/// reason: there is one answer to "what is the weather like here".
pub const NO_DRYING_BELOW_C: f32 = 0.0;
pub const FULL_AT_C: f32 = 24.0;

/// What a sod at this progress is: wet, half dried, or a brick.
pub fn stage(progress: f32) -> BlockId {
    if progress >= 1.0 {
        BLOCK_DRIED_PEAT
    } else if progress >= HALF {
        BLOCK_DRYING_PEAT
    } else {
        BLOCK_PEAT
    }
}

/// Is this a sod that is still drying: wet or half way?
#[inline]
pub fn is_drying_sod(item: BlockId) -> bool {
    matches!(block_kind(item), BLOCK_PEAT | BLOCK_DRYING_PEAT)
}

/// How fast a sod here dries, as a share of the best rate: negative when
/// the rain is taking it back.
///
/// Pure: the ambient, the weather, and the hour and the wind out of the
/// world's clock (`world_days`, the day in the whole number and the hour in
/// the fraction, `Ambient::of`'s convention).
pub fn rate(ambient: &Ambient, weather: Weather, world_days: f32) -> f32 {
    let rained_on = ambient.getting_wet || (weather.is_wet() && !ambient.sheltered);
    if rained_on {
        // Snow lies on it and does neither: it neither dries under a drift
        // nor soaks until the drift melts, and by then the sky is clear.
        // Freezing air is the test, in the degrees the ambient is in --
        // `weather::falls_as_snow` asks the generator's 0..1 scale, which a
        // sample of the air does not carry.
        if ambient.temperature_c <= NO_DRYING_BELOW_C {
            return 0.0;
        }
        return -SOAK * weather.intensity().max(0.5);
    }
    let warmth = ((ambient.temperature_c - NO_DRYING_BELOW_C) / (FULL_AT_C - NO_DRYING_BELOW_C)).clamp(0.0, 1.0);
    if ambient.sheltered {
        return warmth * OPEN_AIR;
    }
    let hour = world_days.rem_euclid(1.0);
    let sun = if weather.is_wet() {
        0.0
    } else {
        (-(std::f32::consts::TAU * hour).cos()).max(0.0)
    };
    let wind = primitive_shared::raft::wind(world_days, weather).strength.clamp(0.0, 1.0);
    warmth * (OPEN_AIR + SUN * sun + WIND * wind)
}

/// Its own file beside the racks', on the racks' terms: a world saved before
/// peat dried on the ground has none, which reads as "nothing is drying".
const SAVE_FORMAT_VERSION: u32 = 1;

#[derive(Serialize, Deserialize)]
struct SaveFile {
    version: u32,
    sods: Vec<(SodPos, f32)>,
}

/// How far along every sod lying out to dry is: the whole of the drying, in
/// progress 0..1 (`stage` turns it into what the sod is).
#[derive(Default)]
pub struct Peat {
    progress: HashMap<SodPos, f32>,
    dirty: bool,
    since_step: f32,
}

impl Peat {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn len(&self) -> usize {
        self.progress.len()
    }

    pub fn is_empty(&self) -> bool {
        self.progress.is_empty()
    }

    pub fn is_dirty(&self) -> bool {
        self.dirty
    }

    /// How far along the sod at `at` is, 0..1, and `None` for a cell that
    /// is not on the list.
    pub fn progress_at(&self, at: SodPos) -> Option<f32> {
        self.progress.get(&at).copied()
    }

    /// A sod has just been set down at `at`: it starts at the beginning of
    /// the stage it is in -- a wet sod at nothing, a half-dried one at the
    /// middle. Anything else set down is not drying, and a cell that held a
    /// sod before is forgotten.
    ///
    /// **The start of its stage, not where it was when it was picked up.**
    /// The stage travels in the item; the progress inside it does not, and
    /// keeping a float per item would be a float in every stack. What a
    /// player loses by picking a sod up is at most half of one stage.
    pub fn lay(&mut self, at: SodPos, item: BlockId) {
        let start = match block_kind(item) {
            BLOCK_PEAT => 0.0,
            BLOCK_DRYING_PEAT => HALF,
            _ => {
                self.forget(at);
                return;
            }
        };
        self.progress.insert(at, start);
        self.dirty = true;
    }

    /// Sets a sod's progress outright: the tests, and nothing a player does.
    pub fn set_progress(&mut self, at: SodPos, progress: f32) {
        let progress = if progress.is_finite() { progress.clamp(0.0, 1.0) } else { 0.0 };
        self.progress.insert(at, progress);
        self.dirty = true;
    }

    pub fn forget(&mut self, at: SodPos) {
        if self.progress.remove(&at).is_some() {
            self.dirty = true;
        }
    }

    /// One interval's worth of weather on every sod in a loaded chunk.
    ///
    /// Answers the cells whose sod **changed what it is** -- wet to half
    /// dried, back again in the rain, or into a brick -- which are the ones
    /// everybody near has to be told about (`tell_set_down`). The store is
    /// rewritten here; the bar in between moves nobody's picture.
    ///
    /// A cell that is no longer a set-down sod -- taken by a hand, knocked
    /// loose, rotted, replaced -- is dropped from the list when it is next
    /// looked at, so nothing else has to remember to tell this.
    pub fn step(
        &mut self,
        world: &Arc<World>,
        fires: &Fires,
        chests: &mut Chests,
        weather: Weather,
        world_days: f32,
        dt: f32,
    ) -> Vec<SodPos> {
        self.since_step += dt.clamp(0.0, 1.0);
        if self.since_step < STEP_INTERVAL_SECS {
            return Vec::new();
        }
        let elapsed = self.since_step;
        self.since_step = 0.0;

        let mut changed = Vec::new();
        let cells: Vec<SodPos> = self.progress.keys().copied().collect();
        for at in cells {
            match world.cached_block(at.0, at.1, at.2) {
                None => continue, // nobody has this chunk; it waits
                Some(block) if is_set_down(block) => {}
                Some(_) => {
                    self.forget(at);
                    continue;
                }
            }
            let Some(sod) = chests.contents(at).block_in(0).filter(|&item| is_drying_sod(item)) else {
                self.forget(at);
                continue;
            };
            let ambient = Ambient::of(
                world,
                fires,
                (at.0 as f32 + 0.5, at.1 as f32, at.2 as f32 + 0.5),
                world_days,
                weather,
            );
            let rate = rate(&ambient, weather, world_days);
            if rate == 0.0 {
                continue;
            }
            let progress = {
                let progress = self.progress.entry(at).or_insert(0.0);
                *progress = (*progress + rate * elapsed / DRY_SECONDS).clamp(0.0, 1.0);
                *progress
            };
            self.dirty = true;
            let now = stage(progress);
            if block_kind(now) == block_kind(sod) {
                continue;
            }
            // The one sod in the store becomes the next stage of itself.
            // Done inside the edit, so a hand that took it between the
            // look above and now leaves nothing to rewrite.
            let rewritten = chests.edit(at, |store| {
                if store.block_in(0).is_some_and(is_drying_sod) {
                    store.take_slot(0);
                    store.put_in_slot(0, primitive_shared::inventory::Stack::new(now, 1));
                    true
                } else {
                    false
                }
            });
            if rewritten {
                changed.push(at);
            }
            if now == BLOCK_DRIED_PEAT {
                // Fuel now, and fuel for good: see the module note on why
                // rain does not undo a brick.
                self.forget(at);
            }
        }
        changed
    }

    fn save_path(dir: &Path) -> PathBuf {
        dir.join("peat.bin")
    }

    /// Writes them out, atomically, the racks' way.
    pub fn save(&mut self, dir: &Path) -> std::io::Result<usize> {
        std::fs::create_dir_all(dir)?;
        let mut sods: Vec<(SodPos, f32)> = self.progress.iter().map(|(&at, &p)| (at, p)).collect();
        sods.sort_by_key(|&(at, _)| at);
        let count = sods.len();
        let bytes = bincode::serialize(&SaveFile { version: SAVE_FORMAT_VERSION, sods })
            .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))?;
        let final_path = Self::save_path(dir);
        let tmp_path = final_path.with_extension("bin.tmp");
        std::fs::write(&tmp_path, &bytes)?;
        std::fs::rename(&tmp_path, &final_path)?;
        self.dirty = false;
        Ok(count)
    }

    /// Reads them back. A missing file is a world from before peat dried on
    /// the ground. A progress off an edited file is repaired rather than
    /// believed, for the racks' reason.
    pub fn load(&mut self, dir: &Path) -> std::io::Result<usize> {
        let bytes = match std::fs::read(Self::save_path(dir)) {
            Ok(bytes) => bytes,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(0),
            Err(e) => return Err(e),
        };
        let save: SaveFile =
            bincode::deserialize(&bytes).map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))?;
        if save.version != SAVE_FORMAT_VERSION {
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                format!("peat.bin is version {}, this build reads {SAVE_FORMAT_VERSION}", save.version),
            ));
        }
        self.progress.clear();
        for (at, progress) in save.sods {
            let progress = if progress.is_finite() { progress.clamp(0.0, 1.0) } else { 0.0 };
            self.progress.insert(at, progress);
        }
        self.dirty = false;
        Ok(self.progress.len())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use primitive_shared::inventory::Stack;
    use primitive_shared::types::{faced, Facing, BLOCK_SET_DOWN};

    const AT: SodPos = (5, primitive_shared::showcase::GROUND_Y + 1, -3);
    /// Noon on the world's first day: the sun at its height.
    const NOON: f32 = 0.5;

    fn fair() -> Ambient {
        Ambient { temperature_c: FULL_AT_C, ..Ambient::default() }
    }

    #[test]
    fn peat_in_the_sun_dries_and_in_the_rain_gets_wetter() {
        let sunny = rate(&fair(), Weather::Clear, NOON);
        assert!(sunny > OPEN_AIR, "a sod in the noon sun dried no faster than one in a shed: {sunny}");
        let mut rained_on = fair();
        rained_on.getting_wet = true;
        let rain = rate(&rained_on, Weather::Rain, NOON);
        let storm = rate(&rained_on, Weather::Storm, NOON);
        assert!(rain < 0.0, "rain did not take the drying back: {rain}");
        assert!(storm < rain, "a storm soaked no more than a shower: {storm} against {rain}");
    }

    #[test]
    fn peat_under_a_roof_dries_slower_than_in_the_open_and_the_rain_does_not_reach_it() {
        let mut roofed = fair();
        roofed.sheltered = true;
        let open = rate(&fair(), Weather::Clear, NOON);
        let inside = rate(&roofed, Weather::Clear, NOON);
        assert!(inside > 0.0, "a sod under a roof did not dry at all");
        assert!(inside < open, "a roof dried it as fast as the sun: {inside} against {open}");
        assert!(rate(&roofed, Weather::Storm, NOON) > 0.0, "the storm got under the roof");
    }

    #[test]
    fn snow_and_frost_stop_it_without_soaking_it() {
        let mut snowed_on = fair();
        snowed_on.temperature_c = -6.0;
        snowed_on.getting_wet = true;
        assert_eq!(rate(&snowed_on, Weather::Rain, NOON), 0.0, "a sod under snow moved");
        let mut frost = fair();
        frost.temperature_c = -6.0;
        assert_eq!(rate(&frost, Weather::Clear, NOON), 0.0, "a frozen sod dried");
    }

    #[test]
    fn the_sun_and_the_wind_are_what_the_open_adds() {
        // Midnight has no sun, so it dries slower than noon on the same day.
        let night = rate(&fair(), Weather::Clear, 0.0);
        let noon = rate(&fair(), Weather::Clear, NOON);
        assert!(night > 0.0 && night < noon, "night {night}, noon {noon}");
        assert!(noon <= OPEN_AIR + SUN + WIND + 1e-6);
    }

    #[test]
    fn a_sod_is_wet_then_half_dried_then_a_brick_and_back_to_wet_past_the_middle() {
        assert_eq!(stage(0.0), BLOCK_PEAT);
        assert_eq!(stage(HALF - 0.01), BLOCK_PEAT);
        assert_eq!(stage(HALF), BLOCK_DRYING_PEAT);
        assert_eq!(stage(1.0), BLOCK_DRIED_PEAT);
    }

    #[test]
    fn a_sod_set_down_in_fair_weather_turns_half_dried_and_then_into_fuel() {
        let (world, mut chests) = a_sod_lying_at(AT);
        let fires = Fires::new();
        let mut peat = Peat::new();
        peat.lay(AT, BLOCK_PEAT);
        peat.set_progress(AT, HALF - 0.001);
        let changed = step_a_while(&mut peat, &world, &fires, &mut chests, Weather::Clear);
        assert_eq!(changed, vec![AT], "nobody was told the sod had turned");
        assert_eq!(chests.contents(AT).block_in(0), Some(BLOCK_DRYING_PEAT));

        peat.set_progress(AT, 0.999);
        step_a_while(&mut peat, &world, &fires, &mut chests, Weather::Clear);
        assert_eq!(chests.contents(AT).block_in(0), Some(BLOCK_DRIED_PEAT), "the sod never became fuel");
        assert_eq!(peat.progress_at(AT), None, "a brick was left on the drying list");
    }

    #[test]
    fn a_half_dried_sod_in_the_rain_goes_back_to_a_wet_one() {
        let (world, mut chests) = a_sod_lying_at(AT);
        chests.edit(AT, |store| {
            store.take_slot(0);
            store.put_in_slot(0, Stack::new(BLOCK_DRYING_PEAT, 1));
        });
        let fires = Fires::new();
        let mut peat = Peat::new();
        peat.lay(AT, BLOCK_DRYING_PEAT);
        peat.set_progress(AT, HALF + 0.001);
        // The test world's sky is open over the sod, so a storm reaches it.
        step_a_while(&mut peat, &world, &fires, &mut chests, Weather::Storm);
        let now = chests.contents(AT).block_in(0);
        // A sod the generator put under something would not be rained on;
        // the cell is open sky by construction (see `a_sod_lying_at`).
        assert_eq!(now, Some(BLOCK_PEAT), "the storm left a half-dried sod half dried");
        assert!(peat.progress_at(AT).is_some_and(|p| p < HALF));
    }

    #[test]
    fn the_drying_survives_a_save_round_trip() {
        let dir = std::env::temp_dir().join(format!("primitive_peat_{}_{:?}", std::process::id(), std::thread::current().id()));
        let _ = std::fs::remove_dir_all(&dir);
        let mut peat = Peat::new();
        peat.set_progress(AT, 0.37);
        peat.set_progress((1, 2, 3), f32::NAN);
        assert_eq!(peat.save(&dir).expect("save"), 2);

        let mut read = Peat::new();
        assert_eq!(read.load(&dir).expect("load"), 2);
        assert!((read.progress_at(AT).unwrap() - 0.37).abs() < 1e-6, "the drying was lost in the save");
        assert_eq!(read.progress_at((1, 2, 3)), Some(0.0));
        let _ = std::fs::remove_dir_all(&dir);

        let mut none = Peat::new();
        assert_eq!(none.load(&dir).expect("a world with no peat file"), 0);
    }

    /// A loaded chunk with a sod set down at `at`, open to the sky: every
    /// cell above it cleared, so the weather reaches it whatever the
    /// generator put there.
    fn a_sod_lying_at(at: SodPos) -> (Arc<World>, Chests) {
        let world = Arc::new(World::with_preset(7, primitive_shared::worldgen::Preset::Test, 64));
        let pos = primitive_shared::types::ChunkPos::from_global(at.0, at.2).0;
        world.insert(world.generate(pos));
        for y in at.1 + 1..primitive_shared::types::CHUNK_SIZE_Y as i32 {
            world.set_block(at.0, y, at.2, primitive_shared::types::BLOCK_AIR);
        }
        world.set_block(at.0, at.1 - 1, at.2, primitive_shared::types::BLOCK_GRASS);
        world.set_block(at.0, at.1, at.2, faced(BLOCK_SET_DOWN, Facing::North));
        let mut chests = Chests::new();
        chests.edit(at, |store| store.put_in_slot(0, Stack::new(BLOCK_PEAT, 1)));
        (world, chests)
    }

    fn step_a_while(peat: &mut Peat, world: &Arc<World>, fires: &Fires, chests: &mut Chests, weather: Weather) -> Vec<SodPos> {
        let mut changed = Vec::new();
        for _ in 0..(STEP_INTERVAL_SECS as usize * 2) {
            changed.extend(peat.step(world, fires, chests, weather, NOON, 1.0));
        }
        changed
    }
}
