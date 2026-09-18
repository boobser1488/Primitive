//! Warmth and water: the two meters that are about *where you are*
//! rather than about what just hit you.
//!
//! ## Why they are one module
//!
//! Hunger is a clock. It runs down at a rate that depends on what you
//! are doing and stops when you eat, and nothing about the world changes
//! it. Health is an accumulator: things happen and it goes down.
//!
//! Temperature and thirst are neither. They are the two places where the
//! *world* reaches into the player continuously -- a cold night, a fire,
//! a rainstorm, a desert at noon, a coat, a lake -- and they are coupled
//! to each other: being hot is most of what makes you thirsty, and being
//! wet is most of what makes you cold. Two modules would mean the
//! coupling lived in whichever one was written second.
//!
//! ## What is here and what is on the server
//!
//! Here: the *arithmetic*. How fast a body approaches the temperature
//! around it, what a coat is worth against a given chill, how fast a
//! working player loses water, what happens at the ends of both scales.
//! Pure functions and constants, no state.
//!
//! On the server: [`primitive_server::climate`], which works out what
//! the temperature around a particular player actually is -- the biome,
//! the hour, the weather, the fire two blocks away, the water they are
//! standing in, the roof over their head. That needs the world, and the
//! world is the server's.
//!
//! In the client: nothing but drawing. The client is told two numbers
//! and paints two gauges. It does not predict them, because a client
//! that decided its own temperature would be a client that is never
//! cold -- and unlike movement, there is no responsiveness argument for
//! predicting a meter that moves over minutes.
//!
//! ## The temperature scale
//!
//! Degrees, and they are meant to read as degrees Celsius, because a
//! number a player half-recognises is worth more than an abstract 0..1.
//! The band a body is comfortable in is [`COMFORT_LOW`] to
//! [`COMFORT_HIGH`]; outside it the body drifts toward the world and the
//! consequences start at [`CHILLED`] and [`OVERHEATED`].
//!
//! **The drift is the mechanic.** A player who walks into a snowfield
//! does not become cold; they *start* becoming cold, at a rate a coat
//! can slow and a fire can reverse, and they have the length of that
//! drift to do something about it. Instant state changes would make
//! temperature a room you are in rather than a thing that happens to
//! you.

// ---- tiredness ----
//
// **The fourth meter, and the argument for it is the night.** Hunger
// says go and find food, thirst says go and find water, warmth says go
// and find shelter -- and all three of them are answered by walking
// somewhere. Nothing in the game ever said *stop*. So a player's night
// was the same as their day with worse visibility, and the shelter they
// built was a box they stood in until the sun came back.
//
// Tiredness is the meter that is answered by lying down, and everything
// else about it follows from that being its only answer: it is slow, it
// is never lethal, and what it costs is the thing a player minds most
// -- their pace and their aim.

/// How long a body can go without sleep before it is finished, in
/// seconds of being awake.
///
/// Twenty in-game hours at the default day length, so a player who
/// sleeps at dusk is fresh at dawn and one who works two nights running
/// is not. Deliberately longer than a day: the mechanic must not be a
/// timer that forces everybody to bed at the same hour, it is a debt
/// that accumulates while you ignore it.
pub const WAKING_SECONDS: f32 = 20.0 * 60.0 * 60.0 / 24.0 * (15.0 / 60.0);

/// Fatigue, 0..1, past which a player is visibly tired.
///
/// Three quarters. The screen says so, the meter darkens, and this is
/// the warning -- the costs below start here rather than at the top, so
/// a player meets them with a quarter of the scale left to act on.
pub const TIRED_AT: f32 = 0.75;

/// How much of their speed a completely exhausted player keeps.
///
/// Four fifths. **Not less**, and the reason is the failure mode of
/// every stamina system: a mechanic that takes away a player's ability
/// to travel *when they are already in trouble* is a mechanic that
/// makes a bad situation unrecoverable rather than interesting. A fifth
/// slower is a walk home that takes noticeably longer, not a walk home
/// you cannot make.
pub const EXHAUSTED_SPEED: f32 = 0.8;

/// ...and how much of their healing.
///
/// A third. Sleep is when a body mends, so a body that never sleeps
/// mends badly -- and this is the cost that actually bites, because it
/// compounds with the diet rule (`food::diet_regen_factor`) rather than
/// replacing it.
pub const EXHAUSTED_REGEN: f32 = 1.0 / 3.0;

/// How much tiredness one second of sleep takes off, as a fraction of
/// the whole meter.
///
/// A full night is eight in-game hours -- five real minutes at the
/// default day length -- and it has to clear a full meter with time to
/// spare, or a player who slept properly would wake up tired and the
/// mechanic would read as broken.
pub const SLEEP_RECOVERY_PER_SECOND: f32 = 1.0 / 120.0;

/// How long a sleeper's screen takes to go dark, in seconds.
///
/// **One number for the two sides that have to agree about it.** The
/// client fades to black over this long (`logic::posture::Sleep`), and the
/// server will not wind the clock to dawn until every sleeper has been
/// asleep for longer (`NIGHT_PASSES_AFTER_SECONDS`). Written in two places
/// they would drift, and the drift is visible in exactly one way: the sun
/// jumping across a sky the player can still see.
///
/// A second and a half: long enough to read as closing your eyes rather
/// than as the renderer failing, short enough that nobody waits for it.
pub const FALLING_ASLEEP_SECONDS: f32 = 1.5;

/// How long everybody has to have been asleep before the night passes.
///
/// **The bug this is for.** The night used to pass on the first tick on
/// which everybody was in bed -- which in singleplayer is the tick after
/// the one player lies down -- and the same function stood them up at
/// dawn. A player reported it as it looked: "при сне игрок сразу встает и
/// не засыпает". They lay down and were standing in the morning fifty
/// milliseconds later, having watched the sun jump.
///
/// The fade and a second over it, because the server's clock and the
/// client's are not the same clock: the second is a round trip and a slow
/// frame, so the jump always lands behind a screen that is already black.
pub const NIGHT_PASSES_AFTER_SECONDS: f32 = FALLING_ASLEEP_SECONDS + 1.0;

/// What a place to lie down is worth.
///
/// Two grades and no more, because the decision is "have I built a bed
/// yet" and a third grade would be a number nobody can feel. A heap of
/// straw is most of a night; a bed is all of it and lets you sleep
/// through to dawn.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Rest {
    /// Dry grass on the floor. Takes the edge off and no more -- see
    /// `Rest::recovery`.
    Straw,
    /// A frame, boards and a hide.
    Bed,
}

impl Rest {
    /// What fraction of the tiredness a night here actually takes.
    ///
    /// Straw leaves a fifth of it behind, which is what makes the bed
    /// worth six planks and a hide without making the straw a mistake:
    /// a player on straw wakes up able to work, and one in a bed wakes
    /// up fresh.
    pub fn recovery(self) -> f32 {
        match self {
            Rest::Straw => 0.8,
            Rest::Bed => 1.0,
        }
    }

    /// What a block is to lie on, if it is anything.
    pub fn of(block: crate::types::BlockId) -> Option<Rest> {
        match crate::types::block_kind(block) {
            crate::types::BLOCK_STRAW_BED => Some(Rest::Straw),
            crate::types::BLOCK_BED => Some(Rest::Bed),
            _ => None,
        }
    }
}

/// How fast tiredness goes while sitting on something made for it.
///
/// **Sitting is not sleeping**, and this number is the whole of the
/// difference: a stool takes tiredness off about a sixth as fast as
/// sleep does and does not pass time, so resting at the fire between
/// jobs is worth doing and is never a substitute for the night. A
/// player who tried to live on stools would spend six times as long
/// sitting as they would sleeping, doing nothing, in real time -- which
/// is its own answer.
pub const SITTING_RECOVERY_PER_SECOND: f32 = SLEEP_RECOVERY_PER_SECOND / 6.0;

