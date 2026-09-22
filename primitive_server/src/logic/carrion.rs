//! Carrion: what happens to a kill nobody came back for.
//!
//! A carcass is a block (see `primitive_shared::animals`), and until now
//! it was a block that waited forever. A deer shot on the first evening
//! was still lying there a week later, as good as the hour it fell,
//! which quietly made the whole butchering mechanic optional: there was
//! never a reason to do the work *now*.
//!
//! So a carcass spoils. Five steps of the rot clock -- the same clock
//! the meat in a pack ages on, `logic::rot`, and therefore the same
//! promise about what a day is -- and what is left of it is bones, the
//! skin, and a lump of carrion worth eating only by someone who has run
//! out of choices (`animals::Species::spoils_into`).
//!
//! ## Where the age lives
//!
//! In a map here, keyed by cell, saved to `carrion.bin` beside the
//! fires. **Not in the block id**, which is where every other piece of
//! per-cell state in this game has gone, and the reason is that the
//! carcass has already spent its variant field: those three bits are
//! the *stage of butchering*, and a carcass that forgot how far it had
//! been cut in order to remember how old it was would be a worse game
//! than one that never rotted.
//!
//! ## How a carcass is found again
//!
//! The same two ways a fire is (`logic::fire`): the file, and the queue
//! of changed cells that `notify_mechanics` feeds. A carcass this
//! server has never seen -- one laid by an older build, or by a mod --
//! joins the map the first time anything happens in its cell, fresh.
//! That is generous in the player's favour and it is the direction to
//! be wrong in: the alternative is scanning loaded chunks for carcasses
//! four times a day, which is a hundred million cells to find, usually,
//! none.
//!
//! A cell in the map that is no longer a carcass has been butchered,
//! broken or built over, and its entry goes. Nothing here reads the
//! world except through `cached_block`, so a kill in a chunk nobody has
//! loaded keeps -- the same bargain the chests get, and the reason a
//! far camp is not a punishment.
//!
//! ## The other body
//!
//! A dead *player* is here too (`types::BLOCK_CORPSE`), and that is the
//! whole reason a death leaves a body rather than the backpack it used
//! to leave: a bag is an object the world has no opinion about. It is
//! the same map, the same clock, the same frost and the same
//! reconciliation -- the only thing a corpse does differently is what it
//! becomes at the end, which is bones that still hold the half of the
//! kit the ground could not take (`Spoiled::player_body`, and
//! `types::BLOCK_REMAINS` for the argument about which half).
//!
//! Sharing the mechanism rather than writing a second one was not a
//! tidiness decision. The alternative was a `graves.rs` with its own
//! clock, its own file, its own queue and its own frost rule, and four
//! places to change the day a day stops being a day -- and the first
//! thing to drift would have been the frost, so that a kill in the snow
//! kept and a body in the snow did not.

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use primitive_shared::animals::Species;
use primitive_shared::types::BlockId;
use serde::{Deserialize, Serialize};

/// Where a carcass is, in world cells.
pub type CarcassPos = (i32, i32, i32);

/// How many steps of the rot clock a carcass keeps before it is
/// spoiled.
///
/// Five, at four steps a day, is a day and a quarter. Deliberately
/// *longer* than raw meat in a pack (which is a day, `food.rs`): a
/// whole animal in its own skin keeps better than a cut in a bag, and
/// the number has to leave room for a hunt that goes wrong -- you
/// should be able to kill a deer at dusk, sleep through the night you
/// could not survive outside, and still have breakfast.
pub const STEPS_TO_SPOIL: u8 = 5;

/// How many steps of the same clock a player's body keeps everything
/// they owned before it becomes bones.
///
/// **Eight: two whole days, and longer than any animal.** The figure is
/// not about how fast meat goes off, it is about how far away a death
/// can be and still be answerable. A player dies at dusk, four hundred
/// blocks out, with no food and no tools -- the trip back is a night to
/// survive, a morning to eat, and a walk; a day and a quarter
/// (`STEPS_TO_SPOIL`) would have had the answer decided before they
/// could stand up, which is the failure mode this whole mechanic exists
/// to avoid. Two days is long enough that hurrying is a *choice* rather
/// than the only move, and short enough that the choice has a cost.
///
/// A body in the frost keeps, exactly as a carcass does: dying in the
/// mountains in winter buys time, which is the one kindness winter
/// offers in this game and is already true of everything else made of
/// meat.
pub const CORPSE_STEPS_TO_ROT: u8 = 8;

