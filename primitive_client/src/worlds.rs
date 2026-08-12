//! Singleplayer worlds: what exists on disk, and how to make and remove
//! one.
//!
//! A world is a directory under `saves/` containing a `world.toml` with
//! its name and seed, alongside whatever the server writes there (the
//! block-edit overlay). The seed lives with the world rather than in the
//! client's settings because it *is* the world: change it and the same
//! saved edits land on completely different terrain.
//!
//! ## Why a metadata file and not just the folder name
//!
//! The folder name has to be safe for a filesystem; the world's name
//! should not have to be. Keeping them apart means a world can be called
//! `My World #2` and live in `my-world-2`, and renaming one later doesn't
//! have to move the other.
//!
//! ## Deleting
//!
//! `delete` removes a directory tree, so it refuses to touch anything
//! that isn't a direct child of the saves root *and* doesn't contain a
//! `world.toml`. A path traversal or a stale entry should fail loudly
//! rather than recursively deleting whatever it happens to point at. The
//! UI asks for confirmation on top of that.

use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};

/// What `world.toml` holds.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
struct WorldMeta {
    name: String,
    /// Absent for a world carried over from before worlds recorded one.
    ///
    /// An `Option` rather than a sentinel because 0 is a perfectly good
    /// seed: someone who types it into the new-world form means it, and
    /// treating it as "unknown" would quietly generate a different
    /// world than the one they asked for.
    #[serde(skip_serializing_if = "Option::is_none")]
    seed: Option<u32>,
    /// Unix seconds, for sorting most-recent-first.
    last_played: u64,
}

