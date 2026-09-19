//! What the server tells a player, as a code the client says in their
//! language (`ServerMessage::Notice`).
//!
//! ## Why codes
//!
//! Every refusal and notice the server sent used to be an English sentence
//! in `ServerMessage::Error` or a server chat line -- "the wood is wet", "it
//! will not stand for the knife" -- and a game that ships in Russian and
//! Polish told a Russian player why their fire would not light in English.
//! The words belong to the player's language, which only the client knows;
//! the server knows only *which* thing happened. `rack::Trade`,
//! `stall::Refusal` and `ServerMessage::Caught` already worked this way, each
//! with a message of its own; this is the one message for everything else,
//! so a new refusal is one variant here and one row in the client's
//! `ui::lang` rather than a new message on the wire.
//!
//! ## Rejected
//!
//! * *The client translating the English it is sent.* A table keyed by the
//!   sentence breaks silently the day somebody fixes a typo on the server.
//! * *A message per subject* (`HorseRefused`, `WetRefused`, ...), the way the
//!   rack and the stall went. Each would be an arm in the client's match that
//!   does the same thing -- put the words in the banner -- and a protocol
//!   change every time a subject is added.
//! * *The English kept here beside the code.* It would be a second copy of the
//!   client's English row, and the two would drift; the server's own tests
//!   compare codes, which is the stronger check anyway.
//!
//! **Appended only.** bincode sends a variant as its index, so a variant put
//! in the middle would make every later one say something else to a client a
//! version behind.

use serde::{Deserialize, Serialize};

/// Something the server tells one player. See the module note.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum Notice {
    // ---- tending an animal (`Animals::tend`) ----
    NothingInHand,
    AnimalGone,
    TooFarAway,
    CannotBeKept,
    NothingToShear,
    TameItFirst,
    FleeceNotGrown,
    OnlyEweGivesMilk,
    NoMilkYet,
    ShiesAway,
    NotHungry,
    DoesNotEatThat,
    // ---- horses (`Animals::saddle_up`, `Animals::mount`, `horses`) ----
    GoesOnAHorse,
    BreakItFirst,
    TooYoungToCarry,
    AlreadySaddled,
    AlreadyBagged,
    AlreadyRiding,
    CannotRideThat,
    SomebodyOnIt,
    TooYoungToRide,
    GentleItFirst,
    LetItSettle,
    HorseThrowsYou,
    HorseIsYours,
    HorseTakesFood,
    NoBagsInReach,
    HorseGone,
    NothingToUnbuckle,
    PackCannotTakeBags,
    // ---- the wet (`wet`) ----
    WoodWet,
    FuelWet,
    TorchWet,
    // ---- the pack and the vessels ----
    PackFull,
    JugNotEmpty,
    BarrelFull,
    BarrelEmpty,
    EmptyRucksackFirst,
    NowhereForBowl,
    // ---- tools and stations ----
    ToolBroke,
    NeedsAKnife,
    NeedHammerAtAnvil,
    NeedSawAtSawhorse,
    HoldBladeToSharpen,
    NotAtStation,
    NotARun,
    NothingOnAnvil,
    PieceWentCold,
    // ---- the field ----
    FurrowHasAsh,
    AshDugIn,
    // ---- sleeping and sitting ----
    AlreadyAsleep,
    HurtCannotSleep,
    TooHungryToSleep,
    TooThirstyToSleep,
    BedTaken,
    SeatTaken,
    NoRoomToSit,
    // ---- rafts, traps, lines, lids ----
    StepAboardForOars,
    RaftAlreadyThere,
    TrapEmpty,
    LineParted,
    WillNotOpen,
    // ---- appended ----
    NoRoomToTakeOff,
    TooMuchLyingAround,
    SetDownOnSolidGround,
    RaftNeedsOpenWater,
}

impl Notice {
    /// Every notice, for the client's test that each has its words in every
    /// language: a notice with no row prints `???`.
    pub const ALL: &'static [Notice] = &[
        Notice::NothingInHand,
        Notice::AnimalGone,
        Notice::TooFarAway,
        Notice::CannotBeKept,
        Notice::NothingToShear,
        Notice::TameItFirst,
        Notice::FleeceNotGrown,
        Notice::OnlyEweGivesMilk,
        Notice::NoMilkYet,
        Notice::ShiesAway,
        Notice::NotHungry,
        Notice::DoesNotEatThat,
        Notice::GoesOnAHorse,
        Notice::BreakItFirst,
        Notice::TooYoungToCarry,
        Notice::AlreadySaddled,
        Notice::AlreadyBagged,
        Notice::AlreadyRiding,
        Notice::CannotRideThat,
        Notice::SomebodyOnIt,
        Notice::TooYoungToRide,
        Notice::GentleItFirst,
        Notice::LetItSettle,
        Notice::HorseThrowsYou,
        Notice::HorseIsYours,
        Notice::HorseTakesFood,
        Notice::NoBagsInReach,
        Notice::HorseGone,
        Notice::NothingToUnbuckle,
        Notice::PackCannotTakeBags,
        Notice::WoodWet,
        Notice::FuelWet,
        Notice::TorchWet,
        Notice::PackFull,
        Notice::JugNotEmpty,
        Notice::BarrelFull,
        Notice::BarrelEmpty,
        Notice::EmptyRucksackFirst,
        Notice::NowhereForBowl,
        Notice::ToolBroke,
        Notice::NeedsAKnife,
        Notice::NeedHammerAtAnvil,
        Notice::NeedSawAtSawhorse,
        Notice::HoldBladeToSharpen,
        Notice::NotAtStation,
        Notice::NotARun,
        Notice::NothingOnAnvil,
        Notice::PieceWentCold,
        Notice::FurrowHasAsh,
        Notice::AshDugIn,
        Notice::AlreadyAsleep,
        Notice::HurtCannotSleep,
        Notice::TooHungryToSleep,
        Notice::TooThirstyToSleep,
        Notice::BedTaken,
        Notice::SeatTaken,
        Notice::NoRoomToSit,
        Notice::StepAboardForOars,
        Notice::RaftAlreadyThere,
        Notice::TrapEmpty,
        Notice::LineParted,
        Notice::WillNotOpen,
        Notice::NoRoomToTakeOff,
        Notice::TooMuchLyingAround,
        Notice::SetDownOnSolidGround,
        Notice::RaftNeedsOpenWater,
    ];

    /// Whether this is news rather than a refusal: said the same way, but
    /// it does not end a run at a station the way a refusal does (the
    /// client's `ServerMessage::Notice` arm).
    pub fn is_news(self) -> bool {
        matches!(self, Notice::HorseIsYours | Notice::HorseTakesFood | Notice::AshDugIn | Notice::HorseThrowsYou)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// `ALL` is what the client's rows are checked against, so a notice left
    /// out of it is a notice nobody checks has any words.
    #[test]
    fn every_notice_is_in_the_list_the_translations_are_checked_against() {
        // The last variant's index is the count: the list must be as long,
        // and hold no notice twice.
        assert_eq!(Notice::ALL.len(), Notice::RaftNeedsOpenWater as usize + 1);
        for (i, notice) in Notice::ALL.iter().enumerate() {
            assert_eq!(*notice as usize, i, "{notice:?} is out of place in `ALL`");
        }
    }
}
