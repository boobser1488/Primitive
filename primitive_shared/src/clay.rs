//! Raw pottery drying before it is fired: wet off the hands, leather-hard,
//! bone-dry -- and what a kiln does to a piece that went in too soon.
//!
//! ## What the player asked for
//!
//! "Сырая керамика должна сохнуть перед обжигом, иначе трескается." A pot
//! thrown and fired in the same breath was a pot a player never had to
//! think about: the kiln took it straight off the hands. Real clay holds a
//! fifth of its weight in water, and water in a wall of clay at seven
//! hundred degrees is steam with nowhere to go -- the pot spalls, or splits
//! from rim to foot. Potters have always set the day's work out to dry
//! first, in the sun if they are in a hurry and in the shade if they want
//! it not to warp, and the kiln is loaded a day or two later.
//!
//! ## The decision it makes
//!
//! **Fire now, or wait.** A wet piece is not refused: it goes into the kiln
//! and comes out fired *or cracked*, by a roll whose odds are how wet it
//! was ([`crack_chance`]) -- half the wet pieces, a few of the leather-hard,
//! none of the dry. A player with a kiln already hot and four pots on the
//! bench can gamble; one who planned ahead lays them out in the morning and
//! loses nothing. Where they dry is the other half of it: set down in the
//! sun and wind they are ready by the next day (`peat` on the server, whose
//! weather rules they share), left in a chest or a pack they get there in
//! two, and a shower on a piece lying out takes the drying back.
//!
//! ## Where the state lives
//!
//! **In the variant field of the piece's own id**, the way a haunch carries
//! its age (`food`, "going off"): three stages, so two bits, on items that
//! have no other use for the field -- a raw pot is never placed as a block
//! (`placeable: false`), so there is no facing to share it with. A stack
//! is still a block and a count, a chest still holds stacks, and the
//! picture and the name tell the stage without a new message.
//!
//! **Nought is wet**, and that was a choice between two readings of every
//! pot already in a save. Nought as bone-dry would have kept old worlds'
//! pots fireable as they were -- and made every recipe, the potter's wheel
//! and every test that makes a raw piece have to *say* it is wet, where the
//! plain id is what they all write. Nought as wet makes the plain id the
//! truth about a pot off the hands; what it costs an old world is that its
//! raw pottery has to dry once, which is two days in the chest it is in.
//!
//! Rejected: **a timestamp per stack**, for `food`'s reason -- a field on
//! every stack that means nothing on thirty-nine slots of forty.

use crate::types::{
    block_kind, BlockId, BLOCK_BOWL_RAW, BLOCK_BRICK_RAW, BLOCK_JUG_RAW, BLOCK_MOULD_RAW, BLOCK_VESSEL_RAW,
    VARIANT_MASK, VARIANT_SHIFT,
};

/// How far a piece of raw pottery has dried.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Dryness {
    /// Straight off the hands or the wheel: dark, soft, and the most likely
    /// to split in the fire.
    Wet = 0,
    /// Firm enough to handle without denting, still cool and damp inside.
    LeatherHard = 1,
    /// Pale and dry through: fires whole.
    BoneDry = 2,
}

impl Dryness {
    fn from_bits(bits: u8) -> Dryness {
        match bits {
            0 => Dryness::Wet,
            1 => Dryness::LeatherHard,
            _ => Dryness::BoneDry,
        }
    }
}

/// Is this a piece of shaped, unfired clay -- the things that dry?
///
/// The four a pit kiln takes (`pit::fires_into`) and the bowl, which only
/// the kiln fires.
#[inline]
pub fn is_raw_pottery(block: BlockId) -> bool {
    matches!(
        block_kind(block),
        BLOCK_VESSEL_RAW | BLOCK_MOULD_RAW | BLOCK_JUG_RAW | BLOCK_BRICK_RAW | BLOCK_BOWL_RAW
    )
}

/// How dry a piece is. Bone-dry for anything that is not raw pottery, so a
/// question asked of a fired pot or a stone has the answer that changes
/// nothing.
#[inline]
pub fn dryness(block: BlockId) -> Dryness {
    if !is_raw_pottery(block) {
        return Dryness::BoneDry;
    }
    Dryness::from_bits(((block & VARIANT_MASK) >> VARIANT_SHIFT) as u8)
}

/// The same piece at this stage. Anything else comes back unchanged.
#[inline]
pub fn with_dryness(block: BlockId, dryness: Dryness) -> BlockId {
    if !is_raw_pottery(block) {
        return block;
    }
    (block & !VARIANT_MASK) | ((dryness as BlockId) << VARIANT_SHIFT)
}

/// One stage drier; bone-dry stays bone-dry.
#[inline]
pub fn drier(block: BlockId) -> BlockId {
    match dryness(block) {
        Dryness::Wet => with_dryness(block, Dryness::LeatherHard),
        _ => with_dryness(block, Dryness::BoneDry),
    }
}

/// Is this raw pottery that still has drying to do?
#[inline]
pub fn is_drying(block: BlockId) -> bool {
    is_raw_pottery(block) && dryness(block) != Dryness::BoneDry
}

/// Are these bits a stage a piece can be at? The third value of the field
/// and above name nothing, and an id carrying them is a claim.
#[inline]
pub fn is_valid_variant(block: BlockId) -> bool {
    (block & VARIANT_MASK) >> VARIANT_SHIFT <= Dryness::BoneDry as BlockId
}