impl Default for WorldMeta {
    fn default() -> Self {
        Self {
            name: "World".to_string(),
            seed: None,
            last_played: 0,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct World {
    pub name: String,
    /// `None` for a world from before seeds were recorded; the caller
    /// supplies its configured default in that case.
    pub seed: Option<u32>,
    pub directory: PathBuf,
    pub last_played: u64,
}

impl World {
    /// "never", or a rough age like "3 days ago". Rough on purpose: the
    /// question this answers is "which of these was I last in", and a
    /// timestamp makes that harder to see, not easier.
    pub fn played_description(&self, now: u64) -> String {
        if self.last_played == 0 {
            return "never played".to_string();
        }
        let seconds = now.saturating_sub(self.last_played);
        match seconds {
            0..=59 => "just now".to_string(),
            60..=3599 => format!("{} min ago", seconds / 60),
            3600..=86_399 => format!("{} h ago", seconds / 3600),
            _ => format!("{} d ago", seconds / 86_400),
        }
    }
}

const META: &str = "world.toml";

pub struct Worlds {
    root: PathBuf,
    worlds: Vec<World>,
}

impl Worlds {
    /// Scans the saves root. A missing root is not an error -- it just
    /// means no worlds yet, which is the state every new install is in.
    pub fn load(root: impl Into<PathBuf>) -> Self {
        let root = root.into();
        let mut worlds = Vec::new();

        // The saves root used to *be* the world -- a single folder,
        // configured by path, with no list around it. Someone upgrading
        // still has that path in their settings, and scanning inside it
        // for subfolders would find none: their world would silently
        // vanish from a screen that says it lists every world they have.
        // So a root that is itself a world counts as one.
        if let Some(adopted) = adopt(&root) {
            worlds.push(adopted);
        }

        if let Ok(entries) = std::fs::read_dir(&root) {
            for entry in entries.flatten() {
                let directory = entry.path();
                if !directory.is_dir() {
                    continue;
                }
                match read_meta(&directory) {
                    Some(meta) => worlds.push(World {
                        name: meta.name,
                        seed: meta.seed,
                        directory,
                        last_played: meta.last_played,
                    }),
                    // A directory with no metadata is either from before
                    // worlds had any, or something that isn't ours.
                    // Adopting it beats hiding a world someone can see in
                    // their file manager.
                    None => {
                        if let Some(adopted) = adopt(&directory) {
                            worlds.push(adopted);
                        }
                    }
                }
            }
        }

        worlds.sort_by(|a, b| {
            b.last_played
                .cmp(&a.last_played)
                .then_with(|| a.name.to_lowercase().cmp(&b.name.to_lowercase()))
        });

        Self { root, worlds }
    }

    pub fn list(&self) -> &[World] {
        &self.worlds
    }

    pub fn get(&self, index: usize) -> Option<&World> {
        self.worlds.get(index)
    }

    /// Creates a world and returns its index.
    ///
    /// The directory is derived from the name but never collides: a
    /// second "My World" becomes `my-world-2`, so two worlds with the
    /// same name are two worlds rather than one shared save.
    pub fn create(&mut self, name: &str, seed: u32) -> Result<usize, String> {
        let seed = Some(seed);
        let name = name.trim();
        if name.is_empty() {
            return Err("a name is required".to_string());
        }

        let directory = self.root.join(self.unique_folder(name));
        std::fs::create_dir_all(&directory)
            .map_err(|e| format!("could not create {}: {e}", directory.display()))?;

        let meta = WorldMeta {
            name: name.to_string(),
            seed,
            last_played: 0,
        };
        write_meta(&directory, &meta)?;

        self.worlds.insert(
            0,
            World {
                name: meta.name,
                seed,
                directory,
                last_played: 0,
            },
        );
        Ok(0)
    }

    /// Records that a world was just opened, so it sorts to the top next
    /// time.
    pub fn mark_played(&mut self, index: usize) {
        let now = unix_now();
        let Some(world) = self.worlds.get_mut(index) else {
            return;
        };
        world.last_played = now;
        let meta = WorldMeta {
            name: world.name.clone(),
            seed: world.seed,
            last_played: now,
        };
        if let Err(e) = write_meta(&world.directory, &meta) {
            eprintln!("could not update {}: {e}", world.directory.display());
        }
    }

    /// Deletes a world's directory, permanently.
    ///
    /// Refuses anything that isn't a direct child of the saves root, or
    /// that has no `world.toml` in it. This function removes a directory
    /// tree; it should be impossible to aim it at something that isn't a
    /// world, whatever the caller passes.
    pub fn delete(&mut self, index: usize) -> Result<String, String> {
        let world = self
            .worlds
            .get(index)
            .ok_or_else(|| "no such world".to_string())?
            .clone();

        if world.directory.parent() != Some(self.root.as_path()) {
            return Err(format!(
                "{} is not inside the saves folder",
                world.directory.display()
            ));
        }
        if !world.directory.join(META).is_file() {
            return Err(format!(
                "{} has no {META}; refusing to delete it",
                world.directory.display()
            ));
        }

        std::fs::remove_dir_all(&world.directory)
            .map_err(|e| format!("could not delete {}: {e}", world.directory.display()))?;
        self.worlds.remove(index);
        Ok(world.name)
    }

    fn unique_folder(&self, name: &str) -> String {
        let base = slug(name);
        let mut candidate = base.clone();
        let mut suffix = 2;
        while self.root.join(&candidate).exists() {
            candidate = format!("{base}-{suffix}");
            suffix += 1;
        }
        candidate
    }
}

/// Filesystem-safe folder name: lowercase, ASCII letters and digits,
/// everything else collapsed to a single dash.
fn slug(name: &str) -> String {
    let mut out = String::new();
    let mut pending_dash = false;
    for c in name.chars() {
        if c.is_ascii_alphanumeric() {
            if pending_dash && !out.is_empty() {
                out.push('-');
            }
            pending_dash = false;
            out.push(c.to_ascii_lowercase());
        } else {
            pending_dash = true;
        }
        if out.len() >= 40 {
            break;
        }
    }
    if out.is_empty() {
        "world".to_string()
    } else {
        out
    }
}

fn read_meta(directory: &Path) -> Option<WorldMeta> {
    let text = std::fs::read_to_string(directory.join(META)).ok()?;
    toml::from_str(&text).ok()
}

fn write_meta(directory: &Path, meta: &WorldMeta) -> Result<(), String> {
    let text = toml::to_string_pretty(meta).map_err(|e| e.to_string())?;
    std::fs::write(directory.join(META), text)
        .map_err(|e| format!("could not write {}: {e}", directory.join(META).display()))
}

/// Takes ownership of a directory that looks like a world but has no
/// metadata -- the layout used before worlds were a thing, where there
/// was exactly one and its seed lived in the client's settings.
///
/// The seed is unknowable from here, so it is left at 0 and the caller's
/// configured default is used. That is the honest answer: the old layout
/// genuinely didn't record it.
fn adopt(directory: &Path) -> Option<World> {
    if !directory.join("edits.bin").is_file() {
        return None;
    }
    let name = directory
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("World")
        .to_string();
    Some(World {
        name,
        seed: None,
        directory: directory.to_path_buf(),
        last_played: 0,
    })
}

pub fn unix_now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A temp directory that cleans itself up. Small enough not to be
    /// worth a dev-dependency.
    struct TempDir(PathBuf);

    impl TempDir {
        fn new(label: &str) -> Self {
            let path = std::env::temp_dir().join(format!(
                "primitive-worlds-{label}-{}-{}",
                std::process::id(),
                SystemTime::now()
                    .duration_since(UNIX_EPOCH)
                    .unwrap()
                    .as_nanos()
            ));
            std::fs::create_dir_all(&path).unwrap();
            Self(path)
        }
        fn path(&self) -> &Path {
            &self.0
        }
    }

    impl Drop for TempDir {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    #[test]
    fn a_missing_saves_folder_is_no_worlds_rather_than_an_error() {
        // The state every fresh install is in.
        let worlds = Worlds::load(std::env::temp_dir().join("primitive-definitely-not-here"));
        assert!(worlds.list().is_empty());
    }

    #[test]
    fn a_created_world_can_be_found_again() {
        let dir = TempDir::new("roundtrip");
        let mut worlds = Worlds::load(dir.path());
        worlds.create("My World", 4242).unwrap();

        let reloaded = Worlds::load(dir.path());
        assert_eq!(reloaded.list().len(), 1);
        assert_eq!(reloaded.list()[0].name, "My World");
        assert_eq!(reloaded.list()[0].seed, Some(4242));
    }

    #[test]
    fn the_seed_belongs_to_the_world_not_the_settings() {
        // Two worlds side by side must keep their own terrain. If the
        // seed lived in the client config, opening the second would
        // regenerate the first one's landscape under its saved edits.
        let dir = TempDir::new("seeds");
        let mut worlds = Worlds::load(dir.path());
        worlds.create("Alpha", 1).unwrap();
        worlds.create("Beta", 2).unwrap();

        let reloaded = Worlds::load(dir.path());
        let alpha = reloaded.list().iter().find(|w| w.name == "Alpha").unwrap();
        let beta = reloaded.list().iter().find(|w| w.name == "Beta").unwrap();
        assert_eq!(alpha.seed, Some(1));
        assert_eq!(beta.seed, Some(2));
        assert_ne!(alpha.directory, beta.directory);
    }

    #[test]
    fn two_worlds_with_the_same_name_get_separate_folders() {
        // Otherwise the second one silently opens the first one's save.
        let dir = TempDir::new("collide");
        let mut worlds = Worlds::load(dir.path());
        worlds.create("My World", 1).unwrap();
        worlds.create("My World", 2).unwrap();

        let dirs: Vec<_> = worlds.list().iter().map(|w| w.directory.clone()).collect();
        assert_eq!(dirs.len(), 2);
        assert_ne!(dirs[0], dirs[1]);
    }

    #[test]
    fn awkward_names_still_produce_a_usable_folder() {
        assert_eq!(slug("My World"), "my-world");
        assert_eq!(slug("  spaced  out  "), "spaced-out");
        assert_eq!(slug("../../etc"), "etc");
        assert_eq!(slug("!!!"), "world");
        assert_eq!(slug(""), "world");
        assert!(slug(&"x".repeat(200)).len() <= 40);
    }

    #[test]
    fn a_name_that_is_only_punctuation_does_not_escape_the_saves_folder() {
        // The folder name is derived from user input, so this is the
        // check that matters most.
        let dir = TempDir::new("traversal");
        let mut worlds = Worlds::load(dir.path());
        worlds.create("../../../evil", 1).unwrap();
        let created = &worlds.list()[0].directory;
        assert_eq!(created.parent(), Some(dir.path()));
    }

    #[test]
    fn an_empty_name_is_refused() {
        let dir = TempDir::new("noname");
        let mut worlds = Worlds::load(dir.path());
        assert!(worlds.create("   ", 1).is_err());
        assert!(worlds.list().is_empty());
    }

    #[test]
    fn deleting_removes_the_folder_and_the_entry() {
        let dir = TempDir::new("delete");
        let mut worlds = Worlds::load(dir.path());
        worlds.create("Doomed", 1).unwrap();
        let path = worlds.list()[0].directory.clone();
        assert!(path.is_dir());

        let name = worlds.delete(0).unwrap();
        assert_eq!(name, "Doomed");
        assert!(!path.exists());
        assert!(worlds.list().is_empty());
    }

    #[test]
    fn delete_refuses_a_directory_that_is_not_a_world() {
        // This function removes a tree. Pointing it at something without
        // a world.toml must fail rather than take the folder with it.
        let dir = TempDir::new("guard");
        let stray = dir.path().join("not-a-world");
        std::fs::create_dir_all(&stray).unwrap();
        std::fs::write(stray.join("important.txt"), "keep me").unwrap();

        let mut worlds = Worlds::load(dir.path());
        // Force an entry pointing at it, as a corrupted list would.
        worlds.worlds.push(World {
            name: "fake".to_string(),
            seed: None,
            directory: stray.clone(),
            last_played: 0,
        });

        assert!(worlds.delete(worlds.list().len() - 1).is_err());
        assert!(stray.join("important.txt").is_file(), "it deleted the folder");
    }

    #[test]
    fn delete_refuses_a_path_outside_the_saves_root() {
        let dir = TempDir::new("outside");
        let elsewhere = TempDir::new("elsewhere");
        std::fs::write(elsewhere.path().join(META), "name = \"x\"\nseed = 1\n").unwrap();

        let mut worlds = Worlds::load(dir.path());
        worlds.worlds.push(World {
            name: "escape".to_string(),
            seed: None,
            directory: elsewhere.path().to_path_buf(),
            last_played: 0,
        });

        assert!(worlds.delete(0).is_err());
        assert!(elsewhere.path().is_dir(), "it deleted a folder outside saves");
    }

    #[test]
    fn deleting_an_index_that_does_not_exist_is_an_error_not_a_panic() {
        let dir = TempDir::new("oob");
        let mut worlds = Worlds::load(dir.path());
        assert!(worlds.delete(7).is_err());
    }

    #[test]
    fn the_most_recently_played_world_comes_first() {
        let dir = TempDir::new("order");
        let mut worlds = Worlds::load(dir.path());
        worlds.create("Old", 1).unwrap();
        worlds.create("New", 2).unwrap();
        let new_index = worlds.list().iter().position(|w| w.name == "New").unwrap();
        worlds.mark_played(new_index);

        let reloaded = Worlds::load(dir.path());
        assert_eq!(reloaded.list()[0].name, "New");
    }

    #[test]
    fn zero_is_a_real_seed_and_not_a_missing_one() {
        // The obvious shortcut -- 0 means "unknown" -- would silently
        // generate a different world for anyone who types 0 in.
        let dir = TempDir::new("zeroseed");
        let mut worlds = Worlds::load(dir.path());
        worlds.create("Zero", 0).unwrap();
        assert_eq!(Worlds::load(dir.path()).list()[0].seed, Some(0));
    }

    #[test]
    fn an_adopted_world_still_has_no_seed_after_being_played() {
        // `mark_played` rewrites the metadata; it must not invent a
        // seed for a world whose seed genuinely isn't known.
        let dir = TempDir::new("adopted-played");
        let legacy = dir.path().join("old");
        std::fs::create_dir_all(&legacy).unwrap();
        std::fs::write(legacy.join("edits.bin"), [0u8; 4]).unwrap();

        let mut worlds = Worlds::load(dir.path());
        worlds.mark_played(0);
        assert_eq!(Worlds::load(dir.path()).list()[0].seed, None);
    }

    #[test]
    fn a_world_folder_from_before_this_existed_is_adopted() {
        // The old layout was a single `saves/singleplayer` with no
        // metadata. Hiding it would look like the player's world had
        // been deleted.
        let dir = TempDir::new("legacy");
        let legacy = dir.path().join("singleplayer");
        std::fs::create_dir_all(&legacy).unwrap();
        std::fs::write(legacy.join("edits.bin"), [0u8; 4]).unwrap();

        let worlds = Worlds::load(dir.path());
        assert_eq!(worlds.list().len(), 1);
        assert_eq!(worlds.list()[0].name, "singleplayer");
    }

    #[test]
    fn a_saves_root_that_is_itself_the_old_single_world_is_not_lost() {
        // Someone upgrading still has `saves/singleplayer` in their
        // settings, pointing at the world folder rather than at a folder
        // of worlds. Scanning inside it finds nothing, and the screen
        // would claim they have no worlds at all.
        let dir = TempDir::new("root-world");
        std::fs::write(dir.path().join("edits.bin"), [0u8; 4]).unwrap();

        let worlds = Worlds::load(dir.path());
        assert_eq!(worlds.list().len(), 1);
        assert_eq!(worlds.list()[0].directory, dir.path());
    }

    #[test]
    fn a_normal_saves_root_is_not_adopted_as_a_world_itself() {
        let dir = TempDir::new("normal-root");
        let mut worlds = Worlds::load(dir.path());
        worlds.create("Only", 1).unwrap();
        assert_eq!(Worlds::load(dir.path()).list().len(), 1);
    }

    #[test]
    fn an_unrelated_folder_is_not_mistaken_for_a_world() {
        let dir = TempDir::new("unrelated");
        std::fs::create_dir_all(dir.path().join("screenshots")).unwrap();
        assert!(Worlds::load(dir.path()).list().is_empty());
    }

    #[test]
    fn ages_read_as_something_a_person_would_say() {
        let world = World {
            name: "w".to_string(),
            seed: None,
            directory: PathBuf::new(),
            last_played: 1_000_000,
        };
        assert_eq!(world.played_description(1_000_010), "just now");
        assert_eq!(world.played_description(1_000_000 + 3 * 3600), "3 h ago");
        assert_eq!(world.played_description(1_000_000 + 5 * 86_400), "5 d ago");

        let never = World {
            last_played: 0,
            ..world
        };
        assert_eq!(never.played_description(9_999_999), "never played");
    }

    #[test]
    fn a_clock_that_went_backwards_does_not_produce_nonsense() {
        let world = World {
            name: "w".to_string(),
            seed: None,
            directory: PathBuf::new(),
            last_played: 5_000,
        };
        // `saturating_sub` keeps this at "just now" rather than
        // underflowing into several hundred billion years ago.
        assert_eq!(world.played_description(1_000), "just now");
    }
}
