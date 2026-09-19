//! What a fire *is*, now that it has an inside.
//!
//! ## Why a screen and not a recipe you run standing near it
//!
//! Smelting used to be an entry in the player's own crafting list with
//! "you must be beside a lit kiln" attached to it. Everything about that
//! was a compromise and the compromise showed: the fire was a *condition
//! on a menu row* rather than a thing that does something. It had no
//! inside, so it could not be loaded and left; it could not run while
//! you walked away; feeding it was a separate right-click gesture with
//! its own rules; and the one question a furnace exists to answer --
//! "what is in there and how far along is it" -- had nowhere to be
//! asked.
//!
//! A hearth now has slots, and what it does with them is its own
//! business rather than the player's. You put ore and charcoal in it,
//! light it, and it works while you do something else. That is what
//! every furnace in every game of this shape is, and the reason it is
//! worth the screen is not the screen: it is that the fire becomes a
//! *place in the world with state*, which is the difference between a
//! recipe and a forge.
//!
//! ## The slots
//!
//! Four for what goes in, one for fuel, two for what comes out. Four
//! rather than one because the recipes here were written before this
//! screen existed and several of them take three or four things --
//! ore, charcoal, a crucible and a mould -- and rewriting the chain to
//! fit a one-slot furnace would be changing the game to fit its
//! interface.
//!
//! ## Why the layout lives in the shared crate
//!
//! Both sides count on it. The server refuses a move that would put
//! charcoal in the output or an ingot in the fuel slot; the client draws
//! the slots in their places and must not offer a gesture the server
//! will silently drop. One table, read twice.

use crate::crafting::{Recipe, Station, RECIPES};
use crate::inventory::Inventory;
use crate::types::{
    block_kind, is_burning, BlockId, BLOCK_BLOOMERY, BLOCK_BLOOMERY_LIT, BLOCK_CAMPFIRE,
    BLOCK_CAMPFIRE_LIT, BLOCK_FIREPIT, BLOCK_FIREPIT_LIT, BLOCK_KILN, BLOCK_KILN_LIT,
};

/// The slots that take ingredients.
pub const INPUT_SLOTS: std::ops::Range<usize> = 0..4;
/// The one that takes fuel.
pub const FUEL_SLOT: usize = 4;
/// The slots the results land in.
pub const OUTPUT_SLOTS: std::ops::Range<usize> = 5..7;
/// Where the fire puts its ash.
///
/// **A slot of its own, and not the output tray.** TerraFirmaCraft keeps
/// its firepit's ash in a hidden counter and throws it out when the pit
/// is broken; the first version of this put it in the output tray
/// instead, where it can be seen. Both were worse. A counter nobody can
/// see is ash nobody knows they have. Ash in the output tray *jams* the
/// hearth: two output slots, one of them holding ash, and the batch of
/// bread after the batch of meat has nowhere to land and the fire sits
/// there burning for nothing. A slot that only ash goes in costs one
/// square on the screen and cannot stop anything.
///
/// It was slot seven because slot seven was free: saves from before it
/// existed have nothing there, because the server refused every move into
/// it, so an old hearth opens with an empty ash slot and nothing moves.
pub const ASH_SLOT: usize = 7;
/// How many slots of the underlying inventory a hearth uses at all.
///
/// The rest of the forty are refused by the server and never drawn. A
/// hearth shares the container store with the chests -- same map, same
/// file, same code for opening, moving and spilling on break -- and the
/// price of that is a type with more slots in it than this needs. The
/// price of the alternative is a second container system.
pub const USED_SLOTS: usize = 8;

/// Which fire this is.
///
/// Named rather than derived from the block id at every call site,
/// because three of the six ids are the lit forms and every question
/// here is about the *kind* of hearth rather than about whether it
/// happens to be alight.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum Kind {
    /// A ring of stones: cooking, and wood burnt down to charcoal.
    Campfire,
    /// A daubed clay kiln: everything that is fired or melted.
    Kiln,
    /// A charcoal shaft: iron, and only iron.
    Bloomery,
}

impl Kind {
    /// What kind of hearth this block is, if it is one.
    pub fn of(block: BlockId) -> Option<Kind> {
        match block_kind(block) {
            // A firepit is a campfire with no stones in it: the same fire,
            // the same slots, the same heat. See `types::BLOCK_FIREPIT`.
            BLOCK_CAMPFIRE | BLOCK_CAMPFIRE_LIT | BLOCK_FIREPIT | BLOCK_FIREPIT_LIT => Some(Kind::Campfire),
            BLOCK_KILN | BLOCK_KILN_LIT => Some(Kind::Kiln),
            BLOCK_BLOOMERY | BLOCK_BLOOMERY_LIT => Some(Kind::Bloomery),
            _ => None,
        }
    }

    /// Can this hearth run that recipe?
    ///
    /// The same ladder `crafting::Heat` describes and for the same
    /// reasons: every fire is a fire, so anything a campfire can do a
    /// kiln can do as well; nothing but a kiln is hot enough to melt,
    /// and nothing but a bloomery wins iron. `Hands` is never run here
    /// -- tying fibre round a stick in a furnace is not a thing the
    /// furnace should offer to do for you.
    pub fn runs(self, station: Station) -> bool {
        match station {
            Station::Hands => false,
            Station::Heat => true,
            Station::Forge => self == Kind::Kiln,
            Station::Bloomery => self == Kind::Bloomery,
            // A workshop is not a fire: its rows are made from the pack
            // beside it, and a door loaded into a kiln would be firewood.
            Station::Bench | Station::Mason | Station::Wheel | Station::Leather => false,
        }
    }

    /// How long one batch takes here, in seconds.
    ///
    /// Longer the hotter the work: a rabbit over a campfire is minutes
    /// of an evening, and a bloom is the evening. The numbers are short
    /// enough that a player watches the first one and long enough that
    /// they stop watching the tenth, which is the point at which a
    /// furnace becomes a thing you load and leave.
    pub fn cook_seconds(self) -> f32 {
        match self {
            Kind::Campfire => 8.0,
            Kind::Kiln => 14.0,
            Kind::Bloomery => 20.0,
        }
    }

    /// What to call it on the screen.
    pub fn name(self) -> &'static str {
        match self {
            Kind::Campfire => "campfire",
            Kind::Kiln => "kiln",
            Kind::Bloomery => "bloomery",
        }
    }
}

