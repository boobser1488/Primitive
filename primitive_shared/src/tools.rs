//! What a tool is like to *use*, as against what it can open.
//!
//! ## Three facts that used to be one number
//!
//! `blocks::Tier` answers **what a tool gets into**: iron ore wants copper,
//! basalt wants bronze, a standing trunk wants a wedged head. For two
//! versions the same enum also answered how fast a tool worked, and --
//! through the server's `hunting_damage` -- how hard a spear hit. Those are
//! three different facts about three different things, and one number
//! carrying all three is how they drifted apart: a spear's thrust was
//! `MELEE × 2.5 × Tier::Iron.speed()`, the weight of a hunter behind two
//! metres of haft multiplied by how quickly an iron *pick* opens rock. Make
//! the pick honest and every spear in the game got weaker, which nobody
//! asked for and no test would have said.
//!
//! So the gate stays on the tier, and everything else is here:
//!
//! * **speed** -- the tier's plain material, times whether the metal is
//!   steeled, times how sharp the edge still is ([`speed`]);
//! * **the edge** -- four steps from sharp to blunt, carried in the id
//!   ([`blunt_step`], [`after_a_swing`]);
//! * **honing** -- the edge back, paid for in metal ([`hone`]);
//! * **damage** -- a blade's factor and a spear's thrust, as named numbers
//!   of their own ([`blade_factor`], [`SPEAR_THRUST`]).
//!
//! ## Iron is not better than bronze. Steel is.
//!
//! The ladder used to put iron half as fast again as bronze, and that is the
//! one rung of it that is not true. Work-hardened tin bronze is about 200–230
//! on the Vickers scale; bloomery iron, worked, is about the same, and softer
//! where the slag strings run through it. What iron had was *availability* --
//! bog ore lies in every wet valley and tin was carried across continents --
//! and one thing bronze can never do: take carbon in a closed fire and harden
//! in water, to 600–800. So wrought iron here is a shade better than bronze
//! (the gate still ranks it higher) and a steeled edge is the jump. The iron
//! age is still the best age; it is the *steel* that makes it so, and the
//! bloomery alone is a way to stop needing tin.
//!
//! ## Why the edge is in the id
//!
//! Three places to keep "how blunt", and two of them do not work:
//!
//! 1. **Worked out from the wear.** A worn tool is a blunt tool, no new state
//!    at all. But mining time is decided from `(block, held id)` by three
//!    parties a frame apart -- the client's bar, the server's refusal, the
//!    stamina bill -- and none of them has the stack, only the id. Passing the
//!    stack through all three is a signature change in the client, the server
//!    and the mod boundary, for one multiplication.
//! 2. **A counter in the high bits of `Stack::damage`.** Every reader of that
//!    field reads swings, and `Inventory::sanitize` clamps it to the tool's
//!    durability on every load -- the counter would be erased the first time
//!    a save was opened.
//! 3. **The variant field**, which is what the poison on a spear and the age
//!    of a steak already live in. A tool stacks to one, so there is never a
//!    pile of axes that disagree about their edge. Chosen. What it costs is
//!    that a tool's id changes every [`edge_swings`] swings, and the server
//!    already sends the pack after every swing that wears anything.
//!
//! ## Why honing eats the rest of the edge
//!
//! A hone that simply reset the edge would be a click after every step of
//! blunting, which is a chore; a hone that restored wear would make a tool
//! last for ever, which is a different game. What a whetstone really does is
//! grind metal away until there is a fresh edge under it, so here the wear
//! is carried on to the *next* step boundary: the life of the tool never
//! grows, and the price of honing is whatever was left of the edge you ground
//! off. Hone a tool the moment it dulls and you have thrown away a whole edge;
//! work it blunt nearly to the next step and honing is almost free, but you
//! worked blunt. That is a decision, and it is a different one in a mine far
//! from home (speed) than in a yard with a spare pick on the wall (metal).

