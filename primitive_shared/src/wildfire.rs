//! Fire that gets loose, the smoke a fire makes indoors, and the torch you
//! stand up: the rules both sides have to agree on. The clocks are the
//! server's (`logic::wildfire` there); what is written here is what a block
//! does when it burns, how far a flame reaches, what fills a room and what
//! a sooty ceiling looks like.
//!
//! ## What the player asked for
//!
//! > «Обугленные текстуры для дерева и возможность его возгорания (например,
//! > костёр случайно поджигает доски).»
//!
//! and, in the same report, smoke that fills a room and soot on the ceiling
//! over a fire. Every one of those is a reason to think about *where* a
//! fire goes, which is the whole of what they are for: a hearth against a
//! plank wall is a risk, a hearth under a roof with no hole in it is a room
//! nobody can sleep in, and a hearth under a stone vault is a black ceiling
//! in a month and nothing worse.
//!
//! ## How a block catches: heat soaks in, and there are no dice
//!
//! The first design was TerraFirmaCraft's and Minecraft's: a chance a
//! second that a flame beside wood sets it alight. It was rejected for the
//! reason `pit` refuses TerraFirmaCraft's firepit dice -- a roll a player
//! cannot see is a roll they cannot plan around. "Is my wall safe beside this
//! hearth" has to have an answer, and with a chance a second the answer is
//! "for a while, probably", which is not one.
//!
//! What is here instead is **heat that soaks in**. Every cell of fuel a
//! flame licks gathers heat at a rate ([`HEARTH_HEAT`], [`BLAZE_HEAT`]),
//! more above a flame than beside it and more downwind than up, and loses it
//! again at [`COOLING_PER_SECOND`] when nothing is licking it. When what it
//! has gathered reaches what that fuel needs ([`Fuel::catch_heat`]) it is
//! alight. So a plank wall beside a campfire catches in about a minute and a
//! log wall in two and a half; a fire moved away after thirty seconds leaves
//! the boards warm, and they cool. A player who sees the wall start to smoke
//! -- the server says so -- has the rest of that minute to move the fire.
//!
//! **Not a perfect wave, though**: each cell's threshold is moved up or down
//! by a quarter by a hash of where it is ([`catch_threshold`]), so a burning
//! wall does not catch plank by plank in lockstep. The hash is of the place,
//! not of the moment, so the same wall catches in the same order every time
//! -- which is still a rule, just not a grid.
//!
//! ## What burns, and what it leaves
//!
//! * **Logs** of any wood, lying or standing ([`Fuel::Log`]): alight as
//!   `BLOCK_BURNING_LOG` for [`LOG_BURN_SECONDS`], then `BLOCK_CHARRED_LOG`.
//! * **Boards**, pegged or not ([`Fuel::Boards`]): `BLOCK_BURNING_PLANKS`,
//!   then `BLOCK_CHARRED_PLANKS`.
//! * **Leaves** ([`Fuel::Leaves`]) and **wool** ([`Fuel::Tinder`]): gone at
//!   once, and the cell stays hot for [`FLASH_SECONDS`] -- a canopy over a
//!   campfire goes up in a breath and takes the tree's crown with it.
//! * **Roofs** burn as what they are made of: a thatch is tinder, a roof of
//!   branches is leaves, a plank step is boards. Tiles do not burn, which is
//!   the reason to fire them. See [`fuel`].
//! * **The undergrowth** ([`Fuel::Grass`]) -- every tuft, fern, crop and
//!   bush -- goes in six seconds and leaves ash where it stood. It is the
//!   floor of the world and it is continuous, which is what makes a
//!   burning tree into a burning wood: the fire runs along the ground with
//!   the wind and climbs the next trunk it reaches. **How fast it catches
//!   is the year's answer** ([`seasoning`]): three fifths of the heat in
//!   high summer, half again as much at midwinter. Green timber and
//!   anything the rain has soaked take four times as long
//!   ([`damp_factor`]), which is the reason a wet wall is worth building.
//!
//! Char does not burn again. Neither does a hearth, a chest, a tool or
//! furniture: a table beside a fire is not what the player asked about, and
//! a model with no row of its own to burn into would need one per piece.
//!
//! ## Bounded, because a world is not kindling
//!
//! A wildfire that ate a forest while its player was away would be the
//! world punishing somebody for a campfire. Four limits, each a number:
//!
//! * only within [`NEAR_PLAYER`] blocks of somebody does anything soak,
//!   catch or burn down -- a fire nobody is near waits, like a crop;
//! * at most [`MAX_CATCHES_PER_STEP`] cells catch in a second;
//! * at most [`MAX_BURNING`] cells burn at once, and a fire at the cap
//!   spreads no further until something burns out;
//! * **the rain puts out** what it can reach: a burning block under open
//!   sky in rain goes out as char, and fuel under open sky in rain gathers
//!   no heat.
//!
//! ## Smoke, and why it is a room and not a cloud
//!
//! Real smoke would be a fluid. What a player needs to know about it is
//! simpler: **is this room vented**. So a fire's smoke is worked out as the
//! air it can reach going sideways and up ([`smoke_room`]) -- a flood fill
//! of at most [`ROOM_MAX_CELLS`] cells, and only when asked (every
//! [`SMOKE_STEP_SECONDS`]). Air that reaches [`ROOM_MAX_RISE`] blocks over
//! the fire, or more cells than that, has found the sky: the fire is vented
//! and there is no smoke. Air that does not is a room, and the room fills
//! towards a thickness set by how big it is ([`smoke_target`]). A window, a
//! door left open or a hole in the roof is a vent, which is the decision:
//! warmth kept in, or air let in.
//!
//! The rejected way was a smoke value per cell, spreading like water. It is
//! what a fluid simulation would do and it would be right in more places --
//! a long hall smoky at one end -- and it is a map of every cell of every
//! house near every fire, kept up to date every tick, to tell a player one
//! thing a whole room already tells them.
//!
//! ## Soot, and why it is on the block
//!
//! The ceiling straight over a fire -- the first block within
//! [`ROOM_MAX_RISE`] that the sky cannot pass -- blackens in three stages,
//! one every [`SOOT_STAGE_SECONDS`] of burning under it. The stage lives in
//! the block's variant field ([`with_soot`]), so it is saved, sent and meshed
//! for nothing, and it is drawn darker by the client's tint. Only on the
//! materials a ceiling is made of ([`may_carry_soot`]); a log's variant is
//! its axis, and a thatch of leaves is not a ceiling.

use crate::types::{
    block_axis, block_kind, blocks_the_sky, is_leafy, oriented, BlockId, BLOCK_AIR, BLOCK_BIRCH_PLANKS, BLOCK_BRICKS,
    BLOCK_BURNING_LOG, BLOCK_BURNING_PLANKS, BLOCK_CAMPFIRE_LIT, BLOCK_CHARRED_LOG,
    BLOCK_CHARRED_PLANKS, BLOCK_COBBLESTONE, BLOCK_FIREPIT_LIT, BLOCK_PLANKS, BLOCK_STANDING_TORCH, BLOCK_STANDING_TORCH_LIT,
    BLOCK_STANDING_TORCH_OUT, BLOCK_WOOL, VARIANT_MASK, VARIANT_SHIFT,
};

/// How often the spread is worked out, in seconds.
///
/// Once a second, not once a tick. Heat soaking into a wall over a minute
/// does not need twenty looks a second, and the cost of the step is the
/// cells round every flame near every player.
pub const STEP_SECONDS: f32 = 1.0;

/// Heat a hearth's flame gives each fuel cell it licks, a second.
///
/// One, so a threshold reads as seconds of a campfire against it.
pub const HEARTH_HEAT: f32 = 1.0;

/// ...and what a block that is itself alight gives. Three times a hearth:
/// a burning wall is a great deal more fire than a ring of stones with
/// sticks in it, which is why a fire that has caught spreads faster than
/// the campfire that started it.
pub const BLAZE_HEAT: f32 = 3.0;

/// How much more the cell over a flame gathers than a cell beside it.
/// Heat rises; a shelf over a hearth is in more danger than the wall
/// behind it.
pub const RISING_FACTOR: f32 = 1.5;

