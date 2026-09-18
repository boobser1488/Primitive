//! What is inside the chests.
//!
//! ## Why this is not in the world overlay
//!
//! The world stores blocks: a cell is one `u16`, and an edited chunk is
//! a sparse map of those. A chest is the first block whose *contents*
//! matter, and they do not fit in a `u16` -- so they live here, keyed by
//! the position of the block they belong to, and the world goes on
//! storing nothing but ids.
//!
//! That split is worth keeping even though it means two maps to save:
//! the world's overlay is what makes an evicted chunk regenerable, and
//! putting a variable-length payload inside it would make every chunk
//! read touch a structure that is only ever interesting for a handful of
//! cells.
//!
//! ## Why its own file
//!
//! `edits.bin` is versioned, and a version bump makes the server refuse
//! to load an older world -- which for a player means every block they
//! ever placed vanishing. Chests go in `chests.bin` beside it: a world
//! saved before chests existed simply has no such file, which reads as
//! "no chests", which is exactly right. Nothing has to be migrated and
//! nothing can be lost.
//!
//! ## Empty chests are not stored
//!
//! An entry appears when something is put in and disappears when the
//! last thing comes out. A world where every chest ever placed is a
//! permanent map entry is a world whose save grows with construction
//! rather than with contents -- and "is there a chest here" is a
//! question the *block* already answers.

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use primitive_shared::inventory::Inventory;

/// Its own version, independent of the world's. See the note above.
///
/// **v2 is the wear on a tool.** A slot went from two numbers to three
/// (see `inventory::Stack::damage`), and bincode writes fields by
/// position with no names and no defaults -- so a v1 file read as a v2
/// one is not a file with a missing field, it is a file that decodes
/// into nonsense. Hence the version, and hence `ChestsV1` below, which
/// is what the old shape looked like.
const SAVE_FORMAT_VERSION: u32 = 2;

/// Where a chest is, in global block coordinates.
pub type ChestPos = (i32, i32, i32);

#[derive(Serialize, Deserialize)]
struct SaveFile {
    version: u32,
    chests: Vec<(ChestPos, Inventory)>,
}

// ---- what a chest file written before tools wore out looks like ----
//
// A minimal copy of the old shape, kept here rather than anywhere more
// general because this is the only thing that reads it. It exists for
// one reason: **losing a chest is losing everything a player owns**, so
// a world saved by the last build has to open in this one.

#[derive(Deserialize)]
struct StackV1 {
    block: primitive_shared::types::BlockId,
    count: u32,
}

#[derive(Deserialize)]
struct InventoryV1 {
    slots: Vec<Option<StackV1>>,
}

#[derive(Deserialize)]
struct SaveFileV1 {
    /// Read past rather than used: the version has already been decoded
    /// on its own to decide which shape to read, and bincode has to be
    /// told about every field in the order they were written.
    #[allow(dead_code)]
    version: u32,
    chests: Vec<(ChestPos, InventoryV1)>,
}

impl From<InventoryV1> for Inventory {
    fn from(old: InventoryV1) -> Self {
        let mut inventory = Inventory::chest();
        for (slot, held) in old.slots.into_iter().enumerate() {
            if let Some(stack) = held {
                // Unworn: a tool out of an older save has had a hard
                // life and no record of it, and a fresh one is the
                // generous reading.
                inventory.put_in_slot(
                    slot,
                    primitive_shared::inventory::Stack::new(stack.block, stack.count),
                );
            }
        }
        inventory
    }
}

#[derive(Default)]
pub struct Chests {
    contents: HashMap<ChestPos, Inventory>,
    /// Set whenever something changes, cleared by a save. The autosave
    /// asks before writing, so a world nobody is storing anything in
    /// costs no disk at all.
    dirty: bool,
}

impl Chests {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn len(&self) -> usize {
        self.contents.len()
    }

    #[allow(dead_code)] // companion to len(), used by tests
    pub fn is_empty(&self) -> bool {
        self.contents.is_empty()
    }

