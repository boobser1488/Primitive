//! The ground: which rock a column is made of and what it breaks into, the
//! soils over it, the grasses on it, and the moss on its stones and trees.
//!
//! ## Why a table, again
//!
//! `wood` argued it for four blocks a wood; a rock is five -- the rock, its
//! cobble, its gravel, its sand and its pebble -- and there are fifteen of
//! them. Every rule about *cobblestone* rather than about *granite cobble*
//! (a fire ring, a recipe, a mushroom's footing) would otherwise be a
//! fifteen-arm match written wherever it is needed, and the one forgotten is
//! the basalt gravel a cactus will not stand on. So the rules ask the table:
//! `as_common` says what a block stands in for, and the rules that care ask
//! about that.
//!
//! ## What the rubble is for
//!
//! **A rock's rubble is the common rubble in its own stone**, and that is
//! the whole of its rule: it stands in for cobblestone, gravel, sand or a
//! pebble anywhere a recipe or a plant asks for one (`stands_in_for`). What
//! it adds is *where it came from* -- granite pebbles on a path say the
//! hills, black basalt sand says a dyke broke the surface -- and what it
//! weighs, which is its rock's weight (`blocks`' templates), so a pack of
//! tuff cobble is carried where a pack of gabbro is not.
//!
//! Rejected: *each rock's rubble with rules of its own* -- basalt gravel
//! that drains, marble sand that makes lime. A rule per rock is a list of
//! correct answers a player has to learn from a wiki, which is the chore the
//! design principle forbids; a rock that says where it came from and what it
//! weighs is a decision on a path home.
//!
//! **Sandstone has no sand of its own**: it *is* sand that stayed sand long
//! enough, and its sand is `BLOCK_SAND`. A "sandstone sand" beside sand would
//! be one material under two names that do not stack.
//!
//! ## The rocks themselves decide the dig
//!
//! Soft rocks (chalk, tuff, shale) break with flint in a fraction of
//! limestone's time; quartzite, gneiss, diorite and andesite want copper as
//! granite does, and gabbro bronze as basalt does (the rows). **Chalk grows
//! flint** as limestone does (`worldgen`'s `flint_spacing`), which is true
//! and is the one reason a knapper looks for white ground.
//!
//! ## Soils
//!
//! Ten soils, each where its climate makes it (`worldgen::Biome`), and one
//! decision each carries: **whether a hoe will make a field of it**
//! (`tills`). Chernozem, loam, loess, rendzina and andosol till; podzol and
//! gley are too sour and too wet, laterite too hard, solonchak too salt and
//! permafrost frozen -- so a farm is a reason to go to the steppe, and a
//! camp in the taiga lives on what it gathers. Laterite and permafrost also
//! *hold a wall* (`falls: false` on their rows): a cellar dug in them does
//! not fill in.
//!
//! ## Grasses
//!
//! Ten grasses, each of one country (`grows_on`): feather grass on the
//! steppe, sedge by still water, cotton grass on bog and tundra, fescue in
//! the mountains, marram on dunes, elephant grass in the savanna, bluegrass
//! under broadleaf, timothy in meadows, tussock grass in the cold wet north,
//! spinifex in the desert. Most are fibre, as the tuft is. **Elephant grass is
//! cane**, which it is, and which puts the arundo's canes in the savanna;
//! **spinifex is resin** -- the desert peoples' glue -- which puts glue
//! within reach of a desert with no pine in it.
//!
//! ## Moss
//!
//! A stone or a trunk in a wet, shaded place carries moss **on its north
//! face** (`MOSSY`, a bit of the variant; the mesher draws it on the north
//! side and the top of a stone). The north face is the point: moss on a
//! trunk is a compass, which is a decision in a wood with no sun. Scraped off
//! by hand (`scraped`), it is a wound dressing and nothing else.

