//! The contract between the Primitive server and a native mod.
//!
//! ## What this crate is
//!
//! A **stable, C-ABI boundary** and nothing else. It contains no logic:
//! it is the set of types and function-pointer tables that a compiled
//! `.dll` or `.so` and the server it is loaded into both agree about.
//! The server implements it; a mod calls it.
//!
//! It is a separate crate from both for one reason: a mod must be able
//! to build against *only this*, with no dependency on the server's
//! internals, or the "stable API" is a promise about a hundred thousand
//! lines of game code rather than about one file.
//!
//! ## Why native mods as well as scripts
//!
//! The server already has scripted plugins ([`primitive_server::plugins`]),
//! and they are the right answer for most of what people want: no build
//! step, no native code, and a broken plugin is a log line rather than a
//! crash. What they cannot be is *fast* or *big*. A mod that adds a
//! world generator, a mob AI, or a physics behaviour is doing work per
//! block or per tick, and a scripting engine with an operation limit is
//! the wrong tool for that by two orders of magnitude.
//!
//! So there are two extension points and they are for different things:
//!
//! | | scripted plugin | native mod |
//! |---|---|---|
//! | build step | none | a Rust (or C) toolchain |
//! | blast radius | a log line | the whole process |
//! | speed | interpreted | native |
//! | reaches | events and effects | every API module below |
//!
//! ## The shape of the boundary
//!
//! Everything crosses as `#[repr(C)]` plain data. No `String`, no `Vec`,
//! no trait objects, no `Result` -- those have no stable layout, and a
//! mod built with a different compiler version than the server would
//! read them as garbage. What crosses instead:
//!
//! - [`Str`] for text: a pointer and a length, valid for the duration of
//!   the call and not one instruction longer.
//! - [`Status`] for "did it work": an integer with named values.
//! - Function pointers, grouped into one table per **API module**.
//!
//! ## The API modules
//!
//! The host hands a mod one [`HostApi`], which is a struct of pointers
//! to sub-tables. The split is by *subject* rather than by convenience,
//! and the point of splitting at all is that a table can grow without
//! moving anything: adding a call to [`WorldApi`] appends a field, which
//! is a minor version bump, and a mod compiled against the older minor
//! version keeps working because it never reads past the end of what it
//! knew about.
//!
//! | table | what it reaches |
//! |---|---|
//! | [`CoreApi`] | logging, the tick, the seed, configuration |
//! | [`WorldApi`] | blocks in the world, chunks, saving |
//! | [`GenerationApi`] | terrain: height, biome, climate, and overriding them |
//! | [`BlocksApi`] | the block table: what a block *is* |
//! | [`ItemsApi`] | dropped stacks in the world |
//! | [`EntitiesApi`] | animals and everything else alive |
//! | [`PlayersApi`] | who is here, where, and how they are doing |
//! | [`InventoryApi`] | what a player is carrying and wearing |
//! | [`NetworkApi`] | chat, and a mod's own messages |
//! | [`EventsApi`] | subscribing, and cancelling |
//! | [`PhysicsApi`] | the rules movement is judged against |
//! | [`SaveApi`] | a mod's own persistent blob |
//! | [`CraftingApi`] | the recipe table, and making things from it |
//! | [`LightingApi`] | how bright a cell is |
//! | [`FoodApi`] | what is edible, what it is worth, and eating it |
//! | [`CombatApi`] | reach, cooldown, and what armour turns |
//! | [`FluidApi`] | water: sources, depth, and taking it away |
//! | [`ContainersApi`] | what is inside a chest, a hearth or a rack |
//! | [`StationsApi`] | fires, smelting and curing -- the things that work while you do not |
//! | [`SimulationApi`] | the world moving on its own: falling, growing, felling |
//! | [`RenderApi`], [`AudioApi`], [`UiApi`] | client-side; null on a server |
//!
//! ## What is deliberately *not* here
//!
//! The tables above are **operations, not structures**. There is no call
//! that hands out the layout of a chunk, the queue the water simulation
//! is working through, or the map of which cells are on fire -- and that
//! is the line, not an oversight. An operation survives the inside being
//! rewritten; a structure freezes it. The water simulation has already
//! been rewritten twice (see `primitive_server::logic::water`), and both
//! times every mod that asked it to *do* something would have kept
//! working while every mod that read its internals would have broken.
//!
//! The same rule decides the smaller refusals, each of which is written
//! down where it would have gone:
//!
//! - A mod cannot **add or remove a recipe**. [`CraftingApi`] reads the
//!   table and runs a row of it; it does not grow one. Recipes are
//!   content the *client* draws its own screen from, so a row that
//!   existed only on the server would be a craft nobody could find --
//!   and removing one is already expressible, and better expressed, by
//!   cancelling [`Event::ItemCrafted`].
//! - A mod cannot **set a player's velocity**. The client integrates its
//!   own motion; the server judges it. See [`PhysicsApi`].
//! - A mod cannot **write light**. There is no light map on a server to
//!   write to. See [`LightingApi`].
//!
//! The last three are declared here and are **null on a dedicated
//! server**, deliberately. A mod that draws something has to be able to
//! ask whether there is anything to draw on, and the honest way to say
//! "there is no renderer in this process" is a null pointer the mod
//! checks -- not a stub that silently does nothing, which is how a mod
//! author spends an afternoon wondering why their particle effect never
//! appears on a headless server.
//!
//! ## Versioning
//!
//! [`API_VERSION`] is a major and a minor. The rule is the usual one and
//! it is enforced by the host at load time (see
//! [`ApiVersion::accepts`]):
//!
//! - **Major** changes when something already in a table changes meaning
//!   or moves. A mod built against a different major is refused.
//! - **Minor** changes when something is *appended*. A mod built against
//!   an older minor is loaded; one built against a newer minor is
//!   refused, because it will reach for a field this host does not have.
//!
//! ## What a mod looks like
//!
//! ```text
//! mods/
//!   bigger_caves/
//!     mod.ron          <- the manifest: name, version, deps, settings
//!     bigger_caves.dll <- windows
//!     libbigger_caves.so <- linux
//! ```
//!
//! and one exported symbol:
//!
//! ```ignore
//! #[no_mangle]
//! pub unsafe extern "C" fn primitive_mod_register(
//!     host: *const HostApi,
//! ) -> ModDescriptor { ... }
//! ```
//!
//! [`declare_mod!`] writes that for you.

#![allow(clippy::missing_safety_doc)] // every unsafe fn here documents itself in prose

use core::ffi::c_void;

#[cfg(feature = "manifest")]
pub mod manifest;

// ---------------------------------------------------------------- version

/// A major/minor pair. See the module note for what each means.
#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ApiVersion {
    pub major: u16,
    pub minor: u16,
}

impl ApiVersion {
    pub const fn new(major: u16, minor: u16) -> Self {
        Self { major, minor }
    }

    /// Whether a host at `self` can load a mod built against `built_for`.
    ///
    /// Same major, and the mod's minor no newer than the host's. The
    /// asymmetry is the whole of the compatibility story: a mod built
    /// against 1.2 running on a 1.5 host only ever reads the first
    /// 1.2-worth of every table, which is still there; a mod built
    /// against 1.5 running on a 1.2 host would read a field that does
    /// not exist, which is not a graceful degradation, it is a segfault
    /// on somebody's server.
    pub const fn accepts(self, built_for: ApiVersion) -> bool {
        self.major == built_for.major && self.minor >= built_for.minor
    }
}

impl core::fmt::Display for ApiVersion {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        write!(f, "{}.{}", self.major, self.minor)
    }
}

/// The version this build of the contract is.
pub const API_VERSION: ApiVersion = ApiVersion::new(2, 2);

// 2.2 added the four workshops: `Station::Bench` to `Leather` and
// `Feasibility::NeedsWorkshop`. A minor on 2.1's rule -- values past the
// ones an older mod has seen, in enums the host fills in and a mod must
// already treat as open, and no struct grew. `CraftHeat` is the one place
// the workshops would naturally go and is left alone for exactly that
// reason: see `Station::Bench`.

// 2.1 added `Work::Ground` -- the shovel. Additive and a minor, on the
// same rule the rest of this note applies: nothing moved, nothing
// changed meaning, and no struct grew. What a mod compiled against 2.0
// can meet is a value of 4 where it has only ever seen 0..3, and the
// answer to that was already written down -- see the variant, and the
// module note on why every enum the host fills in is open.
//
// 1.1 appended `set_flying` and `is_flying` to `PlayersApi`. A mod built
// against 1.0 keeps working -- it never reads past the end of what it
// knew about, and the two new fields are past it -- which is the whole
// of what a minor bump promises. See the module note on versioning.
//
// **2.0 is a major, and the additions are not why.**
//
// Everything this version adds -- eight new tables, the appended calls
// on the old ones, the new events -- is purely additive and would have
// been 1.2 on its own. One thing is not additive, and it is one field:
//
// `PlayerVitals::temperature_c` used to be the temperature the world
// pushed a body toward. It is now the temperature of a *living* body,
// which sits above the air around it by up to
// `primitive_shared::body::METABOLIC_LIFT_C` -- see the entry in
// CHANGELOG.md. A naked player standing in the spawn meadow at noon read
// 16 before and reads 28 now. Same signature, same units, same name:
// different number, every time, for every player.
//
// That is precisely the failure a version number exists to prevent, and
// it is worse than a crash because nothing crashes. A mod that decided
// "this player is freezing" below 18 was right yesterday and is wrong
// today, silently, on somebody's server, in the one subsystem where
// being wrong means a player dies of cold that the mod said was not
// happening. A minor bump would have loaded that mod and let it be
// wrong; a major refuses it at the manifest, before `dlopen`, with a
// line naming both versions.
//
// It cannot be papered over at the boundary either. The lift is not a
// constant that could be subtracted back off: it is scaled by wetness
// and clamped at neutral (`body::felt_ambient`), so reporting the old
// number would mean re-implementing a model the game no longer has. A
// number nothing computes is not compatibility, it is a lie with a
// version stamp on it.
//
// The field is **renamed** to `body_temperature_c` in the same breath,
// and that is the other half of the same decision: a rename turns "your
// mod is now subtly wrong" into a compile error at the exact line that
// has to be re-thought. A mod author who reaches for `temperature_c`
// gets told, by the compiler, to go and read what it means now.
//
// The additive discipline is unchanged and is still tested: every 1.1
// call is at the same offset in the same table with the same meaning,
// so a mod rebuilt against 2.0 without touching anything but that field
// behaves exactly as it did. See
// `every_call_the_previous_version_had_is_still_where_it_was`.

/// The symbol every native mod exports. See [`declare_mod!`].
pub const ENTRY_SYMBOL: &[u8] = b"primitive_mod_register\0";

/// The signature of that symbol.
pub type RegisterFn = unsafe extern "C" fn(host: *const HostApi) -> ModDescriptor;

// ------------------------------------------------------------- plain data

/// Borrowed text across the boundary.
///
/// **Valid for the duration of the call and not one instruction
/// longer.** A mod that wants to keep a string copies it. There is no
/// way to make that safe with a lifetime here, because lifetimes do not
/// survive a `#[repr(C)]` boundary -- so it is a rule, stated here, and
/// [`Str::as_str`] is the only unsafe thing a well-behaved mod ever
/// calls.
#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct Str {
    pub ptr: *const u8,
    pub len: usize,
}

impl Str {
    pub const EMPTY: Str = Str {
        ptr: core::ptr::null(),
        len: 0,
    };