/// How much the wind can add to or take from a cell downwind or upwind of
/// a flame, at a full gale (`raft::Wind::strength` of one).
///
/// Downwind at a gale a cell gathers two and a half times as fast, upwind
/// a tenth as fast (see [`draught`]). That is what makes the wind a thing to
/// look at before lighting a fire beside a wall -- the same wind that fills
/// a sail.
pub const WIND_FACTOR: f32 = 1.5;

/// How fast a warm cell cools when nothing is licking it, a second.
///
/// Twice the rate a hearth warms it, so a wall that had a fire beside it
/// for half a minute is cold again in a quarter of one: a fire moved in
/// time is a fire that did nothing.
pub const COOLING_PER_SECOND: f32 = 2.0;

/// How long a trunk burns before it is char, in seconds. Three minutes: a
/// log house on fire is a fire a player can fight -- break the burning logs
/// out, or let it go and save what is inside.
pub const LOG_BURN_SECONDS: f32 = 180.0;

/// ...and boards. Half a trunk's time, because boards are thin.
pub const BOARDS_BURN_SECONDS: f32 = 90.0;

/// How long the cell a leaf or a tuft of wool was in stays hot after it has
/// gone, in seconds. A flash, which is what a canopy going up is: long
/// enough to light the next leaf, not long enough to be a fire.
pub const FLASH_SECONDS: f32 = 4.0;

/// How close to a player anything happens, in blocks, horizontally.
///
/// Sixty-four, the radius a player can see smoke from (the particles'
/// reach) and more than a view distance of four chunks. A fire beyond it
/// is frozen as it is -- neither spreading nor burning down -- the way a
/// crop nobody is near does not grow.
pub const NEAR_PLAYER: i32 = 64;

/// The most cells that may catch in one step. See the module note.
pub const MAX_CATCHES_PER_STEP: usize = 8;

/// The most cells alight at once, over the whole world.
pub const MAX_BURNING: usize = 256;

/// The most cells the server keeps warming at once. Past it, a newly licked
/// cell gathers nothing until one cools off the list: a bound on memory,
/// never met by a hearth beside a wall.
pub const MAX_WARM_CELLS: usize = 4096;

/// What a block is, as something that burns.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Fuel {
    /// A trunk of any wood.
    Log,
    /// Boards of any wood, pegged or not.
    Boards,
    /// A canopy's leaves.
    Leaves,
    /// Wool: air held in a fleece, gone in a breath.
    Tinder,
    /// A rotten board (`weathering::is_rotten`): wet punk that smoulders a
    /// long while before it takes, and then burns as a board does.
    Punk,
    /// **Standing grass and the undergrowth**: a tuft, a fern, a crop, a
    /// berry bush -- anything a knife cuts that stands in its cell as two
    /// crossed planes.
    ///
    /// This is the one that makes a fire a *fire* rather than an accident
    /// with a wall. Everything else that burns is something a player
    /// built or a tree they walked under; grass is the floor of the
    /// world, it is continuous, and once it is alight the wind decides
    /// where the fire goes. It leaves ash where it stood, which is the
    /// mark a burnt-over meadow carries for a season and the dressing a
    /// field wants (`dressed`).
    Grass,
}

impl Fuel {
    /// How much heat a cell of it gathers before it is alight. See
    /// `HEARTH_HEAT` for the unit: a campfire's seconds.
    pub fn catch_heat(self) -> f32 {
        match self {
            // Two and a half minutes of a campfire against a log wall.
            Fuel::Log => 150.0,
            // A minute against boards.
            Fuel::Boards => 60.0,
            // Ten seconds of a campfire under a canopy. A tree over a
            // hearth is a tree on fire, and the player finds that out
            // quickly enough to learn it rather than slowly enough to miss.
            Fuel::Leaves => 10.0,
            Fuel::Tinder => 15.0,
            // A log's wait. Rot is soft, not dry: what has rotted a board
            // is the water still in it.
            Fuel::Punk => 150.0,
            // **Six seconds**, and the fastest thing in the table on
            // purpose. Dry grass is what a spark finds first: a campfire
            // left in a meadow in high summer has the tuft beside it
            // alight before a player has walked ten paces, which is the
            // whole of the "untended fire" the player asked for. In
            // midwinter the same tuft takes nearly three times as long
            // (`seasoning`), and wet (`damp_factor`) it does not get
            // there at all before whatever was licking it has burnt out.
            Fuel::Grass => 6.0,
        }
    }

    /// How long it burns, or `None` for what flashes away at once.
    pub fn burn_seconds(self) -> Option<f32> {
        match self {
            Fuel::Log => Some(LOG_BURN_SECONDS),
            Fuel::Boards | Fuel::Punk => Some(BOARDS_BURN_SECONDS),
            Fuel::Leaves | Fuel::Tinder | Fuel::Grass => None,
        }
    }
}

/// What this block is as fuel, or `None` if fire does nothing to it.
///
/// **By what the block is, not by a list of ids**, wherever that can be
/// said: a trunk is anything whose row is wood, lies along an axis and
/// fills its cell -- which is every log of every wood there is or will be,
/// and a stripped one. Boards are anything that can be pegged or has been
/// (`types::pegged_form`), which is every plank. A new wood is fuel the
/// moment it has those.
pub fn fuel(block: BlockId) -> Option<Fuel> {
    use crate::blocks::{definition, Shape, Work};
    let kind = block_kind(block);
    if matches!(
        kind,
        BLOCK_BURNING_LOG | BLOCK_BURNING_PLANKS | BLOCK_CHARRED_LOG | BLOCK_CHARRED_PLANKS
    ) {
        return None;
    }
    let def = definition(block);
    if def.work == Work::Wood
        && def.orientable
        && def.shape == Shape::Cube
        && def.thickness == crate::types::LAYERS_PER_BLOCK
        && !def.container
        && def.tool.is_none()
    {
        return Some(Fuel::Log);
    }
    if crate::weathering::is_rotten(block) {
        return Some(Fuel::Punk);
    }
    if crate::types::pegged_form(block).is_some()
        || crate::types::is_pegged(block)
        || matches!(kind, BLOCK_PLANKS | BLOCK_BIRCH_PLANKS)
    {
        return Some(Fuel::Boards);
    }
    // **A roof burns as what it was made of.** Plank steps are boards --
    // and burn down into the burning plank's whole cell, a step's shape
    // given up for the fire's, because a second burning block per shape is
    // four more rows for a thing that lasts ninety seconds. A roof of
    // branches is the canopy it was cut from, and thatch is dry grass: it
    // takes from a spark as wool does and is gone in a breath, which is the
    // whole of why a thatched roof over a hearth is a decision and a tiled
    // one is not. Tiles and cobbles are not here: fired clay and stone.
    if kind == crate::types::BLOCK_PLANK_STAIRS {
        return Some(Fuel::Boards);
    }
    if matches!(kind, crate::types::BLOCK_BRANCH_ROOF | crate::types::BLOCK_BRANCH_SLAB) {
        return Some(Fuel::Leaves);
    }
    if matches!(kind, crate::types::BLOCK_THATCH_ROOF | crate::types::BLOCK_THATCH_SLAB) {
        return Some(Fuel::Tinder);
    }
    if is_leafy(block) {
        return Some(Fuel::Leaves);
    }
    // Wool, and the leaves lying on a wood's floor: a fire that reaches
    // them runs along the ground. See `types::BLOCK_LEAF_LITTER`.
    if kind == BLOCK_WOOL || kind == crate::types::BLOCK_LEAF_LITTER {
        return Some(Fuel::Tinder);
    }
    // **The undergrowth, by what it is**: work a knife does, standing in
    // the cell as two crossed planes. That is every tuft, every fern,
    // every standing crop and every berry bush there is or will be --
    // and it is the floor a fire runs across, which is what turns a
    // burning tree into a burning wood.
    //
    // **Except what stands in water** (`types::stands_in_water`): kelp,
    // reeds in the shallows, a waterlily. They are the same two planes
    // and a knife cuts them, and a burning lake is the one thing a fire
    // must never be. The test for it is the sea's own, so a plant added
    // to the shallows tomorrow is exempt without anybody remembering to
    // exempt it.
    if def.work == Work::Plant && def.shape == Shape::Cross && !crate::types::stands_in_water(block) {
        return Some(Fuel::Grass);
    }
    None
}

