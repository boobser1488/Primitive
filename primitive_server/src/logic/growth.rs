//! What grows back, and what grows in a field.
//!
//! ## The one thing it started as
//!
//! A picked berry bush fills again. That was the whole mechanic when this
//! file was written, and it is the difference between a world a player
//! can live in and a world they strip: without it, food is a finite
//! resource lying on the ground, and the correct way to play is to walk
//! outwards forever.
//!
//! It was deliberately not farming then -- no seed, no tilled soil, no
//! crop stage and no hoe, because every one of those is a system and the
//! thing a player wanted from the first of them was "the bush I found
//! last week still has berries on it". The field came later and goes
//! through the same machine: what grows into what is `types::ripens_into`,
//! and a crop is one more row in it. What a crop adds on top is below.
//!
//! ## How it finds work, and why it is not a queue
//!
//! Picking a bush notifies this mechanic, so the common case costs one
//! push and one deadline. That alone would be enough on a server that
//! never restarts -- and every server restarts. A bush picked before a
//! shutdown would come back bare and stay bare forever, which is the
//! worst failure this could have: it is invisible, it is permanent, and
//! it makes the world quietly poorer every time the process stops.
//!
//! So there is a second source of work: **a sample of cells near the
//! players**, a few dozen a tick, chosen at random. Anything bare it
//! stumbles on gets a deadline like any other. That covers restarts,
//! worlds edited by hand, bushes placed by a plugin, and anything else
//! nobody thought of -- and it costs a fixed handful of block reads per
//! tick whatever the world is doing, which is the property the
//! `CellMechanic` budget exists to guarantee.
//!
//! The sample is the same trick every game of this shape uses for
//! growing things, and it is here for the reason it is there: work
//! proportional to *where the players are* rather than to how much world
//! has ever existed.
//!
//! ## What a crop asks of the place it is in
//!
//! A crop used to be the bush's clock with the soil on it, and that made
//! a field something you put wherever there was grass to till: nothing
//! about *where* decided anything except how fast. Three things decide it
//! now, and each is a question a farmer actually asks.
//!
//! - **Is there water?** A liquid cell within [`WATER_REACH`] blocks of
//!   the tilled earth, level with it or one below ([`watered`]). A dry
//!   field still grows, [`DRY_FIELD_FACTOR`] times slower: slow enough
//!   that a field beside a river is plainly better and a channel dug from
//!   one is worth an afternoon, not so slow that a field on a hill is a
//!   mistake rather than a trade.
//! - **Is it warm enough?** Every crop has an air temperature below which
//!   it does not grow at all ([`min_growing_c`]): wheat is hardy and
//!   cotton is not. Below it the clock *stops* rather than resetting, so
//!   a cold night costs a night and not the crop.
//! - **Is it freezing?** At [`FROST_C`] a crop that is still growing dies
//!   and leaves `types::BLOCK_WITHERED_CROP` standing in the furrow. That
//!   is what turns the season into a decision: sow in spring, reap before
//!   the autumn nights, and a field sown at the end of summer is a bet.
//!
//! The temperature is the server's own air (`climate::Ambient::of`,
//! through [`Soil::air_c`]) -- the number a player standing in the field
//! would be sampled at, with the biome, the altitude, the season, the
//! hour, the rain, a roof and any fire nearby already in it. Nothing here
//! computes weather, and two weather systems that disagreed where a
//! player could see both would be worse than either. It also means that a
//! fire kept burning beside a field on a frosty night keeps the frost off
//! it, which is what orchard growers did with smudge pots, and that a
//! roof over a seedbed keeps the night sky's chill off; both are things a
//! player can work out, and neither needed a line of its own.
//!
//! ## Why the weather is looked at rather than read once
//!
//! A deadline set once, the way the bush's is, cannot see a frost: the
//! coldest hour is the one before dawn, and a deadline that fell at noon
//! would sail straight through it. Asking every growing cell every tick
//! is the per-cell-per-tick shape this file exists to avoid. So a crop is
//! *looked at* every [`LOOK_SECONDS`], give or take a quarter. The
//! pre-dawn chill lasts a few real minutes at the default day length and a
//! look every fifteen seconds cannot miss it, while a field of two
//! hundred cells costs about a dozen looks a second. What a look decides
//! is how fast the crop grows until the next one -- its *pace* -- so a
//! channel that is filled in, or a warm spell that ends, is felt within
//! seconds rather than at the next stage.
//!
//! ## What the wild does on its own
//!
//! "сделай рост трав деревьев и прочего": a meadow a player had cut stayed
//! cut, a clearing stayed a clearing for as long as the world lasted, and
//! the only wild thing that ever came back was the berry on a bush. Three
//! things grow now, all found by the same sample and none of them on a
//! clock of their own, so what they cost is a roll per sampled cell:
//!
//! - **A plant sows the ground round it** ([`offspring`]): grass and flowers
//!   into the open, a fern and a bilberry into the shade, a tall plant as a
//!   shoot that grows into its two cells. Only onto its own ground, under the
//!   light it wants, in air warm enough ([`GREEN_UP_C`]), never thicker than
//!   a meadow grows ([`CROWD`]) and never beside anything a player built.
//! - **A tree drops a seedling into a gap** ([`SEEDLING_ONE_IN`]): rarely,
//!   under open sky, with room round it, and it goes on the young tree's
//!   clock like any sapling the world planted.
//! - **Bare earth greens over** where turf touches it in the open.
//!
//! What none of them may do is the rule the young tree already keeps: grow
//! into a player's things. They fill air and nothing else, and they refuse a
//! cell a build stands beside.

use std::collections::HashMap;

use primitive_shared::protocol::BlockChange;
use primitive_shared::types::{
    block_kind, branch_width, can_grow_on, has_full_top, is_branch, is_cross, is_flat, is_leafy,
    is_liquid, is_plant_shoot, is_plant_top, plant_partner, plant_shoot, ripens_into, BlockId, BLOCK_AIR,
    BLOCK_APPLE_LEAVES, BLOCK_APPLE_LEAVES_PICKED, BLOCK_BARE_BUSH, BLOCK_BASALT, BLOCK_BERRY_BUSH, BLOCK_BILBERRY,
    BLOCK_BILBERRY_BARE, BLOCK_BIRCH_LEAVES, BLOCK_BIRCH_LOG, BLOCK_BRACKEN, BLOCK_CATTAIL, BLOCK_CLAY,
    BLOCK_COBBLESTONE, BLOCK_COTTON_PLANT, BLOCK_COTTON_SEEDS, BLOCK_DIRT, BLOCK_DRY_GRASS, BLOCK_DRY_TURF,
    BLOCK_FARMLAND, BLOCK_FERN, BLOCK_FIREWEED, BLOCK_FLOWER, BLOCK_GRANITE, BLOCK_GRASS, BLOCK_GRAVEL,
    BLOCK_ICE, BLOCK_LEAVES, BLOCK_LIMESTONE, BLOCK_LOG, BLOCK_MAPLE_LEAVES, BLOCK_MUD, BLOCK_NEST,
    BLOCK_NETTLE, BLOCK_PEAT, BLOCK_PLANTAIN, BLOCK_SAND, BLOCK_SANDSTONE, BLOCK_SANDY_SOIL, BLOCK_SEEDS,
    BLOCK_SNOW, BLOCK_STONE, BLOCK_STRAWBERRY, BLOCK_STRAWBERRY_BARE, BLOCK_SUNDEW, BLOCK_TALL_GRASS,
    BLOCK_WHEAT, BLOCK_WITHERED_CROP, CHUNK_SIZE_Y,
};
use primitive_shared::worldgen::{stem_of, tree_stage_cells, young_tree_variant, GROWN_FOOT, TREE_STAGES};

use crate::logic::falling::BlockWorld;
use crate::logic::rng::Rng;

/// Where a growing thing is.
type Cell = (i32, i32, i32);

/// The ground a thing grows in and the air over it, asked of whatever
/// owns the world.
///
/// **A trait rather than a `&World`, and a second parameter rather than
/// a field**, for the reason `BlockWorld` is one: this mechanic is
/// handed a view of the world by its caller and owns nothing, so it can
/// be tested against a flat fixture with no generator in it at all. The
/// implementation the server passes reads `WorldGen::fertility_at`,
/// which is a pure function of the seed and the place -- see
/// `worldgen::Fertility` -- and `climate::Ambient::of` for the air.
///
/// **One trait for both, not a `Soil` and an `Air`.** They are asked by
/// the same caller, of the same world, in the same step, and every test
/// that wants one wants the other; two trait objects would be a second
/// parameter on every call for no second implementer. The water is not
/// on here at all, because it is blocks -- `BlockWorld` already answers
/// it, and a fixture tests it by putting water down.
pub trait Soil {
    /// The multiplier on how long a crop takes here: below one is good
    /// ground, above one is poor. See `Fertility::growth_factor`.
    fn growth_factor(&self, gx: i32, gz: i32) -> f32;

    /// The temperature of the air in this cell, in the degrees `body`
    /// uses, or `None` if nobody has it loaded.
    ///
    /// `None` holds a crop exactly where it is -- neither growing nor
    /// freezing -- because a field nobody has loaded is not having a
    /// mild night, and answering it with a guess would be a guess that
    /// kills a crop.
    fn air_c(&self, gx: i32, gy: i32, gz: i32) -> Option<f32>;
}

/// Ground that is the same everywhere, under air that is the same
/// everywhere. What the tests use, and what a caller that has no
/// generator can pass.
pub struct EvenSoil;

impl EvenSoil {
    /// The air over it: a mild day, warm enough for every crop there is
    /// and nowhere near a frost.
    pub const AIR_C: f32 = 20.0;
}

impl Soil for EvenSoil {
    fn growth_factor(&self, _gx: i32, _gz: i32) -> f32 {
        1.0
    }

    fn air_c(&self, _gx: i32, _gy: i32, _gz: i32) -> Option<f32> {
        Some(Self::AIR_C)
    }
}

/// How long a picked bush takes to come back, in seconds.
///
/// Three in-game days at the default clock, give or take the jitter
/// below. The number is chosen against what a player *does*: an evening
/// of mining is about two thirds of it, so a bush picked on the way out
/// is *not* worth anything on the way back -- and a player who stands
/// over one waiting is wasting an evening, which is the right answer to
/// standing over one.
///
/// **Half again on what it was**, and the half is the whole point. At
/// two days a camp beside four bushes was a larder that refilled itself
/// faster than one person could empty it, so the answer to hunger was
/// to *stay where the bushes are* -- which is the opposite of what
/// foraging is meant to do. At three it is worth walking on: the food
/// you have not eaten is over the hill, and the food that keeps is the
/// food you cooked. See `worldgen::Biome::berry_spacing`, which was
/// thinned in the same pass and for the same reason.
pub const REGROW_SECONDS: f32 = 1800.0;

/// How long a crop spends in each of its stages, in seconds -- on
/// watered ground, in air warm enough for it.
///
/// Nine hundred a stage, so seed to harvest is half an hour of play --
/// about two game days at the default day length. Deliberately longer
/// than one evening and shorter than a session: a field you planted has
/// to be a reason to come back, and one that is not ready by the time
/// you have built the house around it is a field nobody plants twice.
///
/// Those are the *best* half hour. A dry field, a cool night and thin
/// soil each stretch it, and that stretch is where the decisions are.
pub const CROP_STAGE_SECONDS: f32 = 900.0;

/// How long a tired furrow has to lie bare to be rested, in seconds.
///
/// **One crop's time on good ground** -- two stages, half an hour. So two
/// fields sown in turn, one growing while the other rests, give a harvest
/// every half hour at full pace; one field cropped without a break gives
/// one every forty-five minutes (`wildfire::TIRED_FIELD_FACTOR`); and ash
/// skips the wait. The three-field rotation of the Middle Ages rested a
/// third of the land a year for the same arithmetic. Shorter, and resting
/// is free and nobody feels the tiredness; much longer, and a second field
/// is the only answer, which is not a decision.
pub const FALLOW_SECONDS: f32 = CROP_STAGE_SECONDS * 2.0;

/// How long a young tree spends in each of its shapes before it grows into
/// the next, in seconds -- see `worldgen::tree_stage_cells`.
///
/// **Forty minutes a stage, so a sapling is a grown tree in two hours.**
/// Long against a crop's quarter hour, because a tree is a thing a player
/// plants or finds and comes *back* to, not a thing that grows while they
/// watch: fast enough that a clearing cut beside a camp fills in again in an
/// evening of play, slow enough that the first saplings of a world are still
/// saplings when a player has made an axe. On the bush's flat clock, not a
/// crop's -- no water, no warmth, no soil -- for the reason the bush gives:
/// it is the world's own growth, and a rule for hurrying it would be a rule
/// with nothing to decide about.
pub const TREE_STAGE_SECONDS: f32 = 2400.0;

/// How much longer a stage takes in a field with no water near it.
///
/// Two and a half times, chosen against the decision it has to create.
/// A watered field is ripe in two game days; a dry one takes five, and a
/// season is three -- so a dry field sown in spring comes ripe in the
/// autumn frosts if it comes ripe at all, and the river is worth walking
/// to. Half again was the first number considered and it is a rounding
/// error nobody plans around. Five times would make a dry field
/// pointless, and a field on high ground a mistake rather than a trade.
pub const DRY_FIELD_FACTOR: f32 = 2.5;

/// How far water reaches into a field, in blocks, sideways.
///
/// **Four, and counted as a square rather than a circle**, because the
/// square is what a player can count on the ground: four cells along,
/// four across, diagonals included. A circle of four would leave the
/// corners of a nine-by-nine field dry for a reason nobody could see.
/// Four means a channel down the middle of a field nine wide waters it to
/// both edges, which is the field a person would actually lay out.
pub const WATER_REACH: i32 = 4;

/// The air temperature at and below which a growing crop dies.
///
/// **Two below zero, not zero.** A light frost on a still night bruises
/// and does not kill, and a threshold at exactly freezing would make
/// every spring dawn in the starting meadow -- which sits within a degree
/// or two of zero before sunrise (`climate::PRE_DAWN_CHILL_C`) -- a coin
/// toss on the whole field. Two below is a night a player can see
/// coming: autumn in the meadow, any night in the hills, all of winter.
pub const FROST_C: f32 = -2.0;

/// How often a growing crop's water and air are looked at, in seconds.
///
/// See the module note for why there is a look at all. Fifteen, jittered
/// by a quarter so a field sown in one sweep does not do all its reads in
/// one tick.
pub const LOOK_SECONDS: f32 = 15.0;

/// The air a crop needs to grow at all, in degrees -- or `None` if this
/// is not a stage of a planted crop.
///
/// Real base temperatures, because the real ones already make the
/// decision this needs: wheat is a crop of cool country and cotton of
/// hot, and the gap between them is the whole reason cotton sends a
/// player south.
pub fn min_growing_c(block: BlockId) -> Option<f32> {
    match block_kind(block) {
        // Wheat is a cool-season grass. It comes up a few degrees above
        // freezing and grows through a chilly spring, which is why it is
        // the crop of the temperate meadow and the first a player can
        // live on -- it stops on a cold night and carries on at dawn.
        BLOCK_SEEDS | BLOCK_WHEAT => Some(4.0),
        // Cotton is a warm-country shrub and does nothing at all in a
        // cool night; sixteen is where it starts. In the starting meadow
        // that is the middle of a summer day and nothing else, so a
        // cotton field there crawls. In the savanna it is most of every
        // day.
        BLOCK_COTTON_SEEDS | BLOCK_COTTON_PLANT => Some(16.0),
        // Millet is a hot-country grass: fourteen, a little under cotton,
        // because it is grown where the nights are cooler than the cotton's
        // too -- the savanna's edge and the steppe. What it buys for the
        // warmth it asks is speed (`ripening_seconds`). See
        // `types::BLOCK_WILD_MILLET`.
        primitive_shared::types::BLOCK_MILLET | primitive_shared::types::BLOCK_MILLET_PLANT => Some(14.0),
        _ => None,
    }
}

/// The air a picked apple tree needs before it sets fruit again, in
/// degrees.
///
/// **Ten, the base a temperate fruit tree's growing season is usually
/// counted from**, and what it does to the year is the decision this is
/// for. A warm day counts and a cold night does not; spring and autumn
/// count their afternoons; a winter counts next to nothing, so a tree
/// picked bare before the cold has little on it until the spring, and what
/// a player gathered in the autumn is what they eat in the winter. The
/// hills, colder at every hour, are slower all year.
///
/// **Cold stops the clock; frost does not kill.** A crop in a frost
/// withers (`FROST_C`), because a crop is the season's one chance. A tree
/// that has been picked has nothing on it to lose, and one killed by every
/// winter would be a tree nobody walks back to in the spring.
///
/// The flat clock was the other choice, and the bush had it until the
/// winter was made a hunter's season (`frost_takes_the_berries`); an orchard
/// that filled at the same rate through a blizzard would make the one tree a
/// player remembers better than the fire they cooked on.
pub const FRUIT_SET_C: f32 = 10.0;

