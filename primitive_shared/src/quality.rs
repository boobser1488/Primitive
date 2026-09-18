//! How well a thing was made, and what that is worth afterwards.
//!
//! Every axe in this world used to be the same axe. Two players with the
//! same recipe and the same flint got the same tool, and so the only
//! question a workbench ever asked was *whether* you had the materials.
//! Quality is the second question: **you have the materials, and are you
//! in any state to use them?**
//!
//! A piece is judged once, at the moment it is made, out of the maker's
//! own condition -- how tired they are, how comfortable, how hurt, how
//! hungry -- together with the bench they stood at, the tool in their
//! hand, and, where there is one, the mini-game they just played
//! (`crate::minigame`). Then it is rolled. What comes out rides with the
//! item for the rest of its life and never changes again: a good axe does
//! not become a bad one, it becomes a worn good one.
//!
//! ## The decision this is meant to create
//!
//! "A mechanic should create a decision, not a chore" (CLAUDE.md). The
//! decision here is **now or after you have slept**. A player standing at
//! their anvil at the end of a long day, half-starved, with an iron bloom
//! they walked two days for, has a real choice to make, and both answers
//! are defensible: the axe you make tonight is worse than the axe you
//! make tomorrow, and tonight you have an axe.
//!
//! What it is deliberately **not** is a grind. There is no way to raise
//! quality by repetition, no skill number climbing in a corner of the
//! screen, and no recipe gated behind a quality floor. Every path to a
//! better piece is a thing a player *does that day*: eat, sleep, build a
//! room worth sleeping in, stand at the right bench, hold the right
//! hammer, and hit the marker.
//!
//! ## Why the roll narrows instead of swinging
//!
//! The obvious shape is `quality = state + random`, with the same random
//! either way. It was rejected, and the reason is what it does to the two
//! ends: a rested smith at a good anvil would still turn out rubbish one
//! time in five, which reads as the game taking the day's work away, and
//! a wreck would still turn out a masterpiece one time in five, which
//! makes sleeping pointless. So the spread itself is a function of the
//! maker ([`Maker::spread`]): **a steady hand is a narrow one.** A good
//! maker is reliable, which is what being good means; a bad one is
//! erratic, which is what being bad means -- and the one good piece off a
//! terrible day still happens, just rarely, and it is a story when it
//! does.
//!
//! The third option, considered and written down because it keeps coming
//! up: no roll at all, quality a pure function of state. That is a
//! spreadsheet. Two identical days would make two identical axes for ever,
//! and the player would learn the number and stop looking at it.
//!
//! ## Where it is kept
//!
//! In the **top eight bits of `inventory::Stack::damage`**, which is the
//! third thing that field carries and the third time the same argument has
//! been made (see the jug note in `inventory.rs` and the "going off" note
//! in `food.rs`). Both of those rejected a new field on `Stack`, and the
//! reason applies here with more force rather than less: bincode writes
//! fields by position, so a fourth field would have to be versioned into
//! the wire, both container save formats, the rack store, the fire store,
//! the carrion store and the profile store at once -- and an old file read
//! by a new build does not fail, it decodes into nonsense.
//!
//! What made this one look impossible is that the field's *first* meaning
//! is tool wear, and a tool is exactly the thing quality matters most to.
//! It fits anyway because the two numbers are different sizes: a tool's
//! whole life is a few hundred swings ([`WEAR_MASK`] holds sixteen
//! million) and quality is a byte. So wear keeps the low twenty-four bits
//! and quality takes the top eight, and nothing that reads wear may read
//! the raw word any more -- that is what [`Stack::wear`] is for, and every
//! site that used to say `stack.damage` and mean "swings taken" now says
//! `stack.wear()`.
//!
//! Three things fall out of that, all of them wanted:
//!
//! * **Quality survives a save, a chest and the wire for free**, because
//!   `damage` already does all three and is already versioned in all of
//!   them.
//! * **Two pieces of different quality do not stack**, because `add_worn`
//!   merges only stacks whose `damage` matches. That is the same rule food
//!   ages under, and it is the honest one: they *are* different things,
//!   and you will want to eat the poor one first.
//! * **A jug still says what is in it.** A jug's contents were already in
//!   this field, in the low sixteen bits and a count above them; the count
//!   never needed more than five bits for sixteen units, so it was moved
//!   into a byte of its own and the top byte was free there too. See
//!   `inventory::jug_contents`.

