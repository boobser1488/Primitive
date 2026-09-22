//! `mod.ron`: what a mod says about itself before any of its code runs.
//!
//! ## Why RON and not TOML
//!
//! The server's own configuration is TOML, and so are the scripted
//! plugins' manifests, so this being different needs a reason. It has
//! one: a mod manifest carries **settings whose types the host does not
//! know**. A mod declares `settings: { "cave_scale": 1.4, "biomes":
//! ["desert", "tundra"], "seeded": true }`, and the host's job is to
//! carry those to the mod intact rather than to understand them.
//!
//! TOML makes that awkward -- everything is a table, arrays are
//! homogeneous, and there is no way to write a tagged value. RON is
//! Rust's own data notation: it has enums, tuples, nested structures and
//! unambiguous types, and a mod author reading their own manifest is
//! reading something shaped like the Rust they wrote the mod in.
//!
//! ## What is in one
//!
//! ```text
//! (
//!     name: "bigger_caves",
//!     version: "1.2.0",
//!     api: (major: 1, minor: 0),
//!     description: "Wider tunnels and more of them",
//!     authors: ["someone"],
//!     enabled: true,
//!     library: Auto,
//!     dependencies: [
//!         (name: "core_utils", at_least: "1.0.0", optional: false),
//!     ],
//!     load_after: ["terrain_tweaks"],
//!     resources: [
//!         (kind: Texture, name: "glow_moss", path: "textures/glow_moss.png"),
//!     ],
//!     settings: {
//!         "tunnel_width": F64(0.09),
//!         "rooms": Bool(true),
//!     },
//! )
//! ```
//!
//! Every field but `name` and `version` has a default, so the shortest
//! useful manifest is two lines.
//!
//! ## Why `library: Auto`
//!
//! A mod ships one library per platform and they are named differently
//! -- `bigger_caves.dll` on Windows, `libbigger_caves.so` on Linux,
//! `libbigger_caves.dylib` on macOS. `Auto` means "work it out from my
//! name and the platform you are on", which is what every mod wants and
//! saves a manifest that is wrong on two platforms out of three.
//! [`LibrarySpec::Named`] is the escape hatch for a mod whose file is
//! called something else.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::ApiVersion;

/// The file a mod folder must contain.
pub const MANIFEST_FILE: &str = "mod.ron";

/// A mod's declaration of itself.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ModManifest {
    /// The identity everything else is keyed on: the settings, the save
    /// blob, and other mods' dependencies. Must match what the library
    /// returns from its entry point, and the host refuses the mod if it
    /// does not -- a mod whose manifest and library disagree about its
    /// own name would have its settings and its saved state filed under
    /// two different keys.
    pub name: String,
    /// The mod's own version, as `major.minor.patch`. Compared against
    /// other mods' `at_least`; see [`Dependency`].
    pub version: String,
    /// The API version the library was built against.
    ///
    /// Declared here as well as returned by the entry point so the host
    /// can refuse an incompatible mod **without loading its code**.
    /// That is the whole reason this field exists: `dlopen` runs a
    /// library's initialisers, and a mod built against a different major
    /// version of the contract is exactly the mod you do not want to
    /// have run anything before you find out.
    #[serde(default = "default_api")]
    pub api: ManifestVersion,
    #[serde(default)]
    pub description: String,
    #[serde(default)]
    pub authors: Vec<String>,
    /// Off switch that does not involve deleting anything.
    #[serde(default = "default_true")]
    pub enabled: bool,
    #[serde(default)]
    pub library: LibrarySpec,
    /// Other mods this one needs. Missing required ones are a refusal
    /// with a reason; missing optional ones are a log line.
    #[serde(default)]
    pub dependencies: Vec<Dependency>,
    /// Mods this one wants loaded first, without needing them.
    ///
    /// Distinct from `dependencies` on purpose: "I extend it if it is
    /// there" and "I do not work without it" are different claims, and a
    /// manifest that could only say the second would make every soft
    /// integration into a hard requirement.
    #[serde(default)]
    pub load_after: Vec<String>,
    /// Files the mod ships. The host does not read most of these -- what
    /// it does is *resolve* them, so a mod asking for its own texture
    /// gets a path rather than having to guess where it was installed.
    #[serde(default)]
    pub resources: Vec<Resource>,
    /// Whatever the mod wants to configure, in the mod's own vocabulary.
    ///
    /// A `BTreeMap` rather than a `HashMap` so the order is stable,
    /// which matters for exactly one thing: the host logs what it loaded
    /// and two identical installs should produce identical logs.
    #[serde(default)]
    pub settings: BTreeMap<String, SettingValue>,
}

