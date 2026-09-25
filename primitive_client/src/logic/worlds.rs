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
//!
//! ## Copying
//!
//! `copy` is the answer to "I am about to dig under my own house". It is
//! a plain file-by-file duplicate of the directory with a new name and a
//! new folder, and it is **not** a link, a snapshot or a diff: a save is
//! a handful of files a few megabytes at most, and anything cleverer
//! would be a second on-disk format to keep working. The copy keeps the
//! seed, the preset, the zone and the scale -- it has to, or it would be
//! a different world wearing the same name -- and it keeps the clock, so
//! the backup opens on the day it was taken rather than at dawn of day
//! one.
//!
//! ## What the list says about a world
//!
//! The row a player reads is four facts, and three of them are not in
//! `world.toml`: how far the world's own calendar has got, which season
//! that is, and how much disk it takes. The first two come from
//! `clock.txt`, which the *server* writes beside the save (see
//! `primitive_server`'s `save_time_of_day`); the third is a walk of the
//! directory. Both are read at `load` and refreshed by `refresh_facts`
//! rather than measured while the screen is being drawn -- a `build`
//! that touches the filesystem is a frame that stutters when a disk is
//! busy, and this one runs sixty times a second.

use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};

use primitive_shared::season::{self, Season};
use primitive_shared::worldgen::{Preset, Scale, Zone};

use crate::ui::lang::{Language, Msg};

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
    /// Which generator built it. See `worldgen::Preset`.
    ///
    /// Not an `Option`, unlike the seed: a world written before presets
    /// existed was made by the only generator there was, and `Normal` is
    /// exactly that answer. The seed cannot say the same, because the
    /// seed it was made with is genuinely unknown.
    #[serde(default)]
    preset: Preset,
    /// Where on the planet it was laid. See `worldgen::Zone`.
    ///
    /// Not an `Option`, for the preset's reason: a world written before
    /// zones existed woke its player at forty-five degrees, and `Temperate`
    /// is that answer. Its explored chunks keep the banded climate they
    /// were generated in; the country beyond them is the real-scale
    /// temperate zone, and where the two meet a far edge of the old world
    /// can show a seam -- a savanna chunk against a meadow -- which is the
    /// price of a planet that is not twenty kilometres round.
    #[serde(default)]
    zone: Zone,
    /// Which scale its country is drawn at. See `worldgen::Scale`.
    ///
    /// **Absent means regional**, and that is not `Scale::default()`: every
    /// world written before the field existed was drawn at the regional
    /// scale, and its edits -- the only thing a save keeps -- stand on that
    /// ground. Reading it as the Earth's would regenerate every chunk under
    /// the player's buildings. A new world writes `earth` (`create_in`).
    #[serde(default = "Scale::unrecorded")]
    scale: Scale,
    /// Unix seconds, for sorting most-recent-first.
    last_played: u64,
}

impl Default for WorldMeta {
    fn default() -> Self {
        Self {
            name: "World".to_string(),
            seed: None,
            preset: Preset::Normal,
            zone: Zone::Temperate,
            scale: Scale::unrecorded(),
            last_played: 0,
        }
    }
}

/// **`Eq` went when the clock arrived**, and nothing missed it: the
/// world's age is a `f32`, and a float has no total equality. Two worlds
/// are told apart by their directory everywhere it matters.
#[derive(Debug, Clone, PartialEq)]
pub struct World {
    pub name: String,
    /// `None` for a world from before seeds were recorded; the caller
    /// supplies its configured default in that case.
    pub seed: Option<u32>,
    /// Which generator built it: the other half of what makes a world a
    /// world. See `worldgen::Preset`.
    pub preset: Preset,
    /// Where on the planet it was laid: the third thing, beside the seed
    /// and the preset, that its saved edits were written against.
    pub zone: Zone,
    /// Which scale its country is drawn at: the fourth. See `worldgen::Scale`.
    pub scale: Scale,
    pub directory: PathBuf,
    pub last_played: u64,
    /// How far the world's own calendar has got: its age in days, with
    /// the hour in the fraction. `None` for a world that has never been
    /// entered, or one saved before the server wrote a clock down.
    ///
    /// **Read off the server's `clock.txt`, and deliberately not stored
    /// in `world.toml`.** The clock belongs to the running world and is
    /// rewritten on every autosave; a second copy in the client's own
    /// metadata would be a number that is right on the frame it is
    /// written and stale for ever after.
    pub world_time: Option<f32>,
    /// What the save takes on disk, in bytes. Zero for a world whose
    /// directory could not be read, which reads as "nothing to say"
    /// rather than as "empty".
    pub bytes: u64,
}

