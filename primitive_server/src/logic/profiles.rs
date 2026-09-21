//! Who a player is, and what they had when they left.
//!
//! ## Why a UUID and not a name
//!
//! A name is what a player types; it is not an identity. Two people can
//! agree to swap names, one person can rename themselves, and a name is
//! a string that arrives over the wire from a client that may have typed
//! anything. Everything the server stores about a player -- their pack,
//! where they logged out, how much health they had -- is keyed by a
//! UUID, and the name is one more *field* of the record rather than the
//! key to it.
//!
//! ## Where the UUID comes from
//!
//! Derived from the name, deterministically, the way an offline-mode
//! server does it. There is no account service here, so the only stable
//! thing about a returning player is what they call themselves: hashing
//! it means the same name always resolves to the same record, which is
//! exactly the property "I came back and my stuff is still here" needs.
//! Two different names cannot collide in practice (128 bits), and the
//! derivation is written out below rather than pulled in as a
//! dependency, because it has to stay byte-identical across versions --
//! a changed hash silently orphans every saved player.
//!
//! If this ever grows real accounts, the derivation becomes the fallback
//! for unauthenticated joins and nothing else here changes: the rest of
//! the server already speaks UUIDs.
//!
//! ## What is stored
//!
//! The three things a player would notice the loss of:
//!
//! * their **inventory**, which the server owns anyway;
//! * their **place of exit** -- position and facing -- so logging back in
//!   puts them where they left rather than at spawn, which on a large
//!   world is a long walk;
//! * their **health**, so logging out at one heart is not a way to heal.
//!
//! ...and, since operators were added, one bit of *authority*: whether
//! this player may run the commands the console can. That bit lives
//! here rather than in a file of its own for the same reason the
//! inventory does -- it is a fact about a player, it is keyed by the
//! same UUID, and this store is already loaded before the first client
//! can connect and flushed atomically on every autosave. A second
//! `ops.bin` would have meant a second load path, a second save path, a
//! second thing to forget to write, and two files that can disagree
//! about who exists. The one real argument for a separate file is that
//! a `.toml` list of names could be edited by hand while the server is
//! down; against that is that names are not identities here (see
//! above), so a hand-edited name list would be the only place in the
//! server where a name is authoritative. Not worth it.
//!
//! Written to `<world>/players.bin` next to the block edits, in the same
//! shape: one bincode file, written to a temporary and renamed, so a
//! crash mid-write cannot leave a half-file where the profiles were.
//!
//! ## Why the format has a version 2
//!
//! bincode is not self-describing: fields are written back to back with
//! no names or tags, so a reader recovers them by counting bytes in the
//! order the struct declares them. Adding `operator` to `Profile` is
//! therefore *not* a compatible change the way it would be in JSON --
//! an old file read by the new struct would take the first byte of the
//! next profile as this one's operator flag and shear everything after
//! it. So the version goes to 2, and `load` reads a version-1 file
//! through the struct that wrote it and fills the new field in with
//! `false`. Old worlds keep their packs; nobody is silently promoted.

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use primitive_shared::discovery::Discovered;
use primitive_shared::injury::Injuries;
use primitive_shared::inventory::{Equipment, Inventory};
use primitive_shared::types::BlockId;

const SAVE_FORMAT_VERSION: u32 = 11;
/// The last version written before a position was an `f64`.
///
/// Read through `ProfileV10`, and the player comes back where the `f32`
/// had them: within a sixteenth of a block a million blocks out, and within
/// a block ten million out -- which `World::safe_position` stands clear of
/// a wall if it has to. What was not written cannot be recovered. See
/// `PROTOCOL_VERSION`'s fifty-seven for why a position is `f64` now.
const VERSION_WITH_F32_POSITIONS: u32 = 10;
/// The last version written before an illness outlived a reconnect.
///
/// Read through `ProfileV9`, and everybody in one comes back well: the
/// countdown was never written down, and guessing at one would be handing
/// somebody a stomach ache they did not catch. See `Profile::sick_for` for
/// why it is written down now -- quitting to the menu was the cure.
const VERSION_WITHOUT_SICKNESS: u32 = 9;
/// The last version written before a body had wounds: when all that could
/// be wrong with one was a count of seconds of broken leg.
///
/// Read through `ProfileV8`, and **the leg comes with it, set.** The count
/// is what the old rules said was left to mend, and under those rules it
/// mended on its own; under these a break waits for a splint. Handing
/// somebody back an unset leg they had every reason to think was knitting
/// would be a rule changed under them, so it arrives splinted and goes on
/// knitting for the time it had left. Nothing else was recorded, so nothing
/// else comes back.
const VERSION_WITHOUT_INJURIES: u32 = 8;
/// The last version written before a player remembered what they had
/// held and where they had died.
///
/// Read through `ProfileV7`, and upgraded as knowing nothing and owed no
/// bags. Knowing nothing is the honest answer -- nobody wrote it down --
/// and it costs a veteran one evening of picking things up again, because
/// the book is rebuilt from the pack the moment it is next sent. The bags
/// are still in the world; only their marks on the map are lost.
const VERSION_WITHOUT_DISCOVERY: u32 = 7;
/// The last version written before a leg could be broken.
///
/// Written in this same unreleased run, which is exactly why it needs a
/// reader: worlds played during the day it existed have profiles in it,
/// and a positional format read one field short is a profile full of
/// somebody else's numbers. Everybody in one comes back mended.
const VERSION_WITHOUT_FRACTURES: u32 = 6;
/// The last version written before a body could be tired.
///
/// Still readable, and upgraded on the way in as rested: a player who
/// has not played since the update comes back fresh, which is the
/// generous reading of "we have no idea" and the only one that does not
/// hand somebody an exhausted character they did not earn.
const VERSION_WITHOUT_TIREDNESS: u32 = 5;
/// The last version that did not know what a player was wearing, how
/// warm they were, or how much water they had.
///
/// Still readable: a profile out of one comes back dressed in nothing,
/// at a comfortable temperature and with a full skin, which is the
/// generous reading of "we have no idea" -- and the only alternative
/// would be handing everybody who has not played since the update a case
/// of hypothermia they did not earn.
const VERSION_WITHOUT_A_BODY: u32 = 4;
/// The last version that did not know about hunger. Still readable, and
/// upgraded on the way in: a player who logged out before 1.5 comes back
/// with a full stomach, which is the only defensible guess -- the
/// alternative is somebody who has not played for a month logging in to
/// starve.
/// The version written before a tool could wear out, when a slot was
/// two numbers rather than three. See `StackV3`.
const VERSION_WITHOUT_WEAR: u32 = 3;
const VERSION_WITHOUT_HUNGER: u32 = 2;
/// The last version that did not know about operators. Still readable;
/// see the module docs.
const VERSION_WITHOUT_OPERATORS: u32 = 1;
const FILE_NAME: &str = "players.bin";

/// A 128-bit identity, printed in the usual 8-4-4-4-12 form.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct Uuid(pub u128);

impl Uuid {
    /// The UUID for a name.
    ///
    /// Two rounds of a 64-bit mixer over the lower-cased name, with
    /// different salts, glued into 128 bits. Lower-cased because
    /// "Shamkhan" and "shamkhan" are one person as far as anyone typing
    /// them is concerned, and a server where case decides whose pack you
    /// get would be a cruel joke.
    ///
    /// The version and variant nibbles are then forced to those of a
    /// name-based UUID, so what comes out is a well-formed UUID rather
    /// than sixteen bytes wearing the notation.
    pub fn of_name(name: &str) -> Self {
        let lowered = name.to_lowercase();
        let high = mix(lowered.as_bytes(), 0x9E37_79B9_7F4A_7C15);
        let low = mix(lowered.as_bytes(), 0xC2B2_AE3D_27D4_EB4F);
        let mut value = ((high as u128) << 64) | low as u128;
        // Version 3 (name-based) and the RFC 4122 variant.
        value &= !(0xF000u128 << 64);
        value |= 0x3000u128 << 64;
        value &= !(0xC000u128 << 48);
        value |= 0x8000u128 << 48;
        Uuid(value)
    }
}

impl std::fmt::Display for Uuid {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let v = self.0;
        write!(
            f,
            "{:08x}-{:04x}-{:04x}-{:04x}-{:012x}",
            (v >> 96) as u32,
            (v >> 80) as u16,
            (v >> 64) as u16,
            (v >> 48) as u16,
            (v & 0xFFFF_FFFF_FFFF) as u64,
        )
    }
}

/// A 64-bit mix of a byte string. FNV-1a, then avalanched.
///
/// Written out rather than depended on: this value is part of the save
/// format, and a dependency that improves its hash would orphan every
/// stored player.
fn mix(bytes: &[u8], salt: u64) -> u64 {
    let mut h: u64 = 0xCBF2_9CE4_8422_2325 ^ salt;
    for &b in bytes {
        h ^= b as u64;
        h = h.wrapping_mul(0x0000_0100_0000_01B3);
    }
    h ^= h >> 33;
    h = h.wrapping_mul(0xFF51_AFD7_ED55_8CCD);
    h ^= h >> 33;
    h = h.wrapping_mul(0xC4CE_B9FE_1A85_EC53);
    h ^ (h >> 33)
}