    /// Borrows a Rust string for one call.
    pub fn borrow(s: &str) -> Str {
        Str {
            ptr: s.as_ptr(),
            len: s.len(),
        }
    }

    /// # Safety
    /// The pointer must still be valid, which inside a call it is.
    pub unsafe fn as_str<'a>(self) -> &'a str {
        if self.ptr.is_null() || self.len == 0 {
            return "";
        }
        core::str::from_utf8(core::slice::from_raw_parts(self.ptr, self.len)).unwrap_or("")
    }
}

/// Text a mod hands *back* to the host, which the host copies
/// immediately and the mod owns for exactly as long as the call.
pub type OwnedStr = Str;

/// Did it work?
///
/// An integer rather than a `Result`, because `Result` has no stable
/// layout. `Ok` is zero, so `status == 0` reads correctly in C.
#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Status {
    Ok = 0,
    /// The thing named does not exist -- a player who left, a chunk
    /// nobody has loaded, a block id that is not in the table.
    NotFound = 1,
    /// The arguments were wrong: a coordinate out of the world, a slot
    /// past the end of a pack, a NaN.
    BadArgument = 2,
    /// The host refuses on principle: a mod asking to move a player
    /// through the world border, or to write to a world with
    /// persistence off.
    Refused = 3,
    /// The call exists but this host cannot serve it -- the client-only
    /// tables on a dedicated server, and the honest answer to "draw
    /// this" in a process with no window.
    Unavailable = 4,
}

impl Status {
    pub fn is_ok(self) -> bool {
        matches!(self, Status::Ok)
    }
}

/// A cell of the world.
#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BlockPos {
    pub x: i32,
    pub y: i32,
    pub z: i32,
}

/// A point in it.
#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Vec3 {
    pub x: f32,
    pub y: f32,
    pub z: f32,
}

/// A chunk, by its position on the horizontal grid.
#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ChunkPos {
    pub x: i32,
    pub z: i32,
}

/// A connected player. Zero is never a valid one.
///
/// Sixty-four bits, matching the server's own, because a connection
/// number is handed out fresh on every join for the life of a process
/// and a server that has been up for a month has handed out a lot of
/// them.
pub type PlayerId = u64;
/// Anything alive or falling that is not a player.
pub type EntityId = u64;
/// A block, in the game's own numbering.
pub type BlockId = u16;

/// An opaque handle to whatever the host is using to serve this call.
///
/// Every host function takes one. It is how a table of plain function
/// pointers reaches the server's state without the mod ever seeing that
/// state's shape -- and it is why none of these are `static mut`
/// anywhere.
pub type HostHandle = *mut c_void;

/// What a player has on, by body part.
#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BodySlot {
    Head = 0,
    Chest = 1,
    Legs = 2,
    Feet = 3,
}

/// How a player is doing, in one struct.
///
/// One call rather than eight, because a mod that draws a HUD or decides
/// whether somebody is in trouble wants all of it at once, and eight
/// crossings of an FFI boundary to answer one question is eight chances
/// for the answers to be from different ticks.
#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PlayerVitals {
    pub health: f32,
    pub max_health: f32,
    /// 0..1.
    pub nourishment: f32,
    /// 0..1.
    pub hydration: f32,
    /// 0..1.
    pub breath: f32,
    /// **The temperature of the body**, in the degrees the game measures
    /// in -- which is not the temperature of the air, and since 2.0 is
    /// not close to it either.
    ///
    /// A living body burns food and settles well above the air around
    /// it; clothing adds to that offset rather than replacing it. So a
    /// naked player in the spawn meadow at noon is at 28 while the
    /// meadow is at 16, and the comfortable band is 26..34.
    ///
    /// **This field was called `temperature_c` in 1.1 and reported the
    /// meadow's 16.** It was renamed rather than quietly re-valued so
    /// that a mod comparing it against a threshold fails to compile
    /// instead of failing to notice. See the note beside [`API_VERSION`].
    pub body_temperature_c: f32,
    /// The temperature around them. Unchanged in 2.0: this is the air,
    /// and the air did not move.
    pub ambient_c: f32,
    /// 0..1.
    pub wetness: f32,
    /// Kilograms carried, worn included.
    pub carried_kg: f32,
    pub dead: bool,
}

/// One stack, as a mod sees it.
#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ItemStack {
    pub block: BlockId,
    pub count: u32,
    /// Swings taken, against the block's durability. Zero for anything
    /// that does not wear out.
    pub damage: u32,
}

impl ItemStack {
    pub const EMPTY: ItemStack = ItemStack {
        block: 0,
        count: 0,
        damage: 0,
    };
}

/// What the sky is doing.
#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Weather {
    Clear = 0,
    Rain = 1,
    Storm = 2,
}

/// Where a recipe can be worked.
///
/// The game's own `crafting::Station`, mirrored here rather than
/// referenced, because this crate depends on nothing.
#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Station {
    /// Hands, anywhere. Most of the table.
    Hands = 0,
    /// Beside a lit fire of any kind.
    Heat = 1,
    /// Beside a lit kiln: everything that melts or is fired.
    Forge = 2,
    /// Beside a lit bloomery: iron, and only iron.
    Bloomery = 3,
    /// Beside a joiner's bench. The four workshops (2.2) are not hearths:
    /// their rows are crafted from the pack, like `Hands`, only within reach
    /// of the block. [`CraftHeat`] does not report them -- it is an
    /// out-parameter a mod sizes, and a struct that grew would be written
    /// past the end of an older mod's buffer -- so a mod that wants to know
    /// asks [`Feasibility`], which answers `NeedsWorkshop`.
    Bench = 4,
    /// Beside a mason's block. See `Bench`.
    Mason = 5,
    /// Beside a potter's wheel. See `Bench`.
    Wheel = 6,
    /// Beside a currier's bench. See `Bench`.
    Leather = 7,
}

/// What is burning within working range of a player.
///
/// Three bools rather than one "beside a fire", because there are three
/// hearths in this game and the recipes tell them apart -- a campfire is
/// about six hundred degrees and copper melts at a thousand.
#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct CraftHeat {
    pub fire: bool,
    pub kiln: bool,
    pub bloomery: bool,
}

/// Why a craft cannot happen, or that it can.
///
/// Four different "no"s rather than one, because they are four different
/// things for the player to do about it: go and find more tin, empty a
/// slot, go and stand by the fire, or go and *build a kiln*.
#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Feasibility {
    Ready = 0,
    MissingIngredients = 1,
    /// The output has nowhere to go.
    NoRoom = 2,
    NeedsFire = 3,
    NeedsForge = 4,
    NeedsBloomery = 5,
    /// It wants a workshop that is not within reach; which one is the
    /// recipe's [`RecipeInfo::station`]. Added in 2.2.
    NeedsWorkshop = 6,
}

/// One row of the recipe table, without its lists.
///
/// The inputs and the returns are read one at a time through
/// [`CraftingApi::recipe_input`] and [`CraftingApi::recipe_return`]
/// rather than as pointers into the host's table. A pointer would be
/// faster and would hand a mod the address of a `&'static [(BlockId,
/// u32)]` whose layout is the *server's* to change; a count and an
/// index cost one call each and outlive the decision.
#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RecipeInfo {
    pub output: BlockId,
    pub output_count: u32,
    pub station: Station,
    pub input_count: u32,
    /// What comes back out on top of the output -- a crucible, a mould.
    /// Zero for nearly every row.
    pub return_count: u32,
}

/// What eating something costs, for the rows that cost anything.
#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct FoodHarm {
    /// Health taken, in the units [`PlayerVitals::max_health`] is in.
    pub health: f32,
    /// ...and how much of the stomach it turns out.
    pub nourishment: f32,
}

/// The lowest tool that gets into a block at all, and what a tool *is*.
#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Tier {
    Hand = 0,
    Flint = 1,
    Copper = 2,
    Bronze = 3,
    Iron = 4,
    /// A lashed stone head, between the hand and the glued tool.
    /// Numbered last rather than in order because the numbers are the
    /// ABI: a mod built against the old table still reads the old
    /// tiers as it did.
    Stone = 5,
}

/// What kind of work a block is, on both readings: which tool opens this
/// material, and what this tool is for.
#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Work {
    Any = 0,
    /// Rock and ore. A pick.
    Stone = 1,
    /// Standing timber. An axe.
    Wood = 2,
    /// Growing things. A knife.
    Plant = 3,
    /// Loose ground: soil, sand, gravel, snow, ash. A shovel.
    ///
    /// **Read off a tool only.** No block in the game asks for `Ground`
    /// -- loose ground is `Any`, because a fist has always been able to
    /// dig a hole -- so a mod reading a *block's* work will never see
    /// this value. A mod reading a *tool's* will, and what it means is
    /// "this is a shovel": the host digs loose ground twice as fast with
    /// it and takes nothing away from anything else.
    ///
    /// Added in 2.1. **This enum is open**, and always was: the host
    /// hands out whatever value the block table holds, so a mod that
    /// matches on it needs an arm for values it does not know. One that
    /// does not have one is a mod that will trip over the next material
    /// this game learns about, whichever version that lands in.
    Ground = 4,
}

/// The tool half of the block table.
///
/// **A second struct rather than more fields on [`BlockProperties`]**,
/// and that is a rule rather than a preference: a mod allocates a
/// `BlockProperties` on its own stack and hands the host a pointer to
/// it, so growing that struct would have the host write past the end of
/// a buffer a mod compiled against the older version sized. Every
/// out-parameter struct in this file is frozen for that reason, and new
/// columns arrive as new structs.
#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct BlockTooling {
    /// The lowest tier that opens this material at all.
    pub needs: Tier,
    /// What kind of work it is.
    pub work: Work,
    /// What tier this *is*, if it is a tool. [`Tier::Hand`] when
    /// `is_tool` is false.
    pub tool: Tier,
    pub is_tool: bool,
    /// Swings before it breaks, or 0 for anything that does not wear
    /// out.
    pub durability: u32,
    /// Seconds to mine once it is lying down, or negative for a block
    /// that has no such state. Only a felled log does.
    pub felled: f32,
    /// What stands in the cell after breaking it, when breaking does not
    /// empty the cell. Zero for everything except the picked berry bush.
    pub leaves_behind: BlockId,
    /// How fast a player moves over it, and how well a foot holds.
    pub drag: f32,
    pub grip: f32,
    /// How much of its cell it fills, in eighths, measured up from the
    /// floor. Eight is a whole block.
    pub thickness: u8,
}

/// What a species is, as numbers.
///
/// The columns a mod deciding "should this thing be here, and can the
/// player get away from it" actually needs. A copy rather than a
/// pointer into the game's own table, for the reason
/// [`BlockProperties`] is one.
#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct SpeciesInfo {
    pub max_health: f32,
    /// What one blow from it costs.
    pub damage: f32,
    pub walk_speed: f32,
    pub run_speed: f32,
    pub hostile: bool,
    /// How far off it notices a player.
    pub awareness: f32,
    /// How close a player has to get before it takes offence.
    pub provoke_range: f32,
    pub height: f32,
    pub width: f32,
    pub length: f32,
    /// How many different things it leaves behind. Read them with
    /// [`EntitiesApi::species_drop`].
    pub drop_count: u32,
}

// ---------------------------------------------------------------- events