impl World {
    /// Which day of the world the player left on, counting from one --
    /// what the list calls the time they have lived there.
    ///
    /// `None` rather than "day 1" for a world nobody has opened: a world
    /// with no history should say so, and saying "day 1" about a folder
    /// that has never been entered is a small lie the player has no way
    /// to check.
    pub fn day(&self) -> Option<u32> {
        self.world_time.map(season::day_number)
    }

    /// The season it was left in. See `season::Season`.
    pub fn season(&self) -> Option<Season> {
        self.world_time.map(Season::at)
    }

    /// The calendar date it was last played on, as `dd.mm.yyyy`, or
    /// `None` for a world nobody has opened.
    ///
    /// **A date as well as "3 days ago", because the two answer
    /// different questions.** The age answers "which of these was I last
    /// in", which is why it is what the row leads with; the date answers
    /// "is the backup I took the one from before the flood", which is the
    /// question somebody asks once and needs an exact answer to. A list
    /// of five worlds all saying "2 d ago" is a list that cannot answer
    /// the second one at all.
    ///
    /// UTC, and it is worth saying why rather than leaving it to be
    /// discovered: a local time needs the platform's timezone database,
    /// which on Android means JNI and on a desktop means a crate, for a
    /// line that is read to tell two saves apart. The date can be a day
    /// out for somebody who played near midnight; the ordering it is
    /// read for cannot.
    pub fn played_on(&self) -> Option<String> {
        (self.last_played != 0).then(|| {
            let (year, month, day) = civil_from_unix(self.last_played);
            format!("{day:02}.{month:02}.{year:04}")
        })
    }
}

/// Year, month and day from Unix seconds, in UTC.
///
/// Howard Hinnant's `civil_from_days`, which is the short exact one: no
/// table of month lengths and no loop over years, and correct across the
/// leap rules because it counts from a March-based era. Written out here
/// rather than pulled in, because a date crate is a dependency tree for
/// one line of one screen.
fn civil_from_unix(seconds: u64) -> (u64, u64, u64) {
    let days = seconds / 86_400 + 719_468;
    let era = days / 146_097;
    let day_of_era = days - era * 146_097;
    let year_of_era =
        (day_of_era - day_of_era / 1460 + day_of_era / 36_524 - day_of_era / 146_096) / 365;
    let year = year_of_era + era * 400;
    let day_of_year = day_of_era - (365 * year_of_era + year_of_era / 4 - year_of_era / 100);
    let shifted_month = (5 * day_of_year + 2) / 153;
    let day = day_of_year - (153 * shifted_month + 2) / 5 + 1;
    // The era starts in March, so months 0..=9 are March..December and
    // 10, 11 are the January and February of the *next* year.
    let month = if shifted_month < 10 { shifted_month + 3 } else { shifted_month - 9 };
    (if month <= 2 { year + 1 } else { year }, month, day)
}

/// What a save takes on disk, as a person would say it.
///
/// Two digits of precision and no more: the question is "is this the big
/// one" rather than "how many bytes", and `3.1 MB` answers it where
/// `3 284 129 B` does not. Bytes only under a kilobyte, which is a world
/// nobody has walked in yet.
///
/// The unit is not translated, and that is deliberate: `MB` is what the
/// same number is labelled in every file manager a player has, in all
/// four of these languages, and a translated unit would be the one place
/// in the interface where a familiar number came out in unfamiliar
/// letters.
pub fn size_description(bytes: u64) -> String {
    const KB: f64 = 1024.0;
    let bytes = bytes as f64;
    if bytes < KB {
        return format!("{bytes:.0} B");
    }
    for (limit, unit) in [(KB * KB, "kB"), (KB * KB * KB, "MB")] {
        if bytes < limit {
            let value = bytes / (limit / KB);
            // One decimal under ten, none above: `9.4 MB` and `132 MB`
            // are both three characters of information, and `132.4 MB`
            // is one of them padded out.
            return if value < 10.0 {
                format!("{value:.1} {unit}")
            } else {
                format!("{value:.0} {unit}")
            };
        }
    }
    format!("{:.1} GB", bytes / (KB * KB * KB))
}