/// Everything the server remembers about someone who is not here.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Profile {
    pub uuid: Uuid,
    /// The last name this player went by. Kept for the console and the
    /// player list; nothing is looked up by it.
    pub username: String,
    pub inventory: Inventory,
    /// Where they left the world, and which way they were facing.
    pub position: (f64, f64, f64),
    pub yaw: f32,
    pub pitch: f32,
    pub health: f32,
    /// Which hotbar slot was in hand.
    pub selected_slot: u8,
    /// How many times this profile has joined. Cheap, and the first
    /// question anyone asks of a player record.
    pub joins: u64,
    /// Whether this player runs commands at console permission.
    ///
    /// Stored per player rather than per connection so that it survives
    /// a reconnect, a restart, and being offline when it is granted --
    /// all three of which are the normal case for "make Alice an
    /// operator". Nobody is one by default, including on a world that
    /// predates the field: an upgrade that hands out authority is not an
    /// upgrade anyone wants.
    ///
    /// Deliberately *not* marked `#[serde(default)]`: that attribute
    /// would read as "old files are fine", and with bincode it does
    /// nothing at all. The version bump is what makes old files fine.
    pub operator: bool,
    /// How full they were when they left, in `food::MAX_NOURISHMENT`
    /// units.
    ///
    /// Stored for the same reason health is: logging out is not a meal,
    /// and a player who quits at the end of a hard evening should not
    /// find the bar refilled in the morning. What a *new* profile gets
    /// is a full one, which is decided in `Profile::new` rather than
    /// here -- see the note on `VERSION_WITHOUT_HUNGER` for what an old
    /// file gets.
    pub nourishment: f32,
    /// What they had on. Stored for the obvious reason -- a coat is
    /// worth more than most of a pack -- and kept apart from the
    /// inventory for the reason `inventory::Equipment` exists at all.
    pub equipment: Equipment,
    /// How much water they had left, in `body::MAX_HYDRATION` units.
    ///
    /// On exactly the terms hunger is: logging out is not a drink.
    pub hydration: f32,
    /// How warm they were, and how wet.
    ///
    /// **Stored, and that is a decision rather than an obvious one.**
    /// The alternative is to hand everybody a comfortable body on every
    /// join, which would make logging out and back in the answer to a
    /// blizzard -- a free reset on the one meter that is *about*
    /// enduring something. So it is written down, and the one case that
    /// needs care is a player who logs out freezing: they come back
    /// freezing, which is the point, and they come back at the spawn
    /// they left rather than in the blizzard, which is what makes it
    /// survivable.
    pub body_c: f32,
    pub wetness: f32,
    /// How much longer the bad water they drank is still taking, in
    /// seconds.
    ///
    /// **Saved, and it used to say in so many words that it was not
    /// worth saving**: "a minute and a half of illness is not worth a
    /// file format". The player found the other half of that sentence --
    /// quitting to the menu and coming back was the cure, and it was
    /// free, instant and available at the exact moment being ill costs
    /// anything. A meter that a reconnect clears is the same exploit
    /// `fatigue` and `injuries` are saved to close; that it is short is
    /// what makes it *cheap* to use, not harmless.
    ///
    /// The countdown does not run while nobody is playing -- it is
    /// seconds of world, and a paused server cures nobody (see
    /// `Vitals::sick_for`), so logging out for a week and coming back
    /// leaves exactly the illness that was left.
    pub sick_for: f32,
    /// Every wound they left with. Saved for the reason `fatigue` is: a
    /// deep cut or a leg half knitted that a reconnect cleared would be an
    /// injury nobody carried. It used to be `fracture_for`, a count of
    /// seconds of broken leg -- see `VERSION_WITHOUT_INJURIES`.
    pub injuries: Injuries,
    /// How tired they were.
    ///
    /// **Saved, and for one reason: otherwise logging out is a night's
    /// sleep.** Tiredness is a debt built up across a whole day (see
    /// `body::WAKING_SECONDS`), and a meter that a reconnect clears is
    /// a meter nobody would ever fill. It is also the cheapest possible
    /// exploit to find by accident, which is the worst kind.
    pub fatigue: f32,
    /// Every kind of block they have held, which is what their recipe
    /// book is. Block ids and not recipe indices -- see
    /// `primitive_shared::discovery` for why that is the difference
    /// between knowledge that survives an update and knowledge that is
    /// silently rewired by one.
    pub discovered: Vec<BlockId>,
    /// Where the bags they have not gone back for are, oldest first.
    ///
    /// **Saved**, because the way back is most needed by the player who
    /// died, rage-quit and came back the next evening -- and a map mark
    /// that a reconnect wiped would send them back to spawn with nothing
    /// and no idea which hill it was.
    pub bags: Vec<(i32, i32, i32)>,
}

impl Profile {
    fn new(uuid: Uuid, username: &str, spawn: (f32, f32, f32), health: f32) -> Self {
        Self {
            uuid,
            username: username.to_string(),
            inventory: Inventory::new(),
            equipment: Equipment::new(),
            hydration: primitive_shared::body::MAX_HYDRATION,
            body_c: primitive_shared::body::NEUTRAL_C,
            wetness: 0.0,
            // A new player has just woken up, in one piece.
            fatigue: 0.0,
            injuries: Injuries::default(),
            sick_for: 0.0,
            // Nothing held, nowhere to go back to.
            discovered: Vec::new(),
            bags: Vec::new(),
            position: primitive_shared::geometry::wide(spawn),
            yaw: 0.0,
            pitch: 0.0,
            health,
            selected_slot: 0,
            joins: 0,
            operator: false,
            // Nobody arrives hungry.
            nourishment: primitive_shared::food::MAX_NOURISHMENT,
        }
    }

    /// Whether the stored place of exit is *readable*.
    ///
    /// Finite numbers inside the world's height, and nothing more --
    /// which is all this can honestly check. Whether a position is
    /// inside a mountain is a question about the world, and a profile
    /// store has never seen one; this comment used to claim otherwise
    /// and the claim was the bug. A player logged back in wherever they
    /// left, and if the world had moved under it -- somebody built
    /// there, or the generator changed and grew a tree exactly where
    /// they had been standing -- they came back inside it, welded in
    /// place, with dying no help because it put them back in the same
    /// cell.
    ///
    /// The world checks it now, on the way in: see
    /// `World::safe_position`, which keeps the column and moves only the
    /// height.
    fn place_of_exit(&self) -> Option<(f64, f64, f64)> {
        let (x, y, z) = self.position;
        let sane = x.is_finite()
            && y.is_finite()
            && z.is_finite()
            && y >= 0.0
            && y < primitive_shared::types::CHUNK_SIZE_Y as f64;
        sane.then_some((x, y, z))
    }
}

/// The four numbers a body is, bundled so `store` does not grow a
/// twelfth positional argument.
///
/// A struct rather than four more parameters because `store` is called
/// from two places and both of them build it out of one
/// `PlayerRuntime` -- so the bundle is the thing that actually travels,
/// and four `f32`s in a row is exactly the signature that eventually
/// gets called with two of them the wrong way round.
#[derive(Debug, Clone, PartialEq)]
pub struct StoredBody {
    pub equipment: Equipment,
    pub hydration: f32,
    pub body_c: f32,
    pub wetness: f32,
    /// How tired. See `Profile::fatigue`.
    pub fatigue: f32,
    /// ...and what is wrong with them. See `Profile::injuries`.
    pub injuries: Injuries,
    /// How much longer bad water is still taking. See `Profile::sick_for`.
    pub sick_for: f32,
}

impl Default for StoredBody {
    fn default() -> Self {
        Self {
            equipment: Equipment::new(),
            hydration: primitive_shared::body::MAX_HYDRATION,
            body_c: primitive_shared::body::NEUTRAL_C,
            wetness: 0.0,
            fatigue: 0.0,
            injuries: Injuries::default(),
            sick_for: 0.0,
        }
    }
}

#[derive(Serialize, Deserialize)]
struct SaveFile {
    version: u32,
    profiles: Vec<Profile>,
}

// ---- what a player looked like while a position was an `f32` ----
//
// Positional, like every shape below it: four bytes a coordinate read as
// eight is every field after it shifted. See `VERSION_WITH_F32_POSITIONS`.

#[derive(Serialize, Deserialize)]
struct ProfileV10 {
    uuid: Uuid,
    username: String,
    inventory: Inventory,
    position: (f32, f32, f32),
    yaw: f32,
    pitch: f32,
    health: f32,
    selected_slot: u8,
    joins: u64,
    operator: bool,
    nourishment: f32,
    equipment: Equipment,
    hydration: f32,
    body_c: f32,
    wetness: f32,
    sick_for: f32,
    injuries: Injuries,
    fatigue: f32,
    discovered: Vec<BlockId>,
    bags: Vec<(i32, i32, i32)>,
}

#[derive(Serialize, Deserialize)]
struct SaveFileV10 {
    version: u32,
    profiles: Vec<ProfileV10>,
}

impl From<ProfileV10> for Profile {
    fn from(old: ProfileV10) -> Self {
        Self {
            uuid: old.uuid,
            username: old.username,
            inventory: old.inventory,
            position: primitive_shared::geometry::wide(old.position),
            yaw: old.yaw,
            pitch: old.pitch,
            health: old.health,
            selected_slot: old.selected_slot,
            joins: old.joins,
            operator: old.operator,
            nourishment: old.nourishment,
            equipment: old.equipment,
            hydration: old.hydration,
            body_c: old.body_c,
            wetness: old.wetness,
            sick_for: old.sick_for,
            injuries: old.injuries,
            fatigue: old.fatigue,
            discovered: old.discovered,
            bags: old.bags,
        }
    }
}

// ---- what a player looked like before an illness survived a reconnect ----
//
// Positional, like every shape above it: one field short read as the new
// one is a profile full of the next player's numbers. See
// `VERSION_WITHOUT_SICKNESS`.

#[derive(Serialize, Deserialize)]
struct ProfileV9 {
    uuid: Uuid,
    username: String,
    inventory: Inventory,
    position: (f32, f32, f32),
    yaw: f32,
    pitch: f32,
    health: f32,
    selected_slot: u8,
    joins: u64,
    operator: bool,
    nourishment: f32,
    equipment: Equipment,
    hydration: f32,
    body_c: f32,
    wetness: f32,
    injuries: Injuries,
    fatigue: f32,
    discovered: Vec<BlockId>,
    bags: Vec<(i32, i32, i32)>,
}

#[derive(Serialize, Deserialize)]
struct SaveFileV9 {
    version: u32,
    profiles: Vec<ProfileV9>,
}

impl From<ProfileV9> for Profile {
    fn from(old: ProfileV9) -> Self {
        Self {
            uuid: old.uuid,
            username: old.username,
            inventory: old.inventory,
            position: primitive_shared::geometry::wide(old.position),
            yaw: old.yaw,
            pitch: old.pitch,
            health: old.health,
            selected_slot: old.selected_slot,
            joins: old.joins,
            operator: old.operator,
            nourishment: old.nourishment,
            equipment: old.equipment,
            hydration: old.hydration,
            body_c: old.body_c,
            wetness: old.wetness,
            // Nobody wrote it down. See `VERSION_WITHOUT_SICKNESS`.
            sick_for: 0.0,
            injuries: old.injuries,
            fatigue: old.fatigue,
            discovered: old.discovered,
            bags: old.bags,
        }
    }
}

