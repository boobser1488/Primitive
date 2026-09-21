//! Injuries: what a blow, a fall or a fire leaves on a body, where it
//! leaves it, and what mends it.
//!
//! ## Why a body has parts now
//!
//! Health is an accumulator, and for most of this game's life it was the
//! only thing anything did to a player. A wolf's bite, a boar's tusk, a
//! bad landing and a step into a campfire were the same event -- a number
//! off a bar -- and they all had the same answer, which was to eat
//! something and wait. Nothing about *how* a player had been hurt changed
//! what they did next, so being hurt was never a decision.
//!
//! The one exception was the broken leg (`body::FRACTURE_SECONDS`), and it
//! is the reason this module looks the way it does: it was the only
//! injury a player walked about with, and it was the only one anybody
//! remembered. This is that idea carried to the rest of the body.
//!
//! ## Four kinds, because there are four questions
//!
//! Every kind is here because it asks the player something different, and
//! a fifth that asked one of the same questions was left out.
//!
//! * A **cut** bleeds. A shallow one closes on its own; a deep one goes on
//!   taking health until it is bandaged, and a bleeding body does not mend.
//!   *Bandage it here, or race home?*
//! * A **bruise** fades by itself and costs nothing -- except that a limb
//!   carrying one breaks under the next heavy blow. *Fight on with that
//!   arm?*
//! * A **fracture** slows a leg or weakens an arm, and it does not knit
//!   until it is splinted. *Splint it and keep working slowly, or splint
//!   it and go to bed?*
//! * A **burn**, if it is a bad one, stops the body mending until it is
//!   dressed. *Carry on at the wrong end of the health bar, or go and make
//!   a poultice?*
//!
//! **Frostbite was considered and left out.** The cold already warns three
//! times before it kills (see `body::shiver_hunger_multiplier`), and its
//! answer -- a fire, a roof, a coat -- would be exactly the same answer
//! with a black toe on the mannequin. A consequence with no new answer is
//! a number, not a decision. **Infection** was left out for the opposite
//! reason: a cut that turned bad after twenty minutes on its own clock is
//! a punishment a player cannot see coming, and every cut would become a
//! timer to babysit.
//!
//! ## Why the dice are not in here
//!
//! Which part a blow lands on is a roll, and this crate has no random
//! source on purpose -- the client would need one that agrees with the
//! server's (see `crafting::Attempt` for the same argument). So every
//! function that needs luck takes the roll as a number, and [`roll`] turns
//! any seed into one: the server seeds it, and a test hands in the exact
//! part it wants.
//!
//! ## What is on the server and what is on the client
//!
//! The rules are here and nowhere else. The server owns the state and
//! steps it; the client is told the whole `Injuries` value when it changes
//! and draws it on the mannequin -- and *applies* two of its effects to
//! its own prediction (a broken leg's pace, a broken arm's slower digging),
//! for the reason `body::FRACTURE_SPEED` already gave: a limp the server
//! corrected twenty times a second would be a player dragged about, not a
//! player limping.

use serde::{Deserialize, Serialize};

use crate::types::{block_kind, BlockId, BLOCK_BANDAGE, BLOCK_POULTICE, BLOCK_SPLINT, BLOCK_WILLOW_BARK};

/// How many parts a body is divided into.
pub const PARTS: usize = 6;

/// Where on a body.
///
/// **Six, and not the four equipment squares.** Clothing is worn on a head,
/// a chest, legs and feet because that is how garments are cut; an injury
/// is on an *arm*, and which arm matters -- a player with a broken left arm
/// and one with a broken right arm are both weaker, and both need to see on
/// the mannequin which one it is to know where to put the splint. Feet were
/// folded into the legs: a foot that could be hurt separately from its leg
/// would be a seventh square to drop a bandage on and no new consequence.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum Part {
    Head,
    Torso,
    LeftArm,
    RightArm,
    LeftLeg,
    RightLeg,
}

impl Part {
    /// Every part, in the order the wire and the save file index them.
    pub const ALL: [Part; PARTS] = [
        Part::Head,
        Part::Torso,
        Part::LeftArm,
        Part::RightArm,
        Part::LeftLeg,
        Part::RightLeg,
    ];

    pub fn index(self) -> usize {
        self as usize
    }

    /// Out of range is `None`, like every other index off a socket.
    pub fn from_index(index: usize) -> Option<Part> {
        Self::ALL.get(index).copied()
    }

    pub fn is_leg(self) -> bool {
        matches!(self, Part::LeftLeg | Part::RightLeg)
    }

    pub fn is_arm(self) -> bool {
        matches!(self, Part::LeftArm | Part::RightArm)
    }

    /// Whether a bone here breaks in this game.
    ///
    /// Limbs only. A cracked skull or a broken rib is real, and it would
    /// need an effect of its own -- a blurred screen, a shortened breath --
    /// that no player could fix with anything they can make in the stone
    /// age. A splint is the answer this game has, and a splint goes on a
    /// limb. So a blow that would break a head bruises it instead.
    pub fn can_break(self) -> bool {
        self.is_leg() || self.is_arm()
    }

    /// The part's name in the server's own messages. The client says it in
    /// four languages from `lang`; this is English for the log and the
    /// refusal text.
    pub fn name(self) -> &'static str {
        match self {
            Part::Head => "head",
            Part::Torso => "body",
            Part::LeftArm => "left arm",
            Part::RightArm => "right arm",
            Part::LeftLeg => "left leg",
            Part::RightLeg => "right leg",
        }
    }
}

/// What sort of harm.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum Kind {
    Cut,
    Bruise,
    Fracture,
    Burn,
}

impl Kind {
    pub const ALL: [Kind; 4] = [Kind::Cut, Kind::Bruise, Kind::Fracture, Kind::Burn];

    pub fn name(self) -> &'static str {
        match self {
            Kind::Cut => "cut",
            Kind::Bruise => "bruise",
            Kind::Fracture => "fracture",
            Kind::Burn => "burn",
        }
    }
}

/// What a wound can be dressed with.
///
/// Remembered on the wound rather than reduced to "treated", because a
/// poultice mends a burn faster than a bandage does and the rate has to
/// know which one is on it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum Treatment {
    Bandage,
    Splint,
    Poultice,
    /// Willow bark bound on a bruise. Last, because the wire and the saves
    /// carry this by its place in the list.
    WillowBark,
}