impl World {
    /// "never", or a rough age like "3 days ago". Rough on purpose: the
    /// question this answers is "which of these was I last in", and a
    /// timestamp makes that harder to see, not easier.
    ///
    /// Takes the language because the row is read by a player, not a
    /// log: the number is universal, the words around it are not.
    pub fn played_description(&self, now: u64, language: Language) -> String {
        if self.last_played == 0 {
            return language.text(Msg::NeverPlayed).to_string();
        }
        let seconds = now.saturating_sub(self.last_played);
        match seconds {
            0..=59 => language.text(Msg::JustNow).to_string(),
            60..=3599 => format!("{} {}", seconds / 60, language.text(Msg::MinutesAgo)),
            3600..=86_399 => format!("{} {}", seconds / 3600, language.text(Msg::HoursAgo)),
            _ => format!("{} {}", seconds / 86_400, language.text(Msg::DaysAgo)),
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
                        preset: meta.preset,
                        zone: meta.zone,
                        scale: meta.scale,
                        world_time: read_clock(&directory),
                        bytes: directory_bytes(&directory),
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
    ///
    /// Tests only: the new-world form always says where the world is laid,
    /// and a second way in that quietly picks the zone is a way for a later
    /// caller to forget to ask.
    #[cfg(test)]
    pub fn create(&mut self, name: &str, seed: u32, preset: Preset) -> Result<usize, String> {
        self.create_in(name, seed, preset, Zone::default())
    }

    /// Creates a world laid in a zone at the newest generator's scale.
    ///
    /// Tests only, like `create` above it and for the same reason: the
    /// new-world form asks for all five things now (`create_at`), and a
    /// second way in that quietly picks one of them is a way for a later
    /// caller to forget to ask. It kept its name because a dozen tests
    /// say it.
    #[cfg(test)]
    pub fn create_in(&mut self, name: &str, seed: u32, preset: Preset, zone: Zone) -> Result<usize, String> {
        self.create_at(name, seed, preset, zone, Scale::default())
    }

    /// The whole of what a new world is: a name, a seed, a generator, a
    /// place on the planet and the scale its country is drawn at.
    ///
    /// **The scale is on the form now**, and it is the one choice here
    /// that cannot be described as better or worse: the landforms are the
    /// planet at the Earth's size, and the regional world is an
    /// archipelago whose next island is a walk rather than a voyage.
    /// Which of those a player wants is a question about the game they
    /// mean to play, which is exactly what this form is for. The default
    /// is `Scale::default` -- the newest -- so nobody has to answer it.
    pub fn create_at(
        &mut self,
        name: &str,
        seed: u32,
        preset: Preset,
        zone: Zone,
        scale: Scale,
    ) -> Result<usize, String> {
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
            preset,
            zone,
            scale,
            last_played: 0,
        };
        write_meta(&directory, &meta)?;

        self.worlds.insert(
            0,
            World {
                name: meta.name,
                seed,
                preset,
                zone,
                scale,
                world_time: None,
                bytes: directory_bytes(&directory),
                directory,
                last_played: 0,
            },
        );
        Ok(0)
    }

    /// Gives a world a new name, and returns the name it now has.
    ///
    /// **The folder does not move.** A world's name and its folder were
    /// separated on purpose (see the module note), and renaming is the
    /// moment that pays: the save stays exactly where the running server
    /// left it, so a rename cannot be the thing that loses a world. What
    /// it costs is a folder called `my-world` holding a world called
    /// `Дом`, which nobody but the person reading the saves folder ever
    /// sees.
    pub fn rename(&mut self, index: usize, name: &str) -> Result<String, String> {
        let name = name.trim();
        if name.is_empty() {
            return Err("a name is required".to_string());
        }
        let world = self.worlds.get_mut(index).ok_or_else(|| "no such world".to_string())?;
        world.name = name.to_string();
        let meta = WorldMeta {
            name: world.name.clone(),
            seed: world.seed,
            preset: world.preset,
            zone: world.zone,
            scale: world.scale,
            last_played: world.last_played,
        };
        write_meta(&world.directory, &meta)?;
        Ok(name.to_string())
    }

    /// Duplicates a world into a folder of its own, and returns the index
    /// of the copy.
    ///
    /// The insurance a player takes out before flooding their own mine.
    /// See the module note for why it is a plain file copy.
    ///
    /// The copy is *not* marked as played, so it sorts under the original
    /// rather than above it: a backup that jumped to the top of the list
    /// would be the row a thumb lands on next time, which is the one way
    /// this feature could lose somebody their world.
    pub fn copy(&mut self, index: usize, name: &str) -> Result<usize, String> {
        let source = self
            .worlds
            .get(index)
            .ok_or_else(|| "no such world".to_string())?
            .clone();
        let name = name.trim();
        if name.is_empty() {
            return Err("a name is required".to_string());
        }
        let directory = self.root.join(self.unique_folder(name));
        copy_tree(&source.directory, &directory)?;

        let meta = WorldMeta {
            name: name.to_string(),
            seed: source.seed,
            preset: source.preset,
            zone: source.zone,
            scale: source.scale,
            last_played: source.last_played,
        };
        write_meta(&directory, &meta)?;

        let copy = World {
            name: name.to_string(),
            seed: source.seed,
            preset: source.preset,
            zone: source.zone,
            scale: source.scale,
            world_time: read_clock(&directory),
            bytes: directory_bytes(&directory),
            directory,
            last_played: source.last_played,
        };
        // Straight after the world it was taken from, which is where the
        // eye is already looking.
        let at = (index + 1).min(self.worlds.len());
        self.worlds.insert(at, copy);
        Ok(at)
    }

    /// Re-reads the two facts that change while a world is being played:
    /// its calendar and its size on disk.
    ///
    /// **In place, and that is the point.** Reloading the list would be
    /// simpler and would re-sort it -- and the menu addresses worlds by
    /// index, so a list that re-sorted under a highlighted row would aim
    /// DELETE at a different world than the one the player is looking at.
    /// Nothing here adds, removes or reorders anything.
    pub fn refresh_facts(&mut self) {
        for world in &mut self.worlds {
            world.world_time = read_clock(&world.directory);
            world.bytes = directory_bytes(&world.directory);
        }
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
            preset: world.preset,
            zone: world.zone,
            scale: world.scale,
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

/// Copies a directory's files and subdirectories into a new one.
///
/// **Files only, and no links followed.** `std::fs::copy` on a symbolic
/// link copies what it points at, so a save folder with a link in it
/// would otherwise pull the whole of whatever that is into the copy --
/// and a link pointing at its own parent would not stop. What a world is
/// made of is plain files, so anything that is not one is skipped rather
/// than chased.
fn copy_tree(from: &Path, to: &Path) -> Result<(), String> {
    std::fs::create_dir_all(to).map_err(|e| format!("could not create {}: {e}", to.display()))?;
    let entries = std::fs::read_dir(from)
        .map_err(|e| format!("could not read {}: {e}", from.display()))?;
    for entry in entries.flatten() {
        let source = entry.path();
        let target = to.join(entry.file_name());
        // `file_type` does not follow a link; `is_dir`/`is_file` do,
        // which is the difference this is relying on.
        match entry.file_type() {
            Ok(kind) if kind.is_dir() => copy_tree(&source, &target)?,
            Ok(kind) if kind.is_file() => {
                std::fs::copy(&source, &target)
                    .map_err(|e| format!("could not copy {}: {e}", source.display()))?;
            }
            _ => {}
        }
    }
    Ok(())
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
        // The only generator there was when this layout was in use.
        preset: Preset::Normal,
        // ...laid where every world then was.
        zone: Zone::Temperate,
        // ...and drawn at the only scale there was.
        scale: Scale::unrecorded(),
        world_time: read_clock(directory),
        bytes: directory_bytes(directory),
        directory: directory.to_path_buf(),
        last_played: 0,
    })
}

/// What the server left its clock at, in days. See the module note.
///
/// Anything unreadable, missing or nonsensical is `None` rather than a
/// number: the list would rather say nothing about the calendar than say
/// "day 1" about a world that is on day two hundred. The same tolerance
/// the server's own reader has -- a bad clock must never be the reason a
/// save cannot be looked at.
fn read_clock(directory: &Path) -> Option<f32> {
    let text = std::fs::read_to_string(directory.join("clock.txt")).ok()?;
    let days: f32 = text.trim().parse().ok()?;
    (days.is_finite() && days >= 0.0).then_some(days)
}

/// How much disk a save takes, following the directories inside it.
///
/// **Bounded, and that is the whole of the care this needs.** It runs
/// over a folder a player owns, and a player can put anything in a
/// folder -- a symbolic link back to its own parent among them. A walk
/// with no ceiling on it is then a menu that never opens. Ten thousand
/// entries is far past any real save and far short of a hang.
fn directory_bytes(directory: &Path) -> u64 {
    let mut total = 0;
    let mut seen = 0usize;
    let mut stack = vec![directory.to_path_buf()];
    while let Some(next) = stack.pop() {
        let Ok(entries) = std::fs::read_dir(&next) else {
            continue;
        };
        for entry in entries.flatten() {
            seen += 1;
            if seen > 10_000 {
                return total;
            }
            match entry.file_type() {
                Ok(kind) if kind.is_dir() => stack.push(entry.path()),
                Ok(kind) if kind.is_file() => {
                    total += entry.metadata().map(|m| m.len()).unwrap_or(0);
                }
                _ => {}
            }
        }
    }
    total
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
        worlds.create("My World", 4242, Preset::Normal).unwrap();

        let reloaded = Worlds::load(dir.path());
        assert_eq!(reloaded.list().len(), 1);
        assert_eq!(reloaded.list()[0].name, "My World");
        assert_eq!(reloaded.list()[0].seed, Some(4242));
    }