/// The air this block needs to set fruit, or `None` if it is not a tree
/// waiting to. See `FRUIT_SET_C`.
///
/// Not `min_growing_c`, deliberately: that table is what makes a thing a
/// *crop*, and a crop is watered, fertilised and frozen. None of the three
/// is true of a leaf in a canopy -- a picked apple leaf that a frost
/// "withered" would leave dead stalks hanging in a tree.
pub fn fruit_sets_above_c(block: BlockId) -> Option<f32> {
    if block == primitive_shared::types::BLOCK_PALM_FRONDS_PICKED {
        return Some(COCONUT_SET_C);
    }
    // **A picked bilberry and a picked wild strawberry fruit on the apple's
    // terms**: in warm air and not through a winter, and a frost does not
    // kill a shrub with nothing on it. What a forest floor gives in August is
    // what it gives, and a player who strips it in autumn waits for spring.
    //
    // **The bush too, now**, and it used to be the exception -- a flat
    // clock, "the thin larder that is everywhere". A flat clock fruits a
    // bush in the middle of a winter the frost has just stripped it for
    // (`frost_takes_the_berries`), and the player asked for no berries in
    // winter at all.
    if matches!(block_kind(block), BLOCK_BILBERRY_BARE | BLOCK_STRAWBERRY_BARE | BLOCK_BARE_BUSH) {
        return Some(FRUIT_SET_C);
    }
    // ...and a tall plant's shoot, which is not fruit and wants the same
    // kind of wait: it comes up in a spring and not in a snowfall. The
    // lower line is the one a wild plant greens at (`GREEN_UP_C`).
    if is_plant_shoot(block) {
        return Some(GREEN_UP_C);
    }
    // ...and a hive filling, only while its bees fly: they bring the nectar,
    // and a bee does not fly in the cold (`bees::BEES_FLY_C`). The same line
    // that says whether a raid stings, so a hive that is safe to rob is a
    // hive that is not filling.
    if primitive_shared::bees::is_hive(block) && ripens_into(block).is_some() {
        return Some(primitive_shared::bees::BEES_FLY_C);
    }
    (block == BLOCK_APPLE_LEAVES_PICKED).then_some(FRUIT_SET_C)
}

/// The air a wild plant needs before it spreads, or a shoot grows, in degrees.
///
/// **Five, the base temperature grassland growth is counted from**, and the
/// line a meadow greens at in spring. Below it nothing is sown: a clearing a
/// player cut in December is still bare in January, and the grass comes back
/// with the warm weeks -- which is what grass does.
pub const GREEN_UP_C: f32 = 5.0;

/// One sampled plant in how many is asked to sow a cell beside it.
///
/// **Forty**, worked out from what a player should see. The sample visits
/// two dozen cells a tick round a player (`SAMPLE_PER_TICK`), twenty ticks a
/// second at the default `tick_rate_hz`, in a box of about fifteen thousand cells of which a meadow's plants
/// are a few hundred: some twenty plants a second are sampled, and at forty
/// half a sowing a second is tried -- most refused by the crowd, the ground or
/// the roll landing on a plant. A cut patch a few strides across has only its
/// rim to be sown from, a few dozen of those plants, and fills in over an
/// hour or so of play beside it. Four would fill it while a player stood and
/// watched, which reads as the grass undoing the scythe; four hundred would
/// leave it bare for a session, which reads as grass that does not grow.
const SPREAD_ONE_IN: f32 = 40.0;

/// One sampled leaf in a wood in how many drops a seedling.
///
/// **Rare, because a tree is the slow thing.** A wood's canopy is most of
/// what the sample touches there, so at three thousand a seedling is tried
/// about once a couple of minutes near a player, and nearly always refused:
/// it needs a gap in the crowns overhead and room for a tree round it. What a
/// player notices is that the clearing they cut beside camp has a sapling or
/// two in it after a few evenings -- and grows back as a wood after that,
/// on the young tree's clock (`TREE_STAGE_SECONDS`).
const SEEDLING_ONE_IN: f32 = 3000.0;

/// One sampled cell of bare earth in how many greens over, if it may.
const TURF_ONE_IN: f32 = 40.0;

/// How far a plant sows, in cells, either way.
///
/// Two: a patch creeps outward a stride at a time, rather than seeding the
/// far side of a clearing from the near one.
const SPREAD_REACH: i32 = 2;

/// How many plants any five-by-five patch of ground may hold.
///
/// **Seven of twenty-five, which is under a third** -- about as thick as the
/// generator grows a meadow (`worldgen::Biome::grass_spacing`, a tuft in three
/// columns on a plain). A meadow that only ever thickened would be a lawn of
/// tufts in a few hours, a field a player could no longer see the ground of,
/// and the flowers and the plantain in it crowded out by grass.
///
/// **Any patch, not the patch round the cell being sown.** The first version
/// counted only the five-by-five centred on the new plant, and that bounds
/// nothing: every sowing into a patch with room pushes the patches beside it
/// past seven, and a cut lawn sown over and over came out more than half
/// tufts (`a_cut_meadow_fills_in_over_an_evening_and_stops_short_of_a_lawn`).
/// So a plant goes in only where every patch it would fall into has room
/// (`crowded`).
const CROWD: u8 = 7;

/// How many of one kind may stand within `KIN_REACH` of a new one, for the
/// plants that are worth something -- the berries, the plantain, the sundew
/// and the tall plants.
///
/// **Three**: a stand, not a field. A bilberry that carpeted the floor round
/// the first one a player found would be the berry bush's hedgerow economy
/// again, the thing `worldgen::Biome::berry_spacing` was thinned twice to be
/// rid of.
const KIN: usize = 3;

/// How far `KIN` is counted, in columns: **four, twice what a plant sows
/// across**, so every cell a plant can sow into sees the plant itself and
/// every sibling it has already sown. Counted over the sowing reach alone, a
/// strawberry sowing two columns east did not see the one it had sown two
/// columns west, and one plant grew a patch.
const KIN_REACH: i32 = 2 * SPREAD_REACH;

/// The air a picked palm needs before it sets coconuts again, in degrees.
///
/// **Twenty, not the apple's ten**, because a coconut palm is a tropical
/// tree and fruits all year only where the year is warm. In the tropics
/// that is nearly every hour (the season barely reaches there, see
/// `season::seasonal_swing`); on a warm pocket of a temperate coast a palm
/// picked bare in autumn is bare until summer -- which is the difference
/// between a coast to live on and a coast to visit.
pub const COCONUT_SET_C: f32 = 20.0;

/// How long this block takes to become the next thing, if it becomes
/// anything.
///
/// The one place the growing things differ, and they differ only in the
/// number: *what* each turns into is `types::ripens_into`, which both
/// sides of the socket can read.
fn ripening_seconds(block: BlockId) -> Option<f32> {
    ripens_into(block)?;
    Some(match block_kind(block) {
        BLOCK_BARE_BUSH => REGROW_SECONDS,
        // The bush's clock -- but counted only in warm air, so in the
        // year it is slower than the bush and stops for the winter. See
        // `FRUIT_SET_C`. `ripens_into` has already said this is a picked
        // leaf and not the canopy round it.
        BLOCK_APPLE_LEAVES => REGROW_SECONDS,
        // ...and a picked palm, on the same clock in warmer air.
        primitive_shared::types::BLOCK_PALM_FRONDS => REGROW_SECONDS,
        // **A robbed nest too**, which `ripens_into` has always said and
        // this never did: the nest fell through to the crop's fifteen
        // minutes and refilled twice as fast as the comment beside it
        // promised. With nests now three times rarer (`worldgen`'s
        // `NEST_SPACING`), a nest you can empty every quarter of an hour
        // is still a henhouse -- just a smaller one.
        BLOCK_NEST => REGROW_SECONDS,
        // ...and a raided hive, a comb at a time on the bush's clock in warm
        // air (`fruit_sets_above_c`): a hive stripped to the comb is full
        // again in three of the bush's waits, which is most of a season --
        // what the bees are paid for being robbed. See `bees`.
        primitive_shared::types::BLOCK_WILD_HIVE => REGROW_SECONDS,
        // ...and the two berries of the forest floor, and a tall plant's
        // shoot: the world's own growth, on the world's clock, counted in
        // warm air (`fruit_sets_above_c`).
        BLOCK_BILBERRY_BARE | BLOCK_STRAWBERRY_BARE => REGROW_SECONDS,
        BLOCK_FIREWEED | BLOCK_CATTAIL | BLOCK_NETTLE | BLOCK_BRACKEN => REGROW_SECONDS,
        primitive_shared::types::BLOCK_ARUNDO => REGROW_SECONDS,
        // **Millet ripens in six tenths of wheat's time**, which is the
        // other half of its bargain: it wants warm air (`min_growing_c`),
        // and in warm air it is the quick crop. Not half -- a grain that
        // ripened twice as fast as wheat for three to a head would out-feed
        // bread even with porridge's smaller bowl.
        primitive_shared::types::BLOCK_MILLET | primitive_shared::types::BLOCK_MILLET_PLANT => CROP_STAGE_SECONDS * 0.6,
        _ => CROP_STAGE_SECONDS,
    })
}

/// Is this one of the stages of a planted crop?
///
/// The question fertility, water and frost all ask, and it is asked of
/// the *stage* rather than of a list of crops. **It is the warmth table,
/// deliberately**: a crop that could be planted without saying what air
/// it needs would be a crop that grows through a blizzard, and the test
/// `every_stage_of_a_sown_crop_says_what_warmth_it_needs` holds the table
/// to every block that stands in a field and ripens.
fn is_crop(block: BlockId) -> bool {
    min_growing_c(block).is_some()
}

/// Is there water close enough to this tilled earth to keep it wet?
///
/// `gy` is the level of the earth itself, not of the crop on it. Water
/// counts at that level -- a channel dug beside the field and filled --
/// or one below it, which is a river whose surface sits a step under its
/// bank. Two below is a well, not a field's water.
///
/// Ice is not liquid, so a frozen channel waters nothing; that costs no
/// rule of its own, because nothing grows in air cold enough to freeze
/// one.
///
/// Public because the hoe asks it too: the moment earth is turned is the
/// moment a farmer learns whether it is wet (`lib.rs`, `field_note`).
pub fn watered(world: &dyn BlockWorld, gx: i32, gy: i32, gz: i32) -> bool {
    (gy - 1..=gy).any(|y| {
        (-WATER_REACH..=WATER_REACH).any(|dx| {
            (-WATER_REACH..=WATER_REACH)
                .any(|dz| world.block(gx + dx, y, gz + dz).is_some_and(is_liquid))
        })
    })
}

/// ...and how much of that is random, as a fraction.
///
/// A quarter, so a hedgerow picked in one sweep does not come back in
/// one instant. Bushes ripening in lockstep is the sort of thing that
/// makes a world read as a spreadsheet.
const REGROW_JITTER: f32 = 0.25;

/// How many cells the random sample looks at per tick, at most.
///
/// Independent of the budget the queue gets, because the two are
/// different kinds of work: the queue is bounded by what players did and
/// the sample is bounded by nothing at all. A few dozen block reads a
/// tick is nothing next to the falling-sand simulation next to it.
const SAMPLE_PER_TICK: usize = 24;

/// How far from a player the sample reaches, in blocks.
///
/// About a chunk and a half. Far enough that bushes come back before you
/// walk back to them, near enough that the reads land in chunks that are
/// certainly loaded.
const SAMPLE_RADIUS: f32 = 20.0;

/// One thing on the clock.
#[derive(Debug, Clone, Copy)]
struct Ripening {
    /// Seconds of growing left, at full pace.
    left: f32,
    /// How many seconds of growing each second is worth, as of the last
    /// look: one for a watered crop in warm air, a fraction for a dry
    /// one, nothing in air too cold for it. Always one for what the world
    /// planted itself.
    pace: f32,
    /// Seconds until the next look. Infinite for a bush or a nest, which
    /// fill on the flat clock whatever the weather -- see
    /// `the_ground_does_not_hurry_what_the_world_planted_itself`.
    look_in: f32,
}

/// What a look at a growing crop found.
#[derive(Debug, Clone, Copy, PartialEq)]
enum Look {
    /// Growing, at this pace until the next look.
    Growing(f32),
    /// Killed by frost where it stands.
    Frozen,
    /// Nothing to look after any more: harvested, dug up, built over, or
    /// in a chunk nobody has loaded. The sample finds it again if it
    /// comes back.
    Gone,
}

/// Looks at one growing crop: is it still there, is it frozen, and how
/// fast is it growing.
fn look(world: &dyn BlockWorld, soil: &dyn Soil, at: Cell) -> Look {
    let Some(here) = world.block(at.0, at.1, at.2) else {
        return Look::Gone;
    };
    if ripens_into(here).is_none() {
        return Look::Gone;
    }
    // **A picked apple tree asks the air and nothing else** -- no water,
    // no soil, no frost. Asked before the crop's questions, although the
    // crop table would answer "not a crop" anyway, so that nobody adds a
    // warmth row for it there and has a frost wither a canopy.
    if let Some(needs) = fruit_sets_above_c(here) {
        let warm = soil
            .air_c(at.0, at.1, at.2)
            .is_some_and(|air| air.is_finite() && air >= needs);
        return Look::Growing(if warm { 1.0 } else { 0.0 });
    }
    let Some(needs) = min_growing_c(here) else {
        return Look::Growing(1.0);
    };
    let Some(air) = soil.air_c(at.0, at.1, at.2).filter(|c| c.is_finite()) else {
        return Look::Growing(0.0);
    };
    // Frost first: air below the frost line is also below every crop's
    // minimum, and a crop in it has to die rather than merely wait.
    if air <= FROST_C {
        return Look::Frozen;
    }
    if air < needs {
        return Look::Growing(0.0);
    }
    if watered(world, at.0, at.1 - 1, at.2) {
        Look::Growing(1.0)
    } else {
        Look::Growing(1.0 / DRY_FIELD_FACTOR)
    }
}

/// What is growing, and where to look for more.
pub struct Growth {
    /// Cell -> what is left of its growing, and how fast it is going.
    ripening: HashMap<Cell, Ripening>,
    /// Cells that changed and have not been looked at.
    pending: Vec<Cell>,
    /// Where the players are, as of the last tick. The sample walks out
    /// from these. Written once a tick by the caller rather than read
    /// from the registry, because a `CellMechanic` is handed a world and
    /// nothing else -- and giving it the player registry would make
    /// every mechanic able to reach every player.
    watching: Vec<(f32, f32, f32)>,
    /// Root -> seconds until the young tree standing on it grows its next
    /// shape. Its own map rather than a row in `ripening`, because growing
    /// is not one block becoming another: it is a tree rebuilt, and none of
    /// the crop's looks apply.
    young: HashMap<Cell, f32>,
    /// Tired furrow -> seconds of lying bare it still needs before it is
    /// rested ([`FALLOW_SECONDS`]). Its own map for the young tree's reason:
    /// nothing grows here, the ground just gets better, and none of a
    /// crop's looks apply.
    ///
    /// Not saved, and it need not be: the sample finds a tired furrow again
    /// after a restart and starts it over, which costs a player at most one
    /// fallow's wait -- and the flag itself is in the block, which is saved.
    resting: HashMap<Cell, f32>,
    rng: Rng,
}

impl Default for Growth {
    fn default() -> Self {
        Self::new()
    }
}

impl Growth {
    pub fn new() -> Self {
        Self {
            ripening: HashMap::new(),
            pending: Vec::new(),
            watching: Vec::new(),
            young: HashMap::new(),
            resting: HashMap::new(),
            rng: Rng::from_clock(),
        }
    }

    /// The same, with a stated seed. Tests want a world where the same
    /// bush ripens at the same moment twice.
    pub fn seeded(seed: u64) -> Self {
        Self {
            rng: Rng::seeded(seed),
            ..Self::new()
        }
    }

    /// How many things are waiting to grow. Reported in `/stats`.
    pub fn pending(&self) -> usize {
        self.ripening.len() + self.pending.len()
    }

    pub fn ripening(&self) -> usize {
        self.ripening.len()
    }

    /// Where to sample from this tick.
    pub fn watch(&mut self, points: Vec<(f32, f32, f32)>) {
        self.watching = points;
    }

