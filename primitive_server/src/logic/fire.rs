//! Fires: what is burning, for how much longer, and what puts them out.
//!
//! ## The furnace that is not a furnace
//!
//! Smelting used to be a recipe you could run standing in a field, and
//! the note in `crafting` said so plainly: the fire was "implied". The
//! honest alternative was a furnace -- a block with an inside, a fuel
//! slot, a burn timer and a screen to watch it in -- and none of that is
//! about metal.
//!
//! What is here instead is a fire that is a *place*: a block you build,
//! light with flint, feed with whatever burns, and lose to the rain. The
//! interface to it is standing next to it (see `crafting::Station`).
//! There is no screen, no fuel slot and nothing to watch, and yet every
//! one of the things the furnace was for is here -- fuel that runs out,
//! a reason to gather wood, and a structure worth building a roof over.
//!
//! ## Why the state lives here rather than in the block id
//!
//! A block id has three spare bits, and eight levels of "how much fuel
//! is left" would have fitted in them -- saved for free, replicated for
//! free, and no file of its own. It was the first design and it is
//! wrong for one reason: **every change to it is a block update**. A
//! fire burning down through eight levels sends eight `BlockUpdate`s to
//! every player who can see it, and worse, every one of those is an edit
//! to the world overlay, so a lit campfire would make the autosave
//! rewrite `edits.bin` for as long as it burned. That is exactly the
//! bug the water simulation was fixed for in 1.4.
//!
//! So the fuel is a number in a map here, keyed by position like a
//! chest's contents, and the *only* thing that reaches the world is the
//! moment a fire goes out.
//!
//! ## Why it is saved
//!
//! Because a fire is a place with state, and a player who logs out
//! beside one that is nearly out should not log back in beside one with
//! a full load. `fires.bin`, its own file with its own version, for the
//! reason chests have one: a world saved before fires existed simply has
//! no such file, which reads as "nothing is burning", which is right.
//!
//! ## The heat
//!
//! A fire here is TerraFirmaCraft's firepit: it has a **temperature**, and
//! the temperature is what a batch asks for (`hearth::needs_degrees`).
//! Four things decide it, and each is a rule a player can hold:
//!
//! * **What is burning.** Every fuel burns at a heat as well as for a time
//!   (`hearth::fuel_degrees`); the fire climbs towards the heat of its
//!   hottest fuel, and no higher.
//! * **What it is built as.** A kiln's walls and a bloomery's shaft add
//!   their draught (`hearth::Kind::draught`).
//! * **The rain.** A fire the rain reaches aims three hundred degrees
//!   lower -- TerraFirmaCraft's figure -- *and* burns its fuel faster, as
//!   it always did here. So a wet fire stops pouring metal long before it
//!   goes out, which is the warning.
//! * **Time.** The temperature climbs and falls at a rate rather than
//!   jumping, so a fire has to come up to heat, and a fire that has gone
//!   out is still hot for a while -- and still cooking what it is hot
//!   enough for.
//!
//! ### Why the fuel is a list of loads
//!
//! TerraFirmaCraft keeps one number of burn time and one of temperature,
//! and the temperature is whatever it burnt *last*. Here that would be an
//! exploit in one move: a fire with twenty minutes of wood in it and one
//! lump of charcoal thrown on top burns all twenty minutes at charcoal's
//! heat. It only works in TerraFirmaCraft because its fire takes the next
//! item only when the last is gone. Here a hotter fuel has to go on at
//! once -- otherwise a player who lights a kiln for copper watches the
//! laid kindling burn for four minutes before the charcoal in the slot is
//! touched -- and so what is in the fire has to be kept apart.
//!
//! The rejected third way was one number of fuel and a running average of
//! temperature, weighted by seconds. It cannot be gamed, but it cannot be
//! read either: a fire whose heat is a blend of everything ever put in it
//! is a fire nobody can predict, and the gauge is only worth drawing if a
//! player can say what it will do next. Loads, burnt hottest first, say
//! exactly that: the fire is as hot as the hottest thing still in it.
//!
//! ### What is saved, and why
//!
//! * **The loads**, because they are what the player put there. Losing
//!   them to a restart would turn a kiln full of charcoal into a kiln of
//!   laid kindling that pours nothing.
//! * **The temperature**, because it costs a float and without it every
//!   restart is a minute of cold hearths with every batch stalled.
//! * **Not the embers** of fires that have gone out. A minute of cooling
//!   lost to a restart is not worth a file format.

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use primitive_shared::hearth::Kind;
use primitive_shared::protocol::BlockChange;
use primitive_shared::types::{
    is_burning, BlockId,
};
use primitive_shared::weather::Weather;

use crate::logic::falling::BlockWorld;

/// Its own version, independent of the world's. See the note above.
///
/// 2: a fire remembers what it is burning and how hot it is. Version 1
/// files are still read -- see `Fires::load` -- because a world whose
/// fires all came back cold and empty after an update would be a world
/// whose player lost a kiln full of charcoal to a version number.
const SAVE_FORMAT_VERSION: u32 = 2;

/// Where a fire is, in global block coordinates.
pub type FirePos = (i32, i32, i32);

/// How long a freshly lit fire burns on what it was built from.
///
/// Four minutes, which is a bit under half an in-game day at the default
/// clock. Long enough to smelt a pack of ore or cook an evening's meat
/// without standing over it; short enough that a fire you want *tomorrow*
/// is a fire you have to think about fuel for.
pub const INITIAL_FUEL_SECONDS: f32 = 240.0;

/// The most fuel a fire will hold.
///
/// A cap rather than a bottomless pit, because without one a player with
/// a stack of coal makes an eternal fire in one gesture and the whole
/// mechanic evaporates. Twenty minutes is two in-game days: enough to
/// leave a fire going while you go mining, not enough to forget it
/// exists.
pub const MAX_FUEL_SECONDS: f32 = 1200.0;

/// How hot a fire burns on what it was laid with.
///
/// A fire is laid of sticks over whatever branch was to hand, which burns
/// like birch -- warm enough to cook, to make charcoal and to fire a pot
/// in a kiln, and nowhere near metal. Cooler than the sticks themselves
/// ([`primitive_shared::hearth::fuel_degrees`]), because a laid fire is
/// kindling under wood rather than kindling alone, and because a laid
/// kiln that could forge iron before anyone had fed it would make the
/// fuel slot optional.
pub const LAID_FIRE_C: f32 = 650.0;