const SAVE_FORMAT_VERSION: u32 = 1;

#[derive(Serialize, Deserialize)]
struct SaveFile {
    version: u32,
    carcasses: Vec<(CarcassPos, u8)>,
}

/// What a carcass turned into, for the caller to write into the world.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Spoiled {
    pub at: CarcassPos,
    /// What to leave on the ground. Already the species' own list --
    /// see `Species::spoils_into`.
    pub leaves: Vec<(BlockId, u32)>,
    /// **What to leave standing**: the animal's own skeleton, in the
    /// cell the carcass was in.
    ///
    /// A carcass used to simply vanish and drop its parts on the grass,
    /// and what a player saw when they came back a day late was two
    /// items lying in a field -- the world tidying up after them rather
    /// than something having happened. See `types::BLOCK_BONES`.
    pub bones: Option<BlockId>,
    /// **This was somebody**, not a deer.
    ///
    /// The caller has one more thing to do for a body than for a kill:
    /// what the corpse was holding is a container keyed by this cell,
    /// and the soft half of it goes with the flesh (`what_the_ground_takes`).
    /// Said as a flag rather than left to be worked out from `bones ==
    /// Some(BLOCK_REMAINS)`, because the day somebody gives a second
    /// thing the same remains is the day a hundred stacks quietly
    /// disappear out of it.
    pub player_body: bool,
}

/// Takes the soft half out of what a corpse was carrying, and answers
/// whether it took anything.
///
/// Run once, at the moment the body becomes bones, and never again --
/// which is why it lives beside the constant that decides *when* rather
/// than in the container store. What goes and what stays is
/// `types::rots_with_a_body`, and the argument for the line it draws is
/// on `types::BLOCK_REMAINS`.
///
/// The slot is emptied where it stands rather than compacted, for the
/// reason the rot pass rewrites food in place: a player who opens their
/// remains should see the gaps where the leather was, not a tidy list
/// that makes them wonder what they are missing.
pub fn what_the_ground_takes(inventory: &mut primitive_shared::inventory::Inventory) -> bool {
    let mut taken = false;
    for slot in 0..inventory.slots().len() {
        let Some(stack) = inventory.slots()[slot] else {
            continue;
        };
        if !primitive_shared::types::rots_with_a_body(stack.block) {
            continue;
        }
        inventory.take_slot(slot);
        taken = true;
    }
    taken
}

/// How old every carcass this server knows about is.
#[derive(Default)]
pub struct Carrion {
    age: HashMap<CarcassPos, u8>,
    /// Cells that changed and have not been looked at yet. Filled by
    /// `on_block_changed`, which may not read the world -- the same
    /// `CellMechanic` contract the fires keep.
    pending: Vec<CarcassPos>,
    dirty: bool,
}

impl Carrion {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn len(&self) -> usize {
        self.age.len()
    }

    pub fn is_empty(&self) -> bool {
        self.age.is_empty()
    }

    pub fn is_dirty(&self) -> bool {
        self.dirty
    }

    /// How far gone the carcass at `at` is, in steps. Zero for one this
    /// has never heard of, which is also what a fresh kill reads as.
    pub fn age_at(&self, at: CarcassPos) -> u8 {
        self.age.get(&at).copied().unwrap_or(0)
    }

    /// A cell changed. Queued; the world is not ours to read here.
    pub fn on_block_changed(&mut self, gx: i32, gy: i32, gz: i32) {
        self.pending.push((gx, gy, gz));
    }

    pub fn pending(&self) -> usize {
        self.pending.len()
    }

    /// Reconciles the queue against the world: new carcasses join at
    /// nothing, and cells that stopped being carcasses drop out.
    ///
    /// Separate from `step` because the two run on different clocks --
    /// this every tick, so a butchered carcass stops being tracked at
    /// once, and the ageing four times a day.
    pub fn reconcile(
        &mut self,
        budget: usize,
        mut block_at: impl FnMut(CarcassPos) -> Option<BlockId>,
    ) {
        let batch: Vec<CarcassPos> = self
            .pending
            .drain(..budget.min(self.pending.len()))
            .collect();
        for at in batch {
            // A cell nobody has loaded says nothing either way. Guessing
            // is how a kill in an evicted chunk gets forgotten by a
            // server that cannot see it.
            let Some(block) = block_at(at) else { continue };
            // A kill or a body -- the one question, asked in one place.
            // See `types::rots_where_it_lies`.
            match (
                primitive_shared::types::rots_where_it_lies(block),
                self.age.contains_key(&at),
            ) {
                (true, false) => {
                    self.age.insert(at, 0);
                    self.dirty = true;
                }
                (false, true) => {
                    self.age.remove(&at);
                    self.dirty = true;
                }
                _ => {}
            }
        }
    }

