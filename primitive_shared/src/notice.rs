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
    // ---- the shore and the grove ----
    /// **A monkey has taken what was in your hand.** Appended, like every
    /// notice before it, because the index is on the wire.
    MonkeyTakesIt,
    // ---- the last six English sentences the server was sending ----
    //
    // These were `ServerMessage::Error(String)` with the words written into
    // the server in English, which is a Russian player being answered in a
    // language they did not choose. Four of them the client could already
    // say in four languages -- it refuses the same gestures itself, before
    // asking (`logic::fishing::Notice`) -- and it only ever saw the English
    // when its guess and the server's disagreed, which is exactly the
    // moment nobody is looking for a translation bug.
    /// A snare that is set and has caught nothing.
    SnareEmpty,
    /// A salt pan of sea water that has not finished drying.
    PanDrying,
    /// An empty salt pan, and no jug of water in the hand.
    PanWantsSea,
    /// An empty salt pan, and a jug of fresh water in the hand.
    PanFreshWater,
    /// **Said once, at the swallow.** Drinking from the sea.
    SeaWaterIsSalt,
    /// The same, for standing water.
    StaleWater,
    // ---- home ----
    /// **Woken rested**: the night was slept in a bed in a shut room by a
    /// lit fire (`comfort::rests_at_home`), and the half day ahead costs less
    /// food. Said at the waking, because a buff nobody is told about is a
    /// buff nobody goes home for.
    SleptAtHome,
    // ---- the rest of the English the server was sending ----
    //
    // Every `ServerMessage::Error` that was still an English sentence, and
    // the pit's and the charcoal pile's words, which went out as chat lines
    // in English too. The ones that carry a number are sent as
    // `ServerMessage::Said` with the numbers beside the code (`Said`), and
    // the client's row has `{0}`, `{1}` where they go.
    /// The anticheat refused an edit. The reason stays in the server's log:
    /// it is for an operator, not a player.
    EditPutBack,
    PluginRefused,
    NotInsideYourself,
    /// A block or a course laid where another player stands. Their name is
    /// not said: a refusal is about the gesture.
    NotInsideSomebody,
    NeedBetterTool,
    NeedsSolidGround,
    LeanToNeedsRoom,
    BedNeedsRoom,
    RackNeedsRoom,
    DoorNeedsRoom,
    TorchNeedsRoom,
    DoesNotGoThere,
    NotCarryingThat,
    /// A block edit the world could not write: past the top or the bottom of it.
    PastTheEdge,
    CannotMakeThat,
    NotCarryingEnough,
    WallWantsMortar,
    WallFinished,
    LiftStillWet,
    HeadArmourFell,
    ChestArmourFell,
    LegsArmourFell,
    FeetArmourFell,
    BackArmourFell,
    FlintShattered,
    /// `{0}` is how many.
    FlintsShattered,
    TrunkScored,
    BarrelHoldsWater,
    BarrelHoldsGrain,
    BarrelHoldsOtherGrain,
    BarrelWantsFullJug,
    OnlyGrainInBarrel,
    /// Said at the swallow, like `StaleWater`.
    WrongCap,
    MeatTurned,
    RawFlesh,
    /// A dressing offered to a part with nothing on it that the dressing
    /// treats.
    NothingItWouldHelp,
    EdgeAlreadySharp,
    ShortOfMaterials,
    NoRoomForIt,
    StruckEarly,
    OutOfTurn,
    Rushed,
    RunNotYet,
    CastShort,
    TooShallowToFish,
    NoFishHere,
    /// `{0}` sticks and `{1}` log make a firepit; `{2}` sticks and `{3}` logs
    /// are lying here.
    FirepitWants,
    NoRaftThere,
    SomebodyAtOars,
    PitNeedsFloor,
    PitOpenAtSide,
    PitSmothered,
    TakePotteryOut,
    PitHoldsFour,
    /// `{0}` fibre, then `{1}` logs.
    PitFibreFirst,
    /// `{0}` logs.
    PitFibrePacked,
    /// `{0}` needed, `{1}` there.
    PitFibreBeforeLogs,
    /// `{0}` fibre and `{1}` logs; `{2}` fibre there.
    PitLitWithFibreAndLogs,
    PitFull,
    /// `{0}` needed, `{1}` there.
    PitLitWithLogs,
    /// `{0}` of `{1}`.
    PitLogs,
    /// `{0}` minutes.
    PitBurning,
    RainOnPit,
    NothingToFire,
    PitAlight,
    PileNeedsFloor,
    PileOpen,
    /// `{0}` minutes.
    CharcoalBurning,
    /// `{0}` logs.
    PileHolds,
    /// `{0}` seconds.
    PileAlight,
    // ---- the field, turned (`lib.rs`, `field_note`) ----
    RichSoilWatered,
    RichSoilDry,
    ThinSoilWatered,
    ThinSoilDry,
    SoilWatered,
    SoilDry,
    // ---- the night ----
    /// **Woken by wolves.** The night was asked whether it found a sleeper
    /// out in the open with no fire (`animals::found_asleep_odds`), and it
    /// did: the clock stops where they came, the sleeper is on their feet,
    /// and the pack is at the edge of a lunge. Said at once, because it is
    /// the only warning there is.
    WokenByWolves,
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
        Notice::MonkeyTakesIt,
        Notice::SnareEmpty,
        Notice::PanDrying,
        Notice::PanWantsSea,
        Notice::PanFreshWater,
        Notice::SeaWaterIsSalt,
        Notice::StaleWater,
        Notice::SleptAtHome,
        Notice::EditPutBack,
        Notice::PluginRefused,
        Notice::NotInsideYourself,
        Notice::NotInsideSomebody,
        Notice::NeedBetterTool,
        Notice::NeedsSolidGround,
        Notice::LeanToNeedsRoom,
        Notice::BedNeedsRoom,
        Notice::RackNeedsRoom,
        Notice::DoorNeedsRoom,
        Notice::TorchNeedsRoom,
        Notice::DoesNotGoThere,
        Notice::NotCarryingThat,
        Notice::PastTheEdge,
        Notice::CannotMakeThat,
        Notice::NotCarryingEnough,
        Notice::WallWantsMortar,
        Notice::WallFinished,
        Notice::LiftStillWet,
        Notice::HeadArmourFell,
        Notice::ChestArmourFell,
        Notice::LegsArmourFell,
        Notice::FeetArmourFell,
        Notice::BackArmourFell,
        Notice::FlintShattered,
        Notice::FlintsShattered,
        Notice::TrunkScored,
        Notice::BarrelHoldsWater,
        Notice::BarrelHoldsGrain,
        Notice::BarrelHoldsOtherGrain,
        Notice::BarrelWantsFullJug,
        Notice::OnlyGrainInBarrel,
        Notice::WrongCap,
        Notice::MeatTurned,
        Notice::RawFlesh,
        Notice::NothingItWouldHelp,
        Notice::EdgeAlreadySharp,
        Notice::ShortOfMaterials,
        Notice::NoRoomForIt,
        Notice::StruckEarly,
        Notice::OutOfTurn,
        Notice::Rushed,
        Notice::RunNotYet,
        Notice::CastShort,
        Notice::TooShallowToFish,
        Notice::NoFishHere,
        Notice::FirepitWants,
        Notice::NoRaftThere,
        Notice::SomebodyAtOars,
        Notice::PitNeedsFloor,
        Notice::PitOpenAtSide,
        Notice::PitSmothered,
        Notice::TakePotteryOut,
        Notice::PitHoldsFour,
        Notice::PitFibreFirst,
        Notice::PitFibrePacked,
        Notice::PitFibreBeforeLogs,
        Notice::PitLitWithFibreAndLogs,
        Notice::PitFull,
        Notice::PitLitWithLogs,
        Notice::PitLogs,
        Notice::PitBurning,
        Notice::RainOnPit,
        Notice::NothingToFire,
        Notice::PitAlight,
        Notice::PileNeedsFloor,
        Notice::PileOpen,
        Notice::CharcoalBurning,
        Notice::PileHolds,
        Notice::PileAlight,
        Notice::RichSoilWatered,
        Notice::RichSoilDry,
        Notice::ThinSoilWatered,
        Notice::ThinSoilDry,
        Notice::SoilWatered,
        Notice::SoilDry,
        Notice::WokenByWolves,
    ];

    /// Whether this is news rather than a refusal: said the same way, but
    /// it does not end a run at a station the way a refusal does (the
    /// client's `ServerMessage::Notice` arm).
    pub fn is_news(self) -> bool {
        matches!(
            self,
            Notice::HorseIsYours
                | Notice::HorseTakesFood
                | Notice::AshDugIn
                | Notice::HorseThrowsYou
                // News, and the worst news the shore has: nothing was
                // refused, something was taken.
                | Notice::MonkeyTakesIt
                // Nothing was refused: the water went down. What these say
                // is what it will cost, said at the swallow rather than an
                // hour later when the sickness starts.
                | Notice::SeaWaterIsSalt
                | Notice::StaleWater
                | Notice::SleptAtHome
                // Nothing refused: something broke, went down wrong, or a
                // pit moved on a stage -- the player did the thing.
                | Notice::HeadArmourFell
                | Notice::ChestArmourFell
                | Notice::LegsArmourFell
                | Notice::FeetArmourFell
                | Notice::BackArmourFell
                | Notice::FlintShattered
                | Notice::FlintsShattered
                | Notice::WrongCap
                | Notice::MeatTurned
                | Notice::RawFlesh
                | Notice::PitFibreFirst
                | Notice::PitFibrePacked
                | Notice::PitFull
                | Notice::PitLogs
                | Notice::PitBurning
                | Notice::PitAlight
                | Notice::PileOpen
                | Notice::CharcoalBurning
                | Notice::PileAlight
                | Notice::RichSoilWatered
                | Notice::RichSoilDry
                | Notice::ThinSoilWatered
                | Notice::ThinSoilDry
                | Notice::SoilWatered
                | Notice::SoilDry
                // Nothing refused: something came.
                | Notice::WokenByWolves
        )
    }
}

