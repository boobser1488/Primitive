//! Health, fall damage, regeneration and death.
//!
//! ## Why this lives on the server
//!
//! Health is the one number a cheat client has the most obvious reason
//! to lie about, so it is tracked here and the client is only ever
//! *told* what it is. The client draws a bar; it never decides what the
//! bar says.
//!
//! ## How a fall is measured
//!
//! Not from velocity, which the server never sees, and not from the
//! client reporting "I fell 12 blocks", which is the same thing as
//! letting the client set its own health. Instead the server watches the
//! `on_ground` flag it already validates for the anti-cheat, and
//! remembers the highest point reached since the player last left the
//! ground. Landing turns that into a distance.
//!
//! The consequence worth knowing: transform updates are throttled and
//! lossy, so the peak is sampled rather than exact. A fall measured this
//! way is never *longer* than the real one -- a missed update can only
//! lose a higher sample -- so the error is always in the player's
//! favour, which is the right direction for it to be wrong in.

use std::time::Instant;

use primitive_shared::body;
use primitive_shared::comfort;
use primitive_shared::food;
use primitive_shared::injury::{self, Injuries};

/// What a player was doing this tick, for the purpose of billing them
/// for it.
///
/// The whole of the hunger mechanic's input, and it is deliberately
/// three booleans rather than a number: the tick loop knows what the
/// player *did* -- it validated the movement and it rate-limits the
/// block edits -- and it does not know, and should not have to invent,
/// how much a swing is worth. The rates live in `primitive_shared::food`
/// where the client can read them too.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Effort {
    /// Moving fast enough that it can only be a sprint.
    pub sprinting: bool,
    /// Swinging at a block. Billed by the second rather than by the
    /// block -- see `food::MINING_DRAIN_PER_SECOND`.
    pub mining: bool,
    /// In the water deep enough to be swimming rather than wading.
    ///
    /// **Swimming is work**, and it was free: a player could cross a
    /// sea and arrive as fed as they left. Billed at the sprint's rate
    /// and multiplied by what they are carrying (`load::swim_effort`),
    /// which is what makes hauling stone across a lake a decision
    /// rather than a slower walk.
    pub swimming: bool,
}

impl Effort {
    /// Doing nothing in particular, which is most ticks.
    pub const IDLE: Effort = Effort {
        sprinting: false,
        mining: false,
        swimming: false,
    };

    /// What this tick costs, in nourishment.
    ///
    /// Additive: a player sprinting *and* mining is paying for both,
    /// which is right -- they are running a wheelbarrow up a hill.
    ///
    /// `kilograms` is what they are carrying, and it is here for one
    /// term only: swimming loaded costs more than swimming empty (see
    /// `load::swim_effort`). Walking loaded is already paid for by
    /// being slower (`load::speed_scale`), and charging it twice would
    /// be a player who cannot afford to carry anything.
    pub fn drain(self, dt: f32, kilograms: f32) -> f32 {
        let mut rate = food::IDLE_DRAIN_PER_SECOND;
        if self.sprinting {
            rate += food::SPRINT_DRAIN_PER_SECOND;
        }
        if self.mining {
            rate += food::MINING_DRAIN_PER_SECOND;
        }
        if self.swimming {
            rate +=
                food::SPRINT_DRAIN_PER_SECOND * primitive_shared::load::swim_effort(kilograms);
        }
        rate * dt.max(0.0)
    }
}

/// Full health. Twenty, so one point can read as half a heart if the
/// client ever wants to draw hearts rather than a bar.
pub const MAX_HEALTH: f32 = 20.0;

/// How far you can fall before it hurts.
pub const SAFE_FALL_BLOCKS: f32 = 3.0;

/// Damage per block beyond `SAFE_FALL_BLOCKS`.
pub const DAMAGE_PER_BLOCK: f32 = 1.0;

/// ...and per block *squared*, which is the part that decides what a
/// cliff is.
///
/// A falling body arrives with energy proportional to the height it fell
/// from, not to the height itself, and a landing is that energy being
/// taken out of it. So the curve is quadratic, and the difference
/// between the two shapes is the whole feel of height in this game: a
/// linear half-point per block -- which is what this was -- made the
/// fatal fall forty-three blocks in a world only sixty-four tall, so
/// nothing you could actually walk off was dangerous and the ground was
/// scenery.
///
/// With these two terms a six-block drop is a scratch, twelve blocks
/// takes half of you, and eighteen kills. That is a world where a cliff
/// is a thing you look down before stepping off, which is what asking
/// for more fall damage is asking for -- and where the ledge you *can*
/// jump off is still the ledge you could jump off before, because the
/// safe distance has not moved.
pub const DAMAGE_PER_BLOCK_SQUARED: f32 = 0.035;

/// How long a head can stay under water before it starts to cost
/// anything.
///
/// Long enough to swim a lake across or dive to a river bed and come
/// back, short enough that the bottom of a deep ocean is somewhere you
/// go *deliberately*. A player who has never noticed it is a player who
/// has never overstayed.
pub const BREATH_SECONDS: f32 = 15.0;
/// Damage per second once the breath has run out.
///
/// Steep on purpose: drowning is not a slow tax on swimming, it is what
/// happens when you did not turn back. Four seconds of it kills a
/// healthy player, which is time enough to reach the surface from a
/// depth anybody sane was swimming at.
pub const DROWNING_PER_SECOND: f32 = 5.0;

/// Damage per second once smoke has taken the breath: a fifth of drowning.
/// See `Vitals::breathe_smoke`.
pub const SMOKE_PER_SECOND: f32 = 1.0;

/// How fast a fire hurts somebody standing in it, per second.
///
/// **Four a second, which is a full player in five seconds.** The
/// comment on `types::is_burning` has said "standing on it hurts" since
/// the day fire was added and nothing ever did: a player could stand in
/// the middle of a burning campfire indefinitely, which is the one thing
/// about fire everybody already knows is false.
///
/// Faster than drowning is slow and slower than a fall is fatal. What it
/// has to be is *immediately obvious* -- a fire that took half a minute
/// to kill would teach nobody anything, because nobody would still be
/// standing in it -- and survivable if you step out, because a campfire
/// is something a player builds at their feet in the dark and will walk
/// into by accident.
pub const BURNING_PER_SECOND: f32 = 4.0;

/// How far under the feet a fire still counts as being stood on, in cells.
///
/// A tenth: a body resting on a pit is flush with its top, and one a tenth
/// of a cell over it is in the air -- jumping across a fire is not standing
/// on it.
pub const ON_A_FIRE: f32 = 0.1;

/// **Whether a body standing at `feet` is in a fire, or on one.**
///
/// `heights` are the points above the feet whose cells count too -- a
/// player's eye, an animal's middle -- and `look` is the world. One function
/// for the player's tick and the animals' step, so a fire cannot burn one and
/// spare the other.
///
/// Two ways a fire reaches a body:
///
/// * **In its cell.** A campfire is a quarter of a cell tall: the feet are in
///   it and the head is in the air above, and asking only one of the two would
///   let somebody stand in a fire by being tall.
/// * **Under the feet** ("сделай урон от огненной ямы"). A burning pit kiln and
///   a burning log pile fill their cell to the rim, so a body standing on one
///   has its feet in the cell *over* the fire -- and only the cells a body was
///   in were ever asked. Sixteen armfuls of fuel alight for an hour, and a
///   player could stand in the middle of them for all of it. The cell
///   [`ON_A_FIRE`] under the feet is asked as well.
///
/// **Not a kiln's roof or a bloomery's.** Their fire is inside clay walls, and
/// `types::is_burning` names them because the fire map burns them down, not
/// because their tops are alight: standing on the oven you built is not
/// standing in the flame. Only a fire open at the top -- a campfire, a firepit,
/// a pit, a pile -- burns what stands on it.
pub fn touches_fire(
    feet: (f32, f32, f32),
    heights: &[f32],
    look: impl Fn(i32, i32, i32) -> Option<primitive_shared::types::BlockId>,
) -> bool {
    use primitive_shared::types::{
        block_kind, is_burning, BLOCK_CAMPFIRE_LIT, BLOCK_FIREPIT_LIT, BLOCK_LOG_PILE_LIT, BLOCK_PIT_KILN_LIT,
    };
    // The pit and the pile are not `is_burning`, on purpose: that is what the
    // fire map adopts and burns down in minutes, and they burn for an hour on
    // their own clock (`logic::pits`).
    let in_the_ground = |block| matches!(block_kind(block), BLOCK_PIT_KILN_LIT | BLOCK_LOG_PILE_LIT);
    let open_at_the_top = |block| {
        matches!(block_kind(block), BLOCK_CAMPFIRE_LIT | BLOCK_FIREPIT_LIT)
            || in_the_ground(block)
            || primitive_shared::wildfire::is_blazing(block)
    };
    let (x, z) = (feet.0.floor() as i32, feet.2.floor() as i32);
    let within = std::iter::once(0.0).chain(heights.iter().copied()).any(|height| {
        // ...and wood that has caught (`wildfire::is_blazing`): a burning
        // wall is walked into and a burning floor is stood on, and both
        // are fire.
        look(x, (feet.1 + height).floor() as i32, z).is_some_and(|block| {
            is_burning(block) || in_the_ground(block) || primitive_shared::wildfire::is_blazing(block)
        })
    });
    within || look(x, (feet.1 - ON_A_FIRE).floor() as i32, z).is_some_and(open_at_the_top)
}

/// Health per second once regeneration starts.
///
/// **A wound is a reason to go home.** At the rate this replaced (0.6 a
/// second) a full bar came back in thirty-three seconds: a boar fight
/// was forgotten before the meat was cooked, a twelve-block fall was a
/// minute's inconvenience, and health was a resource that refilled
/// faster than anybody could spend it on purpose. Nothing that heals
/// that fast is a wound, and nothing about being hurt was a decision.
///
/// A third of that -- the first number tried -- still closed a boar's
/// worth of damage in forty seconds, which is less than the walk back
/// to camp, so it moved nothing. A full game day for a full bar
/// (0.022) made a scratch a session-long tax. This is between them: a
/// twentieth a second, so near death to full takes about four hundred
/// seconds -- half a day at the default clock, roughly the walk home
/// and a meal -- and the eight points a boar takes are back in under
/// three minutes, which is a stretch in which a second boar is a
/// different question from the first.
///
/// Slow enough that clothing, shelter and a full stomach matter *while*
/// healing: the same three things the cold asks for, which is not a
/// coincidence -- see `regenerate` for what stops it.
pub const REGEN_PER_SECOND: f32 = 0.05;

/// Quiet time after taking damage before regeneration begins. Without a
/// delay, small chip damage heals before the player notices it happened,
/// and the health bar stops meaning anything.
pub const REGEN_DELAY_SECS: f32 = 6.0;

/// Health difference small enough not to be worth a network message.
///
/// Regeneration is continuous, so without this the server would send a
/// `Health` message every tick for the entire six seconds after a
/// scratch -- more traffic than the player movement it accompanies.
const REPORT_EPSILON: f32 = 0.05;

/// Damage from falling `distance` blocks while carrying `kilograms`.
///
/// Landing in water costs nothing: that is the whole reason players dig
/// a pool at the bottom of a shaft, and taking it away would make water
/// purely decorative.
///
/// Weight multiplies what is left. That is what stops a deep mine from
/// being a straight drop down the shaft with a full pack: the trip down
/// is free, the trip down *loaded* is not.
pub fn fall_damage(distance: f32, landed_in_liquid: bool, kilograms: f32) -> f32 {
    if landed_in_liquid || !distance.is_finite() {
        return 0.0;
    }
    let past = (distance - SAFE_FALL_BLOCKS).max(0.0);
    let base = past * DAMAGE_PER_BLOCK + past * past * DAMAGE_PER_BLOCK_SQUARED;
    base * primitive_shared::load::fall_multiplier(kilograms)
}

/// What happened to a player's health this update, if anything worth
/// telling them about.
#[derive(Debug, Clone, PartialEq)]
pub enum Outcome {
    /// Health is unchanged, or changed by too little to report.
    Unchanged,
    /// Health changed; the client needs the new value.
    Changed,
    /// Health reached zero. Carries what to put on the death screen.
    Died { cause: String },
}

/// What took the health: the one distinction `lose` is written round.
///
/// **"Was the player hurt" is two questions**, and for a long time this
/// module only had an answer to one of them. Everything that wants to know
/// reads it for a different reason -- mending waits on *any* health lost,
/// the bed waits on being *struck* -- and a body freezing in a blizzard is
/// the case where the two answers differ and the wrong one was being given.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Harm {
    /// Something in the world hit this body: an animal, a spear, a fall, a
    /// stalagmite, a fire, a lungful of water, a lungful of smoke. Delays
    /// mending, and it is the one thing that shuts the bed.
    Blow,
    /// The body failing on its own: hunger, thirst, the cold, the heat, bad
    /// water, a bad mushroom. Delays mending -- it is health going -- and
    /// is not a blow, because there is nothing standing over the player.
    Ailing,
    /// A cut bleeding into a sleeve (`mend`). Neither: what stops a bleeding
    /// body mending is said plainly in `regenerate` instead, and counted as
    /// a blow it would have been a player who could never lie down.
    Bleeding,
}

pub struct Vitals {
    health: f32,
    dead: bool,
    /// Highest point reached since leaving the ground. `None` while
    /// standing on something.
    fall_peak_y: Option<f32>,
    /// Whether the last accepted transform said the player was airborne.
    airborne: bool,
    last_damage: Instant,
    /// When something last *struck* this body. See [`Harm`].
    last_blow: Instant,
    /// The value the client was last told, so we only send changes.
    last_reported: f32,
    /// What the player is carrying, in kilograms.
    ///
    /// Refreshed from the server's own inventory before each fall is
    /// judged. It was a number the client asserted, back when the
    /// inventory lived there; now there is nothing to assert.
    carried_kg: f32,
    /// Seconds of breath left. Refills instantly at the surface -- a
    /// lungful is a lungful, and metering the recovery would only make
    /// a second dive a worse version of the first.
    breath: f32,
    /// How full the player is, 0..`MAX_NOURISHMENT`.
    ///
    /// Beside health rather than in a structure of its own, because
    /// every question about one is a question about the other: hunger
    /// stops healing, empty hunger takes health, and both of them are
    /// restored together by a respawn. Two structs would have meant a
    /// third thing holding them and asking each in turn.
    nourishment: f32,
    /// The value the client was last told, so only real changes are
    /// sent. See `needs_food_report`.
    last_food_reported: f32,
    /// When each food group was last eaten, indexed by
    /// `food::Group as usize`.
    ///
    /// **Three instants rather than a running score**, because the
    /// question the mechanic asks is "when did you last eat one of
    /// these", and an instant answers it exactly while a decaying
    /// number answers it approximately and has to be ticked. Nothing
    /// steps these: they are read against the clock when healing is
    /// worked out (`diet_groups`) and are otherwise inert.
    ///
    /// Not saved. A player who logs out on a mixed diet logs back in on
    /// none, and heals slowly for the first minutes -- which is the
    /// same bargain the rot clock's phase makes (`logic::rot`), for the
    /// same reason: a `diet.bin` for three timestamps is a file format
    /// nobody would thank us for.
    last_of_group: [Option<Instant>; 3],
    /// How much longer bad water is still taking, in seconds.
    ///
    /// A countdown rather than an end time, so it ticks with the
    /// world's clock and not with the wall's -- a paused server does
    /// not cure anybody.
    ///
    /// **Saved now**, and the note here used to say the opposite: "a
    /// minute and a half of illness is not worth a file format". The other
    /// half of that sentence is what a player found -- quitting to the menu
    /// and coming straight back was the cure, free and instant, and
    /// available at exactly the moment being ill costs anything. Being
    /// short is what made it cheap to use rather than harmless. See
    /// `profiles::Profile::sick_for`.
    sick_for: f32,
    /// An illness swallowed and not yet felt: how long it will last once
    /// it starts, and how long until it does. See `body::DIGESTION_SECONDS`.
    ///
    /// **Not saved as itself.** A profile carries `sick_for`, and on the way
    /// to disk the illness still to come is folded into it
    /// (`illness_owed`): a player who logs out with a pond in them comes back
    /// ill at once rather than well, which loses them the two minutes'
    /// grace and nothing else, and a relog stays what it must be -- no cure.
    /// A second field in the file for a two-minute delay was the rejected
    /// alternative, and it would have been a save format for a stopwatch.
    brewing: f32,
    brewing_in: f32,
    /// Skin temperature, in the degrees `primitive_shared::body`
    /// measures in.
    ///
    /// Beside health and hunger for exactly the reason nourishment is:
    /// every question about one is a question about the others. Being
    /// cold makes you hungry, being hot makes you thirsty, and both ends
    /// of the scale take health -- three couplings that would each have
    /// to be plumbed between two structs if these lived apart.
    body_c: f32,
    /// How wet the player is, 0..1. Wet clothing does not insulate.
    ///
    /// Has memory, which is why it is here rather than being recomputed
    /// from the world each tick: climbing out of a lake does not dry
    /// you, and that tail is most of what makes a swim a decision.
    wetness: f32,
    /// Water left, 0..`body::MAX_HYDRATION`.
    hydration: f32,
    /// Every wound on the body. See `primitive_shared::injury`.
    ///
    /// **Saved with the body**, on the argument the broken leg this
    /// replaced was saved on: a deep cut or a set leg is minutes to half
    /// an hour of play, and an injury a reconnect cleared would be an
    /// injury nobody carried. The leg used to be a lone count of seconds
    /// here (`fracture_for`); it is one wound among the rest now, so a
    /// splint and a bandage are the same kind of answer to the same kind
    /// of question.
    injuries: Injuries,
    /// Where the next blow's roll comes from. See `next_roll`.
    blow_seed: u64,
    /// The part of a drop of blood the open cuts have shed and not yet
    /// shown. See `drips`.
    drip_owed: f32,
    /// Seconds of empty stomach not yet rolled for, under
    /// `food::STARVATION_ROLL_SECONDS`. Carried between steps so a night
    /// slept through in one step rolls exactly as often as the same night
    /// lived tick by tick.
    starving_owed: f32,
    /// Seconds of a night at home still keeping the hunger down
    /// (`comfort::RESTED_SECONDS`, `rest_at_home`). Not saved: see the note
    /// on `RESTED_SECONDS` for why a relog costs the morning and nothing else.
    rested_for: f32,
    /// How many starvation rolls this body has made, and the seed they are
    /// made from. Their own sequence rather than `blow_seed`, which every
    /// blow and every mouthful of bad water advances: with that one a
    /// night slept in one step and the same night ticked would roll
    /// different numbers for the same hunger.
    hunger_rolls: u64,
    hunger_seed: u64,
    /// How tired the player is, 0 (fresh) .. 1 (finished).
    ///
    /// Beside the other three meters for the reason they are beside
    /// each other: tiredness slows a body down and stops it mending,
    /// which are questions about movement and about health, and a
    /// fourth struct would mean a fifth thing holding all of them.
    ///
    /// **Saved**, unlike the diet and the sickness. Those are minutes
    /// long and a player who logs out mid-illness has lost nothing they
    /// will notice; this is a debt built up over a whole day, and
    /// logging out to clear it would be the cheapest exploit in the
    /// game and the most obvious.
    fatigue: f32,
    /// Whether the feet are on the point of a stalagmite, as the last
    /// transform found them. Read by the landing (`on_transform`) and set
    /// before it, on `set_carried_weight`'s terms: the server looks at its
    /// own world, and the client has nothing to assert. See
    /// `dripstone::spike_under`.
    on_spike: bool,
    /// Where the body was on the ground at the last transform, and when, for
    /// the speed a push through stakes is made at (`spikes::harm`), and the
    /// time of the last cut from them (`spikes::COOLDOWN`). Not saved: both
    /// are a second old at most.
    last_ground: Option<(f64, f64, std::time::Instant)>,
    staked_at: Option<std::time::Instant>,
    /// How dirty the body is, 0 clean .. 1 caked. See `comfort::step_grime`.
    ///
    /// **Not saved**, and neither is `comfort_level`. Grime is a step into a
    /// river from gone, and comfort is worked out again from where the
    /// player stands within half a minute of joining -- the place is the
    /// truth and the number only follows it. A relog that cleared either
    /// would clear seconds of anything, which is not the exploit `fatigue`
    /// is saved to close; a save format version for them would cost a
    /// frozen copy of the whole profile (see `profiles`) for nothing.
    grime: f32,
    /// Comfort, `comfort::LOWEST..=HIGHEST`: hidden, settled slowly towards
    /// what the body and the place say (`settle_comfort`), and what scales
    /// regeneration and stamina. Not `comfort()`, which is the temperature
    /// band -- see the note at the top of `primitive_shared::comfort`.
    comfort_level: f32,
    /// Bars of food eaten since this body last left dung. See
    /// `comfort::goes_now`. Not saved: the worst a relog does is put the
    /// next pat off by a day's food.
    dung_owed: f32,
    /// On the ground and not yet dead. See `primitive_shared::downed`.
    ///
    /// **Here, beside `dead`, and not a flag on it**, because a downed body
    /// is alive to every rule that asks: it is hungry, it is cold, it can be
    /// fed and bandaged and bitten. What it cannot do is heal (see
    /// `regenerate`), and what it has instead of health is a clock.
    ///
    /// **Not saved.** A player who leaves while down dies on the way out
    /// (the teardown in `net::connection` calls `give_up`), because the
    /// alternatives were both worse: a clock that stopped while they were
    /// away is a free pause at the worst moment of a fight, and one that
    /// kept running is a death nobody was there for -- and the pack would
    /// have had to lie somewhere for it.
    downed: Option<primitive_shared::downed::Down>,
    /// The server's words for what put the body down: what the death screen
    /// says if the clock runs out.
    downed_words: String,
    /// The act that raises the body happened (a bandage, a mouthful, a
    /// drink), and the next step of the clock gets it up. See `offer`.
    rescued: bool,
    /// The client's copy of `downed` is out of date: it went down, it got
    /// up, or something took time off the clock. See `take_downed_report`.
    downed_dirty: bool,
}

