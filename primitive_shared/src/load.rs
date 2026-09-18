//! What carrying things costs.
//!
//! Shared because the two consequences of a load live on opposite sides
//! of the wire: the client slows the player down, and the server hurts
//! them harder when they land. If the two disagreed about what "heavy"
//! meant, a player would be punished for a load they were never shown.
//!
//! Everything here is a pure function of one number -- kilograms -- so
//! that number is the only thing that has to cross the network.

/// What a player can carry before the penalties are at their worst, in
/// kilograms.
///
/// **Ninety, and it was six hundred.** The old number was sized against the
/// stack limit -- two full stacks of stone -- and the result was a player
/// who walked off with a third of a tonne at half pace and thought nothing
/// of it: "сейчас игрок носит чуть-ли не тонны и ему нормально". Ninety
/// kilograms is what a strong person staggers under with a frame on their
/// back; a stack of stone is now three loads, not a third of one, and how
/// much to take on each trip is the decision weight was always meant to be.
/// Stacks stay as large as they were: a slot is storage, and a chest of
/// stone is a fine thing to own. It is walking off with it that costs.
pub const CARRY_CAPACITY_KG: f32 = 90.0;

/// What a player carries without noticing, in kilograms.
///
/// **Twenty-five: the tools, a day's food, a jug of water.** Below it
/// nothing is slower and nothing costs breath, because a player who felt
/// every stick they picked up would learn to leave sticks lying rather than
/// to plan a haul. Above it the penalties ramp up to their worst at
/// `CARRY_CAPACITY_KG`.
pub const FREE_CARRY_KG: f32 = 25.0;

/// Speed at a full load, as a fraction of unencumbered speed.
///
/// Not lower, and never zero. A player who cannot move cannot get
/// anywhere to put anything down, and "I am stuck and do not know why"
/// is a worse outcome than any amount of realism buys.
const MIN_SPEED_SCALE: f32 = 0.45;
/// The floor once far past capacity: a shuffle, and still a move.
const OVERLOADED_SPEED_SCALE: f32 = 0.15;
/// How many capacities of load it takes to get down to that floor. Past
/// capacity the pace keeps falling rather than stepping down once, so two
/// hundred kilograms is worse than a hundred -- the old single step made
/// "overloaded" one state however much was carried.
const OVERLOAD_SPAN: f32 = 2.0;

/// How much worse a landing is at a full load.
const MAX_FALL_MULTIPLIER: f32 = 2.0;

/// How loaded a player is, 0 (nothing felt) to 1 (at capacity).
///
/// Zero up to `FREE_CARRY_KG`, so every consequence that reads it -- the
/// pace, the breath, the landing -- starts where the load starts to be
/// felt. Saturates: carrying more than capacity is still 1 here, and the
/// extra punishment lives in `speed_scale` alone.
pub fn load_fraction(kilograms: f32) -> f32 {
    if !kilograms.is_finite() || kilograms <= FREE_CARRY_KG {
        return 0.0;
    }
    ((kilograms - FREE_CARRY_KG) / (CARRY_CAPACITY_KG - FREE_CARRY_KG)).clamp(0.0, 1.0)
}

/// Movement speed multiplier for a given weight.
pub fn speed_scale(kilograms: f32) -> f32 {
    let load = load_fraction(kilograms);
    let scale = 1.0 - (1.0 - MIN_SPEED_SCALE) * load;
    if kilograms > CARRY_CAPACITY_KG {
        let over = ((kilograms - CARRY_CAPACITY_KG) / (CARRY_CAPACITY_KG * OVERLOAD_SPAN)).clamp(0.0, 1.0);
        return MIN_SPEED_SCALE - (MIN_SPEED_SCALE - OVERLOADED_SPEED_SCALE) * over;
    }
    scale
}

/// Whether a body under this load can jump at all.
///
/// **Not past capacity.** A player with ninety kilograms on their back
/// does not hop onto a boulder, and one who could would carry a quarry up
/// a cliff a block at a time -- the route the weight was supposed to make
/// them think about.
pub fn can_jump(kilograms: f32) -> bool {
    !kilograms.is_finite() || kilograms <= CARRY_CAPACITY_KG
}

/// Fall damage multiplier for a given weight.
///
/// Mass makes a landing worse, which is the other half of why weight is
/// worth tracking: it turns a heavy haul out of a deep mine into a route
/// choice rather than a straight drop down the shaft.
pub fn fall_multiplier(kilograms: f32) -> f32 {
    1.0 + (MAX_FALL_MULTIPLIER - 1.0) * load_fraction(kilograms)
}

/// The load at which a swimmer stops floating altogether, in kilograms.
///
/// **Half of carrying capacity, and the number is the mechanic.** A
/// player with a pack of tools and a day's food floats and swims as
/// they always did; a player hauling three hundred kilograms of stone
/// out of a flooded shaft does not, and has to decide -- between the
/// stone and the far bank -- while the breath meter runs. That is the
/// decision the player asked for when they asked to sink under weight,
/// and it is one they can always undo, because dropping a stack is a
/// keypress.
pub const SWIM_SINK_KG: f32 = CARRY_CAPACITY_KG * 0.5;

/// The load at which floating starts to suffer.
///
/// A fifth of capacity: below it nothing about the water has changed at
/// all, which keeps the ordinary swim -- across a river, into a lake --
/// exactly what it was.
pub const SWIM_FREE_KG: f32 = CARRY_CAPACITY_KG * 0.2;