/// Does a cell of this fuel leave ash behind where it stood?
///
/// Only the undergrowth, and the reason is where the ash would land.
/// Grass stands on the ground, so its ash lies on the ground -- a burnt
/// meadow is grey for as long as nobody digs it in, which is both the
/// scar a player reads and the dressing a field wants (`dressed`). A
/// canopy's leaves are six blocks up over more leaves; ash hanging in
/// the air there would be a grey block floating in a tree.
#[inline]
pub fn leaves_ash(fuel: Fuel) -> bool {
    matches!(fuel, Fuel::Grass)
}

/// How much longer fuel takes to catch in the season the world is in.
///
/// **A multiplier on every threshold, and the reason a fire is a summer
/// problem.** The same campfire in the same meadow is a fire hazard in
/// August and a warm place to sit in March; a player who has learned
/// that has learned something about the world rather than about a
/// number. It reads the season off the world's own clock
/// (`season::ambient_offset_c`) rather than rolling for a "dryness",
/// because a dry spell that arrived on a coin is one nobody can plan
/// around -- the same argument `pit` makes against firepit dice and
/// `weather` makes against a season the sky rolls for.
///
/// Rejected: a wetness that falls with every shower and climbs back over
/// days. It is the honest model and it is a second climate to keep, save
/// and send, for a factor between two thirds and one and a half. The
/// rain already puts fires out and wets what it falls on
/// (`rained_on`); what was missing was the year, and the year is free.
pub fn seasoning(world_days: f32) -> f32 {
    // The offset runs from the winter trough to the summer peak; map it
    // to the two ends and lerp. Summer needs three fifths of the heat
    // that midwinter does, which is the difference between "the grass
    // caught" and "the grass smoked and went out".
    let offset = crate::season::ambient_offset_c(world_days);
    let span = crate::season::SUMMER_PEAK_C - crate::season::WINTER_TROUGH_C;
    let t = ((offset - crate::season::WINTER_TROUGH_C) / span).clamp(0.0, 1.0);
    WET_SEASON + (DRY_SEASON - WET_SEASON) * t
}

/// What high summer takes of the heat a cell needs to catch.
pub const DRY_SEASON: f32 = 0.6;
/// ...and what midwinter asks instead.
pub const WET_SEASON: f32 = 1.6;

/// How much longer fuel that is *itself* wet takes to catch.
///
/// Green timber and anything the rain has soaked (`wood::is_green`,
/// `wet::is_wet`): four times, which for a log is ten minutes of a
/// campfire against it and in practice means it does not catch at all
/// before the fire that was licking it has burnt out. It is the reason
/// to build the wet wall and the reason a sodden roof is worth having --
/// and it is the same fact the hearths already know, where green fuel
/// smokes instead of burning clean (`SMOULDER_SMOKE`).
pub fn damp_factor(block: BlockId) -> f32 {
    if crate::wet::is_wet(block) || crate::wood::is_green(block) {
        DAMP
    } else {
        1.0
    }
}

/// See `damp_factor`.
pub const DAMP: f32 = 4.0;

/// What a cell of fuel becomes the moment it catches: the burning block, or
/// air for what flashes away.
pub fn alight(block: BlockId) -> Option<BlockId> {
    Some(match fuel(block)? {
        Fuel::Log => oriented(BLOCK_BURNING_LOG, block_axis(block)),
        Fuel::Boards | Fuel::Punk => BLOCK_BURNING_PLANKS,
        Fuel::Leaves | Fuel::Tinder | Fuel::Grass => BLOCK_AIR,
    })
}

/// Is this a block that is alight -- one the spread burns down and gives
/// heat from? Not a hearth: those are the fire map's.
#[inline]
pub fn is_blazing(block: BlockId) -> bool {
    matches!(block_kind(block), BLOCK_BURNING_LOG | BLOCK_BURNING_PLANKS)
}

/// What a burning block leaves when it has burnt through, or is put out:
/// the char of what it was, lying the way it lay. `None` for anything that
/// is not alight.
pub fn charred(block: BlockId) -> Option<BlockId> {
    Some(match block_kind(block) {
        BLOCK_BURNING_LOG => oriented(BLOCK_CHARRED_LOG, block_axis(block)),
        BLOCK_BURNING_PLANKS => BLOCK_CHARRED_PLANKS,
        _ => return None,
    })
}

/// How long a burning block has left when a server finds one it has no
/// clock for -- a world saved alight by a build that did not keep one, or
/// written by a mod. The whole burn: a fire nobody timed is a fire that
/// started now.
pub fn burn_seconds_of(block: BlockId) -> Option<f32> {
    match block_kind(block) {
        BLOCK_BURNING_LOG => Some(LOG_BURN_SECONDS),
        BLOCK_BURNING_PLANKS => Some(BOARDS_BURN_SECONDS),
        _ => None,
    }
}

/// Does this hearth's flame lick the cells round it?
///
/// **A campfire and a firepit, and nothing with walls.** A kiln's fire is
/// inside clay, a bloomery's inside a shaft, and a pit kiln's in a hole
/// whose walls it checks are fireproof (`pit::is_fireproof`); a flame you
/// cannot put your hand to is a flame that does not reach the wall beside
/// it.
#[inline]
pub fn licks_as_a_hearth(block: BlockId) -> bool {
    matches!(block_kind(block), BLOCK_CAMPFIRE_LIT | BLOCK_FIREPIT_LIT)
}

/// The cells a flame reaches from the cell it is in, and whether each is
/// the one it rises into.
///
/// Its four sides and the cell over it, and -- when `open_above` says the
/// cell over it is not in the way -- the cell over that: a flame is taller
/// than its cell, and a shelf two up over a campfire is where the heat
/// goes. Not below: fire does not burn down into a floor, and a campfire on
/// a plank floor would otherwise be a campfire that ate its floor in a
/// minute, which is not a decision anybody would make twice.
pub fn licked(at: (i32, i32, i32), open_above: bool) -> impl Iterator<Item = ((i32, i32, i32), bool)> {
    let (x, y, z) = at;
    let sides = [(1, 0), (-1, 0), (0, 1), (0, -1)]
        .into_iter()
        .map(move |(dx, dz)| ((x + dx, y, z + dz), false));
    let up = std::iter::once(((x, y + 1, z), true));
    let higher = open_above.then_some(((x, y + 2, z), true));
    sides.chain(up).chain(higher)
}

/// How much of a flame's heat a cell gets for where it is: over it, or
/// downwind of it, or upwind.
///
/// `offset` is the cell's (dx, dz) from the flame; `wind` is
/// `raft::Wind::vector`, the way the wind blows *toward*, as long as it is
/// strong. Never below a tenth: even upwind of a fire, the wall beside it
/// is beside a fire.
pub fn draught(offset: (i32, i32), rising: bool, wind: (f32, f32)) -> f32 {
    let along = offset.0 as f32 * wind.0 + offset.1 as f32 * wind.1;
    let blown = (1.0 + WIND_FACTOR * along).max(0.1);
    if rising {
        RISING_FACTOR * blown.max(1.0)
    } else {
        blown
    }
}

/// How much heat the cell at `at` needs before this fuel catches: the
/// fuel's own figure, moved up or down by a quarter by where it is.
///
/// A hash of the place and not a roll: see "Not a perfect wave" at the top.
pub fn catch_threshold(fuel: Fuel, at: (i32, i32, i32)) -> f32 {
    let mut h = (at.0 as u32).wrapping_mul(0x9E37_79B1)
        ^ (at.1 as u32).wrapping_mul(0x85EB_CA77)
        ^ (at.2 as u32).wrapping_mul(0xC2B2_AE3D);
    h ^= h >> 15;
    h = h.wrapping_mul(0x2C1B_3C6D);
    h ^= h >> 12;
    let unit = (h & 0xFFFF) as f32 / 65535.0;
    fuel.catch_heat() * (0.75 + 0.5 * unit)
}