/// How fast a fire climbs towards the heat of what it is burning, in
/// degrees a second.
///
/// **Faster than TerraFirmaCraft's** (twenty a second, one a tick), and in
/// proportion: this world's batches take seconds where its take minutes.
/// A wood fire is at cooking heat in seven seconds and at its own heat in
/// twenty-five; a charcoal fire needs three quarters of a minute to reach
/// copper. Long enough that a fire has to catch; short enough that
/// "wait for it to come up" is a breath and not a chore.
pub const RISE_PER_SECOND: f32 = 30.0;

/// How fast a fire falls, towards what it is now burning or towards cold.
///
/// **Half the rise**, where TerraFirmaCraft has the two equal. Its firepit
/// has no stones; this one is a ring of them, and a kiln is clay walls,
/// and both hold heat after the flame is gone. A charcoal fire that runs
/// out is still hot enough for copper for a few seconds, still cooking
/// supper for a minute and a half -- which is what makes letting a fire go
/// out between batches a thing a player can decide to do.
pub const FALL_PER_SECOND: f32 = 15.0;

/// How much cooler the rain makes a fire it can reach, in degrees, at
/// full storm.
///
/// TerraFirmaCraft's three hundred. It sits beside the old rule rather
/// than replacing it -- wet fuel still burns away faster, see
/// `WET_BURN_MULTIPLIER` -- because the two say different things. The
/// faster burn is "this fire will go out"; the cooling is "this fire has
/// stopped working", and it comes first, so a player sees the batch stall
/// while there is still time to put a roof over it.
pub const RAIN_COOLING_C: f32 = 300.0;

/// One helping of fuel in a fire: seconds of it left, and how hot it
/// burns.
type Load = (f32, f32);

/// One burning fire.
#[derive(Debug, Clone)]
struct Fire {
    /// What is in it, **hottest first**. See "Why the fuel is a list of
    /// loads" at the top of the file. Loads of equal heat are merged, so
    /// a fire fed on one fuel is one entry however long it burns.
    loads: Vec<Load>,
    /// How hot it is now.
    degrees: f32,
    /// What the hearth adds, from the block last time it was readable.
    /// Kept rather than read every time because a fire in an evicted
    /// chunk goes on burning, and forgetting that it was a kiln would
    /// cool it for no reason.
    draught: f32,
    /// Whether rain reached it on the last tick. For the screen, which
    /// says so; not saved, because the next tick works it out again.
    exposed: bool,
}

impl Fire {
    /// A fire as it is laid: the kindling it was built from, at whatever
    /// heat the hearth still had in it.
    fn laid(degrees: f32) -> Fire {
        Fire {
            loads: vec![(INITIAL_FUEL_SECONDS, LAID_FIRE_C)],
            degrees,
            draught: 0.0,
            exposed: false,
        }
    }

    fn fuel(&self) -> f32 {
        self.loads.iter().map(|&(seconds, _)| seconds).sum()
    }

    /// How many seconds of fuel at least this hot are left.
    fn seconds_at_least(&self, degrees: f32) -> f32 {
        self.loads
            .iter()
            .filter(|&&(_, heat)| heat >= degrees)
            .map(|&(seconds, _)| seconds)
            .sum()
    }

    /// Puts fuel in, up to the cap. Answers false if there was no room.
    fn add(&mut self, seconds: f32, degrees: f32) -> bool {
        let room = MAX_FUEL_SECONDS - self.fuel();
        if room <= 0.0 {
            return false;
        }
        let seconds = seconds.min(room);
        match self.loads.iter_mut().find(|(_, heat)| *heat == degrees) {
            Some(load) => load.0 += seconds,
            None => {
                let at = self.loads.partition_point(|&(_, heat)| heat > degrees);
                self.loads.insert(at, (seconds, degrees));
            }
        }
        true
    }

    /// Burns this many seconds away, hottest first.
    fn burn(&mut self, mut seconds: f32) {
        while seconds > 0.0 {
            let Some(first) = self.loads.first_mut() else {
                return;
            };
            if first.0 > seconds {
                first.0 -= seconds;
                return;
            }
            seconds -= first.0;
            self.loads.remove(0);
        }
    }

    /// What it is heading for.
    fn target(&self, rain_cooling: f32) -> f32 {
        let burning = self.loads.first().map_or(0.0, |&(_, heat)| heat);
        let wet = if self.exposed { rain_cooling } else { 0.0 };
        (burning + self.draught - wet).max(0.0)
    }
}

/// One tick of a temperature moving towards where it is heading.
fn approach(degrees: f32, target: f32, dt: f32) -> f32 {
    if degrees < target {
        (degrees + RISE_PER_SECOND * dt).min(target)
    } else {
        (degrees - FALL_PER_SECOND * dt).max(target)
    }
}

/// What one of something is worth as fuel, or `None` if it does not
/// burn.
///
/// The ordering is the only part that matters and it is the real one:
/// coal is worth more than wood, a whole log is worth more than the
/// planks cut from it are individually, and a handful of sticks is what
/// you feed a fire when you have nothing else. Sticks are deliberately
/// poor -- a fire fed on kindling alone is a fire you spend your evening
/// tending.
pub fn fuel_value(block: BlockId) -> Option<f32> {
    // The table moved to the shared crate when the fire grew a fuel
    // slot: the client has to know what that slot will take before it
    // lets a player drag something into it, and a second copy of this
    // list is a fire that accepts a log the server will not burn.
    primitive_shared::hearth::fuel_seconds(block)
}

/// Can this be fed to a fire at all?
#[inline]
pub fn is_fuel(block: BlockId) -> bool {
    fuel_value(block).is_some()
}

/// What lights a fire.
///
/// A nodule of flint, struck against the stones of the ring. Not a
/// flake, which is the sharp waste and not the striker, and not a
/// finished tool -- you do not light a fire with your knife, you light
/// it with the rock the knife was made from. It is also the one thing a
/// player certainly has by the time they can build a fire, since the
/// fire is cobble and sticks and the cobble came out of knapping.
///
/// ## Every strike that lights a fire spends the nodule
///
/// The player: "after striking fire the flint should break". It used to
/// be kept, on the argument that a player out of flint could never light
/// a fire again -- and that argument stopped being true when gravel could
/// be sifted for flint anywhere there is gravel (`crafting`, "sifted
/// gravel"). What spending it buys is the decision the fire was missing:
/// a fire kept fed through the night is a nodule saved, and the nodule
/// is also the knife and the spear.
///
/// Three ways were weighed:
///
/// * **A chance to break per strike.** A roll a player cannot see is a
///   roll they cannot plan around, which is the reason `pit` gives for
///   refusing TerraFirmaCraft's dice; "how many fires is this flint worth"
///   has to have an answer.
/// * **Wear, like a tool's.** Flint stacks, and wear lives on the stack
///   (`Stack::damage`): eight nodules and one of them half-worn are two
///   stacks, the knapping recipes would have to say which they take, and
///   the pile in the pack would stop being a pile.
/// * **One nodule per fire that catches (chosen).** Only once it has
///   caught: a strike refused -- a fire already lit, a pit with nothing to
///   fire, rain on an open kiln -- costs nothing, so the price is for a
///   fire and never for a click. See `spend_striker` on the server.
pub const STRIKER: BlockId = primitive_shared::types::BLOCK_FLINT;