impl Treatment {
    /// What a block in the pack is, as a treatment.
    pub fn of(block: BlockId) -> Option<Treatment> {
        match block_kind(block) {
            BLOCK_BANDAGE => Some(Treatment::Bandage),
            BLOCK_SPLINT => Some(Treatment::Splint),
            BLOCK_POULTICE => Some(Treatment::Poultice),
            BLOCK_WILLOW_BARK => Some(Treatment::WillowBark),
            _ => None,
        }
    }

    /// The kinds of wound this dresses, **in the order it looks for them**.
    ///
    /// A bandage covers a cut before a burn on the same arm: the cut is
    /// the one taking health. A poultice is for a burn alone -- a pad of
    /// fungus on an open cut does not stop it bleeding, and letting it
    /// would make the bandage pointless for anybody who had found a tree
    /// with a bracket on it. A splint is for a break and nothing else.
    pub fn suits(self) -> &'static [Kind] {
        match self {
            Treatment::Bandage => &[Kind::Cut, Kind::Burn],
            Treatment::Splint => &[Kind::Fracture],
            Treatment::Poultice => &[Kind::Burn],
            // Bark on a bruise, and nothing else: it takes the swelling
            // down, and it stops no blood and sets no bone.
            Treatment::WillowBark => &[Kind::Bruise],
        }
    }

    pub fn name(self) -> &'static str {
        match self {
            Treatment::Bandage => "bandage",
            Treatment::Splint => "splint",
            Treatment::Poultice => "poultice",
            Treatment::WillowBark => "willow bark",
        }
    }
}

/// One kind of harm on one part.
#[derive(Debug, Clone, Copy, PartialEq, Default, Serialize, Deserialize)]
pub struct Wound {
    /// 0 (nothing) .. 1 (as bad as it gets). What mending takes down.
    pub severity: f32,
    /// What is on it, if anything.
    pub dressed: Option<Treatment>,
}

impl Wound {
    pub fn is_open(self) -> bool {
        self.severity > 0.0
    }

    pub fn is_dressed(self) -> bool {
        self.dressed.is_some()
    }
}

/// Everything wrong with one part.
///
/// A field per kind rather than a list of wounds, because a part has at
/// most one of each -- a second bite on the same arm is a worse cut, not
/// two cuts -- and a fixed shape is a fixed number of bytes on the wire and
/// in the save file.
#[derive(Debug, Clone, Copy, PartialEq, Default, Serialize, Deserialize)]
pub struct PartState {
    pub cut: Wound,
    pub bruise: Wound,
    pub fracture: Wound,
    pub burn: Wound,
}

impl PartState {
    pub fn wound(&self, kind: Kind) -> Wound {
        match kind {
            Kind::Cut => self.cut,
            Kind::Bruise => self.bruise,
            Kind::Fracture => self.fracture,
            Kind::Burn => self.burn,
        }
    }

    fn wound_mut(&mut self, kind: Kind) -> &mut Wound {
        match kind {
            Kind::Cut => &mut self.cut,
            Kind::Bruise => &mut self.bruise,
            Kind::Fracture => &mut self.fracture,
            Kind::Burn => &mut self.burn,
        }
    }

    pub fn is_whole(&self) -> bool {
        Kind::ALL.iter().all(|&kind| !self.wound(kind).is_open())
    }

    /// The wound on this part that matters most, which is what the
    /// mannequin colours the part by.
    pub fn worst(&self) -> Option<(Kind, Wound)> {
        Kind::ALL
            .iter()
            .map(|&kind| (kind, self.wound(kind)))
            .filter(|(_, wound)| wound.is_open())
            .max_by(|a, b| danger(a.0, a.1).total_cmp(&danger(b.0, b.1)))
    }
}

/// How much a wound matters, for choosing which one to show.
///
/// **Undressed outranks dressed, and a break outranks everything**: the
/// question a player asks of the mannequin is "what do I have to do
/// something about", and a splinted leg that is mending is less of an
/// answer to that than a fresh cut. The half-plus-half term keeps a small
/// fracture above a large bruise, because it is.
pub fn danger(kind: Kind, wound: Wound) -> f32 {
    if !wound.is_open() {
        return 0.0;
    }
    let weight = match kind {
        Kind::Fracture => 1.0,
        Kind::Cut => 0.9,
        Kind::Burn => 0.7,
        Kind::Bruise => 0.3,
    };
    let dressed = if wound.is_dressed() { 0.5 } else { 1.0 };
    weight * dressed * (0.5 + 0.5 * wound.severity.clamp(0.0, 1.0))
}

/// Whether this wound goes away without anything put on it.
///
/// **The one rule the whole mechanic turns on**, written once: minor harm
/// mends, serious harm waits for the player. A bruise always fades; a cut
/// closes only while it is shallow; a burn only while it is light; a break
/// never knits unset.
pub fn heals_alone(kind: Kind, wound: Wound) -> bool {
    match kind {
        Kind::Bruise => true,
        Kind::Cut => wound.severity <= CUT_CLOTS_BELOW,
        Kind::Burn => wound.severity <= BURN_MINOR,
        Kind::Fracture => false,
    }
}

// ---- cuts ----

/// How much damage, through the armour, makes a cut as deep as it goes.
///
/// Eight, so a boar's gore (4) is a cut of one half and a bear's swipe is
/// to the bone. Measured against *what gets through*, so leather turns a
/// wolf's bite (3) into a scratch that closes on its own -- which is the
/// best reason there has ever been to wear it in a wood.
pub const CUT_FULL_DAMAGE: f32 = 8.0;

/// The depth below which a cut clots by itself.
///
/// Three tenths: a bite that got through leather is under it, and a bare
/// wolf bite (0.375) is not. So a wolf fight in nothing is a bandage and a
/// wolf fight in a tunic is not -- the decision this line exists to make.
pub const CUT_CLOTS_BELOW: f32 = 0.3;

/// Health a second an undressed cut of full depth takes.
///
/// **Slow, and not stoppable by eating.** A deep wolf bite (0.375) is under
/// a hundredth of a point a second, which is ten minutes to lose five
/// points -- long enough to walk home, short enough that a player with
/// three of them and no bandage is on a clock they can feel. What makes it
/// matter more than the number is that a bleeding body does not mend at
/// all (see [`Injuries::stops_mending`]): the health that is gone stays
/// gone until the bandage is on.
pub const BLEED_PER_SECOND: f32 = 0.04;

