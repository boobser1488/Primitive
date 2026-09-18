//! Loading native mods, and being the host they talk to.
//!
//! ## What this is and what `plugins` is
//!
//! Two extension points, for different things. A **plugin** is a Rhai
//! script: no build step, an operation limit, and a broken one is a log
//! line. A **mod** is a compiled library: no limit, native speed, and a
//! broken one takes the process with it. See
//! [`primitive_modapi`] for the contract itself.
//!
//! Both are driven from the same place -- [`crate::fire_event`] -- so
//! there is one list of events and one order they happen in, rather than
//! two systems that gradually stop agreeing about when a block is
//! broken.
//!
//! ## The load, in order
//!
//! 1. **Read every `mod.ron`.** Nothing is `dlopen`ed yet, which is the
//!    point: a manifest declares the API version its library was built
//!    against, so a mod that cannot possibly work is refused *before*
//!    its initialisers run.
//! 2. **Resolve dependencies.** Required ones that are missing or too
//!    old are a refusal with a reason; optional ones are a log line.
//! 3. **Sort.** A topological order over `dependencies` and
//!    `load_after`, so a mod that extends another is loaded after it. A
//!    cycle is reported and every mod in it is skipped -- the
//!    alternative is picking an arbitrary order and letting the
//!    resulting bug be somebody else's afternoon.
//! 4. **Open, register, load.** `dlopen`, find the one exported symbol,
//!    call it, check what it says about itself, then call `on_load`.
//!
//! ## The one rule about locks, and how it is enforced
//!
//! **No lock may be held while a mod is called** -- and the one that
//! matters most is this module's own. A mod's handler is allowed to call
//! straight back into the host; that is what the API is *for*. So a
//! `dispatch` that held `Context::mods` across the call would deadlock
//! the first time a mod subscribed from `on_load`, registered a command
//! from a hook, or ran `/save` -- all of which reach back through
//! `logic::api_impl` and lock the same mutex.
//!
//! That is not a hypothetical: the first version of this module did hold
//! it, and the integration test in `primitive_server/tests/mods.rs`
//! caught it as "the mod loaded but never subscribed" -- which is what a
//! deadlock looks like when the lock happens to be a different instance.
//!
//! The shape that fixes it is the same in both places:
//!
//! 1. lock, **copy out** the handful of function pointers to call;
//! 2. drop the lock;
//! 3. call, with [`CURRENT`] naming which mod is on the stack so that a
//!    re-entrant API call knows who is asking;
//! 4. lock again and apply whatever the calls asked for.
//!
//! [`CURRENT`] is a thread-local and is saved and restored around every
//! call, so a mod whose handler triggers another event -- by running a
//! command, say -- nests correctly rather than crediting the inner
//! mod's subscriptions to the outer one.
//!
//! ## Why the whole thing is behind a feature
//!
//! **Not for the reason `plugins` is, and it used to be.** This said
//! that a local world has no operator to install mods for and no
//! business `dlopen`ing anything in the game's own process, and the
//! client compiled this crate without the feature at all. The first
//! half was wrong about who installs a mod -- it is the person playing,
//! on their own machine, into a folder they can see -- and the second
//! half is a description of what a mod *is*, not an argument against
//! having one. Singleplayer loads mods now; the client asks for this
//! feature back by name.
//!
//! What the feature is still for is **Android**. A mod is a `.so` a
//! player drops into a folder, and an APK has no such folder an
//! ordinary person can reach, so the phone build asks for neither this
//! module nor `libloading`; the call sites go through the
//! `notify_mods!` macro in `lib.rs`, whose other arm expands to `true`.
//!
//! The `plugins` feature keeps its own answer, unchanged: a plugin is a
//! script a *server operator* installs for players who did not ask for
//! it, and a local world neither needs one nor links the engine that
//! would run it.

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};

use primitive_modapi::manifest::{version_at_least, ModManifest};
use primitive_modapi::{
    ApiVersion, Event, EventData, HookResult, HostApi, ModDescriptor, RegisterFn, Status,
};

/// What the host tells the operator about a load.
///
/// Strings rather than an error type, because every one of them ends up
/// on the console and the caller has nothing to decide.
pub type Report = Vec<String>;

// ---------------------------------------------------------------------
// The real one.
// ---------------------------------------------------------------------

/// The server state a mod's API calls reach through.
///
/// A newtype over the `Arc<Context>` rather than the `Arc` itself,
/// because the pointer stored in `HostApi::handle` has to be stable for
/// the whole life of the load and this is the thing that owns it.
pub struct HostContext(pub std::sync::Arc<crate::Context>);

impl Clone for HostContext {
    fn clone(&self) -> Self {
        HostContext(std::sync::Arc::clone(&self.0))
    }
}

/// One loaded mod.
pub struct LoadedMod {
    pub name: String,
    pub version: String,
    pub description: String,
    /// Who wrote it, straight out of the manifest.
    ///
    /// **Kept even though nothing in the server reads it**, because the
    /// extensions screen does, and the alternative was re-reading
    /// `mod.ron` from disk to answer a question the loader already had
    /// the answer to -- off a folder the operator may since have
    /// changed, which is how a screen ends up disagreeing with the
    /// process it is describing.
    pub authors: Vec<String>,
    pub folder: PathBuf,
    pub api_version: ApiVersion,
    /// What it asked to hear about. A mod that subscribes to nothing is
    /// never called, which is what makes a dozen loaded mods cost
    /// nothing on a tick none of them cares about.
    subscriptions: HashSet<i32>,
    descriptor: ModDescriptor,
    /// **Dropped last, and that is load-bearing.** Unloading the library
    /// invalidates every function pointer in `descriptor`, so the
    /// library has to outlive it -- which the field order in this struct
    /// is what guarantees, because Rust drops fields in declaration
    /// order.
    _library: libloading::Library,
    errors: u32,
    disabled: bool,
}