/// The ramp both tiredness costs share.
///
/// One from nothing at `body::TIRED_AT` to `worst` at the top of the
/// scale. Written once because the two costs must agree about *when*
/// they start: a player who notices their aim going and their speed
/// staying would be a player learning a rule that is not there.
fn tiredness_factor(fatigue: f32, worst: f32) -> f32 {
    if fatigue <= body::TIRED_AT {
        return 1.0;
    }
    let past = ((fatigue - body::TIRED_AT) / (1.0 - body::TIRED_AT)).clamp(0.0, 1.0);
    1.0 - past * (1.0 - worst)
}

impl Vitals {
    pub fn new() -> Self {
        Self {
            health: MAX_HEALTH,
            dead: false,
            fall_peak_y: None,
            airborne: false,
            // Far enough in the past that an untouched player regenerates
            // immediately rather than waiting out a delay they never
            // earned.
            last_damage: Instant::now() - std::time::Duration::from_secs(3600),
            last_blow: Instant::now() - std::time::Duration::from_secs(3600),
            last_reported: MAX_HEALTH,
            breath: BREATH_SECONDS,
            carried_kg: 0.0,
            nourishment: food::MAX_NOURISHMENT,
            last_food_reported: food::MAX_NOURISHMENT,
            // A new player has eaten nothing, and heals at the slow
            // rate until they have -- which is what the first meal is
            // worth and why the first one is usually meat.
            last_of_group: [None; 3],
            sick_for: 0.0,
            brewing: 0.0,
            brewing_in: 0.0,
            body_c: body::NEUTRAL_C,
            wetness: 0.0,
            hydration: body::MAX_HYDRATION,
            // A new player has just woken up. Whatever else the first
            // morning is, it is not a tired one -- and nothing is
            // broken.
            fatigue: 0.0,
            injuries: Injuries::default(),
            // Off the clock, so two players bitten by the same wolf are
            // not bitten on the same arm. Nothing depends on the value: a
            // test that wants a particular part says so with
            // `Injuries::inflict` rather than hoping for it.
            blow_seed: std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_nanos() as u64)
                .unwrap_or(0),
            drip_owed: 0.0,
            starving_owed: 0.0,
            rested_for: 0.0,
            hunger_rolls: 0,
            hunger_seed: std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_nanos() as u64 ^ 0x5EED_F00D)
                .unwrap_or(0),
            on_spike: false,
            last_ground: None,
            staked_at: None,
            grime: 0.0,
            // Neutral: a player who has just arrived has not yet been
            // anywhere, and the first surveys settle it.
            comfort_level: 0.0,
            dung_owed: 0.0,
            downed: None,
            downed_words: String::new(),
            rescued: false,
            downed_dirty: false,
        }
    }

    /// Records the current load. Still sanitised: the value is derived
    /// from block weights and stack counts, and a nonsense one would
    /// reach the damage maths.
    pub fn set_carried_weight(&mut self, kilograms: f32) {
        self.carried_kg = primitive_shared::load::sanitize(kilograms);
    }

    /// Records whether the feet are on the point of a stalagmite, for the
    /// next landing to read. See the field.
    pub fn set_on_spike(&mut self, on_spike: bool) {
        self.on_spike = on_spike;
    }

    /// A transform among sharpened stakes, or out of them.
    ///
    /// The speed is the server's own, from where it last had the body and
    /// when -- a client that claimed to be edging through at a shuffle while
    /// crossing a metre a frame would be believed about nothing else either.
    /// A cut goes on a leg, the roll's, because a stake is at leg height
    /// whichever way it is driven; see `spikes` for the numbers and why.
    pub fn on_stakes(&mut self, touching: bool, x: f64, z: f64, now: std::time::Instant) -> Outcome {
        let speed = self.last_ground.map_or(0.0, |(px, pz, then)| {
            let dt = now.saturating_duration_since(then).as_secs_f64();
            if dt <= 1e-3 {
                0.0
            } else {
                (((x - px).powi(2) + (z - pz).powi(2)).sqrt() / dt) as f32
            }
        });
        self.last_ground = Some((x, z, now));
        if !touching || self.dead {
            return Outcome::Unchanged;
        }
        let cooled = self.staked_at.is_none_or(|at| {
            now.saturating_duration_since(at).as_secs_f32() >= primitive_shared::spikes::COOLDOWN
        });
        let Some((damage, cut)) = primitive_shared::spikes::harm(speed).filter(|_| cooled) else {
            return Outcome::Unchanged;
        };
        self.staked_at = Some(now);
        let leg = if self.next_roll() >= 0.5 {
            primitive_shared::injury::Part::RightLeg
        } else {
            primitive_shared::injury::Part::LeftLeg
        };
        self.injuries.inflict(leg, primitive_shared::injury::Kind::Cut, cut);
        self.hurt(damage, "ran into sharpened stakes")
    }

    pub fn carried_weight(&self) -> f32 {
        self.carried_kg
    }

    pub fn health(&self) -> f32 {
        self.health
    }

    /// Restores a stored health value on join.
    ///
    /// Clamped and sanitised: the number comes off a file on a disk the
    /// operator can edit, and `NaN` health is a player who can neither
    /// die nor heal. Zero or less is read as "start fresh" rather than
    /// as dead, so a profile saved mid-death is not unplayable.
    pub fn set_health(&mut self, health: f32) {
        self.health = if health.is_finite() && health > 0.0 {
            health.min(MAX_HEALTH)
        } else {
            MAX_HEALTH
        };
        self.dead = false;
        // A heal that lands on a downed body gets it up: the mod that hands
        // out health is a rescue the body did not have to crawl for.
        self.stand(false);
        self.last_reported = self.health;
        self.clear_fall();
    }

    pub fn is_dead(&self) -> bool {
        self.dead
    }

    /// True if the client's copy of the health value is out of date.
    pub fn needs_report(&self) -> bool {
        (self.health - self.last_reported).abs() > REPORT_EPSILON
    }

    /// Marks the current value as delivered.
    pub fn mark_reported(&mut self) {
        self.last_reported = self.health;
    }

    /// Folds one accepted transform into the fall tracker.
    ///
    /// `landed_in_liquid` is only consulted on the frame the player
    /// touches down, and the caller is expected to have checked the
    /// block at the player's feet.
    pub fn on_transform(&mut self, y: f32, on_ground: bool, landed_in_liquid: bool) -> Outcome {
        if self.dead {
            return Outcome::Unchanged;
        }

        // Water cancels a fall in progress, not just the landing: a
        // player who dives into a lake and swims to the bottom has not
        // fallen the whole way, and charging them for it would make
        // deep water lethal.
        if landed_in_liquid {
            self.fall_peak_y = None;
            self.airborne = !on_ground;
            return Outcome::Unchanged;
        }

        if !on_ground {
            // Still in the air: remember how high we have been.
            self.fall_peak_y = Some(match self.fall_peak_y {
                Some(peak) => peak.max(y),
                None => y,
            });
            self.airborne = true;
            return Outcome::Unchanged;
        }

        // On the ground. If we were not a moment ago, this is a landing.
        let was_airborne = self.airborne;
        self.airborne = false;
        let Some(peak) = self.fall_peak_y.take() else {
            return Outcome::Unchanged;
        };
        if !was_airborne {
            return Outcome::Unchanged;
        }

        let damage = fall_damage(peak - y, false, self.carried_kg);
        // **A drop onto a spike cuts**, from two blocks rather than the
        // three a fall starts to hurt at -- the ledge a player drops off onto
        // rock for free costs a bandage onto dripstone. A leg, the roll's,
        // because the feet are what met the point; the fall's own bruising
        // and breaking below is unchanged, and adds to it. See `dripstone`
        // for why this is what dripstone is for.
        if self.on_spike {
            if let Some(cut) = primitive_shared::dripstone::spike_cut(peak - y) {
                let leg = if self.next_roll() >= 0.5 {
                    primitive_shared::injury::Part::RightLeg
                } else {
                    primitive_shared::injury::Part::LeftLeg
                };
                self.injuries.inflict(leg, primitive_shared::injury::Kind::Cut, cut);
                let roll = self.next_roll();
                self.injuries.fall(damage, roll);
                return self.hurt(damage + primitive_shared::dripstone::SPIKE_DAMAGE, "fell onto a stalagmite");
            }
        }
        if damage <= 0.0 {
            return Outcome::Unchanged;
        }
        // **A bad landing breaks a leg**, and the threshold is on the
        // damage rather than on the distance because the damage already
        // knows what the player was carrying: the same ledge is a
        // stumble empty-handed and a break under a full pack, which is
        // the honest arithmetic and the one a player can feel. See
        // `body::FRACTURE_DAMAGE`.
        //
        // The longer of the two rather than a sum: a second fall on an
        // already-broken leg does not double the fortnight. Which leg is
        // the roll's, and both are bruised by a landing that hurt -- see
        // `Injuries::fall`.
        let roll = self.next_roll();
        self.injuries.fall(damage, roll);
        self.hurt(damage, "fell from a great height")
    }

    /// Applies damage from something that *struck* this body, and reports
    /// what it did: an animal, a fall, a fire, a lungful of water.
    pub fn hurt(&mut self, amount: f32, cause: &str) -> Outcome {
        self.lose(amount, cause, Harm::Blow)
    }

    /// Applies damage the body is doing to itself -- hunger, thirst, the
    /// cold, the heat, bad water, a bad mushroom -- and reports what it did.
    ///
    /// **The same health down the same path, and not the same event.** See
    /// [`Harm`]: the difference is what `last_blow` is allowed to say.
    fn ail(&mut self, amount: f32, cause: &str) -> Outcome {
        self.lose(amount, cause, Harm::Ailing)
    }

    /// Health going, and what took it.
    ///
    /// **Bleeding was the first thing that was not a blow, and it was not
    /// the last.** A blow restarts the quiet time before healing and shuts
    /// the bed (`last_blow`); a cut bleeding into a sleeve does neither, or
    /// a player with a cut could not lie down at all. Hunger, thirst, the
    /// cold, the heat and bad water are the middle case and used to be sent
    /// down the blow's path with everything else, so a player who crawled
    /// into bed out of a blizzard was told **"you cannot sleep while
    /// something is hurting you"** -- a sentence about a wolf, said about
    /// the weather, with nothing anywhere near them. They read it as a bug,
    /// looked for the animal, and found the cold only by dying of it.
    ///
    /// All three end in the same place, so a death by any of them is one
    /// `Outcome::Died` down the one reporting path.
    fn lose(&mut self, amount: f32, cause: &str, harm: Harm) -> Outcome {
        use primitive_shared::downed::{Cause, Down, OVERKILL};
        if self.dead || amount.is_nan() || amount <= 0.0 {
            return Outcome::Unchanged;
        }
        // **Mending waits on all three of them minus the bleeding**, which is
        // the old rule under a new name: a body that is losing health to the
        // cold is not a body that is putting health back, and saying so here
        // is cheaper than a second reason inside `regenerate`.
        if harm != Harm::Bleeding {
            self.last_damage = Instant::now();
        }
        if harm == Harm::Blow {
            self.last_blow = Instant::now();
        }
        let kind = Cause::of(cause);
        // **Already down: the clock pays, not the health.** What the clock
        // is already counting costs nothing more; anything else takes time
        // off it, and what kills outright still does. See
        // `downed::Cause::shares_the_bill` for which is which.
        if let Some(down) = self.downed.as_mut() {
            if kind.profile().is_none() || amount >= OVERKILL {
                return self.die(cause);
            }
            if kind.shares_the_bill(down.cause) {
                return Outcome::Unchanged;
            }
            self.downed_dirty = true;
            if down.take(amount) {
                return self.die(cause);
            }
            return Outcome::Unchanged;
        }
        let before = self.health;
        self.health = (before - amount).max(0.0);
        if self.health <= 0.0 {
            // **Down rather than dead**, unless the cause has no crawl in it
            // or the blow went a whole bar past the last point. See
            // `primitive_shared::downed`.
            match Down::new(kind).filter(|_| amount - before < OVERKILL) {
                Some(down) => {
                    self.downed = Some(down);
                    self.downed_words = cause.to_string();
                    self.rescued = false;
                    self.downed_dirty = true;
                    // The fall that downed the body is over; a crawl off a
                    // ledge afterwards is a fall of its own.
                    self.fall_peak_y = None;
                }
                None => return self.die(cause),
            }
        }
        Outcome::Changed
    }

    /// The end, by `cause`: the one place `dead` is set.
    fn die(&mut self, cause: &str) -> Outcome {
        self.health = 0.0;
        self.dead = true;
        self.fall_peak_y = None;
        if self.downed.take().is_some() {
            self.downed_dirty = true;
        }
        self.rescued = false;
        self.downed_words.clear();
        Outcome::Died {
            cause: cause.to_string(),
        }
    }

    // ---- downed ----

    /// On the ground, and why, and how long is left. `None` standing or
    /// dead.
    pub fn downed(&self) -> Option<primitive_shared::downed::Down> {
        self.downed
    }

    pub fn is_downed(&self) -> bool {
        self.downed.is_some()
    }

    /// Whether the client has to be told about `downed`, and marks it told.
    pub fn take_downed_report(&mut self) -> bool {
        std::mem::take(&mut self.downed_dirty)
    }

    /// The act that `rescue` is has just been done to this body. Raises it
    /// on the next `step_downed` if it is the act the body was waiting for;
    /// anything else -- a bandage on a body downed by the cold -- is only
    /// what it always was.
    ///
    /// **On the next step rather than here**, so every rescue, an act or a
    /// state, gets up through the one door and is reported the one way.
    pub fn offer(&mut self, rescue: primitive_shared::downed::Rescue) {
        if self.downed.is_some_and(|down| down.rescue() == rescue) {
            self.rescued = true;
        }
    }

    /// One tick of lying on the ground: raised if what saves the body has
    /// happened or is true, dead if the clock has run out.
    pub fn step_downed(&mut self, dt: f32) -> Outcome {
        let Some(mut down) = self.downed else {
            return Outcome::Unchanged;
        };
        let met = self.rescued || down.rescue().met_by(self.body_c, self.breath >= BREATH_SECONDS);
        if met {
            self.stand(true);
            return Outcome::Changed;
        }
        let out = down.tick(dt);
        self.downed = Some(down);
        if out {
            let words = std::mem::take(&mut self.downed_words);
            return self.die(&words);
        }
        Outcome::Unchanged
    }

    /// Lets a downed body die now, rather than wait for the clock: the
    /// respawn key while on the ground. Nothing for a body that is not down.
    pub fn give_up(&mut self) -> Outcome {
        if self.downed.is_none() {
            return Outcome::Unchanged;
        }
        let words = std::mem::take(&mut self.downed_words);
        self.die(&words)
    }

    /// Up off the ground. With `raised`, on `downed::RAISED_HEALTH` -- the
    /// rescue -- and otherwise at whatever health the caller is about to
    /// set (a respawn, a heal, a profile).
    fn stand(&mut self, raised: bool) {
        if self.downed.take().is_none() {
            return;
        }
        self.downed_dirty = true;
        self.rescued = false;
        self.downed_words.clear();
        if raised {
            self.health = self.health.max(primitive_shared::downed::RAISED_HEALTH);
            // A body just off the ground does not start mending at once: the
            // same quiet time a blow buys (`REGEN_DELAY_SECS`).
            self.last_damage = Instant::now();
        }
    }

    /// Heals over time, once the player has been left alone long enough.
    /// One tick of breathing.
    ///
    /// `head_under` is whether the *eyes* are in liquid, which is the
    /// same question the client asks to decide whether to draw the
    /// underwater fog -- so what the player sees and what the server
    /// bills them for are the same thing. Standing chest-deep in a pond
    /// costs nothing, which is right: your head is out.
    ///
    /// The world decides, not the client: this is called from the tick
    /// loop against the server's own copy of the world at the server's
    /// own copy of the player's position.
    pub fn breathe(&mut self, head_under: bool, dt: f32) -> Outcome {
        if self.dead {
            return Outcome::Unchanged;
        }
        if !head_under {
            self.breath = BREATH_SECONDS;
            return Outcome::Unchanged;
        }
        self.breath -= dt;
        if self.breath > 0.0 {
            return Outcome::Unchanged;
        }
        // Out of air, and billed for the part of *this* tick that was
        // spent that way.
        //
        // Two cases, and `min` is the whole of the arithmetic. On the
        // tick the air runs out, `-breath` is the overshoot -- the
        // fraction of the tick after the lungs emptied -- and that is
        // what is charged, rather than the whole tick, which would take
        // a bite out of a player who surfaced almost in time. On every
        // tick after that `-breath` has grown past `dt` and the charge
        // is the whole tick, which is what makes a slow server drown
        // people at the same rate as a fast one.
        //
        // It read `dt.min(-self.breath).max(dt)`, which is `dt` -- the
        // `max` cancelled the `min` exactly. Harmless in the second
        // case, since the answer there is `dt` anyway, and wrong in the
        // first: a tick that was one per cent under water cost a full
        // tick of damage.
        let seconds = dt.min(-self.breath);
        self.hurt(DROWNING_PER_SECOND * seconds, "drowned")
    }

    /// One tick of breathing smoke: the air of a closed room with a fire in
    /// it (`wildfire::smoke_room`), `thickness` nought to one.
    ///
    /// **The drowning meter, run slower, and a gentler bill at the end of
    /// it.** Under `wildfire::SMOKE_CHOKES` the smoke is only something to
    /// see, and the breath refills as it does in open air. Over it the
    /// breath runs down -- at a quarter of the rate water takes it just
    /// over the line, at the whole rate in the thickest smoke -- so a
    /// player who wakes in a smoky hut has a minute to open the door, and
    /// the meter they already know from diving is what tells them. Out of
    /// breath, it costs a fifth of what drowning does: smoke drives a
    /// person out of a room far more often than it kills them there.
    ///
    /// A method beside `breathe` and not an argument to it, because water
    /// comes first: a head under water is drowning whatever the air above
    /// the surface is like.
    pub fn breathe_smoke(&mut self, thickness: f32, dt: f32) -> Outcome {
        use primitive_shared::wildfire::SMOKE_CHOKES;
        if self.dead {
            return Outcome::Unchanged;
        }
        if thickness.is_nan() || thickness < SMOKE_CHOKES {
            return self.breathe(false, dt);
        }
        let rate = ((thickness - SMOKE_CHOKES) / (1.0 - SMOKE_CHOKES)).clamp(0.25, 1.0);
        self.breath -= dt * rate;
        if self.breath > 0.0 {
            return Outcome::Unchanged;
        }
        let seconds = dt.min(-self.breath);
        self.hurt(SMOKE_PER_SECOND * seconds, "choked on smoke")
    }

    /// How much air is left, as a fraction. Drawn by the HUD.
    pub fn breath_fraction(&self) -> f32 {
        (self.breath / BREATH_SECONDS).clamp(0.0, 1.0)
    }

    /// A tick spent standing in fire.
    ///
    /// Nothing at all when they are not, and no cooldown when they are:
    /// unlike drowning there is no meter to run down first, because a
    /// fire is not something that creeps up on you. The counterpart of
    /// `breathe`, and it goes through `hurt` for the same reason
    /// everything does -- the death message, the plugin hook and the
    /// backpack all hang off that one path.
    pub fn burn(&mut self, in_fire: bool, dt: f32) -> Outcome {
        if !in_fire || dt <= 0.0 {
            return Outcome::Unchanged;
        }
        // ...and a burn on what was in it, which is the feet: a step in
        // and out is a light burn that goes by itself, and standing there
        // is one that will not (`injury::BURN_IN_FIRE_PER_SECOND`).
        if !self.dead {
            self.injuries.scorch(dt);
        }
        self.hurt(BURNING_PER_SECOND * dt, "burned to death")
    }

    pub fn regenerate(&mut self, dt: f32) -> Outcome {
        // **A downed body does not mend.** Health creeping back from nought
        // would be a body that got up on its own by lying still long enough,
        // and the whole of being down is that it does not.
        if self.dead || self.downed.is_some() || self.health >= MAX_HEALTH {
            return Outcome::Unchanged;
        }
        if self.last_damage.elapsed().as_secs_f32() < REGEN_DELAY_SECS {
            return Outcome::Unchanged;
        }
        // **Hungry players do not heal.** The first thing an empty
        // stomach takes, and the one most players meet long before they
        // meet starvation: you are not dying, you are simply not getting
        // better. It goes here rather than in the tick loop so that
        // every path into regeneration -- and there is only one, which
        // is the point -- has to pass it. The line is seven tenths of
        // a bar (`food::REGEN_THRESHOLD`), well above half: a body
        // mends on a surplus, not on a stomach that is merely not
        // empty.
        if self.nourishment_fraction() < food::REGEN_THRESHOLD {
            return Outcome::Unchanged;
        }
        // **Nor do players the cold or the heat is still taking from.**
        // A body past the freezing line is spending everything it has
        // on staying alive; a wound does not close in a snowdrift, it
        // closes by the fire afterwards. Stated here even though the
        // exposure damage goes through `hurt`, whose six-second delay
        // would catch most ticks of it anyway: a rule that holds only
        // because of the timing of another rule is a rule that breaks
        // the day somebody retunes the delay.
        if body::exposure_damage_per_second(self.body_c) > 0.0 {
            return Outcome::Unchanged;
        }
        // **Nor does a body that is busy being ill.** A stomach full of
        // pond water is the same argument the cold makes: what a wound
        // needs is a body with something spare, and this one has not.
        // It is also most of what makes bad water matter -- the health
        // it takes back is small, and the healing it stops is not.
        if self.sick_for > 0.0 {
            return Outcome::Unchanged;
        }
        // **Nor does a body with a wound open on it.** A cut still
        // bleeding, or a bad burn left undressed, is where everything a
        // body has spare is going -- and this is the half of an injury
        // that makes the bandage urgent rather than tidy: the blood a cut
        // takes is slow, and the healing it stops is all of it. See
        // `Injuries::stops_mending`.
        if self.injuries.stops_mending() {
            return Outcome::Unchanged;
        }
        // **And the diet decides how fast.** A body mends on what it is
        // given: everything from one group heals at a third of the
        // rate, two groups at two thirds, all three at full. See
        // `food::diet_regen_factor` for why it is three groups and why
        // the floor is a third rather than nothing.
        let factor = food::diet_regen_factor(self.diet_groups())
            // **And comfort, last.** A rate on top of the rates, never a
            // gate: see `comfort::SLOWEST_RECOVERY` for why the floor is a
            // half and not nothing.
            * comfort::recovery(self.comfort_level);
        // **And so does sleep.** A body mends while it rests, so one
        // that has not rested mends badly: past `body::TIRED_AT` the
        // rate falls away to `body::EXHAUSTED_REGEN`. It multiplies the
        // diet rather than replacing it -- a tired player on one food
        // group heals at a ninth of the rate, which is the honest
        // compound of two bad answers and the reason neither is fatal
        // on its own.
        self.health =
            (self.health + REGEN_PER_SECOND * factor * self.rest_regen_factor() * dt)
                .min(MAX_HEALTH);
        Outcome::Changed
    }

    /// How long since something last *struck* this player, in seconds.
    ///
    /// Read by the sleep gesture, which refuses while something is still
    /// hitting you. **Not the same clock the mending delay reads** -- it
    /// was, and that is why a bed answered a body freezing to death with
    /// "you cannot sleep while something is hurting you". See [`Harm`].
    pub fn last_blow_elapsed(&self) -> f32 {
        self.last_blow.elapsed().as_secs_f32()
    }

    /// Is the cold or the heat taking health right now?
    ///
    /// **The state, not a clock.** The bed asks this rather than "did
    /// anything hurt you in the last ten seconds", because the cold is not
    /// an event: a player standing in a blizzard is freezing in every tick
    /// and in none of them was anything done to them. Past
    /// `body::FREEZING` or `body::SCALDING`, which is the line where
    /// exposure starts costing health (`body::exposure_damage_per_second`)
    /// -- merely *chilled* is a fine reason to go to bed, and the bed is
    /// where a cold player ought to be.
    pub fn exposed_to_death(&self) -> bool {
        primitive_shared::body::exposure_damage_per_second(self.body_c) > 0.0
    }

    /// Every wound on this player: what the client is sent and what the
    /// save file keeps.
    pub fn injuries(&self) -> &Injuries {
        &self.injuries
    }

    /// Sets them. For loading a saved player, and repaired on the way in
    /// because the value comes off a file.
    pub fn set_injuries(&mut self, mut injuries: Injuries) {
        injuries.sanitize();
        self.injuries = injuries;
    }

    /// The roll for where the next blow or fall lands.
    fn next_roll(&mut self) -> f32 {
        self.blow_seed = self.blow_seed.wrapping_add(1);
        injury::roll(self.blow_seed)
    }

    /// What a blow leaves behind, `damage` of it having got through the
    /// armour. Answers where it landed.
    ///
    /// **Apart from `hurt`**, because most of what calls `hurt` is not a
    /// blow -- thirst, the cold, a bad mushroom -- and none of those
    /// leaves a mark on an arm. The caller that knows what struck says so:
    /// the server's `strike_player`.
    pub fn take_blow(&mut self, blow: injury::Blow, damage: f32) -> Option<injury::Part> {
        if self.dead {
            return None;
        }
        let roll = self.next_roll();
        self.injuries.take_blow(blow, damage, roll)
    }

    /// Dresses a wound on `part` with `block`. A refusal changes nothing
    /// (see `Injuries::treat`), so the caller spends the item on `Ok` and
    /// only then. A corpse is not treated.
    pub fn treat(
        &mut self,
        part: injury::Part,
        block: primitive_shared::types::BlockId,
    ) -> Result<injury::Kind, injury::Refusal> {
        if self.dead {
            return Err(injury::Refusal::NothingItHelps);
        }
        // **On a downed body, the rescue it is waiting for is never
        // refused.** The dressing goes on the part asked for if it suits it,
        // on any part it suits if not -- a player lying in the grass is not
        // aiming at the mannequin -- and if nothing on the body suits it at
        // all, it is spent binding what put them down. A bandage refused to a
        // body bleeding out because the bite was a bruise would be the game
        // arguing with somebody who has twenty seconds left.
        let rescue = self.downed.map(|down| down.rescue());
        let rescues = injury::Treatment::of(block)
            .zip(rescue)
            .is_some_and(|(treatment, rescue)| rescue.by_treatment(treatment));
        if !rescues {
            return self.injuries.treat(part, block);
        }
        let treated = self.injuries.treat(part, block).or_else(|refused| {
            injury::Part::ALL
                .iter()
                .find_map(|&other| self.injuries.treat(other, block).ok())
                .ok_or(refused)
        });
        let kind = treated.unwrap_or_else(|_| {
            injury::Treatment::of(block)
                .and_then(|treatment| treatment.suits().first().copied())
                .unwrap_or(injury::Kind::Cut)
        });
        if let Some(rescue) = rescue {
            self.offer(rescue);
        }
        Ok(kind)
    }

    /// One tick of wounds bleeding and mending.
    ///
    /// `asleep` mends them six times as fast -- a set leg takes half an
    /// hour up and a night or two lying down, which is what makes a bed
    /// the second half of the answer to a cliff. See `Injuries::step`.
    ///
    /// **The outcome has to be reported.** Bleeding is the one thing here
    /// that can kill, and an outcome dropped on the floor is a player who
    /// reaches zero and walks on: the starving sleeper who could not die
    /// was exactly that.
    ///
    /// Which wounds closed is not handed back. The server used to say "your
    /// leg has mended" in the chat, and the client says it now, in the
    /// player's own language, off the change in the `Injuries` it is sent --
    /// two voices announcing one knitted leg is one too many.
    pub fn mend(&mut self, dt: f32, asleep: bool) -> Outcome {
        if self.dead || dt <= 0.0 {
            return Outcome::Unchanged;
        }
        let mending = self.injuries.step(dt, asleep);
        if mending.blood > 0.0 {
            self.lose(mending.blood, "bled to death", Harm::Bleeding)
        } else {
            Outcome::Unchanged
        }
    }

    /// How many drops of blood this body shows this tick.
    ///
    /// **The only blood a body sheds on its own, and it comes from the cuts
    /// and nothing else.** Illness, hunger, thirst, the cold and a fire take
    /// health through `hurt` and leak nothing (`Injuries::drips_per_second`);
    /// a blow's spray is `Blow::drops`, sent where the blow lands. What was
    /// here before was nothing on the server and a burst on the client for
    /// every point of health that went, which is how a raw fish became a
    /// fountain.
    ///
    /// **Owed rather than rolled**, for the reason the client's hearths are
    /// counted down: the rate in the constant is the rate on the screen at any
    /// tick rate. Capped per tick so a stalled tick shows a drop or two, not a
    /// spray of everything it owed.
    pub fn drips(&mut self, dt: f32) -> u8 {
        let rate = self.injuries.drips_per_second();
        if self.dead || rate <= 0.0 || !dt.is_finite() || dt <= 0.0 {
            self.drip_owed = 0.0;
            return 0;
        }
        self.drip_owed += rate * dt.min(1.0);
        let whole = self.drip_owed.floor().min(2.0);
        self.drip_owed = (self.drip_owed - whole).min(1.0);
        whole as u8
    }

    /// Whether the client's picture of the body is out of date against
    /// what it was last sent. See `Injuries::worth_reporting`.
    pub fn needs_injury_report(&self, last: &Injuries) -> bool {
        self.injuries.worth_reporting(last)
    }

    /// What a broken arm leaves of a swing, at a person or at an animal.
    /// See `injury::BROKEN_ARM_STRENGTH`.
    pub fn strength_factor(&self) -> f32 {
        self.injuries.strength_factor()
    }

    /// How tired the player is, 0..1.
    pub fn fatigue(&self) -> f32 {
        self.fatigue
    }

    /// Sets it. For loading a saved player and for nothing else -- the
    /// value is clamped, because it comes off a file.
    pub fn set_fatigue(&mut self, value: f32) {
        self.fatigue = value.clamp(0.0, 1.0);
    }

    /// One tick of being awake.
    ///
    /// A flat rate: tiredness is a clock, not a bill for effort. That is
    /// deliberate and it is the difference between this meter and
    /// hunger -- hunger is what you *spend*, tiredness is what the day
    /// costs whatever you do with it, and a version that charged for
    /// sprinting would be a second hunger bar with a different picture.
    ///
    /// A dead player does not get tired, which sounds like a joke and is
    /// the fix for a real thing: a corpse waiting on the death screen
    /// would otherwise wake up exhausted.
    pub fn tire(&mut self, dt: f32) -> Outcome {
        if self.dead || dt <= 0.0 {
            return Outcome::Unchanged;
        }
        let before = self.fatigue;
        self.fatigue = (self.fatigue + dt / body::WAKING_SECONDS).min(1.0);
        if (self.fatigue - before).abs() > f32::EPSILON {
            Outcome::Changed
        } else {
            Outcome::Unchanged
        }
    }

    /// One tick of resting, at `per_second` of the meter a second.
    ///
    /// Both sleeping and sitting come through here, at their own rates
    /// (`body::SLEEP_RECOVERY_PER_SECOND` and
    /// `body::SITTING_RECOVERY_PER_SECOND`), because the difference
    /// between them is how fast and nothing else. `floor` is what this
    /// kind of rest cannot take you below -- straw leaves a fifth of
    /// the night behind; see `body::Rest::recovery`.
    pub fn rest(&mut self, dt: f32, per_second: f32, floor: f32) -> Outcome {
        if self.dead || dt <= 0.0 {
            return Outcome::Unchanged;
        }
        let before = self.fatigue;
        self.fatigue = (self.fatigue - per_second * dt).max(floor).max(0.0);
        if (self.fatigue - before).abs() > f32::EPSILON {
            Outcome::Changed
        } else {
            Outcome::Unchanged
        }
    }

    /// What tiredness does to healing, as a multiplier.
    ///
    /// Nothing at all until `body::TIRED_AT`, then straight down to
    /// `body::EXHAUSTED_REGEN` at the top of the scale. Ramped rather
    /// than switched, so a player watching their health crawl can see
    /// it get worse and connect it to the meter that is filling.
    pub fn rest_regen_factor(&self) -> f32 {
        tiredness_factor(self.fatigue, body::EXHAUSTED_REGEN)
    }

    /// ...and to how fast they move.
    ///
    /// The same ramp against `body::EXHAUSTED_SPEED`. Read by the
    /// server when it tells a client its speed, so a tired player walks
    /// slower on their own screen rather than being corrected into
    /// place by the anti-cheat -- which is what "the server owns the
    /// truth" has to mean for anything a player feels.
    pub fn rest_speed_factor(&self) -> f32 {
        // **A break multiplies rather than replaces.** A tired player
        // with a broken leg is slower than either on its own, which is
        // the honest compound and also the reading a player expects:
        // nothing here cancels anything else out.
        let limp = self.injuries.speed_factor();
        tiredness_factor(self.fatigue, body::EXHAUSTED_SPEED) * limp
    }

    /// How badly this player walks, 0 sound to 1 barely walking: the number
    /// every other client draws the gait from. See `protocol::PlayerState::limp`.
    ///
    /// **The same three facts `rest_speed_factor` already multiplies, read as
    /// a shape instead of as a pace.** A limp is not a fourth meter to keep in
    /// step with the others -- it is what the ones there are already look like
    /// from outside, and deriving it here is what stops a player from being
    /// drawn hobbling while walking at full speed, or the other way about.
    ///
    /// A broken leg is most of it (`FRACTURE_SPEED` is the deepest single
    /// thing that happens to a walk), exhaustion is the rest, and a body down
    /// to its last few points of health drags whatever else is true -- that
    /// last one is the only reason a player with no injuries at all ever
    /// limps, and it is the one a watcher most needs to see, because it is the
    /// player about to die beside them.
    ///
    /// Rejected: **limping off `rest_speed_factor` itself**, which is one line
    /// shorter and wrong the moment anything else multiplies into that factor
    /// -- a heavy pack, a swamp, a slope. Carrying a load is not a limp.
    pub fn limp(&self) -> f32 {
        let broken: f32 = if self.injuries.leg_broken() { 0.7 } else { 0.0 };
        // Nothing until the meter is into the tired band, then up to a third
        // at the end of it: the same ramp the speed takes, so the stagger
        // arrives with the slowdown a player can already feel.
        let tired = (1.0 - tiredness_factor(self.fatigue, 0.0)) * 0.35;
        // The last fifth of the health bar, and only that: a scratch is not a
        // limp, and a player at two points out of twenty is not walking well
        // whatever the reason.
        let failing = (1.0 - self.health / (MAX_HEALTH * 0.2)).clamp(0.0, 1.0) * 0.45;
        broken.max(tired).max(failing).clamp(0.0, 1.0)
    }

    /// Whether the limp is on the left leg: see `protocol::PlayerState::limp_left`.
    ///
    /// **Only a fracture has a side.** Tiredness and a failing body are not
    /// one leg's fault, and they limp on the right, as everything did before
    /// the side was sent -- so this is `false` unless the left leg, and only
    /// the left, is the broken one.
    pub fn limp_left(&self) -> bool {
        self.injuries.broken_leg() == Some(injury::Part::LeftLeg)
    }

    /// How many of the three food groups this player has eaten from
    /// recently. See `food::DIET_MEMORY_SECS`.
    pub fn diet_groups(&self) -> usize {
        self.last_of_group
            .iter()
            .filter(|last| {
                last.is_some_and(|at| at.elapsed().as_secs_f32() <= food::DIET_MEMORY_SECS)
            })
            .count()
    }

    // ---- hunger ----

    /// How full the player is, as a fraction. What the HUD draws.
    pub fn nourishment_fraction(&self) -> f32 {
        (self.nourishment / food::MAX_NOURISHMENT).clamp(0.0, 1.0)
    }

    pub fn nourishment(&self) -> f32 {
        self.nourishment
    }

    /// Restores a stored value on join, with the same suspicion
    /// `set_health` shows: it comes off a file an operator can edit, and
    /// a `NaN` stomach is a player who can neither starve nor eat.
    ///
    /// Unlike health, zero is read as zero rather than as "start
    /// fresh": a player who logged out starving should log back in
    /// starving. What a fresh profile gets is `MAX_NOURISHMENT`, and
    /// that is decided by whoever *has* no stored value rather than
    /// here.
    pub fn set_nourishment(&mut self, value: f32) {
        self.nourishment = if value.is_finite() {
            value.clamp(0.0, food::MAX_NOURISHMENT)
        } else {
            food::MAX_NOURISHMENT
        };
        self.last_food_reported = self.nourishment;
    }

    /// Spends nourishment, and takes health once there is none left.
    ///
    /// `effort` is what the player was *doing* this tick, and it is the
    /// whole mechanic: idling is nearly free and work is not (see
    /// `food::IDLE_DRAIN_PER_SECOND` and its neighbours). The caller is
    /// the tick loop, which is the only place that knows.
    ///
    /// Returns a health outcome rather than a hunger one, because
    /// hunger by itself is never worth interrupting anybody about --
    /// what the caller has to react to is somebody starving to death.
    /// Whether the *bar* needs sending is `needs_food_report`.
    pub fn digest(&mut self, effort: Effort, dt: f32) -> Outcome {
        if self.dead || dt <= 0.0 {
            return Outcome::Unchanged;
        }
        // **Shivering is work.** The first thing a cold player notices
        // is not damage, it is that their food is going faster -- which
        // is what turns a cold night into a reason to build a fire
        // without ever showing a damage number. See
        // `body::shiver_hunger_multiplier`.
        let shivering = body::shiver_hunger_multiplier(self.body_c);
        // The load is the vitals' own copy, kept by `set_carried_weight`
        // -- the same number the fall arithmetic uses, so a player who
        // is too heavy to float is too heavy for exactly one reason.
        // **Slept at home**: a quarter off, for the half day after a night in
        // a bed by a lit fire (`comfort::rests_at_home`). Counted down here,
        // the one place every second of hunger passes through.
        let rested = if self.rested_for > 0.0 { primitive_shared::comfort::RESTED_HUNGER } else { 1.0 };
        self.rested_for = (self.rested_for - dt).max(0.0);
        let drain = effort.drain(dt, self.carried_kg) * shivering * rested;
        let before = self.nourishment;
        self.nourishment = (before - drain).max(0.0);
        if self.nourishment > 0.0 {
            self.starving_owed = 0.0;
            return Outcome::Unchanged;
        }
        // Nothing left to take from, so it comes out of health -- for the
        // part of the step the stomach was really empty (`empty_for`), a
        // roll per five seconds of it (`food::STARVATION_ROLL_SECONDS`).
        self.starving_owed += Self::empty_for(before, drain, dt);
        let mut taken = 0.0;
        while self.starving_owed >= food::STARVATION_ROLL_SECONDS {
            self.starving_owed -= food::STARVATION_ROLL_SECONDS;
            self.hunger_rolls = self.hunger_rolls.wrapping_add(1);
            let seed = self.hunger_seed ^ self.hunger_rolls.wrapping_mul(0x9E37_79B9_7F4A_7C15);
            if injury::roll(seed) < food::STARVATION_CHANCE {
                taken += food::STARVATION_DAMAGE;
            }
        }
        self.ail(taken, "starved")
    }

    /// **A night slept at home**: rested for `comfort::RESTED_SECONDS` from
    /// now, whatever was left of the last one -- two nights at home are not
    /// a day and a half of it.
    pub fn rest_at_home(&mut self) {
        self.rested_for = primitive_shared::comfort::RESTED_SECONDS;
    }

    /// Seconds of the last night at home still keeping the hunger down.
    pub fn rested_for(&self) -> f32 {
        self.rested_for
    }

    /// Fixes the starvation rolls, for a test that compares two bodies
    /// going hungry.
    #[cfg(test)]
    pub fn set_hunger_seed(&mut self, seed: u64) {
        self.hunger_seed = seed;
    }

    /// How much of a step of `dt` seconds a meter that held `before` spent
    /// at empty, when the step drew `spent` out of it at an even rate.
    ///
    /// **A long step has to cost what the same hours cost a tick at a
    /// time.** Hunger and thirst billed the whole step the moment the
    /// meter reached zero anywhere inside it -- nothing at a twentieth of a
    /// second, and fatal for a night slept through, which the server
    /// charges as one step of several hundred seconds
    /// (`sleep_through_to_dawn`). A player who lay down with a few
    /// mouthfuls left was billed the whole night at empty and woke dead
    /// from full health. `breathe` already charged only the overshoot; this
    /// is the same arithmetic for a meter that drains at a rate.
    fn empty_for(before: f32, spent: f32, dt: f32) -> f32 {
        if before <= 0.0 || spent <= 0.0 {
            return dt;
        }
        (dt * (1.0 - before / spent)).clamp(0.0, dt)
    }

    /// Charges for a jump, which is an event rather than a rate.
    pub fn jumped(&mut self) {
        if !self.dead {
            self.nourishment = (self.nourishment - food::JUMP_DRAIN).max(0.0);
        }
    }

    /// Eats one of something, if it is food and if there is room.
    ///
    /// Returns whether the item should be spent. False for anything that
    /// is not food and for a stomach with no room -- a haunch of meat
    /// eaten at full is an item destroyed, which is the same class of
    /// bug as a craft that consumes its ingredients and produces
    /// nothing. See `food::worth_eating`.
    pub fn eat(&mut self, block: primitive_shared::types::BlockId) -> Outcome {
        self.eat_made(block, primitive_shared::quality::Quality::PLAIN)
    }

    /// The same mouthful, out of something somebody cooked.
    ///
    /// **A second entry point rather than an argument on the first**, for
    /// `crafting::craft_made`'s reason: `eat` is the mod ABI's signature
    /// and the one a dozen tests call, and none of them is about who did
    /// the cooking. An unjudged mouthful is worth exactly what it always
    /// was (`quality::PLAIN_FRACTION`).
    pub fn eat_made(
        &mut self,
        block: primitive_shared::types::BlockId,
        quality: primitive_shared::quality::Quality,
    ) -> Outcome {
        // **A coconut is worth eating on a full stomach if the throat is
        // dry**, because what it is for is its water (`food::water_in`). The
        // rule is still that nothing is spent for nothing: full of food and
        // full of water, the nut stays in the pack.
        let water = food::water_in(block).unwrap_or(0.0);
        let thirsty = water > 0.0 && self.hydration < body::MAX_HYDRATION - 1.0;
        // ...and mead is worth drinking on a full stomach if the body is
        // cold, for the coconut's reason: what it is for is not the food.
        let cold = food::warmth_in(block).is_some() && self.body_c < body::COMFORT_LOW;
        if self.dead || !(food::worth_eating(self.nourishment, block) || thirsty || cold) {
            return Outcome::Unchanged;
        }
        if water > 0.0 {
            // Clean: the nut's own water, never a pond's. `drink` refuses a
            // full player, and that is fine -- the food still went down.
            self.drink(water);
        }
        let before = self.nourishment;
        self.nourishment = food::after_eating_made(self.nourishment, block, quality);
        // What went in is what has to come out, some while later. Measured
        // as the bar filled rather than per mouthful, so a berry is a berry's
        // worth and a haunch a haunch's.
        self.dung_owed += ((self.nourishment - before) / food::MAX_NOURISHMENT).max(0.0);
        // **What was eaten, for the diet.** Recorded here rather than at
        // the call site because this is the one function a mouthful
        // passes through -- the mod ABI's `eat` comes here too -- and a
        // diet that a mod could feed a player round would be a diet
        // nobody could rely on. See `diet_regen_factor`.
        if let Some(group) = food::group(block) {
            self.last_of_group[group as usize] = Some(Instant::now());
        }
        // **A bad mushroom is eaten and then paid for.** The stomach has
        // already been emptied by `after_eating`; what is left is the
        // health, and it goes through `hurt` like everything else that
        // takes any -- so it announces itself, drops the pack and fires
        // the hooks if it kills, which is the whole reason nothing here
        // ever writes `health` directly.
        // **And what follows the mouthful.** Raw flesh, a toadstool and
        // meat that has gone off all leave a player ill for a while --
        // the same illness stale water leaves, on purpose, because a
        // player who has learnt what a bad river feels like should
        // recognise bad meat without being told. See
        // `food::sickness_seconds` for why cooking had to stop being an
        // optimisation and start being a decision.
        //
        // Taken as the longer of the two rather than added: two raw
        // haunches are one bad afternoon, not two.
        let illness = food::sickness_seconds(block);
        if illness > 0.0 {
            self.swallow_illness(illness);
        }
        // **And a jug of mead warms whoever drinks it**, at once and up to
        // comfortable (`food::warmth_in`).
        if let Some(warmth) = food::warmth_in(block) {
            self.warm_by(warmth);
        }
        // Anything that went down raises a body downed by hunger -- even the
        // toadstool, whose harm below comes out of the body it raised.
        self.offer(primitive_shared::downed::Rescue::Food);
        if let Some(harm) = food::harm(block) {
            return self.ail(harm.health, "ate something they should not have");
        }
        Outcome::Changed
    }

    /// Sets when a group was last eaten. Tests and the mod ABI only:
    /// nothing a player does comes through here, and a diet that could
    /// be set rather than eaten would be a diet nobody had to keep.
    #[cfg(test)]
    pub fn ate_group(&mut self, group: food::Group, at: Instant) {
        self.last_of_group[group as usize] = Some(at);
    }

    /// Is the client's copy of the hunger bar out of date?
    ///
    /// A twentieth, which is one segment of a bar drawn the width of the
    /// health gauge: anything finer is a message a player cannot see the
    /// effect of. Without a threshold this would be a packet per player
    /// per tick, forever, for a number that takes forty minutes to cross
    /// the bar.
    pub fn needs_food_report(&self) -> bool {
        (self.nourishment_fraction() - self.last_food_reported / food::MAX_NOURISHMENT).abs()
            > 0.05
    }

    pub fn mark_food_reported(&mut self) {
        self.last_food_reported = self.nourishment;
    }

    // ---- comfort ----

    /// The hidden comfort value. See `primitive_shared::comfort`.
    pub fn comfort_level(&self) -> f32 {
        self.comfort_level
    }

    /// What comfort does to regeneration and stamina recovery right now.
    /// The one number of it the client is told, because stamina is the
    /// client's to predict.
    pub fn recovery(&self) -> f32 {
        comfort::recovery(self.comfort_level)
    }

    pub fn grime(&self) -> f32 {
        self.grime
    }

    /// Dirt on the body from work: digging, clearing dung. Clamped, and a
    /// nonsense amount is none.
    pub fn soil(&mut self, amount: f32) {
        if amount.is_finite() && amount > 0.0 {
            self.grime = (self.grime + amount).min(1.0);
        }
    }

    /// One tick of comfort: the grime washed or muddied, then the value
    /// settled towards what this body in this place is worth.
    ///
    /// `place` is the server's last survey, `smoke` the thickness at the
    /// eyes, `in_water` and `rained_on` the climate's own sample -- the same
    /// ones the wetness is stepped from, so being washed and being wet can
    /// never disagree.
    pub fn settle_comfort(
        &mut self,
        place: comfort::Surroundings,
        asleep: bool,
        smoke: f32,
        in_water: bool,
        rained_on: bool,
        dt: f32,
    ) {
        if self.dead {
            return;
        }
        self.grime = comfort::step_grime(self.grime, dt, in_water, rained_on, place.on_mud);
        let body = comfort::Condition {
            body_c: self.body_c,
            fatigue: self.fatigue,
            asleep,
            grime: self.grime,
            wetness: self.wetness,
            bleeding: self.injuries.is_bleeding(),
            smoke,
        };
        self.comfort_level = comfort::step(self.comfort_level, comfort::target(body, place), dt);
    }

    /// Whether this body leaves dung now, in a place this enclosed. Asking
    /// spends nothing: `went` does, once the pat is actually on the ground,
    /// so a body standing where there is nowhere to put one simply goes
    /// on waiting.
    pub fn goes_now(&self, enclosure: f32) -> bool {
        !self.dead && comfort::goes_now(self.dung_owed, enclosure)
    }

    /// The pat has been left.
    pub fn went(&mut self) {
        self.dung_owed = (self.dung_owed - comfort::DUNG_PER_BAR).max(0.0);
    }

    // ---- warmth ----

    /// Skin temperature, in degrees.
    pub fn temperature(&self) -> f32 {
        self.body_c
    }

    /// What that reads as. See `body::Comfort`.
    pub fn comfort(&self) -> body::Comfort {
        body::Comfort::of(self.body_c)
    }

    /// How wet the player is, 0..1.
    pub fn wetness(&self) -> f32 {
        self.wetness
    }

    /// Restores stored values on join, with the same suspicion
    /// `set_health` shows: they come off a file an operator can edit,
    /// and a `NaN` body temperature is a player who can never be warm
    /// and never be cold.
    /// Lifts a cold body by `degrees`, never past the top of comfortable and
    /// never down: what a jug of mead does (`food::warmth_in`).
    ///
    /// **Capped at `body::COMFORT_HIGH` and not at neutral**, so a player
    /// who is merely cool feels the whole of it; and a player already over
    /// the line is left where they are, because a drink that cooled a hot
    /// body would be a second rule nobody asked for.
    pub fn warm_by(&mut self, degrees: f32) {
        if self.dead || !degrees.is_finite() || degrees <= 0.0 {
            return;
        }
        let ceiling = self.body_c.max(body::COMFORT_HIGH);
        self.body_c = (self.body_c + degrees).min(ceiling);
    }

    pub fn set_warmth(&mut self, body_c: f32, wetness: f32) {
        self.body_c = if body_c.is_finite() {
            body_c.clamp(body::MIN_BODY_C, body::MAX_BODY_C)
        } else {
            body::NEUTRAL_C
        };
        self.wetness = if wetness.is_finite() {
            wetness.clamp(0.0, 1.0)
        } else {
            0.0
        };
    }

    /// One tick of the world pulling on the player's temperature.
    ///
    /// Takes what the world is doing (`exposure`: the air, the sun on
    /// bare skin, and whether they are in water -- see `body::Exposure`),
    /// what the player is wearing (`insulation`, and the `shade` loose
    /// cloth casts in hot air and against the sun -- see
    /// `body::felt_under_sky`) and how long for, and hands back a health
    /// outcome -- which is `Unchanged` for all but the ends of the
    /// scale, because being cold is not being hurt until it is.
    ///
    /// **The wetness is stepped here too**, because it is an input to
    /// the same sum and stepping it anywhere else would mean two places
    /// that have to agree about the order. `getting_wet` and `in_water`
    /// come from the same climate sample `exposure` does.
    pub fn warm(
        &mut self,
        exposure: body::Exposure,
        insulation: f32,
        shade: f32,
        wetness_now: f32,
        dt: f32,
    ) -> Outcome {
        if self.dead || dt <= 0.0 || !dt.is_finite() {
            return Outcome::Unchanged;
        }
        self.wetness = if wetness_now.is_finite() {
            wetness_now.clamp(0.0, 1.0)
        } else {
            self.wetness
        };
        let target = body::felt_under_sky(exposure, insulation, shade, self.wetness);
        let rate = body::adjust_rate_under_sky(exposure, insulation, shade, self.wetness);
        self.body_c = body::step_temperature(self.body_c, target, rate, dt);

        let damage = body::exposure_damage_per_second(self.body_c) * dt;
        if damage <= 0.0 {
            return Outcome::Unchanged;
        }
        // Two causes rather than one, because the death screen is the
        // one place the player is told what actually happened -- and
        // "died of exposure" in a desert reads as a bug.
        let cause = if self.body_c < body::FREEZING {
            "froze to death"
        } else {
            "died of heatstroke"
        };
        self.ail(damage, cause)
    }

    // ---- water ----

    /// How much water is left, as a fraction. What the HUD draws.
    pub fn hydration_fraction(&self) -> f32 {
        (self.hydration / body::MAX_HYDRATION).clamp(0.0, 1.0)
    }

    pub fn hydration(&self) -> f32 {
        self.hydration
    }

    /// Restores a stored value on join, on the same terms as
    /// `set_nourishment`: zero is read as zero, because a player who
    /// logged out parched should log back in parched.
    pub fn set_hydration(&mut self, value: f32) {
        self.hydration = if value.is_finite() {
            value.clamp(0.0, body::MAX_HYDRATION)
        } else {
            body::MAX_HYDRATION
        };
    }

    /// Spends water, and takes health once there is none left.
    ///
    /// The shape of `digest`, deliberately: the tick loop already works
    /// out what a player is doing and should not have to learn a second
    /// vocabulary to bill them for it. What is different is the
    /// multiplier -- heat drives thirst, which is the one coupling
    /// between the two meters this module owns.
    pub fn drink_down(&mut self, exertion: body::Exertion, dt: f32) -> Outcome {
        if self.dead || dt <= 0.0 || !dt.is_finite() {
            return Outcome::Unchanged;
        }
        let rate = exertion.thirst_per_second() * body::thirst_multiplier(self.body_c);
        let before = self.hydration;
        self.hydration = (before - rate * dt).max(0.0);
        if self.hydration > 0.0 {
            return Outcome::Unchanged;
        }
        // Only the seconds spent dry, for the reason `digest` gives.
        self.ail(
            body::DEHYDRATION_PER_SECOND * Self::empty_for(before, rate * dt, dt),
            "died of thirst",
        )
    }

    /// Drinks. Returns whether anything actually happened.
    ///
    /// False for a player who is already full, on exactly the rule
    /// `eat` follows: a jug emptied at full hydration is an item
    /// destroyed, which is the same class of bug as a craft that
    /// consumes its ingredients and produces nothing.
    pub fn drink(&mut self, amount: f32) -> bool {
        if self.dead || amount <= 0.0 || !amount.is_finite() {
            return false;
        }
        // A sip of room at the top, so a player at 99% can still empty a
        // jug they are carrying rather than being told no by a
        // rounding error. Deliberately small: the rule is still that
        // drinking at full does nothing.
        if self.hydration >= body::MAX_HYDRATION - 1.0 {
            return false;
        }
        self.hydration = (self.hydration + amount).min(body::MAX_HYDRATION);
        // A drink raises a body downed by thirst or by a bad stomach. Here,
        // in the one function every drink passes through -- a jug, a river,
        // a coconut -- and never the sea, which does not reach it.
        self.offer(primitive_shared::downed::Rescue::Water);
        true
    }

    /// A mouthful from a particular kind of water.
    ///
    /// **The one place the kind of water is charged for**, so a jug and
    /// a hand cupped in a river cannot end up with different rules. A
    /// mouthful of the sea *takes* water rather than giving it (see
    /// `body::Water::hydration`), and anything but a running stream
    /// leaves the player ill for a while.
    ///
    /// Answers whether the drink happened at all: false for a player
    /// with no room, exactly as `drink` does -- except from the sea,
    /// which a full player can always be foolish enough to swallow.
    pub fn drink_water(&mut self, kind: body::Water, mouthful: f32) -> bool {
        if self.dead {
            return false;
        }
        let amount = kind.hydration(mouthful);
        if amount > 0.0 {
            if !self.drink(amount) {
                return false;
            }
        } else {
            // Salt: it comes *out* of the meter, and there is always
            // room to be made worse off.
            self.hydration = (self.hydration + amount).max(0.0);
        }
        let sickness = kind.sickness_seconds();
        // A chance, not a certainty -- see `body::WATER_ILLNESS_CHANCE`.
        if sickness > 0.0 && self.next_roll() < body::WATER_ILLNESS_CHANCE {
            self.swallow_illness(sickness);
        }
        true
    }

    /// Something that will make this player ill has gone down, and will
    /// start to in `body::DIGESTION_SECONDS`.
    ///
    /// The longer of two illnesses rather than the sum: being ill twice
    /// over is still being ill, and stacking would make a second sip of a
    /// pond worse than the first mistake. **The first clock is kept**: a
    /// second bad mouthful does not push back the first one's onset, or
    /// drinking steadily from a pond would postpone the illness for ever.
    fn swallow_illness(&mut self, seconds: f32) {
        if self.brewing <= 0.0 {
            self.brewing_in = body::DIGESTION_SECONDS;
        }
        self.brewing = self.brewing.max(seconds);
    }

    /// The whole illness this body is carrying, felt or not: what a profile
    /// saves. See the note on `brewing`.
    pub fn illness_owed(&self) -> f32 {
        self.sick_for.max(self.brewing)
    }

    /// Makes the next roll a fixed one, for a test that needs to know
    /// whether a mouthful makes somebody ill.
    #[cfg(test)]
    pub fn set_roll_seed(&mut self, seed: u64) {
        self.blow_seed = seed;
    }

    /// A seed whose next roll catches an illness, for the tests that are
    /// about what the illness does rather than whether it comes.
    #[cfg(test)]
    pub fn catch_the_next_illness(&mut self) {
        let seed = (0u64..).find(|s| injury::roll(s + 1) < body::WATER_ILLNESS_CHANCE).unwrap_or(0);
        self.blow_seed = seed;
    }

    /// Lets what was swallowed arrive at once, for a test that is about the
    /// illness and not the wait for it. Not behind `cfg(test)`: the
    /// end-to-end tests reach it through `ServerHandle::digest_player_meal`,
    /// and they build the library as a player does.
    pub fn digest_now(&mut self) {
        if self.brewing > 0.0 {
            self.brewing_in = 0.0;
            self.sicken(0.0);
        }
    }

    /// How much longer this player is ill for, in seconds.
    pub fn sick_for(&self) -> f32 {
        self.sick_for
    }

    /// Puts an illness back, on a body rebuilt from a file.
    ///
    /// Its own setter for the reason `set_fatigue` is one: the value comes
    /// off disk, and a player who logged out ill has to come back ill --
    /// see `profiles::Profile::sick_for` for the exploit that closes.
    pub fn set_sick_for(&mut self, seconds: f32) {
        self.sick_for = seconds.max(0.0);
    }

    /// Venom in the blood: ill for `seconds` more, starting now.
    ///
    /// **Not `swallow_illness`**, which waits `body::DIGESTION_SECONDS` for a
    /// stomach to turn: a sting is felt at once, and venom that arrived two
    /// minutes after the hive would be a lesson taught to the wrong walk.
    /// **Added, not the longer of the two**, unlike a second bad mouthful:
    /// every sting is venom of its own, and six are worse than one. The
    /// caller keeps the total short (`bees::VENOM_SECONDS`).
    pub fn envenom(&mut self, seconds: f32) {
        if self.dead || seconds <= 0.0 {
            return;
        }
        self.sick_for += seconds;
    }

    /// One tick of being ill from bad water.
    ///
    /// Its own step rather than a term inside `regenerate`, because the
    /// two are different facts: sickness *takes* health, and it also
    /// stops the healing (see `regenerate`), and a rule that only
    /// stopped the healing would be invisible to a player at full.
    pub fn sicken(&mut self, dt: f32) -> Outcome {
        if self.dead {
            return Outcome::Unchanged;
        }
        // What was swallowed arrives when its time is up.
        if self.brewing > 0.0 {
            self.brewing_in -= dt.max(0.0);
            if self.brewing_in <= 0.0 {
                self.sick_for = self.sick_for.max(self.brewing);
                self.brewing = 0.0;
                self.brewing_in = 0.0;
            }
        }
        if self.sick_for <= 0.0 || dt <= 0.0 {
            return Outcome::Unchanged;
        }
        let spent = dt.min(self.sick_for);
        self.sick_for -= spent;
        self.ail(body::SICKNESS_PER_SECOND * spent, "drank bad water")
    }

    /// Is the client's copy of the three meters out of date?
    ///
    /// A degree of temperature, a twentieth of the water bar or a
    /// fiftieth of the tiredness bar, which are all about a pixel of
    /// what is drawn. Without a threshold this would be a packet per
    /// player per tick forever, for numbers that take minutes to move.
    ///
    /// **Tiredness is on this gate and not on the health one**, and the
    /// difference is not tidiness. Health messages are *events* -- a
    /// blow, a fall, a mouthful of bad water -- and the first version of
    /// this sent one every tick, because tiredness changes every tick.
    /// Five integration tests went red at once, all of them waiting for
    /// "the next health message" and all of them handed a tick of
    /// tiredness instead: the fall never seemed to hurt, the punch never
    /// seemed to land. A gauge belongs with the gauges.
    pub fn needs_body_report(&self, last: (f32, f32, f32)) -> bool {
        (self.body_c - last.0).abs() > 1.0
            || (self.hydration_fraction() - last.1).abs() > 0.05
            || (self.fatigue - last.2).abs() > 0.02
            // ...and always when the *word* changes, however small the
            // step that crossed the threshold was. The client draws the
            // gauge from the number and the message from the word, and a
            // player who has just started shivering should be told
            // immediately rather than a degree later.
            || body::Comfort::of(self.body_c) != body::Comfort::of(last.0)
    }

    /// Back to full, alive, and with no fall in progress.
    pub fn respawn(&mut self) {
        self.breath = BREATH_SECONDS;
        self.health = MAX_HEALTH;
        // ...and a full stomach. Respawning starving would mean a player
        // who died *of* hunger comes back with two minutes to live and
        // no food, which is not a death penalty, it is a loop.
        //
        // Deliberately not touching `last_food_reported`, for the same
        // reason health does not: the client has to be told, and
        // pretending it already knows leaves a fresh spawn showing an
        // empty bar.
        self.nourishment = food::MAX_NOURISHMENT;
        // ...and warm, dry and watered, for the same reason: a player
        // who died *of* thirst coming back parched with no water in
        // reach is not a death penalty, it is a loop.
        self.body_c = body::NEUTRAL_C;
        self.wetness = 0.0;
        self.hydration = body::MAX_HYDRATION;
        // **...and in one piece.** A body that comes back healed comes
        // back healed: a player who respawned still limping was
        // carrying an injury out of a life that had ended -- and, worse,
        // one they could do nothing about, because the leg that broke
        // was the leg of a corpse. The profile has cleared it since it
        // was written (see `profiles`, which mends the dead on the way
        // in); this is the same rule for the player who never left -- and
        // it is every wound now, not only the leg.
        self.injuries = Injuries::default();
        // ...and rested. Dying is not a night's sleep, but a fresh body
        // at the spawn point with an empty pack is punishment enough
        // without also being a tired one.
        self.fatigue = 0.0;
        // ...and clean, and nobody in particular about comfort: the place
        // they wake in decides that within half a minute.
        self.grime = 0.0;
        self.comfort_level = 0.0;
        // ...and well. An illness belongs to the body that caught it, and
        // that body is the one that died. `sick_for` used to outlive the
        // respawn -- a dead body's clock does not run (`sicken` returns at
        // once for the dead), so it came back exactly as it was -- and a
        // player killed by pond water was met at the spawn point by the
        // rest of the same illness: more damage, and no healing, on top of
        // a death that had already been the price of the mistake.
        self.sick_for = 0.0;
        self.brewing = 0.0;
        self.brewing_in = 0.0;
        self.dead = false;
        self.stand(false);
        self.clear_fall();
        // Deliberately *not* setting `last_reported`: the client has to
        // be told about the restored health, and pretending it already
        // knows would leave a fresh spawn showing an empty bar.
        self.last_damage = Instant::now() - std::time::Duration::from_secs(3600);
        // ...and the blow that killed them is not still landing on the body
        // that came back: a respawn into a bed the player had been mauled
        // beside used to be refused for ten seconds (`HURT_RECENTLY_SECS`).
        self.last_blow = self.last_damage;
    }

    /// Forgets any fall in progress.
    ///
    /// Every server-side reposition has to call this. A teleport moves
    /// the player without them falling, and a rubber-band correction can
    /// move them *downwards* by several blocks -- charging fall damage
    /// for either would mean the anti-cheat and the `/tp` command both
    /// quietly hurt people.
    pub fn clear_fall(&mut self) {
        self.fall_peak_y = None;
        self.airborne = false;
    }
}