/// Everything a mod can be told about.
///
/// **One enum rather than one callback per event**, and that is the
/// versioning decision: adding an event is adding a variant, and a mod
/// compiled against an older minor simply never matches it -- whereas
/// adding a field to a struct of callbacks moves every field after it.
///
/// The payload is a union-by-convention: each variant says which fields
/// of [`EventData`] mean anything. A real `union` would be more honest
/// and much harder to use correctly from a mod, and this struct is
/// forty-eight bytes.
#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Event {
    /// The server has finished starting. `data` is unused.
    ServerStarted = 0,
    /// It is about to stop. Last chance to save.
    ServerStopping = 1,
    /// One server tick. `tick`.
    Tick = 2,
    /// `player`, `text`.
    PlayerJoined = 10,
    PlayerLeft = 11,
    /// `player`, `text`. **Cancellable**: refusing swallows the message.
    PlayerChat = 12,
    /// `player`, `text` (the cause). Not cancellable -- they are already
    /// dead.
    PlayerDied = 13,
    /// `player`, `pos`, `block`. **Cancellable.**
    BlockPlace = 20,
    /// `player`, `pos`, `block` (what is there now). **Cancellable.**
    BlockBreak = 21,
    /// `pos`, `block`. Anything at all changed a cell -- water, sand,
    /// fire, a plugin. Not cancellable: it has happened.
    BlockChanged = 22,
    /// `pos` (`x` and `z`; `y` is zero). A chunk was made.
    ///
    /// **Fired from a generator thread**, in parallel with itself, so a
    /// slow handler here is terrain that does not arrive rather than a
    /// server that stutters. Fired *after* every registered
    /// [`ChunkDecorator`] has run, so what a handler would find if it
    /// asked is what the decorators left.
    ///
    /// There is no `ChunkEvicted` beside it, and there was one until
    /// 2.0. Two reasons, and either would have been enough. Eviction
    /// happens inside the world's own shard lock, and nothing may call
    /// into a mod with a lock held -- so it could not honestly be fired
    /// from where it happens. And it would say almost nothing if it
    /// were: a chunk is evicted because the cache is full and is rebuilt
    /// from the seed and the edit overlay the moment anybody asks for
    /// it, so "that chunk is gone" is true for as long as it takes to
    /// read. A mod that needs to know whether a chunk is there asks
    /// [`WorldApi::is_chunk_loaded`], which is the honest form of the
    /// question.
    ChunkGenerated = 30,
    /// `entity`, `pos`. Something not a player appeared -- the spawner
    /// putting a deer in a meadow, or a mod's own
    /// [`EntitiesApi::spawn`].
    EntitySpawned = 40,
    /// `entity`. Something not a player went: killed, or forgotten
    /// because everybody walked away from it.
    ///
    /// **`pos` is zero**, deliberately. By the time this is fired the
    /// animal is out of the world and there is nothing left to ask where
    /// it was; a coordinate copied out a moment earlier would be a
    /// number that looks authoritative and is not. A mod that wants to
    /// know where something died watches what lands on the ground.
    EntityRemoved = 41,
    /// `player`, `block` (what is made), `count` (how many of it),
    /// `value` (the recipe's index in the table). **Cancellable**:
    /// refusing is how a mod takes a recipe out of the game.
    ItemCrafted = 50,
    /// `player`, `pos`. A hide finished curing on a rack.
    HideCured = 60,
    /// `player`. Somebody drank.
    PlayerDrank = 61,
    /// `player`, `block` (the garment, or 0 for bare), `slot` (a
    /// [`BodySlot`] discriminant). Something was put on or taken off.
    EquipmentChanged = 62,
    /// `value` is the new weather as a [`Weather`] discriminant.
    WeatherChanged = 70,
    /// `float` is the new time of day.
    ///
    /// **Fired when the time is *set*, not when it passes.** Time moves
    /// every tick; an event for that is [`Event::Tick`], which a mod can
    /// pair with [`CoreApi::time_of_day`]. This one means somebody --
    /// `/time`, a mod, a plugin -- moved the sun.
    TimeChanged = 71,
    /// `player`, `text` (the command), `args` (space-joined). Fired for a
    /// command the server does not know. Returning [`HookResult::Cancel`]
    /// means "I handled it".
    Command = 80,

    // ---- since 2.0 ----
    //
    // Every one of these is fired by a real path in the server. That is
    // worth saying because it was not always true: nine of the variants
    // above were declared in 1.0 and never actually happened, which is
    // the worst shape an event can have -- a mod subscribes, the
    // handler is correct, and nothing ever calls it. They all fire now.
    // See `primitive_server::fire_event`.
    /// `player`, `float` (the damage that will land, after armour),
    /// `text` (the cause). **Cancellable**: refusing means the blow does
    /// not happen at all.
    ///
    /// Separate from [`Event::PlayerDied`] because they are different
    /// questions: this one is "should this hurt", and it is asked for
    /// every scratch, including the ones nobody dies of.
    PlayerHurt = 14,
    /// `player`, `float` (how much health went back).
    ///
    /// **Not cancellable**, and that is a decision rather than an
    /// omission. Health comes back two ways -- a mod handing it over,
    /// and the body's own slow regeneration, which decides how much
    /// inside its own clock as it applies it. An event a mod could
    /// refuse in one of those cases and not the other would be an event
    /// nobody could reason about. A mod that wants a world with no
    /// regeneration takes the health straight back with
    /// [`PlayersApi::damage`], and is visibly doing so.
    PlayerHealed = 15,
    /// `player`, `block` (what is being eaten), `slot`. **Cancellable.**
    ///
    /// The counterpart of [`Event::PlayerDrank`], which existed from the
    /// start while eating did not.
    PlayerAte = 16,
    /// `player`, `pos` (the cell their head is in). Their head went
    /// under. Not cancellable: water is where it is.
    PlayerEnteredWater = 17,
    /// `player`, `pos`. Their head came back up.
    PlayerLeftWater = 18,
    /// `player`, `slot` (the new one), `block` (what is in it).
    /// The selected hotbar slot changed.
    HeldSlotChanged = 19,
    /// `player`, `pos` (the trunk struck), `count` (how many cells came
    /// down). **Cancellable**: refusing leaves the tree standing.
    TreeFelled = 23,
    /// `player`, `block`, `count`, `pos`. A stack was taken off the
    /// ground.
    ///
    /// **Not cancellable, and the reason is structural rather than a
    /// judgement**: the pickup happens with the item store and the
    /// player's own state both locked, and no lock may be held while a
    /// mod is called. A mod that wants to refuse an item takes it back
    /// with [`InventoryApi::take`] and drops it again, which is two
    /// calls and honest about what it is doing.
    ItemPickedUp = 51,
    /// `player`, `block`, `count`. Something was thrown down.
    /// **Cancellable.**
    ItemDropped = 52,
    /// `player`, `block` (what it was), `slot`. A tool wore out in
    /// somebody's hands. Not cancellable -- it is already splinters.
    ToolBroke = 53,
    /// `player`, `pos`, `block` (the container). Somebody opened a
    /// chest, a hearth or a rack. **Cancellable**: refusing is a lock.
    ContainerOpened = 63,
    /// `player`, `pos`. They closed it, or were made to.
    ContainerClosed = 64,
    /// `pos`. A hearth finished a batch.
    ///
    /// The position and nothing else: what came out is already in the
    /// output slot, and a mod reads it with
    /// [`ContainersApi::get_slot`] rather than being handed a copy that
    /// could disagree with what is actually in there. Not cancellable
    /// -- it is smelted.
    SmeltingFinished = 65,
    /// `pos`, `block` (what is there now). Something grew: a bush
    /// filled again, a crop moved a stage.
    ///
    /// **Not cancellable.** The growth pass writes the cell and *then*
    /// hands the change to the tick loop, so by the time a mod hears
    /// about it the bush already has berries on it. A mod that wants a
    /// barren region writes the old block back with
    /// [`WorldApi::set_block`], which is one call and is honest about
    /// being an undo rather than a veto.
    ///
    /// There is no `FireSpread` beside this, and there is no place for
    /// one: fire in this game burns *down*, in the hearth it was lit
    /// in. Nothing spreads, so an event for it would be a variant no
    /// mod would ever be called for.
    GrowthStep = 66,
    /// `pos`, `block` (what the cell is now -- a kiln that has burnt
    /// through its charcoal comes back a kiln). A fire ran out of fuel,
    /// was put out, or was broken.
    FireDied = 68,
}

/// The payload of an [`Event`]. Which fields mean anything depends on
/// the variant; the rest are zero.
#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct EventData {
    pub player: PlayerId,
    pub entity: EntityId,
    pub pos: BlockPos,
    pub block: BlockId,
    pub count: u32,
    pub slot: u32,
    pub tick: u64,
    pub value: i64,
    pub float: f32,
    pub text: Str,
    pub args: Str,
}

impl Default for EventData {
    fn default() -> Self {
        Self {
            player: 0,
            entity: 0,
            pos: BlockPos { x: 0, y: 0, z: 0 },
            block: 0,
            count: 0,
            slot: 0,
            tick: 0,
            value: 0,
            float: 0.0,
            text: Str::EMPTY,
            args: Str::EMPTY,
        }
    }
}

/// What a mod says back about an event.
#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HookResult {
    /// Carry on. What a mod that only watched should return.
    Continue = 0,
    /// Stop this from happening. Only meaningful on the cancellable
    /// events; ignored elsewhere, which is deliberate -- a mod that
    /// cancels `PlayerDied` should be a no-op rather than an
    /// exception.
    Cancel = 1,
}

// ------------------------------------------------------------ host tables
//
// Every function takes the `HostHandle` first. That is the whole trick
// that lets a table of plain function pointers reach live server state
// without any global anywhere.

/// Logging, the clock, and the settings a mod was given.
#[repr(C)]
pub struct CoreApi {
    /// Writes a line to the server log, tagged with the mod's name.
    pub log: unsafe extern "C" fn(HostHandle, level: LogLevel, message: Str),
    /// The current tick.
    pub tick: unsafe extern "C" fn(HostHandle) -> u64,
    /// 0.0 = midnight, 0.5 = noon.
    pub time_of_day: unsafe extern "C" fn(HostHandle) -> f32,
    pub set_time_of_day: unsafe extern "C" fn(HostHandle, f32) -> Status,
    pub day_length_seconds: unsafe extern "C" fn(HostHandle) -> f32,
    /// Ticks per second the server is configured for.
    pub tick_rate_hz: unsafe extern "C" fn(HostHandle) -> f32,
    /// A setting out of this mod's own `mod.ron`, by key. Writes into
    /// `out` and answers how many bytes it wanted; a `cap` of zero is
    /// how you ask for the length.
    pub setting: unsafe extern "C" fn(
        HostHandle,
        key: Str,
        out: *mut u8,
        cap: usize,
        written: *mut usize,
    ) -> Status,
    /// Runs a server command at operator permission, as the console
    /// would. The one deliberately blunt instrument in the API: it is
    /// how a mod does something no table here covers, and it is a string
    /// because the alternative is a table with every command in it.
    pub run_command: unsafe extern "C" fn(HostHandle, line: Str) -> Status,
}

#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LogLevel {
    Debug = 0,
    Info = 1,
    Warn = 2,
    Error = 3,
}