/// The API version, in a shape RON writes readably.
#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub struct ManifestVersion {
    pub major: u16,
    pub minor: u16,
}

impl From<ManifestVersion> for ApiVersion {
    fn from(v: ManifestVersion) -> Self {
        ApiVersion::new(v.major, v.minor)
    }
}

fn default_api() -> ManifestVersion {
    ManifestVersion {
        major: crate::API_VERSION.major,
        minor: crate::API_VERSION.minor,
    }
}

fn default_true() -> bool {
    true
}

/// Which file to load.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub enum LibrarySpec {
    /// `<name>.dll` / `lib<name>.so` / `lib<name>.dylib`, beside the
    /// manifest. What almost every mod wants.
    #[default]
    Auto,
    /// A specific file, relative to the mod's folder.
    Named(String),
}

impl LibrarySpec {
    /// Where the library is, given the folder and the mod's name.
    ///
    /// The platform naming is done here rather than by the loader so
    /// that a manifest can be checked -- and this function tested --
    /// without a `dlopen` anywhere.
    pub fn resolve(&self, folder: &Path, name: &str) -> PathBuf {
        match self {
            LibrarySpec::Named(file) => folder.join(file),
            LibrarySpec::Auto => folder.join(platform_library_name(name)),
        }
    }
}

/// What a native library is called on this platform.
///
/// Split out and public because it is the one piece of this file a mod's
/// own build script wants: a mod that copies its artefact into place
/// needs the same answer the host will look for.
pub fn platform_library_name(name: &str) -> String {
    if cfg!(target_os = "windows") {
        format!("{name}.dll")
    } else if cfg!(target_os = "macos") {
        format!("lib{name}.dylib")
    } else {
        format!("lib{name}.so")
    }
}

/// Another mod this one needs.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Dependency {
    pub name: String,
    /// The lowest version that will do, as `major.minor.patch`. Empty
    /// means any.
    #[serde(default)]
    pub at_least: String,
    /// Whether the absence is fatal.
    #[serde(default)]
    pub optional: bool,
}

/// Something the mod ships beside its library.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Resource {
    pub kind: ResourceKind,
    /// What the mod calls it.
    pub name: String,
    /// Where it is, relative to the mod's folder.
    pub path: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ResourceKind {
    Texture,
    Model,
    Sound,
    /// Anything the host has no opinion about. The mod reads it itself.
    Data,
}

/// One setting, in the mod's own vocabulary.
///
/// Tagged rather than inferred, which is most of why this file is RON.
/// `1` and `1.0` are different things to a mod that means a count and a
/// mod that means a scale, and a manifest format that guessed would
/// eventually guess wrong on somebody's machine and not on the author's.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum SettingValue {
    Bool(bool),
    I64(i64),
    F64(f64),
    Text(String),
    List(Vec<SettingValue>),
    Map(BTreeMap<String, SettingValue>),
}

impl SettingValue {
    /// A one-line rendering, which is what the host hands a mod through
    /// [`crate::CoreApi::setting`].
    ///
    /// Deliberately *not* a serialiser for the whole tree: a mod asking
    /// for a setting by key almost always wants a scalar, and the two
    /// that do not get RON back, which is the format they wrote it in.
    pub fn to_text(&self) -> String {
        match self {
            SettingValue::Bool(v) => v.to_string(),
            SettingValue::I64(v) => v.to_string(),
            SettingValue::F64(v) => v.to_string(),
            SettingValue::Text(v) => v.clone(),
            other => ron::ser::to_string(other).unwrap_or_default(),
        }
    }
}

/// What went wrong reading one.
#[derive(Debug)]
pub enum ManifestError {
    Missing(PathBuf),
    Unreadable(std::io::Error),
    Malformed(String),
    /// The manifest parsed and says something impossible.
    Invalid(String),
}