impl Default for Vitals {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod hunger_tests {
    use super::*;
    use primitive_shared::types::{BLOCK_BERRIES, BLOCK_COBBLESTONE, BLOCK_COOKED_MEAT};

    /// Runs `seconds` of game time through `digest` at a 20 Hz tick.
    fn live_for(vitals: &mut Vitals, seconds: f32, effort: Effort) {
        let dt = 1.0 / 20.0;
        for _ in 0..((seconds / dt) as usize) {
            vitals.digest(effort, dt);
        }
    }

    #[test]
    fn a_jug_of_mead_warms_a_cold_body_on_a_full_stomach_and_never_past_comfortable() {
        use primitive_shared::types::BLOCK_JUG_MEAD;
        let mut vitals = Vitals::new();
        vitals.set_warmth(body::CHILLED, 0.0);
        assert_eq!(vitals.eat(BLOCK_JUG_MEAD), Outcome::Changed, "a cold player could not drink it");
        assert!((vitals.body_c - (body::CHILLED + food::MEAD_WARMTH_C)).abs() < 1e-4);
        // Warm already: no hotter.
        let mut warm = Vitals::new();
        warm.set_warmth(body::COMFORT_HIGH - 1.0, 0.0);
        warm.warm_by(food::MEAD_WARMTH_C);
        assert_eq!(warm.body_c, body::COMFORT_HIGH);
    }