// ---- what a pack looked like before tools wore out ----
//
// A slot went from two numbers to three (`inventory::Stack::damage`),
// and bincode writes fields by position with no names: an old file read
// as a new one decodes into nonsense rather than failing. So the old
// shape is frozen here, and every reader below version 4 goes through
// it. Losing a profile is losing everything a player was carrying.

#[derive(Serialize, Deserialize)]
struct StackV3 {
    block: primitive_shared::types::BlockId,
    count: u32,
}

#[derive(Serialize, Deserialize)]
struct InventoryV3 {
    slots: Vec<Option<StackV3>>,
}

impl From<InventoryV3> for Inventory {
    fn from(old: InventoryV3) -> Self {
        let mut inventory = Inventory::new();
        for (slot, held) in old.slots.into_iter().enumerate() {
            if let Some(stack) = held {
                inventory.put_in_slot(
                    slot,
                    primitive_shared::inventory::Stack::new(stack.block, stack.count),
                );
            }
        }
        inventory
    }
}

/// The shape version 4 wrote: today's profile without a body on it.
///
/// A frozen copy rather than something clever with optional fields,
/// because a frozen copy is the only thing that stays correct -- bincode
/// writes fields by position with no names and no defaults, so a v4 file
/// read as a v5 one is not a file with three fields missing, it is a
/// file that decodes into nonsense. Losing a profile is losing
/// everything a player was carrying.
#[derive(Serialize, Deserialize)]
struct ProfileV4 {
    uuid: Uuid,
    username: String,
    inventory: Inventory,
    position: (f32, f32, f32),
    yaw: f32,
    pitch: f32,
    health: f32,
    selected_slot: u8,
    joins: u64,
    operator: bool,
    nourishment: f32,
}

#[derive(Serialize, Deserialize)]
struct SaveFileV4 {
    version: u32,
    profiles: Vec<ProfileV4>,
}

impl From<ProfileV4> for Profile {
    fn from(old: ProfileV4) -> Self {
        Profile {
            uuid: old.uuid,
            username: old.username,
            inventory: old.inventory,
            position: primitive_shared::geometry::wide(old.position),
            yaw: old.yaw,
            pitch: old.pitch,
            health: old.health,
            selected_slot: old.selected_slot,
            joins: old.joins,
            operator: old.operator,
            nourishment: old.nourishment,
            // See `VERSION_WITHOUT_A_BODY`: nothing on, comfortable, dry
            // and watered.
            equipment: Equipment::new(),
            hydration: primitive_shared::body::MAX_HYDRATION,
            body_c: primitive_shared::body::NEUTRAL_C,
            wetness: 0.0,
            fatigue: 0.0,
            injuries: Injuries::default(),
            sick_for: 0.0,
            discovered: Vec::new(),
            bags: Vec::new(),
        }
    }
}

/// The shape version 7 wrote: today's profile without the memory of what
/// was held and where the bags are. See `VERSION_WITHOUT_DISCOVERY`.
#[derive(Serialize, Deserialize)]
struct ProfileV7 {
    uuid: Uuid,
    username: String,
    inventory: Inventory,
    position: (f32, f32, f32),
    yaw: f32,
    pitch: f32,
    health: f32,
    selected_slot: u8,
    joins: u64,
    operator: bool,
    nourishment: f32,
    equipment: Equipment,
    hydration: f32,
    body_c: f32,
    wetness: f32,
    fracture_for: f32,
    fatigue: f32,
}

#[derive(Serialize, Deserialize)]
struct SaveFileV7 {
    version: u32,
    profiles: Vec<ProfileV7>,
}

impl From<ProfileV7> for Profile {
    fn from(old: ProfileV7) -> Self {
        Profile {
            uuid: old.uuid,
            username: old.username,
            inventory: old.inventory,
            position: primitive_shared::geometry::wide(old.position),
            yaw: old.yaw,
            pitch: old.pitch,
            health: old.health,
            selected_slot: old.selected_slot,
            joins: old.joins,
            operator: old.operator,
            nourishment: old.nourishment,
            equipment: old.equipment,
            hydration: old.hydration,
            body_c: old.body_c,
            wetness: old.wetness,
            // See `VERSION_WITHOUT_INJURIES`: a leg that was knitting
            // arrives set.
            injuries: set_leg(old.fracture_for),
            fatigue: old.fatigue,
            // See `VERSION_WITHOUT_SICKNESS`: nobody comes back ill.
            sick_for: 0.0,
            discovered: Vec::new(),
            bags: Vec::new(),
        }
    }
}

/// The shape version 8 wrote: today's profile with the broken leg as a
/// count of seconds rather than a set of wounds. See
/// `VERSION_WITHOUT_INJURIES`.
#[derive(Serialize, Deserialize)]
struct ProfileV8 {
    uuid: Uuid,
    username: String,
    inventory: Inventory,
    position: (f32, f32, f32),
    yaw: f32,
    pitch: f32,
    health: f32,
    selected_slot: u8,
    joins: u64,
    operator: bool,
    nourishment: f32,
    equipment: Equipment,
    hydration: f32,
    body_c: f32,
    wetness: f32,
    fracture_for: f32,
    fatigue: f32,
    discovered: Vec<BlockId>,
    bags: Vec<(i32, i32, i32)>,
}

#[derive(Serialize, Deserialize)]
struct SaveFileV8 {
    version: u32,
    profiles: Vec<ProfileV8>,
}

impl From<ProfileV8> for Profile {
    fn from(old: ProfileV8) -> Self {
        Profile {
            uuid: old.uuid,
            username: old.username,
            inventory: old.inventory,
            position: primitive_shared::geometry::wide(old.position),
            yaw: old.yaw,
            pitch: old.pitch,
            health: old.health,
            selected_slot: old.selected_slot,
            joins: old.joins,
            operator: old.operator,
            nourishment: old.nourishment,
            equipment: old.equipment,
            hydration: old.hydration,
            body_c: old.body_c,
            wetness: old.wetness,
            injuries: set_leg(old.fracture_for),
            fatigue: old.fatigue,
            // See `VERSION_WITHOUT_SICKNESS`: nobody comes back ill.
            sick_for: 0.0,
            discovered: old.discovered,
            bags: old.bags,
        }
    }
}

/// What an old count of seconds of broken leg is as wounds: the left leg,
/// splinted, with as much left to knit as the count said. Which leg was
/// never recorded, and either answer is the same limp.
fn set_leg(fracture_for: f32) -> Injuries {
    use primitive_shared::injury::{Kind, Part};
    let mut injuries = Injuries::default();
    if fracture_for.is_finite() && fracture_for > 0.0 {
        let left = (fracture_for / primitive_shared::body::FRACTURE_SECONDS).min(1.0);
        injuries.inflict(Part::LeftLeg, Kind::Fracture, left);
        let _ = injuries.treat(Part::LeftLeg, primitive_shared::types::BLOCK_SPLINT);
    }
    injuries
}

/// The shape version 6 wrote: today's profile without the break.
#[derive(Serialize, Deserialize)]
struct ProfileV6 {
    uuid: Uuid,
    username: String,
    inventory: Inventory,
    position: (f32, f32, f32),
    yaw: f32,
    pitch: f32,
    health: f32,
    selected_slot: u8,
    joins: u64,
    operator: bool,
    nourishment: f32,
    equipment: Equipment,
    hydration: f32,
    body_c: f32,
    wetness: f32,
    fatigue: f32,
}

#[derive(Serialize, Deserialize)]
struct SaveFileV6 {
    version: u32,
    profiles: Vec<ProfileV6>,
}

impl From<ProfileV6> for Profile {
    fn from(old: ProfileV6) -> Self {
        Profile {
            uuid: old.uuid,
            username: old.username,
            inventory: old.inventory,
            position: primitive_shared::geometry::wide(old.position),
            yaw: old.yaw,
            pitch: old.pitch,
            health: old.health,
            selected_slot: old.selected_slot,
            joins: old.joins,
            operator: old.operator,
            nourishment: old.nourishment,
            equipment: old.equipment,
            hydration: old.hydration,
            body_c: old.body_c,
            wetness: old.wetness,
            fatigue: old.fatigue,
            // Mended, and well. See `VERSION_WITHOUT_FRACTURES` and
            // `VERSION_WITHOUT_SICKNESS`.
            injuries: Injuries::default(),
            sick_for: 0.0,
            discovered: Vec::new(),
            bags: Vec::new(),
        }
    }
}

/// The shape version 5 wrote: today's profile without the tiredness.
///
/// Frozen here for the reason every other `ProfileVn` is: bincode
/// writes fields by position and by count, so a version-5 file read
/// through today's struct does not fail -- it takes the first bytes of
/// the *next* profile as this one's fatigue and shears everything after
/// it. Losing a profile is losing everything a player was carrying.
#[derive(Serialize, Deserialize)]
struct ProfileV5 {
    uuid: Uuid,
    username: String,
    inventory: Inventory,
    position: (f32, f32, f32),
    yaw: f32,
    pitch: f32,
    health: f32,
    selected_slot: u8,
    joins: u64,
    operator: bool,
    nourishment: f32,
    equipment: Equipment,
    hydration: f32,
    body_c: f32,
    wetness: f32,
}

#[derive(Serialize, Deserialize)]
struct SaveFileV5 {
    version: u32,
    profiles: Vec<ProfileV5>,
}

impl From<ProfileV5> for Profile {
    fn from(old: ProfileV5) -> Self {
        Profile {
            uuid: old.uuid,
            username: old.username,
            inventory: old.inventory,
            position: primitive_shared::geometry::wide(old.position),
            yaw: old.yaw,
            pitch: old.pitch,
            health: old.health,
            selected_slot: old.selected_slot,
            joins: old.joins,
            operator: old.operator,
            nourishment: old.nourishment,
            equipment: old.equipment,
            hydration: old.hydration,
            body_c: old.body_c,
            wetness: old.wetness,
            // Rested, and in one piece.
            fatigue: 0.0,
            injuries: Injuries::default(),
            sick_for: 0.0,
            discovered: Vec::new(),
            bags: Vec::new(),
        }
    }
}