/// Where quality starts in `inventory::Stack::damage`.
pub const QUALITY_SHIFT: u32 = 24;

/// What is left of that word for everything else: a tool's wear, and a
/// jug's contents.
///
/// Sixteen million. The longest-lived tool in the game is a few hundred
/// swings, so the only way to reach this is arithmetic that has already
/// gone wrong somewhere else.
pub const WEAR_MASK: u32 = (1 << QUALITY_SHIFT) - 1;

/// How well one particular thing was made: 0 the worst a hand can do,
/// 255 the best.
///
/// A byte, and not an `f32`, because this is a number that has to live in
/// eight bits of a word that is already saved everywhere. Everything that
/// *uses* it asks for [`Quality::fraction`] and works in 0..1; the byte is
/// storage, not arithmetic.
///
/// **The default is the middle and not zero.** Zero means "the worst piece
/// anyone has ever made", and every stack in every world written before
/// this existed has a zero in those bits -- so if zero meant bad, the
/// first start-up after this change would turn every axe in every chest
/// into a ruin. [`Quality::PLAIN`] is what an unmarked item reads as, and
/// [`Quality::is_marked`] is how the interface knows not to draw a word
/// under something nobody judged.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, serde::Serialize, serde::Deserialize)]
pub struct Quality(u8);

impl Quality {
    /// What an item carries when nothing judged it: an old save, a drop
    /// off an animal, a stone picked up off the ground.
    ///
    /// **Zero, and read as the middle.** See the type's own note: the byte
    /// has to be zero for every item that already exists, so zero cannot
    /// be allowed to mean "bad". Anything a maker actually judged lands in
    /// 1..=255, which is why one whole value of the scale is spent on
    /// saying "nobody looked at this".
    pub const PLAIN: Quality = Quality(0);

    /// The best a piece can be.
    pub const FINEST: Quality = Quality(255);

    /// From the byte in a stack's word.
    pub fn from_byte(byte: u8) -> Quality {
        Quality(byte)
    }

    /// ...and back out.
    pub fn byte(self) -> u8 {
        self.0
    }

    /// From a judgement, 0..1. Clamped, and never zero: a piece somebody
    /// made badly is still a piece somebody made, and it must not read
    /// back as unmarked.
    pub fn from_fraction(value: f32) -> Quality {
        let scaled = (value.clamp(0.0, 1.0) * 254.0).round() as u8;
        Quality(scaled.saturating_add(1))
    }

    /// Where this sits on the scale, 0..1. An unmarked piece answers
    /// [`PLAIN_FRACTION`] -- the middle -- because that is what "nobody
    /// judged this" has to mean everywhere a number is wanted.
    pub fn fraction(self) -> f32 {
        if self.0 == 0 {
            PLAIN_FRACTION
        } else {
            (self.0 - 1) as f32 / 254.0
        }
    }

    /// Did anyone judge this piece? False for everything the world made
    /// and everything in a save older than this mechanic.
    pub fn is_marked(self) -> bool {
        self.0 != 0
    }

    /// Which of the four words the interface puts under it, or `None` for
    /// an unmarked piece.
    ///
    /// **Four bands and not a percentage**, for the reason `minigame`
    /// gives three: a number is a score and a word is a craftsman telling
    /// you how it went. The bands are uneven on purpose -- *fine* is the
    /// narrow one, because a masterpiece that happened half the time would
    /// not be one.
    pub fn band(self) -> Option<Band> {
        if !self.is_marked() {
            return None;
        }
        let f = self.fraction();
        Some(if f >= 0.88 {
            Band::Fine
        } else if f >= 0.62 {
            Band::Good
        } else if f >= 0.28 {
            Band::Plain
        } else {
            Band::Poor
        })
    }
}

/// What an unmarked piece is worth: the middle of the scale.
///
/// Every multiplier below is 1.0 at exactly this value, which is the whole
/// point of it -- an old axe out of an old chest behaves precisely as it
/// did before quality existed, and the mechanic is invisible until
/// somebody makes something.
pub const PLAIN_FRACTION: f32 = 0.5;