use crate::blocks::Tier;
use crate::inventory::Stack;
use crate::types::{
    block_kind, BlockId, BLOCK_BRONZE_AXE, BLOCK_BRONZE_KNIFE, BLOCK_BRONZE_PICKAXE,
    BLOCK_COPPER_AXE, BLOCK_COPPER_KNIFE, BLOCK_COPPER_PICKAXE, BLOCK_COPPER_SHOVEL,
    BLOCK_IRON_AXE, BLOCK_IRON_KNIFE, BLOCK_IRON_PICKAXE, BLOCK_STONE_AXE, BLOCK_STONE_PICKAXE,
    BLOCK_WEDGED_AXE, BLOCK_WEDGED_PICKAXE, VARIANT_MASK, VARIANT_SHIFT,
};

/// The two variant bits that say how blunt a tool is: 0 sharp, 3 blunt.
pub const EDGE_MASK: BlockId = 0b011 << VARIANT_SHIFT;

/// The third bit, on iron tools only: carburised and quenched. See the
/// "steel" rows in `crafting`.
///
/// **The bit a spear spends on its poison** (`types::POISONED` is the low
/// bit), and there is no clash: a spear takes no edge here, and nothing that
/// takes an edge is a weapon.
pub const HARDENED: BlockId = 0b100 << VARIANT_SHIFT;

/// The bluntest step. An edge stops getting worse here and the tool goes on
/// wearing until it breaks.
pub const BLUNTEST: u8 = 3;

/// How fast a tool works at each step of its edge.
///
/// Halved at the bluntest, and not worse: a blunt axe still fells a tree, it
/// just bruises its way through, and a tool that stopped working before it
/// broke would be the "broken tool item" `inventory::wear_tool` refuses to
/// have.
const EDGE_SPEED: [f32; 4] = [1.0, 0.8, 0.65, 0.5];

/// What a steeled edge adds to the speed of the iron under it. Wrought iron
/// is 4.4 (`Tier::speed`), steel is 6.6 -- ten per cent past what iron was
/// before the ladder was made honest, and the reason to build the kiln twice.
pub const STEEL_SPEED: f32 = 1.5;

/// ...and how much longer the hardened edge holds before it dulls a step.
pub const STEEL_EDGE: u32 = 2;

/// **The spear's thrust, as a number of its own.**
///
/// Fifteen punches for a flint point, which is exactly what
/// `2.5 × Tier::Iron.speed()` came to while iron was 6.0 -- so no hunt in any
/// save changes. What changes is that it is no longer a mining speed: the
/// thrust is the haft and the hunter, and the head moves it a quarter at a
/// time (the server's `hunting_damage`).
pub const SPEAR_THRUST: f32 = 15.0;