    /// One step of the rot clock over every carcass. Hands back the
    /// ones that have gone.
    ///
    /// `state_at` answers what is in the cell and how cold it is there:
    /// `None` for a chunk nobody has loaded, which is left alone, and
    /// `Some((block, keeps))` otherwise -- `keeps` being the freezing
    /// test the packs and chests use, so a kill left in the snow keeps
    /// exactly as long as the meat in your bag does. That is the
    /// mechanic: winter is the larder.
    pub fn step(
        &mut self,
        mut state_at: impl FnMut(CarcassPos) -> Option<(BlockId, bool)>,
    ) -> Vec<Spoiled> {
        let mut spoiled = Vec::new();
        let mut gone = Vec::new();
        for (&at, age) in self.age.iter_mut() {
            let Some((block, keeps)) = state_at(at) else {
                continue;
            };
            if !primitive_shared::types::rots_where_it_lies(block) {
                gone.push(at);
                continue;
            }
            if keeps {
                continue;
            }
            *age = age.saturating_add(1);
            self.dirty = true;
            // A player's body keeps longer than any animal, and what it
            // becomes is not an animal's skeleton. Everything before
            // this point -- the clock, the frost, the queue -- is the
            // same for both, which is the point of them sharing a map.
            if primitive_shared::types::block_kind(block)
                == primitive_shared::types::BLOCK_CORPSE
            {
                if *age < CORPSE_STEPS_TO_ROT {
                    continue;
                }
                gone.push(at);
                spoiled.push(Spoiled {
                    at,
                    // Nothing falls on the grass: what the body was
                    // carrying stays *in* it, minus what rotted, and a
                    // heap of drops beside a grave would despawn while
                    // the player was still walking.
                    leaves: Vec::new(),
                    bones: Some(primitive_shared::types::BLOCK_REMAINS),
                    player_body: true,
                });
                continue;
            }
            if *age < STEPS_TO_SPOIL {
                continue;
            }
            gone.push(at);
            let species = Species::of_carcass(block);
            // Only what the knife has not taken yet -- see `spoils_into`.
            let stage = primitive_shared::animals::butchering_stage(block);
            let uncut: Vec<(BlockId, u32)> =
                species.map(|species| species.spoils_into(stage)).unwrap_or_default();
            // The bones are no longer dropped: they *are* the block that
            // stays. Everything else the body was worth still falls on
            // the grass around it.
            let leaves = uncut
                .iter()
                .copied()
                .filter(|&(left, _)| left != primitive_shared::types::BLOCK_BONE)
                .collect();
            // ...and a skeleton only where the bones are still in it. One cut
            // down past them used to stand up again as a frame that breaks
            // into the bones already in the player's pack.
            let bones = species
                .filter(|_| uncut.iter().any(|&(left, _)| left == primitive_shared::types::BLOCK_BONE))
                .map(primitive_shared::types::bones_of);
            spoiled.push(Spoiled {
                at,
                leaves,
                bones,
                player_body: false,
            });
        }
        for at in gone {
            self.age.remove(&at);
            self.dirty = true;
        }
        spoiled
    }

    fn save_path(dir: &Path) -> PathBuf {
        dir.join("carrion.bin")
    }