/// How long a shallow cut takes to close from the clotting line, in
/// seconds. A minute, so a scratch that stops bleeding does so while the
/// player is still looking at it.
pub const CUT_CLOT_SECONDS: f32 = 60.0;

/// How long a bandaged cut of full depth takes to close, in seconds.
///
/// Five minutes awake. **Gradual on purpose**: the bandage stops the
/// bleeding at once -- that is what it is for -- and the wound itself goes
/// down a little at a time, so the player watches the arm on the
/// mannequin fade from red rather than seeing it wiped clean by a click.
pub const CUT_MEND_SECONDS: f32 = 300.0;

// ---- bruises ----

/// How much damage makes a bruise as bad as it gets.
pub const BRUISE_FULL_DAMAGE: f32 = 6.0;

/// How long a full bruise takes to fade, in seconds. Three minutes: the
/// length of the next fight, which is when it matters.
pub const BRUISE_SECONDS: f32 = 180.0;

/// A bruise this bad or worse is a limb that breaks under a heavy blow.
///
/// **The only thing a bruise costs**, and that is enough: a player whose
/// arm is purple on the mannequin has been told, before the second boar,
/// that this is the arm that will not survive it.
pub const BRUISE_BREAKS_AT: f32 = 0.4;

/// A blow this heavy breaks a bruised limb.
///
/// A boar's or a lion's, not a fist's (1.5): nobody breaks an arm in a
/// scuffle, and nothing here should make fighting another player with bare
/// hands a way to cripple them.
pub const BREAKS_BRUISED: f32 = 3.5;

/// ...and this heavy breaks a limb outright, bruised or not. A bear.
pub const BREAKS_OUTRIGHT: f32 = 8.0;

// ---- fractures ----

/// How much a broken arm leaves of a swing.
///
/// Half, for a blow and for a pick alike. **An arm, not a hand**: which one
/// is broken does not matter to how hard a two-handed pick comes down, and
/// a rule that asked which arm was the swinging one would be a rule about
/// handedness nobody chose.
pub const BROKEN_ARM_STRENGTH: f32 = 0.5;

// ---- burns ----

/// Severity a second each leg takes standing in fire.
///
/// A quarter: a step in and out of a campfire is a light burn that goes
/// on its own, and three seconds of it is a burn that will not. The feet
/// are what is in the fire, so the legs are what burn.
pub const BURN_IN_FIRE_PER_SECOND: f32 = 0.25;

/// The line under which a burn heals undressed.
pub const BURN_MINOR: f32 = 0.35;

/// How long a dressed burn of full severity takes to heal, in seconds.
pub const BURN_MEND_SECONDS: f32 = 360.0;

/// How much faster a poultice heals a burn than a bandage does.
///
/// Three times. **This is what makes the poultice worth a fungus**: a
/// bandage does cover a burn, so a player with only bandages is not stuck,
/// and a player who took the trouble to make the right thing for the job
/// gets their health back sooner.
pub const POULTICE_FACTOR: f32 = 3.0;

/// How much faster willow bark bound on a bruise takes it down.
///
/// **Three times, and the reason is the next blow, not the health.** A
/// bruise is the warning a break gives (`BRUISE_BREAKS_AT`): a heavy hit on
/// a bruised limb breaks it. A bruise left alone is three minutes of a limb
/// that will snap under the next boar; bark on it is one. So the bark is a
/// decision about what to do between two fights -- stop and bind the leg, or
/// walk on with the warning still on it -- and a player who carries a
/// strip or two from the river's willows has the shorter answer.
///
/// Willow because the bark is what salicin was first taken from, chewed and
/// bound on for pain and swelling long before it was a pill.
pub const BARK_FACTOR: f32 = 3.0;

// ---- falls ----

/// Fall damage below which the legs are not even bruised. A jump off a
/// wall is not an injury.
pub const FALL_BRUISES_FROM: f32 = 2.0;

/// Which part a roll lands on, for a blow with this reach.
///
/// What an attacker can get at. A boar goes for the legs; a wolf for
/// whatever limb is nearest; a bear or a lion rears and comes down on the
/// head and shoulders; a falling block lands on top of a person.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Reach {
    /// Knee-high: legs and body.
    Low,
    /// Limbs first.
    Limbs,
    /// From above.
    High,
    /// Anywhere at all -- a person swinging at a person.
    Any,
}

impl Reach {
    /// The weights, in `Part::ALL` order. Each row sums to one.
    fn weights(self) -> [f32; PARTS] {
        match self {
            //              head  body  l.arm r.arm l.leg r.leg
            Reach::Low => [0.00, 0.30, 0.00, 0.00, 0.35, 0.35],
            Reach::Limbs => [0.00, 0.10, 0.20, 0.20, 0.25, 0.25],
            Reach::High => [0.20, 0.30, 0.25, 0.25, 0.00, 0.00],
            Reach::Any => [0.15, 0.35, 0.15, 0.15, 0.10, 0.10],
        }
    }

    /// The part a roll in 0..1 lands on.
    ///
    /// A nonsense roll is the body, which is the one part every reach can
    /// hit -- a `NaN` off somebody's arithmetic must still land somewhere.
    pub fn pick(self, roll: f32) -> Part {
        if !roll.is_finite() {
            return Part::Torso;
        }
        let roll = roll.clamp(0.0, 0.999_999);
        let mut edge = 0.0;
        let weights = self.weights();
        for (part, weight) in Part::ALL.iter().zip(weights) {
            edge += weight;
            if roll < edge {
                return *part;
            }
        }
        // Rounding in the running sum; the last part with any weight.
        Part::ALL
            .iter()
            .zip(weights)
            .rev()
            .find(|(_, weight)| *weight > 0.0)
            .map(|(part, _)| *part)
            .unwrap_or(Part::Torso)
    }
}