#[derive(Serialize, Deserialize)]
struct SaveFile {
    version: u32,
    /// Where, what is in it, and how hot it is.
    fires: Vec<(FirePos, Vec<Load>, f32)>,
}

/// Just the version, read first so the rest can be read by the right
/// shape. Bincode is positional, and both shapes start with it.
#[derive(Deserialize)]
struct SaveVersion {
    version: u32,
}

/// What version 1 wrote: where, and seconds of fuel.
#[derive(Deserialize)]
struct SaveFileV1 {
    _version: u32,
    fires: Vec<(FirePos, f32)>,
}

/// Every fire that is burning, and how much longer it has.
#[derive(Default)]
pub struct Fires {
    /// Position -> what is burning there.
    burning: HashMap<FirePos, Fire>,
    /// Hearths that have gone out and are still hot: position -> degrees.
    ///
    /// Apart from `burning`, because everything that asks "is there a
    /// fire here" -- warmth, the rain, the scan a recipe runs -- means a
    /// flame, and a cooling kiln is not one. Only the heat is still there,
    /// and the one thing that asks about heat is the hearth's own batch.
    embers: HashMap<FirePos, f32>,
    /// Cells that changed and have not been looked at yet. See the
    /// `CellMechanic` contract: `on_block_changed` may not touch the
    /// world, so all it can do is write the coordinate down.
    pending: Vec<FirePos>,
    /// What the sky is doing, as of the last tick. Rain puts fires out,
    /// which is the whole reason a roof is worth building.
    weather: Weather,
    dirty: bool,
}

impl Fires {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn len(&self) -> usize {
        self.burning.len()
    }

    pub fn is_empty(&self) -> bool {
        self.burning.is_empty()
    }

    pub fn is_dirty(&self) -> bool {
        self.dirty
    }

    pub fn set_weather(&mut self, weather: Weather) {
        self.weather = weather;
    }

    /// How much fuel the fire at `at` has left, if it is burning.
    pub fn fuel_left(&self, at: FirePos) -> Option<f32> {
        self.burning.get(&at).map(Fire::fuel)
    }

    /// How hot the hearth at `at` is, burning or not. Zero for one that
    /// is cold, or that was never a fire.
    pub fn degrees(&self, at: FirePos) -> f32 {
        self.burning
            .get(&at)
            .map(|fire| fire.degrees)
            .or_else(|| self.embers.get(&at).copied())
            .unwrap_or(0.0)
    }

    /// Is the rain getting at the fire at `at`? False for one that is not
    /// burning: the rain on a cold hearth is nobody's concern.
    pub fn is_wet(&self, at: FirePos) -> bool {
        self.burning.get(&at).is_some_and(|fire| fire.exposed)
    }

    /// Every cell that is alight.
    ///
    /// A list rather than an iterator, because the caller that wants it
    /// (`logic::smelting`) goes on to feed some of them, which needs
    /// this map mutably.
    pub fn burning_cells(&self) -> Vec<FirePos> {
        self.burning.keys().copied().collect()
    }

    /// Every hearth with any heat in it: the burning ones, and the ones
    /// that have gone out and not yet cooled.
    ///
    /// What `logic::smelting` walks now, where it used to walk only the
    /// burning. A batch asks for a temperature, not a flame, and a kiln
    /// that burnt through its charcoal with the last copper half poured is
    /// still hot enough to finish it -- which is the reason to let a fire
    /// run down rather than feed it to the end.
    pub fn heated_cells(&self) -> Vec<FirePos> {
        self.burning
            .keys()
            .chain(self.embers.keys())
            .copied()
            .collect()
    }

    /// Burns a fire down by hand. Tests only: the real thing happens in
    /// `step`, once a tick, for every fire at once.
    #[cfg(test)]
    pub fn burn_for_test(&mut self, at: FirePos, seconds: f32) {
        if let Some(fire) = self.burning.get_mut(&at) {
            let total = fire.fuel();
            fire.burn(seconds.min(total - 0.01));
        }
    }

    /// Lights a laid fire, or tops up a burning one.
    ///
    /// Returns false if there is nothing to light -- the caller has
    /// already checked the block, but two players can strike the same
    /// fire in the same tick and the second one must not spend their
    /// flint on a fire that is already going.
    pub fn light(&mut self, at: FirePos) -> bool {
        if self.burning.contains_key(&at) {
            return false;
        }
        // **From whatever heat the hearth still has**, and from cold if it
        // has none. Struck flint does not make a hot fire; it makes a
        // flame, and the fire comes up to heat from there -- but a kiln
        // relit a minute after it went out is still most of the way
        // there, and starting it from nothing would make letting a fire
        // go out between batches cost a full warm-up for no reason.
        let warm = self.embers.remove(&at).unwrap_or(0.0);
        self.burning.insert(at, Fire::laid(warm));
        self.dirty = true;
        true
    }

    /// Feeds a burning fire more of what it is already burning hottest.
    /// Returns false if it is already as stoked as it can get, so the
    /// caller does not spend the log.
    ///
    /// A fire that is *not* burning cannot be fed, which is deliberate:
    /// piling wood onto a cold hearth and having it silently vanish is
    /// the sort of thing a player does once and never trusts again.
    ///
    /// **"More of the same"** is what a caller that knows only seconds --
    /// a mod, through `feed_fire` -- can be taken to mean. Guessing a
    /// temperature for it instead would be a mod that feeds a copper fire
    /// and cools it.
    pub fn feed(&mut self, at: FirePos, seconds: f32) -> bool {
        let Some(fire) = self.burning.get(&at) else {
            return false;
        };
        let heat = fire.loads.first().map_or(LAID_FIRE_C, |&(_, heat)| heat);
        self.feed_with(at, seconds, heat)
    }

    /// Feeds a burning fire a fuel that burns this hot. Same refusals as
    /// [`Fires::feed`].
    pub fn feed_with(&mut self, at: FirePos, seconds: f32, degrees: f32) -> bool {
        let Some(fire) = self.burning.get_mut(&at) else {
            return false;
        };
        if !fire.add(seconds, degrees) {
            return false;
        }
        self.dirty = true;
        true
    }