/// How fast tiredness goes sitting on this block: a chair a quarter as
/// fast as sleep, a stool a sixth, anything else not at all.
///
/// **The back is what the extra plank buys.** A chair that rested exactly
/// as a stool does would be a stool that points somewhere -- furniture
/// with one right answer, which is the cheaper one. Half again as fast
/// makes it the seat worth building in the room a player comes back to,
/// and still leaves sitting at a fifth of what a night does, so no chair
/// is a bed. Rejected: a chair that rests *slower* than a stool in
/// exchange for its facing, which nobody would build twice.
pub fn sitting_recovery(block: crate::types::BlockId) -> f32 {
    match crate::types::block_kind(block) {
        crate::types::BLOCK_CHAIR => SLEEP_RECOVERY_PER_SECOND / 4.0,
        crate::types::BLOCK_STOOL => SITTING_RECOVERY_PER_SECOND,
        _ => 0.0,
    }
}

// ---- a broken leg ----
//
// **The one injury in the game that is not a number off the health
// bar.** Falling has always cost health and nothing else, which makes a
// bad landing a thing you eat a haunch of meat about: ten seconds later
// the fall never happened. A drop that would break a leg in the world
// this game is set in should be a *fortnight*, and the mechanic that
// makes that bearable rather than cruel already exists -- sleep.
//
// So a hard landing breaks a leg; a broken leg is slow rather than
// deadly; and it mends in bed.
//
// **This note used to end "there is no splint, no bandage and no arm,
// because each of those is a second system and none of them changes the
// decision".** Then the rest of the body got wounds (`injury`), and each
// of those things turned out to change a decision after all: a splint is
// the question "set it here or limp home", a bandage is "stop the bleeding
// now or race", and an arm is "fight on at half strength". The break is one
// wound among the rest now and the numbers below are still its numbers --
// what changed is that it does not knit until it is splinted, and the bed
// is the second half of the answer rather than the whole of it.

/// How much fall damage counts as a break, in points of health.
///
/// Six, which is a fall of about nine blocks unloaded -- comfortably
/// past the four the body shrugs off (`SAFE_FALL_BLOCKS`) and short of
/// the drop that kills outright. Chosen against what a player is doing
/// when it happens: stepping off a ledge in a mine, misjudging a cliff
/// path, being knocked off a roof. All three are mistakes with a story;
/// tripping over a kerb is not, and does not break anything.
pub const FRACTURE_DAMAGE: f32 = 6.0;

/// How long a break takes to mend while awake, in seconds.
///
/// Half an hour of play -- two in-game days at the default clock. That
/// is deliberately far longer than any other cost in the game, because
/// it is the only one meant to change what a player *does* rather than
/// what they watch: a hunter with a broken leg goes home.
pub const FRACTURE_SECONDS: f32 = 1800.0;

/// How much faster it mends in a bed.
///
/// Six times, so a full night (eight in-game hours) takes most of a
/// break off and two nights clear it. Sleep is the answer to this the
/// way food is the answer to hunger -- and it is the reason a bed is
/// worth building before the first cliff rather than after it.
pub const FRACTURE_SLEEP_FACTOR: f32 = 6.0;

/// How much of their speed a player with a broken leg keeps.
///
/// Just over half. Slower than exhaustion (`EXHAUSTED_SPEED`) by a
/// wide margin, because a limp is not tiredness -- and still fast
/// enough to walk home, which is the one thing this must never take
/// away: an injury that leaves a player unable to reach their bed is a
/// world they have to abandon.
pub const FRACTURE_SPEED: f32 = 0.55;

/// The temperature a body sits at when nothing is pulling on it.
///
/// Not 37: this is skin and extremities, the part of a person that
/// actually gets cold, and the number that reads correctly against an
/// ambient scale in the same units.
pub const NEUTRAL_C: f32 = 30.0;

/// The band inside which nothing at all happens.
pub const COMFORT_LOW: f32 = 26.0;
pub const COMFORT_HIGH: f32 = 34.0;

/// Below this a player is visibly cold: the screen says so, the shivering
/// starts, and hunger runs faster.
pub const CHILLED: f32 = 22.0;
/// ...and above this, the other end.
pub const OVERHEATED: f32 = 38.0;

/// Below this, cold does damage.
pub const FREEZING: f32 = 12.0;
/// ...and above this, heat does.
pub const SCALDING: f32 = 45.0;

/// Damage per second at the very ends of the scale.
///
/// Ramped in from `FREEZING`/`SCALDING` rather than switched on, so the
/// first point of damage arrives as a warning and the rate only becomes
/// serious well past it. A player who has ignored a blue screen, a
/// shiver and a rising hunger drain has had three warnings; the fourth
/// is not owed to them.
pub const EXPOSURE_PER_SECOND: f32 = 1.2;

/// How many degrees past the threshold count as "as bad as it gets".
pub const EXPOSURE_RAMP: f32 = 12.0;

/// How fast an unclothed body moves toward the temperature around it, as
/// a fraction of the gap per second.
///
/// A time constant of about five minutes, and the number was chosen by
/// what it makes true rather than by feel: **ten seconds in freezing air
/// must cost nothing and two minutes must cost something.** That is the
/// whole legibility of the mechanic -- crossing a snowfield is free,
/// standing in one is not -- and the two tests below are the statement
/// of it.
///
/// Slower than it looks, deliberately. A meter that visibly moves while
/// a player watches it is a meter they will stand and watch; one that
/// moves over minutes is one they notice having moved, which is the
/// difference between a temperature system and a stamina bar.
pub const BASE_ADJUST_PER_SECOND: f32 = 0.0035;

/// The most insulation can slow that drift down to.
///
/// Clothing does not stop heat loss, it slows it. A floor here is what
/// keeps a fully clothed player in a blizzard on a clock rather than
/// immune -- and it is why shelter and fire exist as answers rather than
/// being redundant with a good coat.
pub const MIN_ADJUST_FRACTION: f32 = 0.18;

/// How many degrees of insulation halve the drift rate.
///
/// So a full leather set (about eight degrees, weighted) roughly halves
/// how fast the cold gets in, and there is no amount of clothing that
/// gets to zero.
pub const INSULATION_HALVING: f32 = 8.0;

/// How much of the *gap* a coat closes outright.
///
/// Distinct from slowing the drift: insulation both slows the approach
/// and moves where the approach is heading, because a coat is warm
/// inside.
pub const INSULATION_OFFSET_SCALE: f32 = 0.75;

/// How far above the air a living body sits with nothing on.
///
/// **Without this the model was of a corpse, and the whole system read
/// as broken.** `felt_ambient` returned the air temperature when
/// insulation was zero, so a player with no clothes drifted to exactly
/// whatever the world said -- and the world says something much colder
/// than this body's comfortable band almost everywhere. Sampled from
/// the real generator, seed 1337, 1764 columns at sea level and noon,
/// which is the warmest hour there is:
///
/// ```text
/// min -7.2   p10 3.7   median 15.3   p90 27.3   max 41.8
/// the spawn column (0,0): 16.0
/// the comfortable band:   26.0 .. 34.0
/// ```
///
/// Ninety per cent of the world is below the band at midday. A player
/// woke up in a meadow, at noon, in fair weather, and was *cold* --
/// from the first minute, everywhere, with no clothing yet invented.
/// And because they were already pinned at the cold end, the two things
/// that are supposed to answer being cold could not be felt: a fire
/// moved a number that was already at the floor, and stepping into a
/// lake changed nothing that was not already true. Both mechanics
/// worked and neither was legible, which is what "the temperature is
/// broken" meant.
///
/// A body burns food and gives off heat; it settles well above the air
/// around it, and that offset is what clothing *adds to* rather than
/// replaces. Twelve degrees is chosen so that the world reads the way
/// it was designed to: the spawn meadow at noon is comfortable, its
/// night is cold, a storm is cold, the cold tenth of the map is cold
/// without clothes, and a desert is still hot. Measured against those
/// five cases rather than picked -- see
/// `the_world_a_player_wakes_up_in_is_survivable_with_no_clothes`.
pub const METABOLIC_LIFT_C: f32 = 12.0;