use crate::types::{
    block_kind, BlockId, VARIANT_MASK, VARIANT_SHIFT, BLOCK_ANDESITE, BLOCK_ANDESITE_COBBLE,
    BLOCK_ANDESITE_GRAVEL, BLOCK_ANDESITE_PEBBLE, BLOCK_ANDESITE_SAND, BLOCK_ANDOSOL, BLOCK_BASALT,
    BLOCK_BASALT_COBBLE, BLOCK_BASALT_GRAVEL, BLOCK_BASALT_PEBBLE, BLOCK_BASALT_SAND, BLOCK_BLUEGRASS, BLOCK_CHALK,
    BLOCK_CHALK_COBBLE, BLOCK_CHALK_GRAVEL, BLOCK_CHALK_PEBBLE, BLOCK_CHALK_SAND, BLOCK_CHERNOZEM, BLOCK_COBBLESTONE,
    BLOCK_COTTON_GRASS, BLOCK_DIORITE, BLOCK_DIORITE_COBBLE, BLOCK_DIORITE_GRAVEL, BLOCK_DIORITE_PEBBLE,
    BLOCK_DIORITE_SAND, BLOCK_DIRT, BLOCK_DOLOMITE, BLOCK_DOLOMITE_COBBLE, BLOCK_DOLOMITE_GRAVEL,
    BLOCK_DOLOMITE_PEBBLE, BLOCK_DOLOMITE_SAND, BLOCK_ELEPHANT_GRASS, BLOCK_FEATHER_GRASS, BLOCK_FESCUE,
    BLOCK_GABBRO, BLOCK_GABBRO_COBBLE, BLOCK_GABBRO_GRAVEL, BLOCK_GABBRO_PEBBLE, BLOCK_GABBRO_SAND, BLOCK_GLEY,
    BLOCK_GNEISS, BLOCK_GNEISS_COBBLE, BLOCK_GNEISS_GRAVEL, BLOCK_GNEISS_PEBBLE, BLOCK_GNEISS_SAND, BLOCK_GRANITE,
    BLOCK_GRANITE_COBBLE, BLOCK_GRANITE_GRAVEL, BLOCK_GRANITE_PEBBLE, BLOCK_GRANITE_SAND, BLOCK_GRASS,
    BLOCK_GRAVEL, BLOCK_LATERITE, BLOCK_LIMESTONE, BLOCK_LIMESTONE_COBBLE, BLOCK_LIMESTONE_GRAVEL,
    BLOCK_LIMESTONE_PEBBLE, BLOCK_LIMESTONE_SAND, BLOCK_LOAM, BLOCK_LOESS, BLOCK_MARBLE, BLOCK_MARBLE_COBBLE,
    BLOCK_MARBLE_GRAVEL, BLOCK_MARBLE_PEBBLE, BLOCK_MARBLE_SAND, BLOCK_MARRAM, BLOCK_MUD, BLOCK_PEAT,
    BLOCK_PEBBLE, BLOCK_PERMAFROST, BLOCK_PODZOL, BLOCK_QUARTZITE, BLOCK_QUARTZITE_COBBLE,
    BLOCK_QUARTZITE_GRAVEL, BLOCK_QUARTZITE_PEBBLE, BLOCK_QUARTZITE_SAND, BLOCK_RENDZINA, BLOCK_SAND,
    BLOCK_SANDSTONE, BLOCK_SANDSTONE_COBBLE, BLOCK_SANDSTONE_GRAVEL, BLOCK_SANDSTONE_PEBBLE, BLOCK_SANDY_SOIL,
    BLOCK_SEDGE, BLOCK_SHALE, BLOCK_SHALE_COBBLE, BLOCK_SHALE_GRAVEL, BLOCK_SHALE_PEBBLE, BLOCK_SHALE_SAND,
    BLOCK_SNOW, BLOCK_SOLONCHAK, BLOCK_SPINIFEX, BLOCK_STONE, BLOCK_TIMOTHY, BLOCK_TUFF, BLOCK_TUFF_COBBLE,
    BLOCK_TUFF_GRAVEL, BLOCK_TUFF_PEBBLE, BLOCK_TUFF_SAND, BLOCK_TUSSOCK_GRASS, BLOCK_DRY_TURF,
};

/// One rock and the four things it breaks into.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Rock {
    pub stone: BlockId,
    pub cobble: BlockId,
    pub gravel: BlockId,
    pub sand: BlockId,
    pub pebble: BlockId,
}

/// Which of a rock's four kinds of rubble.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Form {
    Cobble,
    Gravel,
    Sand,
    Pebble,
}