    #[test]
    fn the_world_type_survives_a_round_trip_to_disk() {
        // The preset is written into `world.toml` beside the seed, and
        // it has to come back: a test world that reloaded as an ordinary
        // one would drop the player's buildings onto terrain that never
        // existed, which is the same failure a forgotten seed causes.
        let dir = TempDir::new("presets");
        let mut worlds = Worlds::load(dir.path());
        worlds.create("Lab", 5, Preset::Test).unwrap();
        worlds.create("Home", 5, Preset::Normal).unwrap();

        let reloaded = Worlds::load(dir.path());
        let preset_of = |name: &str| {
            reloaded
                .list()
                .iter()
                .find(|w| w.name == name)
                .unwrap()
                .preset
        };
        assert_eq!(preset_of("Lab"), Preset::Test);
        assert_eq!(preset_of("Home"), Preset::Normal);
    }

    #[test]
    fn a_world_remembers_where_on_the_planet_it_was_laid() {
        // At real scale nobody walks from one zone into another, so the
        // zone a world was made in is its climate for ever -- and a world
        // that forgot it on the next launch would put a tropical player's
        // hut in the snow.
        let dir = TempDir::new("zones");
        let mut worlds = Worlds::load(dir.path());
        worlds.create_in("Coast", 5, Preset::Normal, Zone::Tropics).unwrap();
        worlds.create_in("Home", 5, Preset::Normal, Zone::Temperate).unwrap();
        let zone_of = |name: &str| {
            Worlds::load(dir.path()).list().iter().find(|w| w.name == name).unwrap().zone
        };
        assert_eq!(zone_of("Coast"), Zone::Tropics);
        assert_eq!(zone_of("Home"), Zone::Temperate);
    }