/// The word under the item. See [`Quality::band`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Band {
    /// Made badly. It works; it will not work for long.
    Poor,
    /// What a competent person makes on an ordinary day.
    Plain,
    /// Better than it had to be.
    Good,
    /// The one out of a long winter that gets a name.
    Fine,
}

impl Band {
    /// The English word. Translated on the way to the screen
    /// (`ui::lang`), like every other piece of prose in the game.
    pub fn note(self) -> &'static str {
        match self {
            Band::Poor => "poor",
            Band::Plain => "plain",
            Band::Good => "good",
            Band::Fine => "fine",
        }
    }
}

/// Is this a thing quality means anything about?
///
/// **Only what it changes**: something that wears out (a tool, a garment)
/// and something that is eaten. A brick, a plank, a nail and a length of
/// cord come out unmarked however well the day went.
///
/// This is not tidiness, it is the inventory. Two pieces of different
/// quality do not stack (see the module note, and `Inventory::add_worn`),
/// which is right for an axe and ruinous for a brick: a click for
/// sixty-four of something would put sixty-four one-item stacks in a
/// pack of forty squares, and the ones that did not fit would be
/// *dropped on the floor*. That is exactly what it did the first time,
/// and the test that caught it counted eighty-seven flint blades where a
/// hundred and thirty-five blows had landed.
///
/// So the rule is the honest one: a thing carries a judgement when the
/// judgement does something. Everything below reads off the same tables
/// the effects do, so a new food is judged on the day it exists and a new
/// ornament is not.
pub fn takes_quality(block: crate::types::BlockId) -> bool {
    crate::types::tool_durability(block).is_some() || crate::food::nutrition(block).is_some()
}

// ---- what the maker brings ----

/// The state a piece is judged out of.
///
/// **Plain numbers rather than a borrow of the server's player.** This
/// module is asked the same question by the crafting screen, by the anvil,
/// by the fire and by a mod, and three of those four do not have a
/// `PlayerState` to hand. Filling in five floats at the call site is also
/// the only way the tests below can state the property they are testing
/// without standing up a world.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Maker {
    /// 0 fresh .. 1 finished. `comfort::Condition::fatigue`.
    pub fatigue: f32,
    /// 0 wretched .. 1 at home. The player's own comfort level.
    pub comfort: f32,
    /// 0 dying .. 1 whole.
    pub health: f32,
    /// 0 fed .. 1 starving.
    pub hunger: f32,
    /// Standing at the bench, forge, anvil or wheel this job wants,
    /// rather than doing it in a field with your hands.
    pub at_station: bool,
    /// How good the tool in hand is for this job, 0..1. Zero for bare
    /// hands, and for a job that wants no tool -- see [`Maker::craft`] for
    /// why that is not the insult it looks like.
    pub tool: f32,
    /// What the station's mini-game scored, where the job had one.
    ///
    /// `None` is not "played badly": it is a job with no marker to hit,
    /// which is most of them.
    pub accuracy: Option<f32>,
}

impl Maker {
    /// **Everything right at once**: slept, fed, whole, at the bench this
    /// job wants, with the best tool for it.
    ///
    /// This is the ceiling and not a default, and it is worth saying so
    /// because the list is long: four things about the body, two about
    /// the workshop, and on a job with a mini-game a marker to hit as
    /// well. A player who has all of them has earned a fine piece, and a
    /// player who has none of them is the [`ORDINARY`](Maker::ORDINARY)
    /// case below on a bad day.
    pub const RESTED: Maker = Maker {
        fatigue: 0.0,
        comfort: 1.0,
        health: 1.0,
        hunger: 0.0,
        at_station: true,
        tool: 1.0,
        accuracy: None,
    };

    /// A workaday afternoon: a few hours' work behind you, a roof that is
    /// only half a home, a bit hungry, the middling tool.
    ///
    /// **What most pieces in most worlds are made by**, and the maker the
    /// bands below are aimed at: this one's rolls land across *plain*,
    /// *good* and occasionally *fine*, which is the spread the mechanic
    /// exists to produce. It is also what a mod gets if it asks for a
    /// piece without saying who made it.
    pub const ORDINARY: Maker = Maker {
        fatigue: 0.35,
        comfort: 0.4,
        health: 1.0,
        hunger: 0.25,
        at_station: true,
        tool: 0.7,
        accuracy: None,
    };