/// The shape version 3 wrote: today's profile with the old pack in it.
#[derive(Serialize, Deserialize)]
struct ProfileV3 {
    uuid: Uuid,
    username: String,
    inventory: InventoryV3,
    position: (f32, f32, f32),
    yaw: f32,
    pitch: f32,
    health: f32,
    selected_slot: u8,
    joins: u64,
    operator: bool,
    nourishment: f32,
}

#[derive(Serialize, Deserialize)]
struct SaveFileV3 {
    version: u32,
    profiles: Vec<ProfileV3>,
}

impl From<ProfileV3> for Profile {
    fn from(old: ProfileV3) -> Self {
        Profile {
            uuid: old.uuid,
            username: old.username,
            inventory: old.inventory.into(),
            position: primitive_shared::geometry::wide(old.position),
            yaw: old.yaw,
            pitch: old.pitch,
            health: old.health,
            selected_slot: old.selected_slot,
            joins: old.joins,
            operator: old.operator,
            nourishment: old.nourishment,
            equipment: Equipment::new(),
            hydration: primitive_shared::body::MAX_HYDRATION,
            body_c: primitive_shared::body::NEUTRAL_C,
            wetness: 0.0,
            // See `VERSION_WITHOUT_TIREDNESS`: rested.
            fatigue: 0.0,
            injuries: Injuries::default(),
            sick_for: 0.0,
            discovered: Vec::new(),
            bags: Vec::new(),
        }
    }
}

/// `players.bin` as version 1 wrote it: `Profile` without `operator`.
///
/// A copy of the old struct rather than something clever with optional
/// fields, because a copy is the only thing that stays correct: it is
/// frozen at the shape those bytes were written in, and no future edit
/// to `Profile` can accidentally change how an existing file is read.
/// It costs one struct that is never touched again, and it is what
/// keeps a world made last week loading this week.
/// The shape version 2 wrote, kept for the same reason `ProfileV1` is:
/// a frozen copy is the only thing that stays correct, because no future
/// edit to `Profile` can accidentally change how an existing file reads.
#[derive(Serialize, Deserialize)]
struct ProfileV2 {
    uuid: Uuid,
    username: String,
    inventory: InventoryV3,
    position: (f32, f32, f32),
    yaw: f32,
    pitch: f32,
    health: f32,
    selected_slot: u8,
    joins: u64,
    operator: bool,
}

#[derive(Serialize, Deserialize)]
struct SaveFileV2 {
    version: u32,
    profiles: Vec<ProfileV2>,
}

impl From<ProfileV2> for Profile {
    fn from(old: ProfileV2) -> Self {
        Profile {
            uuid: old.uuid,
            username: old.username,
            inventory: old.inventory.into(),
            position: primitive_shared::geometry::wide(old.position),
            yaw: old.yaw,
            pitch: old.pitch,
            health: old.health,
            selected_slot: old.selected_slot,
            joins: old.joins,
            operator: old.operator,
            // A world from before hunger existed. Full, because the
            // alternative is that everybody who has not played since the
            // update logs in starving -- a rule nobody agreed to,
            // applied to people who were not there.
            nourishment: primitive_shared::food::MAX_NOURISHMENT,
            equipment: Equipment::new(),
            hydration: primitive_shared::body::MAX_HYDRATION,
            body_c: primitive_shared::body::NEUTRAL_C,
            wetness: 0.0,
            // See `VERSION_WITHOUT_TIREDNESS`: rested.
            fatigue: 0.0,
            injuries: Injuries::default(),
            sick_for: 0.0,
            discovered: Vec::new(),
            bags: Vec::new(),
        }
    }
}

#[derive(Serialize, Deserialize)]
struct ProfileV1 {
    uuid: Uuid,
    username: String,
    inventory: InventoryV3,
    position: (f32, f32, f32),
    yaw: f32,
    pitch: f32,
    health: f32,
    selected_slot: u8,
    joins: u64,
}

#[derive(Serialize, Deserialize)]
struct SaveFileV1 {
    version: u32,
    profiles: Vec<ProfileV1>,
}

impl From<ProfileV1> for Profile {
    fn from(old: ProfileV1) -> Self {
        Profile {
            uuid: old.uuid,
            username: old.username,
            inventory: old.inventory.into(),
            position: primitive_shared::geometry::wide(old.position),
            yaw: old.yaw,
            pitch: old.pitch,
            health: old.health,
            selected_slot: old.selected_slot,
            joins: old.joins,
            // Upgrading a world does not hand anyone the keys.
            operator: false,
            nourishment: primitive_shared::food::MAX_NOURISHMENT,
            equipment: Equipment::new(),
            hydration: primitive_shared::body::MAX_HYDRATION,
            body_c: primitive_shared::body::NEUTRAL_C,
            wetness: 0.0,
            // See `VERSION_WITHOUT_TIREDNESS`: rested.
            fatigue: 0.0,
            injuries: Injuries::default(),
            sick_for: 0.0,
            discovered: Vec::new(),
            bags: Vec::new(),
        }
    }
}

/// What `set_operator` did.
///
/// Three answers and not a `bool`, because "already an operator" and
/// "just made one" are different things to say to whoever asked, and a
/// command that silently succeeds when it did nothing is how an
/// operator ends up believing a promotion happened that never did. The
/// username comes back with it, spelled the way the profile spells it:
/// the caller typed a name, and the reply should name the record that
/// was found rather than echo the typing.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum OperatorChange {
    Changed { username: String },
    Unchanged { username: String },
    /// Nobody has ever played under that name here.
    NoSuchPlayer,
}

/// Every player the server has ever seen, and where their things are.
#[derive(Default)]
pub struct Profiles {
    by_uuid: HashMap<Uuid, Profile>,
    /// Set when anything changed since the last save, so an idle server
    /// does not rewrite the file on every autosave tick.
    dirty: bool,
    /// Set when there was a file and this build could not read it -- one
    /// from a newer server, or a damaged one. **Nothing is written over it
    /// for the rest of the run.** "Left alone rather than guessed at" used
    /// to last until the first player joined: the join marked the store
    /// dirty and the autosave replaced everybody's packs with one empty
    /// one. What this costs is that the run's own progress is not saved,
    /// and that run started everybody from nothing anyway.
    unreadable: bool,
}

impl Profiles {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn len(&self) -> usize {
        self.by_uuid.len()
    }

    pub fn is_empty(&self) -> bool {
        self.by_uuid.is_empty()
    }

    /// The record for a name, created if this is a first visit.
    ///
    /// Returns the profile as it stands *before* this join is counted
    /// into it, which is what the caller needs to restore state from.
    pub fn join(&mut self, username: &str, spawn: (f32, f32, f32), health: f32) -> Profile {
        let uuid = Uuid::of_name(username);
        let profile = self
            .by_uuid
            .entry(uuid)
            .or_insert_with(|| Profile::new(uuid, username, spawn, health));
        // A returning player may have changed how they capitalise their
        // name; the record follows what they answer to now.
        profile.username = username.to_string();
        profile.joins += 1;
        profile.inventory.sanitize();
        self.dirty = true;
        profile.clone()
    }

    /// Records where a player left off.
    #[allow(clippy::too_many_arguments)] // one argument per thing remembered
    #[allow(clippy::too_many_arguments)] // it is a record, and this is its columns
    pub fn store(
        &mut self,
        uuid: Uuid,
        inventory: Inventory,
        position: (f64, f64, f64),
        yaw: f32,
        pitch: f32,
        health: f32,
        nourishment: f32,
        selected_slot: u8,
        body: StoredBody,
    ) {
        let Some(profile) = self.by_uuid.get_mut(&uuid) else {
            return;
        };
        profile.inventory = inventory;
        profile.position = position;
        profile.yaw = yaw;
        profile.pitch = pitch;
        profile.health = health;
        profile.nourishment = nourishment;
        profile.selected_slot = selected_slot;
        profile.equipment = body.equipment;
        profile.hydration = body.hydration;
        profile.body_c = body.body_c;
        profile.wetness = body.wetness;
        // **These two were never written down**, while the fields they
        // fill said "saved" in bold: tiredness and the broken leg went
        // into `StoredBody` on every save and were dropped here, so logging
        // out was a night's sleep and a splint. The two lines that were
        // missing.
        profile.fatigue = body.fatigue;
        profile.injuries = body.injuries;
        // ...and the third of them: the illness that used to be cured by
        // quitting to the menu. See `Profile::sick_for`.
        profile.sick_for = body.sick_for;
        self.dirty = true;
    }

    /// Records what a player has found and where their bags are.
    ///
    /// **Its own call rather than two more columns of `store`**, because
    /// `store` is the body -- what a reconnect must not reset -- and this
    /// is the memory, which changes a few times an evening. Marked dirty
    /// only when it actually changed, so an autosave of an unchanged
    /// player does not rewrite the file for a list it already has.
    pub fn remember(&mut self, uuid: Uuid, discovered: &Discovered, bags: &[(i32, i32, i32)]) {
        let Some(profile) = self.by_uuid.get_mut(&uuid) else {
            return;
        };
        if profile.discovered != discovered.kinds() {
            profile.discovered = discovered.kinds().to_vec();
            self.dirty = true;
        }
        if profile.bags != bags {
            profile.bags = bags.to_vec();
            self.dirty = true;
        }
    }

    pub fn get(&self, uuid: Uuid) -> Option<&Profile> {
        self.by_uuid.get(&uuid)
    }

    /// Whether this player runs commands at console permission.
    ///
    /// Asked once per command rather than latched onto the connection at
    /// join time, which is what makes `/op` take effect on the player's
    /// very next line instead of on their next login. It is a hash
    /// lookup behind a lock that the same command already takes.
    ///
    /// Someone with no profile -- which is to say the console, whose
    /// caller is `None` -- never reaches this: the console is an
    /// operator by construction and cannot be demoted, because the
    /// person holding the keyboard the server is running under can
    /// already do anything a `/deop` could take away.
    pub fn is_operator(&self, uuid: Uuid) -> bool {
        self.by_uuid.get(&uuid).is_some_and(|p| p.operator)
    }

