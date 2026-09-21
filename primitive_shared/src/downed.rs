//! Downed: the minute between a body giving out and a body being dead.
//!
//! ## Why a player does not simply die any more
//!
//! Health reaching zero was the end, and it was the end the same way
//! whatever took it: a wolf, a ledge, an empty stomach and a frost all
//! put up the same death screen on the same frame. The last point of
//! health was therefore the one point nobody could do anything with --
//! and it is exactly the moment a player most wants to do something.
//!
//! So a body that gives out now goes *down* first. It lies on the ground
//! and crawls, slowly, for a while; the cause decides how slowly, for how
//! long, and what gets it up again. At the end of that time it dies the
//! way it always did -- the pack goes into a corpse, the death screen goes
//! up, nothing about dying itself has changed. What has changed is that
//! the last point of health asks a question: **crawl to the fire, or eat
//! what is in the pack? Bind the bite here, or drag the leg home to the
//! splint?** If a cause had one correct answer, it would not be worth a
//! crawl, and it does not get one (see the drowned and the crushed below).
//!
//! ## The table
//!
//! Seconds on the ground, the crawl as a fraction of a walk, and what
//! raises the body. Every row is argued, because a number here with no
//! reason is a number somebody retunes into a chore.
//!
//! | cause | seconds | crawl | what raises you |
//! |---|---|---|---|
//! | a blow (animal, stakes, another player, bees) | 30 | 0.30 | a dressing |
//! | bleeding out | 20 | 0.30 | a dressing |
//! | a fall (broken legs) | 90 | 0.10 | a splint |
//! | burning (fire, lightning) | 15 | 0.35 | a dressing |
//! | the cold | 60 | 0.20 | warmth |
//! | the heat | 40 | 0.25 | cooling down |
//! | hunger | 60 | 0.25 | anything eaten |
//! | thirst | 40 | 0.25 | anything drunk |
//! | a bad stomach (bad water, bad food) | 40 | 0.25 | anything drunk |
//! | smoke | 20 | 0.35 | clean air |
//! | drowning | -- | -- | nothing: dead at once |
//! | crushed (a tree, a roof, a rockfall) | -- | -- | nothing: dead at once |
//!
//! * **A blow is short**, because a fight is short: thirty seconds is long
//!   enough to get a bandage out of the pack and not long enough to walk
//!   home for one. **Bleeding is shorter still**: a body that bled down to
//!   nothing has been losing blood for minutes already and has less left.
//!   A bandage *or* a poultice is a dressing; willow bark is not, because
//!   it stops no blood.
//! * **A fall is long and nearly still.** Both legs are gone; what is left
//!   is arms, and a tenth of a walk. Ninety seconds is a real distance at
//!   that pace -- about thirty blocks -- which is the decision: the splint
//!   in the pack now, or the camp that is just about in reach.
//! * **Fire is the shortest** of the survivable ones and the crawl is the
//!   quickest, because the answer to burning is first of all *getting out
//!   of it*: a body still in the flames goes on being billed for them (see
//!   `shares_the_bill`), and fifteen seconds is how long there is.
//! * **Cold and heat raise a body by themselves** once it is back inside
//!   the band the exposure stops hurting at, with a margin so the edge is
//!   not a flicker. What saves a freezing player is the fire; what the
//!   player decides is whether the fire is near enough to crawl to or
//!   whether there is time to light one where they lie.
//! * **Hunger and thirst** are raised by the mouthful, because the pack is
//!   exactly where the answer is -- the decision is whether it holds one.
//!   **A bad stomach is raised by water**, which is what a body emptied by
//!   sickness is short of. It is not raised by food: the stomach is what
//!   is failing.
//! * **Smoke** raises the body the moment the lungs are full again, which
//!   is the moment it has crawled out of the room. The crawl is the answer
//!   and it has to be quick.
//! * **Drowning has no downed state.** A body that goes down under water is
//!   still under water; lying face down in a lake for a minute is not a
//!   crawl, it is the same death with a delay. Downed only on the shore was
//!   considered and left out: the server would have to decide what "the
//!   shore" is for a body at the bottom of a pond, and a rule that only
//!   holds in the shallows is a rule a player learns by dying in the deep.
//! * **Crushed has none either.** A tree or a roof across the body is not
//!   something anybody crawls out from under.
//!
//! ## Overkill
//!
//! A blow that takes a body **a whole bar of health past zero**
//! ([`OVERKILL`]) kills outright, whatever its cause: a forty-block fall is
//! not a broken leg, and a bear that hits for forty is not a bite. This is
//! also what keeps the existing ways of dying certain where they meant to
//! be -- the tree and the command that hand in `f32::MAX` still kill.
//!
//! ## What happens to the body while it is down
//!
//! The health is nought and stays there; the timer is what is left. Damage
//! still arrives, and what it does depends on whether it is the same thing
//! that put the body down ([`Cause::shares_the_bill`]):
//!
//! * **The condition that downed you is already the timer.** Hunger does
//!   not take the timer twice for a body downed by hunger; the cold does
//!   not for a body downed by the cold; a bite that is bleeding does not
//!   for a body downed by the bite.
//! * **Anything else takes the timer**, at [`SECONDS_PER_HEALTH`] a point:
//!   a wolf still at a downed player finishes them, a crawl off a ledge
//!   costs what the landing would have, a body still lying in the fire
//!   burns through fifteen seconds in two.
//!
//! ## What is shared and what is the server's
//!
//! This module is the rules and nothing else: which cause, how long, how
//! slow, what saves. The server owns the timer (`survival::Vitals`) and
//! tells the client a [`Down`] when it changes; the client counts it down
//! for the screen, draws the crawl, and *predicts* the pace. The pace is
//! held loosely by the server: the anticheat judges a downed body against
//! a crawl's budget (`AntiCheat::set_crawling`, the fastest crawl in this
//! table) rather than per cause, the same as it has no per-player speed
//! for a broken leg. The timer and the rescue are the server's.