    #[test]
    fn a_new_player_starts_full() {
        let vitals = Vitals::new();
        assert_eq!(vitals.nourishment_fraction(), 1.0);
        assert!(!vitals.needs_food_report(), "a fresh player is already in sync");
    }

    #[test]
    fn standing_about_for_ten_minutes_barely_registers() {
        // The first claim the rates make: what makes you hungry is what
        // you did, not how long you were logged in. Ten minutes is a
        // whole in-game day at the default clock.
        let mut vitals = Vitals::new();
        live_for(&mut vitals, 600.0, Effort::IDLE);
        assert!(
            vitals.nourishment_fraction() > 0.7,
            "ten idle minutes took the bar to {}",
            vitals.nourishment_fraction()
        );
        // ...and specifically, it has not crossed the line where wounds
        // stop closing, or standing still would be a punishment.
        assert!(vitals.nourishment_fraction() >= food::REGEN_THRESHOLD);
    }

    #[test]
    fn an_evening_of_digging_does() {
        let mut idle = Vitals::new();
        let mut working = Vitals::new();
        live_for(&mut idle, 600.0, Effort::IDLE);
        live_for(&mut working, 600.0, Effort { sprinting: false, mining: true, swimming: false });
        assert!(
            working.nourishment() < idle.nourishment() - 2.0,
            "ten minutes of mining cost {} against an idle {}",
            20.0 - working.nourishment(),
            20.0 - idle.nourishment()
        );
    }