/// Blocks in the world, and the chunks they live in.
#[repr(C)]
pub struct WorldApi {
    /// What is at a cell. `Status::NotFound` for a chunk nobody has
    /// loaded -- deliberately *not* "generate it and tell me", because a
    /// mod that could make the server generate a chunk by asking about
    /// it is a denial of service with a nice interface.
    pub get_block: unsafe extern "C" fn(HostHandle, BlockPos, out: *mut BlockId) -> Status,
    /// Puts one there and tells everybody who can see it.
    pub set_block: unsafe extern "C" fn(HostHandle, BlockPos, BlockId) -> Status,
    /// Fills a box. Bounded by the host at a few thousand cells -- a mod
    /// asking for a million-block region would hold the tick loop for as
    /// long as it took. Answers how many were written.
    pub fill: unsafe extern "C" fn(
        HostHandle,
        from: BlockPos,
        to: BlockPos,
        BlockId,
        written: *mut u32,
    ) -> Status,
    pub world_seed: unsafe extern "C" fn(HostHandle) -> u32,
    pub spawn_point: unsafe extern "C" fn(HostHandle) -> Vec3,
    /// Whether a chunk is loaded right now.
    pub is_chunk_loaded: unsafe extern "C" fn(HostHandle, ChunkPos) -> bool,
    /// Asks for a chunk to be generated, nearest-first, and returns
    /// immediately. The queue and the pool are the server's; see
    /// `primitive_server::chunkgen`.
    pub request_chunk: unsafe extern "C" fn(HostHandle, ChunkPos) -> Status,
    /// How many chunks are in the cache.
    pub loaded_chunk_count: unsafe extern "C" fn(HostHandle) -> u32,
    pub weather: unsafe extern "C" fn(HostHandle) -> Weather,
    pub set_weather: unsafe extern "C" fn(HostHandle, Weather) -> Status,
    /// The temperature around a point, in the degrees the game uses.
    pub temperature_at: unsafe extern "C" fn(HostHandle, Vec3, out: *mut f32) -> Status,

    // ---- since 2.0 ----
    /// Breaks a cell the way a player's swing does: the drop lands on
    /// the ground, a bush is picked rather than pulled up, and anything
    /// that was standing on it comes down with it.
    ///
    /// **The difference from `set_block(pos, air)` is the whole reason
    /// it exists.** Writing air destroys what was there; a mod clearing
    /// a road with `set_block` deletes every log it walks over and the
    /// player never sees the wood. Pass `drop = false` to get the
    /// destroying kind on purpose.
    pub break_block: unsafe extern "C" fn(HostHandle, BlockPos, drop: bool) -> Status,
    /// The highest non-air cell in a column of the **loaded** world,
    /// edits and all.
    ///
    /// Not the same question as [`GenerationApi::height_at`], which asks
    /// the generator and therefore answers about terrain nobody has
    /// built on. This is where a mod puts something so that it lands on
    /// top of the house rather than inside it. `NotFound` for a column
    /// in an unloaded chunk -- see the note on [`WorldApi::get_block`].
    pub surface_at: unsafe extern "C" fn(HostHandle, x: i32, z: i32, out: *mut i32) -> Status,
    /// Tells the simulations a cell changed: the sand looks again at
    /// what it is standing on, the water re-floods, the fire re-checks
    /// its fuel, the growth wakes up.
    ///
    /// [`WorldApi::set_block`] does this itself. This is for the other
    /// case -- a mod that changed the world some other way, or one that
    /// wants a column of sand that has been sitting there since worldgen
    /// to notice that it never had anything under it.
    pub disturb: unsafe extern "C" fn(HostHandle, BlockPos) -> Status,
}

/// Terrain, as the generator sees it.
#[repr(C)]
pub struct GenerationApi {
    /// The surface height of a column.
    pub height_at: unsafe extern "C" fn(HostHandle, x: i32, z: i32) -> i32,
    /// Which biome, as the game's own index.
    pub biome_at: unsafe extern "C" fn(HostHandle, x: i32, z: i32) -> u32,
    /// The biome's name, into `out`.
    pub biome_name: unsafe extern "C" fn(
        HostHandle,
        biome: u32,
        out: *mut u8,
        cap: usize,
        written: *mut usize,
    ) -> Status,
    /// Warmth and wetness at a cell, both 0..1.
    pub climate_at:
        unsafe extern "C" fn(HostHandle, BlockPos, warmth: *mut f32, humidity: *mut f32) -> Status,
    /// Registers a decorator: after the host has generated a chunk, this
    /// is handed the chunk's blocks to modify.
    ///
    /// **The one place a mod gets to change terrain**, and it is a
    /// decorator rather than a replacement generator on purpose. The
    /// server's whole save format rests on generation being
    /// deterministic and reproducible from a seed -- see
    /// `primitive_server::world` -- so a mod that generated terrain from
    /// anything but its coordinates would produce a world that cannot be
    /// evicted and regenerated. A decorator is handed the coordinates
    /// and nothing else, which makes that mistake awkward to make.
    pub register_decorator: unsafe extern "C" fn(HostHandle, ChunkDecorator) -> Status,
}

/// Called with a whole chunk's blocks, after the host generated them.
///
/// `blocks` is `len` cells in the game's own order, and writing to it is
/// the point. Must be a pure function of (`pos`, `seed`, the incoming
/// blocks) -- see [`GenerationApi::register_decorator`].
pub type ChunkDecorator = unsafe extern "C" fn(
    user: *mut c_void,
    pos: ChunkPos,
    seed: u32,
    blocks: *mut BlockId,
    len: usize,
);

/// The block table: what a block *is*, as opposed to where one is.
#[repr(C)]
pub struct BlocksApi {
    /// How many kinds there are.
    pub count: unsafe extern "C" fn(HostHandle) -> u32,
    /// The id at an index, for walking the table.
    pub id_at: unsafe extern "C" fn(HostHandle, index: u32, out: *mut BlockId) -> Status,
    pub name: unsafe extern "C" fn(
        HostHandle,
        BlockId,
        out: *mut u8,
        cap: usize,
        written: *mut usize,
    ) -> Status,
    /// The id for a name, which is what a mod reading its own config has.
    pub by_name: unsafe extern "C" fn(HostHandle, name: Str, out: *mut BlockId) -> Status,
    pub properties: unsafe extern "C" fn(HostHandle, BlockId, out: *mut BlockProperties) -> Status,

    // ---- since 2.0 ----
    /// The tool half of the same row. See [`BlockTooling`] for why it is
    /// a second struct rather than more fields on the first.
    pub tooling: unsafe extern "C" fn(HostHandle, BlockId, out: *mut BlockTooling) -> Status,
    /// Seconds to break this block **with that tool in hand**, which is
    /// what a mod writing its own mining rule actually wants:
    /// [`BlockProperties::hardness`] alone is the time for a *just
    /// adequate* tool, and says nothing about the one the player is
    /// holding. Pass a `tool` of 0 for bare hands.
    ///
    /// `Refused` when that tool does not get into that block at all --
    /// which is a different answer from "slowly", and is the whole of
    /// the tool ladder.
    pub break_seconds:
        unsafe extern "C" fn(HostHandle, BlockId, tool: BlockId, out: *mut f32) -> Status,
    /// Whether one block is the same *kind* as another, ignoring the
    /// variant bits an orientable block carries. A log lying east-west
    /// and a log lying north-south are two ids and one block.
    pub same_kind: unsafe extern "C" fn(HostHandle, BlockId, BlockId) -> bool,
}

/// The columns of the block table a mod can read.
///
/// A copy rather than a pointer into the host's own table, so that
/// growing `BlockDef` on the server is not an ABI break.
#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct BlockProperties {
    pub id: BlockId,
    /// Seconds to break with a just-adequate tool. Negative for
    /// "nothing takes this apart".
    pub hardness: f32,
    /// 0..15.
    pub opacity: u8,
    pub emission: u8,
    pub solid: bool,
    pub placeable: bool,
    pub falls: bool,
    pub container: bool,
    pub weight_kg: f32,
    pub stack_limit: u32,
    /// What breaking it yields, or 0.
    pub drops: BlockId,
}

/// Stacks lying on the ground.
#[repr(C)]
pub struct ItemsApi {
    /// Drops a stack into the world. `velocity` may be zero.
    pub spawn: unsafe extern "C" fn(
        HostHandle,
        at: Vec3,
        velocity: Vec3,
        ItemStack,
        out: *mut EntityId,
    ) -> Status,
    pub remove: unsafe extern "C" fn(HostHandle, EntityId) -> Status,
    /// How many are lying about.
    pub count: unsafe extern "C" fn(HostHandle) -> u32,
    /// Fills `out` with up to `cap` of the stacks within `radius` of a
    /// point, and writes how many there were.
    pub near: unsafe extern "C" fn(
        HostHandle,
        at: Vec3,
        radius: f32,
        out: *mut ItemStack,
        cap: usize,
        written: *mut usize,
    ) -> Status,

    // ---- since 2.0 ----
    /// Drops a stack that already has wear on it.
    ///
    /// [`ItemsApi::spawn`] throws away `ItemStack::damage`, and that is
    /// a repair: a half-spent axe a mod moved from a pack to the ground
    /// came back new. This is the door a worn thing has to come through.
    pub spawn_worn: unsafe extern "C" fn(
        HostHandle,
        at: Vec3,
        velocity: Vec3,
        ItemStack,
        out: *mut EntityId,
    ) -> Status,
    /// Sweeps up every stack within a radius, and says how many stacks
    /// went. **They are destroyed, not moved** -- this is the cleanup a
    /// mod does after a fight, not a way to collect loot.
    pub clear_near: unsafe extern "C" fn(
        HostHandle,
        at: Vec3,
        radius: f32,
        removed: *mut u32,
    ) -> Status,
    /// How long a dropped stack lasts before it is gone, in seconds.
    pub lifetime_seconds: unsafe extern "C" fn(HostHandle) -> f32,
    /// The most stacks that may lie in the world at once. Past this,
    /// [`ItemsApi::spawn`] answers [`Status::Refused`].
    pub capacity: unsafe extern "C" fn(HostHandle) -> u32,
}

/// Everything alive that is not a player.
#[repr(C)]
pub struct EntitiesApi {
    pub count: unsafe extern "C" fn(HostHandle) -> u32,
    /// Where one is.
    pub position: unsafe extern "C" fn(HostHandle, EntityId, out: *mut Vec3) -> Status,
    pub health: unsafe extern "C" fn(HostHandle, EntityId, out: *mut f32) -> Status,
    /// Hurts one, as a blow would.
    pub damage: unsafe extern "C" fn(HostHandle, EntityId, amount: f32) -> Status,
    /// Its species, as the game's own index, and the name for it.
    pub species: unsafe extern "C" fn(HostHandle, EntityId, out: *mut u32) -> Status,
    pub species_name: unsafe extern "C" fn(
        HostHandle,
        species: u32,
        out: *mut u8,
        cap: usize,
        written: *mut usize,
    ) -> Status,
    /// Puts one in the world.
    pub spawn: unsafe extern "C" fn(
        HostHandle,
        species: u32,
        at: Vec3,
        out: *mut EntityId,
    ) -> Status,
    pub remove: unsafe extern "C" fn(HostHandle, EntityId) -> Status,
    /// Every entity within a radius, into `out`.
    pub near: unsafe extern "C" fn(
        HostHandle,
        at: Vec3,
        radius: f32,
        out: *mut EntityId,
        cap: usize,
        written: *mut usize,
    ) -> Status,

