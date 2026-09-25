//! The land a player has actually seen, from above.
//!
//! ## Two halves, and only one of them is here
//!
//! A map (`types::BLOCK_MAP`) shows the land a player has **walked**,
//! and drawing that takes two things that live in different places:
//!
//! * **which ground the player is allowed to have looked at** -- the
//!   server's, kept in their profile and sent here as a set of cells
//!   (`primitive_shared::trail`). It grows only while a map is in the
//!   pack, it survives a new computer, and a client cannot hand itself
//!   the world by editing a file.
//! * **what that ground looks like** -- this file's, surveyed off the
//!   chunks the server streamed anyway. The alternative was to send the
//!   picture too: a second copy of the world on the wire, as colours, for
//!   a page nobody has open.
//!
//! What is drawn is the intersection. That is why a map fills in as a
//! circle around the path rather than as a square of view distance --
//! and why a better computer no longer draws a bigger map, which is what
//! this used to do and what made the whole thing a machine setting.
//!
//! ## What "seen" means, on this side of it
//!
//! A chunk the server streamed to this client. That is not the same as a
//! column the player looked at -- a chunk behind a hill arrives with the
//! ones in front of it -- and it is the right answer anyway: a survey
//! that waited for the camera to sweep every column would be a map with
//! a stripe of holes down every valley the player walked along without
//! turning their head. The trail is what stops that generosity reaching
//! the paper.
//!
//! ## What is kept
//!
//! Two bytes a column and nothing else: what the top of it is made of,
//! reduced to one of nine grounds (`Ground`), and how high that top is.
//! The height is what makes a hill read as a hill -- see
//! `map_screen::shade` -- and the ground is what makes a lake read as a
//! lake. A whole chunk is 512 bytes, so ten thousand of them, which is a
//! long way walked in every direction, is five megabytes on disk.
//!
//! **Not the chunks.** Keeping the blocks would be a second copy of the
//! world, on the client, forever; and a map drawn from them would pay for
//! a column scan on every frame it was open.
//!
//! ## When it is built, and what that costs
//!
//! A chunk is surveyed after it is integrated, from a queue drained in
//! [`ExploredMap::catch_up`] under a time budget of its own -- the same
//! shape as every other streaming phase (see `streaming_budget` in
//! `lib.rs`), because an unbudgeted phase is a phase that surveys forty
//! chunks in the frame a world opens. Surveying one is a walk down each
//! of its 256 columns from the chunk's skyline to the first thing a bird
//! would land on; `a_survey_is_cheap_enough_to_run_during_streaming`
//! measures it.
//!
//! An edit re-queues its chunk rather than patching one column, because
//! the chunk is already in hand and a second code path for "this column
//! changed" is a second place for the answer to differ.
//!
//! ## Why the picture is still personal in multiplayer
//!
//! Each client keeps its survey filed under the server's address, the
//! world's seed and the player's name. The alternative -- the server
//! sending everyone the union of what anybody has explored -- was
//! rejected: it turns the question "where is my friend's base" into
//! something the game answers for you. What you know of the land is what
//! you walked; what somebody else walked, they can tell you about.
//!
//! ## Marks
//!
//! A cairn piled or a blaze cut goes on the map of the player who put it
//! there, **if they were carrying a map at the time** -- the server's
//! rule, written in `trail`, and the same rule the ground itself follows.
//! The marks arrive with the trail and are never invented here.
//!
//! What this side *does* notice is a mark that is no longer there: the
//! survey of a chunk checks the cells of any marks in it, and a cell that
//! is neither a cairn nor a blaze any more is reported to the server
//! (`ExploredMap::take_lost_marks`), which rubs it off the profile. The
//! client is the one that notices because the client is the one holding
//! the chunk; the worst a lying one can do with it is rub out its own
//! player's mark.
//!
//! **Found at the marks' own cells only**, rather than by a scan of the
//! whole chunk for heaps of stones. A scan of 65 thousand cells a chunk
//! would be the survey's cost thirty times over, for cairns built in
//! caves -- which are not landmarks anybody sees from a hill.

use std::collections::{HashMap, HashSet, VecDeque};
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use primitive_shared::blocks::{definition, Matter, Shape};
use primitive_shared::packed::PackedChunk;
use primitive_shared::types::{
    block_kind, is_air, BlockId, ChunkPos, BLOCK_ASH, BLOCK_BASALT, BLOCK_BOUGH, BLOCK_CLAY,
    BLOCK_COBBLESTONE, BLOCK_DIRT, BLOCK_FARMLAND, BLOCK_GRANITE, BLOCK_GRASS, BLOCK_GRAVEL,
    BLOCK_BLAZE, BLOCK_CAIRN, BLOCK_ICE, BLOCK_LIMESTONE, BLOCK_PEAT, BLOCK_SAND, BLOCK_SANDSTONE,
    BLOCK_SANDY_SOIL, BLOCK_SNOW, BLOCK_STONE, BLOCK_TWIG, CHUNK_SIZE_X, CHUNK_SIZE_Y,
    CHUNK_SIZE_Z,
};