/// How many times a mod may return from a hook badly before it stops
/// being called.
///
/// Native code cannot "return badly" the way a script can -- there is no
/// exception to catch -- so this counts the one thing that is
/// observable: a hook that reported a failure through the API. It is
/// belt and braces; the real protection against a bad mod is not
/// installing it.
const MAX_ERRORS: u32 = 32;

thread_local! {
    /// Which mod is on this thread's stack right now, as an index into
    /// `ModHost::mods`.
    ///
    /// **A thread-local rather than a field**, because the field would
    /// have to be read while the host is locked and the whole point of
    /// the shape above is that the host is *not* locked while a mod
    /// runs. It is saved and restored around every call, so a nested
    /// dispatch -- a mod whose handler runs a command that fires another
    /// event -- credits each mod's own calls to itself.
    static CURRENT: std::cell::Cell<usize> = const { std::cell::Cell::new(usize::MAX) };
}

/// Sets `CURRENT` for the length of a call and puts it back afterwards.
///
/// A guard rather than two statements, because the thing in the middle
/// is a call into somebody else's code and "and then set it back" is
/// exactly the line that gets skipped by an early return.
struct Calling(usize);

impl Calling {
    fn mod_at(index: usize) -> Self {
        let previous = CURRENT.with(|c| c.replace(index));
        Calling(previous)
    }
}

impl Drop for Calling {
    fn drop(&mut self) {
        CURRENT.with(|c| c.set(self.0));
    }
}

/// One mod's handler, copied out so it can be called with no lock held.
#[derive(Clone, Copy)]
struct Listener {
    index: usize,
    on_event: unsafe extern "C" fn(
        *mut std::ffi::c_void,
        Event,
        *const EventData,
    ) -> HookResult,
    user: *mut std::ffi::c_void,
}

pub struct ModHost {
    mods: Vec<LoadedMod>,
    /// Kept alive for as long as any mod is loaded: every `HostApi`
    /// handed out points into this.
    tables: Option<Box<HostTables>>,
    /// Set by an API call from inside a hook.
    pending_subscriptions: Vec<(usize, i32)>,
    /// Every loaded mod's settings, by mod name. Read by
    /// `CoreApi::setting`.
    settings: HashMap<String, std::collections::BTreeMap<String, primitive_modapi::manifest::SettingValue>>,
    /// Each mod's own saved blob. Written by `SaveApi::store`, handed
    /// back by `SaveApi::load`, and persisted beside the world -- see
    /// `mods.bin`.
    blobs: HashMap<String, Vec<u8>>,
    /// Whether any blob changed since the last save, so an idle server
    /// writes nothing.
    blobs_dirty: bool,
    /// Commands mods have claimed, and the help line for each.
    commands: HashMap<String, String>,
    /// Chunk decorators, in load order. See
    /// `GenerationApi::register_decorator`.
    decorators: Vec<primitive_modapi::ChunkDecorator>,
}

// **Why this is sound, stated rather than assumed.**
//
// `ModHost` holds raw pointers: a `ModDescriptor` full of function
// pointers per mod, and a `HostTables` full of them shared between all
// of them. Rust will not infer `Send`/`Sync` through those, and it is
// right not to -- a raw pointer says nothing about what it points at.
//
// What makes it safe here is three facts, all of them enforced
// structurally rather than by convention:
//
// 1. **Everything pointed at outlives the pointer.** A mod's function
//    pointers live in its library, which is a field of the same struct
//    declared *after* the descriptor, so the descriptor is dropped
//    first. `HostTables` is a `Box` this struct owns and clears in
//    `unload_all`.
// 2. **Nothing is reachable without the mutex.** The only `ModHost` in
//    the process is `Context::mods`, behind a `std::sync::Mutex`, so
//    two threads never touch one at once.
// 3. **The pointers are addresses, not thread-affine handles.** A
//    `dlopen`ed function may be called from any thread; the mod's own
//    state behind `descriptor.user` is the mod's problem, and the
//    contract says so.
//
// The one thing this does *not* claim is that a badly written mod is
// safe. It is not, and no amount of marker traits would make it so --
// which is the note at the top of `primitive_modapi` about blast radius.
unsafe impl Send for ModHost {}
unsafe impl Sync for ModHost {}

impl Default for ModHost {
    fn default() -> Self {
        Self::new()
    }
}

impl ModHost {
    pub fn new() -> Self {
        Self {
            mods: Vec::new(),
            tables: None,
            pending_subscriptions: Vec::new(),
            settings: HashMap::new(),
            blobs: HashMap::new(),
            blobs_dirty: false,
            commands: HashMap::new(),
            decorators: Vec::new(),
        }
    }

    // ---- what the API tables reach ----
    //
    // Everything below is called from `logic::api_impl`, from inside a
    // mod's own call. `current` is the mod that is asking; outside a
    // hook it is `usize::MAX`, and every one of these answers "no" to
    // that rather than panicking on an index.