/// May this block go in this slot?
///
/// **The output slots are the interesting ones.** Nothing may be put
/// into them -- they are where the hearth puts things, and a player who
/// can fill them can jam the furnace with something it will never
/// consume. Everything else is a matter of not wasting the player's
/// time: fuel that is not fuel would sit in the fuel slot doing nothing
/// and reading as a fire that refuses to burn.
pub fn accepts(slot: usize, block: BlockId) -> bool {
    // The ash slot is an output like the other two, for the same reason:
    // a player who could put a stone in it would be a player whose fire
    // silently stops making ash.
    if OUTPUT_SLOTS.contains(&slot) || slot == ASH_SLOT {
        return false;
    }
    if slot == FUEL_SLOT {
        return is_fuel(block);
    }
    INPUT_SLOTS.contains(&slot)
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
///
/// **Shared**, because both sides need it now: the server burns it and
/// the client has to know what the fuel slot will accept before it lets
/// a player drag something into it.
pub fn fuel_seconds(block: BlockId) -> Option<f32> {
    // A green log is gone sooner, for the heat's reason (`GREEN_HEAT`).
    let green = if crate::wood::is_green(block) { GREEN_BURN } else { 1.0 };
    fuel_seconds_seasoned(block).map(|seconds| seconds * green)
}

/// How much of a seasoned log's burn a green one gives: seven tenths.
///
/// **Shorter, not longer**, though a wet log smoulders for ever on a grate:
/// what a hearth is fed for is heat, and the part of a green log's life
/// spent boiling its sap is time the fire is not doing anything a player
/// asked of it. Counted as burn, it would make green wood the *long* fuel
/// and a reason to prefer it.
pub const GREEN_BURN: f32 = 0.7;

/// How much of a seasoned log's heat a green one reaches: a little over
/// half.
///
/// **Chosen by the line it must not cross**: no green wood, in any hearth,
/// reaches [`FIRING_C`] (`green_wood_cooks_supper_and_fires_no_pot`). The
/// hottest wood is saxaul at 800 and the best draught the bloomery's 250;
/// at 0.55 that is 690, ten degrees short, and a green oak in a kiln is
/// 550 -- a fire that cooks, boils and dries a rack and never fires a pot
/// or pours a metal. Seasoned, the same oak is 878 and fires. That gap is
/// the decision: the wood cut today is supper's, and the kiln's was cut a
/// week ago.
pub const GREEN_HEAT: f32 = 0.55;

/// [`fuel_seconds`] for the fuel dry, which is what every fuel but a green
/// log already is.
fn fuel_seconds_seasoned(block: BlockId) -> Option<f32> {
    match block_kind(block) {
        crate::types::BLOCK_COAL => Some(300.0),
        crate::types::BLOCK_LOG
        | crate::types::BLOCK_BIRCH_LOG
        | crate::types::BLOCK_FIR_LOG
        | crate::types::BLOCK_SAXAUL_LOG
        | crate::types::BLOCK_PINE_LOG
        | crate::types::BLOCK_WILLOW_LOG => Some(180.0),
        // Longer than a log and shorter than coal, which is where peat
        // sits in every hearth that ever burned it -- and the reason a
        // treeless bog is worth a shovel. Only the dried brick: peat as
        // it comes out of the ground is mostly water.
        crate::types::BLOCK_DRIED_PEAT => Some(240.0),
        crate::types::BLOCK_PLANKS
        | crate::types::BLOCK_BIRCH_PLANKS
        | crate::types::BLOCK_FIR_PLANKS
        | crate::types::BLOCK_SAXAUL_PLANKS
        | crate::types::BLOCK_PINE_PLANKS
        | crate::types::BLOCK_WILLOW_PLANKS => Some(60.0),
        crate::types::BLOCK_STICK => Some(25.0),
        // Cane is a stick with nothing inside it: the same heat, and gone in
        // half the time.
        crate::types::BLOCK_CANE => Some(12.0),
        // **Tallow: the best fuel an animal gives.** Rendered fat burns
        // hot and long -- longer than the stick it would be smeared on
        // -- which is why the same lump makes two torches (see the
        // "fat torch" row in `crafting`). What it costs is a bear, or
        // the belly of a boar in autumn.
        crate::types::BLOCK_FAT => Some(150.0),
        // **A handful of feathers is kindling and nothing else.** They
        // catch instantly and are gone, which is exactly what down does
        // in a fire -- and it is the honest use for the thing a bird
        // gives most of. Twelve seconds will not cook a haunch; it will
        // get a fire going when everything else you have is damp.
        crate::types::BLOCK_FEATHER => Some(12.0),
        // **Leaves are kindling too, and worse than feathers are.** A
        // handful flares and is ash in eight seconds: enough to catch a
        // fire from a spark, never enough to keep one. What they are
        // really worth is the field (`crafting`, "mulch"), and a player
        // burning a stack of them is choosing the evening over the harvest.
        crate::types::BLOCK_LEAF_HANDFUL => Some(8.0),
        // **Amadou smoulders**, and this is the number that says so: a
        // shelf fungus is worth more than the stick beside it and less
        // than the plank, because what it does in a fire is glow for a
        // long time without ever flaring. Nobody smelts with it. What
        // it is really for is the torch wad (see `crafting`) -- this
        // row exists so that a player who has a pocket of them and no
        // wood can still keep a fire alive through the evening, which
        // is the situation the bog puts them in.
        crate::types::BLOCK_BRACKET_FUNGUS => Some(45.0),
        _ => None,
    }
}

/// Can this be fed to a fire at all?
#[inline]
pub fn is_fuel(block: BlockId) -> bool {
    fuel_seconds(block).is_some()
}

// ---- how hot ----
//
// **A fire has a temperature, and what a hearth can do is decided by it.**
// This is TerraFirmaCraft's firepit, brought across: every fuel burns at
// a temperature as well as for a time, the fire climbs towards the
// temperature of what it is burning and falls when it runs out, and
// every batch -- a steak, an ingot, a pot -- names the heat it wants. The
// climbing and falling are the server's (`logic::fire`); what is here is
// the part both sides have to agree on, because the client draws the
// line on the gauge that the server holds the batch to.
//
// **Why port it at all**, when "any lit fire cooks, a kiln melts" already
// worked: because it worked by having one correct answer. Every hearth
// took every fuel the same way, so the fuel slot was a question of *how
// long* and never of *what for*, and charcoal was a thing a recipe asked
// for as an ingredient rather than a thing a fire needed to burn. With a
// temperature the slot becomes a decision. Wood cooks supper and melts
// nothing; charcoal pours copper and chars whatever supper was left on
// it; peat in a bog kiln fires a pot and pours nothing. Three fuels,
// three uses, and a player loading the slot is choosing between them.
//
// The numbers are TerraFirmaCraft's wherever it has one -- the fuels'
// burn temperatures, food at two hundred, the metals' melting points and
// its rule that metal is forged at sixty per cent of its melting point
// and welded at eighty -- and they are named where they are not.

/// How hot one of something burns, in degrees Celsius, or `None` if it
/// does not burn.
///
/// **Paired with [`fuel_seconds`] and never out of step with it** (see
/// `everything_that_burns_says_how_hot_as_well_as_how_long`): a fuel
/// with a burn time and no temperature would be a fuel the fire takes
/// and cannot heat on.
///
/// What decides the order is one line: **nothing but charcoal reaches
/// the melting point of copper, in any hearth.** TerraFirmaCraft's own
/// figures nearly keep that and not quite. Its stick *bundle* burns at
/// 900, and a kiln's draught on top of nine hundred would pour copper on
/// kindling -- which is the whole charcoal chain made pointless by the
/// cheapest thing in the world. A single stick here is not a bundle, and
/// burns at 800: still the hottest wood, which is what kindling is for,
/// and a hundred degrees short of the line with the hottest shaft there
/// is (`molten_metal_wants_charcoal_whatever_the_hearth`).
pub fn fuel_degrees(block: BlockId) -> Option<f32> {
    let green = if crate::wood::is_green(block) { GREEN_HEAT } else { 1.0 };
    fuel_degrees_seasoned(block).map(|degrees| degrees * green)
}

/// [`fuel_degrees`] for the fuel dry.
fn fuel_degrees_seasoned(block: BlockId) -> Option<f32> {
    match block_kind(block) {
        // TerraFirmaCraft's charcoal. Coal here *is* charcoal (see the
        // "charcoal" row in `crafting`), so it takes charcoal's number
        // rather than mined coal's 1415.
        crate::types::BLOCK_COAL => Some(1350.0),
        // Oak and birch as TerraFirmaCraft has them. Planks are the same
        // wood cut small, which burns shorter (`fuel_seconds`) and no
        // hotter: a plank fire is not a hotter fire than a log fire, and
        // a table that said so would make carpentry a way to melt metal.
        crate::types::BLOCK_LOG | crate::types::BLOCK_PLANKS => Some(728.0),
        crate::types::BLOCK_BIRCH_LOG | crate::types::BLOCK_BIRCH_PLANKS => Some(652.0),
        // **Fir is a resinous softwood and burns as birch does**: fast and
        // no hotter. **Saxaul is the other end** -- a wood heavier than
        // water, the charcoal-burner's timber of the steppe, and the hottest
        // wood fire there is. Not as hot as charcoal: a desert fire that
        // melted copper without a charcoal pit would skip the step the pit
        // is for.
        crate::types::BLOCK_FIR_LOG | crate::types::BLOCK_FIR_PLANKS => Some(652.0),
        // Pine is resinous and burns as hot as fir; willow is light and soft
        // and burns cool and quick, which is why nobody smelts over it.
        crate::types::BLOCK_PINE_LOG | crate::types::BLOCK_PINE_PLANKS => Some(652.0),
        crate::types::BLOCK_WILLOW_LOG | crate::types::BLOCK_WILLOW_PLANKS => Some(560.0),
        crate::types::BLOCK_SAXAUL_LOG | crate::types::BLOCK_SAXAUL_PLANKS => Some(800.0),
        // TerraFirmaCraft's peat exactly: the coolest real fuel, and the
        // longest. It is what makes a bog kiln a kiln -- enough for a pot,
        // never enough for an ingot.
        crate::types::BLOCK_DRIED_PEAT => Some(600.0),
        // Kindling flares. See the note above for why 800 and not 900.
        crate::types::BLOCK_STICK => Some(800.0),
        crate::types::BLOCK_CANE => Some(800.0),
        // Green leaves smoke more than they burn: a cool, short flame that
        // lights kindling and cooks nothing.
        crate::types::BLOCK_LEAF_HANDFUL => Some(450.0),
        // Rendered fat burns hot, as hot as kindling and for six times as
        // long -- and no hotter, for the same reason sticks are not.
        crate::types::BLOCK_FAT => Some(800.0),
        // Amadou smoulders and down flashes; both are enough to cook over
        // and neither is enough to char wood into charcoal. The bog's
        // evening meal, and nothing else.
        crate::types::BLOCK_BRACKET_FUNGUS => Some(300.0),
        crate::types::BLOCK_FEATHER => Some(250.0),
        _ => None,
    }
}

impl Kind {
    /// How much hotter this hearth burns the same fuel than an open fire.
    ///
    /// **The kiln and the shaft are TerraFirmaCraft's bellows, built in.**
    /// Its firepit reaches the temperature of its fuel and no more; its
    /// forge is pushed past that by air. Here a kiln's clay walls hold the
    /// heat in and a bloomery's shaft draws like a chimney, which is what
    /// they were built to do, so the extra is a property of the hearth
    /// rather than a block to pump. A bellows to stand at and click would
    /// be a chore wearing a mechanic's clothes.
    ///
    /// Small, and bounded by one test: with the draught added, the
    /// hottest fuel short of charcoal still falls short of copper (see
    /// [`fuel_degrees`]). A kiln on oak is 878 -- a pot, not an ingot.
    pub fn draught(self) -> f32 {
        match self {
            Kind::Campfire => 0.0,
            Kind::Kiln => 150.0,
            Kind::Bloomery => 250.0,
        }
    }

    /// How hot this hearth gets on this fuel, dry and left alone.
    pub fn reaches(self, fuel: BlockId) -> Option<f32> {
        fuel_degrees(fuel).map(|degrees| degrees + self.draught())
    }
}

/// Where food is done. TerraFirmaCraft cooks every meat, every dough and
/// every root at two hundred.
pub const COOKING_C: f32 = 200.0;
/// Where wood turns to charcoal in its own smoke. Not a TerraFirmaCraft
/// number -- its charcoal pile has no temperature -- but the real one, and
/// placed where it does work: every fire of wood clears it, and the two
/// fuels that only smoulder do not.
pub const CHARCOAL_C: f32 = 400.0;
/// Where fired clay is fired: a pot, a mould, a jug, a brick.
///
/// **Not TerraFirmaCraft's**, which fires pottery in a pit kiln that
/// goes to 1600 whatever it is burning. Taking that here would mean no pot
/// without charcoal, and charcoal comes before pots only by a detour --
/// the pot is what melts the first copper, and the charcoal is what the
/// copper needs, not the clay. Seven hundred is low earthenware, which is
/// what the first pots in the world actually were, and it is what a kiln
/// in a bog reaches on peat.
pub const FIRING_C: f32 = 700.0;
/// **Limestone gives up its lime at about nine hundred degrees**: past a
/// pot's firing, short of copper's melt. A kiln on charcoal, not a campfire.
pub const LIME_C: f32 = 900.0;
/// The metals, as TerraFirmaCraft melts them.
pub const TIN_MELTS_C: f32 = 230.0;
pub const BRONZE_MELTS_C: f32 = 950.0;
pub const COPPER_MELTS_C: f32 = 1080.0;
pub const IRON_MELTS_C: f32 = 1535.0;
/// Where an iron bloom forms: ore reduced in a shaft of charcoal below
/// its melting point. The real figure; TerraFirmaCraft's bloomery asks for
/// its fuel rather than a number.
pub const BLOOM_C: f32 = 1200.0;
/// Where tin comes out of its ore: **eleven hundred, not the two hundred and
/// thirty tin melts at.**
///
/// The row used to ask for the melting point, and a melting point is the
/// wrong number for a smelt: cassiterite is an oxide, charcoal has to take
/// the oxygen off it, and the rock it came in has to run off as slag, and
/// neither happens in a wood fire. At 230 a pinch of kindling smelted tin
/// ore, which made the one rare metal in the game the one that needed no
/// charcoal at all. Eleven hundred is a charcoal fire with a draught, which
/// is what every tin smelt in history was.
pub const TIN_SMELTS_C: f32 = 1100.0;
/// Where a bar of iron takes carbon into itself, packed in charcoal in a
/// closed fire: cementation. Nine hundred to a thousand in life; the middle of
/// it here, which a kiln reaches on charcoal and, just, on kindling.
pub const CEMENTATION_C: f32 = 950.0;
/// TerraFirmaCraft works metal at sixty per cent of its melting point...
const FORGING_SHARE: f32 = 0.6;
/// ...and welds it at eighty, which is what beating the slag out of a
/// bloom is.
const WELDING_SHARE: f32 = 0.8;
/// Where water boils.
pub const BOILING_C: f32 = 100.0;
/// How long a jug takes to boil through, in seconds.
///
/// Longer than a steak at a campfire, because a jug of water is a lot of
/// cold to heat, and long enough that boiling is a cost in fuel rather
/// than a click -- the whole point of the rule it is part of.
pub const BOIL_SECONDS: f32 = 20.0;
/// Where cooked food left on the fire starts to turn to ash.
///
/// **TerraFirmaCraft's orange**, the heat at which its metal first looks
/// like fire rather than like something hot. Its 1.20 firepit does not
/// burn food at all -- a cooked steak has no further recipe and simply
/// sits there -- and its older versions burned anything past "very hot".
/// Neither creates a decision: never burning makes the tray a larder, and
/// burning at four hundred and eighty would char dinner on every wood fire
/// there is, which is a chore. At orange, no fire of wood ever burns food
/// and every fire of charcoal does, and *that* is a choice a player makes
/// when they load the fuel slot.
pub const CHAR_C: f32 = 930.0;
/// How long one piece of cooked food lasts in the tray of a fire that
/// hot, in seconds.
///
/// Enough to see the warning and come for it; not enough to walk away
/// from a charcoal fire with supper in it and find supper there.
pub const CHAR_SECONDS: f32 = 30.0;

/// How hot the fire has to be for this recipe to run, or `None` for a
/// recipe that says nothing -- which a test forbids for anything a hearth
/// runs (`every_batch_a_hearth_runs_names_a_heat_some_fire_can_reach`).
///
/// **By what comes out**, because that is what the heat is for: a copper
/// ingot is molten copper whether it came from ore or from nuggets, and a
/// bronze axe is forged bronze whatever it was hafted with. Reading the
/// name instead would work until the next recipe was added under a name
/// nobody remembered to list here, and it would run at no heat at all.
pub fn needs_degrees(recipe: &Recipe) -> Option<f32> {
    use crate::types::*;
    Some(match block_kind(recipe.output.0) {
        BLOCK_COOKED_MEAT | BLOCK_ROASTED_RIBS | BLOCK_BREAD | BLOCK_ROASTED_ROOT | BLOCK_COOKED_FISH
        | BLOCK_ROAST_HUMAN_FLESH
        | BLOCK_MILLET_PORRIDGE
        | BLOCK_STEW
        // A jug of the sea boiled dry: water's boil, which a cooking fire
        // is past.
        | BLOCK_SALT => COOKING_C,
        BLOCK_COAL => CHARCOAL_C,
        // ...and roof tiles, which are fired as a brick is.
        BLOCK_BRICK | BLOCK_VESSEL | BLOCK_MOULD | BLOCK_JUG | BLOCK_BOWL | BLOCK_TILE_SLAB => FIRING_C,
        BLOCK_QUICKLIME => LIME_C,
        // Reduced out of its ore, which is hotter work than the metal's own
        // melting point: see `TIN_SMELTS_C`.
        BLOCK_TIN_INGOT => TIN_SMELTS_C,
        // Poured: the metal has to be liquid. Bronze too, because what
        // is poured into the alloy is copper.
        BLOCK_COPPER_INGOT
        | BLOCK_BRONZE_INGOT
        | BLOCK_COPPER_HOE_HEAD
        | BLOCK_COPPER_SHOVEL_HEAD
        | BLOCK_COPPER_AXE_HEAD
        | BLOCK_COPPER_PICK_HEAD => COPPER_MELTS_C,
        // **The anvil is poured, not forged**, which is the one thing about
        // it that surprises people: you cannot beat a block of bronze that
        // size into shape, because the thing you would beat it on is the
        // thing you are making. It is cast in a sand bed in one go, and the
        // metal has to be liquid to do it. See `types::BLOCK_ANVIL`.
        | crate::types::BLOCK_ANVIL => BRONZE_MELTS_C,
        // Forged: an ingot worked hot onto a haft or a strap.
        // ...and a hook, which is a wire of the same bar bent hot.
        BLOCK_COPPER_KNIFE | BLOCK_COPPER_SPEAR | BLOCK_COPPER_HOOK | crate::types::BLOCK_COPPER_SAW => {
            COPPER_MELTS_C * FORGING_SHARE
        }
        BLOCK_BRONZE_KNIFE
        | BLOCK_BRONZE_AXE
        | BLOCK_BRONZE_PICKAXE
        | BLOCK_BRONZE_SPEAR
        | BLOCK_BRONZE_HELM
        | BLOCK_BRONZE_CUIRASS
        | BLOCK_BRONZE_GREAVES
        | BLOCK_BRONZE_BOOTS
        // ...and the smith's own two, which are forged like every other
        // bronze head. See `types::BLOCK_STONE_HAMMER`.
        | crate::types::BLOCK_BRONZE_HAMMER
        | crate::types::BLOCK_BRONZE_CHISEL
        // ...and a saw, a strip of the same bar drawn thin and toothed.
        | crate::types::BLOCK_BRONZE_SAW => BRONZE_MELTS_C * FORGING_SHARE,
        BLOCK_IRON_KNIFE
        | BLOCK_IRON_AXE
        | BLOCK_IRON_PICKAXE
        | BLOCK_IRON_SPEAR
        | BLOCK_IRON_HELM
        | BLOCK_IRON_CUIRASS
        | BLOCK_IRON_GREAVES
        | BLOCK_IRON_BOOTS
        // Nails are a bar drawn out and cut hot: the knife's heat.
        | BLOCK_NAILS
        | crate::types::BLOCK_IRON_HAMMER
        | crate::types::BLOCK_IRON_SAW => IRON_MELTS_C * FORGING_SHARE,
        BLOCK_IRON_BLOOM => BLOOM_C,
        BLOCK_IRON_INGOT => IRON_MELTS_C * WELDING_SHARE,
        BLOCK_STEEL_INGOT => CEMENTATION_C,
        _ => return None,
    })
}

/// How long one batch of this recipe takes in this hearth, in seconds.
///
/// **By recipe, and it was one number a hearth.** A campfire did everything
/// in eight seconds and a bloomery everything in twenty, so a steak and a
/// bloom were the same wait and charcoal came off a fire as fast as supper
/// did. Nothing a furnace did was worth walking away from, and walking away
/// is the one thing a furnace with slots exists to let a player do.
///
/// The numbers are real durations put through the world's clock -- a day is
/// nine hundred seconds, so an hour is thirty-seven and a half -- and squeezed
/// only where the literal figure would be a day of standing at a screen:
///
/// | work | in life | here |
/// |---|---|---|
/// | charring three logs in a fire | hours of smoulder | 75 s, two hours |
/// | firing a pot or a brick | three to six hours | 110 s, three hours |
/// | smelting ore in a crucible | one to two hours | 75 s |
/// | melting what is already metal | under an hour | 40 s |
/// | forging a tool, a point or a plate | an hour or two | 45 s |
/// | a bloom | six to ten hours | 300 s, eight hours |
/// | beating a bloom into a bar | two to three hours | 120 s |
/// | carburising a bar | hours to a day | 300 s |
///
/// **Food keeps the hearth's own time** (`Kind::cook_seconds`): supper is the
/// one batch a player stands and waits for, and it was right already.
pub fn batch_seconds(recipe: &Recipe, kind: Kind) -> f32 {
    use crate::types::*;
    let from_ore = recipe.inputs.iter().any(|&(block, _)| {
        matches!(
            block_kind(block),
            BLOCK_COPPER_ORE | BLOCK_TIN_ORE | BLOCK_IRON_ORE | BLOCK_IRON_DUST
        )
    });
    match block_kind(recipe.output.0) {
        BLOCK_COAL => 75.0,
        BLOCK_BRICK | BLOCK_VESSEL | BLOCK_MOULD | BLOCK_JUG => 110.0,
        BLOCK_IRON_BLOOM | BLOCK_STEEL_INGOT => 300.0,
        BLOCK_IRON_INGOT => 120.0,
        BLOCK_COPPER_INGOT | BLOCK_TIN_INGOT if from_ore => 75.0,
        BLOCK_COPPER_INGOT
        | BLOCK_TIN_INGOT
        | BLOCK_BRONZE_INGOT
        | BLOCK_COPPER_HOE_HEAD
        | BLOCK_COPPER_SHOVEL_HEAD
        | BLOCK_COPPER_AXE_HEAD
        | BLOCK_COPPER_PICK_HEAD => 40.0,
        // Everything hotter than cooking that is none of the above is metal
        // worked at a forging heat: knives, points, the steeled tools and
        // the armour.
        _ if needs_degrees(recipe).is_some_and(|degrees| degrees > COOKING_C) => 45.0,
        _ => kind.cook_seconds(),
    }
}

/// What a batch leaves in the tray besides the thing it was for.
///
/// **Slag, off every smelt of ore and off the beating of a bloom.** A crucible
/// of malachite is mostly rock, and what is not copper runs off as a glassy
/// crust; a bloom comes out of the shaft half slag and loses most of the rest
/// on the anvil. The tray showing it is the honest picture of why ore costs
/// what it costs -- most of what went in was never metal -- and what comes out
/// is a heavy stone a player can build with.
///
/// Not off melting: nuggets and ingots are metal already, and a pour of them
/// leaves nothing but the pot.
///
/// A function keyed off the inputs rather than a field on `Recipe`, for the
/// reason [`needs_degrees`] is one: a hundred and sixty rows would each have to
/// grow a field that means something on six of them.
pub fn byproduct(recipe: &Recipe) -> Option<(BlockId, u32)> {
    use crate::types::*;
    recipe
        .inputs
        .iter()
        .any(|&(block, _)| {
            matches!(
                block_kind(block),
                BLOCK_COPPER_ORE | BLOCK_TIN_ORE | BLOCK_IRON_ORE | BLOCK_IRON_DUST | BLOCK_IRON_BLOOM
            )
        })
        .then_some((BLOCK_SLAG, 1))
}

/// What a hearth is doing with what is in it.
///
/// **A recipe, or a jug on the boil.** Boiling is not a row in `RECIPES`,
/// and it cannot be: a hearth counts its ingredients *by kind* (see
/// `Inventory::count_within`), so a recipe asking for a jug of pond water
/// would be satisfied by a jug of river water and would boil it for ever,
/// turning it into itself a batch at a time. The jug is changed where it
/// stands instead -- which is also the honest picture: the pot is on the
/// fire, and what comes off it is the same pot.
#[derive(Debug, Clone, Copy)]
pub enum Batch {
    Recipe(&'static Recipe),
    Boil,
}

impl Batch {
    /// How hot the fire has to be for this to go on.
    pub fn degrees(self) -> f32 {
        match self {
            // A recipe with no heat would run at none; the test that
            // forbids one is what makes this unreachable, and infinity is
            // the answer that fails safe if it ever is not.
            Batch::Recipe(recipe) => needs_degrees(recipe).unwrap_or(f32::INFINITY),
            Batch::Boil => BOILING_C,
        }
    }

    /// How long it takes at that heat.
    pub fn seconds(self, kind: Kind) -> f32 {
        match self {
            Batch::Recipe(recipe) => batch_seconds(recipe, kind),
            Batch::Boil => BOIL_SECONDS,
        }
    }
}

/// What this hearth would work on next: a recipe if its load adds up to
/// one, a jug to boil if it does not.
///
/// **The recipe first**, because it is what the player loaded the hearth
/// *for*. A jug of pond water set beside the steaks is a jug that waits
/// its turn; the other order would boil the water while the steaks sat
/// raw, which is not what anybody putting both in meant.
pub fn next_batch(kind: Kind, contents: &Inventory) -> Option<Batch> {
    if let Some((_, recipe)) = next_recipe(kind, contents) {
        return Some(Batch::Recipe(recipe));
    }
    boiling_slot(contents).map(|_| Batch::Boil)
}

/// What boiling this turns it into, if boiling does anything to it.
///
/// **Standing water, and only standing water.** Boiling kills what makes
/// a pond sick (see `body::Water`), so a jug of it comes off the fire as
/// good as a river. It does not take the salt out of the sea -- nothing
/// does that but letting the water go, and a jug that boiled sea water
/// into fresh would make the whole coast a spring. Fresh water is already
/// fresh; a jug of it on the fire is left alone.
///
/// This is what turns the standing-water rule from a wall into a choice:
/// the pond beside the camp is drinkable after all, for twenty seconds of
/// fire, and whether that is cheaper than the walk to the river is the
/// player's arithmetic.
pub fn boils_into(block: BlockId) -> Option<BlockId> {
    use crate::body::Water;
    (block_kind(block) == crate::types::BLOCK_JUG_WATER
        && crate::types::vessel_water(block) == Water::Standing)
        .then(|| crate::types::jug_of(Water::Fresh))
}

/// The first ingredient slot holding something that boiling would change.
fn boiling_slot(contents: &Inventory) -> Option<usize> {
    INPUT_SLOTS
        .clone()
        .find(|&slot| contents.block_in(slot).and_then(boils_into).is_some())
}

/// Boils one jug through, in place. Returns false and changes nothing if
/// there is no longer anything to boil -- a player at the screen can take
/// the jug off between the tick that started it and the tick that ends it.
///
/// **In place**, keeping the slot, the count and the wear: the jug does
/// not go to the output tray, because it did not become anything else.
pub fn boil(contents: &mut Inventory) -> bool {
    let Some(slot) = boiling_slot(contents) else {
        return false;
    };
    let Some(boiled) = contents.block_in(slot).and_then(boils_into) else {
        return false;
    };
    contents.retype_slot(slot, boiled)
}

/// Is cooked food sitting in the tray of a fire hot enough to char it?
///
/// **The tray, not the ingredients.** What is still raw is still cooking
/// and leaves the heat the moment it is done; what is done and left there
/// is what burns. A campfire's tray is inside the ring of stones -- it is
/// the side of the fire, not a table beside it -- and that is the whole of
/// the rule a player has to hold: take supper off a hot fire.
///
/// Shared, because the screen has to say so in the same words the server
/// acts on, and a warning that disagreed with the fire by one degree would
/// be a warning a player learns to ignore.
pub fn is_charring(contents: &Inventory, degrees: f32) -> bool {
    degrees >= CHAR_C
        && contents.slots()[OUTPUT_SLOTS]
            .iter()
            .flatten()
            .any(|stack| crate::food::is_food(stack.block))
}

/// Turns one piece of cooked food in the tray to ash.
///
/// **One piece, not the stack**, so leaving supper too long costs part of
/// it rather than all of it -- a loss sized to the inattention. The ash
/// goes to the ash slot, and if that is full it goes nowhere: a pinch of
/// burnt meat is not worth jamming anything for.
pub fn char_one(contents: &mut Inventory) -> bool {
    let Some(slot) = OUTPUT_SLOTS
        .clone()
        .find(|&slot| contents.block_in(slot).is_some_and(crate::food::is_food))
    else {
        return false;
    };
    contents.take_from(slot, 1);
    contents.add_within(ASH_SLOT..ASH_SLOT + 1, crate::types::BLOCK_ASH, 1);
    true
}

/// How a temperature looks.
///
/// **TerraFirmaCraft's heat colours**, which are the colours a smith
/// actually reads, and the reason the screen names one instead of printing
/// a number: "bright red" tells a player what the fire is doing in words
/// they already know, and the font has no degree sign anyway. Its top
/// three bands (yellow-white, white, brilliant white) are one here: no
/// fire in this world gets past the first of them by enough to matter.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Glow {
    Cold,
    Warm,
    Hot,
    VeryHot,
    FaintRed,
    DarkRed,
    BrightRed,
    Orange,
    Yellow,
    White,
}

impl Glow {
    /// Coolest first.
    pub const ALL: [Glow; 10] = [
        Glow::Cold,
        Glow::Warm,
        Glow::Hot,
        Glow::VeryHot,
        Glow::FaintRed,
        Glow::DarkRed,
        Glow::BrightRed,
        Glow::Orange,
        Glow::Yellow,
        Glow::White,
    ];

    /// Where this band begins, in degrees.
    pub fn starts_at(self) -> f32 {
        match self {
            Glow::Cold => f32::NEG_INFINITY,
            Glow::Warm => 1.0,
            Glow::Hot => 80.0,
            Glow::VeryHot => 210.0,
            Glow::FaintRed => 480.0,
            Glow::DarkRed => 580.0,
            Glow::BrightRed => 730.0,
            Glow::Orange => 930.0,
            Glow::Yellow => 1100.0,
            Glow::White => 1300.0,
        }
    }

    /// The band a temperature falls in. A temperature that is not a number
    /// is cold, which is the answer that draws nothing alarming.
    pub fn of(degrees: f32) -> Glow {
        Glow::ALL
            .iter()
            .rev()
            .copied()
            .find(|glow| degrees >= glow.starts_at())
            .unwrap_or(Glow::Cold)
    }
}

/// What this hearth would start cooking, given what is in it.
///
/// `None` for an empty one, for one whose ingredients do not add up to
/// anything, and for one whose output slots are too full to take the
/// result -- the last of which is deliberately not an error anywhere: a
/// furnace with a full output tray simply stops, and starts again when
/// somebody empties it.
///
/// **The richest match, then the first in `RECIPES` order** -- so a
/// hearth loaded with the same things always does the same thing, and
/// what it does is the thing the load was for.
///
/// The order alone was the whole rule, and it quietly closed off a third
/// of the game. A hearth has no recipe picker: the player says what they
/// want by *what they put in it*, and this is the only thing that reads
/// that. But the table runs cheapest-first within a family -- knife,
/// axe, pick; helm, boots, greaves, cuirass -- and a knife's ingredients
/// are a strict subset of a pick's. So a kiln loaded with three copper
/// ingots, a haft and a strap matched "copper knife" first and made one,
/// spending the haft and the strap on it: **the copper pick could not be
/// made at all**, and with it the iron ore it is the only tool for. The
/// same shadow fell on every piece of armour but the two helms. Twelve
/// recipes in the table that nothing in the world could reach.
///
/// So a recipe is passed over when another one the hearth could equally
/// run asks for *everything it asks for and more* -- see
/// [`asks_for_more`]. Between recipes that are not comparable that way
/// nothing changes, which is every other pair in the table (checked by
/// `the_only_recipes_that_shadow_each_other_are_the_ones_that_should`).
///
/// The order of the tests is cost. Counting ingredients reads four
/// slots; the domination scan is a walk of two short lists; only
/// `fits_in_output` clones the container, and it runs last and stops at
/// the first candidate -- so the common case still costs one clone.
pub fn next_recipe(kind: Kind, contents: &Inventory) -> Option<(usize, &'static Recipe)> {
    // **What the row refuses does not count as loaded** (`crafting::refuses`):
    // the salt row asks for a jug of water and means the sea, and a fire with
    // the river in it read as loaded for salt -- so the jug somebody put
    // there to boil was never boiled, and no salt came either.
    let loaded_for = |recipe: &Recipe| {
        let held = |block| {
            INPUT_SLOTS
                .clone()
                .filter_map(|slot| contents.slots().get(slot).copied().flatten())
                .filter(|stack| {
                    crate::types::block_kind(stack.block) == crate::types::block_kind(block)
                        && !crate::crafting::refuses(recipe, stack.block)
                })
                .map(|stack| stack.count)
                .sum::<u32>()
        };
        kind.runs(recipe.station) && recipe.inputs.iter().all(|&(block, count)| held(block) >= count)
    };
    RECIPES.iter().enumerate().find(|(_, recipe)| {
        loaded_for(recipe)
            && !RECIPES
                .iter()
                .any(|richer| asks_for_more(richer, recipe) && loaded_for(richer))
            && fits_in_output(contents, recipe)
    })
}

/// Does `richer` want everything `plainer` wants, and more of it?
///
/// The comparison that decides which of two possible batches a loaded
/// hearth is actually loaded *for*. Not "costs more" in any general
/// sense -- a recipe asking for two of one thing does not shadow one
/// asking for three of another, because a player with two could not have
/// meant the second. It has to be a superset, item for item, or the
/// hearth would start guessing.
///
/// Equal loads are not "more": two recipes asking for exactly the same
/// things are settled by table order, as everything used to be.
fn asks_for_more(richer: &Recipe, plainer: &Recipe) -> bool {
    let wanted = |recipe: &Recipe, block| {
        recipe
            .inputs
            .iter()
            .find(|&&(candidate, _)| candidate == block)
            .map_or(0, |&(_, count)| count)
    };
    let total = |recipe: &Recipe| recipe.inputs.iter().map(|&(_, count)| count).sum::<u32>();
    plainer
        .inputs
        .iter()
        .all(|&(block, count)| wanted(richer, block) >= count)
        && total(richer) > total(plainer)
}

/// Is there room for everything this recipe would produce?
///
/// **What is returned goes back where it came from.** A crucible and a
/// mould are not results, they are the apparatus: they were in the
/// input slots when the batch started and they belong there when it
/// ends, so a hearth loaded with a pot, a mould, ore and charcoal keeps
/// working batch after batch until the ore runs out. Putting them in the
/// output tray instead would mean a player carrying the pot back from
/// one side of the screen to the other after every single ingot -- and
/// with only two output slots, three things coming out at once would
/// not fit at all.
fn fits_in_output(contents: &Inventory, recipe: &Recipe) -> bool {
    fits_in_output_as(contents, recipe, 0)
}

/// [`fits_in_output`], for a result carrying `word` in its damage field:
/// a marked piece only stacks onto the same mark, so it can need a slot a
/// plain one would not.
fn fits_in_output_as(contents: &Inventory, recipe: &Recipe, word: u32) -> bool {
    // Checked against a copy that has already had the ingredients taken
    // out of it, because that is the state the results actually land in:
    // the ore leaves the input slot the pot is going back into.
    let mut trial = contents.clone();
    for &(block, count) in recipe.inputs {
        trial.take_within(INPUT_SLOTS, block, count);
    }
    if trial.add_worn_within(OUTPUT_SLOTS, recipe.output.0, recipe.output.1, word) > 0 {
        return false;
    }
    // ...and the slag beside it, or the batch does not start: a smelt that
    // ran and threw its slag away would be a tray that lies about what ore
    // is. See [`byproduct`].
    if let Some((block, count)) = byproduct(recipe) {
        if trial.add_within(OUTPUT_SLOTS, block, count) > 0 {
            return false;
        }
    }
    recipe
        .returns
        .iter()
        .all(|&(block, count)| trial.add_within(INPUT_SLOTS, block, count) == 0)
}

/// Runs one batch: takes the ingredients out and puts the results in.
///
/// Returns false and changes nothing if the recipe no longer fits, which
/// can happen between the tick that started it and the tick that
/// finishes it -- a player can empty the input slots while it cooks.
pub fn complete(contents: &mut Inventory, recipe: &Recipe) -> bool {
    complete_made(contents, recipe, None)
}

/// [`complete`], with the result judged: `quality` is the cook's, where
/// the server found one (`logic::smelting`), and only lands on a result
/// that carries a judgement at all (`quality::takes_quality`) -- an ingot
/// is an ingot.
///
/// **A tray with no room for another grade gets the batch unmarked**
/// rather than refusing it. `next_recipe` asks about room for a plain
/// result, since nobody knows the grade until it is rolled; a fine
/// haunch arriving at a tray already holding a good one and a slag
/// needs a third slot, and refusing there would reset the batch every
/// time it finished -- a hearth that cooks for ever and never serves.
pub fn complete_made(contents: &mut Inventory, recipe: &Recipe, quality: Option<crate::quality::Quality>) -> bool {
    if !recipe
        .inputs
        .iter()
        .all(|&(block, count)| contents.count_within(INPUT_SLOTS, block) >= count)
        || !fits_in_output(contents, recipe)
    {
        return false;
    }
    let word = quality
        .filter(|_| crate::quality::takes_quality(recipe.output.0))
        .map(|quality| crate::inventory::Stack::new(recipe.output.0, 0).with_quality(quality).damage)
        .filter(|&word| fits_in_output_as(contents, recipe, word))
        .unwrap_or(0);
    for &(block, count) in recipe.inputs {
        contents.take_within(INPUT_SLOTS, block, count);
    }
    contents.add_worn_within(OUTPUT_SLOTS, recipe.output.0, recipe.output.1, word);
    if let Some((block, count)) = byproduct(recipe) {
        contents.add_within(OUTPUT_SLOTS, block, count);
    }
    // The apparatus, back in the input slots it was loaded into.
    for &(block, count) in recipe.returns {
        contents.add_within(INPUT_SLOTS, block, count);
    }
    true
}

/// The raw pottery a batch of `recipe` would take out of the input slots,
/// piece by piece, in the order `Inventory::take_within` takes it -- so the
/// wetness judged is the wetness of the pieces that actually go into the
/// fire, not of whatever else is lying in the slot beside them.
fn pieces_to_fire(contents: &Inventory, recipe: &Recipe) -> Vec<BlockId> {
    let mut pieces = Vec::new();
    for &(block, count) in recipe.inputs {
        if !crate::clay::is_raw_pottery(block) {
            continue;
        }
        let mut left = count;
        for stack in INPUT_SLOTS.filter_map(|slot| contents.slots().get(slot).copied().flatten()) {
            if left == 0 {
                break;
            }
            if crate::types::block_kind(stack.block) != crate::types::block_kind(block) {
                continue;
            }
            let taken = stack.count.min(left);
            pieces.extend(std::iter::repeat_n(stack.block, taken as usize));
            left -= taken;
        }
    }
    pieces
}

/// [`complete_made`], for a batch that fires raw pottery: each piece that
/// went in wet is rolled against `clay::crack_chance`, and a piece that
/// cracks comes out as nothing. Answers how many cracked, or `None` if the
/// batch did not run at all.
///
/// **One roll a piece, and the batch still runs.** Four wet bricks are four
/// chances, so a batch comes out with some of its bricks and not none or all
/// -- the player's "cracks some of it". The rolls are the caller's, for
/// `crafting::Attempt`'s reason: the shared crate holds no dice.
///
/// A cracked piece takes its share of the result out of the tray and nothing
/// else: the clay and the three hours of fuel are what it cost.
pub fn complete_fired(
    contents: &mut Inventory,
    recipe: &Recipe,
    quality: Option<crate::quality::Quality>,
    mut roll: impl FnMut() -> f32,
) -> Option<u32> {
    let pieces = pieces_to_fire(contents, recipe);
    if !complete_made(contents, recipe, quality) {
        return None;
    }
    let cracked = pieces.iter().filter(|&&piece| crate::clay::cracks(piece, roll())).count() as u32;
    if cracked > 0 && !pieces.is_empty() {
        // One piece in is one piece out for every firing row (a brick is a
        // brick), and the share is worked out rather than assumed so a row
        // that ever fires four into two stays honest.
        let lost = recipe.output.1 * cracked / pieces.len() as u32;
        contents.take_within(OUTPUT_SLOTS, recipe.output.0, lost);
    }
    Some(cracked)
}

/// Whether this block is a hearth that is currently alight.
#[inline]
pub fn is_lit(block: BlockId) -> bool {
    Kind::of(block).is_some() && is_burning(block)
}

#[cfg(test)]
mod tests {
    use super::*;

    use crate::inventory::Stack;
    use crate::types::{BLOCK_COAL, BLOCK_COOKED_MEAT, BLOCK_LOG, BLOCK_RAW_MEAT, BLOCK_STONE};

    fn loaded(slots: &[(usize, BlockId, u32)]) -> Inventory {
        let mut inventory = Inventory::new();
        for &(slot, block, count) in slots {
            inventory.put_in_slot(slot, Stack::new(block, count));
        }
        inventory
    }

    #[test]
    fn the_slots_do_not_overlap_and_fit_the_inventory() {
        const { assert!(INPUT_SLOTS.end <= FUEL_SLOT) };
        const { assert!(FUEL_SLOT < OUTPUT_SLOTS.start) };
        const { assert!(OUTPUT_SLOTS.end == ASH_SLOT) };
        const { assert!(ASH_SLOT + 1 == USED_SLOTS) };
        const { assert!(USED_SLOTS <= crate::inventory::SLOTS) };
    }

    #[test]
    fn nothing_may_be_put_into_the_output_or_the_ash() {
        for slot in OUTPUT_SLOTS.chain(ASH_SLOT..ASH_SLOT + 1) {
            assert!(!accepts(slot, BLOCK_STONE));
            assert!(!accepts(slot, BLOCK_COAL), "even fuel");
            assert!(!accepts(slot, crate::types::BLOCK_ASH), "even ash, in slot {slot}");
        }
    }

    #[test]
    fn the_fuel_slot_takes_fuel_and_nothing_else() {
        assert!(accepts(FUEL_SLOT, BLOCK_COAL));
        assert!(accepts(FUEL_SLOT, BLOCK_LOG));
        assert!(!accepts(FUEL_SLOT, BLOCK_STONE));
        // ...and the input slots take whatever, including fuel: coal is
        // an ingredient of every smelt in the game.
        assert!(accepts(0, BLOCK_COAL));
        assert!(accepts(0, BLOCK_STONE));
    }

    #[test]
    fn a_hearth_makes_the_thing_it_was_loaded_for_rather_than_the_cheapest_one() {
        // **A hearth has no recipe picker.** What the player wants is
        // said entirely by what they put in it, so this is the whole of
        // the conversation -- and for two versions it answered the wrong
        // half of it.
        //
        // The table runs cheapest-first inside a family, and a knife's
        // ingredients are a strict subset of a pick's. Taking the first
        // match therefore meant a kiln loaded with three copper ingots,
        // a haft and a strap made a *knife* and spent the haft and strap
        // doing it. The copper pick could not be made at all -- and it
        // is the only tool that touches iron ore, so the iron age was
        // shut. The same shadow covered every piece of armour but the
        // two helms: twelve recipes in the table that nothing in the
        // world could reach.
        use crate::types::*;
        let forge = |blocks: &[(BlockId, u32)]| {
            let mut fire = Inventory::new();
            for &(block, count) in blocks {
                fire.add_within(INPUT_SLOTS, block, count);
            }
            fire
        };
        let makes = |kind: Kind, fire: &Inventory| {
            next_recipe(kind, fire).map(|(_, recipe)| recipe.output.0)
        };

        // **The castings, one rung at a time**, which is where the tool
        // family lives now: a kiln pours a head and a person hafts it
        // (see the copper bench in `crafting`). What comes out is what
        // the metal in the tray was enough for, exactly as before -- the
        // ladder moved from finished tools to the pieces they are made
        // of, and the shadowing rule did not change at all.
        let pour = [(BLOCK_VESSEL, 1), (BLOCK_MOULD, 1)];
        for (ingots, wanted) in [
            (1, BLOCK_COPPER_SHOVEL_HEAD),
            (2, BLOCK_COPPER_AXE_HEAD),
            (3, BLOCK_COPPER_PICK_HEAD),
        ] {
            let mut load = vec![(BLOCK_COPPER_INGOT, ingots)];
            load.extend_from_slice(&pour);
            assert_eq!(
                makes(Kind::Kiln, &forge(&load)),
                Some(wanted),
                "{ingots} copper ingots made the wrong thing"
            );
        }
        // ...and the hoe blade, the one casting poured without a
        // crucible: a finger of metal straight into the mould. That is
        // *why* it is drawn that way -- two castings asking for the same
        // load would be one casting, with the second unreachable for
        // ever.
        assert_eq!(
            makes(Kind::Kiln, &forge(&[(BLOCK_COPPER_INGOT, 1), (BLOCK_MOULD, 1)])),
            Some(BLOCK_COPPER_HOE_HEAD),
            "a mould and one ingot made the wrong thing"
        );
        // The knife stays whole, and is still the plainest thing a kiln
        // makes. See its recipe for why a blade is beaten rather than
        // cast.
        assert_eq!(
            makes(
                Kind::Kiln,
                &forge(&[
                    (BLOCK_COPPER_INGOT, 1),
                    (BLOCK_WORKED_STICK, 1),
                    (BLOCK_SINEW, 1)
                ])
            ),
            Some(BLOCK_COPPER_KNIFE),
            "a haft, a sinew and an ingot made the wrong thing"
        );

        // ...and the armour family, whose four rungs are not in cost
        // order in the table, so this is the case that says the rule is
        // about *what is asked for* rather than about the row number.
        for (ingots, leather, wanted) in [
            (2, 1, BLOCK_BRONZE_HELM),
            (3, 1, BLOCK_BRONZE_BOOTS),
            (4, 1, BLOCK_BRONZE_GREAVES),
            (5, 2, BLOCK_BRONZE_CUIRASS),
        ] {
            assert_eq!(
                makes(
                    Kind::Bloomery,
                    &forge(&[(BLOCK_BRONZE_INGOT, ingots), (BLOCK_LEATHER, leather)])
                ),
                Some(wanted),
                "{ingots} bronze and {leather} leather made the wrong thing"
            );
        }
    }

    #[test]
    fn the_only_recipes_that_shadow_each_other_are_the_ones_that_should() {
        // The other half of the rule: passing a recipe over is only safe
        // where the two loads are genuinely comparable. A hearth that
        // preferred whatever was *dearest* would make a crucible out of
        // the clay somebody meant for bricks.
        //
        // So the shadowing has to stay confined to the families it was
        // written for -- tools and armour, where one rung really is the
        // next rung with more metal in it. This is the list, and anything
        // that joins it has to say so here.
        //
        // Per hearth, because that is how `next_recipe` asks: a recipe
        // is only ever passed over for one the *same fire* could run, so
        // a bloomery row can never shadow something a campfire makes.
        let mut shadowed: Vec<&str> = Vec::new();
        for kind in [Kind::Campfire, Kind::Kiln, Kind::Bloomery] {
            for plainer in RECIPES.iter().filter(|r| kind.runs(r.station)) {
                if RECIPES
                    .iter()
                    .filter(|r| kind.runs(r.station))
                    .any(|richer| asks_for_more(richer, plainer))
                {
                    shadowed.push(plainer.name);
                }
            }
        }
        shadowed.sort_unstable();
        shadowed.dedup();
        // Five ladders -- three of tools and two of armour -- and what
        // is shadowed is every rung of each but its top. Nothing outside
        // them, which is the property that matters: no smelt, no
        // firing, no cooking and no tanning is comparable with anything
        // else, so none of them changed at all.
        //
        // **Copper's ladder is the four castings now**, not the finished
        // tools: a copper axe is hafted by hand and a hearth never sees
        // it. Same relation, one step earlier in the chain -- see the
        // copper bench in `crafting`, where the loads were chosen to
        // make exactly this ladder, because a hearth infers its recipe
        // from what is in the tray and two equal loads are one recipe.
        // The copper knife is off the list for the same reason it used
        // to be on it: what shadowed it was the copper axe.
        //
        // **And one entry here is not a ladder at all**, which is the
        // first time that has been true and is worth saying out loud:
        // the *bronze ingot* shadows the pick casting. Three copper, a
        // crucible and a mould is what a pick head costs; the same load
        // with a lump of tin in it is bronze. They are comparable
        // because they are the same pour with one thing added, and the
        // answer the hearth gives is the right one -- nobody drops tin
        // in the fire by accident, and a tray with tin in it is a tray
        // that meant bronze. To cast a pick head, do not put tin in it.
        // **And "boil salt" is shadowed by the rows that want a jug and more**
        // -- the dough and the porridge. That is the right answer too: a tray
        // with flour and a jug in it meant bread, and nobody loads a fire with
        // supper and a jug of the sea to get salt out of it.
        // **And "cooked meat" and "roast a root" are shadowed by the stew**,
        // for the same reason: a tray with a haunch, a root, a jug of water
        // and two bowls in it is a tray that meant stew. To roast them, leave
        // the bowls out -- which is the whole decision `types::BLOCK_STEW` is
        // about, made by what the player puts on the fire.
        assert_eq!(
            shadowed,
            [
                "axe casting",
                "boil salt",
                "br. greaves",
                "bronze axe",
                "bronze boots",
                // **The smith's own two, and their ladder is "add a
                // lashing".** A hammer and a chisel are driven into their
                // hafts and nothing is tied round them (see the rows in
                // `crafting`), so their loads are the metal tools' loads with
                // the sinew left out -- and a tray with sinew in it is a tray
                // that meant an edge. One bar and a haft is a chisel; add a
                // tendon and it is a knife. Two bars and a haft is a hammer;
                // add a tendon and it is an axe. That is the right answer
                // every time: nobody drops a tendon in a fire by accident.
                "bronze chisel",
                "bronze hammer",
                "bronze helm",
                "bronze knife",
                "cooked meat",
                // A copper bar alone is hooks, on the nails' terms: a bar with
                // a haft, a mould or a crucible beside it is what those were
                // for. See the "copper hooks" row in `crafting`.
                "copper hooks",
                "hoe casting",
                "iron axe",
                "iron boots",
                "iron greaves",
                // ...and the iron hammer, shadowed by the iron axe for the
                // reason the bronze one is shadowed by the bronze axe.
                "iron hammer",
                "iron helm",
                "iron knife",
                // A bar alone is nails, and a bar with anything else in the
                // tray is what the anything was for: the foot of the iron
                // ladder, on purpose. See the "nails" row in `crafting`.
                "nails",
                "pick casting",
                "roast a root",
                "shovel casting",
                // The steeled tools are a ladder of their own, the iron
                // ones' shape with steel bars and a jug of water in it.
                "steel axe",
                "steel knife",
            ]
        );
        // Nothing shadows itself, and the relation cannot point both
        // ways -- either would make which batch runs depend on the order
        // the check happened to walk the table in.
        for a in RECIPES {
            assert!(!asks_for_more(a, a), "{} shadows itself", a.name);
            for b in RECIPES {
                assert!(
                    !(asks_for_more(a, b) && asks_for_more(b, a)),
                    "{} and {} shadow each other",
                    a.name,
                    b.name
                );
            }
        }
    }

    #[test]
    fn a_campfire_cooks_meat_and_a_kiln_does_not_melt_iron() {
        let meat = loaded(&[(0, BLOCK_RAW_MEAT, 3)]);
        let (_, recipe) = next_recipe(Kind::Campfire, &meat).expect("a campfire cooks meat");
        assert_eq!(recipe.output.0, BLOCK_COOKED_MEAT);

        // The ladder, checked at its two edges: a campfire runs nothing
        // that needs a kiln, and only a bloomery wins iron.
        assert!(!Kind::Campfire.runs(Station::Forge));
        assert!(!Kind::Kiln.runs(Station::Bloomery));
        assert!(Kind::Kiln.runs(Station::Heat), "a kiln is still a fire");
    }

    #[test]
    fn what_is_in_the_output_is_not_an_ingredient() {
        // The whole reason the slots have ranges. Cooked meat sitting in
        // the output must not be counted as raw meat's neighbour, and
        // fuel in the fuel slot must not be smelted -- a hearth that ate
        // its own results would empty its output tray for ever.
        let mut inventory = loaded(&[(FUEL_SLOT, BLOCK_COAL, 8), (5, BLOCK_RAW_MEAT, 4)]);
        assert!(
            next_recipe(Kind::Campfire, &inventory).is_none(),
            "it cooked what was in the fuel and output slots"
        );
        inventory.put_in_slot(0, Stack::new(BLOCK_RAW_MEAT, 1));
        assert!(next_recipe(Kind::Campfire, &inventory).is_some());
    }

    #[test]
    fn a_full_output_stops_it_rather_than_losing_the_result() {
        let inventory = loaded(&[
            (0, BLOCK_RAW_MEAT, 4),
            (5, BLOCK_STONE, crate::inventory::MAX_STACK),
            (6, BLOCK_STONE, crate::inventory::MAX_STACK),
        ]);
        assert!(
            next_recipe(Kind::Campfire, &inventory).is_none(),
            "it started a batch it had nowhere to put"
        );
    }

    #[test]
    fn running_a_batch_moves_exactly_what_the_recipe_says() {
        let mut inventory = loaded(&[(0, BLOCK_RAW_MEAT, 3)]);
        let (_, recipe) = next_recipe(Kind::Campfire, &inventory).unwrap();
        assert!(complete(&mut inventory, recipe));
        assert_eq!(
            inventory.count_within(INPUT_SLOTS, BLOCK_RAW_MEAT),
            3 - recipe.inputs[0].1
        );
        assert_eq!(
            inventory.count_within(OUTPUT_SLOTS, recipe.output.0),
            recipe.output.1
        );
    }

    #[test]
    fn the_crucible_stays_in_the_fire() {
        // The apparatus goes back into the input slots, so a hearth
        // loaded with a pot, a mould, ore and charcoal smelts batch
        // after batch without a player shuttling the pot back across the
        // screen between each one.
        use crate::types::{BLOCK_COPPER_ORE, BLOCK_MOULD, BLOCK_VESSEL};
        // Three smelts' worth: three ore and two charcoal a pour.
        let mut inventory = loaded(&[
            (0, BLOCK_COPPER_ORE, 9),
            (1, BLOCK_COAL, 6),
            (2, BLOCK_VESSEL, 1),
            (3, BLOCK_MOULD, 1),
        ]);
        for batch in 0..3 {
            let (_, recipe) = next_recipe(Kind::Campfire, &inventory)
                .unwrap_or_else(|| panic!("nothing to smelt on batch {batch}"));
            assert!(complete(&mut inventory, recipe));
            assert_eq!(
                inventory.count_within(INPUT_SLOTS, BLOCK_VESSEL),
                1,
                "the crucible left the fire after batch {batch}"
            );
            assert_eq!(inventory.count_within(INPUT_SLOTS, BLOCK_MOULD), 1);
        }
        assert_eq!(
            inventory.count_within(OUTPUT_SLOTS, crate::types::BLOCK_COPPER_INGOT),
            3
        );
    }

    #[test]
    fn a_batch_whose_ingredients_went_away_does_not_run() {
        // A player can empty the input slots while it cooks; the tick
        // that finishes the batch is not the tick that started it.
        let mut inventory = loaded(&[(0, BLOCK_RAW_MEAT, 3)]);
        let (_, recipe) = next_recipe(Kind::Campfire, &inventory).unwrap();
        inventory.take_within(INPUT_SLOTS, BLOCK_RAW_MEAT, 3);
        assert!(!complete(&mut inventory, recipe));
        assert!(inventory.is_empty(), "it produced something out of nothing");
    }

    #[test]
    fn every_recipe_for_a_fire_can_be_run_by_some_hearth() {
        // The rule that keeps the two tables honest now that the player
        // cannot run these at all: a recipe whose station no hearth
        // serves is a recipe nobody can ever make.
        for recipe in RECIPES {
            if !recipe.station.is_hearth() {
                continue;
            }
            assert!(
                [Kind::Campfire, Kind::Kiln, Kind::Bloomery]
                    .iter()
                    .any(|kind| kind.runs(recipe.station)),
                "'{}' needs a station no hearth is",
                recipe.name
            );
        }
    }

    /// Every id a block can have, stripped of its variant.
    fn every_block() -> impl Iterator<Item = BlockId> {
        (0..(1u32 << crate::types::VARIANT_SHIFT)).map(|id| id as BlockId)
    }

    const HEARTHS: [Kind; 3] = [Kind::Campfire, Kind::Kiln, Kind::Bloomery];

    #[test]
    fn everything_that_burns_says_how_hot_as_well_as_how_long() {
        // A fuel with a burn time and no temperature is a fuel the fire
        // takes and cannot heat on; the other way round, a fuel the slot
        // refuses that the fire would have burned hot.
        for block in every_block() {
            assert_eq!(
                fuel_seconds(block).is_some(),
                fuel_degrees(block).is_some(),
                "block {block} burns for a time or at a heat, and not both"
            );
        }
    }

    #[test]
    fn a_wet_pot_in_the_kiln_can_crack_and_a_dry_one_never_does() {
        use crate::types::{BLOCK_BRICK, BLOCK_BRICK_RAW};
        let row = RECIPES.iter().find(|r| r.inputs == [(BLOCK_BRICK_RAW, 4)]).expect("the brick row");
        let batch = |raw: BlockId, rolls: &[f32]| {
            let mut kiln = Inventory::new();
            kiln.put_in_slot(0, Stack::new(raw, 4));
            let mut rolls = rolls.iter().copied();
            let cracked = complete_fired(&mut kiln, row, None, || rolls.next().unwrap_or(0.99)).expect("the batch ran");
            (cracked, kiln.count_within(OUTPUT_SLOTS, BLOCK_BRICK))
        };
        // Wet: two of the four rolls land under a half, and two bricks come out.
        assert_eq!(batch(BLOCK_BRICK_RAW, &[0.1, 0.9, 0.3, 0.7]), (2, 2));
        // Bone-dry: the same dice crack nothing.
        let dry = crate::clay::with_dryness(BLOCK_BRICK_RAW, crate::clay::Dryness::BoneDry);
        assert_eq!(batch(dry, &[0.1, 0.9, 0.3, 0.7]), (0, 4));
        // ...and the rolls a wet batch needs are not all bad luck: a batch
        // whose dice all land high comes out whole, wet or not.
        assert_eq!(batch(BLOCK_BRICK_RAW, &[0.9; 4]), (0, 4));
    }

    #[test]
    fn green_wood_cooks_supper_and_fires_no_pot() {
        // The seasoning decision, in the hearth's numbers: a log off the
        // stump is fuel for supper and not for the kiln, in any hearth, and
        // the same log seasoned fires a pot in a kiln.
        for wood in crate::wood::WOODS {
            let green = crate::wood::green(wood.log);
            for kind in HEARTHS {
                let heat = kind.reaches(green).expect("a green log is still fuel");
                assert!(heat >= COOKING_C, "green {} cannot cook in a {kind:?}", crate::types::block_name(wood.log));
                assert!(
                    heat < FIRING_C,
                    "green {} reaches {heat} in a {kind:?}, enough to fire a pot",
                    crate::types::block_name(wood.log)
                );
            }
            assert!(
                fuel_seconds(green) < fuel_seconds(wood.log),
                "green {} burns as long as seasoned",
                crate::types::block_name(wood.log)
            );
        }
        assert!(Kind::Kiln.reaches(BLOCK_LOG).is_some_and(|heat| heat >= FIRING_C), "seasoned oak no longer fires a pot");
    }

    #[test]
    fn every_batch_a_hearth_runs_names_a_heat_some_fire_can_reach() {
        // A recipe with no temperature would run at none, and one whose
        // temperature no fuel in any hearth that runs it can reach is a
        // recipe the heat has quietly taken out of the game.
        let hottest = |kind: Kind| {
            every_block()
                .filter_map(|fuel| kind.reaches(fuel))
                .fold(0.0f32, f32::max)
        };
        for recipe in RECIPES.iter().filter(|r| r.station.is_hearth()) {
            let needs = needs_degrees(recipe)
                .unwrap_or_else(|| panic!("'{}' does not say how hot it wants the fire", recipe.name));
            assert!(
                HEARTHS
                    .iter()
                    .any(|&kind| kind.runs(recipe.station) && hottest(kind) >= needs),
                "'{}' wants {needs} and no hearth that runs it gets that hot",
                recipe.name
            );
        }
    }

    #[test]
    fn molten_metal_wants_charcoal_whatever_the_hearth() {
        // **The line the fuel table is drawn to.** Everything that pours
        // copper -- ingots, castings, the bronze that is copper with tin in
        // it -- wants charcoal in the fuel slot, in every hearth that can
        // run it. If sticks in a kiln could do it, the charcoal chain is
        // three logs for nothing.
        use crate::types::BLOCK_COAL;
        for recipe in RECIPES.iter().filter(|r| r.station.is_hearth()) {
            let needs = needs_degrees(recipe).expect("every hearth recipe has a heat");
            if needs < COPPER_MELTS_C {
                continue;
            }
            for kind in HEARTHS.into_iter().filter(|kind| kind.runs(recipe.station)) {
                for fuel in every_block().filter(|&b| is_fuel(b) && block_kind(b) != BLOCK_COAL) {
                    let heat = kind.reaches(fuel).expect("a fuel");
                    assert!(
                        heat < needs,
                        "a {} on block {fuel} reaches {heat}, enough for '{}' without charcoal",
                        kind.name(),
                        recipe.name
                    );
                }
            }
            assert!(
                HEARTHS.iter().any(|&kind| kind.runs(recipe.station)
                    && kind.reaches(BLOCK_COAL).is_some_and(|heat| heat >= needs)),
                "'{}' cannot be made even on charcoal",
                recipe.name
            );
        }
    }

    #[test]
    fn a_campfire_on_anything_but_charcoal_cooks_supper_and_never_chars_it() {
        // The other half of the decision the fuel slot is: wood is for
        // eating, charcoal is for metal, and the fire that is good at one
        // is bad at the other.
        use crate::types::BLOCK_COAL;
        for fuel in every_block().filter(|&b| is_fuel(b) && block_kind(b) != BLOCK_COAL) {
            let heat = Kind::Campfire.reaches(fuel).expect("a fuel");
            assert!(heat >= COOKING_C, "block {fuel} burns at {heat}, too cool to cook on");
            assert!(heat < CHAR_C, "block {fuel} burns at {heat} and chars the supper");
        }
        assert!(
            Kind::Campfire.reaches(BLOCK_COAL).is_some_and(|heat| heat >= CHAR_C),
            "a charcoal fire is a safe place to leave supper, and the choice is gone"
        );
    }

    #[test]
    fn a_kiln_fires_pottery_on_what_a_bog_gives_it() {
        // Peat is the fuel of the place with no trees, and a kiln built
        // there has to be able to make the pot the copper is melted in.
        use crate::types::{BLOCK_BIRCH_LOG, BLOCK_DRIED_PEAT, BLOCK_LOG};
        for fuel in [BLOCK_DRIED_PEAT, BLOCK_LOG, BLOCK_BIRCH_LOG] {
            let heat = Kind::Kiln.reaches(fuel).expect("a fuel");
            assert!(heat >= FIRING_C, "a kiln on block {fuel} reaches {heat}, short of a pot");
        }
    }

    #[test]
    fn boiling_makes_pond_water_safe_and_leaves_the_sea_salt() {
        use crate::body::Water;
        use crate::types::{jug_of, vessel_water};
        assert_eq!(boils_into(jug_of(Water::Standing)), Some(jug_of(Water::Fresh)));
        assert_eq!(boils_into(jug_of(Water::Salt)), None, "boiling took the salt out of the sea");
        assert_eq!(boils_into(jug_of(Water::Fresh)), None, "fresh water has nothing to boil off");
        assert_eq!(boils_into(BLOCK_STONE), None);

        // In place: the jug is where it was put, and is river water now.
        let mut fire = loaded(&[(2, jug_of(Water::Standing), 1)]);
        assert!(matches!(next_batch(Kind::Campfire, &fire), Some(Batch::Boil)));
        assert!(boil(&mut fire));
        assert_eq!(fire.block_in(2).map(vessel_water), Some(Water::Fresh));
        assert!(next_batch(Kind::Campfire, &fire).is_none(), "it would boil fresh water for ever");
        assert!(!boil(&mut fire));

        // ...and it waits behind the steaks it was put in beside.
        let mut supper = loaded(&[(0, BLOCK_RAW_MEAT, 2), (1, jug_of(Water::Standing), 1)]);
        assert!(matches!(next_batch(Kind::Campfire, &supper), Some(Batch::Recipe(_))));
        supper.take_within(INPUT_SLOTS, BLOCK_RAW_MEAT, 2);
        assert!(matches!(next_batch(Kind::Campfire, &supper), Some(Batch::Boil)));
    }

    #[test]
    fn food_left_in_the_tray_at_orange_heat_chars_a_piece_at_a_time() {
        let mut fire = loaded(&[(OUTPUT_SLOTS.start, BLOCK_COOKED_MEAT, 3)]);
        assert!(!is_charring(&fire, CHAR_C - 1.0), "a wood fire burnt the supper");
        assert!(is_charring(&fire, CHAR_C));

        assert!(char_one(&mut fire));
        assert_eq!(fire.count_within(OUTPUT_SLOTS, BLOCK_COOKED_MEAT), 2, "it took the whole stack");
        assert_eq!(fire.block_in(ASH_SLOT), Some(crate::types::BLOCK_ASH));

        // Metal in the tray is not supper, and does not burn.
        let ingots = loaded(&[(OUTPUT_SLOTS.start, crate::types::BLOCK_COPPER_INGOT, 3)]);
        assert!(!is_charring(&ingots, 1500.0), "an ingot charred");
        // Neither does what is still raw: it is cooking, not waiting.
        let raw = loaded(&[(0, BLOCK_RAW_MEAT, 3)]);
        assert!(!is_charring(&raw, 1500.0), "raw meat in the ingredients charred");
    }

    #[test]
    fn the_heat_colours_climb_in_order_and_start_where_terrafirmacraft_starts_them() {
        for pair in Glow::ALL.windows(2) {
            assert!(pair[0].starts_at() < pair[1].starts_at(), "{pair:?} out of order");
        }
        for glow in Glow::ALL.into_iter().skip(1) {
            assert_eq!(Glow::of(glow.starts_at()), glow);
        }
        assert_eq!(Glow::of(0.0), Glow::Cold);
        assert_eq!(Glow::of(f32::NAN), Glow::Cold);
        // The two lines the rest of the fire is drawn against.
        assert_eq!(Glow::of(CHAR_C), Glow::Orange, "food chars somewhere other than orange");
        assert_eq!(Glow::of(COOKING_C), Glow::Hot);
    }

    fn a_row_making(output: BlockId) -> &'static Recipe {
        RECIPES
            .iter()
            .find(|r| r.station.is_hearth() && block_kind(r.output.0) == output)
            .unwrap_or_else(|| panic!("no hearth row makes {output}"))
    }

    #[test]
    fn a_bloom_is_an_evening_and_supper_is_not() {
        // The order the batches take is the order the work took: a steak,
        // a burn of charcoal, a pot, a bloom. When every batch in a hearth
        // was one number, charcoal came off a campfire as fast as supper.
        use crate::types::{BLOCK_IRON_BLOOM, BLOCK_VESSEL};
        let steak = batch_seconds(a_row_making(BLOCK_COOKED_MEAT), Kind::Campfire);
        let charcoal = batch_seconds(a_row_making(BLOCK_COAL), Kind::Campfire);
        let pot = batch_seconds(a_row_making(BLOCK_VESSEL), Kind::Kiln);
        let bloom = batch_seconds(a_row_making(BLOCK_IRON_BLOOM), Kind::Bloomery);
        assert!(
            steak < charcoal && charcoal < pot && pot < bloom,
            "steak {steak}, charcoal {charcoal}, pot {pot}, bloom {bloom}"
        );
        // A bloom is most of a working day of the world, and never the whole
        // of one: a shaft that ran past nightfall would be a shaft a player
        // cannot tend around sleeping.
        const HOUR: f32 = 900.0 / 24.0;
        assert!(bloom >= 6.0 * HOUR, "a bloom is shorter than six hours: {bloom}");
        assert!(bloom < 900.0, "a bloom takes a whole day: {bloom}");
        // ...and supper did not move.
        assert_eq!(steak, Kind::Campfire.cook_seconds());
    }

    #[test]
    fn a_smelt_leaves_its_slag_in_the_tray_and_a_melt_leaves_none() {
        use crate::types::{BLOCK_COPPER_INGOT, BLOCK_COPPER_ORE, BLOCK_MOULD, BLOCK_NATIVE_COPPER, BLOCK_SLAG, BLOCK_VESSEL};
        let mut smelt = loaded(&[
            (0, BLOCK_COPPER_ORE, 3),
            (1, BLOCK_COAL, 2),
            (2, BLOCK_VESSEL, 1),
            (3, BLOCK_MOULD, 1),
        ]);
        let (_, recipe) = next_recipe(Kind::Campfire, &smelt).expect("a copper smelt");
        assert!(complete(&mut smelt, recipe));
        assert_eq!(smelt.count_within(OUTPUT_SLOTS, BLOCK_COPPER_INGOT), 1);
        assert_eq!(smelt.count_within(OUTPUT_SLOTS, BLOCK_SLAG), 1, "ore smelted clean");

        let mut melt = loaded(&[(0, BLOCK_NATIVE_COPPER, 2), (1, BLOCK_VESSEL, 1), (2, BLOCK_MOULD, 1)]);
        let (_, recipe) = next_recipe(Kind::Campfire, &melt).expect("a melt");
        assert!(complete(&mut melt, recipe));
        assert_eq!(melt.count_within(OUTPUT_SLOTS, BLOCK_SLAG), 0, "melting metal made slag");

        // A tray with room for the ingot and none for the slag does not
        // start: the slag is not thrown away to make the batch fit.
        let jammed = loaded(&[
            (0, BLOCK_COPPER_ORE, 3),
            (1, BLOCK_COAL, 2),
            (2, BLOCK_VESSEL, 1),
            (3, BLOCK_MOULD, 1),
            (5, BLOCK_STONE, crate::inventory::MAX_STACK),
            (6, BLOCK_COPPER_INGOT, 1),
        ]);
        assert!(next_recipe(Kind::Campfire, &jammed).is_none(), "it smelted with nowhere for the slag");
    }
}