    /// A cell changed; look at it next step.
    pub fn on_block_changed(&mut self, gx: i32, gy: i32, gz: i32) {
        self.pending.push((gx, gy, gz));
    }

    /// One tick: adopt what changed, sample for what was missed, look at
    /// the crops that are due a look, and grow whatever is ready.
    pub fn step(
        &mut self,
        world: &dyn BlockWorld,
        soil: &dyn Soil,
        dt: f32,
        budget: usize,
    ) -> Vec<BlockChange> {
        // `drain(..).take(budget)` looks like the same thing and is
        // not: the drain removes *everything* whatever the take
        // consumes, so a budget of sixty-four silently threw away five
        // thousand queued cells. Draining exactly the prefix is the
        // whole of the fix.
        let batch: Vec<Cell> = self
            .pending
            .drain(..budget.min(self.pending.len()))
            .collect();
        for at in batch {
            self.consider(world, soil, at);
        }
        let sown = self.sample(world, soil);

        let mut grown = self.grow_young_trees(world, dt);
        grown.extend(self.rest_furrows(world, dt));
        grown.extend(sown);
        if self.ripening.is_empty() {
            return grown;
        }

        let mut ready = Vec::new();
        let mut frozen = Vec::new();
        // The generator is borrowed apart from the map so the closure can
        // jitter the next look while `retain` holds the map.
        let rng = &mut self.rng;
        self.ripening.retain(|&at, growing| {
            growing.look_in -= dt;
            if growing.look_in <= 0.0 {
                match look(world, soil, at) {
                    Look::Growing(pace) => growing.pace = pace,
                    Look::Frozen => {
                        frozen.push(at);
                        return false;
                    }
                    Look::Gone => return false,
                }
                growing.look_in =
                    LOOK_SECONDS * rng.range(1.0 - REGROW_JITTER, 1.0 + REGROW_JITTER);
            }
            growing.left -= dt * growing.pace;
            if growing.left > 0.0 {
                return true;
            }
            ready.push(at);
            false
        });

        let mut changes: Vec<BlockChange> = frozen
            .into_iter()
            .filter_map(|(x, y, z)| {
                // Still in its furrow. A frozen crop whose field was dug
                // away in the same instant is not worth dead stalks
                // hanging in the air; the collapse pass has it.
                let under = world.block(x, y - 1, z)?;
                if !can_grow_on(BLOCK_WITHERED_CROP, under) {
                    return None;
                }
                world.set(x, y, z, BLOCK_WITHERED_CROP);
                Some(BlockChange {
                    global_x: x,
                    global_y: y,
                    global_z: z,
                    block_id: BLOCK_WITHERED_CROP,
                })
            })
            .collect();

        for (x, y, z) in ready {
            // Still the thing that was growing, and still standing on
            // something that can hold what it grows into. Between the
            // deadline being set and now, a player may have dug the
            // ground out from under it or built a wall through it, and
            // writing a bush over whatever is there instead would be
            // worse than losing the bush.
            let Some(here) = world.block(x, y, z) else { continue };
            let Some(grown) = ripens_into(here) else { continue };
            let Some(under) = world.block(x, y - 1, z) else { continue };
            if !can_grow_on(grown, under) {
                continue;
            }
            // **A shoot grows into two cells, and only into air.** The upper
            // half goes into the cell over it (`types::plant_partner`), and a
            // shoot under a roof, a branch or a player's shelf stays a shoot
            // rather than growing through it -- the young tree's rule. It
            // comes off the clock, and the sample finds it again and gives it
            // another wait; a shoot that asked every tick would be a queue
            // that never empties under every overhang in the world.
            let top = plant_partner((x, y, z), grown);
            if let Some(((tx, ty, tz), _)) = top {
                if world.block(tx, ty, tz) != Some(BLOCK_AIR) {
                    continue;
                }
            }
            world.set(x, y, z, grown);
            changes.push(BlockChange {
                global_x: x,
                global_y: y,
                global_z: z,
                block_id: grown,
            });
            // **The ash is spent on the harvest it fed.** A crop come ripe --
            // one that grows into nothing further -- takes the dressing out of
            // its furrow, so the next sowing wants ash again. See
            // `wildfire::ASH_DRESSING_FACTOR`.
            //
            // **...and an undressed furrow is left tired by it**
            // (`wildfire::after_harvest`): the next crop there is slower
            // until the ground has lain bare a while or had ash dug in. See
            // `wildfire::TIRED_FIELD_FACTOR`.
            let spent = (is_crop(here) && ripens_into(grown).is_none())
                .then(|| primitive_shared::wildfire::after_harvest(under))
                .flatten()
                .filter(|&spent| spent != under);
            if let Some(spent) = spent {
                world.set(x, y - 1, z, spent);
                changes.push(BlockChange {
                    global_x: x,
                    global_y: y - 1,
                    global_z: z,
                    block_id: spent,
                });
            }
            if let Some(((tx, ty, tz), top)) = top {
                world.set(tx, ty, tz, top);
                changes.push(BlockChange {
                    global_x: tx,
                    global_y: ty,
                    global_z: tz,
                    block_id: top,
                });
            }
        }
        // **What grew is looked at again, because nothing else will.** The
        // write above goes to the world directly and no edit path hears of
        // it, so a seed that came up used to wait for a player to wander
        // near enough for `sample` to find its second stage. Queued rather
        // than put on the clock here, so `consider` decides it on the same
        // terms as everything else: a harvest at the end of the line is
        // looked at once and dropped.
        self.pending.extend(
            changes
                .iter()
                .map(|change| (change.global_x, change.global_y, change.global_z)),
        );
        grown.extend(changes);
        grown
    }

    /// Counts down every young tree, and grows the ones whose time is up.
    /// A tree that is still young afterwards goes back on the clock; one
    /// that could not grow -- no room, or not loaded -- tries again after
    /// another stage's wait rather than every tick.
    /// Runs the fallow clock: a tired furrow that has lain bare for
    /// [`FALLOW_SECONDS`] is rested. One that was sown in the meantime, dug
    /// up, dressed or otherwise stopped being a bare tired furrow comes off
    /// the clock without a change -- the sample will find it again if it
    /// is ever bare and tired once more.
    fn rest_furrows(&mut self, world: &dyn BlockWorld, dt: f32) -> Vec<BlockChange> {
        let mut due = Vec::new();
        self.resting.retain(|&at, left| {
            *left -= dt;
            if *left > 0.0 {
                return true;
            }
            due.push(at);
            false
        });
        due.into_iter()
            .filter_map(|(x, y, z)| {
                let here = world.block(x, y, z)?;
                if !primitive_shared::wildfire::is_tired(here) || world.block(x, y + 1, z) != Some(BLOCK_AIR) {
                    return None;
                }
                let rested = primitive_shared::wildfire::rested(here);
                world.set(x, y, z, rested);
                Some(BlockChange {
                    global_x: x,
                    global_y: y,
                    global_z: z,
                    block_id: rested,
                })
            })
            .collect()
    }

    fn grow_young_trees(&mut self, world: &dyn BlockWorld, dt: f32) -> Vec<BlockChange> {
        let mut ready = Vec::new();
        self.young.retain(|&at, left| {
            *left -= dt;
            if *left > 0.0 {
                return true;
            }
            ready.push(at);
            false
        });
        let mut changes = Vec::new();
        for at in ready {
            changes.extend(grow_young_tree(world, at));
            if young_root(world, at) {
                let jitter = self.rng.range(1.0 - REGROW_JITTER, 1.0 + REGROW_JITTER);
                self.young.insert(at, TREE_STAGE_SECONDS * jitter);
            }
        }
        changes
    }

    /// Puts a cell on the clock if it grows and is not on it already.
    fn consider(&mut self, world: &dyn BlockWorld, soil: &dyn Soil, at: Cell) {
        if self.ripening.contains_key(&at) {
            return;
        }
        // A cell nobody has loaded says nothing. Dropping it rather than
        // re-queueing is deliberate: the sample will find it again once
        // somebody is standing near enough for it to matter, and a queue
        // that retries unloaded cells forever is a queue that never
        // empties.
        let Some(block) = world.block(at.0, at.1, at.2) else {
            return;
        };
        // A young tree's root goes on its own clock. Asked first and by the
        // blocks round it, because the same twig high in a crown is only
        // wood.
        if is_branch(block) && young_root(world, at) {
            if !self.young.contains_key(&at) {
                let jitter = self.rng.range(1.0 - REGROW_JITTER, 1.0 + REGROW_JITTER);
                self.young.insert(at, TREE_STAGE_SECONDS * jitter);
            }
            return;
        }
        // A tired furrow with nothing on it goes on the fallow clock. Bare
        // means air: a crop sown into it is the furrow being worked again,
        // and a furrow that rested under wheat would not be resting.
        //
        // **An empty cell asks about the furrow under it**, because that is
        // the cell a harvest changes: a reaped crop is an edit to the cell
        // over the ground, and without this the fallow would start whenever
        // the sample next wandered past rather than at the harvest. Asked
        // here rather than by queueing the cell below on every edit, which
        // doubled the queue for the one edit in thousands that is a reaping.
        let furrow = if block == BLOCK_AIR { (at.0, at.1 - 1, at.2) } else { at };
        let ground = if furrow == at { Some(block) } else { world.block(furrow.0, furrow.1, furrow.2) };
        if ground.is_some_and(primitive_shared::wildfire::is_tired) {
            if !self.resting.contains_key(&furrow)
                && world.block(furrow.0, furrow.1 + 1, furrow.2) == Some(BLOCK_AIR)
            {
                let jitter = self.rng.range(1.0 - REGROW_JITTER, 1.0 + REGROW_JITTER);
                self.resting.insert(furrow, FALLOW_SECONDS * jitter);
            }
            return;
        }
        // **Whatever grows, not whichever bush.** The table lives in
        // `types::ripens_into`, so a second growing thing -- the crop --
        // is a row there rather than a third copy of this test.
        let Some(seconds) = ripening_seconds(block) else {
            return;
        };
        let jitter = self.rng.range(1.0 - REGROW_JITTER, 1.0 + REGROW_JITTER);
        let crop = is_crop(block);
        // A picked apple tree is *looked at* like a crop, because its pace
        // is the air's (`look`), and *planted* like a bush, because the
        // ground under a canopy does not hurry it -- see the soil below.
        let looked_after = crop || fruit_sets_above_c(block).is_some();
        // **The ground the thing is standing in.** A crop takes two
        // thirds of the time on river silt and half again as long on
        // thin dry ground -- see `worldgen::Fertility`. Applied when
        // the deadline is *set*, so a player cannot speed a field up by
        // carrying soil to it after planting, and so the cost is one
        // lookup per cell that starts growing rather than one per tick.
        //
        // Only what a player planted. A bush and a nest are on the flat
        // clock: they are the world's own larder, and making the wild
        // ones fill faster in a good valley would be a rule with no way
        // to notice it and nothing to decide about it.
        //
        // The water and the air are *not* read here, unlike the soil,
        // because unlike the soil they change under a field that is
        // already growing -- see `look`.
        let soil_factor = if crop {
            // ...and whether ash was dug into it, read when the deadline is
            // set for the soil's reason. See `wildfire::ASH_DRESSING_FACTOR`.
            let dressed = world
                .block(at.0, at.1 - 1, at.2)
                .is_some_and(primitive_shared::wildfire::is_dressed);
            let ash = if dressed { primitive_shared::wildfire::ASH_DRESSING_FACTOR } else { 1.0 };
            // ...and whether the last crop tired it. Read at sowing for the
            // same reason: resting a furrow under a crop already in it would
            // be resting nothing.
            let tired = world
                .block(at.0, at.1 - 1, at.2)
                .is_some_and(primitive_shared::wildfire::is_tired);
            let worn = if tired { primitive_shared::wildfire::TIRED_FIELD_FACTOR } else { 1.0 };
            soil.growth_factor(at.0, at.2) * ash * worn
        } else {
            1.0
        };
        self.ripening.insert(
            at,
            Ripening {
                left: seconds * jitter * soil_factor,
                // A crop is looked at before it grows at all: its first
                // look is due now, in this same step, so a seed sown
                // into a frost withers the tick it is noticed instead of
                // growing for fifteen seconds first.
                pace: if looked_after { 0.0 } else { 1.0 },
                look_in: if looked_after { 0.0 } else { f32::INFINITY },
            },
        );
    }

    /// Looks at a handful of random cells near the players.
    ///
    /// The safety net described at the top of the file. Cells are picked
    /// in a cube around a player rather than on the surface, because
    /// "the surface" is a question this mechanic would have to ask the
    /// world generator, and a wrong answer to it is a sample that never
    /// finds anything on a hillside.
    fn sample(&mut self, world: &dyn BlockWorld, soil: &dyn Soil) -> Vec<BlockChange> {
        let mut sown = Vec::new();
        if self.watching.is_empty() {
            return sown;
        }
        for _ in 0..SAMPLE_PER_TICK {
            let Some(&(px, py, pz)) = self.rng.pick(&self.watching) else {
                return sown;
            };
            let at = (
                (px + self.rng.range(-SAMPLE_RADIUS, SAMPLE_RADIUS)).floor() as i32,
                // A shorter reach vertically: a player is standing on the
                // ground, and bushes are within a few metres of it.
                (py + self.rng.range(-4.0, 4.0)).floor() as i32,
                (pz + self.rng.range(-SAMPLE_RADIUS, SAMPLE_RADIUS)).floor() as i32,
            );
            if let Some(bare) = frost_takes_the_berries(world, soil, at) {
                sown.push(bare);
            }
            self.consider(world, soil, at);
            sown.extend(self.propagate(world, soil, at));
        }
        sown
    }

    /// What a sampled cell does for the wild round it, if its roll comes up:
    /// a plant sows, a leaf drops a seedling, bare earth greens. See the
    /// module note, "What the wild does on its own".
    ///
    /// **One roll per cell for all three**, each against its own odds, so a
    /// cell costs one draw and one block read whatever it is.
    fn propagate(&mut self, world: &dyn BlockWorld, soil: &dyn Soil, at: Cell) -> Vec<BlockChange> {
        let roll = self.rng.next_f32();
        let Some(here) = world.block(at.0, at.1, at.2) else {
            return Vec::new();
        };
        if offspring(here).is_some() {
            if roll * SPREAD_ONE_IN < 1.0 {
                return self.spread(world, soil, at).into_iter().collect();
            }
        } else if seeds_trees(here) {
            if roll * SEEDLING_ONE_IN < 1.0 {
                return self.seed_tree(world, soil, at);
            }
        } else if here == BLOCK_DIRT && roll * TURF_ONE_IN < 1.0 {
            return green_up(world, soil, at).into_iter().collect();
        }
        Vec::new()
    }

    /// Sows one cell round the plant at `from`, if the cell the roll lands on
    /// will take it. See [`offspring`] for what is sown, and the constants at
    /// the top for every refusal.
    ///
    /// **One cell tried, not the best of the neighbourhood.** Looking for a
    /// place that would take the seed is a search the size of the patch on
    /// every roll; trying where the seed lands and letting most rolls come to
    /// nothing is what a seed does, and it leaves the rate in `SPREAD_ONE_IN`
    /// honest -- a plant with poor ground round it spreads slowly because most
    /// of its seed falls on it.
    fn spread(&mut self, world: &dyn BlockWorld, soil: &dyn Soil, from: Cell) -> Option<BlockChange> {
        let here = world.block(from.0, from.1, from.2)?;
        let (child, wants) = offspring(here)?;
        let reach = SPREAD_REACH as f32;
        let at = (
            from.0 + self.rng.range(-reach, reach + 1.0).floor() as i32,
            from.1 + self.rng.range(-1.0, 2.0).floor() as i32,
            from.2 + self.rng.range(-reach, reach + 1.0).floor() as i32,
        );
        if world.block(at.0, at.1, at.2)? != BLOCK_AIR {
            return None;
        }
        let under = world.block(at.0, at.1 - 1, at.2)?;
        if !can_grow_on(child, under) {
            return None;
        }
        // A cattail keeps to the water's edge, as the generator plants it:
        // its ground rule would take the whole bank up the hill.
        if matches!(block_kind(child), BLOCK_CATTAIL | primitive_shared::types::BLOCK_ARUNDO)
            && !at_the_waters_edge(world, at)
        {
            return None;
        }
        if built_near(world, at) || crowded(world, at, child) {
            return None;
        }
        let fits = match (wants, sky_over(world, at)) {
            (_, Sky::Roofed) => false,
            (Wants::Open, sky) => sky == Sky::Open,
            (Wants::Shade, sky) => sky == Sky::Canopy,
            (Wants::Either, _) => true,
        };
        if !fits || !warm_enough(soil, at) {
            return None;
        }
        world.set(at.0, at.1, at.2, child);
        // A shoot or a picked berry has a wait ahead of it; the queue is how
        // it gets one without waiting for the sample to stumble on it.
        self.pending.push(at);
        Some(BlockChange {
            global_x: at.0,
            global_y: at.1,
            global_z: at.2,
            block_id: child,
        })
    }