/// How much of that a soaking takes away.
///
/// **Water carries heat off a body far faster than air does**, which is
/// why swimming in a lake is cold in a way that standing in the same
/// air is not. The lift is the body's own heat winning against what the
/// world takes; in water the world takes it faster than it can be made,
/// so half of the lift is gone.
///
/// Applied through `wetness` rather than through a separate "in water"
/// flag, because the two are the same fact and there is already one
/// input carrying it: `climate` sets wetness from standing in water
/// *and* from rain, and rain chilling a player who has no shelter is
/// the same mechanic as a lake doing it. It is also what makes drying
/// off by a fire worth doing.
pub const WET_METABOLIC_LOSS: f32 = 0.5;

/// ...and what the same coat does in the heat, which is the opposite.
///
/// **Clothing is not two-sided and this is the asymmetry that says so.**
/// A body makes heat all the time; in the cold a coat keeps that heat
/// where it belongs, and in the sun it keeps it where it is not wanted.
/// So insulation *raises* the temperature a hot world pulls a player
/// toward rather than shielding them from it -- which is why the answer
/// to a desert is to take the armour off, and why a full iron set is a
/// thing you carry through one rather than wear across it.
///
/// A fifth of the offset a coat gets in the cold: a coat is much better
/// at keeping heat in than at cooking you, which is also true.
///
/// That is the coat against hot *air*. Against the sun a garment can do
/// a second thing -- stand between the sunlight and the skin -- and only
/// loose cloth does it: that half is the cloth's `shade`
/// (`equipment::Garment::shade`), read by [`sun_on_skin`].
pub const HEAT_TRAP_SCALE: f32 = 0.15;

/// How much faster a body moves toward a hot world than toward a cold one.
///
/// **Three times, and the reason is the length of a day.** The sun is
/// high for about a third of one -- five real minutes at the default
/// clock -- and the bare time constant on the cold side is nearly five
/// minutes, so a body at that pace never got as hot as the noon was.
/// Even at *twice* it, simulated with the sun as built
/// (`climate::SUN_C`), a bare body under a 38-degree desert sky peaked
/// at 45.7 in the middle of the afternoon and lost five points of health
/// over the whole day: a temperature system that notices the desert
/// after it has stopped being hot. At three times, measured through the
/// server's own sample (`climate`'s desert tests), the same body is Warm
/// at 09:47 and in heatstroke by 11:26, while a thirty-second dash
/// across the open moves the skin by six degrees and leaves it
/// comfortable -- so crossing the sun stays free and standing in it
/// does not, which is the sentence the cold side's rate was chosen for
/// too.
///
/// It is also what a body does. Heat arrives through the skin directly
/// -- the sun lands on it -- and leaves by sweat, and both are quicker
/// than cold creeping in through a coat, which is what the slow rate
/// models. **Taken both ways, deliberately:** the shade is felt as fast
/// as the sun was. Quickening only in the sun was considered and loses
/// on exactly that: a player who reached a tree in time went on cooking
/// in its shade at the slow rate, a minute of damage after arriving,
/// and that reads as the shade not working.
pub const HOT_ADJUST_FACTOR: f32 = 3.0;

/// How much faster a body in water moves toward the water's temperature.
///
/// **Water carries heat off a body many times faster than air**, and
/// `climate` has always said swimming is the fastest way to get cold
/// while the body moved at one rate in a lake and in the air over it --
/// so a swim changed where the body was heading and not how soon it got
/// there. In the cold that was merely untrue. In the heat it is the
/// whole of a river's value: the answer to a plain at noon is a dip
/// before crossing it, and a dip that takes five minutes to work is not
/// an answer to anything.
///
/// Three times. Measured with the body's own step: thirty seconds in a
/// river takes a body a degree off heatstroke down by seven degrees,
/// more than a minute of open desert sun to earn back
/// (`a_dip_in_the_river_buys_time_on_the_open_plain`; at the air's rate
/// the same dip bought under forty seconds), while a bare swimmer in a
/// temperate lake reaches Cold in under two minutes where it used to
/// take over five. Not more: at six, a swim across a winter
/// river froze a player in about half a minute, and the cold side's
/// clock is a legibility promise (see [`BASE_ADJUST_PER_SECOND`]) that
/// this may bend and must not break.
pub const WATER_ADJUST_FACTOR: f32 = 3.0;

/// The most a soaking takes off a hot body's target, in degrees.
///
/// **Sweat is how a body survives heat, and water on the skin is sweat
/// it did not have to make.** On the warm side of neutral a wet body
/// heads somewhere cooler than a dry one: by up to eight degrees, and
/// never below [`NEUTRAL_C`] -- the water takes away heat *above*
/// neutral, it does not turn a hot day cold. Eight because that is one
/// band on the gauge: a desert noon in the shade (38) goes from Warm to
/// comfortable, an open savanna (41) from Warm to the top of
/// comfortable, and the open desert (52) from killing to Warm.
///
/// The cooling ramps in from nothing at neutral rather than switching on
/// at [`COMFORT_HIGH`]. Switched, a soaked player at 34.1 degrees would
/// be heading for 26 while one at 33.9 headed for 33.9 -- a step of
/// eight degrees across a line nobody can see.
///
/// Short-lived by nature, and that is the other half of the decision:
/// hot dry air dries a player quickly (`climate::HEAT_DRYING`), so the
/// lasting good of a river is the dip itself ([`WATER_ADJUST_FACTOR`])
/// and this is the minute after it.
pub const WET_COOLING_C: f32 = 8.0;

// ---- water ----

/// A full skin of water, in the units the drain rates are per-second in.
///
/// Two thousand, so that idle drain of one unit a second is a bit over
/// half an hour of doing nothing -- longer than a day/night cycle at the
/// default day length, which is the right relationship: thirst should be
/// something a player deals with once a day rather than a timer they
/// watch.
pub const MAX_HYDRATION: f32 = 2000.0;

/// What kind of water this is, and therefore what drinking it costs.
///
/// **Three kinds, because there are three answers a player needs.**
/// Running water is what you want, standing water is what you settle
/// for, and the sea is what you must not: a mouthful of seawater takes
/// more out of a body than it puts in, which is the one fact about
/// drinking at sea that everybody knows and no game ever models.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Water {
    /// A river, a stream, rain off a roof: moving, and safe.
    Fresh,
    /// A pond, a puddle, the still middle of a lake. Drinkable, and it
    /// will make you ill: this is where the "не пей из копытца" of every
    /// language's version of the story comes from.
    Standing,
    /// The sea. Salt.
    Salt,
}

/// How long a mouthful of bad water keeps taking, in seconds.
///
/// Ninety for a pond and three minutes for the sea. Long enough to be a
/// thing that happened to you rather than a number that flickered, and
/// short enough to be survivable if you were near full health -- the
/// mistake is meant to be memorable, not fatal on its own.
pub const SICKNESS_SECONDS: (f32, f32) = (90.0, 180.0);

/// The chance a mouthful of bad water makes a player ill at all.
///
/// **Forty in a hundred**, the player's number ("сделай шанс отравится водой
/// 40 процентов"). It was certain, and certainty made a pond a rule rather
/// than a risk: nobody ever drank one twice, so the pond was simply not
/// water. At two in five a thirsty player far from a river has a real choice
/// -- most mouthfuls are fine, and the one that is not costs an afternoon of
/// healing -- and "drink from what runs" is still the answer that never
/// loses. The sea's salt is not a chance: that is `hydration`, and it is
/// charged every time.
pub const WATER_ILLNESS_CHANCE: f32 = 0.4;

/// How long what was swallowed takes to make a player ill, in seconds.
///
/// **Two minutes of nothing, and then the illness.** It used to start on the
/// mouthful, which taught the wrong lesson -- the game reading the rules
/// out -- and made the cause obvious in a way a stomach never is. With a
/// delay the player has walked on, eaten something else, and has to *think*
/// back to the pond; and the flashing health that follows is the moment
/// they find out. Food illness waits the same, for the same reason.
pub const DIGESTION_SECONDS: f32 = 120.0;

/// Health a second while the sickness lasts.
///
/// A twentieth: ninety seconds of a pond is four and a half points,
/// which is a quarter of a healthy player and hurts more the worse a
/// state they were already in. Slow on purpose -- this is a stomach,
/// not a wound, and what it mostly costs is the *healing* it prevents
/// (see `survival::Vitals::regenerate`).
pub const SICKNESS_PER_SECOND: f32 = 0.05;