    #[test]
    fn sprinting_and_digging_at_once_costs_both() {
        let mut one = Vitals::new();
        let mut both = Vitals::new();
        live_for(&mut one, 300.0, Effort { sprinting: true, mining: false, swimming: false });
        live_for(&mut both, 300.0, Effort { sprinting: true, mining: true, swimming: false });
        assert!(both.nourishment() < one.nourishment());
    }

    /// **Drink from something that is going somewhere.** The three
    /// waters, as the three things they do to a player: see
    /// `body::Water`.
    #[test]
    fn a_stream_quenches_a_pond_makes_you_ill_and_the_sea_makes_you_thirstier() {
        use primitive_shared::body::Water;
        let mouthful = body::DRINK_HYDRATION;

        let mut river = Vitals::new();
        river.set_hydration(body::MAX_HYDRATION * 0.5);
        assert!(river.drink_water(Water::Fresh, mouthful));
        assert!(river.hydration() > body::MAX_HYDRATION * 0.5, "a stream did not quench");
        assert_eq!(river.sick_for(), 0.0, "running water made somebody ill");

        let mut pond = Vitals::new();
        pond.set_hydration(body::MAX_HYDRATION * 0.5);
        pond.catch_the_next_illness();
        assert!(pond.drink_water(Water::Standing, mouthful));
        assert!(pond.hydration() > body::MAX_HYDRATION * 0.5, "a pond did not quench at all");
        assert!(pond.illness_owed() > 0.0, "an unlucky mouthful of a pond cost nothing");

        // ...and the sea takes water out of a body rather than putting
        // it in, which is the one thing everybody knows about drinking
        // at sea and no game ever models.
        let mut sea = Vitals::new();
        sea.set_hydration(body::MAX_HYDRATION * 0.5);
        sea.catch_the_next_illness();
        let before = sea.hydration();
        assert!(sea.drink_water(Water::Salt, mouthful));
        assert!(sea.hydration() < before, "the sea quenched thirst");
        assert!(sea.illness_owed() > pond.illness_owed(), "the sea is no worse than a pond");

        // The illness takes health slowly and stops a wound closing
        // while it lasts -- which is most of what it costs.
        let mut ill = Vitals::new();
        // Thirsty enough to drink at all: a full player cannot, which
        // is `drink`'s own rule and the reason a pond is only ever a
        // mistake somebody had a reason to make.
        ill.set_hydration(body::MAX_HYDRATION * 0.5);
        ill.hurt(5.0, "a boar");
        ill.last_damage = Instant::now() - std::time::Duration::from_secs_f32(REGEN_DELAY_SECS + 1.0);
        ill.catch_the_next_illness();
        ill.drink_water(Water::Standing, mouthful);
        ill.digest_now();
        let health = ill.health();
        assert_eq!(ill.regenerate(10.0), Outcome::Unchanged, "an ill player healed");
        assert!(!matches!(ill.sicken(10.0), Outcome::Unchanged), "being ill cost nothing");
        assert!(ill.health() < health, "the sickness took no health");

        // ...and it ends.
        let mut over = Vitals::new();
        over.catch_the_next_illness();
        over.drink_water(Water::Standing, mouthful);
        over.digest_now();
        over.sicken(body::SICKNESS_SECONDS.0 + 1.0);
        assert_eq!(over.sick_for(), 0.0, "the illness never wore off");
    }

    /// **Two in five, and later.** A pond makes about forty drinkers in a
    /// hundred ill (`body::WATER_ILLNESS_CHANCE`), and none of them feels it
    /// until `body::DIGESTION_SECONDS` have gone by.
    #[test]
    fn a_pond_makes_two_drinkers_in_five_ill_and_only_after_the_water_has_gone_down() {
        use primitive_shared::body::Water;
        let mut ill = 0;
        let drinkers = 2000;
        for seed in 0..drinkers {
            let mut vitals = Vitals::new();
            vitals.set_roll_seed(seed * 7919);
            vitals.set_hydration(body::MAX_HYDRATION * 0.5);
            assert!(vitals.drink_water(Water::Standing, body::DRINK_HYDRATION));
            ill += usize::from(vitals.illness_owed() > 0.0);
        }
        let share = ill as f32 / drinkers as f32;
        assert!((share - body::WATER_ILLNESS_CHANCE).abs() < 0.04, "{share} of pond drinkers fell ill");

        let mut vitals = Vitals::new();
        vitals.set_hydration(body::MAX_HYDRATION * 0.5);
        vitals.catch_the_next_illness();
        vitals.drink_water(Water::Standing, body::DRINK_HYDRATION);
        let health = vitals.health();
        let tick = 0.05;
        let mut t = 0.0;
        while t < body::DIGESTION_SECONDS - 1.0 {
            vitals.sicken(tick);
            t += tick;
        }
        assert_eq!(vitals.sick_for(), 0.0, "the illness arrived {t}s after the mouthful, before digestion");
        assert_eq!(vitals.health(), health, "a player was hurt by water still on its way down");
        for _ in 0..40 {
            vitals.sicken(tick);
        }
        assert!(vitals.sick_for() > 0.0, "the illness never arrived");
        assert!(vitals.health() < health, "the illness arrived and took nothing");
    }

    #[test]
    fn a_second_bad_mouthful_does_not_put_off_the_first() {
        use primitive_shared::body::Water;
        let mut vitals = Vitals::new();
        vitals.set_hydration(0.0);
        vitals.catch_the_next_illness();
        vitals.drink_water(Water::Standing, 1.0);
        vitals.sicken(body::DIGESTION_SECONDS * 0.75);
        vitals.catch_the_next_illness();
        vitals.drink_water(Water::Standing, 1.0);
        vitals.sicken(body::DIGESTION_SECONDS * 0.3);
        assert!(vitals.sick_for() > 0.0, "drinking again reset the clock of the first mouthful");
    }

    /// **Swimming is work, and swimming loaded is harder work.** The
    /// player asked for both; the numbers are `Effort::swimming` and
    /// `load::swim_effort`.
    #[test]
    fn swimming_costs_more_than_walking_and_a_loaded_swimmer_costs_most() {
        let spent = |effort: Effort, kilograms: f32| -> f32 {
            let mut vitals = Vitals::new();
            vitals.set_carried_weight(kilograms);
            let before = vitals.nourishment();
            vitals.digest(effort, 60.0);
            before - vitals.nourishment()
        };
        let walking = spent(Effort::IDLE, 0.0);
        let swimming = spent(
            Effort {
                swimming: true,
                ..Effort::IDLE
            },
            0.0,
        );
        let loaded = spent(
            Effort {
                swimming: true,
                ..Effort::IDLE
            },
            primitive_shared::load::SWIM_SINK_KG,
        );
        assert!(swimming > walking * 3.0, "a minute of swimming cost {swimming}");
        assert!(
            loaded > swimming * 1.5,
            "swimming with a stone pack cost {loaded} against {swimming} empty"
        );
    }

    /// **Past half a load, a swimmer does not float at all.** The other
    /// half of the same mechanic, and the half the client acts on: see
    /// `load::buoyancy`, which the physics multiplies its lift by.
    #[test]
    fn a_heavy_pack_takes_a_swimmer_under() {
        use primitive_shared::load::{buoyancy, SWIM_FREE_KG, SWIM_SINK_KG};
        assert_eq!(buoyancy(0.0), 1.0, "an empty player sank");
        assert_eq!(buoyancy(SWIM_FREE_KG), 1.0, "a light pack cost buoyancy");
        assert_eq!(buoyancy(SWIM_SINK_KG), 0.0, "half a load still floated");
        assert_eq!(buoyancy(SWIM_SINK_KG * 4.0), 0.0);
        let middle = buoyancy((SWIM_FREE_KG + SWIM_SINK_KG) / 2.0);
        assert!((middle - 0.5).abs() < 0.01, "the ramp is not a ramp: {middle}");
        // ...and nonsense from a save or a mod floats rather than
        // drowning somebody: the safe direction, as everywhere else in
        // this file.
        assert_eq!(buoyancy(f32::NAN), 1.0);
    }

    #[test]
    fn a_night_at_home_makes_the_next_half_day_cost_a_quarter_less_food_and_then_it_is_gone() {
        use primitive_shared::comfort::{RESTED_HUNGER, RESTED_SECONDS};
        use primitive_shared::food::MAX_NOURISHMENT;
        let spent = |rested: bool, seconds: f32| {
            let mut vitals = Vitals::new();
            vitals.set_nourishment(MAX_NOURISHMENT);
            if rested {
                vitals.rest_at_home();
            }
            vitals.digest(Effort::IDLE, seconds);
            MAX_NOURISHMENT - vitals.nourishment()
        };
        let (plain, rested) = (spent(false, 300.0), spent(true, 300.0));
        assert!(
            (rested / plain - RESTED_HUNGER).abs() < 0.01,
            "five minutes rested cost {rested} against {plain} tired"
        );
        // After the half day, the ordinary rate again.
        let mut vitals = Vitals::new();
        vitals.set_nourishment(MAX_NOURISHMENT);
        vitals.rest_at_home();
        for _ in 0..(RESTED_SECONDS as usize) {
            vitals.digest(Effort::IDLE, 1.0);
        }
        assert_eq!(vitals.rested_for(), 0.0, "the rest outlasted its half day");
        let before = vitals.nourishment();
        vitals.digest(Effort::IDLE, 300.0);
        assert!((before - vitals.nourishment() - plain).abs() < 1e-3, "the rest was still at work after it ran out");
    }

    #[test]
    fn a_hungry_player_stops_healing_before_they_start_starving() {
        // The order matters more than either rule: hunger is supposed to
        // be noticed as "why am I not getting better" long before it is
        // noticed as damage.
        let mut vitals = Vitals::new();
        vitals.hurt(8.0, "a boar");
        vitals.last_damage = Instant::now() - std::time::Duration::from_secs_f32(REGEN_DELAY_SECS + 1.0);
        assert_eq!(vitals.regenerate(1.0), Outcome::Changed, "a fed player heals");

        vitals.set_nourishment(food::MAX_NOURISHMENT * (food::REGEN_THRESHOLD - 0.05));
        let held = vitals.health();
        assert_eq!(vitals.regenerate(10.0), Outcome::Unchanged, "a hungry player healed");
        assert_eq!(vitals.health(), held);
        // ...and is still nowhere near dying of it.
        assert!(!vitals.is_dead());
    }

    /// **A mixed diet heals three times as fast as one thing.** The
    /// mechanic the player asked for, stated as the only number it
    /// touches: variety is not a bar and not a buff, it is how quickly
    /// a wound closes -- see `food::diet_regen_factor`.
    #[test]
    fn a_player_who_eats_one_thing_heals_a_third_as_fast_as_one_who_eats_three() {
        let heal = |groups: &[food::Group]| -> f32 {
            let mut vitals = Vitals::new();
            vitals.hurt(10.0, "a boar");
            vitals.last_damage =
                Instant::now() - std::time::Duration::from_secs_f32(REGEN_DELAY_SECS + 1.0);
            let now = Instant::now();
            for &group in groups {
                vitals.ate_group(group, now);
            }
            let before = vitals.health();
            assert_eq!(vitals.regenerate(10.0), Outcome::Changed);
            vitals.health() - before
        };

        let one = heal(&[food::Group::Meat]);
        let two = heal(&[food::Group::Meat, food::Group::Plant]);
        let three = heal(&food::Group::ALL);
        assert!(one > 0.0, "a player living on meat alone never heals");
        assert!(two > one, "the second group bought nothing");
        assert!(three > two, "the third group bought nothing");
        assert!(
            (three / one - 3.0).abs() < 0.01,
            "three groups heal {:.2} times as fast as one, not three",
            three / one
        );

        // ...and a meal is forgotten. A player who ate bread two days
        // ago is a player who has not eaten bread.
        let mut stale = Vitals::new();
        stale.ate_group(
            food::Group::Grain,
            Instant::now() - std::time::Duration::from_secs_f32(food::DIET_MEMORY_SECS + 1.0),
        );
        assert_eq!(stale.diet_groups(), 0, "a forgotten meal still counted");
    }

    #[test]
    fn an_empty_stomach_eventually_kills_and_says_why() {
        let mut vitals = Vitals::new();
        vitals.set_hunger_seed(5);
        vitals.set_nourishment(0.0);
        let mut cause = None;
        // An hour at 20 Hz: the rolls are chance, so long enough for any
        // seed. See `food::STARVATION_ROLL_SECONDS`.
        // ...and the clock on the ground after it, which is where a starving
        // body dies now (`downed`).
        for _ in 0..72_000 {
            let outcome = match vitals.digest(Effort::IDLE, 0.05) {
                Outcome::Died { cause } => Outcome::Died { cause },
                _ => vitals.step_downed(0.05),
            };
            if let Outcome::Died { cause: why } = outcome {
                cause = Some(why);
                break;
            }
        }
        assert_eq!(cause.as_deref(), Some("starved"));
        assert!(vitals.is_dead());
    }

    #[test]
    fn starving_takes_long_enough_to_do_something_about() {
        // About eight minutes from the bar emptying, at full health, and never
        // a minute and a half: the rolls are chance, so the bounds are wide
        // and every seed of a handful has to fall inside them.
        for seed in 0..8u64 {
            let mut vitals = Vitals::new();
            vitals.set_hunger_seed(seed);
            vitals.set_nourishment(0.0);
            let mut seconds = 0.0;
            while !vitals.is_dead() && seconds < 3600.0 {
                vitals.digest(Effort::IDLE, 0.05);
                vitals.step_downed(0.05);
                seconds += 0.05;
            }
            assert!(seconds > 180.0, "seed {seed}: starved to death in {seconds}s");
            assert!(seconds < 1800.0, "seed {seed}: starvation is not a threat at {seconds}s");
        }
    }

    #[test]
    fn an_empty_stomach_takes_one_point_at_a_time_and_only_every_five_seconds() {
        // Never a drain: health goes in whole points, and never two inside
        // one five-second roll.
        let mut vitals = Vitals::new();
        vitals.set_hunger_seed(3);
        vitals.set_nourishment(0.0);
        let mut last = vitals.health();
        let mut since_last_hit = f32::INFINITY;
        for _ in 0..(600.0 / 0.05) as usize {
            vitals.digest(Effort::IDLE, 0.05);
            since_last_hit += 0.05;
            let now = vitals.health();
            if now < last {
                assert!((last - now - 1.0).abs() < 1e-4, "hunger took {} in one tick", last - now);
                assert!(since_last_hit > 4.9, "two points of hunger {since_last_hit}s apart");
                since_last_hit = 0.0;
            }
            last = now;
        }
        assert!(vitals.health() < MAX_HEALTH, "ten minutes of an empty stomach took nothing");
    }

    /// One long step -- a night slept through, which the server charges
    /// in a single `digest` and `drink_down` -- has to cost what the same
    /// hours cost a tick at a time.
    ///
    /// It did not: the bar emptying *anywhere* inside the step billed
    /// starvation for the whole of it, so a player who lay down with a
    /// few mouthfuls left was charged four hundred seconds of an empty
    /// stomach for the forty it was really empty, and woke dead from full
    /// health.
    #[test]
    fn a_night_slept_through_in_one_step_starves_and_parches_no_harder_than_the_same_night_lived_tick_by_tick() {
        const NIGHT: f32 = 405.0;
        // Food for all but the last forty-five seconds of the night.
        let food = Effort::IDLE.drain(NIGHT - 45.0, 0.0);
        let mut ticked = Vitals::new();
        let mut slept = Vitals::new();
        ticked.set_hunger_seed(11);
        slept.set_hunger_seed(11);
        ticked.set_nourishment(food);
        slept.set_nourishment(food);
        for _ in 0..(NIGHT / 0.05) as usize {
            ticked.digest(Effort::IDLE, 0.05);
        }
        slept.digest(Effort::IDLE, NIGHT);
        assert!(!ticked.is_dead(), "the night lived tick by tick should be survivable");
        assert!(!slept.is_dead(), "slept through, the same night killed");
        assert!(
            (ticked.health() - slept.health()).abs() < 1.0,
            "hunger: ticked {} against slept {}",
            ticked.health(),
            slept.health()
        );

        // ...and the same for thirst.
        let resting = body::Exertion::RESTING;
        let water = resting.thirst_per_second() * (NIGHT - 5.0);
        let mut ticked = Vitals::new();
        let mut slept = Vitals::new();
        ticked.set_hydration(water);
        slept.set_hydration(water);
        for _ in 0..(NIGHT / 0.05) as usize {
            ticked.drink_down(resting, 0.05);
        }
        slept.drink_down(resting, NIGHT);
        assert!(!ticked.is_dead(), "the dry night lived tick by tick should be survivable");
        assert!(!slept.is_dead(), "slept through, the same dry night killed");
        assert!(
            (ticked.health() - slept.health()).abs() < 1.0,
            "thirst: ticked {} against slept {}",
            ticked.health(),
            slept.health()
        );
    }