// ---- the standing torch ----

/// How long a standing torch's wad of resin burns, in seconds.
///
/// Twenty minutes: two game days' evenings at the default clock, which is
/// a light for a camp and not a lamp for ever. **The rain does not put it
/// out**, and that is resin's whole argument over the hand torch's fibre --
/// a path lit in a storm is a path you did not have to walk in the dark.
pub const TORCH_SECONDS: f32 = 1200.0;

/// Is this either cell of a standing torch?
#[inline]
pub fn is_standing_torch(block: BlockId) -> bool {
    matches!(
        block_kind(block),
        BLOCK_STANDING_TORCH | BLOCK_STANDING_TORCH_LIT | BLOCK_STANDING_TORCH_OUT
    )
}

/// Is this the top of a standing torch, lit or out?
#[inline]
pub fn is_standing_torch_top(block: BlockId) -> bool {
    matches!(block_kind(block), BLOCK_STANDING_TORCH_LIT | BLOCK_STANDING_TORCH_OUT)
}

/// Where the other cell of a standing torch is, and what a new one puts
/// there: `types::bed_partner`'s contract for a pole. The top written for a
/// pole is the lit one, because a torch is set up to be a light; whether a
/// top that is *there* belongs to a pole is [`is_standing_torch_top`], lit
/// or out.
#[inline]
pub fn standing_torch_partner(at: (i32, i32, i32), block: BlockId) -> Option<((i32, i32, i32), BlockId)> {
    match block_kind(block) {
        BLOCK_STANDING_TORCH => Some(((at.0, at.1 + 1, at.2), BLOCK_STANDING_TORCH_LIT)),
        BLOCK_STANDING_TORCH_LIT | BLOCK_STANDING_TORCH_OUT => Some(((at.0, at.1 - 1, at.2), BLOCK_STANDING_TORCH)),
        _ => None,
    }
}

// ---- smoke ----

/// How far over a fire its smoke is followed, in blocks. Air that reaches
/// this high has found a way out: a chimney, a hole in the roof, the sky.
pub const ROOM_MAX_RISE: i32 = 6;

/// The most cells of air a room is followed through. Past it, the fire is
/// in a hall or out of doors, and a room that size does not fill.
pub const ROOM_MAX_CELLS: usize = 400;

/// How often a fire's room is looked at again, in seconds. A room changes
/// when somebody opens a wall, which is not a thing that needs a tick.
pub const SMOKE_STEP_SECONDS: f32 = 2.0;

/// The size of room one fire fills to thickness one, in cells.
///
/// Sixty: a hut four by five inside and three high is at full smoke; a
/// hall twice that is at half. The number that decides whether a smoke
/// hole is worth cutting.
pub const SMOKE_FILLS_CELLS: f32 = 60.0;

/// How fast a room fills, in thickness a second: a minute and a half from
/// clear air to as thick as it gets. Long enough to notice it coming.
pub const SMOKE_RISE_PER_SECOND: f32 = 1.0 / 90.0;

/// ...and clears, once it is vented or the fire is out: a quarter of a
/// minute. Opening a door is an answer that works at once.
pub const SMOKE_CLEAR_PER_SECOND: f32 = 1.0 / 15.0;

/// The thickness at which the air is not fit to breathe and a player's
/// breath starts to run down (`survival::Vitals::breathe_smoke` on the
/// server). Under it, smoke is only something to see.
pub const SMOKE_CHOKES: f32 = 0.5;

/// How much more a fire burning green or wet fuel smokes than a dry one
/// (`hearth::burns_green`): its room fills this much thicker and this much
/// faster, its ceiling soots this much sooner, and the plume over it
/// (`smoulders`) is this much denser.
///
/// **Twice, because the reason is a real one**: the water in a green log
/// boils off as steam and unburnt tar, which is the thick yellow-grey smoke
/// everybody who has lit a fire with the wood they cut that day has stood
/// in. It was the fire's heat alone that knew about green wood
/// (`GREEN_HEAT`), and a hut fire of fresh-cut alder filled its room
/// exactly as a seasoned one did -- so the one thing a player would notice
/// about green wood, the smoke, was the one thing it did not do. With it,
/// dry wood is a reason to keep a woodshed, not only a number on the kiln.
/// Not more: a closed hut at full smoke already chokes (`SMOKE_CHOKES`),
/// and doubling a room that thin takes it to choking.
pub const SMOULDER_SMOKE: f32 = 2.0;

/// The bit on a lit campfire or fire pit that says it is burning green or
/// wet fuel, for the client's plume (`smoulders`).
///
/// **On the block, for the client's sake.** The client knows a fire only as
/// a block -- it has no list of fires, and finds them by sampling columns
/// (`particles::find_hearths`) -- so what it draws over one has to be read
/// off the block. Only the two open hearths carry it: a kiln and a bloomery
/// keep their facing in the same field (`types::burnt_out`), and their smoke
/// goes up a chimney rather than into anybody's face. Every rule that asks
/// about a hearth asks its kind, so the bit changes nothing but the plume.
pub const SMOULDERING: BlockId = 1 << crate::types::VARIANT_SHIFT;

/// Is this an open fire burning green or wet fuel? See [`SMOULDERING`].
#[inline]
pub fn smoulders(block: BlockId) -> bool {
    licks_as_a_hearth(block) && block & SMOULDERING != 0
}

/// `block` with the smouldering bit set or cleared, and anything that is not
/// a lit open hearth handed back as it is.
#[inline]
pub fn with_smoulder(block: BlockId, on: bool) -> BlockId {
    if !licks_as_a_hearth(block) {
        return block;
    }
    if on {
        block_kind(block) | SMOULDERING
    } else {
        block_kind(block)
    }
}

/// What a fire's air is.
#[derive(Debug, Clone, PartialEq)]
pub enum Room {
    /// The fire is under the open sky, or in a hall too big to fill.
    Vented,
    /// A roof over it, and the smoke has nowhere to go: the cells it fills.
    Closed(Vec<(i32, i32, i32)>),
    /// A roof over it and a way out -- a doorway, a window, a hole in the
    /// roof: the cells it fills, and the share of a shut room's smoke that
    /// stays in them (`smoke_kept`).
    Leaky(Vec<(i32, i32, i32)>, f32),
}

/// How much of a shut room's smoke one open face in a *wall* lets out, as
/// the weight in `smoke_kept`.
///
/// **An opening thins the smoke; it does not clear it.** Smoke was all or
/// nothing: the air a fire could reach was followed, and if it got out
/// anywhere -- a doorway, a window, the gap under the eaves -- the room was
/// "vented" and held no smoke at all. Every house a player builds has a
/// way in, so no house ever smoked ("помещения не задымляются"), and the
/// smoke hole the whole mechanic was for was never needed. Now a doorway's
/// two open faces keep three quarters of the smoke in, and it is the hole
/// in the *roof* that clears a room, which is where smoke goes.
pub const SIDE_LEAK: f32 = 0.15;

/// ...and one open face in the *roof* over the room: smoke rises, and a
/// smoke hole one block across halves it.
pub const ROOF_LEAK: f32 = 1.0;

/// The share of a shut room's smoke a room with these openings keeps.
pub fn smoke_kept(side_faces: usize, roof_faces: usize) -> f32 {
    1.0 / (1.0 + SIDE_LEAK * side_faces as f32 + ROOF_LEAK * roof_faces as f32)
}

/// Is there a sky-blocking block over this cell, below `top`? The question
/// that tells the air inside a building from the air outside its door.
fn roofed(look: &impl Fn(i32, i32, i32) -> Option<BlockId>, at: (i32, i32, i32), top: i32) -> bool {
    ((at.1 + 1)..=top).any(|y| look(at.0, y, at.2).is_some_and(blocks_the_sky))
}

/// Does smoke pass through this block? Wherever the sky does: the rule the
/// rain and the fog use for a ceiling (`types::blocks_the_sky`).
#[inline]
fn smoke_passes(block: BlockId) -> bool {
    !blocks_the_sky(block)
}