    /// The name of the mod currently being called, if any.
    ///
    /// Reads the thread-local rather than a field: the host is not
    /// locked while a mod runs, so "who is calling" cannot live here.
    fn current_name(&self) -> Option<&str> {
        let index = CURRENT.with(|c| c.get());
        self.mods.get(index).map(|m| m.name.as_str())
    }

    /// A setting out of the current mod's manifest.
    pub fn setting(&self, key: &str) -> Option<String> {
        let name = self.current_name()?;
        Some(self.settings.get(name)?.get(key)?.to_text())
    }

    /// Records a mod's settings at load time.
    pub fn settings_for(
        &mut self,
        name: &str,
        settings: std::collections::BTreeMap<String, primitive_modapi::manifest::SettingValue>,
    ) {
        self.settings.insert(name.to_string(), settings);
    }

    pub fn subscribe_current(&mut self, event: Event) -> bool {
        let current = CURRENT.with(|c| c.get());
        if current == usize::MAX {
            return false;
        }
        self.pending_subscriptions.push((current, event as i32));
        true
    }

    pub fn unsubscribe_current(&mut self, event: Event) -> bool {
        let current = CURRENT.with(|c| c.get());
        if current == usize::MAX {
            return false;
        }
        let key = event as i32;
        if let Some(m) = self.mods.get_mut(current) {
            m.subscriptions.remove(&key);
        }
        self.pending_subscriptions
            .retain(|&(index, k)| index != current || k != key);
        true
    }

    /// Claims a command name for whichever mod is asking.
    ///
    /// First come, first served: a second mod claiming a taken name is
    /// ignored rather than overriding, because the alternative is that
    /// load order silently decides whose `/home` a player gets.
    pub fn register_command(&mut self, name: String, help: String) {
        self.commands.entry(name).or_insert(help);
    }

    /// Every claimed command, for the help text.
    pub fn commands(&self) -> Vec<(String, String)> {
        let mut all: Vec<(String, String)> = self
            .commands
            .iter()
            .map(|(k, v)| (k.clone(), v.clone()))
            .collect();
        all.sort();
        all
    }

    /// Whether a mod has claimed this command name.
    pub fn claims_command(&self, name: &str) -> bool {
        self.commands.contains_key(name)
    }

    pub fn store_blob(&mut self, bytes: Vec<u8>) -> bool {
        let Some(name) = self.current_name().map(|s| s.to_string()) else {
            return false;
        };
        self.blobs.insert(name, bytes);
        self.blobs_dirty = true;
        true
    }

    pub fn load_blob(&self) -> Option<&Vec<u8>> {
        self.blobs.get(self.current_name()?)
    }

    /// Everything worth writing, if anything changed.
    pub fn take_dirty_blobs(&mut self) -> Vec<(String, Vec<u8>)> {
        if !self.blobs_dirty {
            return Vec::new();
        }
        self.blobs_dirty = false;
        let mut all: Vec<(String, Vec<u8>)> = self
            .blobs
            .iter()
            .map(|(k, v)| (k.clone(), v.clone()))
            .collect();
        // Stable bytes for the same world, so a save with nothing
        // changed produces an identical file.
        all.sort_by(|a, b| a.0.cmp(&b.0));
        all
    }

    pub fn restore_blobs(&mut self, blobs: Vec<(String, Vec<u8>)>) {
        self.blobs = blobs.into_iter().collect();
        self.blobs_dirty = false;
    }

    pub fn add_decorator(&mut self, decorator: primitive_modapi::ChunkDecorator) {
        self.decorators.push(decorator);
    }

    pub fn has_decorators(&self) -> bool {
        !self.decorators.is_empty()
    }

    /// The registered decorators, copied out so they can be called with
    /// no lock held.
    ///
    /// Lock, copy out, drop -- the same three lines `listeners` is, and
    /// for the same reason: a decorator is allowed to call straight back
    /// into the host, and a host that held this mutex across the call
    /// would deadlock the first time one asked for a setting. That is
    /// not hypothetical; it is the bug documented at the top of this
    /// module, in the one shape it can still take.
    fn decorators(&self) -> Vec<primitive_modapi::ChunkDecorator> {
        self.decorators.clone()
    }

    pub fn active_count(&self) -> usize {
        self.mods.iter().filter(|m| !m.disabled).count()
    }

    pub fn names(&self) -> Vec<String> {
        self.mods
            .iter()
            .map(|m| format!("{} {}{}", m.name, m.version, if m.disabled { " (disabled)" } else { "" }))
            .collect()
    }

    /// Whether anything at all wants this event.
    ///
    /// Asked before an `EventData` is even built, because building one
    /// means borrowing strings and reading state that nothing is going
    /// to look at.
    pub fn wants(&self, event: Event) -> bool {
        let key = event as i32;
        self.mods
            .iter()
            .any(|m| !m.disabled && m.subscriptions.contains(&key))
    }

