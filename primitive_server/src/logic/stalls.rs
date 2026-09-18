//! Who owns each barter stall, and what it asks.
//!
//! The goods on a stall are not here: they are in the container store at
//! the stall's cell, like a chest's (see `primitive_shared::stall` for why).
//! This is the part a chest does not have -- a name and three prices -- and
//! it is kept beside the store rather than inside it for the reason the
//! store is kept beside the world: `chests.bin` is a format every container
//! in every world is written in, and a stall's owner is a fact about one
//! kind of block.
//!
//! ## Its own file
//!
//! `stalls.bin`, versioned on its own. A world saved before stalls existed
//! has none, which reads as "no stalls", which is right. A file this build
//! cannot read is refused rather than read as empty: an empty reading is
//! every stall in the world without an owner, and an ownerless stall is
//! claimed by the first player to open it (`crate::open_chest`) -- a
//! corrupt file must not be how a shop changes hands.

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use primitive_shared::stall::{Offer, OFFERS};

const SAVE_FORMAT_VERSION: u32 = 1;

/// Where a stall is, in global block coordinates.
pub type StallPos = (i32, i32, i32);

/// One stall's owner and prices.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Stall {
    /// The owner's player name -- the identity a profile is filed under
    /// (`profiles::Profiles::restore`), so it is who they are across
    /// sessions and not a connection number.
    pub owner: String,
    /// `stall::OFFERS` long, a row each; `None` is a row with no price.
    pub offers: Vec<Option<Offer>>,
}

impl Stall {
    pub fn new(owner: &str) -> Self {
        Self { owner: owner.to_string(), offers: vec![None; OFFERS] }
    }

    /// Row `row`'s price, if it has one.
    pub fn offer(&self, row: usize) -> Option<Offer> {
        self.offers.get(row).copied().flatten()
    }

    /// Back to the shape this build draws: `OFFERS` rows, each a price the
    /// game can have. The file is on a disk an operator can edit.
    fn sanitize(&mut self) {
        self.offers.resize(OFFERS, None);
        for offer in &mut self.offers {
            if offer.is_some_and(|o| !o.is_valid()) {
                *offer = None;
            }
        }
    }
}

#[derive(Serialize, Deserialize)]
struct SaveFile {
    version: u32,
    stalls: Vec<(StallPos, Stall)>,
}

#[derive(Default)]
pub struct Stalls {
    stalls: HashMap<StallPos, Stall>,
    dirty: bool,
}

impl Stalls {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn is_dirty(&self) -> bool {
        self.dirty
    }

    pub fn get(&self, at: StallPos) -> Option<&Stall> {
        self.stalls.get(&at)
    }

    /// Whether `name` owns the stall at `at`. A stall nobody owns is owned
    /// by nobody, not by everybody.
    pub fn is_owner(&self, at: StallPos, name: &str) -> bool {
        self.stalls.get(&at).is_some_and(|stall| stall.owner == name)
    }

    /// A stall put down by `owner`, with no prices yet. Whatever was
    /// recorded at the cell before is forgotten: it belonged to a stall that
    /// is not there any more (see `forget` for how one can be left behind).
    pub fn place(&mut self, at: StallPos, owner: &str) {
        self.stalls.insert(at, Stall::new(owner));
        self.dirty = true;
    }

    /// Gives an ownerless stall to `owner`, and answers whether it did.
    pub fn claim(&mut self, at: StallPos, owner: &str) -> bool {
        if self.stalls.contains_key(&at) {
            return false;
        }
        self.place(at, owner);
        true
    }

    /// Sets row `row`'s price. False for a row the stall does not have, a
    /// price the game cannot have, or a stall that is not recorded.
    pub fn set_offer(&mut self, at: StallPos, row: usize, offer: Option<Offer>) -> bool {
        if offer.is_some_and(|o| !o.is_valid()) {
            return false;
        }
        let Some(slot) = self.stalls.get_mut(&at).and_then(|stall| stall.offers.get_mut(row)) else {
            return false;
        };
        *slot = offer;
        self.dirty = true;
        true
    }

    /// The stall is gone. Called by every path that takes one away -- the
    /// player's break, a mod, the floor dug out -- through the container
    /// spill, so that a record cannot outlive its block. One that does
    /// anyway (a cell written over by something that spills nothing) is
    /// harmless: `place` replaces it when a stall goes there again.
    pub fn forget(&mut self, at: StallPos) -> Option<Stall> {
        let gone = self.stalls.remove(&at);
        if gone.is_some() {
            self.dirty = true;
        }
        gone
    }