/// What hit somebody, in the terms of what it does to them.
///
/// **A property of the blow, not of the attacker**, so a mod or a falling
/// tree can say what it is without this module knowing what a tree is.
/// The server names one per damage source.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Blow {
    /// Teeth: cuts, and never a break. A wolf.
    Bite,
    /// A tusk driven in low: cuts and bruises. A boar.
    Tusk,
    /// A paw with claws and weight behind it: cuts and bruises, and a
    /// heavy one breaks. A bear, a lion.
    Claw,
    /// An edge in a person's hand: cuts. A knife, an axe, a spear.
    Edge,
    /// Anything without an edge: bruises. A fist, a haft, a mod's strike.
    Blunt,
    /// Something heavy arriving from above: bruises, and breaks when it is
    /// heavy enough. A falling block, a falling tree.
    Crush,
}

impl Blow {
    pub fn reach(self) -> Reach {
        match self {
            Blow::Bite => Reach::Limbs,
            Blow::Tusk => Reach::Low,
            Blow::Claw | Blow::Crush => Reach::High,
            Blow::Edge | Blow::Blunt => Reach::Any,
        }
    }

    fn cuts(self) -> bool {
        matches!(self, Blow::Bite | Blow::Tusk | Blow::Claw | Blow::Edge)
    }

    fn bruises(self) -> bool {
        matches!(self, Blow::Tusk | Blow::Claw | Blow::Blunt | Blow::Crush)
    }

    /// How much of this blow shows, in drops of blood, at the moment it
    /// lands.
    ///
    /// **A blow is the only thing that sprays.** The burst belongs to the
    /// moment something struck, and what a cut goes on to lose afterwards is
    /// [`Injuries::drips_per_second`]'s, a drop at a time. An edge or a set
    /// of teeth throws the full spray; a blow that only bruises -- a fist, a
    /// haft, a falling block -- throws a few drops, because a hit that shows
    /// nothing at all reads as a miss, and a punch in the mouth does bleed.
    pub fn drops(self) -> u8 {
        if self.cuts() {
            BLOW_DROPS
        } else {
            BRUISE_DROPS
        }
    }
}

/// Drops of blood a cutting blow throws: the client's own burst for an
/// animal struck (`particles::BLOOD_DROPS`), so a wolf bleeds on a spear
/// exactly as a player bleeds on a wolf.
pub const BLOW_DROPS: u8 = 8;
/// ...and a blow that only bruises.
pub const BRUISE_DROPS: u8 = 3;

/// The most drops a second a bleeding body shows, however badly it is cut.
///
/// **A few, and a ceiling rather than a rate.** The report that this exists
/// for was "I ate a fish and a fountain of blood came out of me for three
/// minutes": the client drew every tick of an illness as a blow, eight drops
/// twenty times a second. Illness is not a cut and now sheds nothing (see
/// `drips_per_second`); a cut is the one harm that *should* leak, and it has to
/// look like a leak -- a drop, and then another -- rather than like the spray a
/// tusk throws. One and a half a second is a trail somebody could follow, and a
/// whole second of the worst of it is under a fifth of one blow's burst.
pub const MAX_DRIPS_PER_SECOND: f32 = 1.5;

/// ...and the fewest, for a cut that is open at all: a scratch shows, slowly.
const MIN_DRIPS_PER_SECOND: f32 = 0.4;

/// Why a treatment was not used.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Refusal {
    /// What was dropped on the body is not a treatment at all.
    NotATreatment,
    /// There is nothing on that part it helps -- no wound of a kind it
    /// suits, or one already dressed.
    NothingItHelps,
}

/// What one step of mending did.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Mending {
    /// Health lost to bleeding this step.
    pub blood: f32,
    /// Wounds that closed completely this step.
    pub closed: Vec<(Part, Kind)>,
}

/// Every wound on a body.
#[derive(Debug, Clone, Copy, PartialEq, Default, Serialize, Deserialize)]
pub struct Injuries {
    parts: [PartState; PARTS],
}

impl Injuries {
    pub fn part(&self, part: Part) -> &PartState {
        &self.parts[part.index()]
    }

    pub fn wound(&self, part: Part, kind: Kind) -> Wound {
        self.part(part).wound(kind)
    }

    /// Not a mark on them.
    pub fn is_whole(&self) -> bool {
        self.parts.iter().all(PartState::is_whole)
    }

    /// Adds harm to a part.
    ///
    /// Severities add, up to one -- except a break, which is the longer of
    /// the two: a second fall on a broken leg is not two broken legs, and
    /// the rule `survival` has kept since fractures existed stays true.
    ///
    /// **A fresh wound undoes the dressing on it.** A bandaged arm bitten
    /// again is bleeding again, and a splint knocked by a second fall is a
    /// splint that has to be set again. Keeping the dressing would make a
    /// bandage armour: put one on at the start of a fight and every cut
    /// after it is already dressed.
    pub fn inflict(&mut self, part: Part, kind: Kind, severity: f32) {
        if !severity.is_finite() || severity <= 0.0 {
            return;
        }
        let kind = if kind == Kind::Fracture && !part.can_break() {
            Kind::Bruise
        } else {
            kind
        };
        let wound = self.parts[part.index()].wound_mut(kind);
        wound.severity = match kind {
            Kind::Fracture => wound.severity.max(severity),
            _ => wound.severity + severity,
        }
        .min(1.0);
        wound.dressed = None;
    }

    /// A blow from something, `damage` of which got through the armour.
    ///
    /// Answers the part it landed on. The bruise that was there *before*
    /// this blow is what decides a break, so a single heavy hit on a clean
    /// arm bruises it and the next one breaks it -- the warning comes
    /// first.
    pub fn take_blow(&mut self, blow: Blow, damage: f32, roll: f32) -> Option<Part> {
        if !damage.is_finite() || damage <= 0.0 {
            return None;
        }
        let part = blow.reach().pick(roll);
        let was_bruised = self.wound(part, Kind::Bruise).severity >= BRUISE_BREAKS_AT;
        if blow.cuts() {
            self.inflict(part, Kind::Cut, damage / CUT_FULL_DAMAGE);
        }
        if blow.bruises() {
            let breaks = damage >= BREAKS_OUTRIGHT || (was_bruised && damage >= BREAKS_BRUISED);
            if breaks && part.can_break() {
                self.inflict(part, Kind::Fracture, 1.0);
            }
            self.inflict(part, Kind::Bruise, damage / BRUISE_FULL_DAMAGE);
        }
        Some(part)
    }

