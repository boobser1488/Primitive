//! Wet walls drying where they stand: daub on a wattle panel, the top lift
//! of a cob wall (`build`).
//!
//! **The peat's weather, word for word** (`peat::rate`): sun, warmth and
//! wind dry a wall; a roof slows it to what the air alone does; frost stops
//! it; rain takes it back. What is the wall's own is what rain *does* to it
//! at the end: a sod goes back to wet, and wet mud on a wall **washes off
//! it** -- the daub off the rods, the lift off the cob under it
//! (`build::washed`). Rain on a wall that has not dried is the cost of
//! building in the wrong week, which is the decision a mud house has always
//! been.
//!
//! **Where the state lives: the wetness in the block, the progress here.**
//! A wall is wet or dry by its id, so the picture, the save and every other
//! client agree about it with nothing new on the wire; how far along the
//! drying is lives in this list, one float a cell, stepped on the peat's
//! slow clock, only in loaded chunks, and saved in its own file beside the
//! peat's. A cell that is no longer a wet wall -- dried, washed, broken, built
//! over -- is dropped the next time it is looked at.
//!
//! Rejected: **a roll per step and no list**, a wall that dries with some
//! chance each interval. It needs nothing saved, and it is a wall that might
//! be dry in a minute or wet at the end of a week in the same weather: a
//! player who cannot tell when their wall will be ready cannot plan the next
//! lift, and planning the next lift is the whole mechanic.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use serde::{Deserialize, Serialize};

use primitive_shared::build;
use primitive_shared::types::BlockId;
use primitive_shared::weather::Weather;

use crate::logic::climate::Ambient;
use crate::logic::fire::Fires;
use crate::logic::world::World;

/// A cell, in global block coordinates.
pub type WallPos = (i32, i32, i32);

/// **How far the rain may take a wet wall back before it washes off**: a
/// fifth of its drying. At a shower's rate that is a few minutes -- a squall
/// that passes costs nothing, a wet afternoon costs the daub.
pub const WASHES_AT: f32 = -0.2;

/// Its own file, on the peat's terms.
const SAVE_FORMAT_VERSION: u32 = 1;

#[derive(Serialize, Deserialize)]
struct SaveFile {
    version: u32,
    walls: Vec<(WallPos, f32)>,
}

#[derive(Default)]
pub struct Walls {
    progress: HashMap<WallPos, f32>,
    dirty: bool,
    since_step: f32,
}

impl Walls {
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

    pub fn progress_at(&self, at: WallPos) -> Option<f32> {
        self.progress.get(&at).copied()
    }

    /// A wet stage has just been laid at `at`: its drying starts from
    /// nothing, whatever the cell was doing before -- a new lift is new mud.
    pub fn lay(&mut self, at: WallPos) {
        self.progress.insert(at, 0.0);
        self.dirty = true;
    }

    /// Sets a wall's progress outright: the tests, and nothing a player does.
    pub fn set_progress(&mut self, at: WallPos, progress: f32) {
        self.progress.insert(at, if progress.is_finite() { progress.clamp(WASHES_AT, 1.0) } else { 0.0 });
        self.dirty = true;
    }

    /// One interval of weather on every wet wall in a loaded chunk. Answers
    /// the cells whose block has to change -- dried, or washed -- and what
    /// each becomes; the caller writes them and tells everybody, as it does
    /// any edit.
    pub fn step(
        &mut self,
        world: &Arc<World>,
        fires: &Fires,
        weather: Weather,
        world_days: f32,
        dt: f32,
    ) -> Vec<(WallPos, BlockId)> {
        self.since_step += dt.clamp(0.0, 1.0);
        if self.since_step < crate::logic::peat::STEP_INTERVAL_SECS {
            return Vec::new();
        }
        let elapsed = self.since_step;
        self.since_step = 0.0;
        let mut changes = Vec::new();
        let cells: Vec<WallPos> = self.progress.keys().copied().collect();
        for at in cells {
            let block = match world.cached_block(at.0, at.1, at.2) {
                None => continue, // nobody has this chunk; it waits
                Some(block) if build::is_wet(block) => block,
                Some(_) => {
                    self.progress.remove(&at);
                    self.dirty = true;
                    continue;
                }
            };
            let ambient =
                Ambient::of(world, fires, (at.0 as f32 + 0.5, at.1 as f32, at.2 as f32 + 0.5), world_days, weather);
            let rate = crate::logic::peat::rate(&ambient, weather, world_days);
            if rate == 0.0 {
                continue;
            }
            let progress = {
                let progress = self.progress.entry(at).or_insert(0.0);
                *progress = (*progress + rate * elapsed / build::dry_seconds(block)).clamp(WASHES_AT, 1.0);
                *progress
            };
            self.dirty = true;
            let now = if progress >= 1.0 {
                build::dried(block)
            } else if progress <= WASHES_AT {
                build::washed(block)
            } else {
                continue;
            };
            self.progress.remove(&at);
            changes.push((at, now));
        }
        changes
    }