    /// Would the fire at `at` take one more of a fuel this hot?
    ///
    /// **When what it has of that heat is nearly gone.** One rule, and it
    /// covers the three cases a fuel slot actually meets. Charcoal thrown
    /// on a wood fire is taken at once, because there is nothing that hot
    /// in it -- a player loading a kiln for copper is not made to watch the
    /// kindling burn first. A second lump waits for the first, because
    /// the first *is* that hot. And a log waits under a charcoal fire until
    /// the charcoal is nearly out, because the charcoal is hotter than a
    /// log: the wood is the fire's next hour, not a way to cool it now.
    ///
    /// `below` is how many seconds count as "nearly gone"; see
    /// `logic::smelting`, which owns why it is not zero.
    pub fn wants(&self, at: FirePos, degrees: f32, below: f32) -> bool {
        self.burning.get(&at).is_some_and(|fire| {
            fire.fuel() < MAX_FUEL_SECONDS && fire.seconds_at_least(degrees) < below
        })
    }

    /// Forgets a fire without touching the world -- what breaking the
    /// block calls.
    pub fn extinguish(&mut self, at: FirePos) {
        if self.burning.remove(&at).is_some() {
            self.dirty = true;
        }
        // The heat goes too. What calls this is a block being broken or a
        // mod putting the fire out on purpose, and in neither case is there
        // a hearth left to be warm.
        self.embers.remove(&at);
    }

    /// Is there a fire burning within `range` of this point?
    ///
    /// What `crafting::Station::Heat` actually asks. A straight scan of
    /// the map: a world has a handful of fires in it and this is asked
    /// once per craft, not once per tick per player -- and a spatial
    /// index over ten entries would cost more to keep than it saves.
    pub fn any_within(&self, of: (f32, f32, f32), range: f32) -> bool {
        self.within(of, range).next().is_some()
    }