use serde::{Deserialize, Serialize};

use crate::injury::Treatment;

/// Damage past zero that kills without a crawl: a whole bar.
///
/// The server's `survival::MAX_HEALTH`, written out because this crate does
/// not know the server's health scale -- and a test on the server says the
/// two are the same number.
pub const OVERKILL: f32 = 20.0;

/// How many seconds a point of damage takes off a downed body's time.
///
/// Two: a wolf's bite of five or six is ten or twelve seconds, so a wolf
/// left at a player downed by it finishes them in two or three more bites
/// -- slower than it took them down, fast enough that nobody mistakes being
/// downed for being safe.
pub const SECONDS_PER_HEALTH: f32 = 2.0;

/// What a body comes back up with.
///
/// Three, not one: one point is a body that goes straight back down at the
/// next crumb of hunger or the next drop of blood, which would be a rescue
/// that rescued nobody. Not more, because being raised is getting up off
/// the ground, not being healed.
pub const RAISED_HEALTH: f32 = 3.0;

/// How far past the harmful line a body has to warm (or cool) before the
/// cold (or heat) lets it up, in the body's degrees.
///
/// One degree: without a margin a body on the line would be raised and put
/// down again on alternate ticks by a fire that flickered.
pub const TEMPERATURE_MARGIN_C: f32 = 1.0;

/// How high the eye of a crawling body is over its feet, in blocks.
///
/// A little under half a block: flat on the ground, head raised to see
/// where it is going. The collider does not shrink -- a body that could
/// crawl under a table it could not stand under would be a body the server
/// then had to fit back into a room it had not got up in.
pub const CRAWL_EYE: f32 = 0.45;

/// What put a body on the ground. See the module note for the table.
///
/// **On the wire** (`Down::cause`) rather than the server's English words,
/// because the client has to say *what saves you* in the player's language,
/// and a sentence it could only print as the server wrote it would be a
/// sentence in English on a Russian screen. The order is the wire's:
/// append.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum Cause {
    /// A blow: an animal, sharpened stakes, another player, stings. The
    /// catch-all for any words this module does not recognise, because
    /// nearly everything that names a cause of its own (a mod's blow, a new
    /// animal) is a blow.
    Wound,
    Bleeding,
    Fall,
    Burn,
    Cold,
    Heat,
    Hunger,
    Thirst,
    Sickness,
    Smoke,
    Drowned,
    Crushed,
}

/// What gets a downed body up.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum Rescue {
    /// A bandage or a poultice.
    Dressing,
    Splint,
    /// Back above the freezing line.
    Warmth,
    /// Back below the scalding line.
    Cooling,
    /// Anything eaten.
    Food,
    /// Anything drunk that was not the sea.
    Water,
    /// A full breath.
    Air,
}

/// One row of the table.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Profile {
    pub seconds: f32,
    /// A fraction of a walk.
    pub crawl: f32,
    pub rescue: Rescue,
}

impl Cause {
    pub const ALL: [Cause; 12] = [
        Cause::Wound,
        Cause::Bleeding,
        Cause::Fall,
        Cause::Burn,
        Cause::Cold,
        Cause::Heat,
        Cause::Hunger,
        Cause::Thirst,
        Cause::Sickness,
        Cause::Smoke,
        Cause::Drowned,
        Cause::Crushed,
    ];