    /// **Cooking had to stop being an optimisation.**
    ///
    /// A roast filled more of the bar than the raw haunch, and that is a
    /// sum: a player in a hurry skips the fire and loses a little. Now
    /// raw flesh leaves them ill for three quarters of a minute, which
    /// makes eating it a decision with a wrong answer most of the time
    /// and a right one when you are starving in the dark with no fire.
    ///
    /// The illness is the *same* one a stale river gives, on purpose --
    /// see `food::sickness_seconds`.
    #[test]
    fn raw_flesh_and_bad_mushrooms_leave_a_player_ill_and_cooking_does_not() {
        use primitive_shared::types::{
            BLOCK_COOKED_MEAT, BLOCK_RAW_MEAT, BLOCK_ROTTEN, BLOCK_TOADSTOOL,
        };
        let mut vitals = Vitals::new();
        vitals.nourishment = 2.0;
        vitals.eat(BLOCK_COOKED_MEAT);
        assert_eq!(vitals.illness_owed(), 0.0, "a roast should not make anybody ill");

        // Owed rather than felt: a meal's illness waits for digestion
        // (`body::DIGESTION_SECONDS`), and what this test is about is how
        // much of it there will be.
        let mut vitals = Vitals::new();
        vitals.nourishment = 2.0;
        vitals.eat(BLOCK_RAW_MEAT);
        let raw = vitals.illness_owed();
        assert!(raw > 0.0, "raw flesh should sit badly");

        // ...and the worse things are worse, in the order anybody would
        // guess: a bad mushroom is the longest of the three.
        let illness = |block| {
            let mut vitals = Vitals::new();
            vitals.nourishment = 2.0;
            vitals.eat(block);
            vitals.illness_owed()
        };
        assert!(illness(BLOCK_ROTTEN) > raw, "rot should be worse than raw");
        assert!(
            illness(BLOCK_TOADSTOOL) > illness(BLOCK_ROTTEN),
            "a toadstool should be the worst of them"
        );

        // **Two raw haunches are one bad afternoon, not two.** Taken as
        // the longer of the two rather than added, or a player who ate
        // three would be ill for the rest of the day and would read that
        // as the game being broken.
        let mut vitals = Vitals::new();
        vitals.nourishment = 0.0;
        vitals.eat(BLOCK_RAW_MEAT);
        vitals.eat(BLOCK_RAW_MEAT);
        assert!(
            (vitals.illness_owed() - raw).abs() < 1e-3,
            "two raw meals gave {} of illness against one meal's {raw}",
            vitals.illness_owed()
        );
    }

    #[test]
    fn eating_fills_and_spends_and_a_full_player_does_neither() {
        let mut vitals = Vitals::new();
        let ate = |outcome: Outcome| !matches!(outcome, Outcome::Unchanged);
        assert!(!ate(vitals.eat(BLOCK_COOKED_MEAT)), "ate on a full stomach");
        assert_eq!(vitals.nourishment(), food::MAX_NOURISHMENT);

        vitals.set_nourishment(4.0);
        assert!(ate(vitals.eat(BLOCK_BERRIES)));
        assert_eq!(vitals.nourishment(), 6.0);
        assert!(ate(vitals.eat(BLOCK_COOKED_MEAT)));
        assert_eq!(vitals.nourishment(), 16.0);
        // Stone is not a meal, and asking to eat it spends nothing.
        assert!(!ate(vitals.eat(BLOCK_COBBLESTONE)));
        assert_eq!(vitals.nourishment(), 16.0);
    }

    #[test]
    fn a_toadstool_is_eaten_and_then_paid_for() {
        // The mistake, end to end on the vitals: it goes down (so the
        // gesture is not refused), it takes health *through the path
        // that announces a death*, and it empties rather than fills.
        let mut vitals = Vitals::new();
        vitals.set_nourishment(12.0);
        let before = vitals.health();
        let outcome = vitals.eat(primitive_shared::types::BLOCK_TOADSTOOL);
        assert!(!matches!(outcome, Outcome::Unchanged), "the game refused the mistake");
        assert!(vitals.health() < before, "it cost nothing");
        assert!(vitals.nourishment() < 12.0, "a bad mushroom fed somebody");

        // ...and on somebody already nearly gone it is the last thing
        // they can take: it puts them on the ground (`downed`), reported
        // as a change rather than swallowed as a scratch.
        let mut dying = Vitals::new();
        dying.set_health(2.0);
        assert_eq!(dying.eat(primitive_shared::types::BLOCK_TOADSTOOL), Outcome::Changed);
        assert!(dying.is_downed(), "a toadstool on the last two points left the eater standing");
    }

    #[test]
    fn the_dead_neither_starve_nor_eat() {
        let mut vitals = Vitals::new();
        vitals.hurt(f32::MAX, "killed");
        vitals.set_nourishment(0.0);
        assert_eq!(vitals.digest(Effort::IDLE, 10.0), Outcome::Unchanged);
        assert_eq!(vitals.eat(BLOCK_COOKED_MEAT), Outcome::Unchanged);
        vitals.jumped();
        assert_eq!(vitals.nourishment(), 0.0);
    }

    #[test]
    fn coming_back_comes_back_fed() {
        // Respawning starving is not a death penalty, it is a loop: two
        // minutes to live, no food, and the same death again.
        let mut vitals = Vitals::new();
        vitals.set_hunger_seed(5);
        vitals.set_nourishment(0.0);
        while !vitals.is_dead() {
            vitals.digest(Effort::IDLE, 1.0);
            vitals.step_downed(1.0);
        }
        vitals.respawn();
        assert_eq!(vitals.nourishment_fraction(), 1.0);
        assert!(vitals.needs_food_report(), "the client was never told");
    }

    #[test]
    fn only_a_visible_change_is_worth_a_packet() {
        // This runs twenty times a second per player for a number that
        // takes forty minutes to cross the bar.
        let mut vitals = Vitals::new();
        vitals.digest(Effort::IDLE, 0.05);
        assert!(!vitals.needs_food_report(), "reporting noise-level changes");
        vitals.set_nourishment(4.0);
        vitals.mark_food_reported();
        assert!(!vitals.needs_food_report());
        vitals.eat(BLOCK_COOKED_MEAT);
        assert!(vitals.needs_food_report(), "a whole meal went unreported");
    }

    #[test]
    fn a_stored_value_off_a_disk_is_clamped_rather_than_believed() {
        let mut vitals = Vitals::new();
        vitals.set_nourishment(f32::NAN);
        assert_eq!(vitals.nourishment(), food::MAX_NOURISHMENT);
        vitals.set_nourishment(-5.0);
        assert_eq!(vitals.nourishment(), 0.0);
        vitals.set_nourishment(1e9);
        assert_eq!(vitals.nourishment(), food::MAX_NOURISHMENT);
        // ...and zero is zero, not "start fresh": somebody who logged
        // out starving logs back in starving.
        vitals.set_nourishment(0.0);
        assert_eq!(vitals.nourishment(), 0.0);
    }
}

#[cfg(test)]
mod tests {

    #[test]
    fn a_body_that_comes_back_comes_back_mended() {
        // **A leg broken in a life that ended stays in it.** A player
        // who respawned still limping was carrying an injury they could
        // do nothing about -- the leg belonged to a corpse -- and the
        // only cure in the game is a bed they now have to limp to. The
        // profile has always mended the dead on the way back in; this
        // is the same rule for the player who never disconnected.
        let mut vitals = Vitals::new();
        let mut wounds = Injuries::default();
        wounds.inflict(injury::Part::LeftLeg, injury::Kind::Fracture, 1.0);
        wounds.inflict(injury::Part::Torso, injury::Kind::Cut, 0.8);
        vitals.set_injuries(wounds);
        vitals.set_fatigue(1.0);
        vitals.hurt(f32::MAX, "fell from a great height");
        assert!(vitals.is_dead());

        vitals.respawn();
        assert!(vitals.injuries().is_whole(), "a corpse's wounds came back with it");
        assert_eq!(vitals.fatigue(), 0.0, "and so did its exhaustion");
        assert_eq!(vitals.rest_speed_factor(), 1.0, "a fresh body walks at a walk");
    }

    /// The leg a wound is on, whichever the roll chose.
    fn broken_leg(vitals: &Vitals) -> Option<injury::Part> {
        [injury::Part::LeftLeg, injury::Part::RightLeg]
            .into_iter()
            .find(|&leg| vitals.injuries().wound(leg, injury::Kind::Fracture).is_open())
    }

    #[test]
    fn a_broken_left_leg_limps_on_the_left_and_a_broken_right_leg_on_the_right() {
        // The limp always favoured the right, whichever leg the fall took.
        // See `protocol::PlayerState::limp_left`.
        for (leg, left) in [(injury::Part::LeftLeg, true), (injury::Part::RightLeg, false)] {
            let mut vitals = Vitals::new();
            assert!(!vitals.limp_left(), "a sound player limps on the left");
            vitals.injuries.inflict(leg, injury::Kind::Fracture, 1.0);
            assert!(vitals.limp() > 0.5, "a broken {leg:?} did not limp");
            assert_eq!(vitals.limp_left(), left, "a broken {leg:?} limped on the wrong side");
        }
        // Tiredness has no side, and keeps the right it always had.
        let mut spent = Vitals::new();
        spent.set_health(MAX_HEALTH * 0.05);
        assert!(spent.limp() > 0.0 && !spent.limp_left(), "a failing body limped on the left");
    }

    #[test]
    fn a_broken_leg_slows_the_walk_until_it_is_splinted_and_healed() {
        // **This used to mend in bed on its own**, and the test said so:
        // "a night in a bed is what mends it". A leg that set itself made
        // the bed the whole answer and the break a wait. Now the bone does
        // not knit unset -- the splint is the decision, the bed is still
        // what makes it quick -- and the limp lasts until it has knitted,
        // because a splint holds a leg straight and does not make it a leg
        // you can run on.
        use primitive_shared::types::BLOCK_SPLINT;
        let mut vitals = Vitals::new();
        assert!(vitals.injuries().is_whole(), "a new player starts in one piece");

        // A short drop hurts and breaks nothing.
        vitals.set_health(MAX_HEALTH);
        vitals.on_transform(30.0, false, false);
        vitals.on_transform(24.0, true, false);
        assert!(!vitals.injuries().leg_broken(), "a six block fall broke a leg");

        // ...and a long one breaks it.
        vitals.set_health(MAX_HEALTH);
        vitals.on_transform(40.0, false, false);
        vitals.on_transform(24.0, true, false);
        let leg = broken_leg(&vitals).expect("a sixteen block fall left the legs alone");
        let limping = primitive_shared::body::FRACTURE_SPEED;
        assert!((vitals.rest_speed_factor() - limping).abs() < 1e-5, "a broken leg cost nothing to walk on");

        // A whole night in bed does nothing for a leg nobody set.
        let outcome = vitals.mend(8.0 * 60.0, true);
        assert_eq!(outcome, Outcome::Unchanged, "a break cost health to lie on");
        assert!(vitals.injuries().leg_broken(), "an unset leg knitted in bed");

        // Splinted, and still limping: the splint is what lets it knit.
        assert_eq!(vitals.treat(leg, BLOCK_SPLINT), Ok(injury::Kind::Fracture));
        assert!((vitals.rest_speed_factor() - limping).abs() < 1e-5, "a splint gave the leg back at once");

        // ...and it knits a little at a time, six times as fast lying down.
        let severity = |v: &Vitals| v.injuries().wound(leg, injury::Kind::Fracture).severity;
        let before = severity(&vitals);
        vitals.mend(60.0, false);
        let awake = before - severity(&vitals);
        let before = severity(&vitals);
        vitals.mend(60.0, true);
        let asleep = before - severity(&vitals);
        assert!(awake > 0.0 && severity(&vitals) > 0.0, "a splinted leg did not knit gradually");
        assert!(asleep > awake * 5.0, "sleeping mended no faster than walking about");

        // Two nights clear it, which is the promise the bed still makes.
        vitals.mend(2.0 * 8.0 * 60.0, true);
        assert!(!vitals.injuries().leg_broken(), "two nights did not knit a splinted leg");
        assert_eq!(vitals.rest_speed_factor(), 1.0);
    }

    #[test]
    fn a_bite_leaves_a_cut_that_bleeds_until_it_is_bandaged() {
        use primitive_shared::types::BLOCK_BANDAGE;
        let mut vitals = Vitals::new();
        vitals.set_health(MAX_HEALTH - 2.0);
        // A wolf's bite, through nothing.
        let part = vitals
            .take_blow(injury::Blow::Bite, 3.0)
            .expect("a bite landed nowhere");
        assert!(vitals.injuries().is_bleeding(), "a bare wolf bite did not bleed");

        let before = vitals.health();
        for _ in 0..60 {
            let outcome = vitals.mend(1.0, false);
            assert_ne!(outcome, Outcome::Unchanged, "a bleeding second took nothing");
        }
        assert!(before - vitals.health() > 0.5, "a minute of a bite cost {}", before - vitals.health());
        assert_eq!(vitals.regenerate(1.0), Outcome::Unchanged, "a bleeding body mended");

        assert_eq!(vitals.treat(part, BLOCK_BANDAGE), Ok(injury::Kind::Cut));
        let dressed = vitals.health();
        assert_eq!(vitals.mend(1.0, false), Outcome::Unchanged, "a bandaged cut still bled");
        assert_eq!(vitals.health(), dressed);
        assert_eq!(vitals.regenerate(1.0), Outcome::Changed, "a bandaged body did not mend");
    }

    #[test]
    fn a_bandage_on_a_leg_with_no_cut_is_refused_and_changes_nothing() {
        use primitive_shared::types::BLOCK_BANDAGE;
        let mut vitals = Vitals::new();
        let mut wounds = Injuries::default();
        wounds.inflict(injury::Part::LeftArm, injury::Kind::Cut, 0.8);
        vitals.set_injuries(wounds);
        assert_eq!(
            vitals.treat(injury::Part::LeftLeg, BLOCK_BANDAGE),
            Err(injury::Refusal::NothingItHelps)
        );
        assert_eq!(vitals.injuries(), &wounds, "a refused bandage touched the body");
    }

    #[test]
    fn bleeding_to_death_is_a_death() {
        // **An outcome that has to go somewhere.** A starving sleeper was
        // once immortal because the tick loop dropped the `Died` its step
        // handed back; bleeding is the same shape of rule, and this is the
        // half of it that lives here -- the step says `Died`, with a cause
        // the death screen can print, exactly once.
        let mut vitals = Vitals::new();
        vitals.set_health(0.5);
        let mut wounds = Injuries::default();
        for part in [injury::Part::Torso, injury::Part::LeftArm, injury::Part::RightArm] {
            wounds.inflict(part, injury::Kind::Cut, 1.0);
        }
        vitals.set_injuries(wounds);
        // Down first and dead when the clock runs out (`downed`), with the
        // words of the bleeding either way.
        let mut died = None;
        for _ in 0..1200 {
            let outcome = match vitals.mend(0.05, false) {
                Outcome::Died { cause } => Outcome::Died { cause },
                _ => vitals.step_downed(0.05),
            };
            if let Outcome::Died { cause } = outcome {
                died = Some(cause);
                break;
            }
        }
        assert_eq!(died.as_deref(), Some("bled to death"));
        assert!(vitals.is_dead());
        assert_eq!(vitals.mend(1.0, false), Outcome::Unchanged, "a corpse died twice");
    }

    #[test]
    fn bleeding_does_not_wake_a_sleeper_or_count_as_being_struck() {
        // See `lose`: a cut that counted as a blow would turn every bed
        // away and wake every sleeper, every tick, for as long as it bled.
        let mut vitals = Vitals::new();
        let mut wounds = Injuries::default();
        wounds.inflict(injury::Part::Torso, injury::Kind::Cut, 0.6);
        vitals.set_injuries(wounds);
        vitals.mend(1.0, true);
        assert!(vitals.health() < MAX_HEALTH, "the cut did not bleed");
        assert!(vitals.last_blow_elapsed() > 60.0, "a bleeding cut counted as a blow");
    }