/// How many swings one step of edge lasts, or `None` for a tool that takes
/// no edge at all.
///
/// **Copper dulls first**, and that is the whole character of the metal:
/// pure copper work-hardens to about 100–120 on the Vickers scale and rolls
/// over on the first stone it meets, so a copper tool spends its life on the
/// whetstone. Bronze holds an edge nearly three times as long; wrought iron a
/// little less than bronze; steel twice as long as iron.
///
/// **Ground stone dulls and is reground**, which is how a stone axe was kept.
///
/// Not here, deliberately:
/// * the flint knife -- a knapped edge does not dull, it chips, and a chipped
///   flint is re-struck rather than honed. Its short life (`durability`) is
///   that chipping;
/// * the spears, whose point is a point and whose paste lives in the bit this
///   would need;
/// * the two hoes, which are implements rather than tools (`types::is_implement`)
///   and are worn by the furrow rather than by a swing;
/// * the flint chisel, for the flint knife's reason;
/// * the hammers, which have a face and not an edge.
///
/// **The bronze chisel and the three saws are here**, and they are worn by
/// the work they are held for (`crafting::used_tool`, the sawhorse) rather
/// than by a swing -- a point of wear a board or a log. Their steps are
/// counted on the same wear, so a saw dulls after thirty logs of copper as an
/// axe does after thirty trees, and a dull one is what the sawhorse feels
/// (`minigame::tolerance`) and the bench's quality roll reads
/// (`crafting::tool_goodness`).
pub fn edge_swings(id: BlockId) -> Option<u32> {
    use crate::types::{BLOCK_BRONZE_CHISEL, BLOCK_BRONZE_SAW, BLOCK_COPPER_SAW, BLOCK_IRON_SAW};
    let base = match block_kind(id) {
        BLOCK_STONE_AXE | BLOCK_STONE_PICKAXE | BLOCK_WEDGED_AXE | BLOCK_WEDGED_PICKAXE => 30,
        BLOCK_COPPER_KNIFE | BLOCK_COPPER_AXE | BLOCK_COPPER_PICKAXE | BLOCK_COPPER_SHOVEL | BLOCK_COPPER_SAW => 30,
        BLOCK_BRONZE_KNIFE | BLOCK_BRONZE_AXE | BLOCK_BRONZE_PICKAXE | BLOCK_BRONZE_SAW | BLOCK_BRONZE_CHISEL => 80,
        BLOCK_IRON_KNIFE | BLOCK_IRON_AXE | BLOCK_IRON_PICKAXE | BLOCK_IRON_SAW => 70,
        _ => return None,
    };
    Some(if is_hardened(id) { base * STEEL_EDGE } else { base })
}

/// Does this kind of thing have an edge that dulls and is honed?
#[inline]
pub fn takes_an_edge(id: BlockId) -> bool {
    edge_swings(block_kind(id)).is_some()
}

/// Can this tool be steeled? Iron, and only iron: carbon goes into iron in a
/// closed fire and into nothing else this world smelts.
#[inline]
pub fn can_be_steeled(id: BlockId) -> bool {
    matches!(block_kind(id), BLOCK_IRON_KNIFE | BLOCK_IRON_AXE | BLOCK_IRON_PICKAXE)
}

/// Is this an iron tool with a hardened edge?
#[inline]
pub fn is_hardened(id: BlockId) -> bool {
    can_be_steeled(id) && id & HARDENED != 0
}

/// The same iron tool, steeled. Anything else comes back unchanged.
#[inline]
pub fn steeled(id: BlockId) -> BlockId {
    if can_be_steeled(id) {
        id | HARDENED
    } else {
        id
    }
}

/// How blunt: 0 is sharp, [`BLUNTEST`] is blunt. Zero for anything that
/// takes no edge, which is the answer that slows nothing down.
#[inline]
pub fn blunt_step(id: BlockId) -> u8 {
    if takes_an_edge(id) {
        ((id & EDGE_MASK) >> VARIANT_SHIFT) as u8
    } else {
        0
    }
}

/// The same tool at another step of its edge.
#[inline]
pub fn with_edge(id: BlockId, step: u8) -> BlockId {
    if !takes_an_edge(id) {
        return id;
    }
    (id & !EDGE_MASK) | ((step.min(BLUNTEST) as BlockId) << VARIANT_SHIFT)
}

/// Whether the variant bits on a tool that takes an edge mean anything.
///
/// Asked by `types::is_known_block`, which is what the anti-cheat and the
/// save loader trust: the edge's two bits on every such tool, the hardened
/// bit on iron and nowhere else -- a steeled copper axe is two ids for one
/// thing and a client inventing one.
pub fn is_valid_variant(id: BlockId) -> bool {
    let variant = id & VARIANT_MASK;
    if !takes_an_edge(id) {
        return variant == 0;
    }
    let allowed = if can_be_steeled(id) { EDGE_MASK | HARDENED } else { EDGE_MASK };
    variant & !allowed == 0
}