    /// Which cause the server's words are.
    ///
    /// **Read off the words the death screen already prints**, rather than a
    /// cause threaded as a second argument through every one of the twenty
    /// places health is taken. A second argument would be twenty chances to
    /// pass the wrong one, and the words are already the one thing every
    /// path is required to say. A test walks every cause the server writes
    /// (`survival`) and says which row it lands on.
    pub fn of(words: &str) -> Cause {
        let words = words.trim();
        match words {
            "drowned" => Cause::Drowned,
            "froze to death" => Cause::Cold,
            "died of heatstroke" => Cause::Heat,
            "starved" => Cause::Hunger,
            "died of thirst" => Cause::Thirst,
            "bled to death" => Cause::Bleeding,
            "burned to death" | "was struck by lightning" => Cause::Burn,
            "choked on smoke" => Cause::Smoke,
            "drank bad water" | "ate something they should not have" => Cause::Sickness,
            "fell from a great height" | "fell onto a stalagmite" | "was thrown by a horse" => Cause::Fall,
            _ if words.starts_with("was crushed") || words.starts_with("was buried") => Cause::Crushed,
            _ => Cause::Wound,
        }
    }

    /// The row, or `None` for a cause that kills without a crawl.
    pub fn profile(self) -> Option<Profile> {
        let row = |seconds, crawl, rescue| Some(Profile { seconds, crawl, rescue });
        match self {
            Cause::Wound => row(30.0, 0.30, Rescue::Dressing),
            Cause::Bleeding => row(20.0, 0.30, Rescue::Dressing),
            Cause::Fall => row(90.0, 0.10, Rescue::Splint),
            Cause::Burn => row(15.0, 0.35, Rescue::Dressing),
            Cause::Cold => row(60.0, 0.20, Rescue::Warmth),
            Cause::Heat => row(40.0, 0.25, Rescue::Cooling),
            Cause::Hunger => row(60.0, 0.25, Rescue::Food),
            Cause::Thirst => row(40.0, 0.25, Rescue::Water),
            Cause::Sickness => row(40.0, 0.25, Rescue::Water),
            Cause::Smoke => row(20.0, 0.35, Rescue::Air),
            Cause::Drowned | Cause::Crushed => None,
        }
    }

    /// A condition, rather than an event: something a body is *in* rather
    /// than something that happened to it.
    fn is_condition(self) -> bool {
        matches!(
            self,
            Cause::Bleeding | Cause::Cold | Cause::Heat | Cause::Hunger | Cause::Thirst | Cause::Sickness | Cause::Smoke
        )
    }

    /// Whether damage of `self`'s kind, arriving while a body lies downed by
    /// `downed_by`, is already what the timer is counting -- and so costs
    /// nothing more.
    ///
    /// **Only a condition, and only its own.** The cold that downed a body
    /// goes on being cold, and billing both the timer and the tick for it
    /// would be the same minute charged twice. An *event* is never the
    /// timer: a second bite, another fall, another second in the fire are
    /// all new, and a body still lying in the flames that put it down must
    /// burn faster than one that crawled out -- which is the whole reason
    /// there is a crawl. The one pairing across two causes is a bite that
    /// is bleeding: the blood is the bite's, and the thirty seconds a blow
    /// gives are the thirty seconds it bleeds for.
    pub fn shares_the_bill(self, downed_by: Cause) -> bool {
        self.is_condition() && (self == downed_by || (self == Cause::Bleeding && downed_by == Cause::Wound))
    }
}

impl Rescue {
    /// Whether putting `treatment` on a downed body is this rescue.
    pub fn by_treatment(self, treatment: Treatment) -> bool {
        match self {
            Rescue::Dressing => matches!(treatment, Treatment::Bandage | Treatment::Poultice),
            Rescue::Splint => treatment == Treatment::Splint,
            _ => false,
        }
    }

    /// Whether the body is already in the state that raises it, for the
    /// rescues that are a state rather than an act. `breath_full` is whether
    /// the lungs are full.
    pub fn met_by(self, body_c: f32, breath_full: bool) -> bool {
        match self {
            Rescue::Warmth => body_c >= crate::body::FREEZING + TEMPERATURE_MARGIN_C,
            Rescue::Cooling => body_c <= crate::body::SCALDING - TEMPERATURE_MARGIN_C,
            Rescue::Air => breath_full,
            Rescue::Dressing | Rescue::Splint | Rescue::Food | Rescue::Water => false,
        }
    }