/// The air a fire's smoke can reach, from the cell over the fire, going
/// sideways and up and never down, **under a roof**. `None` from `look` is a
/// cell nobody has loaded, and it is taken as open: smoke that might be
/// escaping into an unloaded chunk is not smoke to choke a player on.
///
/// The air followed is the air with a ceiling over it within
/// [`ROOM_MAX_RISE`]. An open cell beside it with no ceiling is a way out
/// through a wall (a doorway, a window); an open cell above it with no
/// ceiling is a way out through the roof. Neither is entered: each is
/// counted, and the room keeps [`smoke_kept`] of its smoke. A fire with no
/// roof over it is `Vented`, and so is one whose air runs past
/// [`ROOM_MAX_CELLS`] -- a hall, or a roof so wide it is a sky.
///
/// A cell over the fire that smoke cannot enter at all -- a block laid
/// straight on the flame -- is `Vented`: that fire is smothered, not
/// smoking a room, and its soot goes on that block.
pub fn smoke_room(look: impl Fn(i32, i32, i32) -> Option<BlockId>, fire: (i32, i32, i32)) -> Room {
    match walk_room(look, (fire.0, fire.1 + 1, fire.2), fire.1 + ROOM_MAX_RISE, |_| {}) {
        None => Room::Vented,
        Some(walk) if walk.side == 0 && walk.roof == 0 => Room::Closed(walk.cells),
        Some(walk) => {
            let kept = smoke_kept(walk.side, walk.roof);
            Room::Leaky(walk.cells, kept)
        }
    }
}

/// One face of a room met by [`walk_room`]: where its air stops.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Face {
    /// A way out, from a cell of the room in a direction: `dir.1` is one
    /// for a hole in the roof and nought for a gap in a wall.
    Opening { from: (i32, i32, i32), dir: (i32, i32, i32) },
    /// Something the air cannot pass, beside the room (`ceiling` false) or
    /// over it (`ceiling` true). The floor is never met: the walk does not
    /// go down.
    Solid { block: BlockId, ceiling: bool },
}

/// The air [`walk_room`] followed, and how many ways out it found.
#[derive(Debug, Clone, PartialEq)]
pub struct Walk {
    /// Sorted, so the first is the same cell whoever walked it.
    pub cells: Vec<(i32, i32, i32)>,
    pub side: usize,
    pub roof: usize,
}

/// The walk under [`smoke_room`], from `start`, with ceilings looked for up
/// to `top`, handing every face it meets to `face`. `None` is air that is
/// not a room: no roof near, a cell nobody has loaded, or more than
/// [`ROOM_MAX_CELLS`].
///
/// **One walk for the smoke and the warmth** (`shelter::survey`). They ask
/// different things of a room -- where the smoke goes; what the walls are
/// made of and which way the gaps face the wind -- and the obvious thing
/// was a second flood fill for the second question. Two walks are two rules
/// for what a room is, and the day they disagree a hut is smoky like a
/// house and cold like a field. So the walk is written once, and each asker
/// keeps what it wants out of the faces.
pub fn walk_room(
    look: impl Fn(i32, i32, i32) -> Option<BlockId>,
    start: (i32, i32, i32),
    top: i32,
    mut face: impl FnMut(Face),
) -> Option<Walk> {
    match look(start.0, start.1, start.2) {
        None => return None,
        Some(block) if !smoke_passes(block) => return None,
        _ => {}
    }
    // Open air is a fire with no roof over it *or beside it*. A fire under
    // its own smoke hole has sky straight over it and a roof all round, and
    // that is a room with a hole in it, not a field.
    let roof_near = roofed(&look, start, top)
        || [(1, 0), (-1, 0), (0, 1), (0, -1)]
            .into_iter()
            .any(|(dx, dz)| roofed(&look, (start.0 + dx, start.1, start.2 + dz), top));
    if !roof_near {
        return None;
    }
    let mut seen: std::collections::HashSet<(i32, i32, i32)> = std::collections::HashSet::new();
    let mut queue = std::collections::VecDeque::new();
    let (mut side, mut roof) = (0usize, 0usize);
    seen.insert(start);
    queue.push_back(start);
    while let Some((x, y, z)) = queue.pop_front() {
        for (dx, dy, dz) in [(1, 0, 0), (-1, 0, 0), (0, 0, 1), (0, 0, -1), (0, 1, 0)] {
            let next = (x + dx, y + dy, z + dz);
            if seen.contains(&next) {
                continue;
            }
            let block = look(next.0, next.1, next.2)?;
            if !smoke_passes(block) {
                face(Face::Solid { block, ceiling: dy > 0 });
                continue;
            }
            if next.1 > top || !roofed(&look, next, top) {
                if dy > 0 {
                    roof += 1;
                } else {
                    side += 1;
                }
                face(Face::Opening { from: (x, y, z), dir: (dx, dy, dz) });
                continue;
            }
            seen.insert(next);
            if seen.len() > ROOM_MAX_CELLS {
                return None;
            }
            queue.push_back(next);
        }
    }
    let mut cells: Vec<_> = seen.into_iter().collect();
    cells.sort_unstable();
    Some(Walk { cells, side, roof })
}

/// How thick a closed room of this many cells gets with one fire in it.
pub fn smoke_target(cells: usize) -> f32 {
    (SMOKE_FILLS_CELLS / cells.max(1) as f32).clamp(0.0, 1.0)
}

// ---- soot ----

/// How long a fire burns under a ceiling before it darkens by a stage, in
/// seconds: a minute and a half, so a ceiling over an evening's fire is
/// visibly darker by the time the meal is cooked, and black in three.
///
/// **It was five minutes**, which meant the first stage came after longer
/// than most players keep a fire going under a roof they are watching, and
/// the report was "копоти нету". A ceiling that says a fire has been lit
/// under it has to say so the first evening.
pub const SOOT_STAGE_SECONDS: f32 = 90.0;

/// The darkest a ceiling gets.
pub const SOOT_STAGES: u8 = 3;

/// Can this block be sooted? The stuff a ceiling over a hearth is built of:
/// boards of both woods, pegged or not, cobble and fired brick.
///
/// **Not a log**, whose variant field is its axis; **not** anything else
/// whose variant already means something. See `types::may_carry_variant`,
/// which asks this.
///
/// **Not dressed stone either**, and that one is a trade. A hearth in a
/// cave would blacken its roof, and that would be right -- but stone is
/// what the anti-cheat's tests hold up as the block no bits may ever be
/// set on (`a_stone_block_never_carries_orientation_bits`), because it is
/// the block most placed and most broken, and a second id for it is a
/// second slot in every pack. A cave does not need its roof to say a fire
/// has been lit in it; a house built of cobble and boards does.
#[inline]
pub fn may_carry_soot(block: BlockId) -> bool {
    // Boards by what they are, as `fuel` finds them: anything that can be
    // pegged or has been is boards, so a new wood's planks soot too.
    crate::types::pegged_form(block).is_some()
        || crate::types::is_pegged(block)
        || matches!(block_kind(block), BLOCK_COBBLESTONE | BLOCK_BRICKS)
}

/// How sooted a block is: nought to [`SOOT_STAGES`].
///
/// **The values past the last stage are not more soot.** They used to read
/// as the blackest ceiling, and nothing wrote them; they are a board's
/// weathering now (`weathering`, which explains the sharing), and a rotten
/// roof read as soot would be a roof the rain may no longer touch.
#[inline]
pub fn soot(block: BlockId) -> u8 {
    if !may_carry_soot(block) {
        return 0;
    }
    let code = ((block & VARIANT_MASK) >> VARIANT_SHIFT) as u8;
    if code > SOOT_STAGES {
        0
    } else {
        code
    }
}

/// The same block at a soot stage. Anything that cannot carry soot comes
/// back as it was -- **and so does a board the weather has already
/// greyed**, which the smoke does not blacken. See `weathering`.
#[inline]
pub fn with_soot(block: BlockId, stage: u8) -> BlockId {
    if !may_carry_soot(block) || crate::weathering::weathering(block) > 0 {
        return block;
    }
    block_kind(block) | (BlockId::from(stage.min(SOOT_STAGES)) << VARIANT_SHIFT)
}