    /// A landing that hurt.
    ///
    /// `damage` is the fall's, load included -- see `survival::fall_damage`
    /// -- and the break threshold is the one falls have always used
    /// (`body::FRACTURE_DAMAGE`). One leg breaks, the roll's; both are
    /// bruised, because both hit the ground.
    pub fn fall(&mut self, damage: f32, roll: f32) {
        if !damage.is_finite() || damage < FALL_BRUISES_FROM {
            return;
        }
        if damage >= crate::body::FRACTURE_DAMAGE {
            let leg = if roll.is_finite() && roll >= 0.5 {
                Part::RightLeg
            } else {
                Part::LeftLeg
            };
            self.inflict(leg, Kind::Fracture, 1.0);
        }
        for leg in [Part::LeftLeg, Part::RightLeg] {
            self.inflict(leg, Kind::Bruise, damage / BRUISE_FULL_DAMAGE);
        }
    }

    /// A moment of standing in fire. The feet are in it, so the legs burn.
    pub fn scorch(&mut self, dt: f32) {
        if !dt.is_finite() || dt <= 0.0 {
            return;
        }
        for leg in [Part::LeftLeg, Part::RightLeg] {
            self.inflict(leg, Kind::Burn, BURN_IN_FIRE_PER_SECOND * dt);
        }
    }

    /// Puts a treatment on a part.
    ///
    /// Answers which wound it dressed, or why it did nothing -- and **a
    /// refusal changes nothing**, so the caller can spend the item on `Ok`
    /// and only then. A bandage dropped on a leg with no cut on it is a
    /// bandage the player still has.
    pub fn treat(&mut self, part: Part, block: BlockId) -> Result<Kind, Refusal> {
        let treatment = Treatment::of(block).ok_or(Refusal::NotATreatment)?;
        let state = &mut self.parts[part.index()];
        for &kind in treatment.suits() {
            let wound = state.wound_mut(kind);
            if wound.is_open() && !wound.is_dressed() {
                wound.dressed = Some(treatment);
                return Ok(kind);
            }
        }
        Err(Refusal::NothingItHelps)
    }

    /// Health a second the undressed cuts are taking, all of them.
    pub fn bleeding_per_second(&self) -> f32 {
        self.parts
            .iter()
            .filter(|state| state.cut.is_open() && !state.cut.is_dressed())
            .map(|state| BLEED_PER_SECOND * state.cut.severity)
            .sum()
    }

    pub fn is_bleeding(&self) -> bool {
        self.bleeding_per_second() > 0.0
    }

    /// Drops of blood a second the open cuts show, all of them together.
    ///
    /// **Zero for anything that is not a cut.** Illness, hunger, thirst, the
    /// cold and a fire all take health, and none of them opens the skin, so
    /// none of them leaks -- the rule the fish-and-fountain report broke on
    /// the client's side. A dressed cut shows nothing either: the bandage is
    /// what it is for.
    ///
    /// From [`MIN_DRIPS_PER_SECOND`] for the shallowest open cut to
    /// [`MAX_DRIPS_PER_SECOND`], which a bare wolf bite (0.375) is already at.
    pub fn drips_per_second(&self) -> f32 {
        let depth: f32 = self
            .parts
            .iter()
            .filter(|state| state.cut.is_open() && !state.cut.is_dressed())
            .map(|state| state.cut.severity)
            .sum();
        if depth <= 0.0 {
            return 0.0;
        }
        (MIN_DRIPS_PER_SECOND + 3.0 * depth).min(MAX_DRIPS_PER_SECOND)
    }

    /// Whether the body is too busy with a wound to put health back.
    ///
    /// **Bleeding, or a bad burn left open.** Stated as a rule the
    /// regeneration asks rather than as damage that happens to outpace it,
    /// for the reason the cold's is (see `survival::Vitals::regenerate`): a
    /// rule that holds only because one rate is bigger than another breaks
    /// the day either is retuned.
    pub fn stops_mending(&self) -> bool {
        self.is_bleeding()
            || self
                .parts
                .iter()
                .any(|state| state.burn.is_open() && !state.burn.is_dressed() && !heals_alone(Kind::Burn, state.burn))
    }

    /// Whether either leg is broken, set or not.
    ///
    /// **A splint does not give the leg back.** It is what lets the bone
    /// knit; the limp lasts until it has, which is what makes the bed the
    /// second half of the answer.
    pub fn leg_broken(&self) -> bool {
        [Part::LeftLeg, Part::RightLeg]
            .iter()
            .any(|&leg| self.wound(leg, Kind::Fracture).is_open())
    }

    /// Which leg is broken, if one is: the right when both are.
    ///
    /// What a watcher sees a player favour (`protocol::PlayerState::limp_left`).
    /// Both broken is still one side's limp -- a figure cannot favour two
    /// legs at once -- and the right is the side every limp took before
    /// there was a side to send.
    pub fn broken_leg(&self) -> Option<Part> {
        [Part::RightLeg, Part::LeftLeg]
            .into_iter()
            .find(|&leg| self.wound(leg, Kind::Fracture).is_open())
    }

    pub fn arm_broken(&self) -> bool {
        [Part::LeftArm, Part::RightArm]
            .iter()
            .any(|&arm| self.wound(arm, Kind::Fracture).is_open())
    }

    /// What the legs leave of a walk. See `body::FRACTURE_SPEED`.
    pub fn speed_factor(&self) -> f32 {
        if self.leg_broken() {
            crate::body::FRACTURE_SPEED
        } else {
            1.0
        }
    }

    /// Whether the legs will take a sprint.
    ///
    /// **The sprint goes and the jump stays.** A broken leg that could not
    /// jump would trap a player in the first hole a block deep for half an
    /// hour -- the failure the stamina rule already names, "half a jump is a
    /// way to end up stuck in a hole" -- and an injury that leaves a player
    /// unable to get home is a world they abandon. What cannot be done on a
    /// broken leg is *run*, and that is what running from a bear needs.
    pub fn may_sprint(&self) -> bool {
        !self.leg_broken()
    }

    /// What the arms leave of a blow and of a swing at a block.
    pub fn strength_factor(&self) -> f32 {
        if self.arm_broken() {
            BROKEN_ARM_STRENGTH
        } else {
            1.0
        }
    }