impl Rock {
    /// This rock's piece of rubble of `form`.
    #[inline]
    pub const fn form(&self, form: Form) -> BlockId {
        match form {
            Form::Cobble => self.cobble,
            Form::Gravel => self.gravel,
            Form::Sand => self.sand,
            Form::Pebble => self.pebble,
        }
    }
}

const fn rock(stone: BlockId, cobble: BlockId, gravel: BlockId, sand: BlockId, pebble: BlockId) -> Rock {
    Rock { stone, cobble, gravel, sand, pebble }
}

/// Every rock, **the common stone first**: its rubble is what a recipe names
/// when it means any rubble (see [`stands_in_for`]), as the oak is in `wood`.
pub const ROCKS: [Rock; 15] = [
    rock(BLOCK_STONE, BLOCK_COBBLESTONE, BLOCK_GRAVEL, BLOCK_SAND, BLOCK_PEBBLE),
    // Sandstone's sand is sand: see the module doc.
    rock(BLOCK_SANDSTONE, BLOCK_SANDSTONE_COBBLE, BLOCK_SANDSTONE_GRAVEL, BLOCK_SAND, BLOCK_SANDSTONE_PEBBLE),
    rock(BLOCK_LIMESTONE, BLOCK_LIMESTONE_COBBLE, BLOCK_LIMESTONE_GRAVEL, BLOCK_LIMESTONE_SAND, BLOCK_LIMESTONE_PEBBLE),
    rock(BLOCK_GRANITE, BLOCK_GRANITE_COBBLE, BLOCK_GRANITE_GRAVEL, BLOCK_GRANITE_SAND, BLOCK_GRANITE_PEBBLE),
    rock(BLOCK_BASALT, BLOCK_BASALT_COBBLE, BLOCK_BASALT_GRAVEL, BLOCK_BASALT_SAND, BLOCK_BASALT_PEBBLE),
    rock(BLOCK_SHALE, BLOCK_SHALE_COBBLE, BLOCK_SHALE_GRAVEL, BLOCK_SHALE_SAND, BLOCK_SHALE_PEBBLE),
    rock(BLOCK_CHALK, BLOCK_CHALK_COBBLE, BLOCK_CHALK_GRAVEL, BLOCK_CHALK_SAND, BLOCK_CHALK_PEBBLE),
    rock(BLOCK_DOLOMITE, BLOCK_DOLOMITE_COBBLE, BLOCK_DOLOMITE_GRAVEL, BLOCK_DOLOMITE_SAND, BLOCK_DOLOMITE_PEBBLE),
    rock(BLOCK_MARBLE, BLOCK_MARBLE_COBBLE, BLOCK_MARBLE_GRAVEL, BLOCK_MARBLE_SAND, BLOCK_MARBLE_PEBBLE),
    rock(BLOCK_QUARTZITE, BLOCK_QUARTZITE_COBBLE, BLOCK_QUARTZITE_GRAVEL, BLOCK_QUARTZITE_SAND, BLOCK_QUARTZITE_PEBBLE),
    rock(BLOCK_GNEISS, BLOCK_GNEISS_COBBLE, BLOCK_GNEISS_GRAVEL, BLOCK_GNEISS_SAND, BLOCK_GNEISS_PEBBLE),
    rock(BLOCK_DIORITE, BLOCK_DIORITE_COBBLE, BLOCK_DIORITE_GRAVEL, BLOCK_DIORITE_SAND, BLOCK_DIORITE_PEBBLE),
    rock(BLOCK_GABBRO, BLOCK_GABBRO_COBBLE, BLOCK_GABBRO_GRAVEL, BLOCK_GABBRO_SAND, BLOCK_GABBRO_PEBBLE),
    rock(BLOCK_ANDESITE, BLOCK_ANDESITE_COBBLE, BLOCK_ANDESITE_GRAVEL, BLOCK_ANDESITE_SAND, BLOCK_ANDESITE_PEBBLE),
    rock(BLOCK_TUFF, BLOCK_TUFF_COBBLE, BLOCK_TUFF_GRAVEL, BLOCK_TUFF_SAND, BLOCK_TUFF_PEBBLE),
];