impl Water {
    /// How long being ill from this lasts, in seconds. Zero for water
    /// that does not make you ill.
    pub fn sickness_seconds(self) -> f32 {
        match self {
            Water::Fresh => 0.0,
            Water::Standing => SICKNESS_SECONDS.0,
            Water::Salt => SICKNESS_SECONDS.1,
        }
    }

    /// What a mouthful is worth. **Salt water is worth less than
    /// nothing**: the body spends more water passing the salt than the
    /// mouthful carried, which is why drinking the sea is the one way
    /// to make yourself thirstier.
    pub fn hydration(self, mouthful: f32) -> f32 {
        match self {
            Water::Fresh => mouthful,
            // A pond still quenches: what it costs comes later.
            Water::Standing => mouthful,
            Water::Salt => -mouthful * 0.5,
        }
    }
}

/// Units of water a second at rest, in comfortable weather.
pub const IDLE_THIRST_PER_SECOND: f32 = 1.0;
/// ...walking.
pub const WALK_THIRST_PER_SECOND: f32 = 1.6;
/// ...sprinting, which is the expensive one.
pub const SPRINT_THIRST_PER_SECOND: f32 = 3.4;
/// ...and working at a block, which is the other one.
pub const WORK_THIRST_PER_SECOND: f32 = 2.6;

/// How much faster water goes at the hot end of the scale.
///
/// The coupling between the two meters: a desert at noon roughly doubles
/// the drain, which is what makes carrying a jug the difference between
/// crossing one and turning back. Cold does *not* slow it down by the
/// same rule -- a shivering body burns water too -- so the multiplier
/// bottoms out at one.
pub const HEAT_THIRST_MULTIPLIER: f32 = 1.6;

/// Below this fraction of a full skin, the player is thirsty enough for
/// it to show and for it to start costing them.
pub const PARCHED: f32 = 0.25;

/// Damage per second on an empty skin.
///
/// Faster than starving, because dehydration is: a person survives weeks
/// without food and days without water, and the game's proportions
/// should not say the opposite.
pub const DEHYDRATION_PER_SECOND: f32 = 0.7;

/// How much of a full skin one drink from a jug restores.
pub const JUG_HYDRATION: f32 = 900.0;
/// ...and one mouthful drunk straight from open water.
///
/// Less than a jug per gesture, so that carrying water is worth doing
/// even standing beside a lake -- and so a player at a river is not
/// simply topped up forever in one click.
pub const DRINK_HYDRATION: f32 = 420.0;

/// What the player is doing, for the purposes of the water bill.
///
/// The same shape as `survival::Effort`, deliberately: the tick loop
/// already works out whether somebody is sprinting or mining and should
/// not have to work it out twice in two vocabularies.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct Exertion {
    pub moving: bool,
    pub sprinting: bool,
    pub working: bool,
}

impl Exertion {
    pub const RESTING: Exertion = Exertion {
        moving: false,
        sprinting: false,
        working: false,
    };

    /// Units of water per second, before the heat multiplier.
    pub fn thirst_per_second(self) -> f32 {
        let mut rate = IDLE_THIRST_PER_SECOND;
        if self.moving {
            rate = rate.max(WALK_THIRST_PER_SECOND);
        }
        if self.sprinting {
            rate = rate.max(SPRINT_THIRST_PER_SECOND);
        }
        if self.working {
            rate = rate.max(WORK_THIRST_PER_SECOND);
        }
        rate
    }
}

/// How fast water goes at a given body temperature.
///
/// One at and below comfort, rising to [`HEAT_THIRST_MULTIPLIER`] at
/// [`SCALDING`]. Smooth, because a step in a drain rate is a step a
/// player cannot see the cause of.
pub fn thirst_multiplier(body_c: f32) -> f32 {
    if !body_c.is_finite() || body_c <= COMFORT_HIGH {
        return 1.0;
    }
    let t = ((body_c - COMFORT_HIGH) / (SCALDING - COMFORT_HIGH)).clamp(0.0, 1.0);
    1.0 + (HEAT_THIRST_MULTIPLIER - 1.0) * t
}

/// The temperature a body is actually heading toward, given what is
/// around it and what it is wearing.
///
/// Four things, in order:
///
/// 0. **A living body sits above the air it is in.** See
///    [`METABOLIC_LIFT_C`]: without this term the model described a
///    corpse, and a player in a meadow at noon was cold.
/// 1. **Wet clothing does not insulate.** `wetness` is 0..1 and it is
///    applied to the insulation rather than to the result, because that
///    is where it belongs: a soaked coat is not a colder world, it is a
///    coat that has stopped working. This is what makes rain dangerous
///    in a way that a few degrees of ambient never would be, and what
///    makes a metal set -- which sheds completely -- the right thing to
///    be caught out in.
/// 2. **A coat only warms.** The offset is one-sided: clothing pulls the
///    target up out of the cold and never down out of the heat. A player
///    in full plate in a desert is exactly as hot as one in nothing, and
///    then hotter, because of what the bulk does to the effort they
///    spend.
/// 3. **It never overshoots.** A coat in a chill of five degrees cannot
///    heat a body past neutral; the most it does is stop the cold.
/// 4. **Water cools a hot body.** On the warm side of neutral a soaking
///    takes heat off, up to [`WET_COOLING_C`] and never below neutral --
///    the same water that is the danger in the cold is the relief in the
///    heat, which is what a river in a savanna is for.
pub fn felt_ambient(ambient_c: f32, insulation: f32, wetness: f32) -> f32 {
    if !ambient_c.is_finite() {
        return NEUTRAL_C;
    }
    let dry = (1.0 - wetness.clamp(0.0, 1.0)).max(0.0);
    let effective = insulation.max(0.0) * dry;
    // Water on the skin, on the warm side of neutral. The amount and the
    // ramp are argued on `WET_COOLING_C`; the `min` is the ramp, and it
    // is what keeps a soaked body out of the cold band however hot the
    // day.
    let wet = if wetness.is_finite() { wetness.clamp(0.0, 1.0) } else { 0.0 };
    let evaporation = if ambient_c > NEUTRAL_C {
        wet * (ambient_c - NEUTRAL_C).min(WET_COOLING_C)
    } else {
        0.0
    };
    if ambient_c > COMFORT_HIGH {
        // The hot side: the coat is now on the wrong side of the body's
        // own heat. See `HEAT_TRAP_SCALE`.
        return ambient_c + effective * HEAT_TRAP_SCALE - evaporation;
    }
    if ambient_c >= NEUTRAL_C {
        return ambient_c - evaporation;
    }
    // The body's own heat first, then the coat's help with keeping it.
    // Two terms rather than one because they are two different things:
    // the first is there with nothing on and is taken away by water,
    // the second is what is worn and is taken away by the *same* water.
    let metabolic = METABOLIC_LIFT_C * (1.0 - WET_METABOLIC_LOSS * wetness.clamp(0.0, 1.0));
    let lift = metabolic + effective * INSULATION_OFFSET_SCALE;
    // Never past neutral: a body in the cold closes the gap, it does
    // not invent heat on the far side of it.
    (ambient_c + lift).min(NEUTRAL_C)
}