    /// Drops a seedling of the tree whose leaf is at `leaf` into a gap within
    /// six columns of it, if there is one where the roll lands.
    ///
    /// **Where a seedling takes**: on turf or bare earth, under open sky --
    /// a gap in the crowns, a clearing, the edge of the wood -- with no wood,
    /// no timber and nothing built within three columns of its root and seven
    /// cells up, which is the room a sapling needs to become a young tree
    /// without its limbs meeting another's. What comes up is the sapling the
    /// generator plants (`worldgen::tree_stage_cells`, stage 0), in the
    /// parent's own leaf and bark -- a birch drops birches -- and it is put on
    /// the young tree's clock at once.
    fn seed_tree(&mut self, world: &dyn BlockWorld, soil: &dyn Soil, leaf: Cell) -> Vec<BlockChange> {
        let Some(parent) = world.block(leaf.0, leaf.1, leaf.2) else {
            return Vec::new();
        };
        let x = leaf.0 + self.rng.range(-6.0, 7.0).floor() as i32;
        let z = leaf.2 + self.rng.range(-6.0, 7.0).floor() as i32;
        // The ground under where it fell: the first thing that is not air,
        // leaf, wood or a plant, no further than a tall tree is tall.
        let mut ground = None;
        for y in (leaf.1 - 24..leaf.1).rev() {
            let Some(block) = world.block(x, y, z) else {
                return Vec::new();
            };
            if block == BLOCK_AIR || is_leafy(block) || is_branch(block) || is_cross(block) || is_flat(block) {
                continue;
            }
            ground = Some((y, block));
            break;
        }
        let Some((ground, soil_block)) = ground else {
            return Vec::new();
        };
        let root = (x, ground + 1, z);
        if !matches!(soil_block, BLOCK_GRASS | BLOCK_DIRT)
            || world.block(root.0, root.1, root.2) != Some(BLOCK_AIR)
            || sky_over(world, root) != Sky::Open
        {
            return Vec::new();
        }
        let crowded = (0..=7).any(|dy| {
            (-3..=3).any(|dz| {
                (-3..=3).any(|dx| {
                    world.block(x + dx, root.1 + dy, z + dz).is_some_and(|b| {
                        is_branch(b) || primitive_shared::wood::is_log(b) || is_built(b)
                    })
                })
            })
        });
        if crowded || !warm_enough(soil, root) {
            return Vec::new();
        }
        let leaves = block_kind(parent);
        let variant = primitive_shared::worldgen::young_tree_variant(x, z);
        let Some(cells) = tree_stage_cells(0, variant, leaves, ground_rise(world, root)) else {
            return Vec::new();
        };
        let place = |(dx, dy, dz): (i32, i32, i32)| (x + dx, ground + dy, z + dz);
        // Every piece of wood into air or a plant, or none of it: half a
        // sapling is a twig standing in the grass.
        let room = cells.iter().filter(|(_, id)| is_branch(*id)).all(|&(at, _)| {
            let (px, py, pz) = place(at);
            world.block(px, py, pz).is_some_and(|b| b == BLOCK_AIR || is_cross(b))
        });
        if !room {
            return Vec::new();
        }
        let mut changes = Vec::new();
        for (at, id) in cells {
            let (px, py, pz) = place(at);
            let free = world
                .block(px, py, pz)
                .is_some_and(|b| b == BLOCK_AIR || (is_branch(id) && is_cross(b)));
            if !free {
                continue;
            }
            world.set(px, py, pz, id);
            changes.push(BlockChange {
                global_x: px,
                global_y: py,
                global_z: pz,
                block_id: id,
            });
        }
        self.pending.push(root);
        changes
    }
}

/// What light a wild plant's seedling needs.
#[derive(Debug, Clone, Copy, PartialEq)]
enum Wants {
    /// Nothing over it but sky: grass, the meadow's flowers, the sun-side
    /// plants.
    Open,
    /// A canopy over it: the fern and the bilberry, the forest floor's own.
    Shade,
    /// Either, and never a roof: bracken, in a light wood and its clearing.
    Either,
}

/// What stands over a cell, all the way up.
#[derive(Debug, Clone, Copy, PartialEq)]
enum Sky {
    /// Air, and nothing that shades.
    Open,
    /// Leaves or wood somewhere over it, and nothing else.
    Canopy,
    /// Anything solid: rock over a cave, a roof, a player's shelf.
    Roofed,
}

/// **Whether the sky reaches a cell, and through what**, read straight up.
///
/// Asked of the blocks because the growth mechanic is handed a world of
/// blocks and not its light (`BlockWorld`), and the blocks answer the one
/// question the light would have been asked for: is this a cave, a house or
/// the forest floor. A plant standing on the column above -- a tuft on a
/// ledge -- shades nothing. A cell nobody has loaded above is taken as sky,
/// because the columns a player stands among are loaded to the top.
fn sky_over(world: &dyn BlockWorld, (x, y, z): Cell) -> Sky {
    let mut canopy = false;
    for above in y + 1..CHUNK_SIZE_Y as i32 {
        match world.block(x, above, z) {
            None => break,
            Some(b) if b == BLOCK_AIR || is_cross(b) || is_flat(b) => {}
            Some(b) if is_leafy(b) || is_branch(b) => canopy = true,
            Some(_) => return Sky::Roofed,
        }
    }
    if canopy {
        Sky::Canopy
    } else {
        Sky::Open
    }
}

/// What a wild plant sows round itself, and the light that needs: `None` for
/// anything that does not spread. A tall plant sows a shoot
/// (`types::PLANT_YOUNG`), a berry sows its picked shrub, which fruits after
/// its wait; everything else sows itself.
///
/// **Not the reeds, the roots, the wild crops or the bushes.** Reeds and
/// roots are placed by where water and shade are, and the ground rule a reed
/// has would take a bed of them up the bank; the wild wheat and cotton are
/// things to find, and a stand that spread would be a field nobody sowed; a
/// berry bush is the larder that was thinned twice.
fn offspring(here: BlockId) -> Option<(BlockId, Wants)> {
    if is_plant_top(here) || is_plant_shoot(here) {
        return None;
    }
    Some(match block_kind(here) {
        BLOCK_TALL_GRASS => (BLOCK_TALL_GRASS, Wants::Open),
        BLOCK_DRY_GRASS => (BLOCK_DRY_GRASS, Wants::Open),
        BLOCK_FLOWER => (BLOCK_FLOWER, Wants::Open),
        BLOCK_PLANTAIN => (BLOCK_PLANTAIN, Wants::Open),
        BLOCK_SUNDEW => (BLOCK_SUNDEW, Wants::Open),
        BLOCK_STRAWBERRY | BLOCK_STRAWBERRY_BARE => (BLOCK_STRAWBERRY_BARE, Wants::Open),
        BLOCK_FERN => (BLOCK_FERN, Wants::Shade),
        BLOCK_BILBERRY | BLOCK_BILBERRY_BARE => (BLOCK_BILBERRY_BARE, Wants::Shade),
        BLOCK_BRACKEN => (plant_shoot(BLOCK_BRACKEN), Wants::Either),
        kind @ (BLOCK_FIREWEED | BLOCK_NETTLE | BLOCK_CATTAIL | primitive_shared::types::BLOCK_ARUNDO) => {
            (plant_shoot(kind), Wants::Open)
        }
        _ => return None,
    })
}

/// The ground cover that thins only by the crowd, not by its kind: grass,
/// flowers and ferns are what a patch is made of. See `KIN`.
fn is_ground_cover(id: BlockId) -> bool {
    matches!(block_kind(id), BLOCK_TALL_GRASS | BLOCK_DRY_GRASS | BLOCK_FLOWER | BLOCK_FERN)
}

/// One plant for the `KIN` count, whichever state it is in.
fn kin_of(id: BlockId) -> BlockId {
    match block_kind(id) {
        BLOCK_BILBERRY_BARE => BLOCK_BILBERRY,
        BLOCK_STRAWBERRY_BARE => BLOCK_STRAWBERRY,
        kind => kind,
    }
}

/// Whether a plant at `at` would make any five-by-five patch it falls into
/// hold more than a meadow does, or stand too near too many of `child`'s kind.
/// See `CROWD` and `KIN`. The upper half of a tall plant is not a second plant.
///
/// The plants within `KIN_REACH` are read once, a level either side, into a
/// nine-by-nine grid of counts, and the twenty-five patches that contain `at`
/// are summed from the grid: two hundred and forty-three block reads a try,
/// where asking the world for every patch would be nearly two thousand.
fn crowded(world: &dyn BlockWorld, at: Cell, child: BlockId) -> bool {
    const SIDE: usize = (2 * KIN_REACH + 1) as usize;
    let mut counts = [[0u8; SIDE]; SIDE];
    let mut kin = 0;
    for dy in -1..=1 {
        for dz in -KIN_REACH..=KIN_REACH {
            for dx in -KIN_REACH..=KIN_REACH {
                let Some(b) = world.block(at.0 + dx, at.1 + dy, at.2 + dz) else {
                    continue;
                };
                if !is_cross(b) || is_plant_top(b) {
                    continue;
                }
                counts[(dz + KIN_REACH) as usize][(dx + KIN_REACH) as usize] += 1;
                kin += usize::from(kin_of(b) == kin_of(child));
            }
        }
    }
    if !is_ground_cover(child) && kin >= KIN {
        return true;
    }
    // Every patch with `at` in it, by its centre's offset from `at`.
    let patch = |cx: i32, cz: i32| -> u8 {
        (cz - 2..=cz + 2)
            .flat_map(|z| (cx - 2..=cx + 2).map(move |x| (x, z)))
            .map(|(x, z)| counts[(z + KIN_REACH) as usize][(x + KIN_REACH) as usize])
            .sum()
    };
    (-2..=2).any(|cz| (-2..=2).any(|cx| patch(cx, cz) >= CROWD))
}

/// Whether this is something a player put there.
///
/// **Anything that is not the world's own ground, water, wood or growth.**
/// The world lays turf, earth, sand, snow, gravel, clay, mud, peat, rock and
/// scree, and trees of logs and pieces; everything else in a cell -- planks,
/// a chest, a fire, a wall of dressed stone, tilled earth -- is a build, and a
/// wild plant sown against one is a plant in somebody's floor. Logs count as
/// the world's, because a fallen trunk is, and grass beside a log wall is
/// grass outside a cabin rather than in it.
fn is_built(id: BlockId) -> bool {
    if block_kind(id) == BLOCK_FARMLAND {
        return true;
    }
    if id == BLOCK_AIR || is_cross(id) || is_flat(id) || is_leafy(id) || is_branch(id) || is_liquid(id) {
        return false;
    }
    // Every rock, its rubble and every soil the world lays, and a log of any
    // wood: the ground's own (`ground`), asked of the table rather than
    // added to the list below seventy times.
    if primitive_shared::ground::rock_of(id).is_some()
        || primitive_shared::ground::is_soil(id)
        || primitive_shared::wood::is_log(id)
    {
        return false;
    }
    !matches!(
        block_kind(id),
        BLOCK_GRASS
            | BLOCK_DIRT
            | BLOCK_SAND
            | BLOCK_SNOW
            | BLOCK_ICE
            | BLOCK_GRAVEL
            | BLOCK_CLAY
            | BLOCK_MUD
            | BLOCK_PEAT
            | BLOCK_SANDY_SOIL
            | BLOCK_DRY_TURF
            | BLOCK_STONE
            | BLOCK_GRANITE
            | BLOCK_LIMESTONE
            | BLOCK_SANDSTONE
            | BLOCK_BASALT
            | BLOCK_COBBLESTONE
            | BLOCK_LOG
            | BLOCK_BIRCH_LOG
            | primitive_shared::types::BLOCK_FIR_LOG
            | primitive_shared::types::BLOCK_SAXAUL_LOG
    )
}

/// Whether anything built stands in the three-by-three-by-three round a cell.
fn built_near(world: &dyn BlockWorld, (x, y, z): Cell) -> bool {
    (-1..=1).any(|dy| {
        (-1..=1).any(|dz| (-1..=1).any(|dx| world.block(x + dx, y + dy, z + dz).is_some_and(is_built)))
    })
}

/// Whether water lies against the ground a cattail would stand on: one of the
/// four cells beside its floor is liquid. The reed's waterline, asked of the
/// blocks (`worldgen`'s `beside_water` asks it of the columns).
fn at_the_waters_edge(world: &dyn BlockWorld, (x, y, z): Cell) -> bool {
    [(1, 0), (-1, 0), (0, 1), (0, -1)]
        .iter()
        .any(|&(dx, dz)| world.block(x + dx, y - 1, z + dz).is_some_and(is_liquid))
}

/// Whether the air in a cell is warm enough for anything wild to grow. A cell
/// nobody has loaded is not, for `Soil::air_c`'s reason.
fn warm_enough(soil: &dyn Soil, (x, y, z): Cell) -> bool {
    soil.air_c(x, y, z).is_some_and(|air| air.is_finite() && air >= GREEN_UP_C)
}

/// A leaf a seedling can fall from: the oak's, the birch's and the maple's.
///
/// **Not an apple's**: a young tree keeps its leaf and never fruits
/// (`worldgen::fruit_cells` hangs apples only at generation), so a seedling
/// of one would be an apple tree with no apples on it for good. Not an
/// acacia's or a palm's, whose young trees would be an oak's shape.
fn seeds_trees(id: BlockId) -> bool {
    matches!(block_kind(id), BLOCK_LEAVES | BLOCK_BIRCH_LEAVES | BLOCK_MAPLE_LEAVES)
}

/// A berry in a hard frost falls, and the plant stands bare until warm air
/// fruits it again.
///
/// **"убери ягоды зимой вовсе, сделай охоту единственным способом выжить".**
/// Before this a bush that fruited in September was still full in January,
/// so a winter was lived on whatever the woods had kept -- and the woods kept
/// everything. At [`FROST_C`] now the bush, the bilberry and the strawberry
/// drop what they carry, and nothing sets again below [`FRUIT_SET_C`]
/// (`fruit_sets_above_c`), so the berries a player has in the winter are the
/// ones they picked before the first frost and the rest of the winter's food
/// is meat. That is a decision the autumn has to make, which is the point.
///
/// **Frost, not the calendar.** `Season::Winter` was the other reading, and
/// it would strip a tropical bush in a "winter" that never freezes and leave
/// a taiga bush full through a September snowfall. The air is what a berry
/// actually dies of.
///
/// **Found by the sampler, not swept.** A sweep over every loaded bush on
/// the day the frost comes is a spike, and nobody is there to see the bushes
/// nobody is near. The sampler reaches a cell round a player about every
/// half-minute (`SAMPLE_PER_TICK` over the box `SAMPLE_RADIUS` spans), which
/// is sooner than a player walking up to a bush can pick it.
fn frost_takes_the_berries(world: &dyn BlockWorld, soil: &dyn Soil, at: Cell) -> Option<BlockChange> {
    let bare = match block_kind(world.block(at.0, at.1, at.2)?) {
        BLOCK_BERRY_BUSH => BLOCK_BARE_BUSH,
        BLOCK_BILBERRY => BLOCK_BILBERRY_BARE,
        BLOCK_STRAWBERRY => BLOCK_STRAWBERRY_BARE,
        _ => return None,
    };
    let air = soil.air_c(at.0, at.1, at.2)?;
    if !(air.is_finite() && air <= FROST_C) {
        return None;
    }
    world.set(at.0, at.1, at.2, bare);
    Some(BlockChange { global_x: at.0, global_y: at.1, global_z: at.2, block_id: bare })
}