    /// Every fire burning within `range` of a point.
    ///
    /// The same scan, handing back *which* fires rather than only
    /// whether there were any -- because there are two kinds now and
    /// what a recipe asks for is a kind. The map itself deliberately
    /// does not store which: it stores positions and fuel, the same as
    /// it did before the kiln, and the caller that cares looks the cell
    /// up in the world. That keeps the save format the fire map writes
    /// exactly as it was.
    pub fn within(
        &self,
        of: (f32, f32, f32),
        range: f32,
    ) -> impl Iterator<Item = FirePos> + '_ {
        let range_sq = range * range;
        self.burning.keys().copied().filter(move |&(x, y, z)| {
            // From the *centre* of the fire's cell, and to the player's
            // feet: a player standing on the block beside a fire is
            // beside it, and measuring corner to corner would make that
            // depend on which way the two happened to be rounded.
            let dx = x as f32 + 0.5 - of.0;
            let dy = y as f32 + 0.5 - of.1;
            let dz = z as f32 + 0.5 - of.2;
            dx * dx + dy * dy + dz * dz <= range_sq
        })
    }

    /// A cell changed. Queue it; the world is not ours to read here.
    pub fn on_block_changed(&mut self, gx: i32, gy: i32, gz: i32) {
        self.pending.push((gx, gy, gz));
    }

    pub fn pending(&self) -> usize {
        self.pending.len()
    }

    /// One tick. Burns fuel down, drowns what the rain reaches, and
    /// hands back the cells that stopped burning.
    pub fn step(&mut self, world: &dyn BlockWorld, dt: f32, budget: usize) -> Vec<BlockChange> {
        // First, reconcile what the world says with what we think.
        //
        // Two directions, and both are needed. A cell that is a lit fire
        // and is not in the map is one this server has never seen alight
        // -- a chunk loaded from a save written by an older build, or a
        // fire lit by a plugin -- and it gets a fresh load rather than
        // burning forever unnoticed. A cell in the map that is no longer
        // a lit fire has been broken or replaced, and holding its entry
        // would mean a fire you could warm your hands at through a wall
        // of stone.
        // `drain(..).take(budget)` looks like the same thing and is
        // not: the drain removes *everything* whatever the take
        // consumes, so a budget of sixty-four silently threw away five
        // thousand queued cells. Draining exactly the prefix is the
        // whole of the fix.
        let batch: Vec<FirePos> = self
            .pending
            .drain(..budget.min(self.pending.len()))
            .collect();
        for at in batch {
            // A cell nobody has loaded says nothing either way, and
            // guessing is how a fire in an evicted chunk gets put out by
            // a server that cannot see it.
            let Some(block) = world.block(at.0, at.1, at.2) else {
                continue;
            };
            let hearth = Kind::of(block);
            match (is_burning(block), self.burning.contains_key(&at)) {
                (true, false) => {
                    // **Adopted hot.** A cell that is already alight was
                    // burning when the world was last written, and it is
                    // burning now; bringing it back cold would stall every
                    // batch in every hearth of an old save for a minute,
                    // for a fire nobody put out.
                    let draught = hearth.map_or(0.0, Kind::draught);
                    let mut fire = Fire::laid(LAID_FIRE_C + draught);
                    fire.draught = draught;
                    self.embers.remove(&at);
                    self.burning.insert(at, fire);
                    self.dirty = true;
                }
                (false, true) => {
                    // Put out by something other than its fuel. If it is
                    // still a hearth it is still hot.
                    if let Some(fire) = self.burning.remove(&at) {
                        if hearth.is_some() {
                            self.embers.insert(at, fire.degrees);
                        }
                    }
                    self.dirty = true;
                }
                _ => {}
            }
            // A cooling hearth that has been broken or built over is not a
            // hearth any more, and its heat went with it.
            if hearth.is_none() {
                self.embers.remove(&at);
            }
        }

        // The embers cool, and before the early return below: a world
        // whose last fire has just gone out is exactly the world with
        // embers in it and nothing burning.
        self.embers.retain(|_, degrees| {
            *degrees -= FALL_PER_SECOND * dt;
            *degrees > 0.0
        });

        if self.burning.is_empty() {
            return Vec::new();
        }

        // Rain does not put a fire out instantly -- a fire under a
        // downpour dies over a minute or so, not the moment the first
        // drop lands, and a player who sees it guttering has time to
        // build something over it. Expressed as fuel burning faster,
        // which is what wet wood does.
        let weather_multiplier = if self.weather.is_wet() {
            1.0 + WET_BURN_MULTIPLIER * self.weather.intensity()
        } else {
            1.0
        };
        // ...and as a cooler fire, which is what it does first. See
        // `RAIN_COOLING_C`.
        let rain_cooling = RAIN_COOLING_C * self.weather.intensity();

        let mut went_out = Vec::new();
        self.burning.retain(|&at, fire| {
            // The draught from the block, when the block can be read. A
            // fire in an evicted chunk keeps the draught it had.
            if let Some(kind) = world.block(at.0, at.1, at.2).and_then(Kind::of) {
                fire.draught = kind.draught();
            }
            fire.exposed = weather_multiplier > 1.0 && open_to_the_sky(world, at);
            fire.burn(dt * if fire.exposed { weather_multiplier } else { 1.0 });
            if fire.loads.is_empty() {
                went_out.push((at, fire.degrees));
                return false;
            }
            fire.degrees = approach(fire.degrees, fire.target(rain_cooling), dt);
            true
        });

        if went_out.is_empty() {
            return Vec::new();
        }
        self.dirty = true;
        went_out
            .into_iter()
            .filter_map(|((x, y, z), degrees)| {
                // Only if it is still a fire. Between the fuel running
                // out and this line nothing can have happened -- it is
                // the same tick -- but the map is also reconciled from
                // `pending`, and writing an unlit campfire over whatever
                // a player has since built there would be worse than
                // leaving a stale entry.
                if !world.block(x, y, z).is_some_and(is_burning) {
                    return None;
                }
                // **Whatever it was.** A kiln that has burnt through
                // its charcoal comes back as a kiln; writing a campfire
                // over it -- which is what a hard-coded id did -- would
                // turn a player's furnace into a ring of sticks the
                // first time they left it unattended.
                let cold = world
                    .block(x, y, z)
                    .and_then(primitive_shared::types::burnt_out)?;
                world.set(x, y, z, cold);
                // Out of fuel, not out of heat: it cools from here.
                self.embers.insert((x, y, z), degrees);
                Some(BlockChange {
                    global_x: x,
                    global_y: y,
                    global_z: z,
                    block_id: cold,
                })
            })
            .collect()
    }

    fn save_path(dir: &Path) -> PathBuf {
        dir.join("fires.bin")
    }

    /// Writes them out, atomically, the way the chests are written.
    pub fn save(&mut self, dir: &Path) -> std::io::Result<usize> {
        std::fs::create_dir_all(dir)?;
        let mut fires: Vec<(FirePos, Vec<Load>, f32)> = self
            .burning
            .iter()
            .map(|(&at, fire)| (at, fire.loads.clone(), fire.degrees))
            .collect();
        // Stable bytes for the same world, so a save with nothing
        // changed produces an identical file.
        fires.sort_by_key(|(at, _, _)| *at);
        let count = fires.len();
        let bytes = bincode::serialize(&SaveFile {
            version: SAVE_FORMAT_VERSION,
            fires,
        })
        .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))?;
        let final_path = Self::save_path(dir);
        let tmp_path = final_path.with_extension("bin.tmp");
        std::fs::write(&tmp_path, &bytes)?;
        std::fs::rename(&tmp_path, &final_path)?;
        self.dirty = false;
        Ok(count)
    }

    /// Reads them back. A missing file means nothing is burning, which
    /// is what a world from before fires existed looks like.
    ///
    /// **A file this build cannot read is ignored rather than refused**,
    /// which is the opposite of what the chests do, and deliberately.
    /// Losing a chest is losing everything a player owns; losing the
    /// fuel timers is a fire that comes back with a full load, which the
    /// reconciliation in `step` would have done anyway.
    pub fn load(&mut self, dir: &Path) -> std::io::Result<usize> {
        let bytes = match std::fs::read(Self::save_path(dir)) {
            Ok(bytes) => bytes,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(0),
            Err(e) => return Err(e),
        };
        let Ok(SaveVersion { version }) = bincode::deserialize::<SaveVersion>(&bytes) else {
            return Ok(0);
        };
        // Off a disk an operator can edit: a fire with `NaN` fuel would
        // burn forever and never report going out.
        let usable = |seconds: f32| seconds.is_finite() && seconds > 0.0;
        let fires: Vec<(FirePos, Fire)> = match version {
            // **Before fires had a temperature.** All that was written was
            // how much fuel was left, and the only fuel a version-1 fire
            // can be said to have is what it was laid with. It was
            // burning, so it comes back hot -- the same answer `step`
            // gives a lit cell it has never seen.
            1 => {
                let Ok(save) = bincode::deserialize::<SaveFileV1>(&bytes) else {
                    return Ok(0);
                };
                save.fires
                    .into_iter()
                    .filter(|&(_, fuel)| usable(fuel))
                    .map(|(at, fuel)| {
                        let mut fire = Fire::laid(LAID_FIRE_C);
                        fire.loads[0].0 = fuel.min(MAX_FUEL_SECONDS);
                        (at, fire)
                    })
                    .collect()
            }
            SAVE_FORMAT_VERSION => {
                let Ok(save) = bincode::deserialize::<SaveFile>(&bytes) else {
                    return Ok(0);
                };
                save.fires
                    .into_iter()
                    .filter_map(|(at, loads, degrees)| {
                        let mut fire = Fire::laid(0.0);
                        fire.loads.clear();
                        // Nothing burns hotter than the hottest hearth on
                        // the hottest fuel, and a file that says otherwise
                        // is a file somebody edited; the fire climbs or
                        // falls to its real heat within the minute anyway.
                        fire.degrees = if degrees.is_finite() {
                            degrees.clamp(0.0, 2000.0)
                        } else {
                            0.0
                        };
                        // Through `add`, so the cap, the merging and the
                        // hottest-first order are the ones a live fire has
                        // rather than whatever the file claims.
                        for (seconds, heat) in loads {
                            if usable(seconds) && heat.is_finite() && heat >= 0.0 {
                                fire.add(seconds, heat);
                            }
                        }
                        (!fire.loads.is_empty()).then_some((at, fire))
                    })
                    .collect()
            }
            _ => return Ok(0),
        };
        self.burning.clear();
        self.embers.clear();
        self.burning.extend(fires);
        self.dirty = false;
        Ok(self.burning.len())
    }
}

/// How much faster wet wood burns away, at full storm intensity.
///
/// Six, so a fire caught in a downpour with a full initial load has
/// about forty seconds rather than four minutes. Long enough to notice
/// and do something about; short enough that "it is raining" is a fact
/// about your evening rather than a detail.
const WET_BURN_MULTIPLIER: f32 = 6.0;