    /// Whether another player can do it for you, with what is in their hand.
    ///
    /// **An act can be done by a second pair of hands; a state cannot.** A
    /// friend can bind a bite, set a leg, put food or a jug to a mouth. They
    /// cannot carry a freezing body to the fire -- nothing in this game
    /// carries a player -- but they can build the fire beside it, which is
    /// the same rescue arrived at by the world rather than by a hand.
    pub fn can_be_helped(self) -> bool {
        matches!(self, Rescue::Dressing | Rescue::Splint | Rescue::Food | Rescue::Water)
    }
}

/// Where a dressing goes on a body that is not aiming it: the first part
/// with an open, undressed wound it suits, and the torso if there is none.
///
/// **The client's choice, the server's rule.** A player on the ground puts a
/// bandage on with the use key, not by dropping it on the mannequin -- there
/// is no time to open the pack and find the arm -- so something has to name
/// a part for `ClientMessage::TreatInjury`. The server still decides what the
/// dressing does (`survival::Vitals::treat`), and on a downed body it looks
/// for a part that suits it whatever this names; naming the right one is so
/// a standing player who presses the same key gets the arm that is bleeding
/// and not a refusal about their head.
pub fn part_to_dress(injuries: &crate::injury::Injuries, treatment: Treatment) -> crate::injury::Part {
    use crate::injury::Part;
    Part::ALL
        .into_iter()
        .find(|&part| {
            treatment.suits().iter().any(|&kind| {
                let wound = injuries.wound(part, kind);
                wound.is_open() && !wound.is_dressed()
            })
        })
        .unwrap_or(Part::Torso)
}

/// A body on the ground: why, and how long it has left.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct Down {
    pub cause: Cause,
    /// Seconds left.
    pub left: f32,
    /// Seconds it started with, for a screen that draws how much is gone.
    pub of: f32,
}

impl Down {
    /// A body downed by `cause`, or `None` if that cause kills outright.
    pub fn new(cause: Cause) -> Option<Down> {
        let profile = cause.profile()?;
        Some(Down { cause, left: profile.seconds, of: profile.seconds })
    }

    fn profile(self) -> Profile {
        // A `Down` is only ever made from a cause with a row.
        self.cause.profile().unwrap_or(Profile { seconds: 0.0, crawl: 0.0, rescue: Rescue::Dressing })
    }

    /// The crawl, as a fraction of a walk. Replaces the limp rather than
    /// multiplying it: a body on its belly is not favouring a leg.
    pub fn crawl(self) -> f32 {
        self.profile().crawl
    }

    pub fn rescue(self) -> Rescue {
        self.profile().rescue
    }

    /// Runs the clock. Answers whether it has run out.
    pub fn tick(&mut self, dt: f32) -> bool {
        if dt.is_finite() && dt > 0.0 {
            self.left -= dt;
        }
        self.left <= 0.0
    }

    /// Takes `damage` off the time, at [`SECONDS_PER_HEALTH`]. Answers
    /// whether it has run out.
    pub fn take(&mut self, damage: f32) -> bool {
        if damage.is_finite() && damage > 0.0 {
            self.left -= damage * SECONDS_PER_HEALTH;
        }
        self.left <= 0.0
    }