    /// Grants or revokes operator rights by name, reporting what
    /// actually happened.
    ///
    /// By name and not by connection, so an offline player can be
    /// promoted -- which is the ordinary case, since the reason to make
    /// someone an operator is usually that they are not there and
    /// something needs doing. Anyone who has ever joined has a profile,
    /// and `Uuid::of_name` finds it without them being here.
    ///
    /// A name nobody has ever played under is refused rather than
    /// creating a profile for it: `Uuid::of_name` answers for *every*
    /// string, so accepting unknown names would turn a typo into an
    /// operator record for a player who does not exist, waiting to be
    /// claimed by whoever guesses the misspelling first.
    pub fn set_operator(&mut self, username: &str, operator: bool) -> OperatorChange {
        let Some(profile) = self.by_uuid.get_mut(&Uuid::of_name(username)) else {
            return OperatorChange::NoSuchPlayer;
        };
        if profile.operator == operator {
            return OperatorChange::Unchanged {
                username: profile.username.clone(),
            };
        }
        profile.operator = operator;
        let username = profile.username.clone();
        self.dirty = true;
        OperatorChange::Changed { username }
    }

    /// Every profile, newest name first -- for `/players` and the like.
    pub fn all(&self) -> Vec<&Profile> {
        let mut all: Vec<&Profile> = self.by_uuid.values().collect();
        all.sort_by(|a, b| a.username.cmp(&b.username));
        all
    }

    fn path(dir: &Path) -> PathBuf {
        dir.join(FILE_NAME)
    }

    /// Writes the file, if anything has changed. Returns how many
    /// profiles were written, or `None` if there was nothing to do.
    pub fn save(&mut self, dir: &Path) -> std::io::Result<Option<usize>> {
        if !self.dirty || self.unreadable {
            return Ok(None);
        }
        std::fs::create_dir_all(dir)?;
        let mut profiles: Vec<Profile> = self.by_uuid.values().cloned().collect();
        // Stable bytes for the same state, which makes a diff of two
        // saves mean something.
        profiles.sort_by_key(|p| p.uuid.0);
        let payload = SaveFile {
            version: SAVE_FORMAT_VERSION,
            profiles,
        };
        let bytes = bincode::serialize(&payload)
            .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))?;
        let final_path = Self::path(dir);
        let tmp = final_path.with_extension("bin.tmp");
        std::fs::write(&tmp, &bytes)?;
        std::fs::rename(&tmp, &final_path)?;
        self.dirty = false;
        Ok(Some(payload.profiles.len()))
    }

    /// Reads the file. A missing one is a new world, not an error.
    ///
    /// One that is there and cannot be read -- an error, or a version this
    /// build does not know -- marks the store unreadable, so `save` leaves
    /// it where it is. See `Profiles::unreadable`.
    pub fn load(&mut self, dir: &Path) -> std::io::Result<usize> {
        let read = self.read_file(dir);
        if read.is_err() {
            self.unreadable = true;
        }
        read
    }

    /// Whether the file on disk could not be read and is being left alone.
    pub fn is_unreadable(&self) -> bool {
        self.unreadable
    }

    fn read_file(&mut self, dir: &Path) -> std::io::Result<usize> {
        let bytes = match std::fs::read(Self::path(dir)) {
            Ok(b) => b,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(0),
            Err(e) => return Err(e),
        };
        // The version is the first thing in the file, so it can be read
        // before committing to a shape for the rest of it. This has to
        // happen before any full deserialize: a version-1 file fed to
        // the version-2 struct does not reliably *fail*, it silently
        // reads the wrong bytes into the wrong fields.
        let version: u32 = bincode::deserialize(&bytes)
            .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))?;
        let profiles: Vec<Profile> = match version {
            SAVE_FORMAT_VERSION => {
                let payload: SaveFile = bincode::deserialize(&bytes)
                    .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))?;
                payload.profiles
            }
            VERSION_WITH_F32_POSITIONS => {
                // A world from before a position was an `f64`. See
                // `VERSION_WITH_F32_POSITIONS`.
                let payload: SaveFileV10 = bincode::deserialize(&bytes)
                    .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))?;
                payload.profiles.into_iter().map(Profile::from).collect()
            }
            VERSION_WITHOUT_SICKNESS => {
                // A world from before quitting stopped being the cure.
                // See `VERSION_WITHOUT_SICKNESS`.
                let payload: SaveFileV9 = bincode::deserialize(&bytes)
                    .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))?;
                payload.profiles.into_iter().map(Profile::from).collect()
            }
            VERSION_WITHOUT_INJURIES => {
                // A world from before a body had wounds. A leg that was
                // knitting comes back set; see `VERSION_WITHOUT_INJURIES`.
                let payload: SaveFileV8 = bincode::deserialize(&bytes)
                    .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))?;
                payload.profiles.into_iter().map(Profile::from).collect()
            }
            VERSION_WITHOUT_DISCOVERY => {
                // A world from before anybody kept a recipe book. See
                // `VERSION_WITHOUT_DISCOVERY` for what they come back with.
                let payload: SaveFileV7 = bincode::deserialize(&bytes)
                    .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))?;
                payload.profiles.into_iter().map(Profile::from).collect()
            }
            VERSION_WITHOUT_FRACTURES => {
                // A world from before a fall could break anything.
                let payload: SaveFileV6 = bincode::deserialize(&bytes)
                    .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))?;
                payload.profiles.into_iter().map(Profile::from).collect()
            }
            VERSION_WITHOUT_TIREDNESS => {
                // A world from before anybody got tired. Read with the
                // struct that wrote it; everybody comes back rested.
                let payload: SaveFileV5 = bincode::deserialize(&bytes)
                    .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))?;
                payload.profiles.into_iter().map(Profile::from).collect()
            }
            VERSION_WITHOUT_A_BODY => {
                // A world from before anybody was warm, wet or thirsty.
                // Read with the struct that wrote it and upgraded on the
                // way in: nothing worn, comfortable and watered. See
                // `VERSION_WITHOUT_A_BODY`.
                let payload: SaveFileV4 = bincode::deserialize(&bytes)
                    .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))?;
                payload.profiles.into_iter().map(Profile::from).collect()
            }
            VERSION_WITHOUT_WEAR => {
                // A world from before tools wore out. The pack in it has
                // two numbers a slot where this build writes three, so
                // it is read with the frozen shape and every tool comes
                // back unworn.
                let payload: SaveFileV3 = bincode::deserialize(&bytes)
                    .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))?;
                payload.profiles.into_iter().map(Profile::from).collect()
            }
            VERSION_WITHOUT_HUNGER => {
                // A world from before hunger. Read with the struct that
                // wrote it and upgraded on the way in, exactly the way
                // version 1 is -- the file stays as it is until
                // something marks the store dirty and rewrites it.
                let payload: SaveFileV2 = bincode::deserialize(&bytes)
                    .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))?;
                payload.profiles.into_iter().map(Profile::from).collect()
            }
            VERSION_WITHOUT_OPERATORS => {
                // A world from before operators existed. Read with the
                // struct that wrote it and upgraded on the way in; the
                // file itself stays as it is until something marks the
                // store dirty and it is rewritten as version 2.
                let payload: SaveFileV1 = bincode::deserialize(&bytes)
                    .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))?;
                payload.profiles.into_iter().map(Profile::from).collect()
            }
            _ => {
                // Unknown shape -- a file from a newer server, most
                // likely. Starting fresh loses saved packs, which is
                // bad; guessing at the layout corrupts them, which is
                // worse -- and so is writing over them later.
                self.unreadable = true;
                return Ok(0);
            }
        };
        let count = profiles.len();
        for mut profile in profiles {
            // Everything here came off a disk that a user can edit.
            profile.inventory.sanitize();
            self.by_uuid.insert(profile.uuid, profile);
        }
        self.dirty = false;
        Ok(count)
    }
}

/// What a joining player should be given: their identity, and the state
/// to restore.
pub struct Restored {
    pub uuid: Uuid,
    pub inventory: Inventory,
    /// Where to put them: their place of exit, or spawn on a first
    /// visit.
    pub position: (f64, f64, f64),
    pub yaw: f32,
    pub pitch: f32,
    pub health: f32,
    /// How full they were when they left. See `Profile::nourishment`.
    pub nourishment: f32,
    pub selected_slot: usize,
    /// What they had on, and how their body was doing.
    ///
    /// Bundled rather than four more fields for the reason `StoredBody`
    /// exists: it is what actually travels, and the caller copies it
    /// straight onto the runtime.
    pub body: StoredBody,
    /// What they have held. Repaired on the way in -- see
    /// `Discovered::from_kinds`.
    pub discovered: Discovered,
    /// The bags they have still to go back for.
    pub bags: Vec<(i32, i32, i32)>,
    pub returning: bool,
    /// **Where a player saved dead lies**, if they were saved dead: the
    /// joining connection leaves their body there (`leave_corpse_at`).
    ///
    /// A player saved with no health and a full pack is one whose death
    /// was never finished -- down when the server crashed, so the last
    /// autosave caught the body on the ground with everything still on
    /// it. Restoring that as it stood put them at the spawn *with their
    /// pack*, which is a death that cost nothing, and a reason to pull
    /// the plug on a server when you are going down. Leaving the body
    /// where they lay makes the crash the same as leaving while down
    /// (`give_up` on disconnect): dead where they fell, at the spawn with
    /// nothing, with the way back on the map. After an ordinary death the
    /// pack is already empty and this finds nothing to leave.
    pub died_at: Option<(f64, f64, f64)>,
}

/// Whether the saved worn set has a rucksack on its back.
///
/// Its own function because the equipment in a profile is repaired
/// (`Equipment::sanitize`) a few lines further down than the pack is
/// cut, and reading it twice from two places is how the two answers
/// drift apart.
fn pack_is_worn(worn: &primitive_shared::inventory::Equipment) -> bool {
    worn.in_slot(primitive_shared::equipment::Slot::Back).is_some_and(|stack| {
        primitive_shared::equipment::slot_of(stack.block)
            == Some(primitive_shared::equipment::Slot::Back)
    })
}