/// Can the rain get at this cell?
///
/// Straight up, and only straight up: a roof one block above a fire
/// keeps it alight, and an overhang two cells to the side does not. Rain
/// that fell at an angle would be more honest and would also mean a
/// player could never be sure whether the shelter they built works,
/// which is the one thing a shelter has to be.
///
/// Bounded by the height of the world, so a fire under a mountain costs
/// a few dozen cell reads once a tick rather than a search.
fn open_to_the_sky(world: &dyn BlockWorld, at: FirePos) -> bool {
    let (x, y, z) = at;
    for above in (y + 1)..(primitive_shared::types::CHUNK_SIZE_Y as i32) {
        // A cell nobody has loaded is treated as open. It is the safe
        // way round: the alternative is a fire that never goes out
        // because the column over it happened to be evicted.
        let Some(block) = world.block(x, above, z) else {
            continue;
        };
        // Anything that fills its cell is a roof. A tuft of grass or a
        // stone lying on a ledge is not, which is right: you cannot
        // shelter under a daisy.
        if primitive_shared::types::is_collidable(block) {
            return false;
        }
    }
    true
}

#[cfg(test)]
mod tests {
    use primitive_shared::types::{BLOCK_COAL, BLOCK_LOG, BLOCK_PLANKS, BLOCK_STICK};
    use super::*;
    use crate::logic::falling::tests::TestWorld;
    use primitive_shared::types::{
        BLOCK_AIR, BLOCK_CAMPFIRE, BLOCK_CAMPFIRE_LIT, BLOCK_COBBLESTONE, BLOCK_KILN,
        BLOCK_KILN_LIT,
    };

    /// A kiln that has burnt out has to come back as a kiln.
    ///
    /// It came back as a campfire until `burnt_out` existed, which is
    /// the kind of mistake a second hearth invites and nothing else
    /// would have caught: a player's furnace quietly becoming a ring of
    /// sticks the first time they walked away from it.
    #[test]
    fn a_kiln_that_burns_out_is_still_a_kiln() {
        let world = TestWorld::default();
        world.put(AT.0, AT.1, AT.2, BLOCK_KILN_LIT);
        let mut fires = Fires::new();
        fires.on_block_changed(AT.0, AT.1, AT.2);
        fires.step(&world, 0.0, 64);
        let changes = fires.step(&world, INITIAL_FUEL_SECONDS + 1.0, 64);
        assert_eq!(changes.len(), 1);
        assert_eq!(changes[0].block_id, BLOCK_KILN);
        assert_eq!(world.get(AT.0, AT.1, AT.2), BLOCK_KILN);
    }

    const AT: FirePos = (4, 20, -7);

    fn lit_world() -> TestWorld {
        let world = TestWorld::default();
        world.put(AT.0, AT.1, AT.2, BLOCK_CAMPFIRE_LIT);
        world
    }

    #[test]
    fn a_fire_burns_down_and_goes_out() {
        let world = lit_world();
        let mut fires = Fires::new();
        assert!(fires.light(AT));
        assert_eq!(fires.len(), 1);

        // Nothing happens for most of its life.
        let changes = fires.step(&world, INITIAL_FUEL_SECONDS - 1.0, 16);
        assert!(changes.is_empty(), "it went out early");
        assert!(fires.fuel_left(AT).is_some_and(|f| f > 0.0));

        let changes = fires.step(&world, 2.0, 16);
        assert_eq!(changes.len(), 1, "it never went out");
        assert_eq!(changes[0].block_id, BLOCK_CAMPFIRE);
        assert_eq!(world.get(AT.0, AT.1, AT.2), BLOCK_CAMPFIRE);
        assert!(fires.is_empty());
    }

    #[test]
    fn lighting_a_fire_that_is_already_lit_costs_nothing() {
        // Two players striking the same fire in one tick: the second one
        // must not spend their flint on it.
        let mut fires = Fires::new();
        assert!(fires.light(AT));
        assert!(!fires.light(AT));
        assert_eq!(fires.fuel_left(AT), Some(INITIAL_FUEL_SECONDS));
    }

    #[test]
    fn feeding_it_buys_time_and_stops_at_the_cap() {
        let mut fires = Fires::new();
        fires.light(AT);
        assert!(fires.feed(AT, 300.0));
        assert_eq!(fires.fuel_left(AT), Some(INITIAL_FUEL_SECONDS + 300.0));

        // A stack of coal does not make an eternal fire.
        for _ in 0..40 {
            fires.feed(AT, 300.0);
        }
        assert_eq!(fires.fuel_left(AT), Some(MAX_FUEL_SECONDS));
        assert!(!fires.feed(AT, 300.0), "a full fire took the log anyway");
    }

    #[test]
    fn a_cold_hearth_cannot_be_fed() {
        // Piling wood onto an unlit fire and watching it vanish is the
        // sort of thing a player does once and never trusts again.
        let mut fires = Fires::new();
        assert!(!fires.feed(AT, 300.0));
        assert!(fires.is_empty());
    }

    #[test]
    fn rain_puts_out_what_it_can_reach_and_leaves_the_rest() {
        let sheltered: FirePos = (9, 20, 9);
        let world = lit_world();
        world.put(sheltered.0, sheltered.1, sheltered.2, BLOCK_CAMPFIRE_LIT);
        // A roof one block over the second fire.
        world.put(sheltered.0, sheltered.1 + 1, sheltered.2, BLOCK_COBBLESTONE);

        let mut fires = Fires::new();
        fires.light(AT);
        fires.light(sheltered);
        fires.set_weather(Weather::Storm);

        // Long enough for the exposed one to drown, nowhere near long
        // enough for the sheltered one to burn out.
        let mut changes = Vec::new();
        for _ in 0..100 {
            changes.extend(fires.step(&world, 1.0, 16));
        }
        assert_eq!(changes.len(), 1, "the rain took the wrong number of fires");
        assert_eq!(
            (changes[0].global_x, changes[0].global_y, changes[0].global_z),
            AT,
            "the sheltered fire is the one that died"
        );
        assert!(fires.fuel_left(sheltered).is_some(), "a roof did not keep the rain off");
    }

    #[test]
    fn a_fire_in_clear_weather_is_not_hurried() {
        let world = lit_world();
        let mut wet = Fires::new();
        let mut dry = Fires::new();
        wet.light(AT);
        dry.light(AT);
        wet.set_weather(Weather::Rain);
        wet.step(&world, 10.0, 16);
        dry.step(&world, 10.0, 16);
        assert!(
            wet.fuel_left(AT).unwrap() < dry.fuel_left(AT).unwrap(),
            "rain cost the fire nothing"
        );
    }

    #[test]
    fn a_fire_the_world_no_longer_has_is_forgotten() {
        // Somebody broke it, or a falling block landed on it. Holding
        // the entry would be a fire you could warm your hands at through
        // a wall.
        let world = lit_world();
        let mut fires = Fires::new();
        fires.light(AT);
        world.put(AT.0, AT.1, AT.2, BLOCK_AIR);
        fires.on_block_changed(AT.0, AT.1, AT.2);
        fires.step(&world, 0.05, 16);
        assert!(fires.is_empty());
    }