/// A notice with the numbers it says: "3 of 6 logs on the pit kiln".
///
/// **The numbers beside the code, never in it.** A variant carrying its
/// count would not be a unit variant, and `ALL` -- what the client's rows are
/// checked against, by index -- only works for unit ones; and a count folded
/// into the words on the server is English again. So the code names the
/// sentence, the client's row has `{0}`, `{1}` where the numbers go, and
/// these fill them in order (`ui::lang::said`). A row asking for a number
/// that is not here prints the placeholder, which a test would see.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Said {
    pub what: Notice,
    pub numbers: Vec<u32>,
}

impl Said {
    pub fn new(what: Notice, numbers: &[u32]) -> Said {
        Said { what, numbers: numbers.to_vec() }
    }
}

impl From<Notice> for Said {
    fn from(what: Notice) -> Said {
        Said { what, numbers: Vec::new() }
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
        // and hold no notice twice. **Name the last one here when you
        // append one** -- that is the whole check, and a notice appended
        // to the enum and forgotten in `ALL` is a notice the client is
        // never asked to have words for.
        assert_eq!(Notice::ALL.len(), Notice::WokenByWolves as usize + 1);
        for (i, notice) in Notice::ALL.iter().enumerate() {
            assert_eq!(*notice as usize, i, "{notice:?} is out of place in `ALL`");
        }
    }
}