    // ---- since 2.0 ----
    /// How many species there are, for walking the table.
    pub species_count: unsafe extern "C" fn(HostHandle) -> u32,
    /// What a species *is*, as numbers. See [`SpeciesInfo`].
    pub species_info:
        unsafe extern "C" fn(HostHandle, species: u32, out: *mut SpeciesInfo) -> Status,
    /// One of the things a species leaves behind, by index. The count is
    /// [`SpeciesInfo::drop_count`].
    pub species_drop: unsafe extern "C" fn(
        HostHandle,
        species: u32,
        index: u32,
        out: *mut ItemStack,
    ) -> Status,
    /// Puts health back. Never above the species' own maximum, which is
    /// why there is no `set_health`: a boar with forty health is not a
    /// boar, it is a bug two mods will disagree about.
    pub heal: unsafe extern "C" fn(HostHandle, EntityId, amount: f32) -> Status,
    /// Every entity alive, into `out`. The counterpart of
    /// [`PlayersApi::all`].
    pub all: unsafe extern "C" fn(
        HostHandle,
        out: *mut EntityId,
        cap: usize,
        written: *mut usize,
    ) -> Status,
    /// Kills one and leaves its spoils on the ground where it fell --
    /// which [`EntitiesApi::remove`] deliberately does not do.
    ///
    /// `remove` is "this animal was never here"; this is "this animal
    /// died". A mod despawning distant wildlife wants the first and a
    /// mod running a hunt wants the second, and a single call that did
    /// one of them would be wrong half the time.
    pub kill: unsafe extern "C" fn(HostHandle, EntityId) -> Status,
}

/// Who is here.
#[repr(C)]
pub struct PlayersApi {
    pub count: unsafe extern "C" fn(HostHandle) -> u32,
    /// Every connected player, into `out`.
    pub all: unsafe extern "C" fn(
        HostHandle,
        out: *mut PlayerId,
        cap: usize,
        written: *mut usize,
    ) -> Status,
    pub name: unsafe extern "C" fn(
        HostHandle,
        PlayerId,
        out: *mut u8,
        cap: usize,
        written: *mut usize,
    ) -> Status,
    pub position: unsafe extern "C" fn(HostHandle, PlayerId, out: *mut Vec3) -> Status,
    /// Moves one. Goes out as the same correction the anti-cheat uses,
    /// so the client obeys it without needing to know a mod was
    /// involved -- and the anti-cheat is told, so the jump does not read
    /// as the teleport hack it exists to catch.
    pub teleport: unsafe extern "C" fn(HostHandle, PlayerId, to: Vec3) -> Status,
    pub vitals: unsafe extern "C" fn(HostHandle, PlayerId, out: *mut PlayerVitals) -> Status,
    /// Hurts one, with a cause for the death screen. Armour is applied
    /// by the host, exactly as it is for a punch -- there is no way
    /// through here to bypass a cuirass.
    pub damage:
        unsafe extern "C" fn(HostHandle, PlayerId, amount: f32, cause: Str) -> Status,
    pub heal: unsafe extern "C" fn(HostHandle, PlayerId, amount: f32) -> Status,
    /// Feeds and waters, in the game's own units.
    pub feed: unsafe extern "C" fn(HostHandle, PlayerId, nourishment: f32) -> Status,
    pub water: unsafe extern "C" fn(HostHandle, PlayerId, hydration: f32) -> Status,
    pub kick: unsafe extern "C" fn(HostHandle, PlayerId, reason: Str) -> Status,
    pub is_operator: unsafe extern "C" fn(HostHandle, PlayerId) -> bool,

    // ---- since 1.1 ----
    /// Grants or withdraws flight.
    ///
    /// **The one call in this table that changes how a client's own
    /// physics behaves**, and the reason it can exist at all is that the
    /// server is still the judge: granting flight tells the anti-cheat
    /// as well as the client, so the two agree about what this player
    /// may do. A mod cannot use it to make somebody uncatchable -- reach
    /// on block edits, the rate limits and the block table are all still
    /// enforced. See `ServerMessage::Flight` on the host side.
    ///
    /// `speed` is blocks per second; pass `0.0` for the host's default.
    /// Ignored entirely when `enabled` is false.
    ///
    /// Idempotent, and cheap to call every tick: granting flight to
    /// somebody already flying at the same speed sends nothing.
    ///
    /// Flight does **not** survive the player disconnecting. A mod that
    /// wants it to has to remember who was flying and grant it again on
    /// `PlayerJoined` -- which is the honest place for that decision,
    /// because "should this still be on tomorrow" is a question about
    /// the mod's policy rather than about the player's body.
    pub set_flying:
        unsafe extern "C" fn(HostHandle, PlayerId, enabled: bool, speed: f32) -> Status,
    /// Whether they are flying now. False for a player who is not here.
    pub is_flying: unsafe extern "C" fn(HostHandle, PlayerId) -> bool,

    // ---- since 2.0 ----
    /// Which way they are facing, as a unit vector.
    ///
    /// The same basis the client's camera uses, so this and
    /// [`PhysicsApi::raycast`] compose into "what is this player looking
    /// at" without a mod deriving trigonometry from a yaw it was never
    /// given.
    pub look: unsafe extern "C" fn(HostHandle, PlayerId, out: *mut Vec3) -> Status,
    /// Whether the server believes they are standing on something. This
    /// is the anti-cheat's own answer, not the client's claim.
    pub on_ground: unsafe extern "C" fn(HostHandle, PlayerId) -> bool,
    /// Which hotbar slot is selected, and moving it.
    ///
    /// `set_selected_slot` fires [`Event::HeldSlotChanged`] exactly as
    /// the player's own key press does, so a mod watching for that hears
    /// its own change.
    pub selected_slot: unsafe extern "C" fn(HostHandle, PlayerId, out: *mut u32) -> Status,
    pub set_selected_slot: unsafe extern "C" fn(HostHandle, PlayerId, slot: u32) -> Status,
    /// Brings a dead player back at the spawn point, as the death screen
    /// does. [`Status::Refused`] for somebody who is not dead -- a
    /// respawn is not a teleport, and a mod that wanted one has
    /// [`PlayersApi::teleport`].
    pub respawn: unsafe extern "C" fn(HostHandle, PlayerId) -> Status,
    /// Sets health outright, clamped to 0..`max_health`.
    ///
    /// **Does not kill.** Setting zero leaves a player at zero health
    /// who has not died: dying is a whole event -- a cause, a death
    /// screen, a pack on the ground -- and inferring it from a number
    /// somebody wrote is how a player ends up dead with their inventory
    /// still on them. A mod that means to kill somebody calls
    /// [`PlayersApi::damage`] with enough of it and a cause worth
    /// reading.
    pub set_health: unsafe extern "C" fn(HostHandle, PlayerId, health: f32) -> Status,
    /// Sets how warm and how wet a body is, in the units
    /// [`PlayerVitals::body_temperature_c`] and
    /// [`PlayerVitals::wetness`] are in.
    ///
    /// The world will pull both back where it thinks they belong over
    /// the next few seconds -- this is a shove, not a clamp. A mod that
    /// wants somebody permanently warm shoves them every tick, which is
    /// cheap and is honest about being a policy rather than a fact.
    pub set_warmth: unsafe extern "C" fn(
        HostHandle,
        PlayerId,
        body_temperature_c: f32,
        wetness: f32,
    ) -> Status,
    /// Whether their head is under water right now -- the same test the
    /// server's own breath clock runs, including the twelve per cent at
    /// the top of a full cell that is water and does not look like it.
    ///
    /// There is no matching `set_breath`. Air is not a number a mod
    /// should be able to hand out: it is the clock on being under water,
    /// and a mod that wants somebody to survive a dive pulls them out or
    /// heals them. See `primitive_server::logic::survival::Vitals`,
    /// which offers no setter for the same reason.
    pub is_submerged: unsafe extern "C" fn(HostHandle, PlayerId) -> bool,
}

/// What a player is carrying and wearing.
#[repr(C)]
pub struct InventoryApi {
    /// How many slots a pack has.
    pub slot_count: unsafe extern "C" fn(HostHandle) -> u32,
    pub get_slot:
        unsafe extern "C" fn(HostHandle, PlayerId, slot: u32, out: *mut ItemStack) -> Status,
    /// Puts something in a slot, replacing what was there. The displaced
    /// stack is *dropped in the world* rather than deleted -- the host
    /// will not silently destroy a player's things on a mod's behalf.
    pub set_slot: unsafe extern "C" fn(HostHandle, PlayerId, slot: u32, ItemStack) -> Status,
    /// Adds to the pack wherever it fits; writes back how many did not.
    pub give: unsafe extern "C" fn(
        HostHandle,
        PlayerId,
        ItemStack,
        left_over: *mut u32,
    ) -> Status,
    /// Takes some out; writes back how many were actually found.
    pub take: unsafe extern "C" fn(
        HostHandle,
        PlayerId,
        BlockId,
        count: u32,
        taken: *mut u32,
    ) -> Status,
    pub count_of: unsafe extern "C" fn(HostHandle, PlayerId, BlockId, out: *mut u32) -> Status,
    /// What is on a body part.
    pub get_equipment:
        unsafe extern "C" fn(HostHandle, PlayerId, BodySlot, out: *mut ItemStack) -> Status,
    /// Puts something on. Refused for a block that is not a garment for
    /// that slot -- the host decides what fits where, not the mod.
    pub set_equipment:
        unsafe extern "C" fn(HostHandle, PlayerId, BodySlot, ItemStack) -> Status,

    // ---- since 2.0 ----
    /// What is in the selected hotbar slot -- what the player is
    /// holding, which is what every "may they do this" rule turns on.
    pub held: unsafe extern "C" fn(HostHandle, PlayerId, out: *mut ItemStack) -> Status,
    /// Empties the pack **into the world**, at the player's feet.
    ///
    /// Not into nothing. The host does not silently destroy a player's
    /// things on a mod's behalf, here or in
    /// [`InventoryApi::set_slot`]; a mod that means to destroy them
    /// takes them with [`InventoryApi::take`] first.
    pub spill: unsafe extern "C" fn(HostHandle, PlayerId, dropped: *mut u32) -> Status,
    /// Throws one slot down, exactly as the player's own drop gesture
    /// does -- from eye height, along their look, with the wear on it
    /// intact. Fires [`Event::ItemDropped`], and a mod that cancelled
    /// that event will find this call answering [`Status::Refused`].
    pub drop_slot: unsafe extern "C" fn(
        HostHandle,
        PlayerId,
        slot: u32,
        whole_stack: bool,
    ) -> Status,
    /// How much of a pack is spoken for, in kilograms, and what that
    /// costs. `carried_kg` is already on [`PlayerVitals`]; this is the
    /// other half -- how many slots are actually in use.
    pub used_slots: unsafe extern "C" fn(HostHandle, PlayerId, out: *mut u32) -> Status,
}

/// Chat, and a mod's own traffic.
#[repr(C)]
pub struct NetworkApi {
    pub broadcast: unsafe extern "C" fn(HostHandle, text: Str) -> Status,
    pub tell: unsafe extern "C" fn(HostHandle, PlayerId, text: Str) -> Status,
    /// Sends a mod's own bytes to one client, under a channel name.
    ///
    /// The host wraps it and delivers it; a client-side mod with the
    /// same channel name receives it. Present on a dedicated server and
    /// [`Status::Unavailable`] if no client on the other end understands
    /// the channel, which is the honest answer rather than a silent
    /// success.
    pub send_to: unsafe extern "C" fn(
        HostHandle,
        PlayerId,
        channel: Str,
        data: *const u8,
        len: usize,
    ) -> Status,
    /// The same to everybody.
    pub broadcast_data: unsafe extern "C" fn(
        HostHandle,
        channel: Str,
        data: *const u8,
        len: usize,
    ) -> Status,
}