    /// Loads every mod folder under `dir`. See the module note for the
    /// order and why it is that order.
    pub fn load_dir(&mut self, dir: &Path, host: HostContext) -> Report {
        let mut report = Report::new();

        let entries = match std::fs::read_dir(dir) {
            Ok(entries) => entries,
            Err(_) => {
                report.push(format!("no mod directory at {}", dir.display()));
                return report;
            }
        };
        let mut folders: Vec<PathBuf> = entries
            .filter_map(|e| e.ok())
            .map(|e| e.path())
            .filter(|p| p.is_dir())
            .collect();
        folders.sort(); // a deterministic starting order under the sort

        // ---- 1. read the manifests ----
        let mut found: Vec<(PathBuf, ModManifest)> = Vec::new();
        for folder in folders {
            match ModManifest::read(&folder) {
                Ok(manifest) => {
                    if !manifest.enabled {
                        report.push(format!("'{}' is disabled in its manifest", manifest.name));
                        continue;
                    }
                    // Refused here, before `dlopen` runs a single one of
                    // the library's initialisers. See the module note.
                    let built_for = manifest.api_version();
                    if !primitive_modapi::API_VERSION.accepts(built_for) {
                        report.push(format!(
                            "'{}' was built against mod API {built_for} and this server is {} -- not loaded",
                            manifest.name,
                            primitive_modapi::API_VERSION
                        ));
                        continue;
                    }
                    found.push((folder, manifest));
                }
                Err(e) => report.push(format!("{}: {e}", folder.display())),
            }
        }

        // ---- 2. dependencies ----
        let have: HashMap<String, String> = found
            .iter()
            .map(|(_, m)| (m.name.clone(), m.version.clone()))
            .collect();
        found.retain(|(_, manifest)| {
            for dep in &manifest.dependencies {
                match have.get(&dep.name) {
                    Some(version) if version_at_least(version, &dep.at_least) => {}
                    Some(version) => {
                        if dep.optional {
                            report.push(format!(
                                "'{}' would like '{}' {} and found {version}; carrying on without it",
                                manifest.name, dep.name, dep.at_least
                            ));
                            continue;
                        }
                        report.push(format!(
                            "'{}' needs '{}' {} and found {version} -- not loaded",
                            manifest.name, dep.name, dep.at_least
                        ));
                        return false;
                    }
                    None => {
                        if dep.optional {
                            continue;
                        }
                        report.push(format!(
                            "'{}' needs '{}', which is not installed -- not loaded",
                            manifest.name, dep.name
                        ));
                        return false;
                    }
                }
            }
            true
        });

        // ---- 3. sort ----
        let (order, cycles) = topological_order(&found);
        for name in cycles {
            report.push(format!(
                "'{name}' is in a dependency cycle and was not loaded"
            ));
        }

        // ---- 4. open and register ----
        //
        // The tables are built once and live as long as the host does.
        // Every mod gets a pointer to the *same* `HostApi`, which is
        // correct: they are all talking to one server.
        let tables = Box::new(HostTables::new(host));
        let api: *const HostApi = &tables.api;
        self.tables = Some(tables);

        for index in order {
            let (folder, manifest) = &found[index];
            match self.open_one(folder, manifest, api) {
                Ok(()) => report.push(format!(
                    "loaded mod '{}' {} (API {})",
                    manifest.name,
                    manifest.version,
                    manifest.api_version()
                )),
                Err(e) => report.push(format!("mod '{}' failed: {e}", manifest.name)),
            }
        }

        report
    }

    /// The handlers to call for an event, copied out of the host.
    ///
    /// The whole point is that this is all the *lock* is needed for:
    /// three words per interested mod, and then the calls happen with
    /// nothing held. See the note at the top of the module.
    fn listeners(&self, event: Event) -> Vec<Listener> {
        let key = event as i32;
        self.mods
            .iter()
            .enumerate()
            .filter(|(_, m)| !m.disabled && m.subscriptions.contains(&key))
            .filter_map(|(index, m)| {
                Some(Listener {
                    index,
                    on_event: m.descriptor.on_event?,
                    user: m.descriptor.user,
                })
            })
            .collect()
    }

    /// The `HostApi` every mod was handed, for the calls made outside
    /// the lock.
    fn api(&self) -> Option<*const HostApi> {
        self.tables.as_ref().map(|t| &t.api as *const HostApi)
    }

    fn on_load_of(&self, index: usize) -> Option<
        unsafe extern "C" fn(*mut std::ffi::c_void, *const HostApi) -> Status,
    > {
        self.mods.get(index)?.descriptor.on_load
    }

    fn on_unload_of(&self, index: usize) -> Option<unsafe extern "C" fn(*mut std::ffi::c_void)> {
        self.mods.get(index)?.descriptor.on_unload
    }

    fn user_of(&self, index: usize) -> *mut std::ffi::c_void {
        self.mods
            .get(index)
            .map(|m| m.descriptor.user)
            .unwrap_or(std::ptr::null_mut())
    }

    fn name_of(&self, index: usize) -> String {
        self.mods
            .get(index)
            .map(|m| m.name.clone())
            .unwrap_or_default()
    }

    fn count(&self) -> usize {
        self.mods.len()
    }

    fn disable(&mut self, index: usize) {
        if let Some(m) = self.mods.get_mut(index) {
            m.disabled = true;
        }
    }

    /// Mods that subscribed to nothing and will therefore never be
    /// called. Worth saying out loud: it is almost always a mistake.
    fn silent(&self) -> Vec<String> {
        self.mods
            .iter()
            .filter(|m| !m.disabled && m.subscriptions.is_empty())
            .map(|m| m.name.clone())
            .collect()
    }

    /// Everything loaded, for `unload_all`.
    fn take_all(&mut self) -> Vec<LoadedMod> {
        self.decorators.clear();
        self.tables = None;
        std::mem::take(&mut self.mods)
    }