use primitive_shared::trail::{Mark, Trail};

use crate::logic::chunk_manager::ChunkManager;

/// Columns along one side of a chunk.
const SIDE: usize = CHUNK_SIZE_X;
const COLUMNS: usize = CHUNK_SIZE_X * CHUNK_SIZE_Z;

/// What the top of a column is, as far as a map cares.
///
/// Nine and no more, because a map is read at a glance: a legend with a
/// colour per block would be a legend, and nobody would read it. The
/// question a player brings to a map is "water, open ground, trees, or
/// rock" -- and "is that somebody's house".
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[repr(u8)]
pub enum Ground {
    /// Never streamed. Drawn as nothing at all.
    Unseen = 0,
    Water = 1,
    Sand = 2,
    Grass = 3,
    /// A canopy: leaves, or the trunk where there are none.
    Forest = 4,
    Snow = 5,
    /// Bare stone, scree, ore.
    Rock = 6,
    /// Earth with nothing on it: soil, a field, peat, clay.
    Soil = 7,
    /// Anything a person put there.
    Built = 8,
}

impl Ground {
    fn from_byte(byte: u8) -> Ground {
        match byte {
            1 => Ground::Water,
            2 => Ground::Sand,
            3 => Ground::Grass,
            4 => Ground::Forest,
            5 => Ground::Snow,
            6 => Ground::Rock,
            7 => Ground::Soil,
            8 => Ground::Built,
            // Including a byte from a file this build did not write:
            // nothing is a better guess than something.
            _ => Ground::Unseen,
        }
    }

    /// What a block looks like from above, or `None` for something a bird
    /// would see straight through: air, a tuft of grass, a pebble, a torch.
    ///
    /// **By shape and matter first, by name last.** The first two are the
    /// block table's own words for what a block is, so a block added next
    /// month is already classified correctly; the name is the fallback for
    /// telling a canopy from a wall, where the table has no word, and it
    /// is what keeps a new kind of leaf from being drawn as a house.
    pub fn of(block: BlockId) -> Option<Ground> {
        let kind = block_kind(block);
        // A cairn or a blaze is seen through to the ground it stands on:
        // both are marks on the map (see "Marks" above), not patches of
        // building. Without this a line of blazed trees would read as a
        // row of huts.
        if is_air(kind) || matches!(kind, BLOCK_CAIRN | BLOCK_BLAZE) {
            return None;
        }
        let def = definition(kind);
        // A plant standing in the sea is still the sea from above.
        if def.matter == Matter::Liquid {
            return Some(Ground::Water);
        }
        if def.shape != Shape::Cube {
            return None;
        }
        Some(match kind {
            BLOCK_SNOW | BLOCK_ICE => Ground::Snow,
            BLOCK_SAND | BLOCK_SANDSTONE | BLOCK_SANDY_SOIL => Ground::Sand,
            BLOCK_GRASS => Ground::Grass,
            BLOCK_DIRT | BLOCK_FARMLAND | BLOCK_PEAT | BLOCK_CLAY | BLOCK_ASH => Ground::Soil,
            BLOCK_STONE | BLOCK_GRANITE | BLOCK_BASALT | BLOCK_LIMESTONE | BLOCK_GRAVEL
            | BLOCK_COBBLESTONE => Ground::Rock,
            BLOCK_TWIG | BLOCK_BOUGH => Ground::Forest,
            // ...and every other bark's pieces, which are ids of their own.
            _ if primitive_shared::types::is_branch(block) => Ground::Forest,
            // ...and a fir's needles, which are a fir's leaves.
            _ if def.name.contains("leaves") || def.name.ends_with("needles") || def.name.ends_with("log") => {
                Ground::Forest
            }
            _ if def.name.ends_with("ore") => Ground::Rock,
            // ...and the ground's rocks, rubble and soils, as what they stand
            // in for (`ground::as_common`): granite gravel is rock on a map,
            // chernozem is soil, a new rock is stone.
            _ if kind >= 512 && primitive_shared::ground::as_common(kind) != kind => {
                return Self::of(primitive_shared::ground::as_common(kind));
            }
            _ if primitive_shared::ground::rock_of(kind).is_some() || primitive_shared::ground::is_soil(kind) => {
                Ground::Soil
            }
            _ => Ground::Built,
        })
    }