    #[test]
    fn a_world_from_before_zones_reads_as_temperate() {
        // Every world made before the zone existed woke its player at
        // forty-five degrees.
        let dir = TempDir::new("before-zones");
        let old = dir.path().join("old");
        std::fs::create_dir_all(&old).unwrap();
        std::fs::write(old.join(META), "name = \"Old\"\nseed = 3\npreset = \"normal\"\nlast_played = 0\n").unwrap();
        assert_eq!(Worlds::load(dir.path()).list()[0].zone, Zone::Temperate);
    }

    #[test]
    fn a_world_from_before_the_earth_scale_stays_on_the_ground_its_buildings_stand_on() {
        // A save is edits over regenerated chunks. A world that never wrote
        // a scale down was drawn at the regional one, and reading it as the
        // Earth's would put every house in it inside a new hillside -- while
        // a world made now writes the Earth's and keeps it over a reload.
        let dir = TempDir::new("before-scale");
        let old = dir.path().join("old");
        std::fs::create_dir_all(&old).unwrap();
        std::fs::write(old.join(META), "name = \"Old\"\nseed = 3\npreset = \"normal\"\nzone = \"temperate\"\nlast_played = 0\n")
            .unwrap();
        assert_eq!(Worlds::load(dir.path()).list()[0].scale, Scale::Regional);

        let mut worlds = Worlds::load(dir.path());
        worlds.create_in("New", 3, Preset::Normal, Zone::Temperate).unwrap();
        let reloaded = Worlds::load(dir.path());
        let scale_of = |name: &str| reloaded.list().iter().find(|w| w.name == name).unwrap().scale;
        assert_eq!(scale_of("New"), Scale::Landforms);
        assert_eq!(scale_of("Old"), Scale::Regional);
    }