/// The ten rocks this table added, the ones with no rules older than it.
pub const NEW_ROCKS: [BlockId; 10] = [
    BLOCK_SHALE,
    BLOCK_CHALK,
    BLOCK_DOLOMITE,
    BLOCK_MARBLE,
    BLOCK_QUARTZITE,
    BLOCK_GNEISS,
    BLOCK_DIORITE,
    BLOCK_GABBRO,
    BLOCK_ANDESITE,
    BLOCK_TUFF,
];

/// Every soil this table added. See the module doc for what each decides.
pub const SOILS: [BlockId; 10] = [
    BLOCK_LOAM,
    BLOCK_CHERNOZEM,
    BLOCK_PODZOL,
    BLOCK_LATERITE,
    BLOCK_SOLONCHAK,
    BLOCK_LOESS,
    BLOCK_GLEY,
    BLOCK_RENDZINA,
    BLOCK_ANDOSOL,
    BLOCK_PERMAFROST,
];

/// Every grass this table added.
pub const GRASSES: [BlockId; 10] = [
    BLOCK_FEATHER_GRASS,
    BLOCK_SEDGE,
    BLOCK_COTTON_GRASS,
    BLOCK_FESCUE,
    BLOCK_MARRAM,
    BLOCK_ELEPHANT_GRASS,
    BLOCK_BLUEGRASS,
    BLOCK_TIMOTHY,
    BLOCK_TUSSOCK_GRASS,
    BLOCK_SPINIFEX,
];

/// The rock a block is, or is the rubble of, and which rubble.
#[inline]
pub fn rock_of(id: BlockId) -> Option<(&'static Rock, Option<Form>)> {
    let kind = block_kind(id);
    ROCKS.iter().find_map(|r| {
        if kind == r.stone {
            Some((r, None))
        } else if kind == r.cobble {
            Some((r, Some(Form::Cobble)))
        } else if kind == r.gravel {
            Some((r, Some(Form::Gravel)))
        } else if kind == r.sand {
            // Sand is two rocks' sand; the common stone's comes first.
            Some((r, Some(Form::Sand)))
        } else if kind == r.pebble {
            Some((r, Some(Form::Pebble)))
        } else {
            None
        }
    })
}

/// The rubble of `form` that breaks off the rock `stone`: granite gravel for
/// granite, common gravel for plain stone and for anything that is not a rock.
#[inline]
pub fn rubble_of(stone: BlockId, form: Form) -> BlockId {
    let kind = block_kind(stone);
    ROCKS.iter().find(|r| r.stone == kind).unwrap_or(&ROCKS[0]).form(form)
}

/// Is this one of the ten soils?
#[inline]
pub fn is_soil(id: BlockId) -> bool {
    SOILS.contains(&block_kind(id))
}

/// Is this one of the ten grasses?
#[inline]
pub fn is_grass(id: BlockId) -> bool {
    GRASSES.contains(&block_kind(id))
}

/// What a block stands in for: a rock's rubble is the common rubble of its
/// form, a new rock is stone, a fertile soil is dirt and a dry one is sandy
/// soil. **Permafrost stands in for nothing a plant roots in** and comes back
/// as itself. Everything else comes back as its kind.
///
/// The four older rocks are *not* stone here: each already has rules of its
/// own (flint on limestone, the granite line) that a mapping would blur.
#[inline]
pub fn as_common(id: BlockId) -> BlockId {
    let kind = block_kind(id);
    if kind < 512 {
        return kind;
    }
    if let Some((rock, form)) = rock_of(kind) {
        return match form {
            Some(form) => ROCKS[0].form(form),
            None if NEW_ROCKS.contains(&rock.stone) => BLOCK_STONE,
            None => kind,
        };
    }
    match kind {
        BLOCK_LOAM | BLOCK_CHERNOZEM | BLOCK_PODZOL | BLOCK_LOESS | BLOCK_GLEY | BLOCK_RENDZINA | BLOCK_ANDOSOL => {
            BLOCK_DIRT
        }
        BLOCK_LATERITE | BLOCK_SOLONCHAK => BLOCK_SANDY_SOIL,
        _ => kind,
    }
}