    fn open_one(
        &mut self,
        folder: &Path,
        manifest: &ModManifest,
        api: *const HostApi,
    ) -> Result<(), String> {
        let path = manifest.library_path(folder);
        if !path.exists() {
            return Err(format!("no library at {}", path.display()));
        }
        // Safety: this runs the library's initialisers, which is exactly
        // as dangerous as running any native code, and is why the
        // manifest's API version was checked above. There is no way to
        // make `dlopen` safe; what there is, is a reason to have checked
        // first.
        let library = unsafe { libloading::Library::new(&path) }
            .map_err(|e| format!("could not open {}: {e}", path.display()))?;

        let descriptor = {
            let entry: libloading::Symbol<RegisterFn> = unsafe {
                library
                    .get(primitive_modapi::ENTRY_SYMBOL)
                    .map_err(|e| {
                        format!(
                            "no '{}' symbol: {e}",
                            String::from_utf8_lossy(
                                &primitive_modapi::ENTRY_SYMBOL
                                    [..primitive_modapi::ENTRY_SYMBOL.len() - 1]
                            )
                        )
                    })?
            };
            unsafe { entry(api) }
        };

        // What the library says about itself, checked against what the
        // manifest said. Both, because the manifest is what the loader
        // keyed everything on and the library is what will actually run:
        // a disagreement means the folder and the binary are from
        // different builds, and carrying on would file this mod's
        // settings and saved state under a name its code does not use.
        if !primitive_modapi::API_VERSION.accepts(descriptor.api_version) {
            return Err(format!(
                "the library was built against API {} and this server is {}",
                descriptor.api_version,
                primitive_modapi::API_VERSION
            ));
        }
        let declared = unsafe { descriptor.name.as_str() }.to_string();
        if declared != manifest.name {
            return Err(format!(
                "the library calls itself '{declared}' and the manifest calls it '{}'",
                manifest.name
            ));
        }

        self.settings
            .insert(manifest.name.clone(), manifest.settings.clone());
        self.mods.push(LoadedMod {
            name: manifest.name.clone(),
            version: manifest.version.clone(),
            description: manifest.description.clone(),
            authors: manifest.authors.clone(),
            folder: folder.to_path_buf(),
            api_version: descriptor.api_version,
            subscriptions: HashSet::new(),
            descriptor,
            _library: library,
            errors: 0,
            disabled: false,
        });
        Ok(())
    }

    pub(crate) fn drain_subscriptions(&mut self) {
        for (index, key) in std::mem::take(&mut self.pending_subscriptions) {
            if let Some(m) = self.mods.get_mut(index) {
                m.subscriptions.insert(key);
            }
        }
    }

    /// A mod reported a failure from inside a hook.
    pub fn note_error(&mut self, index: usize) -> Option<String> {
        let m = self.mods.get_mut(index)?;
        m.errors += 1;
        if m.errors >= MAX_ERRORS && !m.disabled {
            m.disabled = true;
            return Some(m.name.clone());
        }
        None
    }

    /// Where a mod's folder is, for resolving its resources and its save
    /// blob.
    pub fn folder_of(&self, name: &str) -> Option<&Path> {
        self.mods
            .iter()
            .find(|m| m.name == name)
            .map(|m| m.folder.as_path())
    }

    /// What each loaded mod says about itself, for `/mods`.
    pub fn describe(&self) -> Vec<String> {
        self.mods
            .iter()
            .map(|m| {
                format!(
                    "{} {} (API {}){}{}{}",
                    m.name,
                    m.version,
                    m.api_version,
                    if m.disabled { " [disabled]" } else { "" },
                    if m.description.is_empty() { "" } else { " -- " },
                    m.description
                )
            })
            .collect()
    }

    /// The same list as columns rather than as lines, with the settings
    /// each mod declared beside it.
    ///
    /// **Nothing here crosses the ABI.** Every column is read out of
    /// `mod.ron`, which the loader parses before it `dlopen`s anything
    /// -- see `open_one` -- so a screen showing a mod's authors and its
    /// settings costs the contract in `primitive_modapi` exactly
    /// nothing. Carrying them through the ABI instead would have meant a
    /// new out-struct frozen for the life of the major version, to move
    /// data the host already has in its hand.
    pub fn catalogue(&self) -> Vec<primitive_shared::protocol::ExtensionInfo> {
        use primitive_shared::protocol::{ExtensionInfo, ExtensionKind};
        self.mods
            .iter()
            .map(|m| ExtensionInfo {
                kind: ExtensionKind::Native,
                name: m.name.clone(),
                version: m.version.clone(),
                description: m.description.clone(),
                authors: m.authors.clone(),
                enabled: !m.disabled,
                reason: if m.disabled {
                    format!("stopped after {} error(s)", m.errors)
                } else if m.errors > 0 {
                    format!("{} error(s)", m.errors)
                } else {
                    String::new()
                },
                built_for: Some((m.api_version.major, m.api_version.minor)),
                settings: self
                    .settings
                    .get(&m.name)
                    .map(|settings| {
                        settings
                            .iter()
                            .map(|(key, value)| (key.clone(), value.to_text()))
                            .collect()
                    })
                    .unwrap_or_default(),
            })
            .collect()
    }
}

// ---------------------------------------------------------------------
// Calling into mods, with nothing locked.
//
// These are free functions taking the `Context` rather than methods on
// `ModHost`, and that is the whole fix: a method takes `&mut self`,
// which means the caller is holding the lock, which means a mod that
// calls back into the host deadlocks. See the note at the top of the
// module.
// ---------------------------------------------------------------------