    #[test]
    fn a_world_from_before_the_landforms_keeps_the_earth_generator_over_a_reload() {
        // The same promise one generator later: a world that wrote `earth`
        // was drawn without hills and steppe, and its unexplored chunks have
        // to come out of that generator or they meet the explored ones at a
        // seam (`worldgen::landforms`).
        let dir = TempDir::new("before-landforms");
        let earth = dir.path().join("earth");
        std::fs::create_dir_all(&earth).unwrap();
        std::fs::write(
            earth.join(META),
            "name = \"Earth\"\nseed = 3\npreset = \"normal\"\nzone = \"temperate\"\nscale = \"earth\"\nlast_played = 0\n",
        )
        .unwrap();
        assert_eq!(Worlds::load(dir.path()).list()[0].scale, Scale::Earth);
        let mut worlds = Worlds::load(dir.path());
        worlds.mark_played(0);
        assert_eq!(Worlds::load(dir.path()).list()[0].scale, Scale::Earth, "a reload moved it to the new generator");
    }

    #[test]
    fn a_world_from_before_presets_reads_as_an_ordinary_one() {
        // There was one generator when those worlds were written, and
        // `Normal` is exactly what they were made with -- unlike the
        // seed, which such a world genuinely does not record.
        let dir = TempDir::new("oldworld");
        let directory = dir.path().join("old");
        std::fs::create_dir_all(&directory).unwrap();
        std::fs::write(directory.join(META), "name = \"Old\"
seed = 3
").unwrap();
        assert_eq!(Worlds::load(dir.path()).list()[0].preset, Preset::Normal);
    }

    #[test]
    fn the_seed_belongs_to_the_world_not_the_settings() {
        // Two worlds side by side must keep their own terrain. If the
        // seed lived in the client config, opening the second would
        // regenerate the first one's landscape under its saved edits.
        let dir = TempDir::new("seeds");
        let mut worlds = Worlds::load(dir.path());
        worlds.create("Alpha", 1, Preset::Normal).unwrap();
        worlds.create("Beta", 2, Preset::Normal).unwrap();

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
        worlds.create("My World", 1, Preset::Normal).unwrap();
        worlds.create("My World", 2, Preset::Normal).unwrap();

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
        worlds.create("../../../evil", 1, Preset::Normal).unwrap();
        let created = &worlds.list()[0].directory;
        assert_eq!(created.parent(), Some(dir.path()));
    }

    #[test]
    fn an_empty_name_is_refused() {
        let dir = TempDir::new("noname");
        let mut worlds = Worlds::load(dir.path());
        assert!(worlds.create("   ", 1, Preset::Normal).is_err());
        assert!(worlds.list().is_empty());
    }

    #[test]
    fn deleting_removes_the_folder_and_the_entry() {
        let dir = TempDir::new("delete");
        let mut worlds = Worlds::load(dir.path());
        worlds.create("Doomed", 1, Preset::Normal).unwrap();
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
            preset: Preset::Normal,
            zone: Zone::Temperate,
            scale: Scale::Earth,
            world_time: None,
            bytes: 0,
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
            preset: Preset::Normal,
            zone: Zone::Temperate,
            scale: Scale::Earth,
            world_time: None,
            bytes: 0,
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
        worlds.create("Old", 1, Preset::Normal).unwrap();
        worlds.create("New", 2, Preset::Normal).unwrap();
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
        worlds.create("Zero", 0, Preset::Normal).unwrap();
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
        worlds.create("Only", 1, Preset::Normal).unwrap();
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
            preset: Preset::Normal,
            zone: Zone::Temperate,
            scale: Scale::Earth,
            world_time: None,
            bytes: 0,
            directory: PathBuf::new(),
            last_played: 1_000_000,
        };
        assert_eq!(world.played_description(1_000_010, Language::English), "just now");
        assert_eq!(
            world.played_description(1_000_000 + 3 * 3600, Language::English),
            "3 h ago"
        );
        assert_eq!(
            world.played_description(1_000_000 + 5 * 86_400, Language::English),
            "5 d ago"
        );
        // ...in whichever language the interface is in.
        assert_eq!(
            world.played_description(1_000_010, Language::Russian),
            "только что"
        );

        let never = World {
            last_played: 0,
            ..world
        };
        assert_eq!(never.played_description(9_999_999, Language::English), "never played");
    }