/// Bare earth greening over: a full block of dirt with air or a plant on it,
/// under open sky, beside turf on its own level, with nothing built round
/// it and warm air over it, becomes turf.
///
/// **Beside turf, not merely in the open.** Grass creeps; it does not fall
/// out of the sky onto a dug patch in the middle of a field of earth. And
/// only a *whole* block of dirt: a spadeful laid as a layer is a player's,
/// and so is tilled earth, which is not dirt at all.
fn green_up(world: &dyn BlockWorld, soil: &dyn Soil, at: Cell) -> Option<BlockChange> {
    if world.block(at.0, at.1, at.2)? != BLOCK_DIRT {
        return None;
    }
    let over = world.block(at.0, at.1 + 1, at.2)?;
    if !(over == BLOCK_AIR || is_cross(over)) {
        return None;
    }
    let beside_turf = [(1, 0), (-1, 0), (0, 1), (0, -1)]
        .iter()
        .any(|&(dx, dz)| world.block(at.0 + dx, at.1, at.2 + dz) == Some(BLOCK_GRASS));
    if !beside_turf || built_near(world, at) || sky_over(world, at) != Sky::Open || !warm_enough(soil, at) {
        return None;
    }
    world.set(at.0, at.1, at.2, BLOCK_GRASS);
    Some(BlockChange {
        global_x: at.0,
        global_y: at.1,
        global_z: at.2,
        block_id: BLOCK_GRASS,
    })
}

/// How far the ground under each column round a root stands above the ground
/// under the root: the highest block with a full top within four of it. What
/// every young tree's limbs are grown against (`worldgen::tree_stage_cells`).
fn ground_rise(world: &dyn BlockWorld, (x, y, z): Cell) -> impl Fn(i32, i32) -> i32 + Copy + '_ {
    move |dx: i32, dz: i32| {
        (-4..=4)
            .rev()
            .find(|&dy| {
                world
                    .block(x + dx, y - 1 + dy, z + dz)
                    .is_some_and(|b| has_full_top(b) && !is_branch(b) && !is_leafy(b))
            })
            .unwrap_or(-5)
    }
}

/// Whether this cell is the root of a tree that is still growing: a piece
/// of wood narrower than a grown tree's foot, standing on turf or earth.
///
/// Nothing else in the world is that. A grown tree's foot is `GROWN_FOOT`
/// wide or wider, a limb has air under it, and a player cannot place a
/// piece of branch.
fn young_root(world: &dyn BlockWorld, (x, y, z): Cell) -> bool {
    // **Never a palm.** A palm's trunk is a piece of branch as narrow as a
    // sapling's, and a palm on a warm coast's turf would otherwise be read
    // as a young oak and grown into one overnight.
    world
        .block(x, y, z)
        .filter(|&b| block_kind(b) != primitive_shared::types::BLOCK_PALM_TRUNK)
        .and_then(branch_width)
        .is_some_and(|width| width < GROWN_FOOT)
        && world
            .block(x, y - 1, z)
            .is_some_and(|under| matches!(block_kind(under), BLOCK_GRASS | BLOCK_DIRT))
}

/// Grows the young tree rooted at `(x, y, z)` into its next shape, if there
/// is room for it, and says what changed.
///
/// **Which shape it is now is read off its stem** -- how many pieces stand
/// straight up from the root and how wide the foot is -- against every
/// young shape its column's variant has (`worldgen::stem_of`). The limbs
/// and leaves are not asked: a shoot the ground refused, a leaf a player
/// took, a leaf another crown put in the way would all make an exact match
/// fail, and the stem is what a player would have to cut down to stop a
/// tree growing.
///
/// **It grows only into room.** Every piece of the next shape has to land
/// in air, leaves, a tuft of grass, or where the tree already has that
/// piece, and no piece may come to stand beside wood that is neither the
/// old tree nor the new one -- a young tree under a roof, against a wall or
/// grown into another tree's limbs waits instead. Writing through them was
/// the alternative, and a tree that ate a player's roof over the course of
/// an evening would be the one bug nobody forgives.
///
/// What the old shape had and the new one does not comes out first, and
/// only where the world still holds exactly that block; then the wood goes
/// in, and the leaves into whatever air is left.
fn grow_young_tree(world: &dyn BlockWorld, (x, y, z): Cell) -> Vec<BlockChange> {
    let variant = young_tree_variant(x, z);
    let Some(foot) = world.block(x, y, z).and_then(branch_width) else {
        return Vec::new();
    };
    let stem = (0..16)
        .take_while(|&dy| world.block(x, y + dy, z).is_some_and(is_branch))
        .count() as i32;
    // The ground round the root, for the limbs. See `ground_rise`.
    let rise = ground_rise(world, (x, y, z));
    // The leaf of the wood whose bark the foot is in: a birch's, a willow's.
    let bark_leaves = world
        .block(x, y, z)
        .and_then(primitive_shared::types::piece_log)
        .and_then(primitive_shared::wood::wood_of)
        .map_or(BLOCK_LEAVES, |wood| wood.leaves);
    let Some(stage) = (0..TREE_STAGES - 1).find(|&s| {
        tree_stage_cells(s, variant, BLOCK_LEAVES, rise).is_some_and(|cells| stem_of(&cells) == (stem, foot))
    }) else {
        return Vec::new();
    };
    let place = |(dx, dy, dz): (i32, i32, i32)| (x + dx, y - 1 + dy, z + dz);
    let Some(old) = tree_stage_cells(stage, variant, BLOCK_LEAVES, rise) else {
        return Vec::new();
    };
    // The leaf it grows: whatever leaf its present shape wears, so an apple
    // sapling stays an apple tree. **With its leaves picked off, what its
    // bark says** -- a birch stripped bare is still a birch, and grown in the
    // oak's leaf its white stem would be the one thing the new shape refused
    // to overwrite, and it would never grow again.
    let leaves = old
        .iter()
        .filter(|(_, id)| is_leafy(*id))
        .find_map(|&(at, _)| {
            let (px, py, pz) = place(at);
            world.block(px, py, pz).filter(|&b| is_leafy(b))
        })
        .unwrap_or(bark_leaves);
    let (Some(old), Some(new)) = (
        tree_stage_cells(stage, variant, leaves, rise),
        tree_stage_cells(stage + 1, variant, leaves, rise),
    ) else {
        return Vec::new();
    };
    // Later cells win, as they do when a tree is written: wood over leaf.
    let old: HashMap<Cell, BlockId> = old.into_iter().map(|(at, id)| (place(at), id)).collect();
    let new: HashMap<Cell, BlockId> = new.into_iter().map(|(at, id)| (place(at), id)).collect();

    for (&(px, py, pz), &id) in &new {
        let Some(there) = world.block(px, py, pz) else {
            return Vec::new(); // not loaded: come back when it is
        };
        if !is_branch(id) {
            continue;
        }
        let ours = old.get(&(px, py, pz)).is_some_and(|&was| was == there);
        if !(there == BLOCK_AIR || is_leafy(there) || is_cross(there) || ours) {
            return Vec::new();
        }
        let beside = [(1, 0, 0), (-1, 0, 0), (0, 1, 0), (0, -1, 0), (0, 0, 1), (0, 0, -1)];
        for (dx, dy, dz) in beside {
            let near = (px + dx, py + dy, pz + dz);
            let foreign = world.block(near.0, near.1, near.2).is_some_and(is_branch)
                && !old.get(&near).is_some_and(|&b| is_branch(b))
                && !new.get(&near).is_some_and(|&b| is_branch(b));
            if foreign {
                return Vec::new();
            }
        }
    }

    let mut changes = Vec::new();
    let mut put = |(px, py, pz): Cell, id: BlockId| {
        world.set(px, py, pz, id);
        changes.push(BlockChange {
            global_x: px,
            global_y: py,
            global_z: pz,
            block_id: id,
        });
    };
    for (&at, &id) in &old {
        if !new.contains_key(&at) && world.block(at.0, at.1, at.2) == Some(id) {
            put(at, BLOCK_AIR);
        }
    }
    for (&at, &id) in new.iter().filter(|(_, id)| is_branch(**id)) {
        if world.block(at.0, at.1, at.2) != Some(id) {
            put(at, id);
        }
    }
    for (&at, &id) in new.iter().filter(|(_, id)| !is_branch(**id)) {
        if world.block(at.0, at.1, at.2).is_some_and(|b| b == BLOCK_AIR || is_cross(b)) {
            put(at, id);
        }
    }
    changes
}

#[cfg(test)]
mod tests {
    use std::cell::Cell as Shared;

    use primitive_shared::types::{
        BLOCK_APPLE_LEAVES_FRUIT, BLOCK_BERRY_BUSH, BLOCK_COTTON_RIPE, BLOCK_DIRT, BLOCK_FARMLAND,
        BLOCK_GRASS, BLOCK_ICE, is_birch_wood, BLOCK_STONE, BLOCK_WATER, BLOCK_WHEAT_RIPE, ALL_BLOCK_IDS,
    };

    use super::*;
    use crate::logic::falling::tests::TestWorld;

    const AT: Cell = (5, 21, 5);

    #[test]
    fn everything_that_grows_back_and_is_not_a_crop_keeps_the_bushs_clock() {
        // **The default arm is the crop's**, and a thing that ripens
        // without being named in `ripening_seconds` falls into it without
        // a word: the robbed nest did, and refilled on a field's fifteen
        // minutes for as long as nests existed, while the comment on
        // `ripens_into` promised the bush's half hour. So the rule is asked
        // of the whole table rather than of the ones somebody remembered.
        for &(id, _) in ALL_BLOCK_IDS.iter() {
            if ripens_into(id).is_none() || is_crop(id) {
                continue;
            }
            assert_eq!(
                ripening_seconds(id),
                Some(REGROW_SECONDS),
                "{} grows back on a crop's clock",
                primitive_shared::types::block_name(id)
            );
        }
    }

    /// A sapling the generator would have planted at `AT`, on a lawn with
    /// room all round it.
    fn world_with_a_sapling() -> TestWorld {
        let world = TestWorld::default();
        for dz in -7..=7 {
            for dx in -7..=7 {
                world.put(AT.0 + dx, AT.1 - 1, AT.2 + dz, BLOCK_GRASS);
            }
        }
        let variant = young_tree_variant(AT.0, AT.2);
        for ((dx, dy, dz), id) in tree_stage_cells(0, variant, BLOCK_LEAVES, |_, _| 0).unwrap() {
            world.put(AT.0 + dx, AT.1 - 1 + dy, AT.2 + dz, id);
        }
        world
    }

    fn stem_at(world: &TestWorld) -> (usize, u8) {
        let stem = (0..20).take_while(|&dy| is_branch(world.get(AT.0, AT.1 + dy, AT.2))).count();
        (stem, branch_width(world.get(AT.0, AT.1, AT.2)).unwrap_or(0))
    }

    #[test]
    fn a_sapling_grows_its_next_shape_after_a_trees_wait_and_not_before() {
        let world = world_with_a_sapling();
        let mut growth = Growth::seeded(11);
        let planted = stem_at(&world);
        growth.on_block_changed(AT.0, AT.1, AT.2);
        growth.step(&world, &EvenSoil, 0.0, 64);
        // Under the soonest the jitter allows: nothing yet.
        growth.step(&world, &EvenSoil, TREE_STAGE_SECONDS * 0.7, 64);
        assert_eq!(stem_at(&world), planted, "a sapling grew before its time");
        // Past the latest: a taller stem on a wider foot.
        let changes = growth.step(&world, &EvenSoil, TREE_STAGE_SECONDS * 0.6, 64);
        let grown = stem_at(&world);
        assert!(
            grown.0 > planted.0 && grown.1 >= planted.1,
            "a sapling of {planted:?} is {grown:?} after a whole stage"
        );
        assert!(!changes.is_empty(), "the tree grew and nobody was told");
    }

    #[test]
    fn a_sapling_left_long_enough_becomes_a_grown_tree_and_stops() {
        let world = world_with_a_sapling();
        let mut growth = Growth::seeded(12);
        growth.on_block_changed(AT.0, AT.1, AT.2);
        growth.step(&world, &EvenSoil, 0.0, 64);
        for _ in 0..(TREE_STAGES as usize + 2) {
            growth.step(&world, &EvenSoil, TREE_STAGE_SECONDS * 1.3, 64);
        }
        let (stem, foot) = stem_at(&world);
        assert!(foot >= GROWN_FOOT, "after every stage the tree stands on a foot of {foot}, {stem} tall");
        assert!(growth.young.is_empty(), "a grown tree is still on the clock");
        // Grown means grown: another long wait changes nothing.
        let before = stem_at(&world);
        growth.on_block_changed(AT.0, AT.1, AT.2);
        growth.step(&world, &EvenSoil, 0.0, 64);
        growth.step(&world, &EvenSoil, TREE_STAGE_SECONDS * 3.0, 64);
        assert_eq!(stem_at(&world), before, "a grown tree grew again");
    }

    #[test]
    fn a_sapling_under_a_roof_waits_rather_than_growing_through_it() {
        let world = world_with_a_sapling();
        // A slab of stone five above the ground, over the whole lawn: the
        // next shape's stem would have to go through it.
        for dz in -7..=7 {
            for dx in -7..=7 {
                world.put(AT.0 + dx, AT.1 + 4, AT.2 + dz, BLOCK_STONE);
            }
        }
        let planted = stem_at(&world);
        let mut growth = Growth::seeded(13);
        growth.on_block_changed(AT.0, AT.1, AT.2);
        growth.step(&world, &EvenSoil, 0.0, 64);
        for _ in 0..4 {
            growth.step(&world, &EvenSoil, TREE_STAGE_SECONDS * 1.3, 64);
        }
        assert_eq!(stem_at(&world), planted, "a sapling grew into a roof");
        assert_eq!(world.get(AT.0, AT.1 + 4, AT.2), BLOCK_STONE, "the roof was eaten");
    }

    /// Ground of a stated quality, for the fertility tests, under the
    /// even soil's mild air.
    struct Ground(f32);

    impl Soil for Ground {
        fn growth_factor(&self, _gx: i32, _gz: i32) -> f32 {
            self.0
        }

        fn air_c(&self, _gx: i32, _gy: i32, _gz: i32) -> Option<f32> {
            Some(EvenSoil::AIR_C)
        }
    }

    /// Ordinary ground under air of a stated temperature, which a test
    /// may change between steps -- a cold night, and then the morning.
    struct Air(Shared<f32>);

    impl Air {
        fn at(degrees: f32) -> Self {
            Air(Shared::new(degrees))
        }
    }

    impl Soil for Air {
        fn growth_factor(&self, _gx: i32, _gz: i32) -> f32 {
            1.0
        }

        fn air_c(&self, _gx: i32, _gy: i32, _gz: i32) -> Option<f32> {
            Some(self.0.get())
        }
    }

    /// Where the channel runs: two cells along from the crop, level with
    /// the tilled earth, which is well inside `WATER_REACH`.
    const CHANNEL: Cell = (AT.0 + 2, AT.1 - 1, AT.2);

    /// A crop standing in tilled earth, with or without the channel.
    fn field(crop: BlockId, water: bool) -> TestWorld {
        let world = TestWorld::default();
        world.put(AT.0, AT.1 - 1, AT.2, BLOCK_FARMLAND);
        world.put(AT.0, AT.1, AT.2, crop);
        if water {
            world.put(CHANNEL.0, CHANNEL.1, CHANNEL.2, BLOCK_WATER);
        }
        world
    }

    /// A seed planted in watered earth.
    fn world_with_a_planted_seed() -> TestWorld {
        field(BLOCK_SEEDS, true)
    }

    /// Notices the crop, then lets `seconds` go by in forty ticks -- small
    /// enough that the looks inside them happen, which one giant step
    /// would not show.
    fn grow_for(growth: &mut Growth, world: &TestWorld, soil: &dyn Soil, seconds: f32) -> Vec<BlockChange> {
        growth.on_block_changed(AT.0, AT.1, AT.2);
        let mut changes = growth.step(world, soil, 0.0, 64);
        for _ in 0..40 {
            changes.extend(growth.step(world, soil, seconds / 40.0, 64));
        }
        changes
    }

    fn here(world: &TestWorld) -> BlockId {
        world.get(AT.0, AT.1, AT.2)
    }

    #[test]
    fn a_crop_on_river_silt_beats_the_same_crop_on_thin_ground() {
        // **What fertility is worth, as one comparison.** The same seed
        // in the same weather, in two soils: the good one is ready and
        // the poor one is not. If this ever comes out equal, fertility
        // has become a number with no effect -- which is exactly what a
        // mechanic like this decays into.
        let plant = |factor: f32| {
            let world = world_with_a_planted_seed();
            let mut growth = Growth::seeded(7);
            growth.on_block_changed(AT.0, AT.1, AT.2);
            growth.step(&world, &Ground(factor), 0.0, 64);
            // A stage and a bit: enough for ordinary ground and for
            // rich, not enough for thin.
            growth.step(&world, &Ground(factor), CROP_STAGE_SECONDS * 1.2, 64);
            world.get(AT.0, AT.1, AT.2)
        };
        assert_eq!(
            plant(primitive_shared::worldgen::Fertility::Rich.growth_factor()),
            BLOCK_WHEAT,
            "a crop on rich ground did not come up"
        );
        assert_eq!(
            plant(primitive_shared::worldgen::Fertility::Poor.growth_factor()),
            BLOCK_SEEDS,
            "thin ground grew a crop as fast as silt"
        );
    }