    /// The colour it is drawn in.
    ///
    /// Muted, and far enough apart in lightness as well as hue that a map
    /// read by somebody who cannot tell green from brown still separates
    /// forest from field: forest is the darkest thing on it after water.
    pub fn colour(self) -> [f32; 3] {
        match self {
            Ground::Unseen => [0.0, 0.0, 0.0],
            Ground::Water => [0.19, 0.34, 0.58],
            Ground::Sand => [0.83, 0.77, 0.55],
            Ground::Grass => [0.42, 0.62, 0.30],
            Ground::Forest => [0.16, 0.34, 0.17],
            Ground::Snow => [0.93, 0.95, 0.97],
            Ground::Rock => [0.53, 0.53, 0.55],
            Ground::Soil => [0.52, 0.40, 0.27],
            Ground::Built => [0.74, 0.50, 0.30],
        }
    }
}

/// One chunk, seen from above.
#[derive(Clone, PartialEq, Eq)]
pub struct Tile {
    ground: [u8; COLUMNS],
    height: [u8; COLUMNS],
}

impl Tile {
    fn blank() -> Self {
        Self {
            ground: [0; COLUMNS],
            height: [0; COLUMNS],
        }
    }

    /// A tile with one ground everywhere, at one height. For tests and for
    /// the pictures `ui::snapshot` draws.
    #[cfg(test)]
    pub fn uniform(ground: Ground, height: u8) -> Self {
        Self {
            ground: [ground as u8; COLUMNS],
            height: [height; COLUMNS],
        }
    }

    /// Sets one column. For tests and pictures.
    #[cfg(test)]
    pub fn set(&mut self, lx: usize, lz: usize, ground: Ground, height: u8) {
        self.ground[lz * SIDE + lx] = ground as u8;
        self.height[lz * SIDE + lx] = height;
    }

    fn at(&self, lx: usize, lz: usize) -> (Ground, u8) {
        let index = lz * SIDE + lx;
        (Ground::from_byte(self.ground[index]), self.height[index])
    }
}

/// Looks down on a chunk.
///
/// From the chunk's skyline downward, so the two thirds of every chunk
/// that is sky costs one call rather than a hundred and fifty reads a
/// column. A column with nothing a bird would land on -- a void, a
/// shaft to the floor of the world -- is left `Unseen`, which is what it
/// looks like from above.
pub fn survey(chunk: &PackedChunk) -> Tile {
    let mut tile = Tile::blank();
    let skyline = chunk.skyline();
    for lz in 0..SIDE {
        for lx in 0..SIDE {
            for y in (0..skyline).rev() {
                let block = chunk.get(lx, y as usize, lz);
                if let Some(ground) = Ground::of(block) {
                    let index = lz * SIDE + lx;
                    tile.ground[index] = ground as u8;
                    // The world is 256 tall, so a height is a byte by
                    // construction; clamped anyway, because a taller world
                    // would otherwise wrap its mountains into valleys.
                    tile.height[index] = y.clamp(0, 255) as u8;
                    break;
                }
            }
        }
    }
    tile
}

/// Where a player can find their way back to, as the server last said.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Landmarks {
    pub spawn: Option<(i32, i32, i32)>,
    /// Oldest first. See `MAX_BAGS` on the server.
    pub bags: Vec<(i32, i32, i32)>,
}

/// Everything seen, and what is still waiting to be looked at.
#[derive(Default)]
pub struct ExploredMap {
    tiles: HashMap<ChunkPos, Box<Tile>>,
    queue: VecDeque<ChunkPos>,
    queued: HashSet<ChunkPos>,
    /// Bumped whenever a tile changes, which is what an open map screen
    /// rebuilds on -- see `Journal::ui_key`.
    revision: u64,
    unsaved: bool,
    path: Option<PathBuf>,
    /// Where the player has walked with a map on them, and what they
    /// marked, **as the server last said**. Nothing here ever adds to it;
    /// see the two halves at the top of this file.
    trail: Trail,
    /// Marks whose cells this client has since looked at and found empty,
    /// waiting to be told to the server. Drained by
    /// [`ExploredMap::take_lost_marks`].
    lost: Vec<(i32, i32, i32)>,
}

/// The first four bytes of a map file.
const MAGIC: &[u8; 4] = b"PMAP";
/// Bumped on any change to what follows the magic.
///
/// **2 added marks after the tiles, and 3 took them away again**: the
/// marks are the server's now (`primitive_shared::trail`), and a second
/// copy of them beside the picture would be a copy that disagrees with
/// the profile the moment a player logs in from anywhere else. Formats 1
/// and 2 are still read -- 2's marks are skipped -- because a player's
/// survey is the one thing on the client that took hours to make, and a
/// new build that blanked it would be a new build that cost them the walk.
const FORMAT: u32 = 3;
/// One tile on disk: its position, then the two arrays.
const RECORD: usize = 8 + COLUMNS * 2;

impl ExploredMap {
    #[cfg(test)]
    pub fn new() -> Self {
        Self::default()
    }