    #[test]
    fn a_clock_that_went_backwards_does_not_produce_nonsense() {
        let world = World {
            name: "w".to_string(),
            seed: None,
            preset: Preset::Normal,
            zone: Zone::Temperate,
            scale: Scale::Earth,
            world_time: None,
            bytes: 0,
            directory: PathBuf::new(),
            last_played: 5_000,
        };
        // `saturating_sub` keeps this at "just now" rather than
        // underflowing into several hundred billion years ago.
        assert_eq!(world.played_description(1_000, Language::English), "just now");
    }

    #[test]
    fn the_list_reads_the_day_and_the_season_off_the_clock_the_server_left() {
        // The calendar is the one thing a player remembers a world by
        // -- "the one where I had just got through the first winter" --
        // and it is in the server's file, not in the client's metadata.
        let dir = TempDir::new("clock");
        let mut worlds = Worlds::load(dir.path());
        worlds.create("Farm", 1, Preset::Normal).unwrap();
        let path = worlds.list()[0].directory.clone();
        // Day 94 with the hour in the fraction: past the first winter
        // and into the second spring, which is what the row should say.
        std::fs::write(path.join("clock.txt"), "93.5\n").unwrap();

        let reloaded = Worlds::load(dir.path());
        assert_eq!(reloaded.list()[0].day(), Some(94));
        assert_eq!(reloaded.list()[0].season(), Some(Season::at(93.5)));
    }

    #[test]
    fn a_world_nobody_has_opened_says_nothing_about_a_calendar() {
        // Rather than "day 1", which is a claim about a world that has
        // not been entered -- see `World::day`.
        let dir = TempDir::new("no-clock");
        let mut worlds = Worlds::load(dir.path());
        worlds.create("Fresh", 1, Preset::Normal).unwrap();
        let reloaded = Worlds::load(dir.path());
        assert_eq!(reloaded.list()[0].day(), None);
        assert_eq!(reloaded.list()[0].season(), None);
        assert_eq!(reloaded.list()[0].played_on(), None);
    }

    #[test]
    fn a_nonsense_clock_is_no_calendar_rather_than_a_refusal_to_list_the_world() {
        // The server's own reader is this forgiving, and for the same
        // reason: a bad clock must never be why a save cannot be seen.
        let dir = TempDir::new("bad-clock");
        let mut worlds = Worlds::load(dir.path());
        worlds.create("Odd", 1, Preset::Normal).unwrap();
        let path = worlds.list()[0].directory.clone();
        std::fs::write(path.join("clock.txt"), "полдень\n").unwrap();
        let reloaded = Worlds::load(dir.path());
        assert_eq!(reloaded.list().len(), 1);
        assert_eq!(reloaded.list()[0].day(), None);
    }

    #[test]
    fn the_size_of_a_save_is_what_is_actually_in_its_folder() {
        let dir = TempDir::new("size");
        let mut worlds = Worlds::load(dir.path());
        worlds.create("Heavy", 1, Preset::Normal).unwrap();
        let path = worlds.list()[0].directory.clone();
        std::fs::write(path.join("edits.bin"), vec![0u8; 4096]).unwrap();
        std::fs::create_dir_all(path.join("chunks")).unwrap();
        std::fs::write(path.join("chunks/a.bin"), vec![0u8; 2048]).unwrap();

        let reloaded = Worlds::load(dir.path());
        // The metadata file is in there too, so this is a floor rather
        // than an equality -- what matters is that the subdirectory was
        // followed and that nothing was counted twice.
        let bytes = reloaded.list()[0].bytes;
        assert!(bytes >= 6144, "the walk missed a file: {bytes}");
        assert!(bytes < 6144 + 1024, "the walk counted something twice: {bytes}");
    }

    #[test]
    fn a_size_reads_as_a_person_would_say_it() {
        assert_eq!(size_description(0), "0 B");
        assert_eq!(size_description(900), "900 B");
        assert_eq!(size_description(4096), "4.0 kB");
        assert_eq!(size_description(3_300_000), "3.1 MB");
        assert_eq!(size_description(140_000_000), "134 MB");
    }

    #[test]
    fn a_date_is_the_day_it_actually_was() {
        // Three dates chosen to catch the two things a hand-written
        // calendar gets wrong: the leap day, and the January that
        // belongs to the year after the March the era counts from.
        assert_eq!(civil_from_unix(0), (1970, 1, 1));
        assert_eq!(civil_from_unix(951_782_400), (2000, 2, 29));
        assert_eq!(civil_from_unix(1_735_689_600), (2025, 1, 1));
    }