/// ...and the same, for a set that also keeps the sun off.
///
/// **What a loose, light garment does in hot air is not insulation run
/// backwards.** Insulation keeps a body's own heat in, and on the hot side
/// that is always bad (see [`HEAT_TRAP_SCALE`]). A thin cloth hanging off
/// the shoulders does something else: it stands between the skin and the
/// sun and lets sweat go -- which is why the people who live in deserts
/// are covered head to foot and nobody there wears a fleece. `shade` is
/// that, in degrees taken off hot air, already weighted by slot
/// (`equipment::Worn::shade`).
///
/// Three rules, each against a particular wrong answer:
///
/// 1. **Only in hot air**, past [`COMFORT_HIGH`]. There is no sun to keep
///    off on a cold night, and a shirt that cooled a shivering player
///    would be a shirt nobody should carry north.
/// 2. **Never below the comfortable top.** Shade makes hot air less hot;
///    it does not make a desert pleasant. Without the floor, a full set in
///    air a degree past the band would pull a body *down through* it -- a
///    step a player feels at exactly the temperature where nothing is
///    supposed to happen.
/// 3. **Wet or dry alike.** A soaked shirt in the heat is cooler, and
///    [`felt_ambient`] says so on its own (see [`WET_COOLING_C`]); the
///    shade is not taken away on top of that.
///
/// Rejected: letting a light garment's insulation go negative. That is
/// one number doing two jobs, and the cold side would read it too -- a
/// cotton shirt would make a winter night colder than bare skin.
pub fn felt_ambient_shaded(ambient_c: f32, insulation: f32, shade: f32, wetness: f32) -> f32 {
    let felt = felt_ambient(ambient_c, insulation, wetness);
    if !ambient_c.is_finite() || ambient_c <= COMFORT_HIGH || !shade.is_finite() {
        return felt;
    }
    (felt - shade.max(0.0)).max(COMFORT_HIGH.min(felt))
}

// ---- the sun ----
//
// **The sun is a number of its own, next to the air, and never folded
// into it.** Two reasons, each a bug the folding would have been. A
// sleeve can stand between the sun and the skin and cannot stand between
// the air and the skin, so a sun already added to the air could not be
// shaded by anything worn. And `climate` reports the air to more than
// bodies -- crops grow by it (`growth`), hides cure by it, meat keeps by
// it -- and a field of cotton does not wear a shirt; a sun added to the
// air would also have made the season move a hot place's temperature by
// more than a cold one's, which is the one thing
// `the_season_moves_every_temperature_by_the_same_amount` exists to
// forbid.

/// How many degrees of cloth's shade keep all the sun off the skin.
///
/// **One notion of shade, not two.** Cloth already carries a number for
/// what it does in hot air (`equipment::Garment::shade`), and a separate
/// "fraction of the sun kept off" beside it would be a second opinion
/// about the same shirt, free to disagree with the first. So the sun
/// reads the same number: a set keeps off as much of the sun as its
/// weighted shade is of this. Seven is the cloth cap's own figure, so a
/// hat keeps the sun off a head entirely -- the oldest answer to the
/// sun there is -- and a full cloth set, whose weighted shade is a
/// little under five, keeps off seven tenths of it.
///
/// Leather, wool, fur and metal cast no shade and so keep none of the
/// sun off, which is the cloth rows' own ruling carried one step further
/// (see the note on them in `equipment::garment`): a hide does not
/// breathe, and what it keeps off it keeps in under it. In the sun they
/// are bare skin plus what they trap ([`HEAT_TRAP_SCALE`]).
///
/// Rejected: a sun-transmission figure per garment, derived from
/// insulation so that a light hide kept some sun off and a fleece held
/// more of it in. It gave leather a couple of degrees of shade the cloth
/// rows had just decided it does not have, and it needed metal filed
/// separately by hand, because metal has the least insulation of
/// anything and would have come out the best thing to wear in a desert.
pub const SHADE_KEEPS_OFF_ALL_THE_SUN_C: f32 = 7.0;

/// What the world is doing to one body right now, in the terms the body
/// maths needs. See the note above on why the sun is apart from the air.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Exposure {
    /// The air, in degrees: `climate::Ambient::temperature_c`.
    pub air_c: f32,
    /// Degrees the sun adds on bare skin here. Zero at night, in rain,
    /// in water, under a roof and under a canopy -- `climate::SUN_C`.
    pub sun_c: f32,
    /// Whether the body is in water, which moves it faster -- see
    /// [`WATER_ADJUST_FACTOR`].
    pub in_water: bool,
}

impl Exposure {
    /// Air and nothing else: in the shade, on dry land.
    pub fn air(air_c: f32) -> Exposure {
        Exposure {
            air_c,
            sun_c: 0.0,
            in_water: false,
        }
    }
}

/// Degrees of sun that reach the skin through what is worn.
///
/// `shade` is `equipment::Worn::shade`. A nonsense sun is no sun and a
/// nonsense shade is no shade: the first errs toward a player who is not
/// hurt by a bad number, the second toward one who is not protected by
/// one, and neither multiplies a `NaN` into a body.
pub fn sun_on_skin(sun_c: f32, shade: f32) -> f32 {
    if !sun_c.is_finite() {
        return 0.0;
    }
    let kept_off = if shade.is_finite() {
        (shade / SHADE_KEEPS_OFF_ALL_THE_SUN_C).clamp(0.0, 1.0)
    } else {
        0.0
    };
    sun_c.max(0.0) * (1.0 - kept_off)
}

/// The temperature a body is heading toward, all of it at once: the air,
/// the sun through what is worn, the coat, the shade and the water.
///
/// The sun that gets through is *added to the air* before anything else
/// is asked, because on skin that is what it is -- radiant heat the body
/// has to shed like the air's -- and because it lets every rule
/// [`felt_ambient`] and [`felt_ambient_shaded`] already have (the coat
/// trapping heat past the comfortable band, the water, the shade's floor)
/// apply to a sunny day without a second copy of any of them.
pub fn felt_under_sky(exposure: Exposure, insulation: f32, shade: f32, wetness: f32) -> f32 {
    let air = exposure.air_c + sun_on_skin(exposure.sun_c, shade);
    felt_ambient_shaded(air, insulation, shade, wetness)
}

/// ...and how fast it gets there: [`adjust_rate`] against the air the sun
/// makes, quickened in water (see [`WATER_ADJUST_FACTOR`]).
pub fn adjust_rate_under_sky(exposure: Exposure, insulation: f32, shade: f32, wetness: f32) -> f32 {
    let air = exposure.air_c + sun_on_skin(exposure.sun_c, shade);
    let rate = adjust_rate(insulation, wetness, air);
    if exposure.in_water {
        rate * WATER_ADJUST_FACTOR
    } else {
        rate
    }
}

/// How fast a body closes the gap to the world, per second.
///
/// Slowed by insulation, floored so it is never zero -- see
/// [`MIN_ADJUST_FRACTION`].
///
/// **Only in the cold.** A coat that also slowed a player heating up
/// would be armour against a desert, and then the sensible thing to do
/// in one would be to put more on. On the hot side the rate is the bare
/// one -- quickened, see [`HOT_ADJUST_FACTOR`] -- and the *target* is
/// what the clothing moves, which is the asymmetry [`HEAT_TRAP_SCALE`]
/// exists for.
pub fn adjust_rate(insulation: f32, wetness: f32, ambient_c: f32) -> f32 {
    if !ambient_c.is_finite() {
        return BASE_ADJUST_PER_SECOND;
    }
    if ambient_c > COMFORT_HIGH {
        return BASE_ADJUST_PER_SECOND * HOT_ADJUST_FACTOR;
    }
    let dry = (1.0 - wetness.clamp(0.0, 1.0)).max(0.0);
    let effective = insulation.max(0.0) * dry;
    let slowdown = 1.0 / (1.0 + effective / INSULATION_HALVING);
    BASE_ADJUST_PER_SECOND * slowdown.max(MIN_ADJUST_FRACTION)
}

/// One step of the body's temperature toward the world's.
///
/// Exponential rather than linear, so the approach has no arrival: a
/// body never *reaches* the ambient temperature, it gets asymptotically
/// close, which is both what happens and what keeps the meter from
/// pinning at an end and staying there while the player walks away.
///
/// `dt` is clamped, because a tick that took a second -- a hitch, a
/// breakpoint, a laptop lid -- must not teleport somebody into
/// hypothermia.
pub fn step_temperature(body_c: f32, target_c: f32, rate: f32, dt: f32) -> f32 {
    if !dt.is_finite() || dt <= 0.0 {
        return body_c;
    }
    let dt = dt.min(MAX_STEP_SECONDS);
    let k = (-rate.max(0.0) * dt).exp();
    let next = target_c + (body_c - target_c) * k;
    if next.is_finite() {
        next.clamp(MIN_BODY_C, MAX_BODY_C)
    } else {
        body_c
    }
}