/// How much of the water's lift a player at this weight still gets,
/// 1 (bobs like a cork) to 0 (goes down like the stone they are
/// carrying).
///
/// A straight ramp between the two numbers above. Shared because both
/// sides need it and for the reason the whole file is shared: the
/// client is what makes the player sink and the server is what drowns
/// them for it, and a disagreement would be a death nobody could see
/// coming.
pub fn buoyancy(kilograms: f32) -> f32 {
    if !kilograms.is_finite() || kilograms <= SWIM_FREE_KG {
        return 1.0;
    }
    if kilograms >= SWIM_SINK_KG {
        return 0.0;
    }
    1.0 - (kilograms - SWIM_FREE_KG) / (SWIM_SINK_KG - SWIM_FREE_KG)
}

/// How much harder swimming is at this weight, as a multiplier on what
/// a stroke costs in nourishment.
///
/// One when light, three at the sinking line: swimming loaded is work,
/// and the bar going down is the game saying so before the breath
/// meter has to.
pub fn swim_effort(kilograms: f32) -> f32 {
    1.0 + 2.0 * (1.0 - buoyancy(kilograms))
}

/// The most weight the server will believe from a client.
///
/// Forty slots of the heaviest block at the largest stack size, with
/// room to spare. A client reporting more than this is either broken or
/// lying, and either way the number is not usable.
pub const MAX_BELIEVABLE_KG: f32 = 20_000.0;

/// Clamps a client-reported weight into something usable.
pub fn sanitize(kilograms: f32) -> f32 {
    if !kilograms.is_finite() || kilograms < 0.0 {
        return 0.0;
    }
    kilograms.min(MAX_BELIEVABLE_KG)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_empty_pack_costs_nothing() {
        assert_eq!(load_fraction(0.0), 0.0);
        assert_eq!(speed_scale(0.0), 1.0);
        assert_eq!(fall_multiplier(0.0), 1.0);
    }

    #[test]
    fn a_heavier_pack_is_always_slower_and_always_hurts_more() {
        let mut last_speed = f32::MAX;
        let mut last_fall = 0.0;
        for kg in (0..=400).step_by(5) {
            let speed = speed_scale(kg as f32);
            let fall = fall_multiplier(kg as f32);
            assert!(speed <= last_speed, "speed rose at {kg} kg");
            assert!(fall >= last_fall, "fall damage fell at {kg} kg");
            last_speed = speed;
            last_fall = fall;
        }
    }

    #[test]
    fn a_days_kit_is_not_felt_and_a_third_of_a_tonne_is_a_shuffle() {
        // "сейчас игрок носит чуть-ли не тонны и ему нормально".
        assert_eq!(speed_scale(FREE_CARRY_KG), 1.0, "the tools and a day's food slowed a player down");
        assert!(speed_scale(300.0) < 0.25, "three hundred kilograms is still a walk: {}", speed_scale(300.0));
        assert!(!can_jump(CARRY_CAPACITY_KG + 1.0), "a player jumped under more than they can carry");
        assert!(can_jump(FREE_CARRY_KG));
    }

    #[test]
    fn a_full_load_is_slow_but_never_immobile() {
        let scale = speed_scale(CARRY_CAPACITY_KG);
        assert!(scale >= MIN_SPEED_SCALE - 1e-6);
        assert!(scale > 0.0, "a full load must not freeze the player");
        assert!(scale < 1.0);
    }

    #[test]
    fn being_overloaded_is_worse_still_but_still_moves() {
        let full = speed_scale(CARRY_CAPACITY_KG);
        let over = speed_scale(CARRY_CAPACITY_KG * 4.0);
        assert!(over < full, "overloading felt the same as a full load");
        assert!(over > 0.0, "an overloaded player cannot put anything down");
    }

    #[test]
    fn a_full_load_makes_a_fall_hurt_twice_as_much() {
        assert!((fall_multiplier(CARRY_CAPACITY_KG) - MAX_FALL_MULTIPLIER).abs() < 1e-5);
        // And it saturates rather than running away.
        assert!((fall_multiplier(CARRY_CAPACITY_KG * 10.0) - MAX_FALL_MULTIPLIER).abs() < 1e-5);
    }

    #[test]
    fn nonsense_weights_are_refused_rather_than_believed() {
        // This number comes off the wire, so none of it is trusted.
        assert_eq!(sanitize(f32::NAN), 0.0);
        assert_eq!(sanitize(-100.0), 0.0);
        assert_eq!(sanitize(f32::INFINITY), 0.0);
        assert_eq!(sanitize(1e30), MAX_BELIEVABLE_KG);
        assert_eq!(sanitize(50.0), 50.0);

        // And the derived values stay sane whatever they are handed.
        for kg in [f32::NAN, f32::INFINITY, -1.0, 1e30] {
            assert!(speed_scale(kg).is_finite() && speed_scale(kg) > 0.0);
            assert!(fall_multiplier(kg).is_finite() && fall_multiplier(kg) >= 1.0);
        }
    }

    #[test]
    fn a_stack_of_stone_is_several_trips_and_not_one() {
        // The tuning claim in the docs, kept honest: a slot of stone is
        // storage, and walking off with a whole one is a mistake.
        let stack = crate::types::block_weight(crate::types::BLOCK_STONE) * 128.0;
        let loads = stack / CARRY_CAPACITY_KG;
        assert!(
            (2.0..6.0).contains(&loads),
            "a stack of stone is {loads:.1} loads, which makes weight either pointless or unplayable"
        );
    }
}