    #[test]
    fn a_fire_this_server_has_never_seen_is_adopted_rather_than_ignored() {
        // A chunk out of a save written before fires had timers, or one
        // lit by a plugin. Without this it burns forever.
        let world = lit_world();
        let mut fires = Fires::new();
        fires.on_block_changed(AT.0, AT.1, AT.2);
        fires.step(&world, 0.05, 16);
        assert_eq!(fires.len(), 1);
        assert!(fires.fuel_left(AT).is_some_and(|f| f > 0.0));
    }

    #[test]
    fn the_scan_says_which_fires_rather_than_only_whether() {
        // What `heat_within_reach` is built on. The map stores positions
        // and fuel and deliberately does not know what is burning at
        // each -- the caller looks that up in the world -- so this has
        // to hand back the cells rather than a yes or no, or a kiln and
        // a campfire are the same answer.
        let world = TestWorld::default();
        let hearth = (10, 20, 10);
        let forge = (12, 20, 10);
        world.put(hearth.0, hearth.1, hearth.2, BLOCK_CAMPFIRE_LIT);
        world.put(forge.0, forge.1, forge.2, BLOCK_KILN_LIT);
        let mut fires = Fires::new();
        fires.on_block_changed(hearth.0, hearth.1, hearth.2);
        fires.on_block_changed(forge.0, forge.1, forge.2);
        fires.step(&world, 0.05, 16);

        let standing = (11.5, 20.0, 10.5);
        let mut found: Vec<FirePos> = fires.within(standing, 3.0).collect();
        found.sort();
        assert_eq!(found, [hearth, forge], "one of the two fires went missing");
        // ...and the kinds are the world's to answer, which is the whole
        // arrangement.
        assert_eq!(world.get(forge.0, forge.1, forge.2), BLOCK_KILN_LIT);
        // Far enough off, and neither is in reach.
        assert_eq!(fires.within((40.0, 20.0, 10.5), 3.0).count(), 0);
    }

    #[test]
    fn the_reconciliation_respects_its_budget() {
        // The `CellMechanic` contract: a player hollowing out a hillside
        // queues thousands of cells, and a tick that drains all of them
        // is a tick that takes a second.
        let world = TestWorld::default();
        let mut fires = Fires::new();
        for i in 0..5000 {
            fires.on_block_changed(i, 30, 0);
        }
        fires.step(&world, 0.05, 64);
        assert_eq!(fires.pending(), 5000 - 64);
    }

    #[test]
    fn standing_next_to_a_fire_is_a_question_with_a_radius() {
        let mut fires = Fires::new();
        fires.light((10, 20, 10));
        // Feet on the block beside it.
        assert!(fires.any_within((11.5, 20.0, 10.5), 3.0));
        assert!(fires.any_within((10.5, 20.0, 10.5), 3.0));
        // Across the room.
        assert!(!fires.any_within((20.0, 20.0, 10.5), 3.0));
        // ...and up a ladder.
        assert!(!fires.any_within((10.5, 40.0, 10.5), 3.0));
    }

    #[test]
    fn only_what_actually_burns_is_fuel() {
        assert!(is_fuel(BLOCK_COAL));
        assert!(is_fuel(BLOCK_LOG));
        assert!(is_fuel(BLOCK_STICK));
        assert!(!is_fuel(BLOCK_COBBLESTONE));
        assert!(!is_fuel(primitive_shared::types::BLOCK_WATER));
        // ...and the ordering is the real one.
        assert!(fuel_value(BLOCK_COAL) > fuel_value(BLOCK_LOG));
        assert!(fuel_value(BLOCK_LOG) > fuel_value(BLOCK_PLANKS));
        assert!(fuel_value(BLOCK_PLANKS) > fuel_value(BLOCK_STICK));
    }