    /// How steady the hand is, out of the body alone: 0..1.
    ///
    /// The weights are an argument about what actually stops a person
    /// making a good thing, and they are in that order on purpose.
    /// **Tiredness first**, and by a long way, because it is the one of
    /// the four a player controls entirely and the one the game already
    /// asks them to manage. Comfort next: a cold wet hour in a lean-to is
    /// not where anything careful gets made. Health next. Hunger last and
    /// smallest -- not because it does not matter but because it already
    /// has teeth elsewhere (`food::STARVATION_DAMAGE`), and a mechanic
    /// that punished the same mistake twice would be punishing it twice.
    pub fn steadiness(&self) -> f32 {
        let fatigue = self.fatigue.clamp(0.0, 1.0);
        let comfort = self.comfort.clamp(0.0, 1.0);
        let health = self.health.clamp(0.0, 1.0);
        let hunger = self.hunger.clamp(0.0, 1.0);
        0.40 * (1.0 - fatigue) + 0.25 * comfort + 0.20 * health + 0.15 * (1.0 - hunger)
    }

    /// ...and out of the bench and the blade: 0..1.
    ///
    /// **Half and half**, because they fail differently and a player can
    /// only fix one of them at a time. Standing at the right station is a
    /// thing you did before you started; the tool is a thing you carried.
    ///
    /// A job that wants no tool passes `tool: 1.0` rather than zero, and
    /// the distinction is the caller's to make: `tool` means "how good is
    /// what you are working with", and a potter's hands are the right
    /// hands. Zero is for the player who is knapping flint with a rock
    /// because they left the hammer at home.
    pub fn craft(&self) -> f32 {
        let station = if self.at_station { 1.0 } else { 0.0 };
        0.5 * station + 0.5 * self.tool.clamp(0.0, 1.0)
    }

    /// Where this piece is aimed, before the roll: 0..1.
    ///
    /// **The mini-game is half of it and never more.** Weighting it
    /// higher would mean a sleepless starving smith and a rested one make
    /// the same nails as long as both hit the marker, which empties the
    /// rest of this module; weighting it lower would make playing it
    /// theatre. Half is the number at which both halves of the sentence
    /// "a good smith, on a good day" mean something.
    pub fn aim(&self) -> f32 {
        let hand = 0.55 * self.steadiness() + 0.45 * self.craft();
        match self.accuracy {
            Some(accuracy) => 0.5 * hand + 0.5 * accuracy.clamp(0.0, 1.0),
            None => hand,
        }
    }

    /// How far either side of the aim the roll may land.
    ///
    /// See the module's "why the roll narrows" note. The floor is a tenth
    /// -- there is always a little in it, or the number would be a
    /// spreadsheet -- and it opens to four tenths for a maker in a bad
    /// way. Only [`steadiness`](Maker::steadiness) widens it: a blunt
    /// hammer makes a *worse* piece, reliably, while exhaustion is what
    /// makes a hand unpredictable.
    pub fn spread(&self) -> f32 {
        0.10 + 0.30 * (1.0 - self.steadiness())
    }

    /// Judge a piece. `roll` is a fresh 0..1 from the server's own
    /// generator -- never the client's, for the reason everything else in
    /// this game is decided on the server.
    pub fn judge(&self, roll: f32) -> Quality {
        let offset = (roll.clamp(0.0, 1.0) * 2.0 - 1.0) * self.spread();
        Quality::from_fraction(self.aim() + offset)
    }
}

// ---- what quality is worth ----

/// How much longer a tool made well lasts.
///
/// 0.6 at the worst to 1.7 at the best, so a fine axe is not quite three
/// times a poor one. **This is the biggest of the four multipliers**, and
/// deliberately: durability is the one quality effect a player can *see*
/// happening without being told a number, because they watch the bar go
/// down at the speed it goes down.
///
/// A poor tool is still worth making. Six tenths of an axe is an axe, and
/// the alternative to an axe is a rock.
pub fn durability_scale(quality: Quality) -> f32 {
    0.55 + 0.9 * quality.fraction()
}