/// The chance a piece this wet splits in the fire.
///
/// **Half the wet ones, and not all of them.** "Cracks some of it" is the
/// player's own phrase, and it is what makes the kiln a gamble rather than
/// a wall: a rule that refused wet pottery would have one answer, and one
/// that cracked everything would be the same rule said with a loss. Half
/// is the odds a potter in a hurry actually takes; leather-hard is a
/// fifth-odd chance -- the outside is dry and the core is not -- and a
/// bone-dry piece never splits for being wet.
#[inline]
pub fn crack_chance(block: BlockId) -> f32 {
    match dryness(block) {
        Dryness::Wet => 0.5,
        Dryness::LeatherHard => 0.15,
        Dryness::BoneDry => 0.0,
    }
}

/// Does a piece this wet crack on a roll of `roll` in 0..1?
#[inline]
pub fn cracks(block: BlockId, roll: f32) -> bool {
    roll < crack_chance(block)
}

/// On how many of the world's four-a-day steps (`food::ROT_STEPS_PER_DAY`)
/// a piece in a pack, a chest or on the floor dries by one stage: every
/// fourth, a day a stage, two days wet to bone-dry.
///
/// **Slower than laid out in the sun, on purpose**, and slow enough to be
/// worth the walk out to set them down: out in fair weather a piece is dry
/// by the next morning (`peat::POTTERY_DRY_SECONDS` on the server). The
/// chest is the patient way and costs nothing but the wait; the yard is the
/// quick way and costs a shower's risk.
pub const DRIES_EVERY: u32 = 4;

/// The word the tooltip puts after the name: how dry the piece is.
pub fn label(block: BlockId) -> Option<&'static str> {
    if !is_raw_pottery(block) {
        return None;
    }
    Some(match dryness(block) {
        Dryness::Wet => "wet",
        Dryness::LeatherHard => "leather-hard",
        Dryness::BoneDry => "bone-dry",
    })
}

/// How a piece at this stage is shaded, as a colour the picture is
/// multiplied by: wet clay dark, leather-hard between, bone-dry the pale
/// picture itself. White for anything else.
///
/// **A shade and not three pictures a piece.** Five kinds at three stages
/// is ten more layers in an atlas that is counted (`CLAUDE.md`, "2048
/// texture layers"), for a difference that *is* a shade -- wet clay is the
/// same shape as dry clay, only darker.
pub fn shade(block: BlockId) -> [f32; 3] {
    if !is_raw_pottery(block) {
        return [1.0; 3];
    }
    match dryness(block) {
        Dryness::Wet => [0.58, 0.55, 0.53],
        Dryness::LeatherHard => [0.78, 0.75, 0.72],
        Dryness::BoneDry => [1.0; 3],
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::{is_known_block, BLOCK_VESSEL};

    const ALL: [BlockId; 5] = [BLOCK_VESSEL_RAW, BLOCK_MOULD_RAW, BLOCK_JUG_RAW, BLOCK_BRICK_RAW, BLOCK_BOWL_RAW];

    #[test]
    fn a_piece_off_the_hands_is_wet_and_dries_in_two_stages() {
        for raw in ALL {
            assert_eq!(dryness(raw), Dryness::Wet, "{} came off the hands dry", crate::types::block_name(raw));
            let leather = drier(raw);
            assert_eq!(dryness(leather), Dryness::LeatherHard);
            let dry = drier(leather);
            assert_eq!(dryness(dry), Dryness::BoneDry);
            assert_eq!(drier(dry), dry, "bone-dry went further");
            assert_eq!(block_kind(dry), raw, "drying changed what the piece is");
            assert!(!is_drying(dry) && is_drying(raw) && is_drying(leather));
        }
    }

    #[test]
    fn every_stage_is_a_known_block_and_the_fourth_is_not() {
        for raw in ALL {
            for stage in [Dryness::Wet, Dryness::LeatherHard, Dryness::BoneDry] {
                assert!(is_known_block(with_dryness(raw, stage)), "{} at {stage:?} is invented", crate::types::block_name(raw));
            }
            assert!(!is_known_block(raw | (3 << VARIANT_SHIFT)), "a fourth stage of drying was let through");
        }
    }

    #[test]
    fn a_wet_piece_is_likelier_to_crack_than_a_leather_hard_one_and_a_dry_one_never_does() {
        let wet = crack_chance(BLOCK_VESSEL_RAW);
        let leather = crack_chance(drier(BLOCK_VESSEL_RAW));
        let dry = crack_chance(drier(drier(BLOCK_VESSEL_RAW)));
        assert!(wet > leather && leather > dry, "{wet} {leather} {dry}");
        assert!(wet < 1.0, "every wet piece cracks: that is a refusal, not a chance");
        assert_eq!(dry, 0.0);
        // ...and nothing that is not raw clay is wet.
        assert_eq!(crack_chance(BLOCK_VESSEL), 0.0);
        assert_eq!(with_dryness(BLOCK_VESSEL, Dryness::Wet), BLOCK_VESSEL);
    }

    #[test]
    fn wet_clay_is_drawn_darker_than_dry_clay() {
        let wet = shade(BLOCK_JUG_RAW);
        let leather = shade(drier(BLOCK_JUG_RAW));
        let dry = shade(drier(drier(BLOCK_JUG_RAW)));
        assert!(wet[0] < leather[0] && leather[0] < dry[0]);
        assert_eq!(dry, [1.0; 3], "bone-dry is not its own picture");
    }
}