    /// One step of the body mending and bleeding.
    ///
    /// The bleeding is billed on the depth the cuts had at the *start* of
    /// the step, so a cut that closes this step still cost this step. What
    /// is asleep mends at `body::FRACTURE_SLEEP_FACTOR` -- every kind of
    /// harm, not only the bone: sleep is when a body mends, and a rule that
    /// made a bed help a leg and not an arm would be a rule to learn.
    pub fn step(&mut self, dt: f32, asleep: bool) -> Mending {
        let mut mending = Mending::default();
        if !dt.is_finite() || dt <= 0.0 {
            return mending;
        }
        mending.blood = self.bleeding_per_second() * dt;
        let sleep = if asleep {
            crate::body::FRACTURE_SLEEP_FACTOR
        } else {
            1.0
        };
        for part in Part::ALL {
            let state = &mut self.parts[part.index()];
            for kind in Kind::ALL {
                let wound = state.wound_mut(kind);
                if !wound.is_open() {
                    continue;
                }
                let seconds = match (kind, wound.dressed) {
                    (Kind::Cut, Some(_)) => Some(CUT_MEND_SECONDS),
                    (Kind::Cut, None) if heals_alone(kind, *wound) => Some(CUT_CLOT_SECONDS / CUT_CLOTS_BELOW),
                    (Kind::Bruise, Some(Treatment::WillowBark)) => Some(BRUISE_SECONDS / BARK_FACTOR),
                    (Kind::Bruise, _) => Some(BRUISE_SECONDS),
                    (Kind::Fracture, Some(_)) => Some(crate::body::FRACTURE_SECONDS),
                    (Kind::Burn, Some(Treatment::Poultice)) => Some(BURN_MEND_SECONDS / POULTICE_FACTOR),
                    (Kind::Burn, Some(_)) => Some(BURN_MEND_SECONDS),
                    (Kind::Burn, None) if heals_alone(kind, *wound) => Some(BURN_MEND_SECONDS),
                    _ => None,
                };
                let Some(seconds) = seconds else {
                    continue;
                };
                wound.severity -= dt * sleep / seconds;
                if wound.severity <= 0.0 {
                    *wound = Wound::default();
                    mending.closed.push((part, kind));
                }
            }
        }
        mending
    }

    /// Whether a client that was last told `last` needs telling again.
    ///
    /// On any wound opening, closing or being dressed, and otherwise when a
    /// severity has moved by a twentieth -- which is about a shade of the
    /// mannequin's red. Without the threshold a bleeding player would be a
    /// message every tick for the length of the wound.
    pub fn worth_reporting(&self, last: &Injuries) -> bool {
        self.parts.iter().zip(last.parts.iter()).any(|(now, then)| {
            Kind::ALL.iter().any(|&kind| {
                let (a, b) = (now.wound(kind), then.wound(kind));
                a.is_open() != b.is_open()
                    || a.dressed != b.dressed
                    || (a.severity - b.severity).abs() > 0.05
            })
        })
    }

    /// Repairs a value that came off a file or a socket.
    ///
    /// A `NaN` severity is a wound that neither closes nor bleeds a
    /// countable amount, and a break on a head is a shape no rule here
    /// produces.
    pub fn sanitize(&mut self) {
        for part in Part::ALL {
            let state = &mut self.parts[part.index()];
            for kind in Kind::ALL {
                let wound = state.wound_mut(kind);
                if !wound.severity.is_finite() || wound.severity <= 0.0 {
                    *wound = Wound::default();
                } else {
                    wound.severity = wound.severity.min(1.0);
                }
            }
            if !part.can_break() {
                state.fracture = Wound::default();
            }
        }
    }
}