/// The first block over a fire that its heat and smoke meet, within
/// [`ROOM_MAX_RISE`]: the ceiling that sooting darkens. `None` under open
/// sky, or where nobody has loaded the column.
pub fn ceiling_over(look: impl Fn(i32, i32, i32) -> Option<BlockId>, fire: (i32, i32, i32)) -> Option<(i32, i32, i32)> {
    for rise in 1..=ROOM_MAX_RISE {
        let at = (fire.0, fire.1 + rise, fire.2);
        let block = look(at.0, at.1, at.2)?;
        if blocks_the_sky(block) {
            return Some(at);
        }
    }
    None
}

// ---- ash on a field ----
//
// "Make ash useful." Ash was a thing a hearth put in its ash slot and a pile
// burnt in the open left behind, and nothing asked for it. Three uses were
// weighed:
//
// * **Lye for the tannery** -- ash and water leached into a bath that
//   loosens hair. Right, and it lands on the drying rack, whose clock is
//   already the weather's and the hide's; a second input to it would be a
//   second number on a screen that is already about waiting.
// * **Ash to keep food** -- packed round meat in a crock. It would be a
//   recipe that turns one perishable into a slower one, and the rot clock
//   already has salt for exactly that; two answers to one problem, and the
//   worse one would be the one nobody used.
// * **Ash on a field (chosen).** Slash-and-burn is how the first farmers
//   fed poor ground, and what it creates is a real decision in a place
//   the game already has one: fertility decides where a field is worth
//   making (`worldgen::Fertility`), and a steppe camp with thin soil and a
//   hearth burning every night now has a way to farm where it lives
//   instead of walking to the river silt -- at the price of the hearth's
//   ash, which the hearth pays out one lump a load. And it is seen: a
//   dressed furrow is drawn grey.

/// How much quicker a crop grows on ground dressed with ash, as a
/// multiplier on its time -- two thirds, `Fertility::Rich`'s own figure, so
/// a dressed field on poor ground is about as good as undressed good ground
/// and a dressed field on good ground is better still.
///
/// **For one crop.** The dressing is spent when what grows on it comes ripe
/// (`logic::growth` on the server), so a field wants ash again every
/// harvest -- a hearth's worth of evenings for a field's worth of bread,
/// which is the trade.
pub const ASH_DRESSING_FACTOR: f32 = 2.0 / 3.0;

/// The variant bit that says tilled earth has been dressed with ash.
const DRESSED: BlockId = 1 << VARIANT_SHIFT;

/// Is this farmland dressed with ash?
#[inline]
pub fn is_dressed(block: BlockId) -> bool {
    block_kind(block) == crate::types::BLOCK_FARMLAND && block & DRESSED != 0
}

/// This farmland, dressed with ash; `None` for anything that is not
/// farmland or is already dressed.
///
/// **Ash also rests a tired furrow** ([`is_tired`]): the dressing clears the
/// flag. What a harvest takes out of the ground that ash puts back is most
/// of what the first farmers knew about it -- potash is named for the pot the
/// ash was leached in -- and a furrow that could be dressed and still be
/// tired would be two debts where the player sees one field.
#[inline]
pub fn dressed(block: BlockId) -> Option<BlockId> {
    (block_kind(block) == crate::types::BLOCK_FARMLAND && !is_dressed(block))
        .then_some((block & !TIRED) | DRESSED)
}

// ---- a furrow cropped out ----
//
// "Make farming realistic." A field sown year after year on the same ground
// gives less every year; that is the whole reason the first farmers moved on
// (slash-and-burn), rested a field (fallow) or carried muck and ash to it.
// Three shapes were weighed:
//
// * **Nutrients, three numbers a cell** (TerraFirmaCraft's nitrogen,
//   phosphorus and potassium, each crop eating one). It is the true shape,
//   and it asks the player to keep three hidden numbers per block in their
//   head -- which is bookkeeping, and crops that differ only in which number
//   they eat are one crop three times.
// * **Yield falls.** Truer to the record, and invisible until the harvest,
//   by which time the decision is a season old.
// * **The furrow tires (chosen).** One bit: a crop that ripens on undressed
//   ground leaves it tired, a tired furrow grows the next crop slower, and
//   it comes back by resting bare for a crop's worth of time or by ash. So
//   one field is slow, two fields sown in turn are quick, and ash is the
//   short cut -- three answers, each a trade, and the ground shows which
//   furrows are tired.

/// The variant bit that says a crop was taken off this farmland undressed
/// and the ground has not rested since.
const TIRED: BlockId = 2 << VARIANT_SHIFT;

/// How much longer a crop takes on a tired furrow, as a multiplier on its
/// time: the inverse of what ash gives, so a tired field dressed is a
/// rested one and a rested one is the field it always was.
pub const TIRED_FIELD_FACTOR: f32 = 1.5;

/// Is this farmland cropped out?
#[inline]
pub fn is_tired(block: BlockId) -> bool {
    block_kind(block) == crate::types::BLOCK_FARMLAND && block & TIRED != 0
}

/// The furrow a harvest leaves: plain earth if it was dressed (the ash is
/// what the crop took), tired if it was not. `None` for anything that is
/// not farmland.
#[inline]
pub fn after_harvest(block: BlockId) -> Option<BlockId> {
    if block_kind(block) != crate::types::BLOCK_FARMLAND {
        return None;
    }
    Some(if is_dressed(block) {
        block_kind(block)
    } else {
        block_kind(block) | TIRED
    })
}

/// The furrow after it has lain fallow: the flag gone, whatever else it had.
#[inline]
pub fn rested(block: BlockId) -> BlockId {
    block & !TIRED
}