/// Subscribing, and everything about how a mod is called back.
#[repr(C)]
pub struct EventsApi {
    /// Asks to be told about an event. A mod that subscribes to nothing
    /// is never called, which is what makes a dozen loaded mods cost
    /// nothing on a tick none of them cares about.
    pub subscribe: unsafe extern "C" fn(HostHandle, Event) -> Status,
    pub unsubscribe: unsafe extern "C" fn(HostHandle, Event) -> Status,
    /// Registers a command name, so `/mycommand` reaches this mod as an
    /// [`Event::Command`] rather than being reported as a typo.
    pub register_command:
        unsafe extern "C" fn(HostHandle, name: Str, help: Str) -> Status,
}

/// The numbers movement is judged against.
///
/// Read-only, deliberately. A mod that could change gravity for one
/// player would be a mod that desynchronises them from every client in
/// the world, because the client integrates its own motion and only the
/// *rules* are shared. What a mod can do is know the rules.
#[repr(C)]
pub struct PhysicsApi {
    pub gravity: unsafe extern "C" fn(HostHandle) -> f32,
    pub walk_speed: unsafe extern "C" fn(HostHandle) -> f32,
    pub sprint_speed: unsafe extern "C" fn(HostHandle) -> f32,
    pub jump_speed: unsafe extern "C" fn(HostHandle) -> f32,
    pub terminal_velocity: unsafe extern "C" fn(HostHandle) -> f32,
    /// Whether a cell stops a player.
    pub is_solid: unsafe extern "C" fn(HostHandle, BlockId) -> bool,
    /// How deep the liquid in a cell is, 0..1. Zero for anything that is
    /// not one.
    pub liquid_depth: unsafe extern "C" fn(HostHandle, BlockId) -> f32,
    /// Casts a ray through the world and answers what it hit.
    pub raycast: unsafe extern "C" fn(
        HostHandle,
        from: Vec3,
        direction: Vec3,
        max_distance: f32,
        hit: *mut BlockPos,
        normal: *mut BlockPos,
    ) -> Status,

    // ---- since 2.0 ----
    //
    // What a load does. Weight is already reported per player
    // (`PlayerVitals::carried_kg`); these are the *rules* it is read
    // against, which is the difference between a mod that can show a
    // number and one that can decide something.
    /// Most a player can carry at all, in kilograms.
    pub carry_capacity_kg: unsafe extern "C" fn(HostHandle) -> f32,
    /// What that load does to walking speed, as a multiplier.
    pub load_speed_scale: unsafe extern "C" fn(HostHandle, kilograms: f32) -> f32,
    /// ...and what it does to a fall, as a multiplier on the damage.
    pub load_fall_multiplier: unsafe extern "C" fn(HostHandle, kilograms: f32) -> f32,
    /// How fast a player crosses a given surface, and how well a foot
    /// holds on it. Both multipliers; one for most ground.
    pub block_drag: unsafe extern "C" fn(HostHandle, BlockId) -> f32,
    pub block_grip: unsafe extern "C" fn(HostHandle, BlockId) -> f32,
    /// How far a fall may be before it costs anything, in blocks.
    pub safe_fall_blocks: unsafe extern "C" fn(HostHandle) -> f32,
}

/// A mod's own persistent blob.
///
/// Deliberately opaque bytes rather than a structured store: a mod knows
/// what its own state is and the host does not, and a key/value API
/// would be the host inventing a schema for data it cannot read. The
/// blob is written beside the world, keyed by the mod's name, and is
/// handed back at load.
#[repr(C)]
pub struct SaveApi {
    /// Writes this mod's blob. Replaces whatever was there.
    pub store: unsafe extern "C" fn(HostHandle, data: *const u8, len: usize) -> Status,
    /// Reads it back. A `cap` of zero asks for the length.
    pub load: unsafe extern "C" fn(
        HostHandle,
        out: *mut u8,
        cap: usize,
        written: *mut usize,
    ) -> Status,
    /// Asks the host to flush everything now, as `/save` does.
    pub save_world: unsafe extern "C" fn(HostHandle) -> Status,
}

/// The recipe table, and making things out of it.
///
/// **Read and run, not grow.** There is no `add_recipe` and no
/// `remove_recipe`, and both absences are decisions:
///
/// - A recipe is *content the client draws its own screen from*. The
///   crafting menu is built on the client out of the same
///   `crafting::RECIPES` the server validates against, and the index
///   into that list is the recipe's identity on the wire. A row that
///   existed only on the server would be a craft with no menu entry --
///   a feature a player cannot find is not a feature.
/// - Taking one away is already expressible, and better expressed, by
///   cancelling [`Event::ItemCrafted`]. Two ways to disable a recipe
///   would be two things to keep in step, and the day they disagreed
///   the answer would depend on load order.
///
/// What is here is everything else: reading the table, asking why a
/// craft will not run, and running one.
#[repr(C)]
pub struct CraftingApi {
    /// How many recipes there are.
    pub recipe_count: unsafe extern "C" fn(HostHandle) -> u32,
    /// One row, without its lists. See [`RecipeInfo`].
    pub recipe: unsafe extern "C" fn(HostHandle, index: u32, out: *mut RecipeInfo) -> Status,
    /// What the menu calls it.
    pub recipe_name: unsafe extern "C" fn(
        HostHandle,
        index: u32,
        out: *mut u8,
        cap: usize,
        written: *mut usize,
    ) -> Status,
    /// One ingredient, by index. `RecipeInfo::input_count` of them.
    pub recipe_input: unsafe extern "C" fn(
        HostHandle,
        index: u32,
        which: u32,
        out: *mut ItemStack,
    ) -> Status,
    /// One thing handed *back* -- the crucible, the mould.
    /// `RecipeInfo::return_count` of them.
    pub recipe_return: unsafe extern "C" fn(
        HostHandle,
        index: u32,
        which: u32,
        out: *mut ItemStack,
    ) -> Status,
    /// Every recipe whose output is this block, into `out`. The question
    /// a mod asks to answer "how does a player get one of these".
    pub recipes_making: unsafe extern "C" fn(
        HostHandle,
        BlockId,
        out: *mut u32,
        cap: usize,
        written: *mut usize,
    ) -> Status,
    /// Every recipe this block is an ingredient of. The other direction,
    /// and the one a mod asks to answer "is this worth keeping".
    pub recipes_using: unsafe extern "C" fn(
        HostHandle,
        BlockId,
        out: *mut u32,
        cap: usize,
        written: *mut usize,
    ) -> Status,
    /// Whether this player could run this recipe where they are standing
    /// -- and if not, which of the four "no"s it is.
    pub feasibility: unsafe extern "C" fn(
        HostHandle,
        PlayerId,
        index: u32,
        out: *mut Feasibility,
    ) -> Status,
    /// What is burning within working range of a player.
    pub heat_at: unsafe extern "C" fn(HostHandle, PlayerId, out: *mut CraftHeat) -> Status,
    /// Runs a recipe against a player's pack, up to `times`, and says
    /// how many actually came out.
    ///
    /// Through the same path the player's own craft takes: the fire is
    /// checked against the *server's* idea of where they are standing,
    /// [`Event::ItemCrafted`] is fired and may be cancelled, and the new
    /// pack is pushed to the client. A mod that wanted to conjure
    /// something regardless has [`InventoryApi::give`], which is honest
    /// about being a gift rather than a craft.
    pub craft: unsafe extern "C" fn(
        HostHandle,
        PlayerId,
        index: u32,
        times: u32,
        made: *mut u32,
    ) -> Status,
    pub station_name: unsafe extern "C" fn(
        HostHandle,
        Station,
        out: *mut u8,
        cap: usize,
        written: *mut usize,
    ) -> Status,
}

/// How bright a cell is.
///
/// **Read-only, and it is not a light map.** The server does not keep
/// one: light is computed where it is drawn, which is the client, and a
/// server that maintained a second copy would be maintaining a second
/// copy that nothing reads. So these answers are computed *on demand*,
/// by flood-filling the containing chunk, and that costs about what
/// meshing one chunk's lighting costs -- which is to say, ask once and
/// remember it, and never once per cell across a region.
///
/// The isolation is visible at a seam and is stated rather than hidden:
/// the chunk is lit as though its four neighbours were walls, so a torch
/// on the far side of a chunk boundary does not reach across, and an
/// overhang whose open sky is in the next chunk reads darker than it
/// looks. Sunlight straight down a column -- which is what nearly every
/// caller means by "is it dark here" -- is exact, and
/// [`LightingApi::open_to_sky`] is exact everywhere.
#[repr(C)]
pub struct LightingApi {
    /// Sunlight at a cell, 0..15, before the time of day is applied.
    /// Fifteen at noon and fifteen at midnight: this is *how much sky
    /// reaches the cell*, and what the hour does to it is
    /// [`CoreApi::time_of_day`]'s business.
    pub sky_light: unsafe extern "C" fn(HostHandle, BlockPos, out: *mut u8) -> Status,
    /// Light from blocks that glow, 0..15. Not modulated by anything.
    pub block_light: unsafe extern "C" fn(HostHandle, BlockPos, out: *mut u8) -> Status,
    /// Whether there is nothing but air and glass between this cell and
    /// the sky.
    ///
    /// Exact, cheap, and free of the seam caveat above -- it walks one
    /// column. This is the call a spawn rule or a crop rule wants.
    pub open_to_sky: unsafe extern "C" fn(HostHandle, BlockPos) -> bool,
    /// The brightest a cell can be, 0..15.
    pub max_light: unsafe extern "C" fn(HostHandle) -> u8,
}

/// What is edible, what it is worth, and putting it in somebody.
#[repr(C)]
pub struct FoodApi {
    /// Can a player put this in their mouth at all?
    ///
    /// **True for the toadstool**, which is the point of it: a thing the
    /// game refuses to let you eat is a thing you cannot get wrong.
    pub is_food: unsafe extern "C" fn(HostHandle, BlockId) -> bool,
    /// What it feeds, in the units [`FoodApi::max_nourishment`] is in.
    /// `NotFound` for anything that is not nourishing -- including the
    /// toadstool, which is food and is not nourishment.
    pub nutrition: unsafe extern "C" fn(HostHandle, BlockId, out: *mut f32) -> Status,
    /// What eating one costs. `NotFound` for the rows that cost nothing.
    pub harm: unsafe extern "C" fn(HostHandle, BlockId, out: *mut FoodHarm) -> Status,
    /// A full stomach, in the game's own units.
    pub max_nourishment: unsafe extern "C" fn(HostHandle) -> f32,
    /// Whether eating this now would be worth it, or would throw most of
    /// it away on a stomach that is nearly full. The rule the client
    /// greys the gesture out with.
    pub worth_eating: unsafe extern "C" fn(HostHandle, PlayerId, BlockId) -> bool,
    /// Eats what is in a slot, through the player's own path: the
    /// nourishment, the harm, the weight, the death if it was the last
    /// of their health, and [`Event::PlayerAte`], which may be
    /// cancelled.
    ///
    /// A full jug in that slot is *drunk* rather than eaten, and the
    /// empty comes back -- the same as the player's own gesture, because
    /// it is the same gesture.
    pub eat: unsafe extern "C" fn(HostHandle, PlayerId, slot: u32) -> Status,
}