/// Runs every registered decorator over a freshly generated chunk, then
/// tells whoever subscribed that a chunk was made.
///
/// **Called from a generator thread**, and that is the one thing a mod
/// author has to hold in their head here: this runs off the tick loop,
/// in parallel with itself, and a slow decorator is terrain that does
/// not arrive rather than a server that stutters.
///
/// A free function taking the `Context` rather than a method, for the
/// reason `dispatch` is one: a method takes `&mut self`, which means the
/// caller is holding the lock, which means a decorator that calls back
/// into the host deadlocks.
pub fn decorate(
    ctx: &std::sync::Arc<crate::Context>,
    pos: primitive_modapi::ChunkPos,
    seed: u32,
    blocks: &mut [u16],
) {
    let decorators = {
        let host = ctx.mods.lock().unwrap_or_else(|e| e.into_inner());
        host.decorators()
    };
    for decorator in decorators {
        // Safety: the pointer came from a loaded mod's library, which is
        // still loaded -- `unload_all` clears this list with everything
        // else, and it takes the lock this just released. The slice is
        // the caller's and outlives the call.
        unsafe {
            decorator(
                std::ptr::null_mut(),
                pos,
                seed,
                blocks.as_mut_ptr(),
                blocks.len(),
            );
        }
    }
}

/// Hands an event to every mod that asked for it.
///
/// Returns [`HookResult::Cancel`] if *any* of them cancelled, which is
/// what makes protection mods composable -- the same convention the
/// scripted plugins use.
pub fn dispatch(ctx: &std::sync::Arc<crate::Context>, event: Event, data: &EventData) -> HookResult {
    // Lock, copy out, drop. Three words per interested mod.
    let listeners = {
        let host = ctx.mods.lock().unwrap_or_else(|e| e.into_inner());
        host.listeners(event)
    };
    if listeners.is_empty() {
        return HookResult::Continue;
    }

    let mut result = HookResult::Continue;
    for listener in listeners {
        // `CURRENT` names the mod on the stack, so an API call made from
        // inside the handler knows who is asking. The guard puts back
        // whatever was there, so a nested dispatch nests.
        let _calling = Calling::mod_at(listener.index);
        // Safety: the mod is loaded and its library is alive -- nothing
        // unloads one except `unload_all`, which takes the same lock
        // this just released and cannot run between the copy and the
        // call, because both happen on this thread. `data` outlives the
        // call by construction.
        let answer = unsafe {
            (listener.on_event)(listener.user, event, data as *const EventData)
        };
        if matches!(answer, HookResult::Cancel) {
            result = HookResult::Cancel;
        }
    }

    // Whatever they asked for while they were running.
    {
        let mut host = ctx.mods.lock().unwrap_or_else(|e| e.into_inner());
        host.drain_subscriptions();
    }
    result
}

/// Tells every loaded mod the world is ready.
///
/// Separate from the load itself, and after *all* of it, so a mod that
/// looks for another one's registrations in `on_load` finds them -- that
/// is the whole reason `on_load` exists apart from the entry point.
pub fn start_all(ctx: &std::sync::Arc<crate::Context>) -> Report {
    let mut report = Report::new();
    let (count, api) = {
        let host = ctx.mods.lock().unwrap_or_else(|e| e.into_inner());
        (host.count(), host.api())
    };
    let Some(api) = api else {
        return report;
    };

    for index in 0..count {
        let (on_load, user, name) = {
            let host = ctx.mods.lock().unwrap_or_else(|e| e.into_inner());
            (host.on_load_of(index), host.user_of(index), host.name_of(index))
        };
        let Some(on_load) = on_load else { continue };
        let status = {
            let _calling = Calling::mod_at(index);
            // Safety: the table is the one the host built and keeps
            // alive; `user` is the mod's own and is never dereferenced
            // here. No lock is held, which is the point.
            unsafe { on_load(user, api) }
        };
        // Applied after every call, so a mod that subscribed during
        // `on_load` is subscribed.
        {
            let mut host = ctx.mods.lock().unwrap_or_else(|e| e.into_inner());
            host.drain_subscriptions();
            if status != Status::Ok {
                host.disable(index);
            }
        }
        if status != Status::Ok {
            report.push(format!(
                "mod '{name}' refused to start ({status:?}) and was disabled"
            ));
        }
    }

    // Worth saying out loud: a mod that subscribed to nothing will never
    // be called, and that is almost always a mistake rather than a
    // choice.
    let silent = {
        let host = ctx.mods.lock().unwrap_or_else(|e| e.into_inner());
        host.silent()
    };
    for name in silent {
        report.push(format!(
            "mod '{name}' subscribed to nothing and will never be called"
        ));
    }
    report
}

/// Tells every mod the server is stopping, in reverse load order, then
/// closes their libraries.
///
/// Reverse, because that is the order that lets a mod's dependencies
/// still be there while it winds up -- the same reason destructors run
/// in reverse declaration order.
pub fn unload_all(ctx: &std::sync::Arc<crate::Context>) {
    let count = {
        let host = ctx.mods.lock().unwrap_or_else(|e| e.into_inner());
        host.count()
    };
    for index in (0..count).rev() {
        let (on_unload, user) = {
            let host = ctx.mods.lock().unwrap_or_else(|e| e.into_inner());
            (host.on_unload_of(index), host.user_of(index))
        };
        let Some(on_unload) = on_unload else { continue };
        let _calling = Calling::mod_at(index);
        unsafe { on_unload(user) };
    }
    // The libraries go last and all at once. Dropping one while a mod's
    // function pointer is still reachable would be a use-after-free,
    // which is why `descriptor` is declared before `_library` on
    // `LoadedMod` -- Rust drops fields in declaration order.
    let taken = {
        let mut host = ctx.mods.lock().unwrap_or_else(|e| e.into_inner());
        host.take_all()
    };
    drop(taken);
}