    pub fn is_dirty(&self) -> bool {
        self.dirty
    }

    /// What is in the chest at `at`. An empty chest has no entry, so
    /// this answers with an empty inventory rather than `None` -- the
    /// caller wants something to show the player either way.
    pub fn contents(&self, at: ChestPos) -> Inventory {
        // `Inventory::chest`, not `default`: a box in the world is
        // `CHEST_SLOTS` long and a player's pack is not, and an empty
        // chest that answered with a pack-sized one would be a chest
        // that shrank the first time somebody opened it.
        self.contents.get(&at).cloned().unwrap_or_else(Inventory::chest)
    }

    /// Is there anything at all in the container at `at`?
    ///
    /// Cheaper than `contents`, which clones forty slots, and the
    /// question a periodic pass actually asks -- see `drying`, which
    /// uses it to notice that a frame has been emptied.
    pub fn holds_anything(&self, at: ChestPos) -> bool {
        self.contents.contains_key(&at)
    }

    /// Where every container holding something is.
    ///
    /// A `Vec` rather than an iterator because every caller is a pass
    /// that *changes* what it is walking -- a rack that finishes a skin
    /// edits the store it was listed from -- and the borrow checker is
    /// right about that. There are tens of these, not thousands: an
    /// empty container has no entry at all (see the note at the top).
    pub fn positions(&self) -> Vec<ChestPos> {
        self.contents.keys().copied().collect()
    }

    /// Runs `edit` against the chest's contents and keeps whatever comes
    /// back, dropping the entry if it came back empty.
    ///
    /// Everything that changes a chest goes through here, which is what
    /// makes "an empty chest is not stored" a property of the type
    /// rather than a rule every caller has to remember.
    pub fn edit<T>(&mut self, at: ChestPos, edit: impl FnOnce(&mut Inventory) -> T) -> T {
        let mut inventory = self.contents.remove(&at).unwrap_or_else(Inventory::chest);
        let result = edit(&mut inventory);
        if !inventory.is_empty() {
            self.contents.insert(at, inventory);
        }
        self.dirty = true;
        result
    }

    /// Empties a chest and hands back what was in it -- what breaking
    /// the block spills into the world.
    pub fn take(&mut self, at: ChestPos) -> Option<Inventory> {
        let taken = self.contents.remove(&at);
        if taken.is_some() {
            self.dirty = true;
        }
        taken
    }

    fn save_path(dir: &Path) -> PathBuf {
        dir.join("chests.bin")
    }