/// May a recipe that asks for `asked` take `held` in its place? The same
/// block, or -- where `asked` is common rubble or dirt -- any rock's rubble of
/// that form, or any soil. `crafting` decides which rows let it.
#[inline]
pub fn stands_in_for(asked: BlockId, held: BlockId) -> bool {
    let (asked, held) = (block_kind(asked), block_kind(held));
    if asked == held {
        return true;
    }
    if asked == BLOCK_DIRT {
        return is_soil(held);
    }
    let common = ROCKS[0];
    if ![common.cobble, common.gravel, common.sand, common.pebble].contains(&asked) {
        return false;
    }
    matches!(rock_of(held), Some((_, Some(form))) if common.form(form) == asked)
}

/// Every block that stands in for `asked` other than itself, in table order.
pub fn stand_ins(asked: BlockId) -> impl Iterator<Item = BlockId> {
    let asked = block_kind(asked);
    let rubble = ROCKS.iter().skip(1).flat_map(|r| [r.cobble, r.gravel, r.sand, r.pebble]);
    rubble.chain(SOILS).filter(move |&held| held != asked && stands_in_for(asked, held))
}

/// Is `id` a rock's rubble or a soil -- a thing a row making it must name
/// exactly? See `crafting::own_rows`.
#[inline]
pub fn is_ground_form(id: BlockId) -> bool {
    is_soil(id) || rock_of(id).is_some_and(|(_, form)| form.is_some()) || block_kind(id) == BLOCK_DIRT
}

/// Will a hoe make a field of this? See the module doc.
#[inline]
pub fn tills(id: BlockId) -> bool {
    matches!(
        block_kind(id),
        BLOCK_GRASS | BLOCK_DIRT | BLOCK_SANDY_SOIL | BLOCK_DRY_TURF
            | BLOCK_LOAM | BLOCK_CHERNOZEM | BLOCK_LOESS | BLOCK_RENDZINA | BLOCK_ANDOSOL
    )
}

/// What each grass roots in. See the module doc for which country is whose.
#[inline]
pub fn grows_on(grass: BlockId, ground: BlockId) -> bool {
    let ground = block_kind(ground);
    let common = as_common(ground);
    match block_kind(grass) {
        BLOCK_FEATHER_GRASS => matches!(ground, BLOCK_CHERNOZEM | BLOCK_LOESS) || matches!(common, BLOCK_GRASS | BLOCK_DRY_TURF),
        BLOCK_SEDGE => matches!(common, BLOCK_MUD | BLOCK_GRASS | BLOCK_DIRT) || ground == BLOCK_GLEY,
        BLOCK_COTTON_GRASS => matches!(ground, BLOCK_PEAT | BLOCK_GLEY | BLOCK_PERMAFROST | BLOCK_SNOW) || common == BLOCK_GRASS,
        BLOCK_FESCUE => matches!(common, BLOCK_GRASS | BLOCK_DIRT | BLOCK_STONE | BLOCK_GRAVEL),
        BLOCK_MARRAM => common == BLOCK_SAND,
        BLOCK_ELEPHANT_GRASS => matches!(common, BLOCK_DRY_TURF | BLOCK_SANDY_SOIL | BLOCK_GRASS),
        BLOCK_BLUEGRASS | BLOCK_TIMOTHY => matches!(common, BLOCK_GRASS | BLOCK_DIRT),
        BLOCK_TUSSOCK_GRASS => matches!(ground, BLOCK_PEAT | BLOCK_PERMAFROST | BLOCK_SNOW) || matches!(common, BLOCK_GRASS | BLOCK_DIRT),
        BLOCK_SPINIFEX => matches!(common, BLOCK_SAND | BLOCK_SANDY_SOIL),
        _ => false,
    }
}

// ---- turf that wraps its sides ----

/// A block whose top is grass and whose sides are soil with a fringe: the
/// meadow's turf and the savanna's dry turf.
#[inline]
pub fn is_turf(id: BlockId) -> bool {
    matches!(block_kind(id), BLOCK_GRASS | BLOCK_DRY_TURF)
}

