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

use primitive_shared::inventory::Inventory;

const SAVE_FORMAT_VERSION: u32 = 2;
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
    pub position: (f32, f32, f32),
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
}

impl Profile {
    fn new(uuid: Uuid, username: &str, spawn: (f32, f32, f32), health: f32) -> Self {
        Self {
            uuid,
            username: username.to_string(),
            inventory: Inventory::new(),
            position: spawn,
            yaw: 0.0,
            pitch: 0.0,
            health,
            selected_slot: 0,
            joins: 0,
            operator: false,
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
    fn place_of_exit(&self) -> Option<(f32, f32, f32)> {
        let (x, y, z) = self.position;
        let sane = x.is_finite()
            && y.is_finite()
            && z.is_finite()
            && y >= 0.0
            && y < primitive_shared::types::CHUNK_SIZE_Y as f32;
        sane.then_some((x, y, z))
    }
}

#[derive(Serialize, Deserialize)]
struct SaveFile {
    version: u32,
    profiles: Vec<Profile>,
}

/// `players.bin` as version 1 wrote it: `Profile` without `operator`.
///
/// A copy of the old struct rather than something clever with optional
/// fields, because a copy is the only thing that stays correct: it is
/// frozen at the shape those bytes were written in, and no future edit
/// to `Profile` can accidentally change how an existing file is read.
/// It costs one struct that is never touched again, and it is what
/// keeps a world made last week loading this week.
#[derive(Serialize, Deserialize)]
struct ProfileV1 {
    uuid: Uuid,
    username: String,
    inventory: Inventory,
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
            inventory: old.inventory,
            position: old.position,
            yaw: old.yaw,
            pitch: old.pitch,
            health: old.health,
            selected_slot: old.selected_slot,
            joins: old.joins,
            // Upgrading a world does not hand anyone the keys.
            operator: false,
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
    pub fn store(
        &mut self,
        uuid: Uuid,
        inventory: Inventory,
        position: (f32, f32, f32),
        yaw: f32,
        pitch: f32,
        health: f32,
        selected_slot: u8,
    ) {
        let Some(profile) = self.by_uuid.get_mut(&uuid) else {
            return;
        };
        profile.inventory = inventory;
        profile.position = position;
        profile.yaw = yaw;
        profile.pitch = pitch;
        profile.health = health;
        profile.selected_slot = selected_slot;
        self.dirty = true;
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
        if !self.dirty {
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
    pub fn load(&mut self, dir: &Path) -> std::io::Result<usize> {
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
                // worse.
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
    pub position: (f32, f32, f32),
    pub yaw: f32,
    pub pitch: f32,
    pub health: f32,
    pub selected_slot: usize,
    pub returning: bool,
}

impl Profiles {
    /// The whole join in one call: look up or create, then hand back
    /// what the connection needs to seed the player with.
    pub fn restore(&mut self, username: &str, spawn: (f32, f32, f32), max_health: f32) -> Restored {
        let profile = self.join(username, spawn, max_health);
        let returning = profile.joins > 1;
        let position = profile.place_of_exit().unwrap_or(spawn);
        Restored {
            uuid: profile.uuid,
            inventory: profile.inventory.clone(),
            // A player who logged out dead comes back alive: the death
            // screen is not a state to be resumed into, and the
            // alternative is a profile nobody can ever play again.
            health: if profile.health > 0.0 {
                profile.health.min(max_health)
            } else {
                max_health
            },
            position: if returning { position } else { spawn },
            yaw: profile.yaw,
            pitch: profile.pitch,
            selected_slot: (profile.selected_slot as usize)
                .min(primitive_shared::inventory::HOTBAR_SLOTS - 1),
            returning,
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
        assert_eq!(restored.position, SPAWN);
        assert!(restored.inventory.is_empty());
        assert_eq!(restored.health, 20.0);
    }

    #[test]
    fn coming_back_returns_the_pack_and_the_place_of_exit() {
        let mut profiles = Profiles::new();
        let first = profiles.restore("miner", SPAWN, 20.0);

        let mut inventory = Inventory::new();
        inventory.add(BLOCK_STONE, 40);
        profiles.store(first.uuid, inventory, (100.0, 33.0, -50.0), 1.5, -0.2, 12.0, 3);

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
        profiles.store(first.uuid, Inventory::new(), (5.0, 5.0, 5.0), 0.0, 0.0, 0.0, 0);
        let again = profiles.restore("unlucky", SPAWN, 20.0);
        assert_eq!(again.health, 20.0, "a stored corpse came back as a corpse");
    }

    #[test]
    fn a_nonsense_place_of_exit_falls_back_to_spawn() {
        // The file is on a disk a user can edit, and a position inside
        // rock or outside the world is a death rather than a nuisance.
        let mut profiles = Profiles::new();
        let first = profiles.restore("wanderer", SPAWN, 20.0);
        for bad in [
            (0.0, f32::NAN, 0.0),
            (0.0, -5.0, 0.0),
            (0.0, 1e9, 0.0),
            (f32::INFINITY, 20.0, 0.0),
        ] {
            profiles.store(first.uuid, Inventory::new(), bad, 0.0, 0.0, 20.0, 0);
            let again = profiles.restore("wanderer", SPAWN, 20.0);
            assert_eq!(again.position, SPAWN, "{bad:?} was believed");
        }
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
        profiles.store(joined.uuid, inventory, (12.0, 30.0, 34.0), 0.0, 0.0, 9.0, 2);
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

        let mut inventory = Inventory::new();
        inventory.add(BLOCK_STONE, 33);
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
                    inventory: Inventory::new(),
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
    fn a_missing_file_is_a_new_world_rather_than_an_error() {
        let mut profiles = Profiles::new();
        let dir = std::env::temp_dir().join("primitive-profiles-not-here");
        let _ = std::fs::remove_dir_all(&dir);
        assert_eq!(profiles.load(&dir).expect("load"), 0);
        assert!(profiles.is_empty());
    }
}

