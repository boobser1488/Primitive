//! Fire that gets loose, smoke in a closed room, soot on the ceiling over a
//! hearth, and the clock on a standing torch: the server's half of
//! `primitive_shared::wildfire`, which holds the rules and the reasons for
//! every number. What is here is the clocks and the bounds.
//!
//! ## Why it is its own mechanic and not part of `fire`
//!
//! `fire::Fires` is a map of *hearths*: fuel slots, loads, a temperature,
//! a save file that a kiln full of charcoal lives in. A plank wall that has
//! caught has none of that -- no slot, no heat a batch asks for, no fuel a
//! player put there -- and folding it in would make every question the fire
//! map answers ("is there a fire within reach to cook at") answer yes beside
//! a burning house. So the hearths stay the fire map's, and this reads them
//! as the flames they are.
//!
//! ## What is saved
//!
//! **What is alight and how long it has left, and the standing torches'
//! wads** (`wildfire.bin`), for the fire map's reason: a player who logs out
//! beside a burning wall should not log back in beside a wall that burns
//! for ever because nobody timed it. A burning block this server has no
//! clock for is adopted with a whole burn (`on_block_changed`), which is
//! what an older save or a mod's block gets.
//!
//! **Not the warmth** soaked into a wall, **not the smoke** in a room and
//! **not the soot** gathered towards the next stage. All three rebuild
//! within a minute of play, and the soot's stages that have happened are on
//! the blocks already.

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use primitive_shared::protocol::BlockChange;
use primitive_shared::types::{block_kind, BlockId, BLOCK_STANDING_TORCH_LIT, BLOCK_STANDING_TORCH_OUT};
use primitive_shared::weather::{Precipitation, Weather};
use primitive_shared::wildfire::{
    self as rules, alight, burn_seconds_of, catch_threshold, ceiling_over, charred, damp_factor, draught, fuel,
    is_blazing, leaves_ash, licked, licks_as_a_hearth, seasoning, smoke_room, smoke_target, soot, with_soot, Fuel,
    Room, BLAZE_HEAT,
    COOLING_PER_SECOND, FLASH_SECONDS, HEARTH_HEAT, MAX_BURNING, MAX_CATCHES_PER_STEP, MAX_WARM_CELLS,
    NEAR_PLAYER, SMOKE_CLEAR_PER_SECOND, SMOKE_RISE_PER_SECOND, SMOKE_STEP_SECONDS, SOOT_STAGES,
    SMOULDER_SMOKE, SOOT_STAGE_SECONDS, STEP_SECONDS, TORCH_SECONDS,
};

use crate::logic::falling::BlockWorld;

/// Its own version, independent of the world's.
const SAVE_FORMAT_VERSION: u32 = 1;

/// A cell, in global block coordinates.
pub type Cell = (i32, i32, i32);

#[derive(Serialize, Deserialize)]
struct SaveFile {
    version: u32,
    /// Burning blocks: where, and seconds left.
    burning: Vec<(Cell, f32)>,
    /// Standing torch tops: where, and seconds of resin left.
    torches: Vec<(Cell, f32)>,
}

/// One hearth's smoke: how thick its room is, and the room.
#[derive(Debug, Default)]
struct Smoke {
    thickness: f32,
    /// The cells it fills, for asking whether a player's head is in one.
    /// Empty while the fire is vented.
    room: HashSet<Cell>,
}