/// **Whether this turf block's side is grass all the way down**, rather than
/// soil with a fringe along its top: true when **the turf goes on past the
/// face at that level** -- either the cell in that direction is the same turf
/// standing lower in its cell (the next lip of a ramp), or that cell is open
/// and the one *diagonally below* is the same turf (the next whole step down
/// the hill).
///
/// "ÑÐ´ÐµÐ»Ð°Ð¹ ÑÐ°Ðº, ÑÑÐ¾Ð±Ñ Ð±Ð»Ð¾Ðº Ð·ÐµÐ¼Ð»Ð¸ Ñ Ð¿Ð¾Ð»Ð½Ð¾Ð¹ ÑÐµÐºÑÑÑÑÐ¾Ð¹ ÑÑÐ°Ð²Ñ Ð±ÑÐ» Ð¿Ð¾ Y, X, -X Ð¸
/// Z, -Z". A slope of turf is a staircase of whole cells, and every riser of
/// it showed a wall of soil: a green hill seen from its foot was brown bands
/// with a green line on each, and that is not what a hillside looks like.
///
/// Three ways to give it grass sides were weighed:
///
/// * *A side is always grass.* Then a bank a player digs into is a wall of
///   turf with grass growing on its underside, a cellar's ceiling edge is
///   green, and the soil under the meadow -- which is the whole reason dirt
///   is a block you can see -- is never visible. The picture that made the
///   request is a slope, not a cut.
/// * *A second block, "grassed on every side", that a player places.* That is
///   an id, a recipe, an inventory slot and a rule about which one the
///   generator lays, for a difference nobody can act on: a mechanic that
///   creates a chore rather than a decision (see `CLAUDE.md`). And the
///   generator would have to guess, at worldgen time, what the neighbours of
///   a cell will be after every later pass has run.
/// * **The turf decides per face, from what is on the other side of it
///   (chosen).** Grass grows over a brow and down the slope behind it; it
///   does not grow on the face of a cut. What is beyond the face is exactly
///   that difference: on a slope the turf carries on a quarter or a whole
///   block lower, at a cut or a cliff there is stone, air or bare soil. So a
///   hillside is green from its foot and a dug bank still shows its earth,
///   with no new block, no new picture and no new layer -- the side wears the
///   top's own picture, which already carries the climate's tint.
///
/// **Both halves of the slope are needed**, and the first was found missing
/// from a picture. A generated slope is a ramp of lips
/// (`worldgen::lips`): four columns of one cell whose turf stands a quarter,
/// a half, three quarters and a whole block up. Every riser there is a
/// quarter-block strip between two turf *cells at the same height*, so the
/// cell diagonally below is the soil under the ramp and the strip stayed
/// brown -- a dark line along every terrace of what was otherwise a green
/// hill. The cliff and the cut keep their soil because in both the
/// neighbouring cell is open air **and** the cell under it is not turf: a
/// cliff drops more than a block, and a dug face has earth behind it.
///
/// **The top has to be showing.** Under a skin of snow or a block laid on it,
/// what the player sees along the hill is white or stone, and a green side
/// under it would be a stripe of summer in a drift. `above` is the cell over
/// this one; a coating (`types::is_covering_flat`) or anything opaque keeps
/// the old soil side.
///
/// The item icon needs no thought for the same reason: a block in a hand has
/// no cell diagonally below, so it is drawn with its soil sides, which is
/// what one turf lifted out of a meadow actually looks like.
#[inline]
pub fn turf_wraps_the_side(here: BlockId, above: BlockId, beyond: BlockId, diagonal_below: BlockId) -> bool {
    let same = |other: BlockId| block_kind(other) == block_kind(here);
    turf_may_wrap(here, above) && (same(beyond) || same(diagonal_below))
}

/// The half of [`turf_wraps_the_side`] that is a question about the block
/// rather than about one of its faces -- is this turf, and is its top the
/// thing a player sees along the hill? A mesher asks this once per cell and
/// pays for the diagonal read only where it can matter.
#[inline]
pub fn turf_may_wrap(here: BlockId, above: BlockId) -> bool {
    is_turf(here) && !crate::types::is_covering_flat(above) && !crate::types::is_opaque(above)
}

// ---- moss ----