    fn save_path(dir: &Path) -> PathBuf {
        dir.join("walls.bin")
    }

    /// Writes them out, atomically, the peat's way.
    pub fn save(&mut self, dir: &Path) -> std::io::Result<usize> {
        std::fs::create_dir_all(dir)?;
        let mut walls: Vec<(WallPos, f32)> = self.progress.iter().map(|(&at, &p)| (at, p)).collect();
        walls.sort_by_key(|&(at, _)| at);
        let count = walls.len();
        let bytes = bincode::serialize(&SaveFile { version: SAVE_FORMAT_VERSION, walls })
            .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))?;
        let final_path = Self::save_path(dir);
        let tmp_path = final_path.with_extension("bin.tmp");
        std::fs::write(&tmp_path, &bytes)?;
        std::fs::rename(&tmp_path, &final_path)?;
        self.dirty = false;
        Ok(count)
    }

    /// Reads them back. A missing file is a world from before walls were
    /// laid wet.
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
                format!("walls.bin is version {}, this build reads {SAVE_FORMAT_VERSION}", save.version),
            ));
        }
        self.progress.clear();
        for (at, progress) in save.walls {
            self.set_progress(at, progress);
        }
        self.dirty = false;
        Ok(self.progress.len())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use primitive_shared::types::{BLOCK_AIR, BLOCK_COB, BLOCK_GRASS, BLOCK_STONE};

    const AT: WallPos = (5, primitive_shared::showcase::GROUND_Y + 1, -3);
    /// Noon on the world's first day: the sun at its height.
    const NOON: f32 = 0.5;

    /// A wet lift of cob under an open sky, the peat's test world.
    fn a_wet_lift_at(at: WallPos) -> (Arc<World>, BlockId) {
        let world = Arc::new(World::with_preset(7, primitive_shared::worldgen::Preset::Test, 64));
        let pos = primitive_shared::types::ChunkPos::from_global(at.0, at.2).0;
        world.insert(world.generate(pos));
        for y in at.1 + 1..primitive_shared::types::CHUNK_SIZE_Y as i32 {
            world.set_block(at.0, y, at.2, BLOCK_AIR);
        }
        world.set_block(at.0, at.1 - 1, at.2, BLOCK_GRASS);
        let lift = build::lay(BLOCK_AIR, BLOCK_COB, false, BLOCK_STONE, false).unwrap().result;
        world.set_block(at.0, at.1, at.2, lift);
        (world, lift)
    }

    fn step_a_while(walls: &mut Walls, world: &Arc<World>, weather: Weather) -> Vec<(WallPos, BlockId)> {
        let fires = Fires::new();
        let mut changed = Vec::new();
        for _ in 0..(crate::logic::peat::STEP_INTERVAL_SECS as usize * 2) {
            changed.extend(walls.step(world, &fires, weather, NOON, 1.0));
        }
        changed
    }

    #[test]
    fn a_wet_lift_of_cob_in_fair_weather_dries_into_a_wall_the_next_can_go_on() {
        let (world, lift) = a_wet_lift_at(AT);
        let mut walls = Walls::new();
        walls.lay(AT);
        walls.set_progress(AT, 0.999);
        let changed = step_a_while(&mut walls, &world, Weather::Clear);
        assert_eq!(changed, vec![(AT, build::dried(lift))]);
        assert!(walls.is_empty(), "a dry wall is still on the list");
        assert!(build::lay(build::dried(lift), BLOCK_COB, false, BLOCK_STONE, false).is_ok());
    }

    #[test]
    fn rain_on_a_wet_lift_washes_it_off_and_a_squall_does_not() {
        let (world, lift) = a_wet_lift_at(AT);
        let mut walls = Walls::new();
        walls.lay(AT);
        assert!(step_a_while(&mut walls, &world, Weather::Storm).is_empty(), "a moment's rain took the lift");
        assert!(walls.progress_at(AT).is_some_and(|p| p < 0.0), "the storm did not reach the wall");
        walls.set_progress(AT, WASHES_AT + 0.0001);
        let changed = step_a_while(&mut walls, &world, Weather::Storm);
        assert_eq!(changed, vec![(AT, build::washed(lift))]);
    }

    #[test]
    fn the_drying_survives_a_save_round_trip() {
        let dir = std::env::temp_dir().join(format!("primitive-walls-{}", std::process::id()));
        let mut walls = Walls::new();
        walls.set_progress(AT, 0.4);
        walls.save(&dir).unwrap();
        let mut back = Walls::new();
        assert_eq!(back.load(&dir).unwrap(), 1);
        assert_eq!(back.progress_at(AT), Some(0.4));
        let _ = std::fs::remove_dir_all(&dir);
    }
}