/// What a step did.
#[derive(Debug, Default)]
pub struct Stepped {
    /// Cells that changed: caught, burnt out, went out, sooted, a torch
    /// burnt down.
    pub changes: Vec<BlockChange>,
    /// News for whoever is near each place.
    pub news: Vec<(Cell, &'static str)>,
}

/// Everything alight that is not a hearth, and the smoke of the hearths.
#[derive(Default)]
pub struct Wildfire {
    /// Burning blocks -> seconds left.
    burning: HashMap<Cell, f32>,
    /// Cells a leaf or a fleece has just flashed out of -> seconds they stay
    /// hot.
    flashes: HashMap<Cell, f32>,
    /// Fuel that flames are licking -> heat gathered so far.
    warm: HashMap<Cell, f32>,
    /// Warm cells already told about, so a wall starting to smoke is said
    /// once and not every second.
    warned: HashSet<Cell>,
    /// Standing torch tops -> seconds of resin left.
    torches: HashMap<Cell, f32>,
    /// Hearths -> their smoke.
    smoke: HashMap<Cell, Smoke>,
    /// Trunk cells scored for resin -> seconds until they bleed again. See
    /// `tap_trunk` on the server for why it is here and not on the block.
    scored: HashMap<Cell, f32>,
    /// Ceilings -> seconds of fire they have had under them.
    soot: HashMap<Cell, f32>,
    pending: Vec<Cell>,
    weather: Weather,
    /// The wind as `raft::Wind::vector`, blowing toward.
    wind: (f32, f32),
    /// Seconds towards the next spread step, and the next look at smoke.
    spread_clock: f32,
    smoke_clock: f32,
    dirty: bool,
    /// Hearths burning green or wet fuel. See `set_smouldering`.
    smouldering: HashSet<Cell>,
    /// The world's age in days, for the season's dryness
    /// (`wildfire::seasoning`). Zero until the tick loop says otherwise,
    /// which is the spring a world opens in -- so a `Wildfire` nobody has
    /// told the date to behaves like one in that spring rather than one
    /// in no season at all.
    world_days: f32,
}

/// Is `at` within reach of anybody? See `wildfire::NEAR_PLAYER`.
fn near_anyone(at: Cell, players: &[(f32, f32, f32)]) -> bool {
    let reach = NEAR_PLAYER as f32;
    players.iter().any(|&(x, _, z)| {
        let (dx, dz) = (at.0 as f32 + 0.5 - x, at.2 as f32 + 0.5 - z);
        dx * dx + dz * dz <= reach * reach
    })
}

/// Can the rain reach this cell? The fire map's own test: nothing solid
/// between it and the top of the world -- and **something wet falling**,
/// which is not the same question as "is the sky wet".
///
/// A storm over hot dry country arrives as dust (`weather::Precipitation`)
/// and dust puts nothing out. That is the whole of why a fire in the
/// desert is a different decision from a fire in a wood: there is no sky
/// to wait for. The snow line is not asked, deliberately -- snow and rain
/// both wet what they land on, so the only branch that can change the
/// answer here is the dust, and asking the season for the rest would be a
/// second copy of the snow line to keep in step with `season`.
fn rained_on(world: &dyn BlockWorld, weather: Weather, at: Cell) -> bool {
    let (temperature, humidity) = world.climate(at.0, at.1, at.2).unwrap_or(TEMPERATE);
    Precipitation::of(weather, false, temperature, humidity).wets()
        && ((at.1 + 1)..primitive_shared::types::CHUNK_SIZE_Y as i32)
            .all(|y| !world.block(at.0, y, at.2).is_some_and(primitive_shared::types::is_collidable))
}

/// What a world that cannot say its climate counts as: the middle of the
/// scale, which is a meadow. See `BlockWorld::climate`.
const TEMPERATE: (f32, f32) = (0.5, 0.5);

fn change((x, y, z): Cell, block: BlockId) -> BlockChange {
    BlockChange {
        global_x: x,
        global_y: y,
        global_z: z,
        block_id: block,
    }
}

impl Wildfire {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn is_dirty(&self) -> bool {
        self.dirty
    }

    /// How many blocks are alight.
    pub fn burning(&self) -> usize {
        self.burning.len()
    }

    /// The sky and the wind, once a tick, from the same clock the rafts use.
    pub fn set_weather(&mut self, weather: Weather, wind: (f32, f32)) {
        self.weather = weather;
        self.wind = wind;
    }

    /// What day of the world it is, once a tick, from the same clock the
    /// animals grow by (`Animals::calendar`).
    ///
    /// The season is the whole of how dry the country is here
    /// (`wildfire::seasoning`): a campfire in a meadow is a hazard in
    /// August and a warm place to sit in March, and that is the one thing
    /// about a fire that a player can plan around without being told a
    /// number.
    pub fn set_calendar(&mut self, world_days: f32) {
        self.world_days = world_days;
    }

    /// The hearths burning green or wet fuel, once a tick, from the fire map
    /// (`Fires::smouldering_cells`): they smoke and soot `SMOULDER_SMOKE`
    /// times as hard. Told, because this file sees a hearth only as a lit
    /// block and the fuel is the fire map's.
    pub fn set_smouldering(&mut self, cells: Vec<Cell>) {
        self.smouldering = cells.into_iter().collect();
    }

    /// A cell changed. Queue it; the world is not ours to read here.
    pub fn on_block_changed(&mut self, x: i32, y: i32, z: i32) {
        self.pending.push((x, y, z));
    }

    /// Sets how thick a hearth's smoke is now, for a scenario that cannot
    /// wait the minute and a half a room takes to fill. The room and the
    /// rest are the next step's: it rises or clears from here towards
    /// whatever the room's openings allow, which is the thing under test.
    pub fn set_smoke(&mut self, hearth: Cell, thickness: f32) {
        let thickness = if thickness.is_finite() { thickness.clamp(0.0, 1.0) } else { 0.0 };
        self.smoke.entry(hearth).or_default().thickness = thickness;
    }

    /// How thick the smoke is in the cell a player's eyes are in: the
    /// thickest room they are standing in, or nought.
    pub fn smoke_at(&self, eye: Cell) -> f32 {
        self.smoke
            .values()
            .filter(|smoke| smoke.room.contains(&eye))
            .map(|smoke| smoke.thickness)
            .fold(0.0, f32::max)
    }

    /// A standing torch lit again with a lump of resin: a whole wad. Says
    /// whether there was a burnt-out top there to light.
    pub fn rewad_torch(&mut self, world: &dyn BlockWorld, at: Cell) -> bool {
        if world.block(at.0, at.1, at.2).map(block_kind) != Some(BLOCK_STANDING_TORCH_OUT) {
            return false;
        }
        world.set(at.0, at.1, at.2, BLOCK_STANDING_TORCH_LIT);
        self.torches.insert(at, TORCH_SECONDS);
        self.dirty = true;
        true
    }

    /// Scores the trunk at `at` for resin, if it is not still healing from
    /// the last time; says whether it bled.
    pub fn score_trunk(&mut self, at: Cell, heals_in: f32) -> bool {
        if self.scored.contains_key(&at) {
            return false;
        }
        self.scored.insert(at, heals_in);
        true
    }

    /// Seconds of resin left in the torch at `at`.
    pub fn torch_seconds_left(&self, at: Cell) -> Option<f32> {
        self.torches.get(&at).copied()
    }