/// ...and how much faster it works.
///
/// 0.9 to 1.12, and the narrowness is the point. Speed is felt on every
/// single swing, so a wide multiplier here would make a poor tool feel
/// broken rather than poor -- and a player who cannot tell "bad tool" from
/// "bug" files the second one. Durability is where the difference is meant
/// to live; this is a nudge that confirms it.
pub fn speed_scale(quality: Quality) -> f32 {
    0.9 + 0.2 * quality.fraction()
}

/// How much of a blow a well-made garment turns.
///
/// 0.75 to 1.25 of what the piece is worth. Narrower than durability
/// because armour's job is to make a fight survivable, and a range wide
/// enough to be interesting at the top is wide enough at the bottom to
/// make a poor jerkin no jerkin at all -- which is the one outcome that
/// would teach players to throw pieces away instead of wearing them.
pub fn protection_scale(quality: Quality) -> f32 {
    0.75 + 0.5 * quality.fraction()
}

/// How much of a meal a well-made one is.
///
/// 0.85 to 1.2. Small, because food is eaten in quantity and a large
/// multiplier here would turn cooking into arithmetic; the real reward for
/// cooking something well is the next one.
pub fn nutrition_scale(quality: Quality) -> f32 {
    0.85 + 0.3 * quality.fraction()
}