    /// The five ways a body kills itself, and none of them is a blow.
    ///
    /// They all were, and the bed read `last_damage` to decide whether to
    /// take a sleeper: a player freezing in a blizzard was told "you cannot
    /// sleep while something is hurting you" with nothing anywhere near
    /// them. Mending still waits on every one of them -- health going is
    /// health going -- which is the other half of the property and the
    /// reason there are two clocks rather than one.
    #[test]
    fn the_cold_the_heat_the_hunger_the_thirst_and_a_sickness_take_health_and_none_of_them_is_a_blow() {
        /// A way the body kills itself, and what it is called in a failure.
        type Ailment = (&'static str, fn(&mut Vitals));
        let cases: [Ailment; 5] = [
            ("the cold", |v| {
                v.set_warmth(body::FREEZING - 6.0, 0.0);
                v.warm(body::Exposure::air(-40.0), 0.0, 0.0, 0.0, 1.0);
            }),
            ("the heat", |v| {
                v.set_warmth(body::SCALDING + 6.0, 0.0);
                v.warm(body::Exposure::air(80.0), 0.0, 0.0, 0.0, 1.0);
            }),
            ("hunger", |v| {
                v.set_nourishment(0.0);
                v.digest(Effort::IDLE, 60.0);
            }),
            ("thirst", |v| {
                v.set_hydration(0.0);
                v.drink_down(body::Exertion::RESTING, 60.0);
            }),
            ("a sickness", |v| {
                v.swallow_illness(30.0);
                // Past `body::DIGESTION_SECONDS` in one step, so what was
                // swallowed has arrived and is being felt.
                v.sicken(300.0);
            }),
        ];
        for (what, harm) in cases {
            let mut vitals = Vitals::new();
            // A blow long ago, so the clocks start together and a test that
            // passes by never having been touched is not possible.
            vitals.last_damage = Instant::now() - std::time::Duration::from_secs(3600);
            vitals.last_blow = vitals.last_damage;
            harm(&mut vitals);
            assert!(vitals.health() < MAX_HEALTH, "{what} took no health at all");
            assert!(vitals.last_blow_elapsed() > 60.0, "{what} was counted as a blow");
            assert!(
                vitals.last_damage.elapsed().as_secs_f32() < REGEN_DELAY_SECS,
                "{what} took health and left the body mending through it",
            );
        }
    }

    #[test]
    fn a_broken_arm_weakens_a_blow() {
        let mut vitals = Vitals::new();
        assert_eq!(vitals.strength_factor(), 1.0);
        let mut wounds = Injuries::default();
        wounds.inflict(injury::Part::RightArm, injury::Kind::Fracture, 1.0);
        vitals.set_injuries(wounds);
        assert_eq!(vitals.strength_factor(), injury::BROKEN_ARM_STRENGTH);
        // ...and the legs do not care about an arm.
        assert_eq!(vitals.rest_speed_factor(), 1.0);
    }


    #[test]
    fn a_day_awake_costs_a_body_something_and_a_night_in_a_bed_gives_it_back() {
        // The whole of the mechanic in one test. Tiredness fills on a
        // clock (`body::WAKING_SECONDS`), it costs healing and pace
        // once past `body::TIRED_AT`, and a night in a real bed clears
        // it while a night on straw does not quite.
        let mut vitals = Vitals::new();
        assert_eq!(vitals.fatigue(), 0.0, "a new player starts the day rested");
        assert_eq!(vitals.rest_regen_factor(), 1.0);
        assert_eq!(vitals.rest_speed_factor(), 1.0);

        // Most of a day awake, and it is still free: the costs are at
        // the top of the scale, not spread over the whole of it.
        vitals.tire(primitive_shared::body::WAKING_SECONDS * 0.5);
        assert!(vitals.fatigue() > 0.4 && vitals.fatigue() < 0.6);
        assert_eq!(vitals.rest_speed_factor(), 1.0, "half a day cost a player their pace");

        // ...and the whole of it is not.
        vitals.tire(primitive_shared::body::WAKING_SECONDS);
        assert_eq!(vitals.fatigue(), 1.0, "tiredness ran past the end of its scale");
        assert!(
            (vitals.rest_speed_factor() - primitive_shared::body::EXHAUSTED_SPEED).abs() < 1e-5
        );
        assert!(
            (vitals.rest_regen_factor() - primitive_shared::body::EXHAUSTED_REGEN).abs() < 1e-5
        );

        // A night on straw leaves a fifth of it -- which is what makes
        // the bed worth building and the straw worth having.
        let night = 8.0 * 60.0;
        let straw = primitive_shared::body::Rest::Straw;
        vitals.rest(
            night,
            primitive_shared::body::SLEEP_RECOVERY_PER_SECOND,
            1.0 - straw.recovery(),
        );
        assert!(
            (vitals.fatigue() - 0.2).abs() < 1e-5,
            "a night on straw left {} rather than a fifth",
            vitals.fatigue()
        );

        // ...and a night in a bed takes all of it.
        let bed = primitive_shared::body::Rest::Bed;
        vitals.rest(
            night,
            primitive_shared::body::SLEEP_RECOVERY_PER_SECOND,
            1.0 - bed.recovery(),
        );
        assert_eq!(vitals.fatigue(), 0.0, "a bed did not clear the night");
    }

    #[test]
    fn a_tired_body_mends_slowly_and_a_rested_one_does_not_notice() {
        // The cost that actually bites, and the check that it compounds
        // with the diet rather than replacing it: a tired player on a
        // poor diet heals at the product of the two, which is a ninth,
        // and neither rule on its own is fatal.
        let mut fresh = Vitals::new();
        fresh.set_health(10.0);
        fresh.set_nourishment(food::MAX_NOURISHMENT);
        let mut tired = Vitals::new();
        tired.set_health(10.0);
        tired.set_nourishment(food::MAX_NOURISHMENT);
        tired.set_fatigue(1.0);

        assert!(
            fresh.rest_regen_factor() > tired.rest_regen_factor(),
            "being finished cost nothing"
        );
        // ...and the two costs multiply rather than one replacing the
        // other: a tired player on one food group heals at a ninth.
        let poor_diet = food::diet_regen_factor(1);
        assert!(
            (poor_diet * tired.rest_regen_factor() - 1.0 / 9.0).abs() < 1e-5,
            "the two rules stopped compounding"
        );

        // The healing itself, over a second each, from the same state.
        let heal = |v: &mut Vitals| {
            let before = v.health();
            v.regenerate(1.0);
            v.health() - before
        };
        let by_fresh = heal(&mut fresh);
        let by_tired = heal(&mut tired);
        assert!(
            by_fresh > by_tired,
            "a finished body healed as fast as a rested one: {by_fresh} against {by_tired}"
        );
    }


    #[test]
    fn standing_in_a_fire_burns_and_stepping_out_stops_it() {
        // **The comment on `types::is_burning` has claimed this since
        // the day fire was added, and nothing did it.** A player could
        // stand in the middle of a burning campfire indefinitely.
        let mut vitals = Vitals::new();
        let full = vitals.health();

        assert!(matches!(vitals.burn(false, 1.0), Outcome::Unchanged), "burned by no fire");
        assert_eq!(vitals.health(), full);

        assert!(!matches!(vitals.burn(true, 1.0), Outcome::Unchanged), "a fire did nothing");
        let after = vitals.health();
        assert!(after < full, "standing in a fire cost nothing");
        // Survivable if you step out: a campfire is something a player
        // builds at their feet in the dark and walks into by accident.
        assert!(after > 0.0, "one second in a fire was fatal");

        // ...and stepping out stops it, rather than leaving something
        // burning down.
        assert!(matches!(vitals.burn(false, 1.0), Outcome::Unchanged));
        assert_eq!(vitals.health(), after);
    }

    #[test]
    fn standing_on_a_burning_pit_burns_as_standing_in_a_campfire_does() {
        // **"сделай урон от огненной ямы".** A burning pit kiln and a burning
        // charcoal pile fill their cells, so whoever stands on one has their
        // feet in the air cell above -- and only the cells a body was in were
        // asked. One fire in a dirt floor at y = 10, and a player's two points.
        use primitive_shared::pit::log_pile_lit;
        use primitive_shared::types::{
            BlockId, BLOCK_AIR, BLOCK_CAMPFIRE_LIT, BLOCK_DIRT, BLOCK_FIREPIT_LIT, BLOCK_KILN_LIT,
            BLOCK_PIT_KILN_LIT, BLOCK_PIT_KILN_LOGS,
        };
        const EYE: [f32; 1] = [1.62];
        let world = |fire: BlockId, y: i32| {
            move |cx: i32, cy: i32, cz: i32| {
                Some(match (cx, cy, cz) {
                    (0, _, 0) if cy == y => fire,
                    (_, 10, _) => BLOCK_DIRT,
                    _ => BLOCK_AIR,
                })
            }
        };
        for fire in [BLOCK_PIT_KILN_LIT, log_pile_lit(5)] {
            let pit = world(fire, 10);
            assert!(touches_fire((0.5, 11.0, 0.5), &EYE, pit), "standing on a burning pit {fire:#x} burned nothing");
            assert!(!touches_fire((0.5, 11.4, 0.5), &EYE, pit), "jumping over a pit {fire:#x} burned");
            assert!(!touches_fire((1.5, 11.0, 0.5), &EYE, pit), "the ground beside a pit {fire:#x} burned");
        }
        // The fires a body stands *in* still burn it there.
        for fire in [BLOCK_CAMPFIRE_LIT, BLOCK_FIREPIT_LIT] {
            let hearth = world(fire, 11);
            assert!(touches_fire((0.5, 11.25, 0.5), &EYE, hearth), "standing in a lit {fire:#x} burned nothing");
            assert!(!touches_fire((1.5, 11.0, 0.5), &EYE, hearth), "beside a lit {fire:#x} burned");
        }
        // A pit not yet lit is logs in a hole, and a kiln's roof is clay.
        assert!(!touches_fire((0.5, 11.0, 0.5), &EYE, world(BLOCK_PIT_KILN_LOGS, 10)), "an unlit pit burned");
        assert!(!touches_fire((0.5, 11.0, 0.5), &EYE, world(BLOCK_KILN_LIT, 10)), "the roof of a lit kiln burned");
    }

    #[test]
    fn a_fire_kills_in_seconds_rather_than_in_a_frame_or_a_minute() {
        // The rate is the whole design: fast enough that nobody stands
        // in one long enough to wonder, slow enough that walking through
        // one is a mistake rather than a death.
        let seconds = MAX_HEALTH / BURNING_PER_SECOND;
        assert!(seconds > 2.0, "a fire kills in {seconds}s -- a step through one is fatal");
        assert!(seconds < 15.0, "a fire kills in {seconds}s -- nobody would notice");
    }

    // ---- breathing ----

    #[test]
    fn thin_smoke_is_only_seen_and_thick_smoke_takes_the_breath_slower_than_water() {
        let mut thin = Vitals::new();
        for _ in 0..2000 {
            assert!(matches!(thin.breathe_smoke(0.3, 0.05), Outcome::Unchanged));
        }
        assert_eq!(thin.breath_fraction(), 1.0, "thin smoke took breath");

        let mut smoke = Vitals::new();
        let mut water = Vitals::new();
        for _ in 0..40 {
            smoke.breathe_smoke(0.6, 0.05);
            water.breathe(true, 0.05);
        }
        assert!(smoke.breath_fraction() < 1.0, "smoke over the line took no breath");
        assert!(smoke.breath_fraction() > water.breath_fraction(), "smoke choked as fast as water drowns");

        // Out of breath in the thickest smoke, it hurts, and gently.
        let mut choking = Vitals::new();
        for _ in 0..((BREATH_SECONDS + 2.0) / 0.05) as usize {
            choking.breathe_smoke(1.0, 0.05);
        }
        let lost = MAX_HEALTH - choking.health();
        assert!(lost > 0.0 && lost < DROWNING_PER_SECOND * 2.0 * 0.5, "two seconds out of breath in smoke cost {lost}");
    }

    #[test]
    fn a_head_above_water_never_runs_out_of_air() {
        let mut vitals = Vitals::new();
        for _ in 0..600 {
            assert!(matches!(vitals.breathe(false, 0.05), Outcome::Unchanged));
        }
        assert_eq!(vitals.health(), MAX_HEALTH);
        assert_eq!(vitals.breath_fraction(), 1.0);
    }

    #[test]
    fn a_long_dive_costs_nothing_and_a_longer_one_costs_everything() {
        // The shape of it: a lake crossing is free, staying under is
        // not, and the line between them is somewhere a player can feel
        // rather than a slow tax on swimming at all.
        let mut vitals = Vitals::new();
        let mut elapsed = 0.0;
        while elapsed < BREATH_SECONDS - 0.1 {
            vitals.breathe(true, 0.05);
            elapsed += 0.05;
        }
        assert_eq!(vitals.health(), MAX_HEALTH, "hurt before the air ran out");
        assert!(vitals.breath_fraction() < 0.02, "the meter did not empty");

        for _ in 0..40 {
            vitals.breathe(true, 0.05);
        }
        assert!(vitals.health() < MAX_HEALTH, "out of air and unharmed");
    }

    #[test]
    fn drowning_kills_and_says_so() {
        let mut vitals = Vitals::new();
        let mut cause = None;
        for _ in 0..2000 {
            if let Outcome::Died { cause: why } = vitals.breathe(true, 0.05) {
                cause = Some(why);
                break;
            }
        }
        assert_eq!(cause.as_deref(), Some("drowned"));
        assert!(vitals.is_dead());
    }

    #[test]
    fn the_meter_has_something_to_say_when_the_air_comes_back() {
        // The bug this pairs with lived in the tick loop rather than
        // here: readings were sent only while the head was under water,
        // so the last thing a client ever heard was "nearly out of air"
        // and it drew that bar for the rest of the session. The tick
        // loop now sends on *change*, which only works if surfacing is
        // a change this can see.
        let mut vitals = Vitals::new();
        for _ in 0..100 {
            vitals.breathe(true, 0.05);
        }
        let underwater = vitals.breath_fraction();
        assert!(underwater < 1.0);
        vitals.breathe(false, 0.05);
        assert!(
            (vitals.breath_fraction() - underwater).abs() > 0.01,
            "surfacing looked like no change at all"
        );
        assert_eq!(vitals.breath_fraction(), 1.0);
    }

    #[test]
    fn surfacing_is_a_whole_lungful() {
        // Metering the recovery would only make the second dive a worse
        // version of the first, which is a rule players work around by
        // waiting rather than by playing differently.
        let mut vitals = Vitals::new();
        for _ in 0..200 {
            vitals.breathe(true, 0.05);
        }
        assert!(vitals.breath_fraction() < 0.5);
        vitals.breathe(false, 0.05);
        assert_eq!(vitals.breath_fraction(), 1.0);
    }

    #[test]
    fn a_slow_tick_does_not_hand_out_free_seconds_under_water() {
        // The server can hitch. A tick worth two seconds must cost two
        // seconds of air, or a laggy server is a server where nobody
        // drowns.
        let mut quick = Vitals::new();
        for _ in 0..(BREATH_SECONDS / 0.05) as usize + 40 {
            quick.breathe(true, 0.05);
        }
        let mut slow = Vitals::new();
        for _ in 0..((BREATH_SECONDS + 2.0) / 1.0) as usize {
            slow.breathe(true, 1.0);
        }
        assert!(slow.health() < MAX_HEALTH, "a hitching server drowns nobody");
        assert!(quick.health() < MAX_HEALTH);
    }

    #[test]
    fn the_tick_the_air_runs_out_is_billed_for_the_part_that_was_dry() {
        // The tick that crosses zero is nearly all breathing and a
        // sliver of drowning, and only the sliver is charged. Charging
        // the whole tick takes a bite out of somebody who surfaced
        // almost in time -- and at a one-second tick, which a hitching
        // server produces, "almost" is a second of damage they did not
        // earn.
        //
        // Staged as one long tick that lands just past empty, so the
        // overshoot is a known number rather than whatever the loop
        // happened to leave behind.
        let mut vitals = Vitals::new();
        let overshoot = 0.1;
        vitals.breathe(true, BREATH_SECONDS + overshoot);
        let charged = MAX_HEALTH - vitals.health();
        let expected = DROWNING_PER_SECOND * overshoot;
        assert!(
            (charged - expected).abs() < 1e-3,
            "a tick with {overshoot}s of it out of air cost {charged} rather than {expected}"
        );

        // ...and the tick after it, which was out of air from end to
        // end, costs the whole of itself. The `min` has to keep doing
        // both jobs.
        let before = vitals.health();
        vitals.breathe(true, 0.5);
        let charged = before - vitals.health();
        assert!(
            (charged - DROWNING_PER_SECOND * 0.5).abs() < 1e-3,
            "a tick wholly under water was billed {charged}"
        );
    }

    #[test]
    fn coming_back_comes_back_with_air() {
        let mut vitals = Vitals::new();
        for _ in 0..2000 {
            vitals.breathe(true, 0.05);
        }
        assert!(vitals.is_dead());
        vitals.respawn();
        assert_eq!(vitals.breath_fraction(), 1.0);
        assert_eq!(vitals.health(), MAX_HEALTH);
    }

    use super::*;

    #[test]
    fn a_short_drop_is_free() {
        assert_eq!(fall_damage(0.0, false, 0.0), 0.0);
        assert_eq!(fall_damage(SAFE_FALL_BLOCKS, false, 0.0), 0.0);
        assert_eq!(fall_damage(SAFE_FALL_BLOCKS - 1.0, false, 0.0), 0.0);
    }

    #[test]
    fn a_long_drop_hurts_more_than_in_proportion() {
        // The quadratic term is the point. Doubling the distance has to
        // do *more* than double the damage, or height is a linear cost
        // and the difference between a ledge and a cliff is arithmetic
        // rather than a decision.
        let five = fall_damage(5.0 + SAFE_FALL_BLOCKS, false, 0.0);
        let ten = fall_damage(10.0 + SAFE_FALL_BLOCKS, false, 0.0);
        let twenty = fall_damage(20.0 + SAFE_FALL_BLOCKS, false, 0.0);
        assert!(five > 0.0);
        assert!(ten > five * 2.0, "twice the drop did not more than double the damage");
        assert!(twenty > ten * 2.0);
    }

    #[test]
    fn the_numbers_that_matter_land_where_they_are_meant_to() {
        // The three the whole curve was tuned for, and the reason to
        // write them down: they are what "more fall damage" actually
        // means, and a change to either constant that quietly moves the
        // fatal cliff to forty blocks would pass every other test here.
        assert!(fall_damage(6.0, false, 0.0) < MAX_HEALTH * 0.25, "a six-block drop should scratch");
        let half = fall_damage(12.0, false, 0.0);
        assert!(
            (MAX_HEALTH * 0.35..MAX_HEALTH * 0.75).contains(&half),
            "twelve blocks took {half} of {MAX_HEALTH}"
        );
        assert!(
            fall_damage(18.0, false, 0.0) >= MAX_HEALTH,
            "eighteen blocks should be the end of it"
        );
    }

    #[test]
    fn water_breaks_any_fall() {
        assert_eq!(fall_damage(60.0, true, 0.0), 0.0);
        // Even a full pack does not stop water working.
        assert_eq!(
            fall_damage(60.0, true, primitive_shared::load::CARRY_CAPACITY_KG),
            0.0
        );
    }

    #[test]
    fn a_loaded_landing_hurts_more() {
        let light = fall_damage(20.0, false, 0.0);
        let heavy = fall_damage(20.0, false, primitive_shared::load::CARRY_CAPACITY_KG);
        assert!(heavy > light, "weight did not make the landing worse");
        assert!(
            heavy <= light * 2.0 + 1e-4,
            "weight more than doubled the fall: {light} then {heavy}"
        );
    }

    #[test]
    fn a_reported_weight_is_clamped_rather_than_believed() {
        // The number comes off the wire from the client, so a broken or
        // hostile one must not be able to reach the damage maths.
        let mut vitals = Vitals::new();
        vitals.set_carried_weight(f32::NAN);
        assert_eq!(vitals.carried_weight(), 0.0);
        vitals.set_carried_weight(-500.0);
        assert_eq!(vitals.carried_weight(), 0.0);
        vitals.set_carried_weight(1e30);
        assert!(vitals.carried_weight() <= primitive_shared::load::MAX_BELIEVABLE_KG);
        vitals.set_carried_weight(120.0);
        assert_eq!(vitals.carried_weight(), 120.0);
    }

    #[test]
    fn carrying_a_load_turns_a_survivable_fall_into_one_that_puts_you_on_the_ground() {
        // The point of the whole mechanic: the trip down a shaft is
        // survivable empty-handed and a decision with a full pack.
        // Thirteen blocks is thirteen and a half points empty-handed and
        // twenty-seven with a full pack: comfortably either side of the
        // twenty that kills.
        let drop = 13.0;
        let mut empty = Vitals::new();
        assert_eq!(take_a_fall(&mut empty, drop, 0.0), Outcome::Changed);
        assert!(!empty.is_dead(), "the empty-handed fall should be survivable");

        // Down with broken legs rather than dead: the fall that "kills" by a
        // few points is the fall `downed` gives ninety seconds and a splint.
        let mut laden = Vitals::new();
        laden.set_carried_weight(primitive_shared::load::CARRY_CAPACITY_KG);
        take_a_fall(&mut laden, drop, 0.0);
        assert_eq!(
            laden.downed().map(|down| down.cause),
            Some(primitive_shared::downed::Cause::Fall),
            "the same fall with a full pack should not be"
        );
    }

    #[test]
    fn what_you_can_still_step_off_is_what_you_could_step_off_before() {
        // The other half of making falls hurt: the short drops a player
        // takes constantly -- off a bench in the terrain, down a bank,
        // out of a doorway -- have to stay free, or moving around the
        // world becomes a chore rather than height becoming dangerous.
        // The safe distance is deliberately unchanged.
        for drop in [0.0, 1.0, 2.0, SAFE_FALL_BLOCKS] {
            assert_eq!(fall_damage(drop, false, 0.0), 0.0, "a {drop}-block drop cost health");
        }
        // ...and one step past it is a scratch rather than a cliff.
        assert!(fall_damage(SAFE_FALL_BLOCKS + 1.0, false, 0.0) < 2.0);
    }

    /// Walks a player through a fall: leave the ground, rise, drop, land.
    fn take_a_fall(vitals: &mut Vitals, from: f32, to: f32) -> Outcome {
        vitals.on_transform(from, false, false);
        // A few sampled points on the way down.
        let mut y = from;
        while y > to {
            y -= 2.0;
            vitals.on_transform(y.max(to), false, false);
        }
        vitals.on_transform(to, true, false)
    }

    #[test]
    fn landing_after_a_long_fall_costs_health() {
        let mut vitals = Vitals::new();
        let outcome = take_a_fall(&mut vitals, 40.0, 28.0);
        assert_eq!(outcome, Outcome::Changed);
        assert!(vitals.health() < MAX_HEALTH, "the fall did nothing");
        assert!(vitals.health() > 0.0, "a twelve block fall should not be fatal");
    }

    #[test]
    fn a_fall_from_the_top_of_the_world_is_fatal() {
        // The other end of the curve: survivable cliffs are only worth
        // having if something is still lethal.
        let mut vitals = Vitals::new();
        let outcome = take_a_fall(&mut vitals, 60.0, 0.0);
        assert!(
            matches!(outcome, Outcome::Died { .. }),
            "a sixty block fall should kill, got {outcome:?}"
        );
    }

    #[test]
    fn a_drop_onto_a_stalagmite_that_rock_would_forgive_cuts_a_leg_and_bleeds() {
        // Two and a half blocks: under the three a fall onto rock hurts at.
        let mut rock = Vitals::new();
        assert_eq!(take_a_fall(&mut rock, 12.5, 10.0), Outcome::Unchanged);
        let mut spiked = Vitals::new();
        spiked.set_on_spike(true);
        assert_eq!(take_a_fall(&mut spiked, 12.5, 10.0), Outcome::Changed);
        assert!(spiked.health() < MAX_HEALTH, "the spike did nothing");
        assert!(spiked.injuries().is_bleeding(), "a spike cut that does not bleed is a scratch");
    }

    #[test]
    fn walking_through_stakes_cuts_a_leg_once_a_second_and_edging_through_does_not() {
        use std::time::{Duration, Instant};
        let start = Instant::now();
        // Twenty transforms a second across the ground at a walk, among
        // stakes the whole way: one cut on entering and one a second after.
        let mut walker = Vitals::new();
        let mut cuts = 0;
        for step in 0..=30 {
            let at = start + Duration::from_millis(step * 50);
            let x = step as f64 * 0.05 * 4.3;
            if walker.on_stakes(true, x, 0.0, at) != Outcome::Unchanged {
                cuts += 1;
            }
        }
        assert_eq!(cuts, 2, "a second and a half among stakes at a walk");
        assert!(walker.health() < MAX_HEALTH && walker.injuries().is_bleeding(), "a cut from stakes that does not bleed");
        // ...and the same ground at a shuffle.
        let mut edger = Vitals::new();
        for step in 0..=30 {
            let at = start + Duration::from_millis(step * 50);
            assert_eq!(edger.on_stakes(true, step as f64 * 0.05 * 0.6, 0.0, at), Outcome::Unchanged);
        }
        assert_eq!(edger.health(), MAX_HEALTH, "edging through the points cut anyway");
        // ...and a walk that never touches one.
        let mut clear = Vitals::new();
        for step in 0..=30 {
            let at = start + Duration::from_millis(step * 50);
            assert_eq!(clear.on_stakes(false, step as f64 * 0.05 * 4.3, 0.0, at), Outcome::Unchanged);
        }
    }

    #[test]
    fn a_hop_onto_a_stalagmite_does_not_cut() {
        let mut vitals = Vitals::new();
        vitals.set_on_spike(true);
        assert_eq!(take_a_fall(&mut vitals, 11.4, 10.0), Outcome::Unchanged);
        assert!(!vitals.injuries().is_bleeding());
    }

    #[test]
    fn landing_after_a_hop_costs_nothing() {
        let mut vitals = Vitals::new();
        let outcome = take_a_fall(&mut vitals, 12.0, 10.0);
        assert_eq!(outcome, Outcome::Unchanged);
        assert_eq!(vitals.health(), MAX_HEALTH);
    }

    #[test]
    fn walking_along_the_ground_never_hurts() {
        // Regression risk: every update says `on_ground`, and if the
        // tracker treated each one as a landing it would charge damage
        // for standing still.
        let mut vitals = Vitals::new();
        for _ in 0..100 {
            assert_eq!(vitals.on_transform(10.0, true, false), Outcome::Unchanged);
        }
        assert_eq!(vitals.health(), MAX_HEALTH);
    }

    #[test]
    fn a_fall_into_water_is_free_even_from_the_sky() {
        let mut vitals = Vitals::new();
        vitals.on_transform(60.0, false, false);
        vitals.on_transform(30.0, false, false);
        // Hits the water surface...
        vitals.on_transform(20.0, false, true);
        // ...and settles on the bottom.
        let outcome = vitals.on_transform(12.0, true, false);
        assert_eq!(outcome, Outcome::Unchanged);
        assert_eq!(vitals.health(), MAX_HEALTH, "water did not break the fall");
    }

    #[test]
    fn a_teleport_downwards_is_not_a_fall() {
        // `clear_fall` is what the anti-cheat's rubber-band and the
        // `/tp` command both rely on.
        let mut vitals = Vitals::new();
        vitals.on_transform(50.0, false, false);
        vitals.clear_fall();
        let outcome = vitals.on_transform(5.0, true, false);
        assert_eq!(outcome, Outcome::Unchanged);
        assert_eq!(vitals.health(), MAX_HEALTH);
    }

    #[test]
    fn enough_damage_kills_and_death_is_reported_once() {
        let mut vitals = Vitals::new();
        // Two bars and a bit: past `downed::OVERKILL`, so dead at once.
        let outcome = vitals.hurt(MAX_HEALTH * 2.0 + 5.0, "crushed");
        assert!(matches!(outcome, Outcome::Died { .. }));
        assert!(vitals.is_dead());
        assert_eq!(vitals.health(), 0.0);
        // Further damage on a corpse is not a second death.
        assert_eq!(vitals.hurt(5.0, "again"), Outcome::Unchanged);
    }

    #[test]
    fn the_dead_do_not_take_fall_damage() {
        let mut vitals = Vitals::new();
        vitals.hurt(f32::MAX, "killed");
        let outcome = take_a_fall(&mut vitals, 60.0, 0.0);
        assert_eq!(outcome, Outcome::Unchanged);
    }

    #[test]
    fn respawning_restores_everything() {
        let mut vitals = Vitals::new();
        vitals.hurt(f32::MAX, "killed");
        vitals.respawn();
        assert!(!vitals.is_dead());
        assert_eq!(vitals.health(), MAX_HEALTH);
        // And no fall is left in progress from before the death.
        assert_eq!(vitals.on_transform(0.0, true, false), Outcome::Unchanged);
    }

    #[test]
    fn a_player_killed_by_bad_water_does_not_come_back_still_ill() {
        let mut vitals = Vitals::new();
        vitals.set_hydration(0.0);
        vitals.catch_the_next_illness();
        assert!(vitals.drink_water(body::Water::Standing, 1.0), "a thirsty player could not drink a pond");
        vitals.digest_now();
        assert!(vitals.sick_for() > 0.0, "a pond made nobody ill");
        vitals.set_health(0.01);
        let mut died = false;
        for _ in 0..10_000 {
            let sick = vitals.sicken(0.05);
            if matches!(sick, Outcome::Died { .. }) || matches!(vitals.step_downed(0.05), Outcome::Died { .. }) {
                died = true;
                break;
            }
        }
        assert!(died, "bad water never killed a player at a hundredth of a heart");
        vitals.respawn();
        assert_eq!(vitals.sick_for(), 0.0, "a fresh body came back carrying the illness that killed the last one");
        assert_eq!(vitals.sicken(1.0), Outcome::Unchanged, "a respawned player was still being hurt by old water");
    }

    /// **The chain the fish-and-fountain report ran down.** Raw fish makes a
    /// player ill (`food::sickness_seconds`), the illness takes health a crumb
    /// a tick through `hurt`, and none of that is a wound -- so none of it may
    /// open a cut or shed a drop, whatever the client once drew for the health
    /// it lost. A minute at the server's own tick, past the end of the illness,
    /// with every step the tick loop runs on a body.
    #[test]
    fn eating_raw_fish_makes_a_player_ill_and_never_starts_bleeding() {
        let mut vitals = Vitals::new();
        vitals.set_nourishment(food::MAX_NOURISHMENT / 2.0);
        assert!(
            !matches!(vitals.eat(primitive_shared::types::BLOCK_RAW_FISH), Outcome::Unchanged),
            "a hungry player could not eat a raw fish"
        );
        assert!(vitals.illness_owed() > 0.0, "raw fish made nobody ill");
        vitals.digest_now();
        assert!(vitals.sick_for() > 0.0, "raw fish, digested, made nobody ill");
        let before = vitals.health();
        let mut drops = 0u32;
        for _ in 0..(60 * 20) {
            vitals.sicken(0.05);
            vitals.mend(0.05, false);
            drops += u32::from(vitals.drips(0.05));
        }
        assert!(vitals.health() < before, "the illness cost nothing");
        assert!(vitals.injuries().is_whole(), "a fish left a wound: {:?}", vitals.injuries());
        assert_eq!(drops, 0, "a poison tick shed {drops} drops of blood");
    }

    /// A cut is the one harm that leaks, and it leaks a drop at a time: every
    /// part of a body cut to the bone, for half a minute of ticks, shows no more
    /// than `injury::MAX_DRIPS_PER_SECOND` a second and never two seconds' worth
    /// in one.
    #[test]
    fn a_bleeding_cut_sheds_no_more_than_a_few_drops_a_second() {
        let mut vitals = Vitals::new();
        let mut wounds = *vitals.injuries();
        for part in injury::Part::ALL {
            wounds.inflict(part, injury::Kind::Cut, 1.0);
        }
        vitals.set_injuries(wounds);
        let (seconds, dt) = (30.0f32, 0.05f32);
        let (mut drops, mut this_second, mut worst_second) = (0u32, 0u32, 0u32);
        for tick in 0..(seconds / dt) as usize {
            let shed = u32::from(vitals.drips(dt));
            drops += shed;
            this_second += shed;
            if (tick + 1) % 20 == 0 {
                worst_second = worst_second.max(this_second);
                this_second = 0;
            }
        }
        assert!(drops > 0, "a body cut to the bone never showed a drop");
        assert!(
            drops as f32 <= injury::MAX_DRIPS_PER_SECOND * seconds + 1.0,
            "{drops} drops in {seconds}s"
        );
        assert!(worst_second <= 2, "one second of bleeding showed {worst_second} drops");
    }

    #[test]
    fn regeneration_waits_and_then_heals() {
        let mut vitals = Vitals::new();
        vitals.hurt(5.0, "ouch");
        let hurt = vitals.health();
        // Immediately after the hit, nothing happens.
        assert_eq!(vitals.regenerate(1.0), Outcome::Unchanged);
        assert_eq!(vitals.health(), hurt);

        // Pretend the delay has passed.
        vitals.last_damage = Instant::now()
            - std::time::Duration::from_secs_f32(REGEN_DELAY_SECS + 1.0);
        assert_eq!(vitals.regenerate(1.0), Outcome::Changed);
        assert!(vitals.health() > hurt, "no healing happened");
    }

    #[test]
    fn a_roofed_room_with_a_bed_heals_faster_than_a_cold_wet_field_beside_dung() {
        use primitive_shared::types::{BLOCK_AIR, BLOCK_BED, BLOCK_DUNG, BLOCK_PLANKS, BLOCK_STONE};
        // A plank hut, three by three inside, on stone at y = 9 -- or the same
        // ground with no hut and two pats of dung beside the feet.
        let hut = |x: i32, y: i32, z: i32| {
            Some(if y < 10 {
                BLOCK_STONE
            } else if (x, y, z) == (1, 10, 1) {
                BLOCK_BED
            } else if (-2..=2).contains(&x) && (-2..=2).contains(&z) && (y == 12 || (y < 12 && (x.abs() == 2 || z.abs() == 2))) {
                BLOCK_PLANKS
            } else {
                BLOCK_AIR
            })
        };
        let field = |x: i32, y: i32, z: i32| {
            Some(if y < 10 {
                BLOCK_STONE
            } else if y == 10 && (x, z) != (0, 0) && x.abs() <= 1 && z == 1 {
                BLOCK_DUNG
            } else {
                BLOCK_AIR
            })
        };
        let home = primitive_shared::comfort::survey(hut, (0, 10, 0));
        let out = primitive_shared::comfort::survey(field, (0, 10, 0));
        let ready = |vitals: &mut Vitals| {
            for &group in &food::Group::ALL {
                vitals.ate_group(group, Instant::now());
            }
            vitals.set_health(10.0);
            vitals.last_damage = Instant::now() - std::time::Duration::from_secs_f32(REGEN_DELAY_SECS + 1.0);
        };
        let mut indoors = Vitals::new();
        ready(&mut indoors);
        let mut outdoors = Vitals::new();
        ready(&mut outdoors);
        // Chilled and soaked, but not past the line where healing stops
        // outright: this is the rate, not the gate.
        outdoors.set_warmth(body::CHILLED, 1.0);
        for _ in 0..(90 * 20) {
            indoors.settle_comfort(home, false, 0.0, false, false, 0.05);
            outdoors.settle_comfort(out, false, 0.0, false, true, 0.05);
        }
        assert!(indoors.comfort_level() > 0.4, "a furnished hut was not comfortable: {}", indoors.comfort_level());
        assert!(outdoors.comfort_level() < -0.4, "a wet field by dung was not miserable: {}", outdoors.comfort_level());
        let (a, b) = (indoors.health(), outdoors.health());
        indoors.regenerate(10.0);
        outdoors.regenerate(10.0);
        let (home_gain, field_gain) = (indoors.health() - a, outdoors.health() - b);
        assert!(
            home_gain > field_gain * 1.5,
            "ten seconds healed {home_gain} at home and {field_gain} in the field"
        );
    }

    #[test]
    fn eating_a_day_of_food_owes_dung_that_waits_for_the_door() {
        let mut vitals = Vitals::new();
        use primitive_shared::types::BLOCK_COOKED_MEAT;
        while vitals.dung_owed < primitive_shared::comfort::DUNG_PER_BAR {
            vitals.nourishment = 0.0;
            vitals.eat(BLOCK_COOKED_MEAT);
        }
        assert!(!vitals.goes_now(1.0), "went on the floor as soon as it was due");
        assert!(vitals.goes_now(0.0));
        vitals.went();
        assert!(!vitals.goes_now(0.0), "went twice for one debt");
    }

    #[test]
    fn healing_from_near_death_to_full_takes_about_half_a_day() {
        // The rate stated as the thing it is for. A wound is a reason
        // to go home: a full bar is minutes, not seconds -- and not a
        // whole session either, or a scratch becomes a tax. The default
        // day is nine hundred seconds.
        //
        // Measured on a **full diet**, which is what `REGEN_PER_SECOND`
        // means: the diet multiplier is a share of this rate, and a
        // player living on one thing takes three times as long (see
        // `food::diet_regen_factor`). Without the meals below this test
        // would be measuring the slow rate and calling it the rate.
        let mut vitals = Vitals::new();
        let now = Instant::now();
        for &group in &food::Group::ALL {
            vitals.ate_group(group, now);
        }
        vitals.hurt(MAX_HEALTH - 1.0, "a cliff");
        vitals.last_damage = Instant::now()
            - std::time::Duration::from_secs_f32(REGEN_DELAY_SECS + 1.0);
        let mut seconds = 0.0;
        while vitals.health() < MAX_HEALTH - 0.01 && seconds < 3600.0 {
            vitals.regenerate(0.05);
            seconds += 0.05;
        }
        assert!(seconds > 240.0, "near death to full took {seconds}s -- a wound that heals in a walk");
        assert!(seconds < 900.0, "near death to full took {seconds}s -- longer than a whole day");
        // ...and the eight points a boar takes are a matter of minutes,
        // not of the whole afternoon.
        let boar = 8.0 / REGEN_PER_SECOND;
        assert!((60.0..300.0).contains(&boar), "a boar's worth heals in {boar}s");
    }

    #[test]
    fn a_freezing_player_does_not_heal_until_they_are_warm_again() {
        // The cold takes and the body does not give back at the same
        // time. The gate is the body temperature, not the damage
        // having happened: a player carried in from the snow who is
        // still below the line is still not healing, however long ago
        // the last point was taken.
        let mut vitals = Vitals::new();
        vitals.hurt(6.0, "a wolf");
        vitals.last_damage = Instant::now()
            - std::time::Duration::from_secs_f32(REGEN_DELAY_SECS + 1.0);
        let hurt = vitals.health();

        vitals.set_warmth(body::FREEZING - 2.0, 0.0);
        assert_eq!(vitals.regenerate(5.0), Outcome::Unchanged, "healed while freezing");
        assert_eq!(vitals.health(), hurt);
        vitals.set_warmth(body::SCALDING + 2.0, 0.0);
        assert_eq!(vitals.regenerate(5.0), Outcome::Unchanged, "healed with heatstroke");
        assert_eq!(vitals.health(), hurt);

        // Merely cold is not the line: shivering costs food, not the
        // right to mend. See `body::being_cold_makes_a_player_hungry_before_it_hurts_them`.
        vitals.set_warmth(body::CHILLED - 1.0, 0.0);
        assert_eq!(vitals.regenerate(5.0), Outcome::Changed, "a cold player could not heal");
        assert!(vitals.health() > hurt);
    }

    #[test]
    fn regeneration_stops_at_full_and_never_revives_the_dead() {
        let mut vitals = Vitals::new();
        assert_eq!(vitals.regenerate(10.0), Outcome::Unchanged);
        assert_eq!(vitals.health(), MAX_HEALTH);

        vitals.hurt(f32::MAX, "killed");
        vitals.last_damage = Instant::now()
            - std::time::Duration::from_secs_f32(REGEN_DELAY_SECS + 1.0);
        assert_eq!(vitals.regenerate(100.0), Outcome::Unchanged);
        assert!(vitals.is_dead(), "regeneration resurrected a dead player");
    }

    #[test]
    fn only_meaningful_changes_are_worth_a_message() {
        let mut vitals = Vitals::new();
        assert!(!vitals.needs_report(), "a fresh player is already in sync");

        vitals.hurt(3.0, "ouch");
        assert!(vitals.needs_report());
        vitals.mark_reported();
        assert!(!vitals.needs_report());

        // A sliver of regeneration is not worth a packet.
        vitals.last_damage = Instant::now()
            - std::time::Duration::from_secs_f32(REGEN_DELAY_SECS + 1.0);
        vitals.regenerate(0.01);
        assert!(!vitals.needs_report(), "reporting noise-level changes");
    }
}

#[cfg(test)]
mod downed_tests {
    use super::*;
    use primitive_shared::downed::{Cause, OVERKILL, RAISED_HEALTH, SECONDS_PER_HEALTH};
    use primitive_shared::types::{BLOCK_BANDAGE, BLOCK_BREAD, BLOCK_SPLINT};