    /// Seconds a burning block has left.
    pub fn burn_seconds_left(&self, at: Cell) -> Option<f32> {
        self.burning.get(&at).copied()
    }

    /// One tick. `hearths` is every hearth the fire map has alight;
    /// `players` is where everybody's feet are.
    pub fn step(
        &mut self,
        world: &dyn BlockWorld,
        hearths: &[Cell],
        players: &[(f32, f32, f32)],
        dt: f32,
        budget: usize,
    ) -> Stepped {
        let mut out = Stepped::default();
        self.reconcile(world, budget);

        // Scored trunks heal on the clock, wherever they are.
        self.scored.retain(|_, left| {
            *left -= dt;
            *left > 0.0
        });

        // The torches burn on every tick, near anybody or not: they are
        // lights, and a light that waited for a player to come back would
        // be a light that lasts for ever in an empty camp.
        let mut burnt_down = Vec::new();
        for (&at, left) in self.torches.iter_mut() {
            *left -= dt;
            if *left <= 0.0 {
                burnt_down.push(at);
            }
        }
        for at in burnt_down {
            self.torches.remove(&at);
            self.dirty = true;
            if world.block(at.0, at.1, at.2).map(block_kind) == Some(BLOCK_STANDING_TORCH_LIT) {
                world.set(at.0, at.1, at.2, BLOCK_STANDING_TORCH_OUT);
                out.changes.push(change(at, BLOCK_STANDING_TORCH_OUT));
            }
        }

        self.spread_clock += dt;
        if self.spread_clock >= STEP_SECONDS {
            let seconds = self.spread_clock;
            self.spread_clock = 0.0;
            self.spread(world, hearths, players, seconds, &mut out);
        }
        self.smoke_clock += dt;
        if self.smoke_clock >= SMOKE_STEP_SECONDS {
            let seconds = self.smoke_clock;
            self.smoke_clock = 0.0;
            self.fill_rooms(world, hearths, players, seconds, &mut out);
        }
        out
    }

    /// The `CellMechanic` contract: cells that changed, a budget of them a
    /// tick. A burning block or a lit torch top with no clock is adopted
    /// with a whole one; an entry whose cell is no longer alight is dropped.
    fn reconcile(&mut self, world: &dyn BlockWorld, budget: usize) {
        let batch: Vec<Cell> = self.pending.drain(..budget.min(self.pending.len())).collect();
        for at in batch {
            let Some(block) = world.block(at.0, at.1, at.2) else {
                continue;
            };
            match (burn_seconds_of(block), self.burning.contains_key(&at)) {
                (Some(seconds), false) => {
                    self.burning.insert(at, seconds);
                    self.dirty = true;
                }
                (None, true) => {
                    self.burning.remove(&at);
                    self.dirty = true;
                }
                _ => {}
            }
            let lit_top = block_kind(block) == BLOCK_STANDING_TORCH_LIT;
            match (lit_top, self.torches.contains_key(&at)) {
                (true, false) => {
                    self.torches.insert(at, TORCH_SECONDS);
                    self.dirty = true;
                }
                (false, true) => {
                    self.torches.remove(&at);
                    self.dirty = true;
                }
                _ => {}
            }
            // Fuel that is not fuel any more is not warming.
            if fuel(block).is_none() {
                self.warm.remove(&at);
                self.warned.remove(&at);
            }
        }
    }