    /// The map kept at `path`, or a blank one filed there.
    ///
    /// **A file that will not read is a blank map, not an error.** The map
    /// is a convenience built from what the server sends, and a player
    /// who is told their world will not open because a picture of it is
    /// damaged has been handed the wrong priority. It is rebuilt as they
    /// walk.
    pub fn open(path: PathBuf) -> Self {
        let tiles = match std::fs::read(&path) {
            Ok(bytes) => Self::from_bytes(&bytes).unwrap_or_else(|| {
                eprintln!("[map] {} is not a map this build can read; starting a blank one", path.display());
                HashMap::new()
            }),
            Err(_) => HashMap::new(),
        };
        Self {
            revision: tiles.len() as u64,
            tiles,
            path: Some(path),
            ..Self::default()
        }
    }

    /// The marks this player has written down, and their names -- empty
    /// for one they did not name.
    pub fn marks(&self) -> &[Mark] {
        self.trail.marks()
    }

    /// The name of the mark at a cell, if there is a mark there.
    pub fn mark_name(&self, at: (i32, i32, i32)) -> Option<&str> {
        self.trail.marks().iter().find(|mark| mark.at == at).map(|mark| mark.name.as_str())
    }

    /// The server said where this player has been. `whole` replaces what
    /// is held; otherwise it is added to it. See `ServerMessage::Trail`.
    pub fn trail_arrived(&mut self, cells: Vec<(i32, i32)>, marks: Vec<Mark>, whole: bool) {
        if whole {
            self.trail = Trail::default();
            self.lost.clear();
        }
        self.trail.absorb(cells);
        self.trail.absorb_marks(marks);
        // The picture does not change, but what of it may be drawn does --
        // and an open map is rebuilt off this number, so a trail that
        // arrived while the page was up has to bump it or the page stays
        // the size it was when it opened.
        self.revision = self.revision.wrapping_add(1);
    }

    /// Everything the server has said about where this player has walked,
    /// for the tests and for the screen's "you have been nowhere" line.
    pub fn trail(&self) -> &Trail {
        &self.trail
    }

    /// **For tests and for the pictures `ui::snapshot` draws**: says the
    /// server told this player they walked every column of every chunk
    /// surveyed so far, and wrote `marks` down.
    ///
    /// Its own call rather than a `Trail` built by hand at every call
    /// site, because "which cells does a chunk cover" is arithmetic that
    /// would then live in eight places and be wrong in one of them.
    #[cfg(test)]
    pub fn pretend_walked(&mut self, marks: Vec<Mark>) {
        let per_chunk = CHUNK_SIZE_X as i32 / primitive_shared::trail::CELL;
        let cells: Vec<(i32, i32)> = self
            .tiles
            .keys()
            .flat_map(|pos| {
                (0..per_chunk).flat_map(move |dz| {
                    (0..per_chunk).map(move |dx| (pos.x * per_chunk + dx, pos.z * per_chunk + dz))
                })
            })
            .collect();
        self.trail_arrived(cells, marks, false);
    }

    /// Marks this client has found gone, for the caller to tell the
    /// server about. Drained, so one lost cairn is reported once.
    pub fn take_lost_marks(&mut self) -> Vec<(i32, i32, i32)> {
        std::mem::take(&mut self.lost)
    }

    /// Rubs out every mark in `pos` whose cell `standing` says is no
    /// longer a cairn or a blaze, and remembers to tell the server.
    ///
    /// **Both, and in that order.** The map stops drawing the mark on this
    /// frame, because the player is standing in front of the heap that is
    /// not there and a mark that waited for a round trip would read as a
    /// map that lies; the server is told so that the *next* session agrees,
    /// since the profile is where marks actually live.
    fn forget_fallen(&mut self, pos: ChunkPos, standing: impl Fn((i32, i32, i32)) -> bool) {
        let gone: Vec<(i32, i32, i32)> = self
            .trail
            .marks()
            .iter()
            .map(|mark| mark.at)
            .filter(|&at| ChunkPos::from_global(at.0, at.2).0 == pos && !standing(at))
            .collect();
        for at in gone {
            self.trail.forget_mark(at);
            self.lost.push(at);
            self.revision = self.revision.wrapping_add(1);
        }
    }

    /// How many chunks have been seen.
    pub fn surveyed(&self) -> usize {
        self.tiles.len()
    }

    /// How many are waiting to be looked at.
    #[cfg(test)]
    pub fn pending(&self) -> usize {
        self.queue.len()
    }

    pub fn revision(&self) -> u64 {
        self.revision
    }

    /// A chunk arrived and is worth looking at.
    pub fn note_chunk(&mut self, pos: ChunkPos) {
        if self.queued.insert(pos) {
            self.queue.push_back(pos);
        }
    }

    /// Something changed at a column.
    pub fn note_edit(&mut self, gx: i32, gz: i32) {
        self.note_chunk(ChunkPos::from_global(gx, gz).0);
    }