/// Orders mods so that a mod is loaded after everything it depends on or
/// asked to follow.
///
/// Kahn's algorithm, and the interesting half is what happens when it
/// does not terminate: whatever is left has a cycle in it, and every mod
/// in that remainder is reported and skipped. Picking an arbitrary order
/// instead would produce a load that works on one machine and not
/// another, which is the worst possible way for a dependency mistake to
/// present.
///
/// Returns `(indices in load order, names that are in a cycle)`.
fn topological_order(found: &[(PathBuf, ModManifest)]) -> (Vec<usize>, Vec<String>) {
    let index_of: HashMap<&str, usize> = found
        .iter()
        .enumerate()
        .map(|(i, (_, m))| (m.name.as_str(), i))
        .collect();

    let mut waiting_on: Vec<HashSet<usize>> = vec![HashSet::new(); found.len()];
    for (i, (_, manifest)) in found.iter().enumerate() {
        for name in manifest
            .dependencies
            .iter()
            .map(|d| d.name.as_str())
            .chain(manifest.load_after.iter().map(|s| s.as_str()))
        {
            if let Some(&j) = index_of.get(name) {
                if j != i {
                    waiting_on[i].insert(j);
                }
            }
        }
    }

    let mut order = Vec::with_capacity(found.len());
    let mut placed: HashSet<usize> = HashSet::new();
    loop {
        // The lowest-numbered mod whose dependencies are all placed.
        // Lowest-numbered rather than any, so the order is stable for a
        // given install: two servers with the same mods load them the
        // same way round, which matters the day a mod's behaviour turns
        // out to depend on it.
        let next = (0..found.len())
            .find(|i| !placed.contains(i) && waiting_on[*i].iter().all(|j| placed.contains(j)));
        match next {
            Some(i) => {
                placed.insert(i);
                order.push(i);
            }
            None => break,
        }
    }
    let cycles = (0..found.len())
        .filter(|i| !placed.contains(i))
        .map(|i| found[i].1.name.clone())
        .collect();
    (order, cycles)
}

// ---------------------------------------------------------------------
// The host side of the API tables.
// ---------------------------------------------------------------------

/// Everything a mod's `HostApi` points at, owned in one box.
///
/// One allocation whose address never moves for as long as any mod is
/// loaded. That is the whole requirement: `HostApi` holds raw pointers
/// to the sub-tables, and a `Vec` that reallocated or a local that went
/// out of scope would leave every loaded mod holding dangling function
/// pointers.
// Every field here is read exactly once -- by `HostApi`, through a raw
// pointer the compiler cannot see -- and then never again by name. They
// exist to *own* the tables for as long as any mod is loaded, which is
// the whole point of the struct and is invisible to the dead-code
// analysis.
#[allow(dead_code)]
pub struct HostTables {
    // Boxed individually so that `api`'s pointers stay valid even if
    // this struct is moved before it is boxed.
    core: Box<primitive_modapi::CoreApi>,
    world: Box<primitive_modapi::WorldApi>,
    generation: Box<primitive_modapi::GenerationApi>,
    blocks: Box<primitive_modapi::BlocksApi>,
    items: Box<primitive_modapi::ItemsApi>,
    entities: Box<primitive_modapi::EntitiesApi>,
    players: Box<primitive_modapi::PlayersApi>,
    inventory: Box<primitive_modapi::InventoryApi>,
    network: Box<primitive_modapi::NetworkApi>,
    events: Box<primitive_modapi::EventsApi>,
    physics: Box<primitive_modapi::PhysicsApi>,
    save: Box<primitive_modapi::SaveApi>,
    // ---- since 2.0 ----
    crafting: Box<primitive_modapi::CraftingApi>,
    lighting: Box<primitive_modapi::LightingApi>,
    food: Box<primitive_modapi::FoodApi>,
    combat: Box<primitive_modapi::CombatApi>,
    fluid: Box<primitive_modapi::FluidApi>,
    containers: Box<primitive_modapi::ContainersApi>,
    stations: Box<primitive_modapi::StationsApi>,
    simulation: Box<primitive_modapi::SimulationApi>,
    /// The context the calls above reach through, kept alive here.
    context: Box<HostContext>,
    api: HostApi,
}