/// A roll in 0..1 out of any seed. SplitMix64: a handful of multiplies,
/// and every bit of the seed reaches every bit of the answer.
pub fn roll(seed: u64) -> f32 {
    let mut z = seed.wrapping_add(0x9E37_79B9_7F4A_7C15);
    z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
    z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
    z ^= z >> 31;
    (z >> 40) as f32 / (1u64 << 24) as f32
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Runs `seconds` of mending at 20 Hz and answers the blood it cost.
    fn live(injuries: &mut Injuries, seconds: f32, asleep: bool) -> f32 {
        let dt = 1.0 / 20.0;
        let mut blood = 0.0;
        for _ in 0..(seconds / dt) as usize {
            blood += injuries.step(dt, asleep).blood;
        }
        blood
    }

    #[test]
    fn a_shallow_cut_closes_on_its_own_and_a_deep_one_bleeds_until_it_is_bandaged() {
        let mut scratch = Injuries::default();
        scratch.inflict(Part::LeftArm, Kind::Cut, 0.2);
        live(&mut scratch, 120.0, false);
        assert!(scratch.is_whole(), "a scratch was still open two minutes later");

        let mut bite = Injuries::default();
        bite.inflict(Part::LeftArm, Kind::Cut, 0.6);
        let blood = live(&mut bite, 600.0, false);
        assert!(bite.is_bleeding(), "a deep cut stopped bleeding on its own");
        assert!(blood > 10.0, "ten minutes of a deep cut cost only {blood}");
        assert_eq!(bite.wound(Part::LeftArm, Kind::Cut).severity, 0.6, "a deep cut closed undressed");

        assert_eq!(bite.treat(Part::LeftArm, BLOCK_BANDAGE), Ok(Kind::Cut));
        assert!(!bite.is_bleeding(), "a bandage did not stop the bleeding");
        assert_eq!(live(&mut bite, 1.0, false), 0.0);
    }

    #[test]
    fn a_dressed_wound_mends_a_little_at_a_time_rather_than_at_once() {
        // What the mannequin is for: watching the arm fade, not seeing it
        // wiped clean by a click.
        let mut injuries = Injuries::default();
        injuries.inflict(Part::RightArm, Kind::Cut, 1.0);
        injuries.treat(Part::RightArm, BLOCK_BANDAGE).expect("a cut takes a bandage");
        let mut last = 1.0;
        for minute in 1..=4 {
            live(&mut injuries, 60.0, false);
            let now = injuries.wound(Part::RightArm, Kind::Cut).severity;
            assert!(now < last && now > 0.0, "minute {minute}: {last} went to {now}");
            last = now;
        }
        live(&mut injuries, 90.0, false);
        assert!(injuries.is_whole(), "a bandaged cut never closed");
    }

    #[test]
    fn sleep_mends_faster_than_waiting() {
        let broken = || {
            let mut injuries = Injuries::default();
            injuries.inflict(Part::LeftLeg, Kind::Fracture, 1.0);
            injuries.treat(Part::LeftLeg, BLOCK_SPLINT).expect("a break takes a splint");
            injuries
        };
        let (mut awake, mut asleep) = (broken(), broken());
        live(&mut awake, 300.0, false);
        live(&mut asleep, 300.0, true);
        assert!(
            asleep.wound(Part::LeftLeg, Kind::Fracture).severity
                < awake.wound(Part::LeftLeg, Kind::Fracture).severity,
            "five minutes in bed mended no more than five minutes up"
        );
    }

    #[test]
    fn a_break_does_not_knit_until_it_is_splinted() {
        let mut injuries = Injuries::default();
        injuries.fall(crate::body::FRACTURE_DAMAGE + 1.0, 0.1);
        assert!(injuries.leg_broken(), "a hard landing broke nothing");
        live(&mut injuries, 3600.0, true);
        assert!(injuries.leg_broken(), "an unset leg knitted in bed");
        let leg = Part::ALL
            .into_iter()
            .find(|&p| injuries.wound(p, Kind::Fracture).is_open())
            .expect("a broken leg somewhere");
        injuries.treat(leg, BLOCK_SPLINT).expect("a splint fits a break");
        // Ten seconds past the half hour: 36,000 steps of float
        // subtraction leave a crumb, and a crumb is still a broken leg.
        live(&mut injuries, crate::body::FRACTURE_SECONDS + 10.0, false);
        assert!(!injuries.leg_broken(), "a splinted leg never knitted");
    }

    #[test]
    fn every_treatment_is_refused_where_it_helps_nothing_and_changes_nothing() {
        let mut injuries = Injuries::default();
        injuries.inflict(Part::LeftLeg, Kind::Cut, 0.8);
        let before = injuries;
        // A splint on a cut, a poultice on a cut, a bandage on a clean arm.
        assert_eq!(injuries.treat(Part::LeftLeg, BLOCK_SPLINT), Err(Refusal::NothingItHelps));
        assert_eq!(injuries.treat(Part::LeftLeg, BLOCK_POULTICE), Err(Refusal::NothingItHelps));
        assert_eq!(injuries.treat(Part::LeftArm, BLOCK_BANDAGE), Err(Refusal::NothingItHelps));
        assert_eq!(
            injuries.treat(Part::LeftLeg, crate::types::BLOCK_STONE),
            Err(Refusal::NotATreatment)
        );
        assert_eq!(injuries, before, "a refused treatment changed the body");
        // ...and a second bandage on a dressed cut is refused too, or a
        // player could spend a stack on one bite.
        injuries.treat(Part::LeftLeg, BLOCK_BANDAGE).expect("the first one fits");
        assert_eq!(injuries.treat(Part::LeftLeg, BLOCK_BANDAGE), Err(Refusal::NothingItHelps));
    }

    #[test]
    fn willow_bark_takes_a_bruise_down_in_a_third_of_the_time_and_dresses_nothing_else() {
        let bruised = |bark: bool| {
            let mut injuries = Injuries::default();
            injuries.inflict(Part::LeftLeg, Kind::Bruise, 0.9);
            if bark {
                assert_eq!(injuries.treat(Part::LeftLeg, BLOCK_WILLOW_BARK), Ok(Kind::Bruise));
            }
            live(&mut injuries, 30.0, false);
            injuries.wound(Part::LeftLeg, Kind::Bruise).severity
        };
        let (left, bound) = (0.9 - bruised(false), 0.9 - bruised(true));
        assert!((bound / left - BARK_FACTOR).abs() < 0.05, "bark mended {bound} against {left}");
        let mut cut = Injuries::default();
        cut.inflict(Part::LeftArm, Kind::Cut, 0.5);
        assert_eq!(cut.treat(Part::LeftArm, BLOCK_WILLOW_BARK), Err(Refusal::NothingItHelps), "bark stopped a bleed");
    }

    #[test]
    fn a_poultice_heals_a_burn_faster_than_a_bandage_does() {
        let burnt = |with| {
            let mut injuries = Injuries::default();
            injuries.inflict(Part::Torso, Kind::Burn, 0.9);
            injuries.treat(Part::Torso, with).expect("a burn takes both");
            live(&mut injuries, 60.0, false);
            injuries.wound(Part::Torso, Kind::Burn).severity
        };
        assert!(burnt(BLOCK_POULTICE) < burnt(BLOCK_BANDAGE));
        // ...and a bad burn left open is a body that does not mend.
        let mut open = Injuries::default();
        open.inflict(Part::Torso, Kind::Burn, 0.9);
        assert!(open.stops_mending(), "a bad open burn let the body mend");
        let mut light = Injuries::default();
        light.inflict(Part::Torso, Kind::Burn, 0.2);
        assert!(!light.stops_mending(), "a light burn stopped the body mending");
    }

    #[test]
    fn a_bruised_limb_breaks_under_the_next_heavy_blow_and_a_clean_one_does_not() {
        // The warning first: one boar's worth on a clean leg bruises it.
        let low_roll = 0.8; // `Reach::Low` puts 0.65..1.0 on the right leg.
        let mut injuries = Injuries::default();
        let part = injuries.take_blow(Blow::Tusk, 4.0, low_roll);
        assert_eq!(part, Some(Part::RightLeg));
        assert!(!injuries.leg_broken(), "a first blow on a clean leg broke it");
        assert!(injuries.wound(Part::RightLeg, Kind::Bruise).severity >= BRUISE_BREAKS_AT);
        // ...and the same blow again is the break it warned of.
        injuries.take_blow(Blow::Tusk, 4.0, low_roll);
        assert!(injuries.leg_broken(), "a heavy blow on a bruised leg left it whole");

        // A fist never does, however bruised the arm.
        let mut scuffle = Injuries::default();
        for _ in 0..20 {
            scuffle.take_blow(Blow::Blunt, crate::combat::MELEE_DAMAGE, 0.55);
        }
        assert!(!scuffle.arm_broken() && !scuffle.leg_broken(), "fists broke a bone");
    }

    #[test]
    fn a_head_is_bruised_where_a_leg_would_break() {
        let mut injuries = Injuries::default();
        injuries.inflict(Part::Head, Kind::Fracture, 1.0);
        assert!(!injuries.wound(Part::Head, Kind::Fracture).is_open(), "a skull broke");
        assert!(injuries.wound(Part::Head, Kind::Bruise).is_open());
    }

    #[test]
    fn a_fresh_wound_tears_the_dressing_off() {
        let mut injuries = Injuries::default();
        injuries.inflict(Part::LeftArm, Kind::Cut, 0.5);
        injuries.treat(Part::LeftArm, BLOCK_BANDAGE).expect("fits");
        injuries.inflict(Part::LeftArm, Kind::Cut, 0.2);
        assert!(injuries.is_bleeding(), "a bitten bandage kept a new cut from bleeding");
    }

    #[test]
    fn a_broken_leg_stops_the_sprint_and_a_broken_arm_weakens_the_swing() {
        let mut injuries = Injuries::default();
        assert!(injuries.may_sprint());
        assert_eq!(injuries.strength_factor(), 1.0);
        assert_eq!(injuries.speed_factor(), 1.0);
        injuries.inflict(Part::LeftLeg, Kind::Fracture, 1.0);
        assert!(!injuries.may_sprint());
        assert_eq!(injuries.speed_factor(), crate::body::FRACTURE_SPEED);
        assert_eq!(injuries.strength_factor(), 1.0, "a leg weakened an arm");
        injuries.inflict(Part::RightArm, Kind::Fracture, 1.0);
        assert_eq!(injuries.strength_factor(), BROKEN_ARM_STRENGTH);
    }

    #[test]
    fn every_reach_lands_somewhere_it_can_reach_and_every_part_can_be_landed_on() {
        let mut landed = [false; PARTS];
        for reach in [Reach::Low, Reach::Limbs, Reach::High, Reach::Any] {
            let weights = reach.weights();
            assert!((weights.iter().sum::<f32>() - 1.0).abs() < 1e-4, "{reach:?} does not sum to one");
            for n in 0..1000 {
                let part = reach.pick(n as f32 / 1000.0);
                assert!(weights[part.index()] > 0.0, "{reach:?} landed on {part:?}");
                landed[part.index()] = true;
            }
            assert!(weights[reach.pick(f32::NAN).index()] > 0.0);
            assert!(weights[reach.pick(7.0).index()] > 0.0);
        }
        assert!(landed.iter().all(|&l| l), "a part no blow can reach: {landed:?}");
        // ...and the seeded roll stays in range.
        for seed in 0..10_000u64 {
            let r = roll(seed);
            assert!((0.0..1.0).contains(&r), "roll({seed}) = {r}");
        }
    }

    #[test]
    fn nonsense_off_a_file_or_a_socket_is_not_contagious() {
        let mut injuries = Injuries::default();
        injuries.parts[Part::Head.index()].fracture.severity = 0.7;
        injuries.parts[Part::LeftArm.index()].cut.severity = f32::NAN;
        injuries.parts[Part::RightLeg.index()].burn.severity = 40.0;
        injuries.sanitize();
        assert!(!injuries.wound(Part::Head, Kind::Fracture).is_open());
        assert!(!injuries.wound(Part::LeftArm, Kind::Cut).is_open());
        assert_eq!(injuries.wound(Part::RightLeg, Kind::Burn).severity, 1.0);
        // ...and nonsense in, nothing out.
        injuries.inflict(Part::Torso, Kind::Cut, f32::NAN);
        injuries.take_blow(Blow::Claw, f32::INFINITY, 0.5);
        injuries.scorch(f32::NAN);
        assert_eq!(injuries.step(f32::NAN, false), Mending::default());
        assert!(!injuries.wound(Part::Torso, Kind::Cut).is_open());
    }

    #[test]
    fn a_small_change_is_not_worth_a_message_and_a_dressing_always_is() {
        let mut injuries = Injuries::default();
        injuries.inflict(Part::LeftArm, Kind::Cut, 0.5);
        let told = injuries;
        let mut drifted = told;
        drifted.parts[Part::LeftArm.index()].cut.severity = 0.49;
        assert!(!drifted.worth_reporting(&told));
        let mut dressed = told;
        dressed.treat(Part::LeftArm, BLOCK_BANDAGE).expect("fits");
        assert!(dressed.worth_reporting(&told));
        assert!(Injuries::default().worth_reporting(&told), "a wound closing was not worth saying");
    }

    #[test]
    fn a_bleeding_cut_shows_a_few_drops_a_second_and_never_a_fountain() {
        let mut injuries = Injuries::default();
        assert_eq!(injuries.drips_per_second(), 0.0, "a whole body leaked");

        // A bruise, a break and a burn open no skin.
        injuries.inflict(Part::Torso, Kind::Bruise, 1.0);
        injuries.inflict(Part::LeftLeg, Kind::Fracture, 1.0);
        injuries.inflict(Part::RightArm, Kind::Burn, 1.0);
        assert_eq!(injuries.drips_per_second(), 0.0, "a wound that is not a cut leaked");

        injuries.inflict(Part::LeftArm, Kind::Cut, 0.05);
        let scratch = injuries.drips_per_second();
        assert!(scratch > 0.0, "an open cut showed nothing");

        // Every part of the body cut to the bone is still a leak, not a spray.
        for part in Part::ALL {
            injuries.inflict(part, Kind::Cut, 1.0);
        }
        let worst = injuries.drips_per_second();
        assert!(worst >= scratch, "a deeper cut showed less than a scratch");
        assert!(worst <= MAX_DRIPS_PER_SECOND, "the worst bleeding shows {worst} drops a second");
        const { assert!(MAX_DRIPS_PER_SECOND < BLOW_DROPS as f32 / 4.0) };

        // ...and the bandage stops it showing, as it stops it costing.
        for part in Part::ALL {
            let _ = injuries.treat(part, BLOCK_BANDAGE);
        }
        assert_eq!(injuries.drips_per_second(), 0.0, "a dressed body leaked");
    }

    #[test]
    fn a_blow_that_cuts_sprays_and_a_blow_that_bruises_only_shows() {
        for blow in [Blow::Bite, Blow::Tusk, Blow::Claw, Blow::Edge] {
            assert_eq!(blow.drops(), BLOW_DROPS, "{blow:?}");
        }
        for blow in [Blow::Blunt, Blow::Crush] {
            assert!(blow.drops() > 0 && blow.drops() < BLOW_DROPS, "{blow:?}");
        }
    }
}