    #[test]
    fn ash_dug_into_thin_ground_grows_a_crop_as_fast_as_ordinary_ground_and_is_spent_on_the_harvest() {
        // The same poor ground twice, once dressed with ash: after a stage
        // and a bit the dressed seed is up and the plain one is not. Then
        // it ripens, and the furrow under the ripe crop is plain earth.
        let plant = |ash: bool| {
            let world = world_with_a_planted_seed();
            if ash {
                let under = world.get(AT.0, AT.1 - 1, AT.2);
                world.put(AT.0, AT.1 - 1, AT.2, primitive_shared::wildfire::dressed(under).unwrap());
            }
            let ground = Ground(1.5);
            let mut growth = Growth::seeded(7);
            growth.on_block_changed(AT.0, AT.1, AT.2);
            growth.step(&world, &ground, 0.0, 64);
            growth.step(&world, &ground, CROP_STAGE_SECONDS * 1.2, 64);
            (world.get(AT.0, AT.1, AT.2), world, growth)
        };
        assert_eq!(plant(false).0, BLOCK_SEEDS, "thin ground came up without ash");
        let (up, world, mut growth) = plant(true);
        assert_eq!(up, BLOCK_WHEAT, "ash did nothing for thin ground");
        assert!(primitive_shared::wildfire::is_dressed(world.get(AT.0, AT.1 - 1, AT.2)), "the ash was spent before the harvest");
        for _ in 0..4 {
            growth.step(&world, &Ground(1.5), CROP_STAGE_SECONDS, 64);
        }
        assert_eq!(world.get(AT.0, AT.1, AT.2), BLOCK_WHEAT_RIPE);
        assert!(
            !primitive_shared::wildfire::is_dressed(world.get(AT.0, AT.1 - 1, AT.2)),
            "the dressing outlived the harvest it fed"
        );
    }

    #[test]
    fn a_furrow_cropped_without_ash_is_slower_until_it_has_lain_fallow() {
        // Ripen a crop on plain ground: the furrow under it is tired.
        let world = world_with_a_planted_seed();
        let ground = Ground(1.0);
        let mut growth = Growth::seeded(7);
        growth.on_block_changed(AT.0, AT.1, AT.2);
        growth.step(&world, &ground, 0.0, 64);
        for _ in 0..4 {
            growth.step(&world, &ground, CROP_STAGE_SECONDS, 64);
        }
        assert_eq!(here(&world), BLOCK_WHEAT_RIPE);
        let under = world.get(AT.0, AT.1 - 1, AT.2);
        assert!(primitive_shared::wildfire::is_tired(under), "a harvest off undressed ground left it fresh");

        // The same seed sown again straight away is behind one sown in rested
        // ground after a stage and a bit.
        let sow_on = |furrow: BlockId| {
            let world = world_with_a_planted_seed();
            world.put(AT.0, AT.1 - 1, AT.2, furrow);
            let mut growth = Growth::seeded(7);
            growth.on_block_changed(AT.0, AT.1, AT.2);
            growth.step(&world, &ground, 0.0, 64);
            growth.step(&world, &ground, CROP_STAGE_SECONDS * 1.2, 64);
            here(&world)
        };
        assert_eq!(sow_on(primitive_shared::wildfire::rested(under)), BLOCK_WHEAT);
        assert_eq!(sow_on(under), BLOCK_SEEDS, "a tired furrow grew as fast as a rested one");

        // Reaped and left bare, it rests -- after the fallow, not before.
        world.put(AT.0, AT.1, AT.2, BLOCK_AIR);
        growth.on_block_changed(AT.0, AT.1, AT.2);
        growth.step(&world, &ground, 0.0, 64);
        growth.step(&world, &ground, FALLOW_SECONDS * 0.5, 64);
        assert!(primitive_shared::wildfire::is_tired(world.get(AT.0, AT.1 - 1, AT.2)), "rested in half the fallow");
        growth.step(&world, &ground, FALLOW_SECONDS, 64);
        assert!(
            !primitive_shared::wildfire::is_tired(world.get(AT.0, AT.1 - 1, AT.2)),
            "a bare furrow never rested"
        );
    }

    #[test]
    fn the_ground_does_not_hurry_what_the_world_planted_itself() {
        // A bush and a nest are the world's own larder, and they fill
        // on the flat clock wherever they stand: a hedgerow that came
        // back faster in a good valley would be a rule with no way to
        // notice it and nothing to decide about.
        let world = TestWorld::default();
        world.put(AT.0, AT.1 - 1, AT.2, BLOCK_GRASS);
        world.put(AT.0, AT.1, AT.2, BLOCK_BARE_BUSH);
        let mut rich = Growth::seeded(11);
        rich.on_block_changed(AT.0, AT.1, AT.2);
        rich.step(&world, &Ground(0.1), 0.0, 64);
        assert_eq!(
            world.get(AT.0, AT.1, AT.2),
            BLOCK_BARE_BUSH,
            "the bush filled the instant it was queued"
        );
        rich.step(&world, &Ground(0.1), REGROW_SECONDS * 0.5, 64);
        assert_eq!(
            world.get(AT.0, AT.1, AT.2),
            BLOCK_BARE_BUSH,
            "the best ground in the world hurried a wild bush"
        );
    }

    #[test]
    fn a_planted_seed_becomes_a_crop_and_then_a_harvest() {
        // Two stages, one machine. The bush was the only thing that grew
        // when this file was written, and its answer was written into
        // three separate places here; the crop is the second thing, and
        // it goes through the same path because that answer moved to
        // `types::ripens_into`.
        let world = world_with_a_planted_seed();
        let mut growth = Growth::new();
        growth.on_block_changed(AT.0, AT.1, AT.2);
        growth.step(&world, &EvenSoil, 0.0, 64);
        assert_eq!(growth.ripening(), 1, "a planted seed is not growing");

        let changes = growth.step(&world, &EvenSoil, CROP_STAGE_SECONDS * 2.0, 64);
        assert_eq!(changes.len(), 1);
        assert_eq!(world.get(AT.0, AT.1, AT.2), BLOCK_WHEAT, "the seed did not come up");

        // ...and the second stage goes the same way.
        growth.on_block_changed(AT.0, AT.1, AT.2);
        growth.step(&world, &EvenSoil, 0.0, 64);
        growth.step(&world, &EvenSoil, CROP_STAGE_SECONDS * 2.0, 64);
        assert_eq!(world.get(AT.0, AT.1, AT.2), BLOCK_WHEAT_RIPE, "it never ripened");

        // Ripe is the end of it: a field left standing does not turn
        // into anything else.
        growth.on_block_changed(AT.0, AT.1, AT.2);
        growth.step(&world, &EvenSoil, 0.0, 64);
        assert_eq!(growth.ripening(), 0, "ripe wheat is still on the clock");
    }

    /// The test above tells the machine about the new stage by hand, and
    /// the game never did: nothing that writes a grown crop reports the
    /// change, so a field came up green and then waited for somebody to
    /// walk within reach of the sample before its second stage started.
    /// A player who sowed and went to the hills came back to stalks.
    #[test]
    fn a_crop_that_came_up_goes_on_ripening_without_being_told() {
        let world = world_with_a_planted_seed();
        let mut growth = Growth::new();
        growth.on_block_changed(AT.0, AT.1, AT.2);
        growth.step(&world, &EvenSoil, 0.0, 64);
        growth.step(&world, &EvenSoil, CROP_STAGE_SECONDS * 2.0, 64);
        assert_eq!(world.get(AT.0, AT.1, AT.2), BLOCK_WHEAT, "the seed did not come up");

        // Nobody says anything, and nobody is standing near it.
        growth.step(&world, &EvenSoil, 0.0, 64);
        growth.step(&world, &EvenSoil, CROP_STAGE_SECONDS * 2.0, 64);
        assert_eq!(world.get(AT.0, AT.1, AT.2), BLOCK_WHEAT_RIPE, "the field stopped at green stalks");
    }

    #[test]
    fn a_cotton_field_goes_from_seed_to_bolls_through_the_same_machine() {
        // The second crop, and it is two rows in `ripens_into` and one in
        // `min_growing_c` -- nothing in this file had to learn its name
        // to grow it. If this goes red and the wheat test above does not,
        // one of those rows is missing.
        let world = field(BLOCK_COTTON_SEEDS, true);
        let mut growth = Growth::seeded(3);
        grow_for(&mut growth, &world, &EvenSoil, CROP_STAGE_SECONDS * 1.3);
        assert_eq!(here(&world), BLOCK_COTTON_PLANT, "the cotton seed did not come up");
        grow_for(&mut growth, &world, &EvenSoil, CROP_STAGE_SECONDS * 1.3);
        assert_eq!(here(&world), BLOCK_COTTON_RIPE, "the cotton never opened its bolls");
        growth.on_block_changed(AT.0, AT.1, AT.2);
        growth.step(&world, &EvenSoil, 0.0, 64);
        assert_eq!(growth.ripening(), 0, "ripe cotton is still on the clock");
    }

    #[test]
    fn a_field_with_no_water_near_it_grows_two_and_a_half_times_slower() {
        // **The river, as one comparison.** The same seed, the same
        // soil, the same air; one has a channel beside it. A stage and a
        // third brings the watered one up and leaves the dry one in the
        // ground -- and the dry one is slow, not barren: given its two
        // and a half times, it comes up too.
        let grow = |water: bool, seconds: f32| {
            let world = field(BLOCK_SEEDS, water);
            let mut growth = Growth::seeded(7);
            grow_for(&mut growth, &world, &EvenSoil, seconds);
            here(&world)
        };
        assert_eq!(grow(true, CROP_STAGE_SECONDS * 1.3), BLOCK_WHEAT, "a watered field did not come up");
        assert_eq!(
            grow(false, CROP_STAGE_SECONDS * 1.3),
            BLOCK_SEEDS,
            "a field with no water near it grew as fast as one beside a channel"
        );
        assert_eq!(
            grow(false, CROP_STAGE_SECONDS * DRY_FIELD_FACTOR * 1.3),
            BLOCK_WHEAT,
            "a dry field never grew at all -- dry is meant to be slow, not barren"
        );
    }

    #[test]
    fn water_reaches_four_blocks_across_and_one_below_the_field_and_no_further() {
        // What a player has to be able to count on the ground. Earth at
        // the level of `AT` minus one, and water put down in one place
        // at a time.
        let wet = |dx: i32, dy: i32, dz: i32, what: BlockId| {
            let world = TestWorld::default();
            world.put(AT.0 + dx, AT.1 - 1 + dy, AT.2 + dz, what);
            watered(&world, AT.0, AT.1 - 1, AT.2)
        };
        assert!(wet(1, 0, 0, BLOCK_WATER), "a channel beside the furrow did not water it");
        assert!(wet(4, 0, 4, BLOCK_WATER), "the corner of the reach is dry -- it is a square, not a circle");
        assert!(wet(-4, -1, 0, BLOCK_WATER), "a river a step below its bank did not water the bank");
        assert!(!wet(5, 0, 0, BLOCK_WATER), "water five blocks off reached the field");
        assert!(!wet(0, -2, 1, BLOCK_WATER), "water two below the field reached it");
        assert!(!wet(1, 0, 0, BLOCK_ICE), "a frozen channel watered a field");
        assert!(!wet(1, 0, 0, BLOCK_STONE));
    }

    #[test]
    fn a_field_whose_channel_is_filled_in_slows_down() {
        // The water is looked at while the crop grows, not only when it
        // was sown: a player who fills the channel in has changed the
        // field, and a crop that kept the river's pace for the rest of
        // its stage would be a crop that remembered water it no longer
        // had.
        let world = world_with_a_planted_seed();
        let mut growth = Growth::seeded(7);
        growth.on_block_changed(AT.0, AT.1, AT.2);
        growth.step(&world, &EvenSoil, 0.0, 64);
        world.put(CHANNEL.0, CHANNEL.1, CHANNEL.2, BLOCK_DIRT);
        for _ in 0..40 {
            growth.step(&world, &EvenSoil, CROP_STAGE_SECONDS * 1.3 / 40.0, 64);
        }
        assert_eq!(
            here(&world),
            BLOCK_SEEDS,
            "the field kept a river's pace after its water was filled in"
        );
    }

    #[test]
    fn a_crop_in_air_too_cold_for_it_waits_and_then_carries_on() {
        // **Cold stops the clock; it does not reset it.** Three whole
        // stages of air a degree above freezing -- below wheat's four,
        // above the frost -- and the seed has not moved but is still
        // growing. Then the morning comes, and what is left of the stage
        // is all that is left: it does not start again from the sowing.
        let world = world_with_a_planted_seed();
        let air = Air::at(1.0);
        let mut growth = Growth::seeded(7);
        grow_for(&mut growth, &world, &air, CROP_STAGE_SECONDS * 3.0);
        assert_eq!(here(&world), BLOCK_SEEDS, "wheat grew in air colder than it can grow in");
        assert_eq!(growth.ripening(), 1, "a cold night took the crop off the clock");

        air.0.set(EvenSoil::AIR_C);
        for _ in 0..40 {
            growth.step(&world, &air, CROP_STAGE_SECONDS * 1.3 / 40.0, 64);
        }
        assert_eq!(here(&world), BLOCK_WHEAT, "the warm morning did not pick up where the night stopped");
    }

    #[test]
    fn cotton_wants_warmer_air_than_wheat_does() {
        // The decision cotton exists to make, as temperatures: a cool
        // day in the meadow grows wheat and does nothing for cotton, and
        // a warm one grows both.
        let grow = |crop: BlockId, degrees: f32| {
            let world = field(crop, true);
            let mut growth = Growth::seeded(9);
            grow_for(&mut growth, &world, &Air::at(degrees), CROP_STAGE_SECONDS * 1.3);
            here(&world)
        };
        assert_eq!(grow(BLOCK_SEEDS, 10.0), BLOCK_WHEAT, "wheat would not grow on a cool day");
        assert_eq!(grow(BLOCK_COTTON_SEEDS, 10.0), BLOCK_COTTON_SEEDS, "cotton grew on a cool day");
        assert_eq!(grow(BLOCK_COTTON_SEEDS, 24.0), BLOCK_COTTON_PLANT, "cotton would not grow on a warm one");
    }

    #[test]
    fn millet_sits_out_a_day_wheat_grows_on_and_outruns_wheat_on_a_hot_one() {
        // Millet's whole bargain as two temperatures: the meadow's cool day
        // grows wheat and leaves millet in the furrow, and on a hot day the
        // millet comes up in the time the wheat is still a seed.
        use primitive_shared::types::{BLOCK_MILLET, BLOCK_MILLET_PLANT};
        let grow = |crop: BlockId, degrees: f32, seconds: f32| {
            let world = field(crop, true);
            let mut growth = Growth::seeded(9);
            grow_for(&mut growth, &world, &Air::at(degrees), seconds);
            here(&world)
        };
        assert_eq!(grow(BLOCK_MILLET, 10.0, CROP_STAGE_SECONDS * 1.3), BLOCK_MILLET, "millet grew on a cool day");
        assert_eq!(grow(BLOCK_SEEDS, 10.0, CROP_STAGE_SECONDS * 1.3), BLOCK_WHEAT, "wheat would not grow on a cool day");
        let quick = CROP_STAGE_SECONDS * 0.8;
        assert_eq!(grow(BLOCK_MILLET, 24.0, quick), BLOCK_MILLET_PLANT, "millet was no quicker than wheat");
        assert_eq!(grow(BLOCK_SEEDS, 24.0, quick), BLOCK_SEEDS, "wheat kept up with millet");
    }