impl std::fmt::Display for ManifestError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ManifestError::Missing(path) => write!(f, "no {} at {}", MANIFEST_FILE, path.display()),
            ManifestError::Unreadable(e) => write!(f, "could not read the manifest: {e}"),
            ManifestError::Malformed(e) => write!(f, "the manifest is not valid RON: {e}"),
            ManifestError::Invalid(why) => write!(f, "{why}"),
        }
    }
}

impl std::error::Error for ManifestError {}

impl ModManifest {
    /// Reads the manifest in a mod's folder.
    pub fn read(folder: &Path) -> Result<ModManifest, ManifestError> {
        let path = folder.join(MANIFEST_FILE);
        let text = match std::fs::read_to_string(&path) {
            Ok(text) => text,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
                return Err(ManifestError::Missing(path))
            }
            Err(e) => return Err(ManifestError::Unreadable(e)),
        };
        Self::parse(&text)
    }

    /// The same, from text. Separate so it can be tested without a
    /// filesystem, which is most of what the tests below do.
    pub fn parse(text: &str) -> Result<ModManifest, ManifestError> {
        let manifest: ModManifest =
            ron::from_str(text).map_err(|e| ManifestError::Malformed(e.to_string()))?;
        manifest.validate()?;
        Ok(manifest)
    }

    /// The checks a manifest has to pass before the host will look at
    /// the library beside it.
    ///
    /// All of them are about *identity*, because identity is what the
    /// rest of the loader keys on: a nameless mod cannot be depended on,
    /// a mod whose name has a path separator in it would write its save
    /// blob somewhere it should not, and a mod that depends on itself is
    /// a cycle the sort will report as a deadlock later and could
    /// report as a mistake now.
    fn validate(&self) -> Result<(), ManifestError> {
        if self.name.trim().is_empty() {
            return Err(ManifestError::Invalid("a mod with no name".to_string()));
        }
        if !self
            .name
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-')
        {
            return Err(ManifestError::Invalid(format!(
                "'{}' is not a usable mod name: letters, digits, '_' and '-' only",
                self.name
            )));
        }
        if self.version.trim().is_empty() {
            return Err(ManifestError::Invalid(format!(
                "'{}' does not say what version it is",
                self.name
            )));
        }
        if let Some(dep) = self.dependencies.iter().find(|d| d.name == self.name) {
            return Err(ManifestError::Invalid(format!(
                "'{}' depends on itself (as '{}')",
                self.name, dep.name
            )));
        }
        Ok(())
    }

    /// The API version this mod was built against.
    pub fn api_version(&self) -> ApiVersion {
        self.api.into()
    }

    /// Where the library is.
    pub fn library_path(&self, folder: &Path) -> PathBuf {
        self.library.resolve(folder, &self.name)
    }

    /// A resource by name, resolved against the mod's folder.
    pub fn resource(&self, folder: &Path, name: &str) -> Option<PathBuf> {
        self.resources
            .iter()
            .find(|r| r.name == name)
            .map(|r| folder.join(&r.path))
    }
}