/// The longest single step the temperature maths will take.
pub const MAX_STEP_SECONDS: f32 = 0.5;

/// The ends of the body scale. Wide enough that neither end is reachable
/// in ordinary play, narrow enough that no arithmetic runs away.
pub const MIN_BODY_C: f32 = -20.0;
pub const MAX_BODY_C: f32 = 70.0;

/// Damage per second from being too cold or too hot, at a given body
/// temperature.
///
/// Zero inside the survivable band, then a linear ramp. Both ends use
/// the same shape so neither is a special case, and the ramp is what
/// makes the first damage a warning rather than a verdict.
pub fn exposure_damage_per_second(body_c: f32) -> f32 {
    if !body_c.is_finite() {
        return 0.0;
    }
    let past = if body_c < FREEZING {
        FREEZING - body_c
    } else if body_c > SCALDING {
        body_c - SCALDING
    } else {
        return 0.0;
    };
    EXPOSURE_PER_SECOND * (past / EXPOSURE_RAMP).clamp(0.05, 1.0)
}

/// What being cold does to hunger.
///
/// Shivering is work, and it is the *first* thing a player notices about
/// being cold -- before any damage, their food goes faster. One and a
/// half times at the chilled threshold rising to two and a half at
/// freezing, which is enough to turn a cold night into a reason to build
/// a fire without ever showing a damage number.
pub fn shiver_hunger_multiplier(body_c: f32) -> f32 {
    if !body_c.is_finite() || body_c >= COMFORT_LOW {
        return 1.0;
    }
    let t = ((COMFORT_LOW - body_c) / (COMFORT_LOW - FREEZING)).clamp(0.0, 1.0);
    1.0 + 1.5 * t
}

/// How the player is doing, in the one word the HUD needs.
///
/// The client draws a gauge from the raw number; this is what the
/// *message* on screen says, and it is derived on the server so that the
/// thresholds cannot drift between the two.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum Comfort {
    Freezing,
    Cold,
    Comfortable,
    Warm,
    Scorching,
}

impl Comfort {
    pub fn of(body_c: f32) -> Comfort {
        if !body_c.is_finite() {
            return Comfort::Comfortable;
        }
        if body_c < FREEZING {
            Comfort::Freezing
        } else if body_c < CHILLED {
            Comfort::Cold
        } else if body_c > SCALDING {
            Comfort::Scorching
        } else if body_c > OVERHEATED {
            Comfort::Warm
        } else {
            Comfort::Comfortable
        }
    }

    /// Whether this state is worth drawing anything for at all.
    ///
    /// The HUD keeps the temperature gauge hidden while a player is
    /// comfortable, on the same rule the breath meter follows: a bar
    /// that is always on screen and always full is a bar nobody reads.
    pub fn is_notable(self) -> bool {
        !matches!(self, Comfort::Comfortable)
    }