/// The bit of the variant that says a stone or a trunk is **mossy**.
///
/// The third bit, which nothing else on these blocks spends: a log's axis is
/// the low two (`ORIENTATION_MASK`), a stone has no variant, and cobble's soot
/// is a stage under four (`wildfire::soot`) that reads this bit as no soot.
///
/// Rejected: *a mossy block per block* -- a mossy log of six woods and a mossy
/// stone of fifteen rocks is twenty-one ids and twenty-one pictures for one
/// green face. Rejected too: *a cover block like snow* -- moss is on the
/// north face of a trunk, and a cover lies on a floor.
pub const MOSSY: BlockId = 0b100 << VARIANT_SHIFT;

/// May this block carry moss? Any wood's log, any rock, and any rock's
/// cobble -- the boulders the generator lays are cobble.
#[inline]
pub fn may_grow_moss(id: BlockId) -> bool {
    let kind = block_kind(id);
    crate::wood::is_log(kind) || ROCKS.iter().any(|r| r.stone == kind || r.cobble == kind)
}

/// Is this block wearing moss?
#[inline]
pub fn is_mossy(id: BlockId) -> bool {
    id & MOSSY != 0 && may_grow_moss(id)
}

/// The same block with moss on it; anything that cannot carry moss comes
/// back as it was.
#[inline]
pub fn with_moss(id: BlockId) -> BlockId {
    if may_grow_moss(id) {
        id | MOSSY
    } else {
        id
    }
}