/// The rules a blow is judged by.
#[repr(C)]
pub struct CombatApi {
    /// How far a player can reach to hit something.
    pub melee_reach: unsafe extern "C" fn(HostHandle) -> f32,
    /// ...and the slack the server allows on top, for a client whose
    /// idea of where it is standing is one packet old.
    pub reach_tolerance: unsafe extern "C" fn(HostHandle) -> f32,
    /// A bare-handed blow.
    pub melee_damage: unsafe extern "C" fn(HostHandle) -> f32,
    /// The shortest gap between two accepted swings.
    pub melee_cooldown_seconds: unsafe extern "C" fn(HostHandle) -> f32,
    /// Whether one point could hit another, by the server's own rule.
    pub within_reach: unsafe extern "C" fn(HostHandle, from: Vec3, to: Vec3) -> bool,
    /// What a blow of `damage` would actually cost this player once
    /// their armour has taken its share.
    ///
    /// The number [`PlayersApi::damage`] will apply, offered separately
    /// so that a mod can decide *before* it hits -- and so that
    /// [`Event::PlayerHurt`] carries a number a mod can compare against
    /// something.
    pub damage_through_armour:
        unsafe extern "C" fn(HostHandle, PlayerId, damage: f32, out: *mut f32) -> Status,
    /// What a blow with this in hand is worth against an animal. Zero
    /// for a block, and bare hands for id 0.
    pub weapon_damage: unsafe extern "C" fn(HostHandle, BlockId, out: *mut f32) -> Status,
    /// The share of any blow that gets past any armour, however good.
    /// A tenth: armour that made a player immune would end every fight
    /// in the game the moment somebody finished a set.
    pub minimum_damage_fraction: unsafe extern "C" fn(HostHandle) -> f32,
}

/// Water: where it is, how deep, and taking it away.
///
/// Operations only. The flood simulation's own queue and its idea of
/// which cells are settling are not here and will not be: they have
/// been rewritten twice, and both times a mod that asked the water to
/// *do* something would have survived it.
#[repr(C)]
pub struct FluidApi {
    pub is_liquid: unsafe extern "C" fn(HostHandle, BlockId) -> bool,
    /// A full cell, which is what feeds everything downhill of it. The
    /// difference between a lake and a puddle that will drain.
    pub is_source: unsafe extern "C" fn(HostHandle, BlockId) -> bool,
    /// Puts a full cell of water down and wakes the flood.
    pub place_source: unsafe extern "C" fn(HostHandle, BlockPos) -> Status,
    /// Takes the water out of a cell and lets whatever is around it flow
    /// back in. `NotFound` when there was none.
    pub remove: unsafe extern "C" fn(HostHandle, BlockPos) -> Status,
    /// How deep the water is at a cell, in blocks, counting straight
    /// down until it stops being water. Zero for dry land.
    ///
    /// The question a mod asks before it drops something in, and the one
    /// [`PhysicsApi::liquid_depth`] cannot answer: that one is about a
    /// *block id* and this one is about a place.
    pub column_depth: unsafe extern "C" fn(HostHandle, BlockPos, out: *mut f32) -> Status,
    /// The height of the water's surface within its own cell, 0..1 --
    /// what a boat would float at and what a player's eyes are compared
    /// against.
    pub surface_height: unsafe extern "C" fn(HostHandle, BlockId, out: *mut f32) -> Status,
    /// How many cells the flood still has queued. A mod that has just
    /// dug a channel can watch this go back to zero rather than guessing
    /// at a number of ticks.
    pub pending: unsafe extern "C" fn(HostHandle) -> u32,
}

/// What is inside a chest, a hearth or a rack.
///
/// **One table for all three, because the server has one store for all
/// three.** A hearth is a container: everything written for chests --
/// opening, moving, spilling when it is broken, saving -- applies to it
/// unchanged, and what differs is only which slots take what. A mod that
/// wants to know which kind it is asks
/// [`ContainersApi::container_kind`].
#[repr(C)]
pub struct ContainersApi {
    /// Whether there is a container at this cell at all, and how many
    /// slots it has.
    pub slot_count: unsafe extern "C" fn(HostHandle, BlockPos, out: *mut u32) -> Status,
    /// Which of the four it is: 0 a chest, 1 a hearth of some kind,
    /// 2 a drying rack, 3 a set-down jug -- one slot, which takes only
    /// loose dry goods and only a jug's measure of them.
    pub container_kind: unsafe extern "C" fn(HostHandle, BlockPos, out: *mut u32) -> Status,
    pub get_slot:
        unsafe extern "C" fn(HostHandle, BlockPos, slot: u32, out: *mut ItemStack) -> Status,
    /// Puts something in a slot, replacing what was there. Whatever is
    /// displaced is dropped **in the world**, on the same rule
    /// [`InventoryApi::set_slot`] follows: the host does not destroy
    /// things on a mod's behalf.
    ///
    /// Refused when the slot will not take that block -- a hearth's fuel
    /// slot takes fuel, and a rack's second slot is where the leather
    /// comes out. What fits where is the container's business, not the
    /// mod's, exactly as it is for a garment.
    pub set_slot: unsafe extern "C" fn(HostHandle, BlockPos, slot: u32, ItemStack) -> Status,
    /// Adds wherever it fits; writes back how many did not.
    pub give:
        unsafe extern "C" fn(HostHandle, BlockPos, ItemStack, left_over: *mut u32) -> Status,
    /// Takes some out; writes back how many were found.
    pub take: unsafe extern "C" fn(
        HostHandle,
        BlockPos,
        BlockId,
        count: u32,
        taken: *mut u32,
    ) -> Status,
    /// Tips the whole thing onto the ground, as breaking it does.
    pub spill: unsafe extern "C" fn(HostHandle, BlockPos) -> Status,
    /// Every container in the world that is holding something, into
    /// `out`.
    pub all: unsafe extern "C" fn(
        HostHandle,
        out: *mut BlockPos,
        cap: usize,
        written: *mut usize,
    ) -> Status,
    /// Shuts this container's screen for everybody who has it open --
    /// which a mod that has just changed what is inside it, or moved it,
    /// has to do.
    pub close: unsafe extern "C" fn(HostHandle, BlockPos) -> Status,
}

/// Fires, smelting and curing: the things that work while the player
/// does something else.
#[repr(C)]
pub struct StationsApi {
    /// Sets a cell alight. False for a cell that will not burn.
    pub light_fire: unsafe extern "C" fn(HostHandle, BlockPos) -> Status,
    /// Adds fuel to one that is already burning, in seconds.
    pub feed_fire: unsafe extern "C" fn(HostHandle, BlockPos, seconds: f32) -> Status,
    /// Puts one out. Fires [`Event::FireDied`].
    pub extinguish: unsafe extern "C" fn(HostHandle, BlockPos) -> Status,
    /// How much longer it has, in seconds. `NotFound` for a cell that is
    /// not burning.
    pub fire_fuel_left: unsafe extern "C" fn(HostHandle, BlockPos, out: *mut f32) -> Status,
    /// What this block is worth as fuel, in seconds. `NotFound` for
    /// anything that does not burn.
    pub fuel_seconds: unsafe extern "C" fn(HostHandle, BlockId, out: *mut f32) -> Status,
    /// Whether anything is burning within `range` of a point -- the
    /// question `Station::Heat` actually asks.
    pub fire_within: unsafe extern "C" fn(HostHandle, at: Vec3, range: f32) -> bool,
    /// How many cells are alight in the whole world.
    pub burning_count: unsafe extern "C" fn(HostHandle) -> u32,
    /// How far along the batch in this hearth is, 0..1. `NotFound` for a
    /// cell that is not a lit hearth with something in it.
    pub smelting_progress: unsafe extern "C" fn(HostHandle, BlockPos, out: *mut f32) -> Status,
    /// How far along the skin on this rack is, 0..1.
    pub drying_progress: unsafe extern "C" fn(HostHandle, BlockPos, out: *mut f32) -> Status,
    /// Moves that along -- including to 1.0, which finishes it on the
    /// next step and fires [`Event::HideCured`] the way twelve minutes
    /// of waiting would.
    pub set_drying_progress:
        unsafe extern "C" fn(HostHandle, BlockPos, progress: f32) -> Status,
    /// What a raw skin turns into. `NotFound` for anything that does not
    /// cure.
    pub cures_into: unsafe extern "C" fn(HostHandle, BlockId, out: *mut BlockId) -> Status,
    /// How fast a rack at this point is drying right now, as a fraction
    /// of its best -- which is what the weather, the hour and a fire
    /// beside it add up to. Zero means it has stopped.
    pub drying_rate: unsafe extern "C" fn(HostHandle, at: Vec3, out: *mut f32) -> Status,
}

/// The world moving on its own: sand falling, plants growing, trees
/// coming down.
///
/// Operations, again. There is no call that hands out the queue any of
/// these is working through -- see the note at the top of this file.
#[repr(C)]
pub struct SimulationApi {
    /// How many cells the falling-block simulation still has to look at.
    pub falling_pending: unsafe extern "C" fn(HostHandle) -> u32,
    /// How many blocks are in the air right now.
    pub falling_entities: unsafe extern "C" fn(HostHandle) -> u32,
    /// How many cells are waiting to grow, and how many are part way
    /// there.
    pub growth_pending: unsafe extern "C" fn(HostHandle) -> u32,
    /// Asks the growth pass to look around a point.
    ///
    /// Growth is sampled outward from where people are, because a world
    /// that ripened everywhere would spend its whole tick budget on
    /// terrain nobody is standing in. This is how a mod makes it look
    /// somewhere else -- a farm nobody is visiting.
    pub watch_growth: unsafe extern "C" fn(HostHandle, at: Vec3) -> Status,
    /// How long a picked bush takes to fill again, and how long a crop
    /// spends in each of its stages, in seconds.
    pub regrow_seconds: unsafe extern "C" fn(HostHandle) -> f32,
    pub crop_stage_seconds: unsafe extern "C" fn(HostHandle) -> f32,
    /// Whether this block is a trunk that is still standing -- as
    /// opposed to the same log lying down, which is deadfall and is
    /// pulled apart rather than felled.
    pub is_standing_trunk: unsafe extern "C" fn(HostHandle, BlockId) -> bool,
    /// Brings a tree down: the trunk lies along the way it fell, the
    /// canopy comes with it, and anything standing underneath is
    /// crushed. Writes how many cells changed.
    ///
    /// Fires [`Event::TreeFelled`], which may be cancelled -- and then
    /// this answers [`Status::Refused`] and the tree is still standing.
    pub fell_tree:
        unsafe extern "C" fn(HostHandle, BlockPos, felled: *mut u32) -> Status,
}

/// Client-side drawing. **Null on a dedicated server.**
#[repr(C)]
pub struct RenderApi {
    /// Registers a texture from raw RGBA. Answers the layer it landed
    /// on, which is what a mod's own block definition refers to.
    pub register_texture: unsafe extern "C" fn(
        HostHandle,
        name: Str,
        rgba: *const u8,
        width: u32,
        height: u32,
        layer: *mut u32,
    ) -> Status,
    /// Draws a line in the world this frame. The debug primitive, and
    /// the only drawing that does not need a mesh.
    pub debug_line:
        unsafe extern "C" fn(HostHandle, from: Vec3, to: Vec3, rgba: u32) -> Status,
}