    /// Writes them out, atomically, the way the fires are written.
    pub fn save(&mut self, dir: &Path) -> std::io::Result<usize> {
        std::fs::create_dir_all(dir)?;
        let mut carcasses: Vec<(CarcassPos, u8)> =
            self.age.iter().map(|(&at, &age)| (at, age)).collect();
        // Stable bytes for the same world, so a save with nothing
        // changed produces an identical file.
        carcasses.sort_by_key(|&(at, _)| at);
        let count = carcasses.len();
        let bytes = bincode::serialize(&SaveFile {
            version: SAVE_FORMAT_VERSION,
            carcasses,
        })
        .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))?;
        let final_path = Self::save_path(dir);
        let tmp_path = final_path.with_extension("bin.tmp");
        std::fs::write(&tmp_path, &bytes)?;
        std::fs::rename(&tmp_path, &final_path)?;
        self.dirty = false;
        Ok(count)
    }

    /// Reads them back. A missing or unreadable file means every
    /// carcass in the world is a fresh one -- the fires' bargain, and
    /// for the fires' reason: losing this is a day's grace, and
    /// refusing to start is a world nobody can play.
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
        self.age.clear();
        for (at, age) in save.carcasses {
            // Clamped to the longest life anything in this map has, and
            // not to the carcass's: the file does not say which cells
            // are bodies (it never needed to -- the world does), so
            // clamping to five would have quietly handed every corpse
            // past its fifth step three free steps at every restart.
            self.age.insert(at, age.min(CORPSE_STEPS_TO_ROT));
        }
        self.dirty = false;
        Ok(self.age.len())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use primitive_shared::animals::carcass_at_stage;
    use primitive_shared::inventory::Stack;
    use primitive_shared::types::{BLOCK_AIR, BLOCK_BONE, BLOCK_HIDE, BLOCK_ROTTEN};

    const AT: CarcassPos = (4, 12, -7);

    fn deer() -> BlockId {
        carcass_at_stage(Species::Deer, 0)
    }

    #[test]
    fn a_carcass_nobody_comes_back_for_is_bones_and_carrion_by_the_second_day() {
        let mut carrion = Carrion::new();
        carrion.on_block_changed(AT.0, AT.1, AT.2);
        carrion.reconcile(16, |_| Some(deer()));
        assert_eq!(carrion.len(), 1, "the kill was not noticed");

        // Four steps is one day, and it is still a deer.
        for step in 1..STEPS_TO_SPOIL {
            let spoiled = carrion.step(|_| Some((deer(), false)));
            assert!(spoiled.is_empty(), "it went at step {step}");
        }
        let spoiled = carrion.step(|_| Some((deer(), false)));
        assert_eq!(spoiled.len(), 1, "it never went");
        assert_eq!(spoiled[0].at, AT);
        // The skin and a lump of carrion on the grass -- and the frame
        // left *standing*, which is the difference between a world that
        // tidies up after a player and one where something happened.
        let left = &spoiled[0].leaves;
        assert!(left.contains(&(BLOCK_HIDE, 2)), "the skin was lost: {left:?}");
        assert!(
            !left.iter().any(|&(block, _)| block == BLOCK_BONE),
            "the bones fell on the grass instead of staying up: {left:?}"
        );
        assert_eq!(
            spoiled[0].bones,
            Some(primitive_shared::types::bones_of(Species::Deer)),
            "no skeleton was left where the deer was"
        );
        assert!(
            left.iter().any(|&(block, _)| block == BLOCK_ROTTEN),
            "nothing spoiled: {left:?}"
        );
        assert!(
            !left.iter().any(|&(block, _)| block
                == primitive_shared::types::BLOCK_RAW_MEAT),
            "a day-and-a-half-old kill gave fresh meat: {left:?}"
        );
        assert!(carrion.is_empty(), "the entry outlived the carcass");
    }

    /// What a knife took is gone, and rot does not bring it back.
    ///
    /// The carcass used to spoil into the whole animal's list whatever
    /// stage it was cut to, so a deer skinned and left lying grew a second
    /// hide by the next day -- and one cut down to the meat stood up again
    /// as a skeleton full of the bones already in the player's pack.
    #[test]
    fn a_carcass_already_cut_does_not_grow_back_what_the_knife_took_as_it_rots() {
        use primitive_shared::types::block_name;
        for (stage, what) in [(1, "skinned"), (4, "cut down to the meat")] {
            let block = carcass_at_stage(Species::Deer, stage);
            let mut carrion = Carrion::new();
            carrion.on_block_changed(AT.0, AT.1, AT.2);
            carrion.reconcile(16, |_| Some(block));
            let mut spoiled = Vec::new();
            for _ in 0..STEPS_TO_SPOIL {
                spoiled = carrion.step(|_| Some((block, false)));
            }
            assert_eq!(spoiled.len(), 1, "{what}: it never went");
            for &(taken, _) in &Species::Deer.butchering()[..stage] {
                if taken == BLOCK_BONE {
                    assert_eq!(spoiled[0].bones, None, "{what}: the bones in the pack stood up again as a skeleton");
                    continue;
                }
                assert!(
                    !spoiled[0].leaves.iter().any(|&(left, _)| left == taken),
                    "{what}: the {} the knife took came back: {:?}",
                    block_name(taken),
                    spoiled[0].leaves
                );
            }
        }
    }

    /// **Frozen meat keeps, and a carcass is meat.** The same test the
    /// packs and the chests get (`rot::keeps`): a kill left in the snow
    /// is a larder, which is the one thing a winter is good for.
    #[test]
    fn a_carcass_in_the_frost_does_not_spoil_at_all() {
        let mut carrion = Carrion::new();
        carrion.on_block_changed(AT.0, AT.1, AT.2);
        carrion.reconcile(16, |_| Some(deer()));
        for _ in 0..STEPS_TO_SPOIL * 4 {
            assert!(carrion.step(|_| Some((deer(), true))).is_empty());
        }
        assert_eq!(carrion.age_at(AT), 0, "the frost let it age");
    }

    /// A cell that stopped being a carcass -- butchered to the last cut,
    /// broken, or built over -- is forgotten, and a cell nobody has
    /// loaded is left exactly as it was.
    #[test]
    fn a_butchered_carcass_is_forgotten_and_an_unloaded_one_is_left_alone() {
        let mut carrion = Carrion::new();
        carrion.on_block_changed(AT.0, AT.1, AT.2);
        carrion.reconcile(16, |_| Some(deer()));

        // Unloaded: no ageing, no forgetting.
        assert!(carrion.step(|_| None).is_empty());
        assert_eq!(carrion.len(), 1, "an unloaded kill was thrown away");
        assert_eq!(carrion.age_at(AT), 0);

        // Taken apart: the entry goes with it, and nothing is left on
        // the ground -- the player has it.
        assert!(carrion.step(|_| Some((BLOCK_AIR, false))).is_empty());
        assert!(carrion.is_empty(), "a butchered carcass is still being aged");
    }

    /// **A player's body outlasts any animal, and then it is bones.**
    ///
    /// The two numbers are the whole mechanic: a deer is gone in a day
    /// and a quarter, and a body holds everything for two days -- long
    /// enough that a death four hundred blocks out is a trip somebody
    /// can choose how to make, rather than a race they have already
    /// lost by the time they stand up.
    #[test]
    fn a_body_holds_everything_for_two_days_and_is_bones_on_the_third() {
        use primitive_shared::types::{BLOCK_CORPSE, BLOCK_REMAINS};
        let mut carrion = Carrion::new();
        carrion.on_block_changed(AT.0, AT.1, AT.2);
        carrion.reconcile(16, |_| Some(BLOCK_CORPSE));
        assert_eq!(carrion.len(), 1, "the body was not noticed");

        // A deer would have been bones five steps in. The body is not.
        for step in 1..CORPSE_STEPS_TO_ROT {
            let spoiled = carrion.step(|_| Some((BLOCK_CORPSE, false)));
            assert!(
                spoiled.is_empty(),
                "the body rotted at step {step}, before its two days were up"
            );
        }
        let spoiled = carrion.step(|_| Some((BLOCK_CORPSE, false)));
        assert_eq!(spoiled.len(), 1, "the body never rotted at all");
        assert_eq!(spoiled[0].at, AT);
        assert!(spoiled[0].player_body, "the caller was not told this was somebody");
        assert_eq!(
            spoiled[0].bones,
            Some(BLOCK_REMAINS),
            "the body left no bones behind"
        );
        // **Nothing is thrown on the grass.** Drops despawn, so a body
        // that spilled would be a body that emptied itself while the
        // owner was walking to it.
        assert!(spoiled[0].leaves.is_empty(), "the body spilled: {:?}", spoiled[0].leaves);
        assert!(carrion.is_empty(), "the bones are still being aged");
    }

    /// The same frost that keeps a kill keeps a body. Dying in the
    /// mountains in winter buys time, which is the one kindness the
    /// winter in this game offers.
    #[test]
    fn a_body_in_the_frost_does_not_rot_either() {
        use primitive_shared::types::BLOCK_CORPSE;
        let mut carrion = Carrion::new();
        carrion.on_block_changed(AT.0, AT.1, AT.2);
        carrion.reconcile(16, |_| Some(BLOCK_CORPSE));
        for _ in 0..CORPSE_STEPS_TO_ROT * 3 {
            assert!(carrion.step(|_| Some((BLOCK_CORPSE, true))).is_empty());
        }
        assert_eq!(carrion.age_at(AT), 0, "the frost let a body rot");
    }

    /// **The bones do not rot a second time.**
    ///
    /// `BLOCK_REMAINS` is deliberately outside `rots_where_it_lies`: a
    /// cell that rotted twice would end as air, and everything still
    /// filed against it -- the iron, the ingots, the tools the whole
    /// mechanic promises are still there -- would be filed against
    /// nothing.
    #[test]
    fn bones_are_the_end_of_it_and_do_not_rot_again() {
        use primitive_shared::types::BLOCK_REMAINS;
        let mut carrion = Carrion::new();
        carrion.on_block_changed(AT.0, AT.1, AT.2);
        carrion.reconcile(16, |_| Some(BLOCK_REMAINS));
        assert!(carrion.is_empty(), "the bones were put on the ageing list");
    }

    /// What the ground takes out of a body, and what it leaves.
    ///
    /// The decision this mechanic exists to create, stated as an
    /// inventory: hurry back and the leather and the food are still
    /// there; take the safe road and the iron is waiting for you in the
    /// bones. See `types::BLOCK_REMAINS` for the argument.
    #[test]
    fn the_ground_takes_the_leather_and_leaves_the_iron() {
        use primitive_shared::inventory::Inventory;
        use primitive_shared::types::{
            BLOCK_COOKED_MEAT, BLOCK_IRON_CUIRASS, BLOCK_IRON_INGOT, BLOCK_IRON_PICKAXE,
            BLOCK_LEATHER_TUNIC, BLOCK_STONE,
        };
        let mut pack = Inventory::new();
        pack.put_in_slot(0, Stack::new(BLOCK_IRON_PICKAXE, 1));
        pack.put_in_slot(1, Stack::new(BLOCK_LEATHER_TUNIC, 1));
        pack.put_in_slot(2, Stack::new(BLOCK_COOKED_MEAT, 6));
        pack.put_in_slot(3, Stack::new(BLOCK_IRON_INGOT, 9));
        pack.put_in_slot(4, Stack::new(BLOCK_IRON_CUIRASS, 1));
        pack.put_in_slot(5, Stack::new(BLOCK_STONE, 40));

        assert!(what_the_ground_takes(&mut pack), "the ground took nothing at all");

        assert_eq!(pack.block_in(0), Some(BLOCK_IRON_PICKAXE), "the pick rotted");
        assert_eq!(pack.block_in(1), None, "the leather tunic survived the grave");
        assert_eq!(pack.block_in(2), None, "the food survived the grave");
        assert_eq!(pack.count_in(3), 9, "the ingots rotted");
        assert_eq!(pack.block_in(4), Some(BLOCK_IRON_CUIRASS), "the iron harness rotted");
        assert_eq!(pack.count_in(5), 40, "the stone rotted");
        // Everything that stayed stayed *where it was*: a player opening
        // their remains should see the gaps where the leather was rather
        // than a tidy list they have to audit from memory.
        assert_eq!(pack.block_in(5), Some(BLOCK_STONE));
        // ...and a second pass has nothing left to take, which is what
        // stops the pass marking the container dirty for ever.
        assert!(!what_the_ground_takes(&mut pack));
    }

    #[test]
    fn what_is_saved_comes_back_and_a_missing_file_is_a_fresh_world() {
        let dir = std::env::temp_dir().join(format!("primitive-carrion-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);

        let mut carrion = Carrion::new();
        carrion.on_block_changed(AT.0, AT.1, AT.2);
        carrion.reconcile(16, |_| Some(deer()));
        carrion.step(|_| Some((deer(), false)));
        assert_eq!(carrion.age_at(AT), 1);
        assert_eq!(carrion.save(&dir).unwrap(), 1);
        assert!(!carrion.is_dirty(), "saving left it dirty");

        let mut read = Carrion::new();
        assert_eq!(read.load(&dir).unwrap(), 1);
        assert_eq!(read.age_at(AT), 1, "the kill came back fresh");

        let mut empty = Carrion::new();
        assert_eq!(empty.load(&dir.join("nowhere")).unwrap(), 0);
        let _ = std::fs::remove_dir_all(&dir);
    }
}