    pub fn name(self) -> &'static str {
        match self {
            Comfort::Freezing => "freezing",
            Comfort::Cold => "cold",
            Comfort::Comfortable => "comfortable",
            Comfort::Warm => "warm",
            Comfort::Scorching => "scorching",
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_chair_rests_you_faster_than_a_stool_and_no_seat_rests_you_like_a_bed() {
        // The chair's reason to exist over the stool it is built from. If
        // the two rested alike, the cheaper one would be the only answer;
        // if a chair rested like a bed, nobody would build the bed.
        use crate::types::{faced, Facing, BLOCK_CHAIR, BLOCK_STONE, BLOCK_STOOL};
        let chair = sitting_recovery(faced(BLOCK_CHAIR, Facing::West));
        let stool = sitting_recovery(BLOCK_STOOL);
        assert!(chair > stool, "a chair rests {chair} a second and a stool {stool}");
        assert!(chair < SLEEP_RECOVERY_PER_SECOND / 2.0, "a chair rests like a bed");
        assert_eq!(stool, SITTING_RECOVERY_PER_SECOND);
        assert_eq!(sitting_recovery(BLOCK_STONE), 0.0, "a rock is a seat");
    }

    /// Runs `seconds` of game time at 20 Hz through the temperature
    /// step, and answers where the body ended up.
    fn soak(mut body: f32, ambient: f32, insulation: f32, wetness: f32, seconds: f32) -> f32 {
        let dt = 1.0 / 20.0;
        let rate = adjust_rate(insulation, wetness, ambient);
        let target = felt_ambient(ambient, insulation, wetness);
        let mut elapsed = 0.0;
        while elapsed < seconds {
            body = step_temperature(body, target, rate, dt);
            elapsed += dt;
        }
        body
    }

    #[test]
    fn a_comfortable_world_leaves_a_body_alone() {
        let after = soak(NEUTRAL_C, NEUTRAL_C, 0.0, 0.0, 600.0);
        assert!((after - NEUTRAL_C).abs() < 0.01);
        assert_eq!(Comfort::of(after), Comfort::Comfortable);
        assert_eq!(exposure_damage_per_second(after), 0.0);
    }

    #[test]
    fn the_cold_arrives_gradually_rather_than_all_at_once() {
        // The whole claim of the mechanic: walking into a snowfield is
        // free and standing in one is not. Ten seconds must leave a
        // player comfortable; three minutes must not.
        //
        // **Two minutes, until [`METABOLIC_LIFT_C`] arrived.** Freezing
        // air used to pull a bare body all the way down to zero, and it
        // now pulls it to twelve -- the gap is smaller, so closing
        // enough of it to feel takes longer. The claim is unchanged and
        // its clock moved: the number here is the new measurement, not
        // a loosened threshold. Anything that makes the cold arrive
        // *slower* than this is a regression and this is what catches
        // it.
        let ambient = 0.0;
        let brief = soak(NEUTRAL_C, ambient, 0.0, 0.0, 10.0);
        assert_eq!(
            Comfort::of(brief),
            Comfort::Comfortable,
            "crossing a cold patch cost something"
        );
        let long = soak(NEUTRAL_C, ambient, 0.0, 0.0, 180.0);
        assert!(
            matches!(Comfort::of(long), Comfort::Cold | Comfort::Freezing),
            "three minutes in freezing air did nothing: {long}"
        );
    }

    /// The world a player wakes up in is survivable with nothing on.
    ///
    /// **It was not, and that is what "the temperature is broken"
    /// meant.** Every number below is the real generator's, seed 1337,
    /// sampled at sea level: the spawn column is 16 degrees at *noon*,
    /// the median column is 15.3, and nine columns in ten are below the
    /// comfortable band at the warmest hour of the day. With no
    /// metabolic term a bare body drifted to exactly those numbers, so
    /// a player was cold in a meadow, at midday, in fair weather,
    /// before they had any way to make clothing.
    ///
    /// The five cases are the design, and the constant was chosen
    /// against them rather than picked: the meadow is comfortable, its
    /// night is not, a lake is not, the cold tenth of the map is not,
    /// and a desert is still hot. Change [`METABOLIC_LIFT_C`] and this
    /// says which half of the world you broke.
    #[test]
    fn the_world_a_player_wakes_up_in_is_survivable_with_no_clothes() {
        // Measured, not invented. `worldgen::climate_column` over 1764
        // columns, mapped through `climate::COLDEST_C..HOTTEST_C`.
        const SPAWN_NOON: f32 = 16.0;
        const SPAWN_NIGHT: f32 = 4.1;
        const COLD_TENTH: f32 = 3.7;
        const DESERT_NOON: f32 = 41.8;
        // `climate::WATER_C`, which is a ceiling rather than an offset.
        const IN_WATER: f32 = 12.0;

        let bare = |ambient, wetness| felt_ambient(ambient, 0.0, wetness);

        let meadow = bare(SPAWN_NOON, 0.0);
        assert_eq!(
            Comfort::of(meadow),
            Comfort::Comfortable,
            "a player waking in the starting meadow at noon is {meadow}, which is not comfortable",
        );
        assert!(
            (COMFORT_LOW..=COMFORT_HIGH).contains(&meadow),
            "the meadow is {meadow}, outside the band the gauge stays hidden for",
        );

        // ...and the three things that are supposed to be a problem
        // still are.
        for (what, target) in [
            ("the night the meadow turns into", bare(SPAWN_NIGHT, 0.0)),
            ("the cold tenth of the map", bare(COLD_TENTH, 0.0)),
            // Soaked: water takes half the body's own heat with it,
            // which is what makes a lake colder than the air over it.
            ("swimming", bare(IN_WATER, 1.0)),
        ] {
            assert!(
                target < COMFORT_LOW,
                "{what} settles at {target}, inside the comfortable band",
            );
        }

        // A desert is untouched: the lift is one-sided, so nothing here
        // has made hot places cooler.
        let desert = bare(DESERT_NOON, 0.0);
        assert!(
            desert > OVERHEATED,
            "the desert settles at {desert}, which is no longer hot",
        );
    }

    #[test]
    fn a_coat_buys_time_and_does_not_buy_immunity() {
        // Both halves matter. A coat that did nothing would make the
        // whole equipment system decoration; a coat that made a player
        // immune would abolish fire and shelter as answers.
        let bare = soak(NEUTRAL_C, -5.0, 0.0, 0.0, 180.0);
        let clothed = soak(NEUTRAL_C, -5.0, 9.0, 0.0, 180.0);
        assert!(clothed > bare + 2.0, "the coat bought nothing");
        let forever = soak(NEUTRAL_C, -5.0, 9.0, 0.0, 3600.0);
        assert!(
            forever < COMFORT_LOW,
            "an hour in freezing air in a coat was survivable: {forever}"
        );
    }

    #[test]
    fn a_soaked_coat_is_no_coat() {
        let dry = soak(NEUTRAL_C, 2.0, 9.0, 0.0, 240.0);
        let wet = soak(NEUTRAL_C, 2.0, 9.0, 1.0, 240.0);
        assert!(wet < dry - 2.0, "being drenched cost nothing: {wet} vs {dry}");
        // ...and a set that sheds rain keeps most of what it had.
        let shed = soak(NEUTRAL_C, 2.0, 9.0, 0.1, 240.0);
        assert!(shed > wet + 1.0);
    }

    #[test]
    fn clothing_does_not_cool_anybody_down() {
        // The one asymmetry, stated so it cannot be tidied away: a coat
        // in a desert is not air conditioning. If insulation were
        // two-sided, the answer to a hot biome would be "wear more",
        // which is the opposite of what anyone would do.
        let hot = 44.0;
        assert!(felt_ambient(hot, 20.0, 0.0) > hot, "a coat shed heat");
        let bare = soak(NEUTRAL_C, hot, 0.0, 0.0, 900.0);
        let dressed = soak(NEUTRAL_C, hot, 20.0, 0.0, 900.0);
        assert!(dressed > bare, "a coat cooled somebody down: {dressed} vs {bare}");
    }

    #[test]
    fn a_coat_never_heats_past_neutral() {
        // Otherwise a player in full leather standing in a chilly wood
        // would run a fever, which is the sort of thing an offset
        // without a ceiling does.
        for ambient in [-30.0f32, -10.0, 0.0, 10.0, 25.0, 29.9] {
            let target = felt_ambient(ambient, 40.0, 0.0);
            assert!(target <= NEUTRAL_C + 1e-4, "{ambient} -> {target}");
        }
    }

    #[test]
    fn damage_ramps_in_rather_than_switching_on() {
        assert_eq!(exposure_damage_per_second(FREEZING + 0.1), 0.0);
        let first = exposure_damage_per_second(FREEZING - 0.5);
        let worse = exposure_damage_per_second(FREEZING - 8.0);
        assert!(first > 0.0);
        assert!(worse > first * 2.0, "the ramp is flat: {first} then {worse}");
        // ...and the same shape at the other end, so neither is special.
        assert_eq!(exposure_damage_per_second(SCALDING - 0.1), 0.0);
        assert!(exposure_damage_per_second(SCALDING + 8.0) > 0.0);
    }

    #[test]
    fn a_long_hitch_cannot_teleport_a_body_across_the_scale() {
        // `dt` comes from a real clock. A frame that took five seconds
        // -- a breakpoint, a laptop waking up -- must not be five
        // seconds of freezing applied at once.
        let one_big = step_temperature(NEUTRAL_C, -40.0, adjust_rate(0.0, 0.0, -40.0), 5.0);
        let many_small = soak(NEUTRAL_C, -40.0, 0.0, 0.0, 0.5);
        assert!((one_big - many_small).abs() < 0.5, "{one_big} vs {many_small}");
    }

    #[test]
    fn a_nonsense_temperature_is_not_contagious() {
        // These numbers pass through a network message and a save file.
        assert_eq!(felt_ambient(f32::NAN, 5.0, 0.0), NEUTRAL_C);
        assert_eq!(adjust_rate(5.0, 0.0, f32::NAN), BASE_ADJUST_PER_SECOND);
        assert_eq!(exposure_damage_per_second(f32::NAN), 0.0);
        assert_eq!(thirst_multiplier(f32::NAN), 1.0);
        assert_eq!(Comfort::of(f32::NAN), Comfort::Comfortable);
        let stepped = step_temperature(20.0, f32::NAN, 0.05, 0.05);
        assert!(
            stepped.is_finite(),
            "a nonsense target produced a nonsense body temperature"
        );
    }

    #[test]
    fn work_costs_water_and_resting_costs_less() {
        assert!(
            Exertion::RESTING.thirst_per_second() < Exertion {
                moving: true,
                ..Exertion::RESTING
            }
            .thirst_per_second()
        );
        let sprint = Exertion {
            moving: true,
            sprinting: true,
            working: false,
        };
        assert!(sprint.thirst_per_second() > WALK_THIRST_PER_SECOND);
        // ...and the heat multiplies it rather than replacing it.
        assert_eq!(thirst_multiplier(NEUTRAL_C), 1.0);
        assert!(thirst_multiplier(SCALDING) > 1.4);
    }

    #[test]
    fn a_full_skin_lasts_longer_than_a_day() {
        // The relationship thirst has to have with the clock: a player
        // who drinks in the morning should not be looking for water
        // again before evening, or the meter is a chore rather than a
        // provision to make.
        let seconds = MAX_HYDRATION / IDLE_THIRST_PER_SECOND;
        assert!(
            seconds > 900.0,
            "a full skin runs out in {seconds}s, inside one default day"
        );
    }

    #[test]
    fn being_cold_makes_a_player_hungry_before_it_hurts_them() {
        // The order of the warnings. At the chilled threshold hunger is
        // already running faster and nothing has taken any health yet.
        assert_eq!(exposure_damage_per_second(CHILLED), 0.0);
        assert!(shiver_hunger_multiplier(CHILLED) > 1.0);
        assert_eq!(shiver_hunger_multiplier(NEUTRAL_C), 1.0);
        assert!(shiver_hunger_multiplier(FREEZING) > shiver_hunger_multiplier(CHILLED));
    }

    /// `soak`, under a sky: the same 20 Hz run through the whole model --
    /// the sun through what is worn, the shade, the water.
    fn bask(mut body: f32, exposure: Exposure, insulation: f32, shade: f32, wetness: f32, seconds: f32) -> f32 {
        let dt = 1.0 / 20.0;
        let rate = adjust_rate_under_sky(exposure, insulation, shade, wetness);
        let target = felt_under_sky(exposure, insulation, shade, wetness);
        let mut elapsed = 0.0;
        while elapsed < seconds {
            body = step_temperature(body, target, rate, dt);
            elapsed += dt;
        }
        body
    }

    /// The open desert at a clear noon: the server's hot desert (warmth
    /// 0.9, no season) and its full sun. Written out because this crate
    /// cannot see `climate`; `climate`'s own tests run the real sample.
    const DESERT_NOON: Exposure = Exposure {
        air_c: 38.4,
        sun_c: 14.0,
        in_water: false,
    };

    #[test]
    fn the_sun_arrives_faster_than_the_cold_and_crossing_it_is_still_free() {
        // The cold side's promise, kept on the hot side at the pace a day
        // allows (`HOT_ADJUST_FACTOR`): a dash across the open is free,
        // standing in it is not, and the shade is felt as soon as it is
        // reached.
        let dash = bask(NEUTRAL_C, DESERT_NOON, 0.0, 0.0, 0.0, 30.0);
        assert_eq!(
            Comfort::of(dash),
            Comfort::Comfortable,
            "thirty seconds across the open desert left the skin at {dash}"
        );
        let stood = bask(NEUTRAL_C, DESERT_NOON, 0.0, 0.0, 0.0, 240.0);
        assert_eq!(
            Comfort::of(stood),
            Comfort::Scorching,
            "four minutes bare in the desert sun left the skin at only {stood}"
        );
        // Told "heatstroke" and walked into shade: out of the damage in
        // twenty seconds.
        let shaded = bask(SCALDING + 1.0, Exposure::air(DESERT_NOON.air_c), 0.0, 0.0, 0.0, 20.0);
        assert!(
            exposure_damage_per_second(shaded) == 0.0,
            "twenty seconds in the shade left the skin at {shaded}, still burning"
        );
    }

    #[test]
    fn a_dip_in_the_river_buys_time_on_the_open_plain() {
        // The decision water exists for in hot country. A player a degree
        // off heatstroke who walks on is burning in seconds; the same
        // player after half a minute in a river has most of a minute of
        // open sun in hand -- and that is with the soaking assumed gone
        // the moment they climb out, which in the desert is nearly true
        // (`climate::HEAT_DRYING`). Without `WATER_ADJUST_FACTOR` the
        // same dip bought about thirty-five seconds.
        let river = Exposure {
            // `climate::WATER_C`: a lake in a desert is not a hot lake.
            air_c: 12.0,
            sun_c: 0.0,
            in_water: true,
        };
        let seconds_until_scalded = |mut body: f32| {
            let mut seconds = 0.0;
            while body <= SCALDING && seconds < 900.0 {
                body = bask(body, DESERT_NOON, 0.0, 0.0, 0.0, 1.0);
                seconds += 1.0;
            }
            seconds
        };
        let hot = SCALDING - 1.0;
        let walked_on = seconds_until_scalded(hot);
        let dipped = bask(hot, river, 0.0, 0.0, 1.0, 30.0);
        let after_the_dip = seconds_until_scalded(dipped);
        assert!(walked_on < 15.0, "a body a degree off heatstroke took {walked_on}s to get there");
        assert!(
            after_the_dip > 60.0,
            "thirty seconds in a river ({hot} -> {dipped}) bought only {after_the_dip}s of sun"
        );
    }

    #[test]
    fn a_soaked_body_is_cooler_in_the_heat_and_never_cold_for_it() {
        let dry = felt_ambient(44.0, 0.0, 0.0);
        let soaked = felt_ambient(44.0, 0.0, 1.0);
        assert!(
            (dry - soaked - WET_COOLING_C).abs() < 1e-4,
            "a soaking took {} off a 44-degree day",
            dry - soaked
        );
        // Never below neutral however the day goes, and no step anywhere
        // on the way up -- in particular none at `COMFORT_HIGH`, where the
        // hot branch begins. See `WET_COOLING_C` on the ramp.
        let mut air = 20.0f32;
        let mut last = felt_ambient(air, 0.0, 1.0);
        while air < 60.0 {
            air += 0.05;
            let now = felt_ambient(air, 0.0, 1.0);
            // Below neutral a soaking is the cold side's business, and it
            // is meant to chill there (`WET_METABOLIC_LOSS`).
            assert!(
                air < NEUTRAL_C || now >= NEUTRAL_C - 1e-3,
                "soaked in {air} degrees heads for {now}, which is cold"
            );
            assert!((now - last).abs() < 0.2, "a soaked body's target stepped by {} at {air}", now - last);
            last = now;
        }
        // ...and the cold side is exactly what it was: water in the cold
        // is still the danger, not the relief.
        assert_eq!(felt_ambient(12.0, 0.0, 1.0), 12.0 + METABOLIC_LIFT_C * (1.0 - WET_METABOLIC_LOSS));
        // Nonsense in, nothing out.
        assert_eq!(felt_ambient(40.0, 0.0, f32::NAN), felt_ambient(40.0, 0.0, 0.0));
        assert_eq!(sun_on_skin(f32::NAN, 0.0), 0.0);
        assert_eq!(sun_on_skin(10.0, f32::NAN), 10.0);
        assert_eq!(
            felt_under_sky(Exposure::air(f32::NAN), 0.0, 0.0, 0.0),
            NEUTRAL_C
        );
    }

    #[test]
    fn cloth_keeps_the_sun_off_and_a_coat_does_not() {
        // `GARMENT_SLOTS`, not `SLOTS`: the fifth slot is the back,
        // which holds a rucksack and never a garment. See
        // `equipment::GARMENT_SLOTS`.
        use crate::equipment::{garment, Worn, GARMENT_SLOTS, SLOTS};
        use crate::types::*;
        let set = |blocks: [BlockId; GARMENT_SLOTS]| {
            let mut pieces = [None; SLOTS];
            for (piece, block) in pieces.iter_mut().zip(blocks) {
                *piece = garment(block);
            }
            Worn::total(pieces)
        };
        let cloth = set([BLOCK_CLOTH_CAP, BLOCK_CLOTH_TUNIC, BLOCK_CLOTH_TROUSERS, BLOCK_CLOTH_WRAPS]);
        let leather = set([BLOCK_LEATHER_CAP, BLOCK_LEATHER_TUNIC, BLOCK_LEATHER_LEGGINGS, BLOCK_LEATHER_BOOTS]);
        let wool = set([BLOCK_WOOL_CAP, BLOCK_WOOL_TUNIC, BLOCK_WOOL_LEGGINGS, BLOCK_WOOL_BOOTS]);
        let iron = set([BLOCK_IRON_HELM, BLOCK_IRON_CUIRASS, BLOCK_IRON_GREAVES, BLOCK_IRON_BOOTS]);
        let in_the_sun = |w: Worn| felt_under_sky(DESERT_NOON, w.insulation, w.shade, 0.0);

        let bare = felt_under_sky(DESERT_NOON, 0.0, 0.0, 0.0);
        assert!(bare > SCALDING, "bare skin in the desert sun heads for only {bare}");
        // Measured: cloth heads for about 38, bare skin for 52.4 -- the
        // difference between a warning and a death.
        assert!(
            in_the_sun(cloth) < SCALDING && in_the_sun(cloth) < bare - 10.0,
            "a full cloth set in the desert sun heads for {} against bare skin's {bare}",
            in_the_sun(cloth)
        );
        // Hide, fleece and plate keep none of it off, and trap what they
        // trap: never cooler than skin in the sun.
        for (name, worn) in [("leather", leather), ("wool", wool), ("iron", iron)] {
            assert!(
                in_the_sun(worn) >= bare,
                "{name} kept the sun off: {} against bare skin's {bare}",
                in_the_sun(worn)
            );
        }
        assert!(in_the_sun(wool) > in_the_sun(leather), "wool was no hotter in the sun than leather");

        // A hat keeps the sun off a head, all of it and no more.
        let cap = set([BLOCK_CLOTH_CAP, BLOCK_AIR, BLOCK_AIR, BLOCK_AIR]);
        let head = crate::equipment::Slot::Head.heat_share();
        assert!(
            (sun_on_skin(14.0, cap.shade) - 14.0 * (1.0 - head)).abs() < 1e-4,
            "a cloth cap let {} of 14 degrees of sun through",
            sun_on_skin(14.0, cap.shade)
        );
        // ...and nothing anybody adds keeps off more than all the sun on
        // its own slot, or a shirt would shade a head.
        for &(id, name) in ALL_BLOCK_IDS {
            if let Some(g) = garment(id) {
                assert!(
                    g.shade <= SHADE_KEEPS_OFF_ALL_THE_SUN_C,
                    "{name} casts {} degrees of shade, more than keeps all the sun off",
                    g.shade
                );
            }
        }
    }
}