    /// Surveys what is waiting, until `budget` is spent. Answers how many
    /// chunks were surveyed.
    ///
    /// A chunk that is no longer loaded when its turn comes is dropped
    /// from the queue rather than kept: it was seen, and if it is seen
    /// again it will be queued again.
    pub fn catch_up(&mut self, chunks: &ChunkManager, budget: Duration) -> usize {
        let started = Instant::now();
        let mut surveyed = 0;
        while let Some(pos) = self.queue.pop_front() {
            self.queued.remove(&pos);
            if let Some(chunk) = chunks.get(pos) {
                // The marks already known here are asked about at their
                // own cells: a cairn that is no longer on top of its
                // column -- roofed over, buried in snow -- is still a
                // cairn, and its name is still the player's. One that is
                // not there at all is reported and rubbed off.
                self.forget_fallen(pos, |(x, y, z)| {
                    let (_, lx, lz) = ChunkPos::from_global(x, z);
                    (0..CHUNK_SIZE_Y as i32).contains(&y)
                        && matches!(block_kind(chunk.get(lx, y as usize, lz)), BLOCK_CAIRN | BLOCK_BLAZE)
                });
                self.insert(pos, survey(chunk));
                surveyed += 1;
            }
            if started.elapsed() >= budget {
                break;
            }
        }
        surveyed
    }

    /// Files a tile. A tile identical to the one already there changes
    /// nothing, so a chunk streamed again on the way back past it does not
    /// rebuild an open map or dirty the file.
    pub fn insert(&mut self, pos: ChunkPos, tile: Tile) {
        if self.tiles.get(&pos).is_some_and(|old| **old == tile) {
            return;
        }
        self.tiles.insert(pos, Box::new(tile));
        self.revision = self.revision.wrapping_add(1);
        self.unsaved = true;
    }

    /// What the top of a column is and how high, if this player has been
    /// to it **and** this client has surveyed it.
    ///
    /// The trail comes first and it is the one that decides: a column
    /// nobody walked to is off the paper however many times its chunk was
    /// streamed. See the two halves at the top of this file.
    pub fn at(&self, gx: i32, gz: i32) -> Option<(Ground, u8)> {
        if !self.trail.knows(gx, gz) {
            return None;
        }
        let (pos, lx, lz) = ChunkPos::from_global(gx, gz);
        let (ground, height) = self.tiles.get(&pos)?.at(lx, lz);
        (ground != Ground::Unseen).then_some((ground, height))
    }

    /// Writes the map where it was opened from, if anything changed.
    ///
    /// A temporary and a rename, like every other save in this game, so a
    /// crash half way through cannot leave half a file where the map was.
    pub fn save(&mut self) -> std::io::Result<bool> {
        let Some(path) = self.path.clone() else {
            return Ok(false);
        };
        if !self.unsaved {
            return Ok(false);
        }
        if let Some(dir) = path.parent().filter(|dir| !dir.as_os_str().is_empty()) {
            std::fs::create_dir_all(dir)?;
        }
        let tmp = path.with_extension("map.tmp");
        std::fs::write(&tmp, self.to_bytes())?;
        std::fs::rename(&tmp, &path)?;
        self.unsaved = false;
        Ok(true)
    }

    /// The file's bytes.
    ///
    /// Written by hand rather than through bincode, and little-endian by
    /// hand: it is five megabytes of two fixed-size arrays, the layout is
    /// the whole format, and a serialiser that one day writes lengths
    /// differently would turn every player's map into noise.
    pub fn to_bytes(&self) -> Vec<u8> {
        let mut positions: Vec<&ChunkPos> = self.tiles.keys().collect();
        // Stable bytes for the same map, so two saves of one walk compare
        // equal.
        positions.sort_by_key(|pos| (pos.x, pos.z));
        let mut bytes = Vec::with_capacity(12 + positions.len() * RECORD);
        bytes.extend_from_slice(MAGIC);
        bytes.extend_from_slice(&FORMAT.to_le_bytes());
        bytes.extend_from_slice(&(positions.len() as u32).to_le_bytes());
        for pos in positions {
            let tile = &self.tiles[pos];
            bytes.extend_from_slice(&pos.x.to_le_bytes());
            bytes.extend_from_slice(&pos.z.to_le_bytes());
            bytes.extend_from_slice(&tile.ground);
            bytes.extend_from_slice(&tile.height);
        }
        // ...and nothing after the tiles: what this file is for is the
        // picture. See `FORMAT` for where the marks went.
        bytes
    }