/// What a hand scraping this block leaves: the block without its moss, or
/// `None` where there is no moss to scrape. The moss goes into the pack
/// (`BLOCK_MOSS`) -- see the server's `pick_by_hand`.
#[inline]
pub fn scraped(id: BlockId) -> Option<BlockId> {
    is_mossy(id).then_some(id & !MOSSY & (VARIANT_MASK | crate::types::KIND_MASK))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::{block_name, is_known_block, BLOCK_FIR_LOG, BLOCK_LOG};

    #[test]
    fn every_rock_breaks_into_its_own_cobble_gravel_sand_and_pebble_and_sandstone_into_sand() {
        let mut seen = std::collections::HashSet::new();
        for rock in ROCKS {
            for form in [Form::Cobble, Form::Gravel, Form::Sand, Form::Pebble] {
                let piece = rock.form(form);
                let name = block_name(piece);
                assert!(is_known_block(piece), "{name} is not a block");
                if rock.stone == BLOCK_SANDSTONE && form == Form::Sand {
                    assert_eq!(piece, BLOCK_SAND, "sandstone grew a sand of its own");
                    continue;
                }
                assert!(seen.insert(piece), "{name} is the rubble of two rocks");
                assert_eq!(rock_of(piece).map(|(r, f)| (r.stone, f)), Some((rock.stone, Some(form))), "{name} forgot its rock");
                assert_eq!(rubble_of(rock.stone, form), piece);
                assert_eq!(as_common(piece), ROCKS[0].form(form), "{name} stands in for the wrong common rubble");
            }
        }
    }

    #[test]
    fn any_rocks_rubble_stands_in_for_common_rubble_and_never_the_other_way() {
        for rock in &ROCKS[1..] {
            for form in [Form::Cobble, Form::Gravel, Form::Sand, Form::Pebble] {
                let piece = rock.form(form);
                assert!(stands_in_for(ROCKS[0].form(form), piece), "{} does not stand in", block_name(piece));
                if piece != ROCKS[0].form(form) {
                    assert!(!stands_in_for(piece, ROCKS[0].form(form)), "common rubble stood in for {}", block_name(piece));
                    assert!(stand_ins(ROCKS[0].form(form)).any(|b| b == piece));
                }
            }
        }
        assert!(!stands_in_for(BLOCK_COBBLESTONE, BLOCK_GRANITE_GRAVEL), "gravel stood in for cobble");
        assert!(!stands_in_for(BLOCK_STONE, BLOCK_GRANITE), "a rock stood in for stone in a recipe");
        for soil in SOILS {
            assert!(stands_in_for(BLOCK_DIRT, soil));
        }
    }

    #[test]
    fn a_hoe_makes_fields_of_black_and_brown_earth_and_not_of_sour_hard_salt_or_frozen_earth() {
        for soil in [BLOCK_CHERNOZEM, BLOCK_LOAM, BLOCK_LOESS, BLOCK_RENDZINA, BLOCK_ANDOSOL] {
            assert!(tills(soil), "{} will not till", block_name(soil));
        }
        for soil in [BLOCK_PODZOL, BLOCK_GLEY, BLOCK_LATERITE, BLOCK_SOLONCHAK, BLOCK_PERMAFROST] {
            assert!(!tills(soil), "{} tilled", block_name(soil));
        }
    }

    #[test]
    fn moss_is_a_bit_on_logs_and_stones_that_a_hand_takes_off_and_nothing_else_carries() {
        for id in [BLOCK_LOG, BLOCK_FIR_LOG, BLOCK_STONE, BLOCK_GRANITE, BLOCK_TUFF, BLOCK_COBBLESTONE] {
            let mossy = with_moss(id);
            assert!(is_mossy(mossy), "{} will not carry moss", block_name(id));
            assert!(is_known_block(mossy), "mossy {} is an invented id", block_name(id));
            assert_eq!(scraped(mossy), Some(id));
            assert_eq!(scraped(id), None, "bare {} gave moss", block_name(id));
        }
        // A lying log keeps its axis under the moss.
        let lying = crate::types::oriented(BLOCK_LOG, crate::types::Axis::X);
        assert_eq!(scraped(with_moss(lying)), Some(lying));
        assert_eq!(with_moss(BLOCK_SAND), BLOCK_SAND, "sand grew moss");
    }

    #[test]
    fn a_turf_side_is_grass_down_a_slope_and_soil_at_a_cut() {
        // "сделай так, чтобы блок земли с полной текстурой травы был по Y, X,
        // -X и Z, -Z". The rule the mesher draws a side by
        // (`turf_wraps_the_side`), stated as the situations a face is ever
        // in. It went red once already: with only the diagonal-below half of
        // it, a generated slope -- which is a ramp of lips inside one cell,
        // not a stair of whole ones -- kept a brown line along every terrace.
        use crate::types::{BLOCK_AIR, BLOCK_SNOW_COVER, BLOCK_STONE};
        let lip = crate::dig::lowered(BLOCK_GRASS, 2);
        // Down a ramp: the cell beyond is the same turf, standing lower.
        assert!(turf_wraps_the_side(BLOCK_GRASS, BLOCK_AIR, lip, BLOCK_DIRT));
        // Down a whole step: the cell beyond is open, the one under it turf.
        assert!(turf_wraps_the_side(BLOCK_GRASS, BLOCK_AIR, BLOCK_AIR, BLOCK_GRASS));
        // A cut: open beyond, earth under it.
        assert!(!turf_wraps_the_side(BLOCK_GRASS, BLOCK_AIR, BLOCK_AIR, BLOCK_DIRT));
        // A cliff: open beyond and open under it.
        assert!(!turf_wraps_the_side(BLOCK_GRASS, BLOCK_AIR, BLOCK_AIR, BLOCK_AIR));
        // Under snow the hillside is white, so the side stays soil.
        assert!(!turf_wraps_the_side(BLOCK_GRASS, BLOCK_SNOW_COVER, BLOCK_AIR, BLOCK_GRASS));
        // And under anything solid, where the top is not what is seen.
        assert!(!turf_wraps_the_side(BLOCK_GRASS, BLOCK_STONE, BLOCK_AIR, BLOCK_GRASS));
        // Two turfs are not one: the savanna's dry turf does not lend the
        // meadow its picture, nor the meadow the savanna.
        assert!(turf_wraps_the_side(BLOCK_DRY_TURF, BLOCK_AIR, BLOCK_AIR, BLOCK_DRY_TURF));
        assert!(!turf_wraps_the_side(BLOCK_DRY_TURF, BLOCK_AIR, BLOCK_AIR, BLOCK_GRASS));
        // Soil is not turf, whatever is beside it: a dirt block dug out of a
        // meadow keeps its own picture.
        assert!(!turf_wraps_the_side(BLOCK_DIRT, BLOCK_AIR, BLOCK_GRASS, BLOCK_GRASS));
    }
}