    #[test]
    fn frost_withers_a_growing_crop_and_spares_a_ripe_one() {
        // Every stage that is still growing dies in a frost and leaves
        // its dead stalks standing, told to the clients like any other
        // change -- and the dead stalks are on no clock of their own.
        for crop in [BLOCK_SEEDS, BLOCK_WHEAT, BLOCK_COTTON_SEEDS, BLOCK_COTTON_PLANT] {
            let name = primitive_shared::types::block_name(crop);
            let world = field(crop, true);
            let mut growth = Growth::seeded(5);
            growth.on_block_changed(AT.0, AT.1, AT.2);
            let changes = growth.step(&world, &Air::at(FROST_C - 3.0), 0.0, 64);
            assert_eq!(here(&world), BLOCK_WITHERED_CROP, "{name} lived through a frost");
            assert_eq!(changes.len(), 1, "the frost was not sent to anybody");
            assert_eq!(changes[0].block_id, BLOCK_WITHERED_CROP);
            growth.on_block_changed(AT.0, AT.1, AT.2);
            growth.step(&world, &EvenSoil, 0.0, 64);
            assert_eq!(growth.ripening(), 0, "the dead stalks of {name} are growing");
        }

        // A harvest already in is not a crop still growing.
        for ripe in [BLOCK_WHEAT_RIPE, BLOCK_COTTON_RIPE] {
            let world = field(ripe, true);
            let mut growth = Growth::seeded(5);
            grow_for(&mut growth, &world, &Air::at(FROST_C - 10.0), CROP_STAGE_SECONDS * 2.0);
            assert_eq!(
                here(&world),
                ripe,
                "frost took a ripe {} that was waiting to be reaped",
                primitive_shared::types::block_name(ripe)
            );
        }

        // ...and a cold night that stays above the line is a night, not
        // a frost.
        let world = world_with_a_planted_seed();
        let mut growth = Growth::seeded(5);
        grow_for(&mut growth, &world, &Air::at(FROST_C + 0.5), CROP_STAGE_SECONDS);
        assert_eq!(here(&world), BLOCK_SEEDS, "a chilly night above the frost line killed a crop");
    }

    #[test]
    fn every_stage_of_a_sown_crop_says_what_warmth_it_needs() {
        // `is_crop` is the warmth table, so a crop added without a row in
        // it would be a crop that ignores the water, the soil and the
        // frost alike. Anything that ripens and stands in tilled earth
        // and nowhere wilder is a sown crop; every one of those needs a
        // row, and nothing else may have one.
        for &(id, name) in ALL_BLOCK_IDS {
            let sown = ripens_into(id).is_some()
                && can_grow_on(id, BLOCK_FARMLAND)
                && !can_grow_on(id, BLOCK_GRASS);
            assert_eq!(
                sown,
                min_growing_c(id).is_some(),
                "{name} is {}a sown crop and {} a warmth it needs",
                if sown { "" } else { "not " },
                if min_growing_c(id).is_some() { "has" } else { "has no" },
            );
        }
    }

    #[test]
    fn a_crop_whose_field_was_dug_out_does_not_grow() {
        // The same guard the bush has: between the deadline being set
        // and it coming due, a player may have taken the ground away.
        let world = world_with_a_planted_seed();
        let mut growth = Growth::new();
        growth.on_block_changed(AT.0, AT.1, AT.2);
        growth.step(&world, &EvenSoil, 0.0, 64);
        world.put(AT.0, AT.1 - 1, AT.2, BLOCK_STONE);
        let changes = growth.step(&world, &EvenSoil, CROP_STAGE_SECONDS * 2.0, 64);
        assert!(changes.is_empty(), "it grew out of bare rock");
        assert_eq!(world.get(AT.0, AT.1, AT.2), BLOCK_SEEDS);
    }

    #[test]
    fn a_raided_hive_fills_again_a_comb_at_a_time_in_warm_air_and_not_in_the_cold() {
        use primitive_shared::bees::{hive_holding, honey_in, HIVE_FULL};
        // Hung in air on a trunk beside it, as the generator hangs one: the
        // cell under a hive is where the bees go in, and it must still fill.
        let three_waits = REGROW_SECONDS * (1.0 + REGROW_JITTER) * 3.3;
        for (air, fills) in [(20.0, true), (5.0, false)] {
            let world = TestWorld::default();
            world.put(AT.0 + 1, AT.1, AT.2, BLOCK_LOG);
            world.put(AT.0, AT.1, AT.2, hive_holding(0));
            let mut growth = Growth::seeded(9);
            // One wait: some honey and not all of it -- the comb at a time.
            grow_for(&mut growth, &world, &Air::at(air), REGROW_SECONDS * (1.0 + REGROW_JITTER) * 1.05);
            if fills {
                assert!(
                    (1..HIVE_FULL).contains(&honey_in(here(&world))),
                    "one wait left a hive holding {}",
                    honey_in(here(&world))
                );
            }
            grow_for(&mut growth, &world, &Air::at(air), three_waits);
            let honey = honey_in(here(&world));
            if fills {
                assert_eq!(honey, HIVE_FULL, "a raided hive did not fill in a warm season");
            } else {
                assert_eq!(honey, 0, "a hive filled while its bees could not fly");
            }
        }
    }

    /// A picked apple leaf in a canopy: more leaves under it, and nothing
    /// that is soil or water anywhere near.
    fn world_with_a_picked_apple_tree() -> TestWorld {
        let world = TestWorld::default();
        world.put(AT.0, AT.1 - 1, AT.2, BLOCK_APPLE_LEAVES);
        world.put(AT.0, AT.1, AT.2, BLOCK_APPLE_LEAVES_PICKED);
        world
    }

    #[test]
    fn a_picked_apple_tree_fruits_again_after_the_bushs_time_and_not_before() {
        // The two halves of "comes back": not while the clock is still
        // running, even at the soonest the jitter allows, and by the
        // latest it allows, fruit -- told to the clients like any other
        // change.
        let world = world_with_a_picked_apple_tree();
        let mut growth = Growth::seeded(21);
        growth.on_block_changed(AT.0, AT.1, AT.2);
        growth.step(&world, &EvenSoil, 0.0, 64);
        assert_eq!(growth.ripening(), 1, "a picked apple leaf is not on the clock");

        let soonest = REGROW_SECONDS * (1.0 - REGROW_JITTER) * 0.99;
        for _ in 0..40 {
            growth.step(&world, &EvenSoil, soonest / 40.0, 64);
        }
        assert_eq!(
            here(&world),
            BLOCK_APPLE_LEAVES_PICKED,
            "the apples were back before the bush's clock could have run"
        );

        let rest = (REGROW_SECONDS * (1.0 + REGROW_JITTER) - soonest) * 1.05;
        let mut changes = Vec::new();
        for _ in 0..40 {
            changes.extend(growth.step(&world, &EvenSoil, rest / 40.0, 64));
        }
        assert_eq!(here(&world), BLOCK_APPLE_LEAVES_FRUIT, "the picked tree never fruited again");
        assert_eq!(changes.len(), 1, "the fruit was not sent to anybody");
        assert_eq!(changes[0].block_id, BLOCK_APPLE_LEAVES_FRUIT);
    }

    #[test]
    fn a_picked_apple_tree_waits_out_the_cold_and_is_not_killed_by_frost() {
        // A winter's worth of hard frost: no fruit, no dead stalks, and
        // still on the clock. Then a cool day that is no frost but below
        // the fruit's warmth, which is still a wait. Then summer.
        let world = world_with_a_picked_apple_tree();
        let air = Air::at(FROST_C - 8.0);
        let mut growth = Growth::seeded(22);
        grow_for(&mut growth, &world, &air, REGROW_SECONDS * 3.0);
        assert_eq!(
            here(&world),
            BLOCK_APPLE_LEAVES_PICKED,
            "a picked apple tree fruited in a frost, or withered in one"
        );
        assert_eq!(growth.ripening(), 1, "a winter took the tree off the clock");

        air.0.set(FRUIT_SET_C - 1.0);
        grow_for(&mut growth, &world, &air, REGROW_SECONDS * 3.0);
        assert_eq!(here(&world), BLOCK_APPLE_LEAVES_PICKED, "apples set in air too cool to set them");

        air.0.set(EvenSoil::AIR_C);
        grow_for(&mut growth, &world, &air, REGROW_SECONDS * 1.3);
        assert_eq!(here(&world), BLOCK_APPLE_LEAVES_FRUIT, "the summer did not bring the apples back");
    }

    #[test]
    fn an_apple_leaf_that_never_bore_fruit_never_will() {
        // **The count a tree is grown with is the count it keeps.** The
        // sample walks the canopy round a player a cell at a time, and
        // when every bare apple leaf ripened it turned a tree of six
        // apples into a tree of forty. Told about the cell and sampled
        // round it for a long while, a plain leaf stays a leaf.
        let world = TestWorld::default();
        world.put(AT.0, AT.1, AT.2, BLOCK_APPLE_LEAVES);
        let mut growth = Growth::seeded(23);
        growth.watch(vec![(AT.0 as f32, AT.1 as f32, AT.2 as f32)]);
        growth.on_block_changed(AT.0, AT.1, AT.2);
        for _ in 0..400 {
            growth.step(&world, &EvenSoil, REGROW_SECONDS * 0.05, 64);
        }
        assert_eq!(growth.ripening(), 0, "a plain apple leaf went on the clock");
        assert_eq!(here(&world), BLOCK_APPLE_LEAVES, "a plain apple leaf grew apples");
    }

    fn world_with_a_picked_bush() -> TestWorld {
        let world = TestWorld::default();
        world.put(AT.0, AT.1 - 1, AT.2, BLOCK_GRASS);
        world.put(AT.0, AT.1, AT.2, BLOCK_BARE_BUSH);
        world
    }

    /// Runs enough ticks for anything with a deadline to reach it.
    fn wait_it_out(growth: &mut Growth, world: &TestWorld) -> Vec<BlockChange> {
        let mut changes = Vec::new();
        for _ in 0..40 {
            changes.extend(growth.step(world, &EvenSoil, REGROW_SECONDS * 0.1, 64));
        }
        changes
    }

    #[test]
    fn a_picked_bush_fills_again() {
        let world = world_with_a_picked_bush();
        let mut growth = Growth::seeded(1);
        growth.on_block_changed(AT.0, AT.1, AT.2);

        // Not immediately: a bush that came back the moment you picked
        // it would make the berries a button rather than a place.
        let soon = growth.step(&world, &EvenSoil, 1.0, 64);
        assert!(soon.is_empty(), "it came back at once");
        assert_eq!(world.get(AT.0, AT.1, AT.2), BLOCK_BARE_BUSH);

        let changes = wait_it_out(&mut growth, &world);
        assert_eq!(changes.len(), 1, "it never came back");
        assert_eq!(changes[0].block_id, BLOCK_BERRY_BUSH);
        assert_eq!(world.get(AT.0, AT.1, AT.2), BLOCK_BERRY_BUSH);
        assert_eq!(growth.ripening(), 0);
    }

    #[test]
    fn a_hedgerow_does_not_ripen_in_lockstep() {
        // Twenty bushes picked in one sweep have to come back over a
        // spread of time, or a world reads as a spreadsheet.
        let world = TestWorld::default();
        let mut growth = Growth::seeded(4242);
        for i in 0..20 {
            world.put(i, 20, 0, BLOCK_GRASS);
            world.put(i, 21, 0, BLOCK_BARE_BUSH);
            growth.on_block_changed(i, 21, 0);
        }
        growth.step(&world, &EvenSoil, 0.05, 64);
        assert_eq!(growth.ripening(), 20);

        // Step in slices and count how many distinct slices produced a
        // bush. If they were all on the same deadline this is one.
        let mut slices_with_a_bush = 0;
        for _ in 0..40 {
            if !growth.step(&world, &EvenSoil, REGROW_SECONDS * 0.05, 64).is_empty() {
                slices_with_a_bush += 1;
            }
        }
        assert!(
            slices_with_a_bush > 2,
            "all twenty came back in {slices_with_a_bush} slice(s)"
        );
    }

    #[test]
    fn the_sample_finds_a_bush_nobody_told_it_about() {
        // The restart case, and the reason the sample exists: a bush
        // picked before a shutdown would otherwise stay bare forever.
        let world = world_with_a_picked_bush();
        let mut growth = Growth::seeded(7);
        growth.watch(vec![(AT.0 as f32, AT.1 as f32, AT.2 as f32)]);

        // No notification at any point -- only the sample. Three
        // minutes of ticks: the sample is a safety net rather than a
        // search, and finding one particular cell in the volume around
        // a player is meant to take a while.
        for _ in 0..4000 {
            growth.step(&world, &EvenSoil, 0.05, 64);
            if growth.ripening() > 0 {
                break;
            }
        }
        assert_eq!(growth.ripening(), 1, "the sample never found it");
    }

    #[test]
    fn a_hard_frost_strips_every_berry_and_the_cold_keeps_it_bare() {
        // The winter as a hunter's season: a bush, a bilberry and a
        // strawberry in full fruit, a frost, and a player standing among
        // them. Everything drops, and nothing comes back while it is cold.
        let world = TestWorld::default();
        let plants = [
            (0, BLOCK_BERRY_BUSH, BLOCK_BARE_BUSH),
            (1, BLOCK_BILBERRY, BLOCK_BILBERRY_BARE),
            (2, BLOCK_STRAWBERRY, BLOCK_STRAWBERRY_BARE),
        ];
        for (x, fruit, _) in plants {
            world.put(x, 20, 0, BLOCK_GRASS);
            world.put(x, 21, 0, fruit);
        }
        let mut growth = Growth::seeded(11);
        growth.watch(vec![(1.0, 21.0, 0.0)]);
        let frost = Air::at(FROST_C - 1.0);
        for _ in 0..6000 {
            growth.step(&world, &frost, 0.05, 64);
        }
        for (x, fruit, bare) in plants {
            assert_eq!(world.get(x, 21, 0), bare, "{fruit} kept its berries through a frost");
        }
        // A whole regrowth in the cold, and still bare.
        for _ in 0..40 {
            growth.step(&world, &frost, REGROW_SECONDS * 0.1, 64);
        }
        for (x, _, bare) in plants {
            assert_eq!(world.get(x, 21, 0), bare, "a berry set again in the frost");
        }
    }

    #[test]
    fn a_cool_night_above_the_frost_line_leaves_the_berries_alone() {
        let world = TestWorld::default();
        world.put(0, 20, 0, BLOCK_GRASS);
        world.put(0, 21, 0, BLOCK_BERRY_BUSH);
        let mut growth = Growth::seeded(12);
        growth.watch(vec![(0.0, 21.0, 0.0)]);
        let chilly = Air::at(FROST_C + 3.0);
        for _ in 0..6000 {
            growth.step(&world, &chilly, 0.05, 64);
        }
        assert_eq!(world.get(0, 21, 0), BLOCK_BERRY_BUSH, "a bush dropped its berries above freezing");
    }

    #[test]
    fn the_sample_costs_nothing_when_nobody_is_playing() {
        // An empty server must not be doing block reads in a loop.
        let world = world_with_a_picked_bush();
        let mut growth = Growth::seeded(7);
        for _ in 0..100 {
            growth.step(&world, &EvenSoil, 0.05, 64);
        }
        assert_eq!(growth.ripening(), 0, "the sample ran with nobody watching");
    }

    #[test]
    fn a_bush_that_was_dug_up_does_not_come_back() {
        // Between the deadline and the moment it fires, a player can dig
        // the ground out or build through the cell. Writing a bush over
        // whatever is there instead is worse than losing the bush.
        let world = world_with_a_picked_bush();
        let mut growth = Growth::seeded(3);
        growth.on_block_changed(AT.0, AT.1, AT.2);
        growth.step(&world, &EvenSoil, 0.05, 64);
        assert_eq!(growth.ripening(), 1);

        world.put(AT.0, AT.1, AT.2, BLOCK_STONE);
        let changes = wait_it_out(&mut growth, &world);
        assert!(changes.is_empty(), "a bush grew inside a block of stone");
        assert_eq!(world.get(AT.0, AT.1, AT.2), BLOCK_STONE);
    }

    #[test]
    fn a_bush_left_hanging_in_the_air_does_not_come_back() {
        let world = world_with_a_picked_bush();
        let mut growth = Growth::seeded(3);
        growth.on_block_changed(AT.0, AT.1, AT.2);
        growth.step(&world, &EvenSoil, 0.05, 64);
        // The soil under it is gone.
        world.put(AT.0, AT.1 - 1, AT.2, BLOCK_STONE);
        let changes = wait_it_out(&mut growth, &world);
        assert!(changes.is_empty(), "a bush ripened on bare rock");
    }

    #[test]
    fn nothing_but_a_bare_bush_is_ever_queued() {
        let world = TestWorld::default();
        world.put(0, 20, 0, BLOCK_DIRT);
        world.put(0, 21, 0, BLOCK_BERRY_BUSH); // already full
        world.put(1, 21, 0, BLOCK_STONE);
        let mut growth = Growth::seeded(1);
        growth.on_block_changed(0, 21, 0);
        growth.on_block_changed(1, 21, 0);
        growth.on_block_changed(9, 9, 9); // air
        growth.step(&world, &EvenSoil, 0.05, 64);
        assert_eq!(growth.ripening(), 0);
    }