    /// How much of the time is left, 0..1.
    pub fn fraction_left(self) -> f32 {
        if self.of <= 0.0 {
            return 0.0;
        }
        (self.left / self.of).clamp(0.0, 1.0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_cause_the_server_writes_lands_on_the_row_it_was_argued_for() {
        let cases = [
            ("drowned", Cause::Drowned),
            ("froze to death", Cause::Cold),
            ("died of heatstroke", Cause::Heat),
            ("starved", Cause::Hunger),
            ("died of thirst", Cause::Thirst),
            ("bled to death", Cause::Bleeding),
            ("burned to death", Cause::Burn),
            ("was struck by lightning", Cause::Burn),
            ("choked on smoke", Cause::Smoke),
            ("drank bad water", Cause::Sickness),
            ("ate something they should not have", Cause::Sickness),
            ("fell from a great height", Cause::Fall),
            ("fell onto a stalagmite", Cause::Fall),
            ("was thrown by a horse", Cause::Fall),
            ("was crushed by a falling tree", Cause::Crushed),
            ("was crushed by falling rock", Cause::Crushed),
            ("was crushed by a collapsing roof", Cause::Crushed),
            ("was buried under falling earth", Cause::Crushed),
            ("was pulled down by a wolf", Cause::Wound),
            ("was struck down by alice", Cause::Wound),
            ("ran into sharpened stakes", Cause::Wound),
            ("was stung to death by bees", Cause::Wound),
        ];
        for (words, cause) in cases {
            assert_eq!(Cause::of(words), cause, "{words:?}");
        }
        // Every animal that can kill is a blow.
        for species in crate::animals::Species::ALL {
            assert_eq!(Cause::of(species.death_cause()), Cause::Wound, "{species:?}");
        }
    }

    #[test]
    fn drowning_and_crushing_kill_at_once_and_everything_else_crawls() {
        for cause in Cause::ALL {
            let instant = matches!(cause, Cause::Drowned | Cause::Crushed);
            assert_eq!(Down::new(cause).is_none(), instant, "{cause:?}");
        }
    }

    #[test]
    fn a_broken_body_crawls_slower_and_longer_than_a_bitten_one() {
        let fall = Down::new(Cause::Fall).unwrap();
        let bite = Down::new(Cause::Wound).unwrap();
        assert!(fall.crawl() < bite.crawl());
        assert!(fall.of > bite.of);
        // ...and bleeding out is the shortest of the blood.
        assert!(Down::new(Cause::Bleeding).unwrap().of < bite.of);
    }

    #[test]
    fn every_crawl_is_slower_than_half_a_walk_and_every_timer_is_long_enough_to_decide_in() {
        for cause in Cause::ALL {
            if let Some(profile) = cause.profile() {
                assert!(profile.crawl > 0.0 && profile.crawl < 0.5, "{cause:?} crawls at {}", profile.crawl);
                assert!(profile.seconds >= 10.0, "{cause:?} gives {} s", profile.seconds);
            }
        }
    }

    #[test]
    fn the_condition_that_downed_a_body_is_the_timer_and_everything_else_takes_from_it() {
        assert!(Cause::Hunger.shares_the_bill(Cause::Hunger));
        assert!(Cause::Cold.shares_the_bill(Cause::Cold));
        assert!(Cause::Bleeding.shares_the_bill(Cause::Wound), "a bite's own blood was billed twice");
        assert!(!Cause::Wound.shares_the_bill(Cause::Wound), "a second bite on a downed body cost nothing");
        assert!(!Cause::Burn.shares_the_bill(Cause::Burn), "a body left in the fire burned no faster");
        assert!(!Cause::Fall.shares_the_bill(Cause::Fall));
        assert!(!Cause::Cold.shares_the_bill(Cause::Fall), "a broken leg in the snow did not freeze");
    }

    #[test]
    fn a_bite_takes_seconds_off_the_clock_and_the_clock_runs_out() {
        let mut down = Down::new(Cause::Wound).unwrap();
        assert!(!down.take(5.0));
        assert!((down.left - (30.0 - 5.0 * SECONDS_PER_HEALTH)).abs() < 1e-4);
        assert!(!down.tick(1.0));
        assert!(down.take(100.0));
        assert_eq!(down.fraction_left(), 0.0);
    }

    #[test]
    fn a_dressing_is_a_bandage_or_a_poultice_and_a_splint_is_a_splint() {
        assert!(Rescue::Dressing.by_treatment(Treatment::Bandage));
        assert!(Rescue::Dressing.by_treatment(Treatment::Poultice));
        assert!(!Rescue::Dressing.by_treatment(Treatment::WillowBark), "bark stopped the bleeding");
        assert!(!Rescue::Dressing.by_treatment(Treatment::Splint));
        assert!(Rescue::Splint.by_treatment(Treatment::Splint));
        assert!(!Rescue::Food.by_treatment(Treatment::Bandage));
    }

    #[test]
    fn a_bandage_from_the_ground_goes_on_the_arm_that_is_bleeding() {
        use crate::injury::{Injuries, Kind, Part};
        let mut body = Injuries::default();
        body.inflict(Part::LeftArm, Kind::Bruise, 0.5);
        body.inflict(Part::RightLeg, Kind::Cut, 0.7);
        assert_eq!(part_to_dress(&body, Treatment::Bandage), Part::RightLeg);
        assert_eq!(part_to_dress(&body, Treatment::WillowBark), Part::LeftArm);
        assert_eq!(part_to_dress(&Injuries::default(), Treatment::Splint), Part::Torso);
    }

    #[test]
    fn a_freezing_body_is_let_up_a_degree_past_the_line_and_not_on_it() {
        use crate::body::FREEZING;
        assert!(!Rescue::Warmth.met_by(FREEZING, false));
        assert!(Rescue::Warmth.met_by(FREEZING + TEMPERATURE_MARGIN_C, false));
        assert!(Rescue::Air.met_by(0.0, true));
        assert!(!Rescue::Food.met_by(30.0, true), "food raised a body nobody fed");
    }
}