    #[test]
    fn fires_survive_a_restart_with_the_fuel_they_had() {
        let dir = std::env::temp_dir().join(format!("primitive-fires-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);

        let mut fires = Fires::new();
        fires.light(AT);
        fires.feed(AT, 100.0);
        let expected = fires.fuel_left(AT).expect("burning");
        assert!(fires.is_dirty());
        assert_eq!(fires.save(&dir).expect("save"), 1);
        assert!(!fires.is_dirty(), "a save left the map dirty");

        let mut restored = Fires::new();
        assert_eq!(restored.load(&dir).expect("load"), 1);
        assert_eq!(restored.fuel_left(AT), Some(expected));

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_world_with_no_fire_file_simply_has_no_fires() {
        let mut fires = Fires::new();
        let nowhere = std::env::temp_dir().join("primitive-fires-that-do-not-exist");
        let _ = std::fs::remove_dir_all(&nowhere);
        assert_eq!(fires.load(&nowhere).expect("a missing file is not an error"), 0);
        assert!(fires.is_empty());
    }

    /// Steps the fires a tick at a time, the way the server does, and
    /// collects what went out.
    fn run(fires: &mut Fires, world: &TestWorld, seconds: f32) -> Vec<BlockChange> {
        let mut changes = Vec::new();
        let mut left = seconds;
        while left > 1e-4 {
            let dt = left.min(0.05);
            changes.extend(fires.step(world, dt, 64));
            left -= dt;
        }
        changes
    }

    const CHARCOAL_C: f32 = 1350.0;

    #[test]
    fn a_fire_climbs_to_the_heat_of_what_it_burns_and_no_higher() {
        let world = lit_world();
        let mut fires = Fires::new();
        fires.light(AT);
        assert_eq!(fires.degrees(AT), 0.0, "a struck fire started hot");
        run(&mut fires, &world, 1.0);
        let early = fires.degrees(AT);
        assert!(
            early > 0.0 && early < LAID_FIRE_C,
            "it did not climb, or climbed all at once: {early}"
        );
        run(&mut fires, &world, 60.0);
        assert_eq!(fires.degrees(AT), LAID_FIRE_C, "it settled away from its own heat");
    }

    #[test]
    fn a_hotter_fuel_goes_on_at_once_and_a_cooler_one_waits_underneath() {
        let world = lit_world();
        let mut fires = Fires::new();
        fires.light(AT);
        // Nothing that hot is in it, so charcoal is taken now -- not after
        // four minutes of kindling.
        assert!(fires.wants(AT, CHARCOAL_C, 1.0), "charcoal waited behind the kindling");
        assert!(fires.feed_with(AT, 300.0, CHARCOAL_C));
        assert!(!fires.wants(AT, CHARCOAL_C, 1.0), "a second lump went on with the first alight");
        // ...and a fuel cooler than what is burning waits for it.
        assert!(!fires.wants(AT, 600.0, 1.0), "peat went on under a burning fire");

        run(&mut fires, &world, 120.0);
        assert_eq!(fires.degrees(AT), CHARCOAL_C);
        // The charcoal burns first, and the kindling is still there under
        // it -- one lump of charcoal buys three hundred seconds of heat,
        // not the whole fire's worth.
        run(&mut fires, &world, 190.0);
        let left = fires.fuel_left(AT).expect("still burning");
        assert!(
            (225.0..235.0).contains(&left),
            "the kindling burnt with the charcoal: {left} left"
        );
        let now = fires.degrees(AT);
        assert!(
            now < CHARCOAL_C && now > LAID_FIRE_C,
            "it did not start falling back to the kindling's heat: {now}"
        );
    }

    #[test]
    fn a_fire_that_runs_out_stays_hot_for_a_while_and_then_goes_cold() {
        let world = lit_world();
        let mut fires = Fires::new();
        fires.light(AT);
        run(&mut fires, &world, INITIAL_FUEL_SECONDS - 0.5);
        let changes = run(&mut fires, &world, 1.0);
        assert_eq!(changes.len(), 1, "it never went out");

        let warm = fires.degrees(AT);
        assert!(warm > LAID_FIRE_C * 0.9, "it went cold the moment it went out: {warm}");
        assert!(fires.heated_cells().contains(&AT), "the batch has no way to find the heat");
        assert!(fires.fuel_left(AT).is_none(), "embers counted as a flame");
        assert!(
            !fires.any_within((AT.0 as f32 + 0.5, AT.1 as f32, AT.2 as f32 + 0.5), 2.0),
            "embers warm a player like a flame"
        );

        run(&mut fires, &world, LAID_FIRE_C / FALL_PER_SECOND + 1.0);
        assert_eq!(fires.degrees(AT), 0.0);
        assert!(fires.heated_cells().is_empty(), "a cold hearth is still on the list");
    }

    #[test]
    fn relighting_a_warm_hearth_starts_from_its_embers() {
        let world = lit_world();
        let mut fires = Fires::new();
        fires.light(AT);
        run(&mut fires, &world, INITIAL_FUEL_SECONDS + 1.0);
        assert!(fires.fuel_left(AT).is_none());
        let warm = fires.degrees(AT);
        assert!(warm > 0.0);

        world.put(AT.0, AT.1, AT.2, BLOCK_CAMPFIRE_LIT);
        assert!(fires.light(AT));
        assert_eq!(fires.degrees(AT), warm, "a relit hearth started from cold");
    }

    #[test]
    fn rain_cools_an_open_fire_before_it_drowns_it_and_a_roof_keeps_the_heat() {
        let sheltered: FirePos = (9, 20, 9);
        let world = lit_world();
        world.put(sheltered.0, sheltered.1, sheltered.2, BLOCK_CAMPFIRE_LIT);
        world.put(sheltered.0, sheltered.1 + 1, sheltered.2, BLOCK_COBBLESTONE);
        let mut fires = Fires::new();
        for at in [AT, sheltered] {
            fires.light(at);
            fires.feed_with(at, 900.0, CHARCOAL_C);
        }
        run(&mut fires, &world, 60.0);
        assert_eq!(fires.degrees(AT), CHARCOAL_C);

        fires.set_weather(Weather::Storm);
        run(&mut fires, &world, 25.0);
        assert!(fires.fuel_left(AT).is_some(), "the storm put it out before it cooled it");
        assert!(fires.is_wet(AT), "the screen would not say why");
        assert_eq!(
            fires.degrees(AT),
            CHARCOAL_C - RAIN_COOLING_C * Weather::Storm.intensity(),
            "the rain did not cool it by what it should"
        );
        assert!(!fires.is_wet(sheltered));
        assert_eq!(fires.degrees(sheltered), CHARCOAL_C, "a roof did not keep the heat in");
    }

    #[test]
    fn a_kiln_burns_the_same_fuel_hotter_than_a_campfire() {
        let world = lit_world();
        let kiln: FirePos = (12, 20, 10);
        world.put(kiln.0, kiln.1, kiln.2, BLOCK_KILN_LIT);
        let mut fires = Fires::new();
        fires.light(AT);
        fires.light(kiln);
        run(&mut fires, &world, 60.0);
        assert_eq!(fires.degrees(AT), LAID_FIRE_C);
        assert_eq!(fires.degrees(kiln), LAID_FIRE_C + Kind::Kiln.draught());
    }

    #[test]
    fn a_fire_the_world_already_had_alight_is_adopted_hot() {
        // An old save's lit kiln: it was burning, so its batch should not
        // stall for a minute while a fire nobody put out warms up again.
        let world = TestWorld::default();
        world.put(AT.0, AT.1, AT.2, BLOCK_KILN_LIT);
        let mut fires = Fires::new();
        fires.on_block_changed(AT.0, AT.1, AT.2);
        fires.step(&world, 0.05, 16);
        assert_eq!(fires.degrees(AT), LAID_FIRE_C + Kind::Kiln.draught());
    }

    #[test]
    fn fires_survive_a_restart_with_what_they_burn_and_how_hot() {
        let dir = std::env::temp_dir().join(format!("primitive-fires-heat-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);

        let world = lit_world();
        let mut fires = Fires::new();
        fires.light(AT);
        fires.feed_with(AT, 300.0, CHARCOAL_C);
        run(&mut fires, &world, 30.0);
        let (fuel, heat) = (fires.fuel_left(AT).unwrap(), fires.degrees(AT));
        fires.save(&dir).expect("save");

        let mut restored = Fires::new();
        assert_eq!(restored.load(&dir).expect("load"), 1);
        assert_eq!(restored.fuel_left(AT), Some(fuel));
        assert_eq!(restored.degrees(AT), heat, "the fire came back at another heat");
        // ...and it still knows the charcoal from the kindling: a second
        // lump is not wanted, which it would be if the load had been lost.
        assert!(!restored.wants(AT, CHARCOAL_C, 1.0), "the charcoal came back as kindling");

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_fire_saved_before_it_had_a_temperature_comes_back_burning_and_hot() {
        let dir = std::env::temp_dir().join(format!("primitive-fires-v1-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        // Exactly what version 1 wrote: the version, then position and fuel.
        let old = bincode::serialize(&(1u32, vec![(AT, 100.0f32)])).unwrap();
        std::fs::write(dir.join("fires.bin"), old).unwrap();

        let mut fires = Fires::new();
        assert_eq!(fires.load(&dir).expect("load"), 1, "an old fire file was thrown away");
        assert_eq!(fires.fuel_left(AT), Some(100.0));
        assert_eq!(fires.degrees(AT), LAID_FIRE_C);

        let _ = std::fs::remove_dir_all(&dir);
    }
}