    #[test]
    fn the_queue_respects_its_budget() {
        let world = TestWorld::default();
        let mut growth = Growth::seeded(1);
        for i in 0..5000 {
            growth.on_block_changed(i, 30, 0);
        }
        growth.step(&world, &EvenSoil, 0.05, 32);
        assert_eq!(growth.pending(), 5000 - 32);
    }

    /// Turf a cell under `AT` and out to `size` either way, under open sky.
    fn a_lawn(size: i32) -> TestWorld {
        let world = TestWorld::default();
        for dz in -size..=size {
            for dx in -size..=size {
                world.put(AT.0 + dx, AT.1 - 1, AT.2 + dz, BLOCK_GRASS);
            }
        }
        world
    }

    /// Asks the plant at `from` to sow `times` times, and says what came up.
    fn sow(growth: &mut Growth, world: &TestWorld, soil: &dyn Soil, from: Cell, times: usize) -> Vec<BlockChange> {
        (0..times).filter_map(|_| growth.spread(world, soil, from)).collect()
    }

    #[test]
    fn a_tuft_of_grass_sows_the_open_turf_round_it() {
        let world = a_lawn(4);
        world.put(AT.0, AT.1, AT.2, BLOCK_TALL_GRASS);
        let mut growth = Growth::seeded(31);
        let sown = sow(&mut growth, &world, &EvenSoil, AT, 400);
        assert!(!sown.is_empty(), "four hundred tries and the grass sowed nothing");
        for change in &sown {
            assert_eq!(change.block_id, BLOCK_TALL_GRASS);
            assert!(
                (change.global_x - AT.0).abs() <= SPREAD_REACH && (change.global_z - AT.2).abs() <= SPREAD_REACH,
                "grass sowed itself {} columns off",
                (change.global_x - AT.0).abs().max((change.global_z - AT.2).abs())
            );
            assert_eq!(world.get(change.global_x, change.global_y - 1, change.global_z), BLOCK_GRASS);
        }
    }

    #[test]
    fn a_cut_meadow_fills_in_over_an_evening_and_stops_short_of_a_lawn() {
        // **What `CROWD` promises, as a field.** One tuft in the middle of a
        // bare lawn thirteen wide, and every plant asked to sow over and over:
        // the patch fills, and no five-by-five of it ever holds more than
        // `CROWD` -- so the thirteen-by-thirteen, which nine such patches
        // cover, holds nine times that at most. Never a carpet a player cannot
        // see the ground through.
        let world = a_lawn(6);
        world.put(AT.0, AT.1, AT.2, BLOCK_TALL_GRASS);
        let mut growth = Growth::seeded(32);
        for _ in 0..60 {
            for dz in -6..=6 {
                for dx in -6..=6 {
                    let at = (AT.0 + dx, AT.1, AT.2 + dz);
                    if world.get(at.0, at.1, at.2) == BLOCK_TALL_GRASS {
                        growth.spread(&world, &EvenSoil, at);
                    }
                }
            }
        }
        let tufts = (-6..=6)
            .flat_map(|dz| (-6..=6).map(move |dx| (dx, dz)))
            .filter(|&(dx, dz)| world.get(AT.0 + dx, AT.1, AT.2 + dz) == BLOCK_TALL_GRASS)
            .count();
        assert!(tufts >= 169 / 8, "a whole season of sowing left {tufts} tufts on 169 cells of turf");
        assert!(tufts <= 9 * CROWD as usize, "the meadow grew into a lawn: {tufts} tufts on 169 cells");
        // ...and the promise itself, patch by patch.
        for cz in -4..=4 {
            for cx in -4..=4 {
                let patch = (-2..=2)
                    .flat_map(|dz| (-2..=2).map(move |dx| (dx, dz)))
                    .filter(|&(dx, dz)| world.get(AT.0 + cx + dx, AT.1, AT.2 + cz + dz) == BLOCK_TALL_GRASS)
                    .count();
                assert!(patch <= CROWD as usize, "a five-by-five of the meadow holds {patch} tufts");
            }
        }
    }

    #[test]
    fn nothing_is_sown_under_a_roof_beside_a_players_floor_or_into_a_frost() {
        let tuft = || {
            let world = a_lawn(4);
            world.put(AT.0, AT.1, AT.2, BLOCK_TALL_GRASS);
            world
        };
        // A slab of stone overhead: a cave, a house.
        let roofed = tuft();
        for dz in -4..=4 {
            for dx in -4..=4 {
                roofed.put(AT.0 + dx, AT.1 + 3, AT.2 + dz, BLOCK_STONE);
            }
        }
        let mut growth = Growth::seeded(33);
        assert!(sow(&mut growth, &roofed, &EvenSoil, AT, 400).is_empty(), "grass grew under a roof");

        // Planks laid in the turf round the tuft: nothing comes up against them.
        let floored = tuft();
        let planks = [(1, 1), (-2, 0), (0, -2), (2, -1)];
        for (dx, dz) in planks {
            floored.put(AT.0 + dx, AT.1 - 1, AT.2 + dz, primitive_shared::types::BLOCK_PLANKS);
        }
        for change in sow(&mut growth, &floored, &EvenSoil, AT, 400) {
            for (dx, dz) in planks {
                assert!(
                    (change.global_x - AT.0 - dx).abs() > 1 || (change.global_z - AT.2 - dz).abs() > 1,
                    "grass came up against a player's floor at ({}, {})",
                    change.global_x,
                    change.global_z
                );
            }
        }

        // A hard frost: not a blade.
        let frozen = tuft();
        assert!(sow(&mut growth, &frozen, &Air::at(0.0), AT, 400).is_empty(), "grass was sown in a frost");
    }

    #[test]
    fn a_fern_spreads_in_the_shade_of_a_canopy_and_grass_does_not() {
        // The forest floor's rule, from the growth side: what the generator
        // plants under a wood is what grows back there.
        let shaded = |plant: BlockId| {
            let world = a_lawn(4);
            for dz in -6..=6 {
                for dx in -6..=6 {
                    world.put(AT.0 + dx, AT.1 + 5, AT.2 + dz, BLOCK_LEAVES);
                }
            }
            world.put(AT.0, AT.1, AT.2, plant);
            let mut growth = Growth::seeded(34);
            sow(&mut growth, &world, &EvenSoil, AT, 400).len()
        };
        assert_eq!(shaded(BLOCK_TALL_GRASS), 0, "grass spread under a closed canopy");
        assert!(shaded(BLOCK_FERN) > 0, "a fern would not spread in the shade it lives in");
        // ...and a fern in the open stays where it is.
        let world = a_lawn(4);
        world.put(AT.0, AT.1, AT.2, BLOCK_FERN);
        let mut growth = Growth::seeded(35);
        assert!(sow(&mut growth, &world, &EvenSoil, AT, 400).is_empty(), "a fern spread into full sun");
    }

    #[test]
    fn a_berry_sows_a_picked_shrub_and_a_stand_stays_a_stand() {
        let world = a_lawn(4);
        world.put(AT.0, AT.1, AT.2, BLOCK_STRAWBERRY);
        let mut growth = Growth::seeded(36);
        let sown = sow(&mut growth, &world, &EvenSoil, AT, 2000);
        assert!(!sown.is_empty(), "a wild strawberry never spread");
        assert!(sown.iter().all(|c| c.block_id == BLOCK_STRAWBERRY_BARE), "a sown strawberry came up in fruit");
        assert!(sown.len() < KIN, "one strawberry sowed a patch of {} -- a larder, not a find", sown.len() + 1);
    }

    #[test]
    fn a_nettle_shoot_grows_into_two_cells_and_waits_under_a_ceiling() {
        use primitive_shared::types::{plant_shoot, PLANT_TOP};
        let open = a_lawn(1);
        open.put(AT.0, AT.1, AT.2, plant_shoot(BLOCK_NETTLE));
        let mut growth = Growth::seeded(37);
        let changes = grow_for(&mut growth, &open, &EvenSoil, REGROW_SECONDS * 1.4);
        assert_eq!(here(&open), BLOCK_NETTLE, "the shoot never grew");
        assert_eq!(open.get(AT.0, AT.1 + 1, AT.2), BLOCK_NETTLE | PLANT_TOP, "a nettle grew without its upper half");
        assert!(changes.iter().any(|c| c.block_id == BLOCK_NETTLE | PLANT_TOP), "the upper half was not sent");

        let under = a_lawn(1);
        under.put(AT.0, AT.1, AT.2, plant_shoot(BLOCK_NETTLE));
        under.put(AT.0, AT.1 + 1, AT.2, BLOCK_STONE);
        let mut growth = Growth::seeded(38);
        grow_for(&mut growth, &under, &EvenSoil, REGROW_SECONDS * 1.4);
        assert_eq!(here(&under), plant_shoot(BLOCK_NETTLE), "a shoot under a stone grew anyway");
        assert_eq!(under.get(AT.0, AT.1 + 1, AT.2), BLOCK_STONE, "a nettle grew through a stone");

        // ...and not in the cold: a shoot comes up in spring.
        let cold = a_lawn(1);
        cold.put(AT.0, AT.1, AT.2, plant_shoot(BLOCK_NETTLE));
        let mut growth = Growth::seeded(39);
        grow_for(&mut growth, &cold, &Air::at(GREEN_UP_C - 3.0), REGROW_SECONDS * 3.0);
        assert_eq!(here(&cold), plant_shoot(BLOCK_NETTLE), "a shoot grew in air too cold to green in");
    }

    #[test]
    fn a_picked_bilberry_fruits_again_in_warm_air_and_waits_out_the_cold() {
        let world = a_lawn(1);
        world.put(AT.0, AT.1, AT.2, BLOCK_BILBERRY_BARE);
        let air = Air::at(FRUIT_SET_C - 2.0);
        let mut growth = Growth::seeded(40);
        grow_for(&mut growth, &world, &air, REGROW_SECONDS * 2.0);
        assert_eq!(here(&world), BLOCK_BILBERRY_BARE, "bilberries set in air too cool for fruit");
        air.0.set(EvenSoil::AIR_C);
        grow_for(&mut growth, &world, &air, REGROW_SECONDS * 1.4);
        assert_eq!(here(&world), BLOCK_BILBERRY, "a picked bilberry never fruited again");
    }

    #[test]
    fn a_tree_drops_a_seedling_into_a_gap_and_the_seedling_grows_in_its_parents_bark() {
        // A birch leaf high over a wide lawn: the seedling that comes up is a
        // birch sapling standing on turf under open sky, it is on the young
        // tree's clock at once, and it grows into a birch.
        let world = a_lawn(10);
        let leaf = (AT.0, AT.1 + 9, AT.2);
        world.put(leaf.0, leaf.1, leaf.2, BLOCK_BIRCH_LEAVES);
        let mut growth = Growth::seeded(41);
        let mut planted = Vec::new();
        for _ in 0..50 {
            planted = growth.seed_tree(&world, &EvenSoil, leaf);
            if !planted.is_empty() {
                break;
            }
        }
        let foot = planted
            .iter()
            .filter(|c| is_branch(c.block_id))
            .min_by_key(|c| c.global_y)
            .expect("fifty tries and no seedling in an open lawn");
        let root = (foot.global_x, foot.global_y, foot.global_z);
        assert_eq!(world.get(root.0, root.1 - 1, root.2), BLOCK_GRASS, "a seedling rooted in something but turf");
        assert!(planted.iter().filter(|c| is_branch(c.block_id)).all(|c| is_birch_wood(c.block_id)), "a birch dropped an oak");
        assert!(young_root(&world, root), "the seedling is not a young tree the clock can grow");

        growth.step(&world, &EvenSoil, 0.0, 64);
        for _ in 0..(TREE_STAGES as usize + 2) {
            growth.step(&world, &EvenSoil, TREE_STAGE_SECONDS * 1.3, 64);
        }
        let foot = world.get(root.0, root.1, root.2);
        assert!(
            branch_width(foot).is_some_and(|w| w >= GROWN_FOOT) && is_birch_wood(foot),
            "the seedling grew into {} and not a grown birch",
            primitive_shared::types::block_name(foot)
        );
    }

    #[test]
    fn a_seedling_never_comes_up_under_the_crowns_or_in_a_frost() {
        let world = a_lawn(10);
        for dz in -10..=10 {
            for dx in -10..=10 {
                world.put(AT.0 + dx, AT.1 + 9, AT.2 + dz, BLOCK_LEAVES);
            }
        }
        let mut growth = Growth::seeded(42);
        for _ in 0..200 {
            assert!(growth.seed_tree(&world, &EvenSoil, (AT.0, AT.1 + 9, AT.2)).is_empty(), "a seedling grew in deep shade");
        }
        let open = a_lawn(10);
        open.put(AT.0, AT.1 + 9, AT.2, BLOCK_LEAVES);
        for _ in 0..200 {
            assert!(growth.seed_tree(&open, &Air::at(0.0), (AT.0, AT.1 + 9, AT.2)).is_empty(), "a seedling grew in a frost");
        }
    }

    #[test]
    fn bare_earth_beside_turf_greens_in_the_open_and_not_indoors() {
        let dug = (AT.0, AT.1 - 1, AT.2);
        let world = a_lawn(2);
        world.put(dug.0, dug.1, dug.2, BLOCK_DIRT);
        assert!(green_up(&world, &EvenSoil, dug).is_some(), "a dug patch in a meadow never greened");
        assert_eq!(world.get(dug.0, dug.1, dug.2), BLOCK_GRASS);

        let indoors = a_lawn(2);
        indoors.put(dug.0, dug.1, dug.2, BLOCK_DIRT);
        indoors.put(AT.0, AT.1 + 2, AT.2, primitive_shared::types::BLOCK_PLANKS);
        assert!(green_up(&indoors, &EvenSoil, dug).is_none(), "an earth floor under a roof grew turf");

        let alone = TestWorld::default();
        alone.put(dug.0, dug.1, dug.2, BLOCK_DIRT);
        assert!(green_up(&alone, &EvenSoil, dug).is_none(), "earth with no turf beside it greened out of nothing");
    }

    #[test]
    fn a_birch_sapling_grows_into_a_birch_in_white_bark() {
        let world = TestWorld::default();
        for dz in -7..=7 {
            for dx in -7..=7 {
                world.put(AT.0 + dx, AT.1 - 1, AT.2 + dz, BLOCK_GRASS);
            }
        }
        let variant = young_tree_variant(AT.0, AT.2);
        for ((dx, dy, dz), id) in tree_stage_cells(0, variant, BLOCK_BIRCH_LEAVES, |_, _| 0).unwrap() {
            world.put(AT.0 + dx, AT.1 - 1 + dy, AT.2 + dz, id);
        }
        // Every leaf picked off first: the bark alone has to say "birch".
        for dy in 0..8 {
            for dz in -2..=2 {
                for dx in -2..=2 {
                    if world.get(AT.0 + dx, AT.1 + dy, AT.2 + dz) == BLOCK_BIRCH_LEAVES {
                        world.put(AT.0 + dx, AT.1 + dy, AT.2 + dz, BLOCK_AIR);
                    }
                }
            }
        }
        let mut growth = Growth::seeded(43);
        growth.on_block_changed(AT.0, AT.1, AT.2);
        growth.step(&world, &EvenSoil, 0.0, 64);
        for _ in 0..(TREE_STAGES as usize + 2) {
            growth.step(&world, &EvenSoil, TREE_STAGE_SECONDS * 1.3, 64);
        }
        let foot = world.get(AT.0, AT.1, AT.2);
        assert!(branch_width(foot).is_some_and(|w| w >= GROWN_FOOT), "a birch sapling stopped growing");
        let oak = (0..20)
            .flat_map(|dy| (-4..=4).flat_map(move |dz| (-4..=4).map(move |dx| (dx, dy, dz))))
            .filter(|&(dx, dy, dz)| {
                let b = world.get(AT.0 + dx, AT.1 + dy, AT.2 + dz);
                is_branch(b) && !is_birch_wood(b)
            })
            .count();
        assert_eq!(oak, 0, "a birch grew {oak} pieces of oak");
    }

    #[test]
    fn one_bush_is_only_queued_once() {
        // A player who clicks a bare bush repeatedly must not be able to
        // pile up deadlines for it -- or, worse, reset the one it has.
        let world = world_with_a_picked_bush();
        let mut growth = Growth::seeded(9);
        for _ in 0..50 {
            growth.on_block_changed(AT.0, AT.1, AT.2);
        }
        growth.step(&world, &EvenSoil, 0.05, 64);
        assert_eq!(growth.ripening(), 1);
    }
}