/// ...and how likely one step of the world's clock is to pass a
/// well-made piece of food by.
///
/// Zero at the bottom and a little over a third at the top, which at
/// `food::ROT_STEPS_PER_DAY` is the difference between meat that lasts
/// two days and meat that lasts three. **A chance rather than a divisor**,
/// because the ages themselves are three bits of a block id
/// (`food.rs`, "going off") and a rate that varied per stack would need a
/// per-stack clock, which is the field that note already refused. A coin
/// flipped at the moment the world ages things needs nothing stored at
/// all.
pub fn keeping_chance(quality: Quality) -> f32 {
    0.35 * (quality.fraction() - PLAIN_FRACTION).max(0.0) / (1.0 - PLAIN_FRACTION)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The same roll for everybody, so what moves is the maker.
    const MIDDLING_ROLL: f32 = 0.5;

    fn wrecked() -> Maker {
        Maker {
            fatigue: 1.0,
            comfort: 0.0,
            health: 0.3,
            hunger: 0.9,
            at_station: false,
            tool: 0.0,
            accuracy: None,
        }
    }

    #[test]
    fn an_unmarked_item_behaves_exactly_as_it_did_before_quality_existed() {
        let plain = Quality::PLAIN;
        assert!(!plain.is_marked(), "an old axe reads as judged");
        assert_eq!(plain.band(), None, "an old axe gets a word under it");
        for scale in [
            durability_scale(plain),
            speed_scale(plain),
            protection_scale(plain),
            nutrition_scale(plain),
        ] {
            assert!(
                (scale - 1.0).abs() < 1e-6,
                "an unjudged item is worth {scale} of itself"
            );
        }
        assert_eq!(keeping_chance(plain), 0.0);
    }

    #[test]
    fn a_rested_maker_at_a_bench_beats_a_wreck_in_a_field_on_the_same_roll() {
        let good = Maker::RESTED.judge(MIDDLING_ROLL);
        let bad = wrecked().judge(MIDDLING_ROLL);
        assert!(
            good > bad,
            "the wreck made a {:?} and the rested smith a {:?}",
            bad.band(),
            good.band()
        );
        assert_eq!(good.band(), Some(Band::Fine));
        assert_eq!(bad.band(), Some(Band::Poor));
    }

    #[test]
    fn every_part_of_the_makers_state_moves_the_piece_on_its_own() {
        let base = Maker::ORDINARY;
        let worse = [
            Maker { fatigue: 1.0, ..base },
            Maker { comfort: 0.0, ..base },
            Maker { health: 0.2, ..base },
            Maker { hunger: 1.0, ..base },
            Maker { at_station: false, ..base },
            Maker { tool: 0.0, ..base },
        ];
        for (index, maker) in worse.iter().enumerate() {
            assert!(
                maker.aim() < base.aim(),
                "input {index} changed for the worse and the piece did not"
            );
        }
    }

    #[test]
    fn the_roll_narrows_for_a_steady_hand_and_opens_for_a_shaking_one() {
        let steady = Maker::RESTED;
        let shaky = wrecked();
        assert!(
            steady.spread() < shaky.spread() / 2.0,
            "a rested maker is as unpredictable as an exhausted one"
        );
        // ...and the point of that: the good maker's worst day still beats
        // the bad maker's best.
        let steady_worst = steady.judge(0.0);
        let shaky_best = shaky.judge(1.0);
        assert!(
            steady_worst > shaky_best,
            "a wreck's luckiest piece ({:?}) beat a rested smith's unluckiest ({:?})",
            shaky_best.band(),
            steady_worst.band()
        );
    }

    #[test]
    fn the_roll_still_decides_something() {
        let maker = Maker::RESTED;
        assert!(
            maker.judge(1.0) > maker.judge(0.0),
            "the same maker makes the same piece every time"
        );
    }

    #[test]
    fn hitting_the_marker_is_half_the_piece_and_never_all_of_it() {
        let struck_true = Maker {
            accuracy: Some(1.0),
            ..wrecked()
        };
        let fumbled = Maker {
            accuracy: Some(0.0),
            ..Maker::RESTED
        };
        assert!(
            struck_true.aim() > wrecked().aim(),
            "playing the mini-game well was worth nothing"
        );
        assert!(
            fumbled.aim() < Maker::RESTED.aim(),
            "playing it badly cost nothing"
        );
        // Neither end swamps the other: a wreck who plays perfectly still
        // does not out-make a rested smith who fumbles badly, because the
        // state is the other half.
        assert!(
            struck_true.aim() < Maker::RESTED.aim(),
            "the mini-game replaced the maker's state instead of joining it"
        );
    }

    #[test]
    fn a_better_tool_lasts_longer_and_a_poor_one_is_still_worth_carrying() {
        let fine = Quality::from_fraction(1.0);
        let poor = Quality::from_fraction(0.0);
        assert!(
            durability_scale(fine) > durability_scale(poor) * 2.0,
            "a fine tool is not worth making"
        );
        assert!(
            durability_scale(poor) > 0.5,
            "a poor tool is not worth making"
        );
        assert!(
            speed_scale(fine) / speed_scale(poor) < 1.3,
            "speed is doing durability's job"
        );
    }

    #[test]
    fn the_bands_are_reachable_and_fine_is_the_rare_one() {
        let mut seen = [0usize; 4];
        // A steady maker rolling across the whole range: the spread of
        // bands a player actually meets.
        for step in 0..=100 {
            let roll = step as f32 / 100.0;
            let band = Maker::ORDINARY.judge(roll).band().expect("a judged piece");
            seen[band as usize] += 1;
        }
        assert!(seen[Band::Plain as usize] > 0, "plain is unreachable");
        assert!(seen[Band::Good as usize] > 0, "good is unreachable");
        assert!(seen[Band::Fine as usize] > 0, "fine is unreachable");
        assert!(
            seen[Band::Fine as usize] < seen[Band::Good as usize],
            "fine is as common as good"
        );
    }

    #[test]
    fn a_judgement_is_only_put_on_what_it_changes() {
        use crate::types::{
            BLOCK_BRICK, BLOCK_COOKED_MEAT, BLOCK_PLANKS, BLOCK_LEATHER_TUNIC, BLOCK_STONE_PICKAXE,
        };
        for block in [BLOCK_LEATHER_TUNIC, BLOCK_STONE_PICKAXE, BLOCK_COOKED_MEAT] {
            assert!(
                takes_quality(block),
                "{} is not judged and quality changes what it does",
                crate::types::block_name(block)
            );
        }
        // ...and the things a judgement would do nothing to but fragment
        // the pack. See the function's own note.
        for block in [BLOCK_BRICK, BLOCK_PLANKS] {
            assert!(
                !takes_quality(block),
                "{} is judged and nothing reads the judgement",
                crate::types::block_name(block)
            );
        }
    }

    #[test]
    fn quality_and_wear_fit_in_one_word_without_touching_each_other() {
        // The longest life any tool has, several times over.
        for wear in [0u32, 1, 500, 100_000] {
            for byte in [0u8, 1, 128, 255] {
                let word = wear | (u32::from(byte) << QUALITY_SHIFT);
                assert_eq!(word & WEAR_MASK, wear);
                assert_eq!((word >> QUALITY_SHIFT) as u8, byte);
            }
        }
    }
}
