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

/// Health per second once regeneration starts.
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

pub const REGEN_PER_SECOND: f32 = 0.6;

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

pub struct Vitals {
    health: f32,
    dead: bool,
    /// Highest point reached since leaving the ground. `None` while
    /// standing on something.
    fall_peak_y: Option<f32>,
    /// Whether the last accepted transform said the player was airborne.
    airborne: bool,
    last_damage: Instant,
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
            last_reported: MAX_HEALTH,
            breath: BREATH_SECONDS,
            carried_kg: 0.0,
        }
    }

    /// Records the current load. Still sanitised: the value is derived
    /// from block weights and stack counts, and a nonsense one would
    /// reach the damage maths.
    pub fn set_carried_weight(&mut self, kilograms: f32) {
        self.carried_kg = primitive_shared::load::sanitize(kilograms);
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
        if damage <= 0.0 {
            return Outcome::Unchanged;
        }
        self.hurt(damage, "fell from a great height")
    }

    /// Applies damage and reports what it did.
    pub fn hurt(&mut self, amount: f32, cause: &str) -> Outcome {
        if self.dead || amount <= 0.0 {
            return Outcome::Unchanged;
        }
        self.health = (self.health - amount).max(0.0);
        self.last_damage = Instant::now();
        if self.health <= 0.0 {
            self.dead = true;
            self.fall_peak_y = None;
            return Outcome::Died {
                cause: cause.to_string(),
            };
        }
        Outcome::Changed
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

    /// How much air is left, as a fraction. Drawn by the HUD.
    pub fn breath_fraction(&self) -> f32 {
        (self.breath / BREATH_SECONDS).clamp(0.0, 1.0)
    }

    pub fn regenerate(&mut self, dt: f32) -> Outcome {
        if self.dead || self.health >= MAX_HEALTH {
            return Outcome::Unchanged;
        }
        if self.last_damage.elapsed().as_secs_f32() < REGEN_DELAY_SECS {
            return Outcome::Unchanged;
        }
        self.health = (self.health + REGEN_PER_SECOND * dt).min(MAX_HEALTH);
        Outcome::Changed
    }

    /// Back to full, alive, and with no fall in progress.
    pub fn respawn(&mut self) {
        self.breath = BREATH_SECONDS;
        self.health = MAX_HEALTH;
        self.dead = false;
        self.clear_fall();
        // Deliberately *not* setting `last_reported`: the client has to
        // be told about the restored health, and pretending it already
        // knows would leave a fresh spawn showing an empty bar.
        self.last_damage = Instant::now() - std::time::Duration::from_secs(3600);
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
mod tests {
    // ---- breathing ----

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
    fn carrying_a_load_turns_a_survivable_fall_into_a_fatal_one() {
        // The point of the whole mechanic: the trip down a shaft is
        // survivable empty-handed and a decision with a full pack.
        // Thirteen blocks is thirteen and a half points empty-handed and
        // twenty-seven with a full pack: comfortably either side of the
        // twenty that kills.
        let drop = 13.0;
        let mut empty = Vitals::new();
        assert_eq!(take_a_fall(&mut empty, drop, 0.0), Outcome::Changed);
        assert!(!empty.is_dead(), "the empty-handed fall should be survivable");

        let mut laden = Vitals::new();
        laden.set_carried_weight(primitive_shared::load::CARRY_CAPACITY_KG);
        assert!(
            matches!(take_a_fall(&mut laden, drop, 0.0), Outcome::Died { .. }),
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
        let outcome = vitals.hurt(MAX_HEALTH + 5.0, "crushed");
        assert!(matches!(outcome, Outcome::Died { .. }));
        assert!(vitals.is_dead());
        assert_eq!(vitals.health(), 0.0);
        // Further damage on a corpse is not a second death.
        assert_eq!(vitals.hurt(5.0, "again"), Outcome::Unchanged);
    }

    #[test]
    fn the_dead_do_not_take_fall_damage() {
        let mut vitals = Vitals::new();
        vitals.hurt(MAX_HEALTH, "killed");
        let outcome = take_a_fall(&mut vitals, 60.0, 0.0);
        assert_eq!(outcome, Outcome::Unchanged);
    }

    #[test]
    fn respawning_restores_everything() {
        let mut vitals = Vitals::new();
        vitals.hurt(MAX_HEALTH, "killed");
        vitals.respawn();
        assert!(!vitals.is_dead());
        assert_eq!(vitals.health(), MAX_HEALTH);
        // And no fall is left in progress from before the death.
        assert_eq!(vitals.on_transform(0.0, true, false), Outcome::Unchanged);
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
    fn regeneration_stops_at_full_and_never_revives_the_dead() {
        let mut vitals = Vitals::new();
        assert_eq!(vitals.regenerate(10.0), Outcome::Unchanged);
        assert_eq!(vitals.health(), MAX_HEALTH);

        vitals.hurt(MAX_HEALTH, "killed");
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