impl HostTables {
    fn new(host: HostContext) -> Self {
        use crate::logic::api_impl;
        let core = Box::new(api_impl::core_table());
        let world = Box::new(api_impl::world_table());
        let generation = Box::new(api_impl::generation_table());
        let blocks = Box::new(api_impl::blocks_table());
        let items = Box::new(api_impl::items_table());
        let entities = Box::new(api_impl::entities_table());
        let players = Box::new(api_impl::players_table());
        let inventory = Box::new(api_impl::inventory_table());
        let network = Box::new(api_impl::network_table());
        let events = Box::new(api_impl::events_table());
        let physics = Box::new(api_impl::physics_table());
        let save = Box::new(api_impl::save_table());
        let crafting = Box::new(api_impl::crafting_table());
        let lighting = Box::new(api_impl::lighting_table());
        let food = Box::new(api_impl::food_table());
        let combat = Box::new(api_impl::combat_table());
        let fluid = Box::new(api_impl::fluid_table());
        let containers = Box::new(api_impl::containers_table());
        let stations = Box::new(api_impl::stations_table());
        let simulation = Box::new(api_impl::simulation_table());
        let context = Box::new(host);
        let api = HostApi {
            version: primitive_modapi::API_VERSION,
            handle: (&*context as *const HostContext) as *mut std::ffi::c_void,
            core: &*core,
            world: &*world,
            generation: &*generation,
            blocks: &*blocks,
            items: &*items,
            entities: &*entities,
            players: &*players,
            inventory: &*inventory,
            network: &*network,
            events: &*events,
            physics: &*physics,
            save: &*save,
            // **Null, and deliberately.** There is no renderer, no audio
            // device and no window in this process. A stub that silently
            // did nothing is how a mod author spends an afternoon
            // wondering why their particle effect never appears on a
            // headless server; a null pointer is a question they can
            // ask.
            render: std::ptr::null(),
            audio: std::ptr::null(),
            ui: std::ptr::null(),
            crafting: &*crafting,
            lighting: &*lighting,
            food: &*food,
            combat: &*combat,
            fluid: &*fluid,
            containers: &*containers,
            stations: &*stations,
            simulation: &*simulation,
        };
        Self {
            core,
            world,
            generation,
            blocks,
            items,
            entities,
            players,
            inventory,
            network,
            events,
            physics,
            save,
            crafting,
            lighting,
            food,
            combat,
            fluid,
            containers,
            stations,
            simulation,
            context,
            api,
        }
    }
}


#[cfg(test)]
mod tests {
    use super::*;

    fn manifest(text: &str) -> (PathBuf, ModManifest) {
        (
            PathBuf::from("."),
            ModManifest::parse(text).expect("manifest"),
        )
    }

    #[test]
    fn a_mod_is_loaded_after_what_it_depends_on() {
        // Declared in the wrong order on purpose: the sort is the whole
        // point, and a test whose input is already sorted proves
        // nothing.
        let found = vec![
            manifest(r#"(name: "top", version: "1", dependencies: [(name: "middle")])"#),
            manifest(r#"(name: "middle", version: "1", dependencies: [(name: "base")])"#),
            manifest(r#"(name: "base", version: "1")"#),
        ];
        let (order, cycles) = topological_order(&found);
        assert!(cycles.is_empty());
        let names: Vec<&str> = order.iter().map(|&i| found[i].1.name.as_str()).collect();
        assert_eq!(names, ["base", "middle", "top"]);
    }

    #[test]
    fn load_after_orders_without_requiring() {
        let found = vec![
            manifest(r#"(name: "second", version: "1", load_after: ["first"])"#),
            manifest(r#"(name: "first", version: "1")"#),
        ];
        let (order, _) = topological_order(&found);
        let names: Vec<&str> = order.iter().map(|&i| found[i].1.name.as_str()).collect();
        assert_eq!(names, ["first", "second"]);
    }

    #[test]
    fn a_cycle_is_reported_and_nothing_in_it_is_loaded() {
        // The alternative is picking an arbitrary order, which produces
        // a load that works on one machine and not another -- the worst
        // possible way for a dependency mistake to present.
        let found = vec![
            manifest(r#"(name: "a", version: "1", dependencies: [(name: "b")])"#),
            manifest(r#"(name: "b", version: "1", dependencies: [(name: "a")])"#),
            manifest(r#"(name: "c", version: "1")"#),
        ];
        let (order, cycles) = topological_order(&found);
        let names: Vec<&str> = order.iter().map(|&i| found[i].1.name.as_str()).collect();
        assert_eq!(names, ["c"], "a cycle took an unrelated mod down with it");
        let mut cycles = cycles;
        cycles.sort();
        assert_eq!(cycles, ["a", "b"]);
    }

    #[test]
    fn a_reference_to_something_not_installed_does_not_stall_the_sort() {
        // Missing dependencies are refused earlier, in `load_dir`. What
        // reaches the sort may still name something absent -- an
        // optional dependency, or a `load_after` for a mod nobody has --
        // and the sort has to ignore it rather than wait for it forever.
        let found = vec![
            manifest(r#"(name: "solo", version: "1", load_after: ["ghost"])"#),
            manifest(
                r#"(name: "hopeful", version: "1", dependencies: [(name: "ghost", optional: true)])"#,
            ),
        ];
        let (order, cycles) = topological_order(&found);
        assert!(cycles.is_empty());
        assert_eq!(order.len(), 2);
    }

    #[test]
    fn the_order_is_stable_for_the_same_install() {
        // Two servers with the same mods have to load them the same way
        // round, or the day a mod's behaviour turns out to depend on the
        // order is the day one of them is unreproducible.
        let found = vec![
            manifest(r#"(name: "alpha", version: "1")"#),
            manifest(r#"(name: "beta", version: "1")"#),
            manifest(r#"(name: "gamma", version: "1")"#),
        ];
        let first = topological_order(&found).0;
        let again = topological_order(&found).0;
        assert_eq!(first, again);
        assert_eq!(first, vec![0, 1, 2]);
    }
}