/// Client-side sound. **Null on a dedicated server.**
#[repr(C)]
pub struct AudioApi {
    pub play_at: unsafe extern "C" fn(HostHandle, name: Str, at: Vec3, volume: f32) -> Status,
    pub play_ui: unsafe extern "C" fn(HostHandle, name: Str, volume: f32) -> Status,
}

/// Client-side interface. **Null on a dedicated server.**
#[repr(C)]
pub struct UiApi {
    /// Puts a line on the player's screen for a while.
    pub toast: unsafe extern "C" fn(HostHandle, text: Str, seconds: f32) -> Status,
    /// Adds a line to the debug panel, under the mod's name.
    pub debug_line: unsafe extern "C" fn(HostHandle, text: Str) -> Status,
}

/// Everything the host offers, in one place.
///
/// The sub-tables are pointers rather than inline structs so that a
/// table can be absent -- which is what a dedicated server does with the
/// three client-side ones, and what an embedded server does with
/// anything it has no business serving.
#[repr(C)]
pub struct HostApi {
    /// What the host implements. A mod must check
    /// [`ApiVersion::accepts`] against its own before using anything,
    /// and the host checks it too -- both, because a mod loaded by an
    /// older host would otherwise read past the end of these tables
    /// before it ever got the chance to complain.
    pub version: ApiVersion,
    /// Passed back to every call. See [`HostHandle`].
    pub handle: HostHandle,

    pub core: *const CoreApi,
    pub world: *const WorldApi,
    pub generation: *const GenerationApi,
    pub blocks: *const BlocksApi,
    pub items: *const ItemsApi,
    pub entities: *const EntitiesApi,
    pub players: *const PlayersApi,
    pub inventory: *const InventoryApi,
    pub network: *const NetworkApi,
    pub events: *const EventsApi,
    pub physics: *const PhysicsApi,
    pub save: *const SaveApi,
    /// Null on a dedicated server. See the module note.
    pub render: *const RenderApi,
    pub audio: *const AudioApi,
    pub ui: *const UiApi,

    // ---- since 2.0 ----
    //
    // **Appended, and after the three null ones.** The client-side
    // pointers keep their offsets even though they are null here,
    // because a mod that checks `render.is_null()` is reading a field
    // and a field that moved is a field read from the wrong place. New
    // tables go on the end, always, whatever the tidier order would
    // have been.
    pub crafting: *const CraftingApi,
    pub lighting: *const LightingApi,
    pub food: *const FoodApi,
    pub combat: *const CombatApi,
    pub fluid: *const FluidApi,
    pub containers: *const ContainersApi,
    pub stations: *const StationsApi,
    pub simulation: *const SimulationApi,
}

// Safe to send: it is a bundle of function pointers and one opaque
// handle, and the host guarantees the handle is valid for as long as the
// mod is loaded. The host is what synchronises access; this assertion is
// about the *bundle* being movable, not about the state behind it.
unsafe impl Send for HostApi {}
unsafe impl Sync for HostApi {}

// ------------------------------------------------------------- mod side

/// What a mod hands back from its entry point.
#[repr(C)]
pub struct ModDescriptor {
    /// The version of *this crate* the mod was built against. Checked by
    /// the host before anything else is called.
    pub api_version: ApiVersion,
    /// The mod's own name. Must match the manifest, and the host says so
    /// if it does not -- a mod whose library and manifest disagree about
    /// its own name is a mod whose settings and save blob would go to
    /// the wrong place.
    pub name: Str,
    pub version: Str,
    /// Called once, after the host has finished loading every mod and
    /// resolved the dependency order. This is where a mod subscribes.
    pub on_load: Option<unsafe extern "C" fn(user: *mut c_void, host: *const HostApi) -> Status>,
    /// Called once on shutdown, before the world is saved.
    pub on_unload: Option<unsafe extern "C" fn(user: *mut c_void)>,
    /// Called for every event the mod subscribed to.
    pub on_event: Option<
        unsafe extern "C" fn(user: *mut c_void, Event, *const EventData) -> HookResult,
    >,
    /// The mod's own state, handed back to it on every call. The host
    /// never dereferences it.
    pub user: *mut c_void,
}

impl Default for ModDescriptor {
    fn default() -> Self {
        Self {
            api_version: API_VERSION,
            name: Str::EMPTY,
            version: Str::EMPTY,
            on_load: None,
            on_unload: None,
            on_event: None,
            user: core::ptr::null_mut(),
        }
    }
}

/// Writes the exported entry point for a mod.
///
/// ```ignore
/// primitive_modapi::declare_mod! {
///     name: "bigger_caves",
///     version: "1.0.0",
///     load: my_load,
///     event: my_event,
/// }
/// ```
#[macro_export]
macro_rules! declare_mod {
    (
        name: $name:expr,
        version: $version:expr,
        load: $load:path,
        event: $event:path $(,)?
    ) => {
        #[no_mangle]
        pub unsafe extern "C" fn primitive_mod_register(
            _host: *const $crate::HostApi,
        ) -> $crate::ModDescriptor {
            unsafe extern "C" fn load_shim(
                user: *mut ::core::ffi::c_void,
                host: *const $crate::HostApi,
            ) -> $crate::Status {
                let _ = user;
                $load(host)
            }
            unsafe extern "C" fn event_shim(
                user: *mut ::core::ffi::c_void,
                event: $crate::Event,
                data: *const $crate::EventData,
            ) -> $crate::HookResult {
                let _ = user;
                $event(event, data)
            }
            $crate::ModDescriptor {
                api_version: $crate::API_VERSION,
                name: $crate::Str::borrow($name),
                version: $crate::Str::borrow($version),
                on_load: Some(load_shim),
                on_unload: None,
                on_event: Some(event_shim),
                user: ::core::ptr::null_mut(),
            }
        }
    };
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_host_takes_an_older_minor_and_refuses_a_newer_one() {
        // The whole compatibility story, as three lines. A mod built
        // against 1.2 on a 1.5 host reads the first 1.2-worth of every
        // table, which is still there; the other way round it would read
        // a field that does not exist.
        let host = ApiVersion::new(1, 5);
        assert!(host.accepts(ApiVersion::new(1, 2)));
        assert!(host.accepts(ApiVersion::new(1, 5)));
        assert!(!host.accepts(ApiVersion::new(1, 6)));
        assert!(!host.accepts(ApiVersion::new(2, 0)));
        assert!(!host.accepts(ApiVersion::new(0, 9)));
    }

    #[test]
    fn a_borrowed_string_comes_back_out_the_same() {
        let text = "bigger caves";
        let borrowed = Str::borrow(text);
        assert_eq!(unsafe { borrowed.as_str() }, text);
        // ...and the empty one is not a null dereference.
        assert_eq!(unsafe { Str::EMPTY.as_str() }, "");
    }

    #[test]
    fn every_table_is_pointer_sized_and_the_bundle_is_plain_data() {
        // The property the whole boundary rests on: `HostApi` is a
        // version, a handle and a run of pointers, so a mod compiled by
        // a different toolchain reads it correctly. If somebody adds a
        // `String` to it, this stops being true and the failure is a
        // mod reading garbage rather than a compile error -- so it is
        // asserted here.
        let expected = core::mem::size_of::<ApiVersion>()
            + core::mem::size_of::<HostHandle>()
            + 23 * core::mem::size_of::<*const u8>();
        // Padding after the four-byte version on a 64-bit target is
        // real, so the check is "no larger than the fields plus one
        // pointer of padding" rather than an exact equality.
        assert!(
            core::mem::size_of::<HostApi>() <= expected + core::mem::size_of::<*const u8>(),
            "HostApi is {} bytes, expected about {expected}",
            core::mem::size_of::<HostApi>()
        );
    }

    #[test]
    fn every_call_the_previous_version_had_is_still_where_it_was() {
        // **The property the whole additive discipline rests on**, and
        // the one thing a major bump does *not* excuse. 2.0 refuses a
        // 1.1 mod at the manifest, so this is not about loading one --
        // it is about the mod that is rebuilt against 2.0 with nothing
        // changed but the one renamed field. If a new call had been
        // inserted in the middle of a table rather than appended, that
        // rebuild would silently call the wrong function pointer, and
        // the symptom would be a mod whose "give the player a torch"
        // teleported somebody instead.
        //
        // Offsets rather than a size, because a size only catches
        // *shrinking*. These are the last field of every table as 1.1
        // left it; each must still sit at the index 1.1 put it at.
        use core::mem::{offset_of, size_of};
        let p = size_of::<*const u8>();
        assert_eq!(offset_of!(CoreApi, run_command), 7 * p, "CoreApi");
        assert_eq!(offset_of!(WorldApi, temperature_at), 10 * p, "WorldApi");
        assert_eq!(
            offset_of!(GenerationApi, register_decorator),
            4 * p,
            "GenerationApi"
        );
        assert_eq!(offset_of!(BlocksApi, properties), 4 * p, "BlocksApi");
        assert_eq!(offset_of!(ItemsApi, near), 3 * p, "ItemsApi");
        assert_eq!(offset_of!(EntitiesApi, near), 8 * p, "EntitiesApi");
        assert_eq!(offset_of!(PlayersApi, is_flying), 13 * p, "PlayersApi");
        assert_eq!(offset_of!(InventoryApi, set_equipment), 7 * p, "InventoryApi");
        assert_eq!(offset_of!(PhysicsApi, raycast), 7 * p, "PhysicsApi");
        // ...and the bundle, whose three client-side pointers are read
        // by name and checked for null by every mod that draws
        // anything.
        assert_eq!(offset_of!(HostApi, ui), offset_of!(HostApi, audio) + p);
        assert_eq!(offset_of!(HostApi, crafting), offset_of!(HostApi, ui) + p);
    }

    #[test]
    fn a_mod_built_against_the_previous_major_is_refused_rather_than_loaded() {
        // The body's temperature changed meaning under it (see the note
        // beside `API_VERSION`), and a mod that read it is now wrong
        // rather than broken. Wrong is the failure a version number
        // exists to prevent, so this host must say no.
        assert!(!API_VERSION.accepts(ApiVersion::new(1, 1)));
        assert!(!API_VERSION.accepts(ApiVersion::new(1, 0)));
        // ...and it must still say yes to everything built against
        // itself, including a mod that only uses the 1.1-era subset.
        assert!(API_VERSION.accepts(ApiVersion::new(2, 0)));
    }

    #[test]
    fn no_out_parameter_struct_grew_when_the_tables_did() {
        // A mod allocates one of these on its own stack and hands the
        // host a pointer to it. Growing one has the host write past the
        // end of a buffer sized by a mod compiled against the older
        // version -- which is a stack smash rather than a wrong answer,
        // and no version check catches it, because the check passed.
        //
        // The numbers are what 1.1 shipped. New columns arrive as new
        // structs; see `BlockTooling`.
        use core::mem::size_of;
        assert_eq!(size_of::<BlockProperties>(), 28, "BlockProperties");
        assert_eq!(size_of::<PlayerVitals>(), 40, "PlayerVitals");
        assert_eq!(size_of::<ItemStack>(), 12, "ItemStack");
    }

    #[test]
    fn status_ok_is_zero() {
        // So that `status == 0` reads correctly from C, which is the
        // only reading a C mod author will try first.
        assert_eq!(Status::Ok as i32, 0);
        assert!(Status::Ok.is_ok());
        assert!(!Status::NotFound.is_ok());
    }
}