    /// One spread step: heat into what the flames lick, catches, burning
    /// down, and the rain.
    fn spread(
        &mut self,
        world: &dyn BlockWorld,
        hearths: &[Cell],
        players: &[(f32, f32, f32)],
        seconds: f32,
        out: &mut Stepped,
    ) {
        // The flashes cool first, so one that runs out this step gives no
        // heat in it.
        self.flashes.retain(|_, left| {
            *left -= seconds;
            *left > 0.0
        });

        // Every flame near anybody, with how hot it is.
        let mut flames: Vec<(Cell, f32)> = Vec::new();
        for &at in hearths {
            if near_anyone(at, players) && world.block(at.0, at.1, at.2).is_some_and(licks_as_a_hearth) {
                flames.push((at, HEARTH_HEAT));
            }
        }
        for &at in self.burning.keys().chain(self.flashes.keys()) {
            if near_anyone(at, players) {
                flames.push((at, BLAZE_HEAT));
            }
        }

        // Heat into what they lick.
        let mut licked_now: HashSet<Cell> = HashSet::new();
        for &(at, heat) in &flames {
            let open_above = world
                .block(at.0, at.1 + 1, at.2)
                .is_some_and(|block| !primitive_shared::types::is_collidable(block));
            for (cell, rising) in licked(at, open_above) {
                let Some(block) = world.block(cell.0, cell.1, cell.2) else {
                    continue;
                };
                if fuel(block).is_none() || self.burning.contains_key(&cell) {
                    continue;
                }
                // Wet fuel gathers nothing: see the module note on the rain.
                if rained_on(world, self.weather, cell) {
                    continue;
                }
                if !self.warm.contains_key(&cell) && self.warm.len() >= MAX_WARM_CELLS {
                    continue;
                }
                let gain = heat * draught((cell.0 - at.0, cell.2 - at.2), rising, self.wind) * seconds;
                *self.warm.entry(cell).or_insert(0.0) += gain;
                licked_now.insert(cell);
            }
        }

        // What nothing licked cools -- but only near somebody, so a wall a
        // player walked away from keeps its warmth for when they are back,
        // exactly as the fire beside it waits.
        self.warm.retain(|cell, heat| {
            if licked_now.contains(cell) || !near_anyone(*cell, players) {
                return true;
            }
            *heat -= COOLING_PER_SECOND * seconds;
            *heat > 0.0
        });
        let warm = &self.warm;
        self.warned.retain(|cell| warm.contains_key(cell));

        // Catches: the hottest first, within the bounds.
        //
        // **The threshold is the rules' one, moved by the year and by how
        // wet the fuel itself is** (`seasoning`, `damp_factor`). Both are
        // multiplies on one number rather than terms scattered through
        // the heat it gathers, so "how close is this to catching" stays a
        // single share a test can state: a green log in March needs six
        // times what a dry one in August does, and that is the whole of
        // the difference between a fire that gets loose and one that does
        // not.
        let dryness = seasoning(self.world_days);
        let mut ready: Vec<(Cell, Fuel, f32)> = self
            .warm
            .iter()
            .filter_map(|(&cell, &heat)| {
                let block = world.block(cell.0, cell.1, cell.2)?;
                let kind = fuel(block)?;
                Some((cell, kind, heat / (catch_threshold(kind, cell) * dryness * damp_factor(block))))
            })
            .collect();
        ready.sort_by(|a, b| b.2.total_cmp(&a.2).then(a.0.cmp(&b.0)));
        let mut caught = 0;
        for (cell, kind, share) in ready {
            if share < 1.0 {
                // Half way there is smoke off the boards: the warning a
                // player gets while there is time to move the fire.
                if share >= 0.5 && self.warned.insert(cell) {
                    out.news.push((cell, "the wood beside the fire is smoking: it will catch"));
                }
                continue;
            }
            if caught >= MAX_CATCHES_PER_STEP || self.burning.len() >= MAX_BURNING {
                break;
            }
            let Some(block) = world.block(cell.0, cell.1, cell.2) else {
                continue;
            };
            let Some(mut now) = alight(block) else {
                continue;
            };
            // **A burnt tuft leaves its ash where it stood** (`leaves_ash`)
            // -- on the ground and nowhere else, because ash is a cover
            // and one in mid-air would be a grey square hanging in a tree.
            // The cell stays hot either way (`flashes`), so the fire runs
            // on across the meadow and what it leaves behind it is the
            // grey the player follows home.
            if leaves_ash(kind)
                && world
                    .block(cell.0, cell.1 - 1, cell.2)
                    .is_some_and(primitive_shared::types::is_collidable)
            {
                now = primitive_shared::types::BLOCK_ASH;
            }
            world.set(cell.0, cell.1, cell.2, now);
            out.changes.push(change(cell, now));
            self.warm.remove(&cell);
            self.warned.remove(&cell);
            match kind.burn_seconds() {
                Some(burn) => {
                    self.burning.insert(cell, burn);
                    out.news.push((cell, "the wood has caught fire"));
                }
                None => {
                    self.flashes.insert(cell, FLASH_SECONDS);
                }
            }
            self.dirty = true;
            caught += 1;
        }

        // Burning down, and the rain putting out what it reaches.
        let mut done: Vec<Cell> = Vec::new();
        for (&cell, left) in self.burning.iter_mut() {
            if !near_anyone(cell, players) {
                continue;
            }
            if rained_on(world, self.weather, cell) {
                done.push(cell);
                continue;
            }
            *left -= seconds;
            if *left <= 0.0 {
                done.push(cell);
            }
        }
        done.sort_unstable();
        for cell in done {
            self.burning.remove(&cell);
            self.dirty = true;
            let Some(block) = world.block(cell.0, cell.1, cell.2) else {
                continue;
            };
            if !is_blazing(block) {
                continue;
            }
            if let Some(char) = charred(block) {
                world.set(cell.0, cell.1, cell.2, char);
                out.changes.push(change(cell, char));
            }
        }
    }