    /// Reads a file's bytes back, or `None` if they are not a map.
    ///
    /// A count that disagrees with the length is refused outright rather
    /// than read as far as it goes: a truncated file is a file whose last
    /// tile would be half somebody's lake and half zeros.
    fn from_bytes(bytes: &[u8]) -> Option<HashMap<ChunkPos, Box<Tile>>> {
        if bytes.len() < 12 || &bytes[..4] != MAGIC {
            return None;
        }
        let word = |at: usize| -> Option<[u8; 4]> { bytes.get(at..at + 4)?.try_into().ok() };
        let format = u32::from_le_bytes(word(4)?);
        if !(1..=FORMAT).contains(&format) {
            return None;
        }
        let count = u32::from_le_bytes(word(8)?) as usize;
        let tiles_end = 12 + count.checked_mul(RECORD)?;
        // Formats 1 and 3 are the tiles and nothing else, to the byte; a
        // 2 has its marks after them, and they are read past rather than
        // read: they are the server's now, and it will say.
        if (format != 2 && bytes.len() != tiles_end) || bytes.len() < tiles_end {
            return None;
        }
        let mut tiles = HashMap::with_capacity(count);
        for record in bytes[12..tiles_end].chunks_exact(RECORD) {
            let x = i32::from_le_bytes(record[0..4].try_into().ok()?);
            let z = i32::from_le_bytes(record[4..8].try_into().ok()?);
            let mut tile = Tile::blank();
            tile.ground.copy_from_slice(&record[8..8 + COLUMNS]);
            tile.height.copy_from_slice(&record[8 + COLUMNS..]);
            tiles.insert(ChunkPos::new(x, z), Box::new(tile));
        }
        Some(tiles)
    }
}

/// Where a singleplayer world keeps its map: beside the world, so
/// deleting the world deletes the map and copying it copies the map.
pub fn cache_for_world(directory: &Path) -> PathBuf {
    directory.join("explored.map")
}

/// Where the map of a world on somebody else's server is kept.
///
/// Filed by address, seed **and** name: a server that is reset with a new
/// seed is a new world with an old address, and two people sharing one
/// computer have walked two different maps. Everything that is not a
/// letter, a digit or a dash becomes an underscore, because an address is
/// `host:port` and a colon is not a thing a Windows file name may hold.
pub fn cache_for_server(address: &str, seed: u32, username: &str) -> PathBuf {
    let tidy = |text: &str| -> String {
        text.chars()
            .map(|c| if c.is_ascii_alphanumeric() || c == '-' { c.to_ascii_lowercase() } else { '_' })
            .collect()
    };
    PathBuf::from("maps").join(format!("{}-{seed}-{}.map", tidy(address), tidy(username)))
}

#[cfg(test)]
mod tests {
    use super::*;
    use primitive_shared::trail::MarkKind;
    use primitive_shared::types::{
        Chunk, BLOCK_AIR, BLOCK_LEAVES, BLOCK_LOG, BLOCK_PLANKS, BLOCK_TALL_GRASS, BLOCK_WATER,
        CHUNK_SIZE_Y, CHUNK_VOLUME,
    };

    fn empty_chunk() -> Chunk {
        Chunk {
            pos: ChunkPos::new(0, 0),
            blocks: vec![BLOCK_AIR; CHUNK_VOLUME],
        }
    }

    /// A chunk of stone up to `ground`, with whatever the test puts on it.
    fn chunk_with(ground: usize, top: impl Fn(usize, usize) -> Vec<BlockId>) -> PackedChunk {
        let mut chunk = empty_chunk();
        for lz in 0..SIDE {
            for lx in 0..SIDE {
                for y in 0..ground {
                    chunk.set(lx, y, lz, BLOCK_STONE);
                }
                for (above, block) in top(lx, lz).into_iter().enumerate() {
                    chunk.set(lx, ground + above, lz, block);
                }
            }
        }
        PackedChunk::pack(&chunk)
    }

    #[test]
    fn the_map_colours_a_column_by_what_is_on_top_of_it() {
        let chunk = chunk_with(60, |lx, _| match lx {
            0 => vec![BLOCK_GRASS],
            1 => vec![BLOCK_SAND, BLOCK_WATER, BLOCK_WATER],
            2 => vec![BLOCK_DIRT, BLOCK_LOG, BLOCK_LOG, BLOCK_LEAVES],
            3 => vec![BLOCK_PLANKS],
            _ => vec![],
        });
        let tile = survey(&chunk);
        assert_eq!(tile.at(0, 5), (Ground::Grass, 60));
        assert_eq!(tile.at(1, 5), (Ground::Water, 62), "a lake was drawn as its bed");
        assert_eq!(tile.at(2, 5), (Ground::Forest, 63), "a tree was drawn as the soil under it");
        assert_eq!(tile.at(3, 5), (Ground::Built, 60));
        assert_eq!(tile.at(4, 5), (Ground::Rock, 59));
    }

    #[test]
    fn a_tuft_of_grass_is_seen_through_to_the_meadow_under_it() {
        let chunk = chunk_with(40, |_, _| vec![BLOCK_GRASS, BLOCK_TALL_GRASS]);
        assert_eq!(survey(&chunk).at(7, 7), (Ground::Grass, 40));
    }