impl Profiles {
    /// The whole join in one call: look up or create, then hand back
    /// what the connection needs to seed the player with.
    pub fn restore(&mut self, username: &str, spawn: (f32, f32, f32), max_health: f32) -> Restored {
        let profile = self.join(username, spawn, max_health);
        let returning = profile.joins > 1;
        let position = profile.place_of_exit().unwrap_or(primitive_shared::geometry::wide(spawn));
        Restored {
            uuid: profile.uuid,
            // **Cut to the length the worn set says it should be**, which
            // `sanitize` deliberately will not do: it sees an
            // `Inventory` and cannot tell a pack from a chest, so it
            // leaves lengths alone (see the note there). Here the worn
            // set is right beside the pack, so this is the one place that
            // can say whether the ten rucksack squares exist.
            //
            // It matters for one save shape: one written when the pack
            // was forty squares. What will not fit in twenty is folded
            // into whatever is free, and what still does not fit is
            // logged rather than silently dropped -- a player is owed the
            // sentence, even if there is nothing to be done about it.
            inventory: {
                let mut pack = profile.inventory.clone();
                let worn = pack_is_worn(&profile.equipment);
                let lost = pack.fit_to_backpack(worn);
                if !lost.is_empty() {
                    eprintln!(
                        "[profiles] {}: {} stack(s) would not fit the smaller pack and are gone",
                        profile.username,
                        lost.len()
                    );
                }
                pack
            },
            // A player who logged out dead comes back alive: the death
            // screen is not a state to be resumed into, and the
            // alternative is a profile nobody can ever play again.
            health: if profile.health > 0.0 {
                profile.health.min(max_health)
            } else {
                max_health
            },
            // A player who logged out dead comes back fed as well as
            // alive, for the same reason: they are being put back at
            // spawn with nothing, and starting that two minutes from
            // starvation is a punishment for having quit.
            nourishment: if profile.health > 0.0 {
                profile.nourishment
            } else {
                primitive_shared::food::MAX_NOURISHMENT
            },
            // ...and a player who logged out dead comes back at spawn,
            // which is where the respawn they never pressed would have put
            // them. Their place of exit is where they died: this used to
            // hand it back, so quitting on the death screen was a respawn
            // beside your own pack with the walk back skipped -- and a
            // player who drowned on the sea floor came back alive on the
            // sea floor, to drown again.
            position: if returning && profile.health > 0.0 { position } else { primitive_shared::geometry::wide(spawn) },
            yaw: profile.yaw,
            pitch: profile.pitch,
            selected_slot: (profile.selected_slot as usize)
                .min(primitive_shared::inventory::HOTBAR_SLOTS - 1),
            body: StoredBody {
                equipment: {
                    // Repaired on the way in, on the same terms the pack
                    // is: this comes out of a file, and a garment filed
                    // against the wrong body part is a client that would
                    // be told it is wearing boots on its head.
                    let mut worn = profile.equipment.clone();
                    worn.sanitize();
                    worn
                },
                // A player who logged out dead comes back watered and
                // warm as well as alive and fed, for the reason the two
                // above give: they are being put back at spawn with
                // nothing, and starting that parched is a punishment for
                // having quit.
                hydration: if profile.health > 0.0 {
                    profile.hydration
                } else {
                    primitive_shared::body::MAX_HYDRATION
                },
                body_c: if profile.health > 0.0 {
                    profile.body_c
                } else {
                    primitive_shared::body::NEUTRAL_C
                },
                wetness: if profile.health > 0.0 { profile.wetness } else { 0.0 },
                // ...and the same for tiredness: a player who died is
                // rebuilt rested, because dying is not a night's sleep
                // but being put back at spawn with nothing already is
                // punishment enough.
                fatigue: if profile.health > 0.0 { profile.fatigue } else { 0.0 },
                // ...and every wound heals on the way back: dying is a
                // reset of the body, and a player rebuilt at spawn with a
                // leg they broke before it would be carrying an injury from
                // a life that ended. Repaired on the way in otherwise,
                // because it comes off a file.
                injuries: if profile.health > 0.0 {
                    let mut wounds = profile.injuries;
                    wounds.sanitize();
                    wounds
                } else {
                    Injuries::default()
                },
                // Bad water, on the terms tiredness comes back on: a player
                // who logged out ill is still ill, and one who *died* of it
                // is not -- death rebuilds the body, and an illness that
                // outlived the person who caught it would be a countdown
                // nobody could wait out. Clamped on the way in because it
                // comes off a file, where a negative countdown would be an
                // illness with no end.
                sick_for: if profile.health > 0.0 { profile.sick_for.max(0.0) } else { 0.0 },
            },
            // Knowledge and bags survive a death: dying costs what you
            // were carrying, not what you know, and the bags are the way
            // back to what you were carrying.
            discovered: Discovered::from_kinds(profile.discovered.iter().copied()),
            bags: profile.bags.clone(),
            returning,
            died_at: if profile.health > 0.0 { None } else { profile.place_of_exit() },
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use primitive_shared::types::BLOCK_STONE;

    const SPAWN: (f32, f32, f32) = (0.5, 40.0, 0.5);

    #[test]
    fn a_name_always_resolves_to_the_same_uuid() {
        // The whole point: a returning player finds their own pack.
        assert_eq!(Uuid::of_name("shamkhan"), Uuid::of_name("shamkhan"));
        assert_eq!(Uuid::of_name("Shamkhan"), Uuid::of_name("shamkhan"));
        assert_ne!(Uuid::of_name("shamkhan"), Uuid::of_name("shamkhan2"));
        assert_ne!(Uuid::of_name(""), Uuid::of_name("a"));
    }

    #[test]
    fn a_uuid_is_printed_in_the_usual_shape() {
        let text = Uuid::of_name("player").to_string();
        assert_eq!(text.len(), 36, "{text}");
        let groups: Vec<&str> = text.split('-').collect();
        assert_eq!(
            groups.iter().map(|g| g.len()).collect::<Vec<_>>(),
            vec![8, 4, 4, 4, 12],
            "{text}"
        );
        assert!(text.chars().all(|c| c.is_ascii_hexdigit() || c == '-'));
        // Version 3, RFC 4122 variant.
        assert!(groups[2].starts_with('3'), "{text}");
        assert!(
            matches!(groups[3].chars().next(), Some('8'..='9' | 'a' | 'b')),
            "{text}"
        );
    }

    #[test]
    fn a_first_visit_starts_at_spawn_with_nothing() {
        let mut profiles = Profiles::new();
        let restored = profiles.restore("newcomer", SPAWN, 20.0);
        assert!(!restored.returning);
        assert_eq!(restored.position, primitive_shared::geometry::wide(SPAWN));
        assert!(restored.inventory.is_empty());
        assert_eq!(restored.health, 20.0);
    }

    #[test]
    fn coming_back_returns_the_pack_and_the_place_of_exit() {
        let mut profiles = Profiles::new();
        let first = profiles.restore("miner", SPAWN, 20.0);

        let mut inventory = Inventory::new();
        inventory.add(BLOCK_STONE, 40);
        profiles.store(first.uuid, inventory, (100.0, 33.0, -50.0), 1.5, -0.2, 12.0, 20.0, 3, StoredBody::default());

        let again = profiles.restore("miner", SPAWN, 20.0);
        assert!(again.returning);
        assert_eq!(again.uuid, first.uuid);
        assert_eq!(again.position, (100.0, 33.0, -50.0));
        assert_eq!(again.inventory.count(BLOCK_STONE), 40);
        assert_eq!(again.health, 12.0);
        assert_eq!(again.selected_slot, 3);
        assert_eq!(again.yaw, 1.5);
    }

    #[test]
    fn logging_out_dead_is_not_a_way_to_stay_dead() {
        let mut profiles = Profiles::new();
        let first = profiles.restore("unlucky", SPAWN, 20.0);
        profiles.store(first.uuid, Inventory::new(), (5.0, 5.0, 5.0), 0.0, 0.0, 0.0, 20.0, 0, StoredBody::default());
        let again = profiles.restore("unlucky", SPAWN, 20.0);
        assert_eq!(again.health, 20.0, "a stored corpse came back as a corpse");
    }

    #[test]
    fn a_player_who_quit_on_the_death_screen_comes_back_at_spawn_and_not_beside_their_pack() {
        let mut profiles = Profiles::new();
        let first = profiles.restore("drowned", SPAWN, 20.0);
        // Died on the sea floor a long way out, and quit rather than
        // pressing respawn. Their pack is lying there.
        let died_at = (900.0, 12.0, -700.0);
        profiles.store(first.uuid, Inventory::new(), died_at, 0.0, 0.0, 0.0, 20.0, 0, StoredBody::default());
        let again = profiles.restore("drowned", SPAWN, 20.0);
        assert!(again.returning);
        assert_eq!(again.health, 20.0);
        assert_eq!(again.position, primitive_shared::geometry::wide(SPAWN), "quitting on the death screen respawned the player where they died");
    }

    #[test]
    fn a_player_saved_down_with_a_full_pack_is_told_where_their_body_lies() {
        // The server crashed while they were on the ground: the last
        // autosave has no health in it and everything still in the pack.
        let mut profiles = Profiles::new();
        let first = profiles.restore("crashed", SPAWN, 20.0);
        let mut pack = Inventory::new();
        pack.add(BLOCK_STONE, 12);
        let lay_at = (300.5, 40.0, -120.5);
        profiles.store(first.uuid, pack, lay_at, 0.0, 0.0, 0.0, 20.0, 0, StoredBody::default());
        let again = profiles.restore("crashed", SPAWN, 20.0);
        assert_eq!(again.died_at, Some(lay_at), "a death the crash interrupted was not handed back to be finished");
        assert_eq!(again.position, primitive_shared::geometry::wide(SPAWN));
        // ...and a living player has no body to leave.
        profiles.store(first.uuid, Inventory::new(), lay_at, 0.0, 0.0, 12.0, 20.0, 0, StoredBody::default());
        assert_eq!(profiles.restore("crashed", SPAWN, 20.0).died_at, None);
    }

    #[test]
    fn a_nonsense_place_of_exit_falls_back_to_spawn() {
        // The file is on a disk a user can edit, and a position inside
        // rock or outside the world is a death rather than a nuisance.
        let mut profiles = Profiles::new();
        let first = profiles.restore("wanderer", SPAWN, 20.0);
        for bad in [
            (0.0, f64::NAN, 0.0),
            (0.0, -5.0, 0.0),
            (0.0, 1e9, 0.0),
            (f64::INFINITY, 20.0, 0.0),
        ] {
            profiles.store(first.uuid, Inventory::new(), bad, 0.0, 0.0, 20.0, 20.0, 0, StoredBody::default());
            let again = profiles.restore("wanderer", SPAWN, 20.0);
            assert_eq!(again.position, primitive_shared::geometry::wide(SPAWN), "{bad:?} was believed");
        }
    }

    #[test]
    fn a_player_who_logged_out_holding_a_jug_of_grain_comes_back_holding_one() {
        use primitive_shared::inventory::{filled_jug, jug_contents};
        use primitive_shared::types::BLOCK_GRAIN;
        // **The profile format did not have to move for this.**
        // A jug's contents ride in `inventory::Stack::damage` -- the
        // field `VERSION_WITHOUT_WEAR` was introduced to add -- so the
        // store already carries them, and a profile written before jugs
        // held anything reads back as a jug holding nothing, which is
        // the truth about it. Losing a profile is losing everything a
        // player was carrying, and a fourth field on `Stack` would have
        // meant another frozen shape in this file.
        //
        // **What this asserts is that the reader for the version the
        // jug shipped in still exists**, not that the format has stood
        // still. It has moved once since -- tiredness went into
        // `StoredBody` at version 6 -- and that is exactly the change
        // this guard is here to make somebody think about: a new
        // version is fine, a new version *without* the old reader
        // beside it is a world of empty jugs.
        assert_eq!(
            VERSION_WITHOUT_TIREDNESS, 5,
            "the reader for the version the jug shipped in has gone"
        );

        let dir = std::env::temp_dir().join(format!(
            "primitive-profiles-jug-{}",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&dir);

        let mut profiles = Profiles::new();
        let joined = profiles.restore("potter", SPAWN, 20.0);
        let mut inventory = Inventory::new();
        inventory.put_in_slot(4, filled_jug(BLOCK_GRAIN, 13));
        profiles.store(
            joined.uuid,
            inventory,
            primitive_shared::geometry::wide(SPAWN),
            0.0,
            0.0,
            20.0,
            20.0,
            0,
            StoredBody::default(),
        );
        assert_eq!(profiles.save(&dir).expect("save"), Some(1));

        let mut reloaded = Profiles::new();
        assert_eq!(reloaded.load(&dir).expect("load"), 1);
        let restored = reloaded.restore("potter", SPAWN, 20.0);
        assert_eq!(
            jug_contents(&restored.inventory.slots()[4].unwrap()),
            Some((BLOCK_GRAIN, 13)),
            "the grain did not survive the logout"
        );

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn profiles_survive_a_round_trip_through_a_file() {
        let dir = std::env::temp_dir().join(format!(
            "primitive-profiles-{}",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&dir);

        let mut profiles = Profiles::new();
        let joined = profiles.restore("saver", SPAWN, 20.0);
        let mut inventory = Inventory::new();
        inventory.add(BLOCK_STONE, 7);
        profiles.store(joined.uuid, inventory, (12.0, 30.0, 34.0), 0.0, 0.0, 9.0, 20.0, 2, StoredBody::default());
        assert_eq!(profiles.save(&dir).expect("save"), Some(1));
        // Nothing changed since: the file is not rewritten.
        assert_eq!(profiles.save(&dir).expect("save"), None);

        let mut reloaded = Profiles::new();
        assert_eq!(reloaded.load(&dir).expect("load"), 1);
        let restored = reloaded.restore("saver", SPAWN, 20.0);
        assert_eq!(restored.uuid, joined.uuid);
        assert_eq!(restored.inventory.count(BLOCK_STONE), 7);
        assert_eq!(restored.position, (12.0, 30.0, 34.0));
        assert_eq!(restored.health, 9.0);

        let _ = std::fs::remove_dir_all(&dir);
    }

    /// A scratch directory of this test's own, removed on the way in and
    /// on the way out. Never a real world: `saves/` belongs to whoever
    /// is playing.
    fn scratch(tag: &str) -> PathBuf {
        let dir = std::env::temp_dir()
            .join(format!("primitive-profiles-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        dir
    }

    #[test]
    fn nobody_is_an_operator_until_someone_says_so() {
        let mut profiles = Profiles::new();
        let joined = profiles.restore("alice", SPAWN, 20.0);
        assert!(!profiles.is_operator(joined.uuid));

        assert_eq!(
            profiles.set_operator("alice", true),
            OperatorChange::Changed {
                username: "alice".to_string()
            }
        );
        // Immediately, without a save, a reload, or a reconnect: this is
        // the lookup a chat command does on the very next line typed.
        assert!(profiles.is_operator(joined.uuid));
    }

    #[test]
    fn opping_an_operator_says_so_instead_of_pretending_to_work() {
        let mut profiles = Profiles::new();
        profiles.restore("alice", SPAWN, 20.0);
        profiles.set_operator("alice", true);
        assert_eq!(
            profiles.set_operator("alice", true),
            OperatorChange::Unchanged {
                username: "alice".to_string()
            }
        );
        // ...and the same the other way, for someone who never was one.
        profiles.restore("bob", SPAWN, 20.0);
        assert_eq!(
            profiles.set_operator("bob", false),
            OperatorChange::Unchanged {
                username: "bob".to_string()
            }
        );
    }

    #[test]
    fn an_offline_player_can_be_promoted_but_a_nonexistent_one_cannot() {
        let mut profiles = Profiles::new();
        // "Offline" is the normal state of a profile: nothing here knows
        // or cares whether anyone is connected.
        let joined = profiles.restore("absent", SPAWN, 20.0);
        assert_eq!(
            profiles.set_operator("ABSENT", true),
            OperatorChange::Changed {
                username: "absent".to_string()
            },
            "a name is matched the way a UUID is derived: case-blind"
        );
        assert!(profiles.is_operator(joined.uuid));

        // A typo must not become a standing invitation for whoever
        // guesses it.
        assert_eq!(
            profiles.set_operator("abesnt", true),
            OperatorChange::NoSuchPlayer
        );
        assert!(!profiles.is_operator(Uuid::of_name("abesnt")));
    }

    #[test]
    fn being_an_operator_survives_a_restart() {
        let dir = scratch("ops");
        let mut profiles = Profiles::new();
        let alice = profiles.restore("alice", SPAWN, 20.0);
        let bob = profiles.restore("bob", SPAWN, 20.0);
        assert!(matches!(
            profiles.set_operator("alice", true),
            OperatorChange::Changed { .. }
        ));
        assert_eq!(profiles.save(&dir).expect("save"), Some(2));

        let mut reloaded = Profiles::new();
        assert_eq!(reloaded.load(&dir).expect("load"), 2);
        assert!(reloaded.is_operator(alice.uuid), "alice lost her keys");
        assert!(!reloaded.is_operator(bob.uuid), "bob was handed keys");

        // ...and so does losing it.
        assert!(matches!(
            reloaded.set_operator("alice", false),
            OperatorChange::Changed { .. }
        ));
        assert_eq!(reloaded.save(&dir).expect("save"), Some(2));
        let mut again = Profiles::new();
        again.load(&dir).expect("load");
        assert!(!again.is_operator(alice.uuid));

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn granting_operator_marks_the_store_for_saving_and_refusing_does_not() {
        let dir = scratch("ops-dirty");
        let mut profiles = Profiles::new();
        profiles.restore("alice", SPAWN, 20.0);
        profiles.save(&dir).expect("save");

        // A refusal and a no-op must not cost a rewrite of the file.
        profiles.set_operator("nobody", true);
        profiles.set_operator("alice", false);
        assert_eq!(profiles.save(&dir).expect("save"), None);

        profiles.set_operator("alice", true);
        assert_eq!(profiles.save(&dir).expect("save"), Some(1));

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_world_saved_before_operators_existed_still_loads() {
        // The reason `players.bin` has a version 2 at all. bincode does
        // not name its fields, so a version-1 file read as a version-2
        // one would shear every profile after the first; the migration
        // is what keeps a fortnight of somebody's mining in the world.
        let dir = scratch("v1");
        std::fs::create_dir_all(&dir).expect("mkdir");

        // The old shape, built by hand: what the last build actually
        // wrote, two numbers a slot.
        let mut slots: Vec<Option<StackV3>> = (0..primitive_shared::inventory::SLOTS)
            .map(|_| None)
            .collect();
        slots[0] = Some(StackV3 {
            block: BLOCK_STONE,
            count: 33,
        });
        let inventory = InventoryV3 { slots };
        let old = SaveFileV1 {
            version: VERSION_WITHOUT_OPERATORS,
            profiles: vec![
                ProfileV1 {
                    uuid: Uuid::of_name("veteran"),
                    username: "veteran".to_string(),
                    inventory,
                    position: (7.0, 30.0, -9.0),
                    yaw: 0.5,
                    pitch: -0.25,
                    health: 11.0,
                    selected_slot: 4,
                    joins: 12,
                },
                ProfileV1 {
                    uuid: Uuid::of_name("second"),
                    username: "second".to_string(),
                    inventory: InventoryV3 {
                        slots: (0..primitive_shared::inventory::SLOTS).map(|_| None).collect(),
                    },
                    position: (1.0, 20.0, 2.0),
                    yaw: 0.0,
                    pitch: 0.0,
                    health: 20.0,
                    selected_slot: 0,
                    joins: 1,
                },
            ],
        };
        std::fs::write(
            Profiles::path(&dir),
            bincode::serialize(&old).expect("serialize v1"),
        )
        .expect("write");

        let mut profiles = Profiles::new();
        assert_eq!(profiles.load(&dir).expect("load"), 2);
        let restored = profiles.restore("veteran", SPAWN, 20.0);
        assert_eq!(restored.position, (7.0, 30.0, -9.0));
        assert_eq!(restored.inventory.count(BLOCK_STONE), 33);
        assert_eq!(restored.health, 11.0);
        // The second profile is the one that would be shredded if the
        // bytes were read with the wrong struct.
        let second = profiles
            .get(Uuid::of_name("second"))
            .expect("the second profile survived");
        assert_eq!(second.username, "second");
        assert_eq!(second.joins, 1);
        // An upgrade hands nobody the keys.
        assert!(!profiles.is_operator(Uuid::of_name("veteran")));
        assert!(!profiles.is_operator(Uuid::of_name("second")));

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn what_a_player_has_held_and_where_their_bags_lie_survive_a_restart() {
        let dir = scratch("discovery");
        std::fs::create_dir_all(&dir).expect("mkdir");
        let mut profiles = Profiles::new();
        let joined = profiles.restore("wayfarer", SPAWN, 20.0);
        let mut held = Discovered::new();
        held.note(BLOCK_STONE);
        profiles.remember(joined.uuid, &held, &[(12, 40, -3), (-600, 71, 90)]);
        profiles.save(&dir).expect("save");

        let mut after = Profiles::new();
        assert_eq!(after.load(&dir).expect("load"), 1);
        let restored = after.restore("wayfarer", SPAWN, 20.0);
        assert!(restored.discovered.has_held(BLOCK_STONE), "the book was forgotten");
        assert_eq!(restored.bags, vec![(12, 40, -3), (-600, 71, 90)], "the way back was forgotten");

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn remembering_the_same_thing_twice_does_not_rewrite_the_file() {
        let dir = scratch("remember-twice");
        std::fs::create_dir_all(&dir).expect("mkdir");
        let mut profiles = Profiles::new();
        let joined = profiles.restore("steady", SPAWN, 20.0);
        let held = Discovered::from_kinds([BLOCK_STONE]);
        profiles.remember(joined.uuid, &held, &[(1, 2, 3)]);
        assert!(profiles.save(&dir).expect("save").is_some());
        profiles.remember(joined.uuid, &held, &[(1, 2, 3)]);
        assert_eq!(profiles.save(&dir).expect("save"), None, "an unchanged memory was written again");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_version_seven_file_comes_back_with_its_pack_knowing_nothing() {
        let dir = scratch("v7");
        std::fs::create_dir_all(&dir).expect("mkdir");
        let mut inventory = Inventory::new();
        inventory.add(BLOCK_STONE, 9);
        let old = SaveFileV7 {
            version: VERSION_WITHOUT_DISCOVERY,
            profiles: vec![ProfileV7 {
                uuid: Uuid::of_name("elder"),
                username: "elder".to_string(),
                inventory,
                position: (3.0, 50.0, 4.0),
                yaw: 0.0,
                pitch: 0.0,
                health: 14.0,
                selected_slot: 2,
                joins: 5,
                operator: false,
                nourishment: 30.0,
                equipment: Equipment::new(),
                hydration: 20.0,
                body_c: 36.0,
                wetness: 0.0,
                fracture_for: 0.0,
                fatigue: 0.25,
            }],
        };
        std::fs::write(Profiles::path(&dir), bincode::serialize(&old).expect("serialize v7"))
            .expect("write");

        let mut profiles = Profiles::new();
        assert_eq!(profiles.load(&dir).expect("load"), 1);
        let restored = profiles.restore("elder", SPAWN, 20.0);
        assert_eq!(restored.inventory.count(BLOCK_STONE), 9, "the pack was sheared");
        assert_eq!(restored.health, 14.0);
        assert!(restored.discovered.is_empty());
        assert!(restored.bags.is_empty());

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn an_illness_survives_a_reconnect_and_a_death_ends_it() {
        // **"Сделай сохранение отравления".** Quitting to the menu and
        // coming straight back was the cure: free, instant, and available
        // at the one moment being ill costs anything. Death is the other
        // half and is deliberate -- a countdown that outlived the person
        // who caught it is one nobody could wait out.
        let dir = scratch("illness");
        std::fs::create_dir_all(&dir).expect("mkdir");
        let mut profiles = Profiles::new();
        let joined = profiles.restore("the thirsty", SPAWN, 20.0);
        let ill = StoredBody { sick_for: 72.0, ..StoredBody::default() };
        profiles.store(joined.uuid, Inventory::new(), primitive_shared::geometry::wide(SPAWN), 0.0, 0.0, 12.0, 30.0, 0, ill.clone());
        profiles.save(&dir).expect("save");

        let mut reloaded = Profiles::new();
        assert_eq!(reloaded.load(&dir).expect("load"), 1);
        let back = reloaded.restore("the thirsty", SPAWN, 20.0);
        assert_eq!(back.body.sick_for, 72.0, "the illness was cured by the main menu");

        // ...and the one who died of it is rebuilt well, like the body it
        // is: stored dead, restored at spawn with nothing.
        let dead = StoredBody { sick_for: 72.0, ..StoredBody::default() };
        reloaded.store(back.uuid, Inventory::new(), primitive_shared::geometry::wide(SPAWN), 0.0, 0.0, 0.0, 30.0, 0, dead);
        reloaded.save(&dir).expect("save");
        let mut again = Profiles::new();
        assert_eq!(again.load(&dir).expect("load"), 1);
        assert_eq!(again.restore("the thirsty", SPAWN, 20.0).body.sick_for, 0.0, "an illness outlived its host");

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn injuries_survive_a_save_and_a_death_does_not_keep_them() {
        use primitive_shared::injury::{Kind, Part};
        use primitive_shared::types::BLOCK_BANDAGE;
        let dir = scratch("injuries");
        std::fs::create_dir_all(&dir).expect("mkdir");
        let mut wounds = Injuries::default();
        wounds.inflict(Part::RightArm, Kind::Cut, 0.7);
        wounds.treat(Part::RightArm, BLOCK_BANDAGE).expect("a cut takes a bandage");
        wounds.inflict(Part::LeftLeg, Kind::Fracture, 0.4);

        let mut profiles = Profiles::new();
        let joined = profiles.restore("walking wounded", SPAWN, 20.0);
        let body = StoredBody {
            injuries: wounds,
            fatigue: 0.6,
            ..StoredBody::default()
        };
        profiles.store(joined.uuid, Inventory::new(), primitive_shared::geometry::wide(SPAWN), 0.0, 0.0, 12.0, 30.0, 0, body.clone());
        profiles.save(&dir).expect("save");

        let mut reloaded = Profiles::new();
        assert_eq!(reloaded.load(&dir).expect("load"), 1);
        let back = reloaded.restore("walking wounded", SPAWN, 20.0);
        assert_eq!(back.body.injuries, wounds, "the wounds did not survive the file");
        assert_eq!(back.body.fatigue, 0.6, "and neither did the tiredness");

        // ...and a player who logged out dead comes back whole.
        reloaded.store(joined.uuid, Inventory::new(), primitive_shared::geometry::wide(SPAWN), 0.0, 0.0, 0.0, 30.0, 0, body);
        assert!(reloaded.restore("walking wounded", SPAWN, 20.0).body.injuries.is_whole());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_version_eight_file_comes_back_with_its_broken_leg_set() {
        use primitive_shared::injury::{Kind, Part};
        let dir = scratch("v8");
        std::fs::create_dir_all(&dir).expect("mkdir");
        let old = SaveFileV8 {
            version: VERSION_WITHOUT_INJURIES,
            profiles: vec![ProfileV8 {
                uuid: Uuid::of_name("limper"),
                username: "limper".to_string(),
                inventory: Inventory::new(),
                position: (3.0, 50.0, 4.0),
                yaw: 0.0,
                pitch: 0.0,
                health: 14.0,
                selected_slot: 0,
                joins: 2,
                operator: false,
                nourishment: 30.0,
                equipment: Equipment::new(),
                hydration: 20.0,
                body_c: 30.0,
                wetness: 0.0,
                fracture_for: primitive_shared::body::FRACTURE_SECONDS / 2.0,
                fatigue: 0.1,
                discovered: vec![BLOCK_STONE],
                bags: vec![(1, 2, 3)],
            }],
        };
        std::fs::write(Profiles::path(&dir), bincode::serialize(&old).expect("serialize v8"))
            .expect("write");

        let mut profiles = Profiles::new();
        assert_eq!(profiles.load(&dir).expect("load"), 1);
        let restored = profiles.restore("limper", SPAWN, 20.0);
        let leg = restored.body.injuries.wound(Part::LeftLeg, Kind::Fracture);
        assert!((leg.severity - 0.5).abs() < 1e-4, "half a break came back as {}", leg.severity);
        assert!(leg.is_dressed(), "a leg that was knitting came back unset");
        assert_eq!(restored.bags, vec![(1, 2, 3)], "the rest of the profile was sheared");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_file_from_a_newer_server_is_left_alone_rather_than_guessed_at() {
        let dir = scratch("v99");
        std::fs::create_dir_all(&dir).expect("mkdir");
        let payload = SaveFile {
            version: SAVE_FORMAT_VERSION + 1,
            profiles: Vec::new(),
        };
        std::fs::write(
            Profiles::path(&dir),
            bincode::serialize(&payload).expect("serialize"),
        )
        .expect("write");

        let mut profiles = Profiles::new();
        assert_eq!(profiles.load(&dir).expect("load"), 0);
        assert!(profiles.is_empty());

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_file_that_could_not_be_read_is_not_overwritten_by_the_next_save() {
        // "Left alone" has to survive the first player to join, and it did
        // not: the join marks the store dirty, and the autosave replaced a
        // file full of other people's packs with one holding a single empty
        // one. The newer-server case and the corrupt-file case both.
        for (name, bytes) in [
            (
                "v99-then-save",
                bincode::serialize(&SaveFile { version: SAVE_FORMAT_VERSION + 1, profiles: Vec::new() })
                    .expect("serialize"),
            ),
            ("garbage-then-save", vec![7u8, 1, 2]),
        ] {
            let dir = scratch(name);
            std::fs::create_dir_all(&dir).expect("mkdir");
            std::fs::write(Profiles::path(&dir), &bytes).expect("write");

            let mut profiles = Profiles::new();
            let _ = profiles.load(&dir);
            profiles.join("newcomer", SPAWN, 20.0);
            let _ = profiles.save(&dir);

            assert_eq!(
                std::fs::read(Profiles::path(&dir)).expect("the file is gone"),
                bytes,
                "{name}: an unreadable players file was written over"
            );
            let _ = std::fs::remove_dir_all(&dir);
        }
    }

    #[test]
    fn a_missing_file_is_a_new_world_rather_than_an_error() {
        let mut profiles = Profiles::new();
        let dir = std::env::temp_dir().join("primitive-profiles-not-here");
        let _ = std::fs::remove_dir_all(&dir);
        assert_eq!(profiles.load(&dir).expect("load"), 0);
        assert!(profiles.is_empty());
    }
}