    /// Smoke and soot, every `SMOKE_STEP_SECONDS`.
    fn fill_rooms(
        &mut self,
        world: &dyn BlockWorld,
        hearths: &[Cell],
        players: &[(f32, f32, f32)],
        seconds: f32,
        out: &mut Stepped,
    ) {
        let look = |x, y, z| world.block(x, y, z);
        let lit: HashSet<Cell> = hearths
            .iter()
            .copied()
            .filter(|&at| near_anyone(at, players) && world.block(at.0, at.1, at.2).is_some_and(licks_as_a_hearth))
            .collect();

        for &at in &lit {
            // Green or wet fuel: thicker and faster, and sooner black. See
            // `SMOULDER_SMOKE`.
            let thick = if self.smouldering.contains(&at) { SMOULDER_SMOKE } else { 1.0 };
            let smoke = self.smoke.entry(at).or_default();
            match smoke_room(look, at) {
                Room::Vented => {
                    smoke.thickness = (smoke.thickness - SMOKE_CLEAR_PER_SECOND * seconds).max(0.0);
                    if smoke.thickness <= 0.0 {
                        smoke.room.clear();
                    }
                }
                room @ (Room::Closed(_) | Room::Leaky(..)) => {
                    let (cells, kept) = match room {
                        Room::Leaky(cells, kept) => (cells, kept),
                        Room::Closed(cells) => (cells, 1.0),
                        Room::Vented => unreachable!(),
                    };
                    let target = (smoke_target(cells.len()) * kept * thick).min(1.0);
                    smoke.thickness = if smoke.thickness < target {
                        (smoke.thickness + SMOKE_RISE_PER_SECOND * thick * seconds).min(target)
                    } else {
                        (smoke.thickness - SMOKE_CLEAR_PER_SECOND * seconds).max(target)
                    };
                    smoke.room = cells.into_iter().collect();
                }
            }

            // Soot on the ceiling over it.
            if let Some(ceiling) = ceiling_over(look, at) {
                if let Some(block) = world.block(ceiling.0, ceiling.1, ceiling.2) {
                    if rules::may_carry_soot(block) {
                        let had = soot(block);
                        let gathered = self
                            .soot
                            .entry(ceiling)
                            .or_insert(f32::from(had) * SOOT_STAGE_SECONDS);
                        *gathered += seconds * thick;
                        let stage = ((*gathered / SOOT_STAGE_SECONDS) as u8).min(SOOT_STAGES);
                        // ...and not onto a board the weather has greyed,
                        // which `with_soot` hands back unchanged (see
                        // `primitive_shared::weathering`): written anyway, it
                        // was a change every step that changed nothing.
                        let sooted = with_soot(block, stage);
                        if stage > had && sooted != block {
                            world.set(ceiling.0, ceiling.1, ceiling.2, sooted);
                            out.changes.push(change(ceiling, sooted));
                        }
                    }
                }
            }
        }

        // A hearth that is out, or out of anybody's reach, clears.
        self.smoke.retain(|at, smoke| {
            if lit.contains(at) {
                return true;
            }
            smoke.thickness -= SMOKE_CLEAR_PER_SECOND * seconds;
            smoke.thickness > 0.0
        });
        // Soot on a ceiling with no fire under it any more is forgotten; the
        // stages it reached are on the block.
        let ceilings: HashSet<Cell> = lit.iter().filter_map(|&at| ceiling_over(look, at)).collect();
        self.soot.retain(|at, _| ceilings.contains(at));
    }

    fn save_path(dir: &Path) -> PathBuf {
        dir.join("wildfire.bin")
    }