    #[test]
    fn land_nobody_has_been_near_is_blank() {
        let mut map = ExploredMap::new();
        assert_eq!(map.at(0, 0), None);
        map.insert(ChunkPos::new(0, 0), Tile::uniform(Ground::Grass, 64));
        map.pretend_walked(Vec::new());
        assert_eq!(map.at(15, 15), Some((Ground::Grass, 64)));
        assert_eq!(map.at(16, 0), None, "the next chunk over was drawn without being seen");
        assert_eq!(map.at(-1, 0), None, "the chunk to the west was drawn without being seen");
    }

    #[test]
    fn a_column_with_nothing_to_land_on_stays_unseen() {
        let chunk = PackedChunk::pack(&empty_chunk());
        let mut map = ExploredMap::new();
        map.insert(ChunkPos::new(0, 0), survey(&chunk));
        assert_eq!(map.at(3, 3), None);
    }

    #[test]
    fn every_block_in_the_game_is_either_seen_through_or_given_a_ground() {
        // `Unseen` is what the map draws for "never streamed", and a block
        // that classified as it would punch a hole in the land.
        for &(id, name) in primitive_shared::types::ALL_BLOCK_IDS {
            assert_ne!(Ground::of(id), Some(Ground::Unseen), "{name} is drawn as a hole");
        }
    }