/// How fast this works, against a block its tool already brings `tier` to.
///
/// `tier` is `types::tool_tier_against`'s answer, so a tool held against the
/// wrong kind of work arrives here as `Hand` and works at a hand's speed --
/// sharp or blunt, a pick is no use on a trunk either way.
pub fn speed(tier: Tier, held: Option<BlockId>) -> f32 {
    let base = tier.speed();
    let Some(held) = held.filter(|_| tier != Tier::Hand) else {
        return base;
    };
    let steel = if is_hardened(held) { STEEL_SPEED } else { 1.0 };
    base * steel * EDGE_SPEED[blunt_step(held) as usize]
}

/// What one swing did to the edge: the id the tool should carry now that its
/// wear is `damage`.
///
/// **A step at every multiple of the edge**, counted on the same wear the
/// durability counts, so there is no second counter to keep: honing carries
/// the wear to a boundary (see [`hone`]) and the next step comes exactly one
/// edge later.
pub fn after_a_swing(id: BlockId, damage: u32) -> BlockId {
    let Some(edge) = edge_swings(id) else {
        return id;
    };
    let step = blunt_step(id);
    if damage > 0 && damage.is_multiple_of(edge) && step < BLUNTEST {
        with_edge(id, step + 1)
    } else {
        id
    }
}

/// A tool honed on a whetstone: sharp again, and worn on to the next
/// boundary of its edge. See the module note for why the wear goes *up*.
///
/// Never past the last swing: a hone does not break the tool it is sharpening,
/// it leaves it one swing from breaking.
pub fn hone(stack: Stack) -> Stack {
    let Some(edge) = edge_swings(stack.block) else {
        return stack;
    };
    // This tool's own life, not its kind's: a fine axe has more swings in
    // it (`quality::durability_scale`), so the boundary a hone may push it
    // to is further out. Reading the kind's would have honed a fine axe to
    // a plain one's last swing and thrown the rest of it away.
    let life = stack.life().unwrap_or(u32::MAX);
    let next = (stack.wear() / edge + 1).saturating_mul(edge);
    Stack {
        block: with_edge(stack.block, 0),
        count: stack.count,
        damage: next.min(life.saturating_sub(1)),
    }
    .with_quality(stack.quality())
}

/// How much of a sharp tool's work this one still does, 0.5..1: the same
/// share the edge takes off digging ([`speed`]) and cutting
/// ([`blade_factor`]). One for anything that takes no edge.
///
/// Public for the places the edge reaches that are not a swing: the width
/// of a saw's sweet spot at the sawhorse (`minigame::tolerance`) and how
/// good a chisel's work is at the bench (`crafting::tool_goodness`).
pub fn edge_factor(id: BlockId) -> f32 {
    EDGE_SPEED[blunt_step(id) as usize]
}

/// A tool honed at the honing stone, by a hand whose run came to `verdict`
/// (`minigame::Game::Whet`).
///
/// **A fair run is exactly the whetstone's hone** ([`hone`]): sharp, and
/// worn on to the next boundary of its edge. The mini-game is never a worse
/// deal than the menu -- the promise `minigame::verdict` makes for every
/// station.
///
/// **A fine run takes the edge back with almost no metal**: sharp, and worn
/// a quarter of the way to the boundary instead of all of it. What it gives
/// up is the length of the new edge -- the next step still falls on the
/// boundary (`after_a_swing` counts on the wear), so a tool honed finely
/// dulls again sooner. That is the trade a careful grinder makes for real:
/// a stroke that takes off a whisker of steel sets a fine edge on the old
/// bevel, and it is the bevel that wears away. Over the life of the tool it
/// is strictly more swings; in the day it is another trip to the stone.
///
/// **A ruined run grinds the metal and misses the bevel**: worn on to the
/// boundary as the whetstone would, and only one step sharper.
///
/// Rejected: *a fine hone that also lengthened the new edge*. The edge is
/// counted on the wear with no second counter (see the module note), so a
/// longer edge could only be wear given back -- a tool that lived longer the
/// more often it was sharpened, which is the "hone that restores wear"
/// already turned down above.
pub fn hone_by(stack: Stack, verdict: crate::minigame::Verdict) -> Stack {
    use crate::minigame::Verdict;
    let Some(edge) = edge_swings(stack.block) else {
        return stack;
    };
    let step = blunt_step(stack.block);
    match verdict {
        Verdict::Fair => hone(stack),
        Verdict::Ruined => {
            let honed = hone(stack);
            Stack { block: with_edge(honed.block, step.saturating_sub(1)), ..honed }
        }
        Verdict::Fine => {
            let life = stack.life().unwrap_or(u32::MAX);
            let wear = stack.wear();
            let next = (wear / edge + 1).saturating_mul(edge);
            let ground = (next - wear) / 4;
            Stack {
                block: with_edge(stack.block, 0),
                count: stack.count,
                damage: (wear + ground).min(life.saturating_sub(1)).max(wear),
            }
            .with_quality(stack.quality())
        }
    }
}