/// Compares two `major.minor.patch` strings.
///
/// Its own five lines rather than a semver dependency, because what is
/// needed is "is this at least that" over three integers, and anything
/// missing or unparseable counts as zero -- a mod whose version is
/// `"nightly"` sorts below everything, which is the safe direction for a
/// dependency check to be wrong in.
pub fn version_at_least(have: &str, want: &str) -> bool {
    if want.trim().is_empty() {
        return true;
    }
    let parse = |s: &str| -> (u64, u64, u64) {
        let mut parts = s.trim().split('.').map(|p| p.trim().parse::<u64>().unwrap_or(0));
        (
            parts.next().unwrap_or(0),
            parts.next().unwrap_or(0),
            parts.next().unwrap_or(0),
        )
    };
    parse(have) >= parse(want)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_shortest_useful_manifest_is_two_fields() {
        let manifest = ModManifest::parse(r#"(name: "tiny", version: "0.1.0")"#).expect("parse");
        assert_eq!(manifest.name, "tiny");
        assert!(manifest.enabled, "a mod without an `enabled` line is on");
        assert!(manifest.dependencies.is_empty());
        assert_eq!(manifest.api_version(), crate::API_VERSION);
    }

    #[test]
    fn a_full_manifest_round_trips() {
        let text = r#"(
            name: "bigger_caves",
            version: "1.2.0",
            api: (major: 1, minor: 0),
            description: "Wider tunnels",
            authors: ["someone"],
            enabled: true,
            library: Auto,
            dependencies: [ (name: "core_utils", at_least: "1.0.0", optional: false) ],
            load_after: ["terrain_tweaks"],
            resources: [ (kind: Texture, name: "moss", path: "textures/moss.png") ],
            settings: { "tunnel_width": F64(0.09), "rooms": Bool(true) },
        )"#;
        let manifest = ModManifest::parse(text).expect("parse");
        assert_eq!(manifest.dependencies.len(), 1);
        assert_eq!(manifest.load_after, ["terrain_tweaks"]);
        assert_eq!(
            manifest.settings.get("tunnel_width"),
            Some(&SettingValue::F64(0.09))
        );
        assert_eq!(
            manifest.resource(Path::new("mods/bigger_caves"), "moss"),
            Some(PathBuf::from("mods/bigger_caves").join("textures/moss.png"))
        );
    }

    #[test]
    fn a_setting_keeps_the_type_it_was_written_as() {
        // The whole reason this file is RON. `1` and `1.0` mean
        // different things to the mod that reads them, and a format that
        // guessed would guess wrong on somebody else's machine.
        let manifest = ModManifest::parse(
            r#"(name: "t", version: "1", settings: { "count": I64(1), "scale": F64(1.0) })"#,
        )
        .expect("parse");
        assert_eq!(manifest.settings["count"], SettingValue::I64(1));
        assert_eq!(manifest.settings["scale"], SettingValue::F64(1.0));
        assert_eq!(manifest.settings["count"].to_text(), "1");
        assert_eq!(manifest.settings["scale"].to_text(), "1");
    }

    #[test]
    fn a_nameless_or_self_depending_mod_is_refused_before_its_code_runs() {
        // Both are refusals rather than warnings, and the reason is that
        // the loader keys everything on the name: a mod with none cannot
        // be depended on or given a save blob, and one that depends on
        // itself is a cycle better reported here than as a stall later.
        assert!(ModManifest::parse(r#"(name: "", version: "1")"#).is_err());
        assert!(ModManifest::parse(r#"(name: "x", version: "")"#).is_err());
        assert!(ModManifest::parse(
            r#"(name: "loop", version: "1", dependencies: [(name: "loop")])"#
        )
        .is_err());
        // ...and a name that would escape its own folder.
        assert!(ModManifest::parse(r#"(name: "../etc", version: "1")"#).is_err());
        assert!(ModManifest::parse(r#"(name: "a/b", version: "1")"#).is_err());
    }

    #[test]
    fn the_library_name_follows_the_platform() {
        let folder = Path::new("mods/thing");
        let auto = LibrarySpec::Auto.resolve(folder, "thing");
        let expected = if cfg!(target_os = "windows") {
            "thing.dll"
        } else if cfg!(target_os = "macos") {
            "libthing.dylib"
        } else {
            "libthing.so"
        };
        assert_eq!(auto.file_name().unwrap().to_str().unwrap(), expected);
        // ...and the escape hatch takes the name as written.
        let named = LibrarySpec::Named("odd_name.bin".to_string()).resolve(folder, "thing");
        assert_eq!(named, folder.join("odd_name.bin"));
    }

    #[test]
    fn version_comparison_orders_by_number_and_not_by_string() {
        // "10" < "9" as strings, which is the bug this exists to avoid.
        assert!(version_at_least("1.10.0", "1.9.0"));
        assert!(version_at_least("2.0.0", "1.99.99"));
        assert!(!version_at_least("1.0.0", "1.0.1"));
        assert!(version_at_least("1.0.0", "1.0.0"));
        // ...anything at all satisfies "no requirement".
        assert!(version_at_least("nonsense", ""));
        // ...and an unparseable version satisfies nothing but zero.
        assert!(!version_at_least("nightly", "0.0.1"));
    }

    #[test]
    fn a_malformed_manifest_says_so_rather_than_panicking() {
        let broken = ModManifest::parse("this is not ron at all {{{");
        assert!(matches!(broken, Err(ManifestError::Malformed(_))));
        assert!(broken.unwrap_err().to_string().contains("valid RON"));
    }
}