/// Can this block carry the dressing in its variant field? Farmland, and
/// only farmland. See `types::may_carry_variant`.
#[inline]
pub fn may_carry_dressing(block: BlockId) -> bool {
    block_kind(block) == crate::types::BLOCK_FARMLAND
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::{
        Axis, BLOCK_BIRCH_LOG, BLOCK_CAMPFIRE, BLOCK_PEGGED_BIRCH_PLANKS, BLOCK_PEGGED_PLANKS, BLOCK_STONE, BLOCK_CHEST, BLOCK_KILN_LIT, BLOCK_LEAVES, BLOCK_LOG,
        BLOCK_STRIPPED_LOG,
    };
    use std::collections::HashMap;

    /// The smouldering bit goes only on an open hearth that is alight, and
    /// takes nothing else about it away: still a campfire to every rule that
    /// asks, and a kiln's facing is never touched (`SMOULDERING`).
    #[test]
    fn only_a_lit_open_hearth_carries_the_smoulder_and_it_stays_what_it_was() {
        use crate::types::{is_burning, BLOCK_CAMPFIRE_LIT};
        let green = with_smoulder(BLOCK_CAMPFIRE_LIT, true);
        assert!(smoulders(green) && !smoulders(BLOCK_CAMPFIRE_LIT));
        assert_eq!(block_kind(green), BLOCK_CAMPFIRE_LIT);
        assert!(licks_as_a_hearth(green) && is_burning(green) && crate::hearth::is_lit(green));
        assert_eq!(crate::types::burnt_out(green), Some(BLOCK_CAMPFIRE), "a smouldering fire went out as something else");
        assert_eq!(with_smoulder(green, false), BLOCK_CAMPFIRE_LIT);
        for other in [BLOCK_CAMPFIRE, BLOCK_KILN_LIT, BLOCK_STONE] {
            assert_eq!(with_smoulder(other, true), other, "{other} took the smoulder bit");
        }
    }

    #[test]
    fn a_cropped_furrow_tires_and_ash_or_rest_brings_it_back() {
        use crate::types::{is_known_block, BLOCK_FARMLAND};
        let tired = after_harvest(BLOCK_FARMLAND).unwrap();
        assert!(is_tired(tired), "an undressed harvest left the furrow as it was");
        assert!(is_known_block(tired), "a tired furrow is an id the anti-cheat refuses");
        assert_eq!(block_kind(tired), BLOCK_FARMLAND);
        // The dressed harvest spends the ash and leaves good earth.
        assert_eq!(after_harvest(dressed(BLOCK_FARMLAND).unwrap()), Some(BLOCK_FARMLAND));
        // Ash rests it...
        let fed = dressed(tired).unwrap();
        assert!(is_dressed(fed) && !is_tired(fed), "ash on a tired furrow left it tired");
        // ...and so does lying fallow.
        assert_eq!(rested(tired), BLOCK_FARMLAND);
        assert_eq!(after_harvest(BLOCK_STONE), None);
        assert!(!is_tired(BLOCK_STONE | TIRED));
    }

    #[test]
    fn every_wood_burns_and_char_does_not() {
        for log in [BLOCK_LOG, BLOCK_BIRCH_LOG, BLOCK_STRIPPED_LOG] {
            assert_eq!(fuel(log), Some(Fuel::Log), "log {log} does not burn");
        }
        for boards in [BLOCK_PLANKS, BLOCK_BIRCH_PLANKS, BLOCK_PEGGED_PLANKS, BLOCK_PEGGED_BIRCH_PLANKS] {
            assert_eq!(fuel(boards), Some(Fuel::Boards), "boards {boards} do not burn");
        }
        assert_eq!(fuel(BLOCK_LEAVES), Some(Fuel::Leaves));
        for never in [
            BLOCK_CHARRED_LOG,
            BLOCK_CHARRED_PLANKS,
            BLOCK_BURNING_LOG,
            BLOCK_BURNING_PLANKS,
            BLOCK_STONE,
            BLOCK_CHEST,
            BLOCK_CAMPFIRE,
            BLOCK_AIR,
        ] {
            assert_eq!(fuel(never), None, "{never} burns");
        }
    }

    #[test]
    fn thatch_and_branches_burn_tiles_do_not_and_every_roof_keeps_the_rain_off() {
        use crate::types::{
            faced, Facing, BLOCK_BRANCH_SLAB, BLOCK_BRANCH_ROOF,
            BLOCK_THATCH_SLAB, BLOCK_THATCH_ROOF, BLOCK_TILE_SLAB, BLOCK_TILE_ROOF,
        };
        let east = |kind| faced(kind, Facing::East);
        assert_eq!(fuel(east(BLOCK_THATCH_ROOF)), Some(Fuel::Tinder), "a turned step of thatch does not burn");
        assert_eq!(fuel(BLOCK_THATCH_SLAB), Some(Fuel::Tinder));
        assert_eq!(fuel(east(BLOCK_BRANCH_ROOF)), Some(Fuel::Leaves), "a turned step of branches does not burn");
        assert_eq!(fuel(BLOCK_BRANCH_SLAB), Some(Fuel::Leaves));
        for fireproof in [east(BLOCK_TILE_ROOF), BLOCK_TILE_SLAB] {
            assert_eq!(fuel(fireproof), None, "{fireproof} burns");
        }
        // Every roof is a roof to the rain, the chill and the smoke: the
        // ceiling rule the three of them read (`types::blocks_the_sky`).
        for roof in [
            BLOCK_TILE_ROOF, BLOCK_TILE_SLAB, BLOCK_THATCH_ROOF, BLOCK_THATCH_SLAB, BLOCK_BRANCH_ROOF,
            BLOCK_BRANCH_SLAB,
        ] {
            assert!(blocks_the_sky(east(roof)), "{roof} lets the sky through");
        }
        // ...and a fire under a slab of thatch is under a roof, not in the open.
        let look = |x: i32, y: i32, z: i32| Some(if (x, y, z) == (0, 4, 0) { BLOCK_THATCH_SLAB } else { BLOCK_AIR });
        assert!(roofed(&look, (0, 1, 0), 6), "a slab of thatch overhead is not a roof");
    }

    #[test]
    fn a_log_burns_and_chars_along_the_axis_it_lay_on() {
        let lying = oriented(BLOCK_LOG, Axis::X);
        let burning = alight(lying).unwrap();
        assert!(is_blazing(burning));
        assert_eq!(block_axis(burning), Axis::X);
        let char = charred(burning).unwrap();
        assert_eq!(block_kind(char), BLOCK_CHARRED_LOG);
        assert_eq!(block_axis(char), Axis::X, "the char stood up");
        assert_eq!(alight(BLOCK_LEAVES), Some(BLOCK_AIR), "a leaf does not flash away");
    }

    #[test]
    fn a_wall_beside_a_campfire_takes_about_a_minute_and_a_log_wall_longer() {
        // The two numbers a player can hold, in a calm: boards in about a
        // minute, logs in about two and a half -- within the quarter the
        // place moves them.
        let boards = catch_threshold(Fuel::Boards, (3, 10, -7)) / (HEARTH_HEAT * draught((1, 0), false, (0.0, 0.0)));
        assert!((45.0..=75.0).contains(&boards), "boards caught in {boards}s");
        let logs = catch_threshold(Fuel::Log, (3, 10, -7)) / HEARTH_HEAT;
        assert!(logs > boards * 2.0, "a log wall caught in {logs}s against boards' {boards}s");
    }

    #[test]
    fn the_wind_hurries_the_cell_downwind_and_spares_the_cell_upwind() {
        let east = (1.0, 0.0);
        let downwind = draught((1, 0), false, east);
        let upwind = draught((-1, 0), false, east);
        let calm = draught((1, 0), false, (0.0, 0.0));
        assert!(downwind > calm * 2.0 && upwind < calm * 0.5, "{downwind} {calm} {upwind}");
        assert!(upwind > 0.0, "a wall upwind of a fire can never catch");
        assert!(draught((0, 0), true, (0.0, 0.0)) > calm, "the cell over a flame is no hotter than the one beside it");
    }

    #[test]
    fn the_same_wall_catches_in_the_same_order_every_time_but_not_all_at_once() {
        let thresholds: Vec<f32> = (0..16).map(|x| catch_threshold(Fuel::Boards, (x, 5, 0))).collect();
        let again: Vec<f32> = (0..16).map(|x| catch_threshold(Fuel::Boards, (x, 5, 0))).collect();
        assert_eq!(thresholds, again);
        let (low, high) = thresholds.iter().fold((f32::MAX, f32::MIN), |(l, h), &t| (l.min(t), h.max(t)));
        assert!(high - low > 10.0, "a wall of boards catches in lockstep: {low}..{high}");
        assert!(low >= 45.0 && high <= 75.0);
    }

    #[test]
    fn a_flame_does_not_lick_the_floor_and_reaches_two_up_only_through_air() {
        let cells: Vec<_> = licked((0, 10, 0), true).collect();
        assert!(cells.iter().all(|((_, y, _), _)| *y >= 10), "a flame licked the floor");
        assert!(cells.iter().any(|((_, y, _), _)| *y == 12));
        assert!(!licked((0, 10, 0), false).any(|((_, y, _), _)| y == 12));
        assert!(licks_as_a_hearth(BLOCK_CAMPFIRE_LIT) && !licks_as_a_hearth(BLOCK_KILN_LIT));
    }

    /// A little world: stone everywhere listed, air everywhere else inside
    /// the box, and nothing loaded outside it.
    fn world(solid: &[(i32, i32, i32)]) -> impl Fn(i32, i32, i32) -> Option<BlockId> + '_ {
        let map: HashMap<(i32, i32, i32), BlockId> = solid.iter().map(|&c| (c, BLOCK_STONE)).collect();
        move |x, y, z| {
            if !(-20..=20).contains(&x) || !(-20..=20).contains(&z) || !(0..=60).contains(&y) {
                return None;
            }
            Some(*map.get(&(x, y, z)).unwrap_or(&BLOCK_AIR))
        }
    }

    /// A hut round (0, 1, 0): walls from -2 to 2, a roof at 4, the floor at
    /// 0. `door` knocks a hole in the wall.
    fn hut(door: bool) -> Vec<(i32, i32, i32)> {
        let mut cells = Vec::new();
        for x in -2i32..=2 {
            for z in -2i32..=2 {
                cells.push((x, 0, z));
                cells.push((x, 4, z));
                for y in 1..4 {
                    if x.abs() == 2 || z.abs() == 2 {
                        if door && x == 2 && z == 0 && y <= 2 {
                            continue;
                        }
                        cells.push((x, y, z));
                    }
                }
            }
        }
        cells
    }

    #[test]
    fn a_fire_in_a_shut_hut_fills_it_and_an_open_door_lets_the_smoke_out() {
        let shut = hut(false);
        let room = smoke_room(world(&shut), (0, 1, 0));
        let Room::Closed(cells) = room else {
            panic!("a hut with no opening vented its smoke");
        };
        // Inside is three by three by three; the smoke starts over the
        // fire and never goes down, so it fills the upper two layers.
        assert_eq!(cells.len(), 3 * 3 * 2, "the room is {} cells", cells.len());
        assert!(smoke_target(cells.len()) >= 1.0, "a small hut is not full of smoke");

        // A doorway thins it and does not clear it: a house with a way in
        // is still a house with a fire in it.
        let open = hut(true);
        let Room::Leaky(cells, kept) = smoke_room(world(&open), (0, 1, 0)) else {
            panic!("a hut with a doorway was vented as if it had no roof");
        };
        // The two layers of the hut, and the top cell of the doorway, which
        // is under the eaves.
        assert_eq!(cells.len(), 3 * 3 * 2 + 1);
        assert!((0.5..1.0).contains(&kept), "a doorway kept {kept} of the smoke");
        assert_eq!(smoke_room(world(&[]), (0, 1, 0)), Room::Vented, "a fire in a field filled a room");
    }

    #[test]
    fn a_smoke_hole_in_the_roof_clears_more_smoke_than_a_doorway_in_the_wall() {
        let doorway = match smoke_room(world(&hut(true)), (0, 1, 0)) {
            Room::Leaky(_, kept) => kept,
            other => panic!("the doorway hut is {other:?}"),
        };
        let mut holed = hut(false);
        holed.retain(|&cell| cell != (0, 4, 0));
        let hole = match smoke_room(world(&holed), (0, 1, 0)) {
            Room::Leaky(_, kept) => kept,
            other => panic!("the smoke-hole hut is {other:?}"),
        };
        assert!(hole < doorway, "a hole in the roof kept {hole} of the smoke, a doorway {doorway}");
    }

    #[test]
    fn a_shut_door_keeps_a_huts_smoke_in_and_opening_it_lets_some_of_it_out() {
        // The hut with its doorway, and a real door hung in it: the wall
        // `smoke_room` sees is the door's, shut or open. What a player
        // choking in a smoky hut does about it is open the door.
        use crate::types::{door_partner, door_swung, faced, Facing, BLOCK_DOOR};
        let open_wall = hut(true);
        let lower = faced(BLOCK_DOOR, Facing::West);
        let (top_at, top) = door_partner((2, 1, 0), lower).unwrap();
        for (lower, top, closed) in [(lower, top, true), (door_swung(lower), door_swung(top), false)] {
            let stone = world(&open_wall);
            let look = |x, y, z| match (x, y, z) {
                (2, 1, 0) => Some(lower),
                at if at == top_at => Some(top),
                _ => stone(x, y, z),
            };
            let room = smoke_room(look, (0, 1, 0));
            assert_eq!(matches!(room, Room::Closed(_)), closed, "a door {} made the hut {room:?}", if closed { "shut" } else { "open" });
            if !closed {
                assert!(matches!(room, Room::Leaky(_, kept) if kept < 1.0), "an open door let nothing out: {room:?}");
            }
        }
    }

    #[test]
    fn soot_darkens_a_ceiling_in_three_stages_and_never_touches_a_log() {
        let mut ceiling = BLOCK_PLANKS;
        for stage in 1..=5 {
            ceiling = with_soot(ceiling, stage);
        }
        assert_eq!(soot(ceiling), SOOT_STAGES);
        assert_eq!(block_kind(ceiling), BLOCK_PLANKS);
        let log = oriented(BLOCK_LOG, Axis::Z);
        assert_eq!(with_soot(log, 2), log, "soot turned a log");
        let stone = [(0, 4, 0)];
        assert_eq!(ceiling_over(world(&stone), (0, 1, 0)), Some((0, 4, 0)));
        assert_eq!(ceiling_over(world(&[]), (0, 1, 0)), None);
    }

    #[test]
    fn a_standing_torch_is_two_cells_that_find_each_other() {
        let (top_at, top) = standing_torch_partner((0, 5, 0), BLOCK_STANDING_TORCH).unwrap();
        assert_eq!((top_at, top), ((0, 6, 0), BLOCK_STANDING_TORCH_LIT));
        for top in [BLOCK_STANDING_TORCH_LIT, BLOCK_STANDING_TORCH_OUT] {
            assert_eq!(standing_torch_partner((0, 6, 0), top), Some(((0, 5, 0), BLOCK_STANDING_TORCH)));
        }
        assert_eq!(standing_torch_partner((0, 6, 0), BLOCK_LOG), None);
    }

    #[test]
    fn ash_dresses_farmland_once_and_nothing_else() {
        use crate::types::{is_known_block, BLOCK_DIRT, BLOCK_FARMLAND};
        let field = dressed(BLOCK_FARMLAND).unwrap();
        assert!(is_dressed(field) && is_known_block(field), "a dressed field is an id the server refuses");
        assert_eq!(block_kind(field), BLOCK_FARMLAND);
        assert_eq!(dressed(field), None, "a field took ash twice");
        assert_eq!(dressed(BLOCK_DIRT), None);
        const { assert!(ASH_DRESSING_FACTOR < 1.0) };
    }

    #[test]
    fn the_undergrowth_burns_and_the_lake_does_not() {
        use crate::types::{BLOCK_DRY_GRASS, BLOCK_KELP, BLOCK_TALL_GRASS, BLOCK_WATER};
        assert_eq!(fuel(BLOCK_TALL_GRASS), Some(Fuel::Grass));
        assert_eq!(fuel(BLOCK_DRY_GRASS), Some(Fuel::Grass));
        // The one thing a fire must never be.
        assert_eq!(fuel(BLOCK_KELP), None, "the sea was kindling");
        assert_eq!(fuel(BLOCK_WATER), None);
        // Gone in a breath, and it leaves the ash a burnt meadow carries.
        assert_eq!(Fuel::Grass.burn_seconds(), None);
        assert_eq!(alight(BLOCK_TALL_GRASS), Some(crate::types::BLOCK_AIR));
        assert!(leaves_ash(Fuel::Grass));
        assert!(!leaves_ash(Fuel::Leaves), "ash was hung in a canopy");
    }

    #[test]
    fn a_tuft_takes_before_a_plank_and_a_plank_before_a_log() {
        // The order is the mechanic: a fire finds the grass, the grass
        // finds the wall, and the player has the minute in between.
        assert!(Fuel::Grass.catch_heat() < Fuel::Boards.catch_heat());
        assert!(Fuel::Boards.catch_heat() < Fuel::Log.catch_heat());
    }

    #[test]
    fn the_same_meadow_is_a_hazard_in_summer_and_is_not_in_winter() {
        use crate::season::{MIDSUMMER_WORLD_TIME, YEAR_DAYS};
        let summer = seasoning(MIDSUMMER_WORLD_TIME);
        let winter = seasoning(MIDSUMMER_WORLD_TIME + YEAR_DAYS * 0.5);
        assert!(summer < winter, "summer {summer} winter {winter}");
        assert!((DRY_SEASON..=WET_SEASON).contains(&summer));
        assert!((DRY_SEASON..=WET_SEASON).contains(&winter));
        // A whole year and the factor never leaves its two ends: a
        // threshold multiplied by nothing is fuel that catches instantly.
        for day in 0..(YEAR_DAYS as i32 * 2) {
            let f = seasoning(day as f32);
            assert!((DRY_SEASON..=WET_SEASON).contains(&f), "day {day} gave {f}");
        }
    }

    #[test]
    fn wet_wood_and_green_wood_resist_what_dry_wood_takes() {
        let green = crate::wood::green(BLOCK_LOG);
        assert_eq!(damp_factor(green), DAMP, "green timber caught like seasoned");
        assert_eq!(damp_factor(BLOCK_LOG), 1.0);
        const { assert!(DAMP > 1.0) };
    }
}