    /// Writes what is alight and the torches, atomically.
    pub fn save(&mut self, dir: &Path) -> std::io::Result<usize> {
        std::fs::create_dir_all(dir)?;
        let mut burning: Vec<(Cell, f32)> = self.burning.iter().map(|(&c, &s)| (c, s)).collect();
        let mut torches: Vec<(Cell, f32)> = self.torches.iter().map(|(&c, &s)| (c, s)).collect();
        burning.sort_by_key(|(c, _)| *c);
        torches.sort_by_key(|(c, _)| *c);
        let count = burning.len() + torches.len();
        let bytes = bincode::serialize(&SaveFile {
            version: SAVE_FORMAT_VERSION,
            burning,
            torches,
        })
        .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))?;
        let final_path = Self::save_path(dir);
        let tmp_path = final_path.with_extension("bin.tmp");
        std::fs::write(&tmp_path, &bytes)?;
        std::fs::rename(&tmp_path, &final_path)?;
        self.dirty = false;
        Ok(count)
    }

    /// Reads them back. A missing or unreadable file is nothing alight --
    /// the fire map's reasoning: every burning block and torch it would
    /// have held is adopted with a whole clock the first time its cell is
    /// looked at, which costs a fire a few minutes and nobody anything.
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
        // Off a disk an operator can edit: a clock of `NaN` never runs out.
        let usable = |s: f32| s.is_finite() && s > 0.0;
        self.burning = save.burning.into_iter().filter(|&(_, s)| usable(s)).collect();
        self.torches = save
            .torches
            .into_iter()
            .filter(|&(_, s)| usable(s))
            .map(|(c, s)| (c, s.min(TORCH_SECONDS)))
            .collect();
        Ok(self.burning.len() + self.torches.len())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::logic::falling::tests::TestWorld;
    use primitive_shared::types::{
        oriented, Axis, BLOCK_AIR, BLOCK_BURNING_PLANKS, BLOCK_CAMPFIRE_LIT, BLOCK_CHARRED_LOG,
        BLOCK_CHARRED_PLANKS, BLOCK_COBBLESTONE, BLOCK_LEAVES, BLOCK_LOG, BLOCK_PLANKS, BLOCK_STONE,
    };

    const FIRE: Cell = (0, 10, 0);
    const PLAYER: [(f32, f32, f32); 1] = [(3.0, 10.0, 3.0)];

    /// A stone floor under a campfire.
    fn camp() -> TestWorld {
        let world = TestWorld::default();
        for x in -8..=8 {
            for z in -8..=8 {
                world.put(x, 9, z, BLOCK_STONE);
            }
        }
        world.put(FIRE.0, FIRE.1, FIRE.2, BLOCK_CAMPFIRE_LIT);
        world
    }

    /// Runs the mechanic for `seconds` at twenty ticks a second.
    fn run(wildfire: &mut Wildfire, world: &TestWorld, hearths: &[Cell], players: &[(f32, f32, f32)], seconds: f32) -> Stepped {
        let mut all = Stepped::default();
        for _ in 0..(seconds * 20.0) as usize {
            let stepped = wildfire.step(world, hearths, players, 0.05, 64);
            all.changes.extend(stepped.changes);
            all.news.extend(stepped.news);
        }
        all
    }

    #[test]
    fn boards_beside_a_campfire_smoke_first_catch_in_about_a_minute_and_burn_to_char() {
        let world = camp();
        let boards = (1, 10, 0);
        world.put(boards.0, boards.1, boards.2, BLOCK_PLANKS);
        let mut wildfire = Wildfire::new();

        let early = run(&mut wildfire, &world, &[FIRE], &PLAYER, 30.0);
        assert_eq!(world.get(boards.0, boards.1, boards.2), BLOCK_PLANKS, "boards caught in half a minute");

        let later = run(&mut wildfire, &world, &[FIRE], &PLAYER, 50.0);
        assert_eq!(world.get(boards.0, boards.1, boards.2), BLOCK_BURNING_PLANKS, "boards had not caught after eighty seconds");
        let smoking = early.news.iter().chain(&later.news).position(|(at, text)| *at == boards && text.contains("smoking"));
        let caught = later.news.iter().position(|(at, text)| *at == boards && text.contains("caught"));
        assert!(smoking.is_some() && caught.is_some(), "the boards caught without smoking first");

        run(&mut wildfire, &world, &[FIRE], &PLAYER, rules::BOARDS_BURN_SECONDS + 2.0);
        assert_eq!(world.get(boards.0, boards.1, boards.2), BLOCK_CHARRED_PLANKS, "the boards did not burn to char");
        assert_eq!(wildfire.burning(), 0);
    }

    #[test]
    fn a_fire_in_dry_grass_runs_downwind_and_leaves_ash_behind_it() {
        use primitive_shared::types::{BLOCK_ASH, BLOCK_DRY_GRASS};
        // A meadow round a campfire, in a wind out of the west.
        let world = camp();
        for x in -4..=4 {
            for z in -4..=4 {
                if (x, z) != (0, 0) {
                    world.put(x, 10, z, BLOCK_DRY_GRASS);
                }
            }
        }
        let mut wildfire = Wildfire::new();
        wildfire.set_weather(primitive_shared::weather::Weather::Clear, (1.0, 0.0));
        // High summer: the season the fire is a hazard in.
        wildfire.set_calendar(primitive_shared::season::MIDSUMMER_WORLD_TIME);
        run(&mut wildfire, &world, &[FIRE], &PLAYER, 8.0);

        // Seconds, not minutes: a tuft beside a fire in August is the
        // fastest thing in the fuel table, and what it leaves is ash.
        assert_eq!(world.get(1, 10, 0), BLOCK_ASH, "the tuft beside the fire did not burn");
        let burnt = |ahead: bool| {
            (1..=4)
                .filter(|&d| {
                    let x = if ahead { d } else { -d };
                    world.get(x, 10, 0) == BLOCK_ASH
                })
                .count()
        };
        assert!(
            burnt(true) > burnt(false),
            "the fire ran upwind as fast as down: {} ahead, {} behind",
            burnt(true),
            burnt(false)
        );

        // ...and it keeps going: left alone, the meadow burns out to the
        // edge of what the fire can reach, which is what makes a grass
        // fire a thing a player runs from rather than stamps on.
        run(&mut wildfire, &world, &[FIRE], &PLAYER, 60.0);
        let left = (-4..=4)
            .flat_map(|x| (-4..=4).map(move |z| (x, z)))
            .filter(|&(x, z)| world.get(x, 10, z) == BLOCK_DRY_GRASS)
            .count();
        assert!(left < 40, "{left} tufts of eighty survived the fire");
        assert_eq!(world.get(4, 10, 0), BLOCK_ASH, "the fire did not cross the meadow downwind");
    }

    #[test]
    fn a_storm_over_the_desert_does_not_put_the_fire_out() {
        use primitive_shared::weather::Weather;
        // The same burning roof, twice, under the same storm: once in a
        // meadow, where the rain reaches it, and once in hot dry country,
        // where the storm is a dust storm and there is no sky to wait
        // for. See `rained_on`.
        let quenched = |climate: Option<(f32, f32)>| {
            let world = camp();
            let open = (0, 12, 0);
            world.put(open.0, open.1, open.2, BLOCK_BURNING_PLANKS);
            world.set_climate(climate);
            let mut wildfire = Wildfire::new();
            wildfire.on_block_changed(open.0, open.1, open.2);
            wildfire.set_weather(Weather::Storm, (0.0, 0.0));
            run(&mut wildfire, &world, &[], &PLAYER, 3.0);
            world.get(open.0, open.1, open.2) == BLOCK_CHARRED_PLANKS
        };
        assert!(quenched(None), "the rain did not put out a fire in a world with no climate");
        assert!(quenched(Some((0.5, 0.8))), "the rain did not reach a fire in a marsh");
        assert!(!quenched(Some((0.95, 0.1))), "a dust storm put a fire out in the desert");
    }

    #[test]
    fn a_fire_moved_in_time_did_nothing() {
        let world = camp();
        let boards = (1, 10, 0);
        world.put(boards.0, boards.1, boards.2, BLOCK_PLANKS);
        let mut wildfire = Wildfire::new();
        run(&mut wildfire, &world, &[FIRE], &PLAYER, 30.0);
        // Put out: no hearth any more.
        world.put(FIRE.0, FIRE.1, FIRE.2, BLOCK_AIR);
        run(&mut wildfire, &world, &[], &PLAYER, 120.0);
        assert_eq!(world.get(boards.0, boards.1, boards.2), BLOCK_PLANKS);
        assert!(wildfire.warm.is_empty(), "the boards never cooled");
    }

    #[test]
    fn nothing_happens_where_nobody_is() {
        let world = camp();
        world.put(1, 10, 0, BLOCK_PLANKS);
        let mut wildfire = Wildfire::new();
        let far = [(500.0, 10.0, 500.0)];
        run(&mut wildfire, &world, &[FIRE], &far, 300.0);
        assert_eq!(world.get(1, 10, 0), BLOCK_PLANKS, "a fire nobody was near spread");
    }

    #[test]
    fn a_burning_log_wall_spreads_but_never_more_than_the_cap_at_once() {
        // A long wall of logs lit at one end: it spreads along, a step at a
        // time, and the number alight never passes the cap however long it
        // runs.
        let world = camp();
        for x in 1..=40 {
            for y in 10..=12 {
                world.put(x, y, 0, oriented(BLOCK_LOG, Axis::X));
            }
        }
        let mut wildfire = Wildfire::new();
        let players = [(20.0, 10.0, 3.0)];
        let mut most_caught_in_a_step = 0;
        for _ in 0..600 * 20 {
            let stepped = wildfire.step(&world, &[FIRE], &players, 0.05, 64);
            let caught = stepped
                .changes
                .iter()
                .filter(|c| is_blazing(c.block_id))
                .count();
            most_caught_in_a_step = most_caught_in_a_step.max(caught);
            assert!(wildfire.burning() <= MAX_BURNING);
        }
        assert!(most_caught_in_a_step <= MAX_CATCHES_PER_STEP, "{most_caught_in_a_step} caught in one step");
        let charred = (1..=40).filter(|&x| block_kind(world.get(x, 10, 0)) == BLOCK_CHARRED_LOG).count();
        assert!(charred > 3, "a burning log wall did not spread along itself ({charred} charred)");
    }

    #[test]
    fn rain_puts_out_a_burning_roof_and_leaves_one_under_cover_burning() {
        let world = camp();
        let open = (5, 10, 5);
        let covered = (-5, 10, -5);
        world.put(covered.0, covered.1 + 3, covered.2, BLOCK_COBBLESTONE);
        for at in [open, covered] {
            world.put(at.0, at.1, at.2, BLOCK_BURNING_PLANKS);
        }
        let mut wildfire = Wildfire::new();
        wildfire.on_block_changed(open.0, open.1, open.2);
        wildfire.on_block_changed(covered.0, covered.1, covered.2);
        wildfire.set_weather(Weather::Rain, (0.0, 0.0));
        run(&mut wildfire, &world, &[], &[(0.0, 10.0, 0.0)], 3.0);
        assert_eq!(world.get(open.0, open.1, open.2), BLOCK_CHARRED_PLANKS, "the rain did not put out an open fire");
        assert_eq!(world.get(covered.0, covered.1, covered.2), BLOCK_BURNING_PLANKS, "the rain reached under a roof");
    }

    #[test]
    fn a_canopy_over_a_campfire_goes_up_fast_and_leaves_air() {
        let world = camp();
        let leaf = (0, 11, 0);
        world.put(leaf.0, leaf.1, leaf.2, BLOCK_LEAVES);
        let mut wildfire = Wildfire::new();
        run(&mut wildfire, &world, &[FIRE], &PLAYER, 12.0);
        assert_eq!(world.get(leaf.0, leaf.1, leaf.2), BLOCK_AIR, "a leaf over a campfire survived twelve seconds");
    }

    #[test]
    fn the_wind_brings_the_downwind_wall_down_first() {
        let world = camp();
        let (downwind, upwind) = ((1, 10, 0), (-1, 10, 0));
        world.put(downwind.0, downwind.1, downwind.2, BLOCK_PLANKS);
        world.put(upwind.0, upwind.1, upwind.2, BLOCK_PLANKS);
        let mut wildfire = Wildfire::new();
        wildfire.set_weather(Weather::Clear, (0.8, 0.0));
        run(&mut wildfire, &world, &[FIRE], &PLAYER, 40.0);
        assert_eq!(world.get(downwind.0, downwind.1, downwind.2), BLOCK_BURNING_PLANKS, "downwind had not caught in a strong wind");
        assert_eq!(world.get(upwind.0, upwind.1, upwind.2), BLOCK_PLANKS, "upwind caught as fast as downwind");
    }

    /// A shut cobble hut round the fire: walls at 2, roof at 13.
    fn hut(world: &TestWorld, door: bool) {
        for x in -2i32..=2 {
            for z in -2i32..=2 {
                world.put(x, 13, z, BLOCK_COBBLESTONE);
                for y in 10..13 {
                    if (x.abs() == 2 || z.abs() == 2) && !(door && x == 2 && z == 0 && y <= 11) {
                        world.put(x, y, z, BLOCK_COBBLESTONE);
                    }
                }
            }
        }
    }

    #[test]
    fn a_shut_hut_fills_with_smoke_and_opening_the_door_thins_it_and_a_roof_hole_more() {
        let world = camp();
        hut(&world, false);
        let mut wildfire = Wildfire::new();
        let head = (1, 11, 1);
        run(&mut wildfire, &world, &[FIRE], &PLAYER, 120.0);
        let thick = wildfire.smoke_at(head);
        assert!(thick >= rules::SMOKE_CHOKES, "two minutes in a shut hut with a fire and the smoke is only {thick}");

        // A doorway lets some out and the room is still smoky: a house with a
        // way in is still a house with a fire in it.
        world.put(2, 10, 0, BLOCK_AIR);
        world.put(2, 11, 0, BLOCK_AIR);
        run(&mut wildfire, &world, &[FIRE], &PLAYER, 60.0);
        let doorway = wildfire.smoke_at(head);
        assert!(doorway < thick, "an open door let no smoke out: {doorway} against {thick}");
        assert!(doorway > 0.2, "an open door cleared the smoke out of a house altogether: {doorway}");

        // ...and a hole in the roof over the fire takes it down further.
        world.put(0, 13, 0, BLOCK_AIR);
        run(&mut wildfire, &world, &[FIRE], &PLAYER, 60.0);
        let holed = wildfire.smoke_at(head);
        assert!(holed < doorway, "a smoke hole did nothing a doorway had not: {holed} against {doorway}");
    }

    /// **Green or wet fuel smokes harder** (`SMOULDER_SMOKE`), which the room
    /// and the soot learn from the fire map (`set_smouldering`): the same hut
    /// fills twice as fast, and its ceiling blackens in half the time.
    #[test]
    fn green_wood_fills_a_hut_with_smoke_faster_and_soots_its_ceiling_sooner() {
        let burn = |smouldering: bool, seconds: f32| {
            let world = camp();
            hut(&world, true);
            let mut wildfire = Wildfire::new();
            if smouldering {
                wildfire.set_smouldering(vec![FIRE]);
            }
            run(&mut wildfire, &world, &[FIRE], &PLAYER, seconds);
            (wildfire.smoke_at((1, 11, 1)), soot(world.get(0, 13, 0)))
        };
        let (dry, _) = burn(false, 30.0);
        let (green, _) = burn(true, 30.0);
        assert!(green > dry * 1.5, "green wood smoked no harder than dry: {green} against {dry}");
        let early = SOOT_STAGE_SECONDS * 0.5 + 10.0;
        assert_eq!(burn(false, early).1, 0, "dry wood sooted the ceiling early");
        assert_eq!(burn(true, early).1, 1, "green wood sooted the ceiling no sooner than dry");
    }

    #[test]
    fn the_ceiling_over_a_hearth_blackens_a_stage_at_a_time() {
        let world = camp();
        hut(&world, true);
        let mut wildfire = Wildfire::new();
        run(&mut wildfire, &world, &[FIRE], &PLAYER, SOOT_STAGE_SECONDS - 10.0);
        assert_eq!(soot(world.get(0, 13, 0)), 0, "the ceiling sooted before its time");
        run(&mut wildfire, &world, &[FIRE], &PLAYER, 20.0);
        assert_eq!(soot(world.get(0, 13, 0)), 1);
        assert_eq!(block_kind(world.get(0, 13, 0)), BLOCK_COBBLESTONE);
        assert_eq!(soot(world.get(1, 13, 0)), 0, "soot spread across the roof rather than over the fire");
    }

    #[test]
    fn a_standing_torch_burns_out_and_resin_lights_it_again() {
        let world = camp();
        let top = (3, 11, 3);
        world.put(3, 10, 3, primitive_shared::types::BLOCK_STANDING_TORCH);
        world.put(top.0, top.1, top.2, BLOCK_STANDING_TORCH_LIT);
        let mut wildfire = Wildfire::new();
        wildfire.on_block_changed(top.0, top.1, top.2);
        // In the rain the whole time: resin does not care.
        wildfire.set_weather(Weather::Storm, (0.0, 0.0));
        run(&mut wildfire, &world, &[], &PLAYER, TORCH_SECONDS - 5.0);
        assert_eq!(world.get(top.0, top.1, top.2), BLOCK_STANDING_TORCH_LIT, "the rain or the clock put the torch out early");
        run(&mut wildfire, &world, &[], &PLAYER, 10.0);
        assert_eq!(world.get(top.0, top.1, top.2), BLOCK_STANDING_TORCH_OUT);
        assert!(wildfire.rewad_torch(&world, top));
        assert_eq!(world.get(top.0, top.1, top.2), BLOCK_STANDING_TORCH_LIT);
        assert!(!wildfire.rewad_torch(&world, top), "a lit torch took more resin");
    }

    #[test]
    fn what_is_alight_survives_a_restart() {
        let dir = std::env::temp_dir().join(format!("primitive-wildfire-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let world = camp();
        world.put(4, 10, 4, BLOCK_BURNING_PLANKS);
        let mut wildfire = Wildfire::new();
        wildfire.on_block_changed(4, 10, 4);
        run(&mut wildfire, &world, &[], &PLAYER, 10.0);
        let left = wildfire.burn_seconds_left((4, 10, 4)).unwrap();
        assert!(wildfire.is_dirty());
        assert_eq!(wildfire.save(&dir).unwrap(), 1);
        let mut back = Wildfire::new();
        assert_eq!(back.load(&dir).unwrap(), 1);
        assert_eq!(back.burn_seconds_left((4, 10, 4)), Some(left));
        let _ = std::fs::remove_dir_all(&dir);
    }
}