    #[test]
    fn the_cache_comes_back_from_disk_exactly_as_it_went() {
        let dir = std::env::temp_dir().join(format!("primitive-map-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let path = dir.join("explored.map");

        let mut map = ExploredMap::open(path.clone());
        let mut tile = Tile::uniform(Ground::Forest, 70);
        tile.set(3, 9, Ground::Water, 61);
        map.insert(ChunkPos::new(-4, 12), tile);
        map.insert(ChunkPos::new(7, -1), Tile::uniform(Ground::Snow, 140));
        assert!(map.save().expect("save"), "a changed map was not written");
        assert!(!map.save().expect("save"), "an unchanged map was written again");

        let mut back = ExploredMap::open(path);
        // The walk is the server's and is not in this file; the tiles are.
        back.pretend_walked(Vec::new());
        assert_eq!(back.surveyed(), 2);
        assert_eq!(back.at(-4 * 16 + 3, 12 * 16 + 9), Some((Ground::Water, 61)));
        assert_eq!(back.at(-4 * 16, 12 * 16), Some((Ground::Forest, 70)));
        assert_eq!(back.at(7 * 16 + 1, -1), Some((Ground::Snow, 140)));
        assert_eq!(back.to_bytes(), map.to_bytes());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_cairn_stands_on_the_ground_it_is_drawn_over() {
        // A cairn is a mark on the paper, not a patch of building: the
        // survey walks past it to the meadow underneath, so a line of them
        // does not read as a row of huts.
        let chunk = chunk_with(60, |lx, lz| match (lx, lz) {
            (5, 9) => vec![BLOCK_GRASS, BLOCK_CAIRN],
            (6, 9) => vec![BLOCK_GRASS, BLOCK_BLAZE],
            _ => vec![BLOCK_GRASS],
        });
        let tile = survey(&chunk);
        assert_eq!(tile.at(5, 9), (Ground::Grass, 60), "the cairn hid the meadow it stands on");
        assert_eq!(tile.at(6, 9), (Ground::Grass, 60), "the blaze hid the meadow it stands on");
    }

    #[test]
    fn a_map_file_from_before_the_marks_moved_to_the_server_still_opens() {
        // Its tiles are what the file is for, and they are the thing that
        // took hours to make. See `FORMAT`.
        let mut map = ExploredMap::new();
        map.insert(ChunkPos::new(1, 2), Tile::uniform(Ground::Grass, 64));
        let tiles_only = map.to_bytes();
        for (format, trailer) in [(1u32, Vec::new()), (2, 0u32.to_le_bytes().to_vec())] {
            let mut bytes = tiles_only.clone();
            bytes[4..8].copy_from_slice(&format.to_le_bytes());
            bytes.extend_from_slice(&trailer);
            let tiles = ExploredMap::from_bytes(&bytes).unwrap_or_else(|| panic!("a format {format} map was refused"));
            assert_eq!(tiles.len(), 1);
        }
    }

    #[test]
    fn a_mark_whose_cairn_is_gone_is_reported_once_and_a_standing_one_is_not() {
        let mut map = ExploredMap::new();
        map.insert(ChunkPos::new(0, 0), Tile::uniform(Ground::Grass, 64));
        map.pretend_walked(vec![
            Mark { at: (1, 65, 1), kind: MarkKind::Cairn, name: String::new() },
            Mark { at: (8, 65, 8), kind: MarkKind::Cairn, name: "home".into() },
            Mark { at: (40, 70, 40), kind: MarkKind::Blaze, name: "far".into() },
        ]);
        map.forget_fallen(ChunkPos::new(0, 0), |at| at == (8, 65, 8));
        assert_eq!(map.take_lost_marks(), vec![(1, 65, 1)], "the standing cairn was reported gone");
        assert_eq!(map.mark_name((1, 65, 1)), None, "the map went on drawing a heap that is not there");
        assert_eq!(map.mark_name((8, 65, 8)), Some("home"), "the standing cairn lost its name");
        // Drained, and rubbed out: a cairn taken apart is told to the
        // server once, not once a frame for the rest of the session.
        map.forget_fallen(ChunkPos::new(0, 0), |at| at == (8, 65, 8));
        assert!(map.take_lost_marks().is_empty());
        // A mark in another chunk is not this chunk's to look at.
        map.forget_fallen(ChunkPos::new(0, 0), |_| false);
        assert_eq!(map.take_lost_marks(), vec![(8, 65, 8)]);
        assert_eq!(map.mark_name((40, 70, 40)), Some("far"), "a far mark was rubbed out from here");
    }

    #[test]
    fn the_map_draws_only_what_the_player_walked_to() {
        // The whole of what the trail is for: a chunk this client was sent
        // is not a chunk this player has been to.
        let mut map = ExploredMap::new();
        map.insert(ChunkPos::new(0, 0), Tile::uniform(Ground::Grass, 64));
        map.insert(ChunkPos::new(4, 0), Tile::uniform(Ground::Sand, 64));
        assert_eq!(map.at(8, 8), None, "a streamed chunk was drawn before anybody walked there");
        map.trail_arrived(vec![primitive_shared::trail::cell_of(8, 8)], Vec::new(), true);
        assert_eq!(map.at(8, 8), Some((Ground::Grass, 64)));
        assert_eq!(map.at(4 * 16, 0), None, "the far chunk came with the near one");
    }

    #[test]
    fn a_damaged_file_is_a_blank_map_rather_than_a_world_that_will_not_open() {
        let mut bytes = {
            let mut map = ExploredMap::new();
            map.insert(ChunkPos::new(1, 1), Tile::uniform(Ground::Sand, 64));
            map.to_bytes()
        };
        bytes.truncate(bytes.len() - 7);
        assert!(ExploredMap::from_bytes(&bytes).is_none(), "a truncated map was read");
        assert!(ExploredMap::from_bytes(b"not a map at all").is_none());
    }

    #[test]
    fn seeing_a_chunk_again_unchanged_rebuilds_nothing() {
        let mut map = ExploredMap::new();
        map.insert(ChunkPos::new(0, 0), Tile::uniform(Ground::Grass, 64));
        let revision = map.revision();
        map.insert(ChunkPos::new(0, 0), Tile::uniform(Ground::Grass, 64));
        assert_eq!(map.revision(), revision);
    }

    #[test]
    fn a_chunk_queued_twice_is_surveyed_once() {
        let mut map = ExploredMap::new();
        map.note_chunk(ChunkPos::new(2, 2));
        map.note_edit(2 * 16 + 5, 2 * 16 + 5);
        assert_eq!(map.pending(), 1);
    }

    #[test]
    fn a_server_map_is_filed_somewhere_a_file_name_can_be() {
        let path = cache_for_server("Play.Example.org:25565", 42, "Кто/то");
        let name = path.file_name().and_then(|n| n.to_str()).expect("a name");
        assert!(name.chars().all(|c| c.is_ascii_alphanumeric() || "-_.".contains(c)), "{name}");
        assert_ne!(
            cache_for_server("host:1", 1, "a"),
            cache_for_server("host:1", 2, "a"),
            "a reset server shared its old map"
        );
    }

    #[test]
    fn a_survey_is_cheap_enough_to_run_during_streaming() {
        // **The measurement the budget rests on.** Real terrain: the
        // generator's own chunks, a patch of them, surveyed and timed.
        // Printed rather than asserted tightly, because a debug build is
        // ten times a release one; the bound is generous enough to hold in
        // debug and to fail if a survey ever becomes a full scan of the
        // chunk's volume.
        let generator = primitive_shared::worldgen::WorldGen::with_preset(
            4242,
            primitive_shared::worldgen::Preset::Normal,
        );
        let chunks: Vec<PackedChunk> = (0..4)
            .flat_map(|x| (0..4).map(move |z| ChunkPos::new(x, z)))
            .map(|pos| PackedChunk::pack(&generator.generate_chunk(pos)))
            .collect();
        let started = Instant::now();
        let mut columns = 0;
        for chunk in &chunks {
            let tile = survey(chunk);
            columns += tile.ground.iter().filter(|&&g| g != 0).count();
        }
        let per_chunk = started.elapsed() / chunks.len() as u32;
        println!(
            "[map] survey: {:?} per chunk over {} chunks ({} columns seen, world {} tall)",
            per_chunk,
            chunks.len(),
            columns,
            CHUNK_SIZE_Y
        );
        assert!(columns > 0, "real terrain surveyed as nothing");
        assert!(per_chunk < Duration::from_millis(20), "a survey took {per_chunk:?}");
    }
}