    #[test]
    fn renaming_keeps_the_world_where_the_server_left_it() {
        // The folder is what a running server has open. A rename that
        // moved it would be the one edit to a save that could lose one.
        let dir = TempDir::new("rename");
        let mut worlds = Worlds::load(dir.path());
        worlds.create("Before", 7, Preset::Normal).unwrap();
        let path = worlds.list()[0].directory.clone();

        worlds.rename(0, "  После  ").unwrap();
        assert_eq!(worlds.list()[0].name, "После");
        assert_eq!(worlds.list()[0].directory, path);

        let reloaded = Worlds::load(dir.path());
        assert_eq!(reloaded.list()[0].name, "После");
        assert_eq!(reloaded.list()[0].seed, Some(7), "the rename rewrote the seed");
    }

    #[test]
    fn an_empty_rename_is_refused_and_changes_nothing() {
        let dir = TempDir::new("rename-empty");
        let mut worlds = Worlds::load(dir.path());
        worlds.create("Keep", 1, Preset::Normal).unwrap();
        assert!(worlds.rename(0, "   ").is_err());
        assert_eq!(worlds.list()[0].name, "Keep");
    }

    #[test]
    fn a_copy_is_the_same_world_in_a_folder_of_its_own() {
        // The insurance: everything about the world has to survive, or
        // the backup is a different world with the same name in it.
        let dir = TempDir::new("copy");
        let mut worlds = Worlds::load(dir.path());
        worlds.create_at("Mine", 4242, Preset::Test, Zone::Tropics, Scale::Earth).unwrap();
        let source = worlds.list()[0].directory.clone();
        std::fs::write(source.join("edits.bin"), vec![9u8; 64]).unwrap();
        std::fs::write(source.join("clock.txt"), "40.25\n").unwrap();

        let at = worlds.copy(0, "Mine (backup)").unwrap();
        let copy = worlds.list()[at].clone();
        assert_ne!(copy.directory, source, "the copy shares the original's folder");
        assert_eq!(copy.seed, Some(4242));
        assert_eq!(copy.preset, Preset::Test);
        assert_eq!(copy.zone, Zone::Tropics);
        assert_eq!(copy.scale, Scale::Earth, "the copy moved to a different generator");
        assert_eq!(copy.day(), Some(41), "the backup opened at dawn of day one");
        assert_eq!(std::fs::read(copy.directory.join("edits.bin")).unwrap(), vec![9u8; 64]);

        // ...and it is still there on the next launch, under its own name.
        let reloaded = Worlds::load(dir.path());
        assert_eq!(reloaded.list().len(), 2);
        assert!(reloaded.list().iter().any(|w| w.name == "Mine (backup)"));
        assert!(reloaded.list().iter().any(|w| w.name == "Mine"));
    }

    #[test]
    fn a_copy_does_not_jump_above_the_world_it_was_taken_from() {
        // A backup that sorted to the top of the list would be the row a
        // thumb lands on next time, which is how this feature could lose
        // somebody the world it was meant to protect.
        let dir = TempDir::new("copy-order");
        let mut worlds = Worlds::load(dir.path());
        worlds.create("Home", 1, Preset::Normal).unwrap();
        worlds.mark_played(0);
        let at = worlds.copy(0, "Home 2").unwrap();
        assert_eq!(at, 1);
        assert_eq!(worlds.list()[0].name, "Home");
    }

    #[test]
    fn refreshing_the_facts_moves_no_row() {
        // The menu addresses worlds by index. A refresh that re-sorted
        // would aim DELETE at a world the player is not looking at.
        let dir = TempDir::new("refresh");
        let mut worlds = Worlds::load(dir.path());
        worlds.create("First", 1, Preset::Normal).unwrap();
        worlds.create("Second", 2, Preset::Normal).unwrap();
        let order: Vec<String> = worlds.list().iter().map(|w| w.name.clone()).collect();

        let path = worlds.list()[1].directory.clone();
        std::fs::write(path.join("clock.txt"), "12.0\n").unwrap();
        worlds.mark_played(0);
        worlds.refresh_facts();

        assert_eq!(
            worlds.list().iter().map(|w| w.name.clone()).collect::<Vec<_>>(),
            order,
            "a refresh reordered the list under the selection"
        );
        assert_eq!(worlds.list()[1].day(), Some(13));
    }
}