    fn save_path(dir: &Path) -> PathBuf {
        dir.join("stalls.bin")
    }

    /// Writes them out, atomically: a temp file and a rename.
    pub fn save(&mut self, dir: &Path) -> std::io::Result<usize> {
        std::fs::create_dir_all(dir)?;
        let mut stalls: Vec<(StallPos, Stall)> = self.stalls.iter().map(|(&at, s)| (at, s.clone())).collect();
        // Stable bytes for an unchanged world.
        stalls.sort_by_key(|&(at, _)| at);
        let count = stalls.len();
        let bytes = bincode::serialize(&SaveFile { version: SAVE_FORMAT_VERSION, stalls })
            .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))?;
        let final_path = Self::save_path(dir);
        let tmp_path = final_path.with_extension("bin.tmp");
        std::fs::write(&tmp_path, &bytes)?;
        std::fs::rename(&tmp_path, &final_path)?;
        self.dirty = false;
        Ok(count)
    }

    /// Reads them back. A missing file is a world with no stalls; a file
    /// this build cannot read is an error (see the module note).
    pub fn load(&mut self, dir: &Path) -> std::io::Result<usize> {
        let bytes = match std::fs::read(Self::save_path(dir)) {
            Ok(bytes) => bytes,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(0),
            Err(e) => return Err(e),
        };
        let bad = |e| std::io::Error::new(std::io::ErrorKind::InvalidData, e);
        let version: u32 = bincode::deserialize(&bytes).map_err(bad)?;
        if version != SAVE_FORMAT_VERSION {
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                format!("stall save is format v{version}, this server speaks v{SAVE_FORMAT_VERSION}"),
            ));
        }
        let save: SaveFile = bincode::deserialize(&bytes).map_err(bad)?;
        self.stalls.clear();
        for (at, mut stall) in save.stalls {
            stall.sanitize();
            self.stalls.insert(at, stall);
        }
        self.dirty = false;
        Ok(self.stalls.len())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use primitive_shared::types::{BLOCK_FLINT, BLOCK_HIDE};

    const AT: StallPos = (7, 30, -9);
    const FLINT_FOR_HIDE: Offer = Offer { give: BLOCK_FLINT, give_count: 4, take: BLOCK_HIDE, take_count: 1 };

    fn scratch(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("primitive_stalls_{}_{name}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        dir
    }

    #[test]
    fn only_the_player_who_put_it_down_owns_it() {
        let mut stalls = Stalls::new();
        stalls.place(AT, "ada");
        assert!(stalls.is_owner(AT, "ada"));
        assert!(!stalls.is_owner(AT, "bob"));
        assert!(!stalls.claim(AT, "bob"), "somebody claimed a stall that already had an owner");
        assert!(!stalls.is_owner((0, 0, 0), "ada"), "a stall nobody put down belongs to somebody");
    }

    #[test]
    fn a_price_the_game_cannot_have_is_not_set() {
        let mut stalls = Stalls::new();
        stalls.place(AT, "ada");
        assert!(!stalls.set_offer(AT, 0, Some(Offer { take_count: 0, ..FLINT_FOR_HIDE })));
        assert!(!stalls.set_offer(AT, OFFERS, Some(FLINT_FOR_HIDE)), "a row the stall does not have took a price");
        assert!(stalls.set_offer(AT, 2, Some(FLINT_FOR_HIDE)));
        assert_eq!(stalls.get(AT).unwrap().offer(2), Some(FLINT_FOR_HIDE));
    }

    #[test]
    fn owners_and_prices_survive_a_round_trip_through_a_file() {
        let dir = scratch("roundtrip");
        let mut stalls = Stalls::new();
        stalls.place(AT, "ada");
        stalls.set_offer(AT, 1, Some(FLINT_FOR_HIDE));
        assert_eq!(stalls.save(&dir).expect("save"), 1);
        assert!(!stalls.is_dirty());

        let mut back = Stalls::new();
        assert_eq!(back.load(&dir).expect("load"), 1);
        assert_eq!(back.get(AT), stalls.get(AT));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_stall_file_this_build_cannot_read_is_refused_rather_than_read_as_ownerless() {
        let dir = scratch("bad");
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(Stalls::save_path(&dir), b"no").unwrap();
        assert!(Stalls::new().load(&dir).is_err());
        let _ = std::fs::remove_dir_all(&dir);
    }
}