    /// Runs a downed body's clock at 20 Hz until it gets up, dies, or
    /// `seconds` pass. Answers the outcome that ended it, if one did.
    fn lie_for(vitals: &mut Vitals, seconds: f32) -> Outcome {
        let mut t = 0.0;
        while t < seconds {
            let outcome = vitals.step_downed(0.05);
            if outcome != Outcome::Unchanged {
                return outcome;
            }
            t += 0.05;
        }
        Outcome::Unchanged
    }

    #[test]
    fn the_overkill_line_is_one_whole_bar_of_health() {
        assert_eq!(OVERKILL, MAX_HEALTH, "downed::OVERKILL has drifted from the server's bar");
    }

    #[test]
    fn a_body_that_gives_out_goes_down_and_is_not_dead() {
        let mut vitals = Vitals::new();
        assert_eq!(vitals.hurt(MAX_HEALTH, "was pulled down by a wolf"), Outcome::Changed);
        assert!(!vitals.is_dead());
        let down = vitals.downed().expect("not down");
        assert_eq!(down.cause, Cause::Wound);
        assert_eq!(vitals.health(), 0.0);
        assert!(vitals.take_downed_report(), "the client was never told it was down");
        assert!(!vitals.take_downed_report(), "told twice");
    }

    #[test]
    fn drowning_a_tree_and_a_blow_a_bar_past_zero_still_kill_at_once() {
        let mut drowned = Vitals::new();
        assert!(matches!(drowned.hurt(MAX_HEALTH, "drowned"), Outcome::Died { .. }));
        let mut crushed = Vitals::new();
        assert!(matches!(crushed.hurt(f32::MAX, "was crushed by a falling tree"), Outcome::Died { .. }));
        let mut cliff = Vitals::new();
        assert!(matches!(cliff.hurt(MAX_HEALTH * 2.0 + 1.0, "fell from a great height"), Outcome::Died { .. }));
        assert!(cliff.is_dead() && cliff.downed().is_none());
    }

    #[test]
    fn a_downed_body_left_alone_dies_when_the_clock_runs_out_with_the_words_that_downed_it() {
        let mut vitals = Vitals::new();
        vitals.hurt(MAX_HEALTH, "fell from a great height");
        let of = vitals.downed().unwrap().of;
        assert_eq!(lie_for(&mut vitals, of - 1.0), Outcome::Unchanged, "died before the clock ran out");
        assert_eq!(
            lie_for(&mut vitals, 2.0),
            Outcome::Died { cause: "fell from a great height".to_string() }
        );
        assert!(vitals.is_dead());
    }

    #[test]
    fn an_animal_still_biting_a_downed_body_finishes_it_sooner() {
        let mut vitals = Vitals::new();
        vitals.hurt(MAX_HEALTH, "was pulled down by a wolf");
        let before = vitals.downed().unwrap().left;
        assert_eq!(vitals.hurt(5.0, "was pulled down by a wolf"), Outcome::Unchanged);
        let after = vitals.downed().unwrap().left;
        assert!((before - after - 5.0 * SECONDS_PER_HEALTH).abs() < 1e-3, "a bite took {} s", before - after);
        let mut bites = 0;
        while !vitals.is_dead() {
            vitals.hurt(5.0, "was pulled down by a wolf");
            bites += 1;
            assert!(bites < 10, "a wolf never finished a downed player");
        }
    }

    #[test]
    fn the_hunger_that_downed_a_body_is_not_billed_twice_but_a_fall_on_top_of_it_is() {
        let mut vitals = Vitals::new();
        vitals.hurt(MAX_HEALTH, "starved");
        let left = vitals.downed().unwrap().left;
        vitals.hurt(1.0, "starved");
        assert_eq!(vitals.downed().unwrap().left, left, "the hunger that was the clock took from the clock");
        vitals.hurt(2.0, "fell from a great height");
        assert!(vitals.downed().unwrap().left < left, "a fall onto a starving body cost nothing");
    }

    #[test]
    fn a_starving_body_on_the_ground_is_raised_by_a_mouthful() {
        let mut vitals = Vitals::new();
        vitals.set_nourishment(0.0);
        vitals.hurt(MAX_HEALTH, "starved");
        assert!(!matches!(vitals.eat(BLOCK_BREAD), Outcome::Unchanged), "the bread was refused");
        assert_eq!(vitals.step_downed(0.05), Outcome::Changed);
        assert!(vitals.downed().is_none());
        assert_eq!(vitals.health(), RAISED_HEALTH);
        assert!(!vitals.is_dead());
    }

    #[test]
    fn a_bandage_raises_a_bitten_body_even_where_the_bite_left_only_a_bruise() {
        let mut vitals = Vitals::new();
        vitals.hurt(MAX_HEALTH, "was gored by a boar");
        assert!(vitals.treat(injury::Part::Head, BLOCK_BANDAGE).is_ok(), "the bandage was refused to a downed body");
        assert_eq!(vitals.step_downed(0.05), Outcome::Changed);
        assert!(vitals.downed().is_none());
    }

    #[test]
    fn the_wrong_help_is_no_help() {
        // A bandage on a body the cold put down, and bread on a broken one.
        let mut cold = Vitals::new();
        cold.set_warmth(body::FREEZING - 5.0, 0.0);
        cold.hurt(MAX_HEALTH, "froze to death");
        assert!(cold.treat(injury::Part::Head, BLOCK_BANDAGE).is_err());
        assert_eq!(cold.step_downed(0.05), Outcome::Unchanged);
        let mut fallen = Vitals::new();
        fallen.set_nourishment(5.0);
        fallen.hurt(MAX_HEALTH, "fell from a great height");
        fallen.eat(BLOCK_BREAD);
        assert_eq!(fallen.step_downed(0.05), Outcome::Unchanged, "bread set a broken leg");
        assert!(fallen.treat(injury::Part::Torso, BLOCK_SPLINT).is_ok());
        assert_eq!(fallen.step_downed(0.05), Outcome::Changed);
    }

    #[test]
    fn a_freezing_body_is_raised_by_the_fire_it_crawled_to() {
        let mut vitals = Vitals::new();
        vitals.set_warmth(body::FREEZING - 4.0, 0.0);
        vitals.hurt(MAX_HEALTH, "froze to death");
        assert_eq!(vitals.step_downed(0.05), Outcome::Unchanged);
        vitals.warm_by(8.0);
        assert_eq!(vitals.step_downed(0.05), Outcome::Changed, "warm again at {}", vitals.temperature());
    }

    #[test]
    fn a_downed_body_does_not_mend_and_letting_go_is_dying_now() {
        let mut vitals = Vitals::new();
        vitals.hurt(MAX_HEALTH, "was pulled down by a wolf");
        vitals.last_damage = Instant::now() - std::time::Duration::from_secs(3600);
        assert_eq!(vitals.regenerate(100.0), Outcome::Unchanged);
        assert_eq!(vitals.health(), 0.0);
        assert_eq!(vitals.give_up(), Outcome::Died { cause: "was pulled down by a wolf".to_string() });
        vitals.respawn();
        assert!(vitals.downed().is_none() && !vitals.is_dead());
        assert_eq!(vitals.give_up(), Outcome::Unchanged, "a standing player gave up");
    }
}