    /// Writes the chests out. Atomic, like the world save: a temp file
    /// and a rename, so a crash mid-write cannot leave a truncated one.
    pub fn save(&mut self, dir: &Path) -> std::io::Result<usize> {
        std::fs::create_dir_all(dir)?;
        let mut chests: Vec<(ChestPos, Inventory)> = self
            .contents
            .iter()
            .map(|(&at, inventory)| (at, inventory.clone()))
            .collect();
        // Stable file bytes for the same world, so a save with nothing
        // changed produces an identical file.
        chests.sort_by_key(|&(at, _)| at);
        let count = chests.len();

        let payload = SaveFile {
            version: SAVE_FORMAT_VERSION,
            chests,
        };
        let bytes = bincode::serialize(&payload)
            .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))?;
        let final_path = Self::save_path(dir);
        let tmp_path = final_path.with_extension("bin.tmp");
        std::fs::write(&tmp_path, &bytes)?;
        std::fs::rename(&tmp_path, &final_path)?;
        self.dirty = false;
        Ok(count)
    }

    /// Reads them back. A missing file is not an error: it is a world
    /// from before chests existed, or one nobody has stored anything in.
    ///
    /// A file this build cannot read is refused rather than ignored --
    /// starting up and quietly presenting every chest as empty is how a
    /// player loses everything they own without being told.
    pub fn load(&mut self, dir: &Path) -> std::io::Result<usize> {
        let bytes = match std::fs::read(Self::save_path(dir)) {
            Ok(bytes) => bytes,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(0),
            Err(e) => return Err(e),
        };
        // The version is the first field of both shapes, so it can be
        // read before deciding which shape this is.
        let version: u32 = bincode::deserialize(&bytes)
            .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))?;
        let save: SaveFile = match version {
            SAVE_FORMAT_VERSION => bincode::deserialize(&bytes)
                .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))?,
            1 => {
                let old: SaveFileV1 = bincode::deserialize(&bytes)
                    .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))?;
                SaveFile {
                    version: SAVE_FORMAT_VERSION,
                    chests: old
                        .chests
                        .into_iter()
                        .map(|(at, inventory)| (at, inventory.into()))
                        .collect(),
                }
            }
            other => {
                return Err(std::io::Error::new(
                    std::io::ErrorKind::InvalidData,
                    format!(
                        "chest save is format v{other}, this server speaks v{SAVE_FORMAT_VERSION}"
                    ),
                ))
            }
        };
        self.contents.clear();
        for (at, mut inventory) in save.chests {
            // The file is on a disk an operator can edit, and slot
            // counts change between versions.
            inventory.sanitize();
            if !inventory.is_empty() {
                self.contents.insert(at, inventory);
            }
        }
        self.dirty = false;
        Ok(self.contents.len())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use primitive_shared::types::{BLOCK_DIRT, BLOCK_STONE};

    const AT: ChestPos = (12, 34, -56);

    #[test]
    fn an_untouched_chest_is_empty_and_costs_nothing() {
        let chests = Chests::new();
        assert!(chests.contents(AT).is_empty());
        assert_eq!(chests.len(), 0, "asking about a chest created one");
    }

    #[test]
    fn what_goes_in_comes_back_out() {
        let mut chests = Chests::new();
        chests.edit(AT, |inventory| inventory.add(BLOCK_STONE, 40));
        assert_eq!(chests.contents(AT).count(BLOCK_STONE), 40);
        assert_eq!(chests.len(), 1);
    }

    #[test]
    fn emptying_a_chest_forgets_it() {
        // Or a world's save grows with how much has been *built* rather
        // than with what is in it.
        let mut chests = Chests::new();
        chests.edit(AT, |inventory| inventory.add(BLOCK_STONE, 5));
        assert_eq!(chests.len(), 1);
        chests.edit(AT, |inventory| inventory.take_exact(BLOCK_STONE, 5));
        assert_eq!(chests.len(), 0, "an emptied chest kept its entry");
    }

    #[test]
    fn breaking_one_hands_back_everything_in_it() {
        let mut chests = Chests::new();
        chests.edit(AT, |inventory| {
            inventory.add(BLOCK_STONE, 9);
            inventory.add(BLOCK_DIRT, 3);
        });
        let spilled = chests.take(AT).expect("nothing came back");
        assert_eq!(spilled.count(BLOCK_STONE), 9);
        assert_eq!(spilled.count(BLOCK_DIRT), 3);
        assert_eq!(chests.len(), 0);
        assert!(chests.take(AT).is_none(), "it came back twice");
    }

    #[test]
    fn a_chest_that_saved_a_filled_jug_loads_one_back() {
        use primitive_shared::inventory::{filled_jug, jug_contents};
        use primitive_shared::types::BLOCK_GRAIN;
        // **The format did not have to change for this, and the test is
        // here to say so.** What is in a jug rides in
        // `inventory::Stack::damage` (see `inventory::jug_contents`),
        // which is precisely what `SAVE_FORMAT_VERSION` 2 was bumped
        // for -- so a chest file already carries it and a v2 file
        // written before jugs held anything reads back as jugs holding
        // nothing, which is what it is. Had the contents needed a
        // fourth field on `Stack`, this file would be v3 and every v2
        // chest in the world would decode into nonsense.
        assert_eq!(SAVE_FORMAT_VERSION, 2, "the chest format moved under a jug");

        let dir = std::env::temp_dir().join(format!(
            "primitive_chests_{}_{}",
            std::process::id(),
            "jug"
        ));
        let _ = std::fs::remove_dir_all(&dir);

        let mut chests = Chests::new();
        chests.edit(AT, |inventory| {
            inventory.put_in_slot(0, filled_jug(BLOCK_GRAIN, 12));
        });
        assert_eq!(chests.save(&dir).expect("save"), 1);

        let mut read_back = Chests::new();
        assert_eq!(read_back.load(&dir).expect("load"), 1);
        assert_eq!(
            jug_contents(&read_back.contents(AT).slots()[0].unwrap()),
            Some((BLOCK_GRAIN, 12)),
            "the grain did not survive the disk"
        );

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn chests_survive_a_round_trip_through_a_file() {
        let dir = std::env::temp_dir().join(format!(
            "primitive_chests_{}_{}",
            std::process::id(),
            "roundtrip"
        ));
        let _ = std::fs::remove_dir_all(&dir);

        let mut chests = Chests::new();
        chests.edit(AT, |inventory| inventory.add(BLOCK_STONE, 77));
        chests.edit((0, 0, 0), |inventory| inventory.add(BLOCK_DIRT, 1));
        assert!(chests.is_dirty());
        assert_eq!(chests.save(&dir).expect("save"), 2);
        assert!(!chests.is_dirty(), "saving left it looking unsaved");

        let mut read_back = Chests::new();
        assert_eq!(read_back.load(&dir).expect("load"), 2);
        assert_eq!(read_back.contents(AT).count(BLOCK_STONE), 77);
        assert_eq!(read_back.contents((0, 0, 0)).count(BLOCK_DIRT), 1);

        let _ = std::fs::remove_dir_all(&dir);
    }

    /// **The compartment is squares of the one container, so the format
    /// did not change for it** -- and what could still lose it is a load
    /// that clamps the length (`Inventory::sanitize`, which runs over every
    /// entry here) or an empty-box default that is a chest's forty. A body
    /// that went to disk with its rucksack must come back with it, on the
    /// same square.
    #[test]
    fn a_saved_corpse_loads_with_its_rucksack_compartment() {
        use primitive_shared::inventory::{Stack, CORPSE_COMPARTMENT, CORPSE_SLOTS};
        let dir = std::env::temp_dir().join(format!("primitive_chests_{}_corpse", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);

        let mut chests = Chests::new();
        chests.edit(AT, |inventory| {
            *inventory = Inventory::body(true);
            inventory.put_in_slot(CORPSE_COMPARTMENT.end - 1, Stack::new(BLOCK_STONE, 6));
        });
        chests.save(&dir).expect("save");

        let mut read_back = Chests::new();
        assert_eq!(read_back.load(&dir).expect("load"), 1);
        let body = read_back.contents(AT);
        assert_eq!(body.slots().len(), CORPSE_SLOTS, "the body came back without its rucksack");
        assert_eq!(body.block_in(CORPSE_COMPARTMENT.end - 1), Some(BLOCK_STONE));

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_world_from_before_chests_existed_simply_has_none() {
        // The whole reason this is its own file rather than a field in
        // the world save: no migration, and nothing refused.
        let dir = std::env::temp_dir().join(format!("primitive_chests_{}_none", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).expect("mkdir");

        let mut chests = Chests::new();
        assert_eq!(chests.load(&dir).expect("a missing file is not an error"), 0);
        assert!(chests.is_empty());

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_file_this_build_cannot_read_is_refused_rather_than_ignored() {
        let dir = std::env::temp_dir().join(format!("primitive_chests_{}_bad", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).expect("mkdir");
        std::fs::write(Chests::save_path(&dir), b"not a chest file").expect("write");

        let mut chests = Chests::new();
        assert!(
            chests.load(&dir).is_err(),
            "a corrupt file loaded as an empty world of chests"
        );

        let _ = std::fs::remove_dir_all(&dir);
    }
}