/// How hard a blade cuts, as a multiple of a punch, before the edge.
///
/// **The numbers the hunt was balanced against**, kept when the mining ladder
/// moved: they were `Tier::speed` while iron was 6.0, and every animal's
/// health in the game was tuned to them. What a better metal gives a knife is
/// a better edge, and what a blunt one takes away is the same share it takes
/// from digging -- a blunt knife is a worse knife, which is the one place the
/// edge reaches the hunt.
pub fn blade_factor(tier: Tier, held: BlockId) -> f32 {
    let metal = match tier {
        Tier::Hand => 1.0,
        Tier::Stone => 1.4,
        Tier::Flint => 2.0,
        Tier::Copper => 2.4,
        Tier::Bronze => 4.0,
        Tier::Iron => 6.0,
    };
    metal * EDGE_SPEED[blunt_step(held) as usize]
}

/// What a player is told about a tool beyond its name: steeled, and how
/// blunt. `None` for a sharp plain tool and for anything that is not one.
///
/// The picture cannot say it -- a steeled axe and a wrought one are the same
/// shape, and a dull edge is a few texels -- and the difference is the whole
/// of why a player would pick one out of the chest rather than the other.
pub fn label(id: BlockId) -> Option<&'static str> {
    if !takes_an_edge(id) {
        return None;
    }
    match (is_hardened(id), blunt_step(id)) {
        (false, 0) => None,
        (false, 1) => Some("dulled"),
        (false, 2) => Some("dull"),
        (false, _) => Some("blunt"),
        (true, 0) => Some("steeled"),
        (true, 1) => Some("steeled, dulled"),
        (true, 2) => Some("steeled, dull"),
        (true, _) => Some("steeled, blunt"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::{break_seconds_with, is_known_block, tool_durability, BLOCK_IRON_ORE, BLOCK_LOG, BLOCK_STONE};

    #[test]
    fn a_blunt_tool_is_slower_and_a_honed_one_is_as_fast_as_new() {
        let sharp = break_seconds_with(BLOCK_STONE, Some(BLOCK_BRONZE_PICKAXE)).unwrap();
        let mut last = sharp;
        for step in 1..=BLUNTEST {
            let blunt = break_seconds_with(BLOCK_STONE, Some(with_edge(BLOCK_BRONZE_PICKAXE, step))).unwrap();
            assert!(blunt > last, "step {step} of the edge is no slower than the one before it");
            last = blunt;
        }
        assert!(last <= sharp * 2.0 + 1e-4, "a blunt pick stopped working rather than working slowly");

        let blunt = Stack::worn(with_edge(BLOCK_BRONZE_PICKAXE, BLUNTEST), 1, 250);
        let honed = hone(blunt);
        assert_eq!(blunt_step(honed.block), 0);
        assert_eq!(
            break_seconds_with(BLOCK_STONE, Some(honed.block)),
            Some(sharp),
            "a honed pick is not as quick as a new one"
        );
    }

    #[test]
    fn honing_grinds_off_the_rest_of_the_edge_and_never_adds_life() {
        // The price, in both directions. A pick honed the swing after it
        // dulled has thrown a whole edge away; one worked nearly to the next
        // step pays almost nothing for its edge -- and neither of them is a
        // single swing further from breaking than it was.
        let edge = edge_swings(BLOCK_COPPER_PICKAXE).unwrap();
        let just_dulled = hone(Stack::worn(with_edge(BLOCK_COPPER_PICKAXE, 1), 1, edge));
        assert_eq!(just_dulled.damage, 2 * edge, "honing a fresh dull step was not dear");
        let nearly_next = hone(Stack::worn(with_edge(BLOCK_COPPER_PICKAXE, 1), 1, 2 * edge - 1));
        assert_eq!(nearly_next.damage, 2 * edge, "honing at the end of a step was not cheap");

        let life = tool_durability(BLOCK_COPPER_PICKAXE).unwrap();
        for damage in 0..life {
            let honed = hone(Stack::worn(with_edge(BLOCK_COPPER_PICKAXE, 2), 1, damage));
            assert!(honed.damage >= damage, "honing at {damage} gave wear back");
            assert!(honed.damage < life, "honing at {damage} broke the pick");
        }
    }

    #[test]
    fn a_fine_hand_at_the_stone_grinds_less_than_the_whetstone_and_a_ruined_one_no_less() {
        use crate::minigame::Verdict;
        let edge = edge_swings(BLOCK_BRONZE_AXE).unwrap();
        let life = tool_durability(BLOCK_BRONZE_AXE).unwrap();
        for wear in [edge, edge + 3, 2 * edge + edge / 2, life - 2] {
            let dull = Stack::worn(with_edge(BLOCK_BRONZE_AXE, 2), 1, wear);
            let fair = hone_by(dull, Verdict::Fair);
            let fine = hone_by(dull, Verdict::Fine);
            let ruined = hone_by(dull, Verdict::Ruined);
            assert_eq!(fair, hone(dull), "a fair run is not the whetstone's hone");
            assert!(fine.damage <= fair.damage, "a fine run at {wear} ground more than the whetstone");
            assert!(fine.damage >= dull.damage, "a fine run at {wear} gave wear back");
            assert!(fine.damage < life && fair.damage < life && ruined.damage < life, "a hone broke the axe");
            assert_eq!(blunt_step(fine.block), 0);
            assert_eq!(blunt_step(ruined.block), 1, "a ruined run sharpened as well as a good one");
            assert!(ruined.damage >= fair.damage, "a ruined run was cheaper than a fair one");
            // A fine edge dulls where the old one would have: on the boundary.
            let next = (wear / edge + 1) * edge;
            if next < life {
                assert_eq!(blunt_step(after_a_swing(fine.block, next)), 1, "a fine hone moved the boundary");
            }
        }
        // ...and a saw and a bronze chisel take an edge, where a flint chisel chips.
        assert!(takes_an_edge(crate::types::BLOCK_COPPER_SAW) && takes_an_edge(crate::types::BLOCK_BRONZE_CHISEL));
        assert!(!takes_an_edge(crate::types::BLOCK_FLINT_CHISEL) && !takes_an_edge(crate::types::BLOCK_STONE_HAMMER));
    }

    #[test]
    fn an_edge_dulls_a_step_at_a_time_and_stops_at_blunt() {
        let edge = edge_swings(BLOCK_IRON_AXE).unwrap();
        let mut id = BLOCK_IRON_AXE;
        for damage in 1..=edge * 6 {
            id = after_a_swing(id, damage);
            let expected = (damage / edge).min(BLUNTEST as u32) as u8;
            assert_eq!(blunt_step(id), expected, "at {damage} swings");
        }
        // ...and the edge is the same step long after a hone as before one.
        let honed = hone(Stack::worn(id, 1, edge * 3 + 5));
        let mut id = honed.block;
        for damage in honed.damage + 1..honed.damage + edge {
            id = after_a_swing(id, damage);
            assert_eq!(blunt_step(id), 0, "a honed edge dulled early, at {damage}");
        }
        assert_eq!(blunt_step(after_a_swing(id, honed.damage + edge)), 1);
    }

    #[test]
    fn copper_dulls_before_bronze_and_steel_holds_longest() {
        let copper = edge_swings(BLOCK_COPPER_AXE).unwrap();
        let bronze = edge_swings(BLOCK_BRONZE_AXE).unwrap();
        let iron = edge_swings(BLOCK_IRON_AXE).unwrap();
        let steel = edge_swings(steeled(BLOCK_IRON_AXE)).unwrap();
        assert!(copper < iron && iron <= bronze && bronze < steel, "{copper} {iron} {bronze} {steel}");
        assert_eq!(edge_swings(crate::types::BLOCK_FLINT_KNIFE), None, "knapped flint chips, it does not dull");
        assert_eq!(edge_swings(crate::types::BLOCK_FLINT_SPEAR), None, "the spear's bit is the poison's");
    }

    #[test]
    fn wrought_iron_is_bronze_and_steel_is_the_jump() {
        // The claim in the module note, as the times a player sees on a rock.
        let bronze = break_seconds_with(BLOCK_IRON_ORE, Some(BLOCK_BRONZE_PICKAXE)).unwrap();
        let iron = break_seconds_with(BLOCK_IRON_ORE, Some(BLOCK_IRON_PICKAXE)).unwrap();
        let steel = break_seconds_with(BLOCK_IRON_ORE, Some(steeled(BLOCK_IRON_PICKAXE))).unwrap();
        assert!(iron <= bronze, "wrought iron is slower than bronze");
        assert!(iron > bronze * 0.8, "wrought iron is a whole age ahead of bronze again");
        assert!(steel < iron * 0.7, "steeling an iron pick is not worth a kiln");
        // A steeled axe is an axe: no faster on rock than a hand is.
        assert_eq!(
            break_seconds_with(BLOCK_STONE, Some(steeled(BLOCK_IRON_AXE))),
            None,
            "a steeled axe opened rock"
        );
        assert!(break_seconds_with(BLOCK_LOG, Some(steeled(BLOCK_IRON_AXE))).is_some());
    }

    #[test]
    fn the_variant_bits_of_a_tool_mean_its_edge_and_iron_may_be_steeled() {
        for step in 0..=BLUNTEST {
            assert!(is_known_block(with_edge(BLOCK_COPPER_AXE, step)), "a copper axe at step {step}");
            assert!(is_known_block(with_edge(steeled(BLOCK_IRON_PICKAXE), step)));
        }
        assert!(!is_known_block(BLOCK_COPPER_AXE | HARDENED), "steeled copper is an invented id");
        assert!(!is_known_block(crate::types::BLOCK_FLINT_KNIFE | (1 << VARIANT_SHIFT)));
        assert_eq!(steeled(BLOCK_BRONZE_KNIFE), BLOCK_BRONZE_KNIFE);
        assert_eq!(label(BLOCK_IRON_AXE), None);
        assert_eq!(label(steeled(BLOCK_IRON_AXE)), Some("steeled"));
        assert_eq!(label(with_edge(BLOCK_COPPER_KNIFE, BLUNTEST)), Some("blunt"));
    }

    #[test]
    fn a_spear_thrust_does_not_move_when_the_mining_ladder_does() {
        // What the audit found: the thrust was a mining speed. Fifteen is what
        // it came to, and it has to stay fifteen whatever iron is worth at a
        // rock face.
        assert_eq!(SPEAR_THRUST, 15.0);
        assert!(Tier::Iron.speed() < 6.0, "the ladder did not move, so this test proves nothing");
        assert_eq!(blade_factor(Tier::Iron, BLOCK_IRON_KNIFE), 6.0, "the iron knife's cut moved with the pick");
    }
}
