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
/// Sized against the stack limit rather than against realism: a full
/// stack of stone is a little over three hundred kilograms, so this is
/// roughly two of them. That makes a stone-hauling trip a decision about
/// how many loads to make, which is the point of having weight at all,
/// while leaving room to carry a sensible spread of lighter blocks
/// without noticing.
pub const CARRY_CAPACITY_KG: f32 = 600.0;

/// Speed at a full load, as a fraction of unencumbered speed.
///
/// Not lower, and never zero. A player who cannot move cannot get
/// anywhere to put anything down, and "I am stuck and do not know why"
/// is a worse outcome than any amount of realism buys.
const MIN_SPEED_SCALE: f32 = 0.45;
/// The floor once past capacity entirely.
const OVERLOADED_SPEED_SCALE: f32 = 0.25;

/// How much worse a landing is at a full load.
const MAX_FALL_MULTIPLIER: f32 = 2.0;

/// How loaded a player is, 0 (empty) to 1 (at capacity).
///
/// Saturates: carrying more than capacity is still 1 here, and the extra
/// punishment lives in `speed_scale` alone.
pub fn load_fraction(kilograms: f32) -> f32 {
    if !kilograms.is_finite() || kilograms <= 0.0 {
        return 0.0;
    }
    (kilograms / CARRY_CAPACITY_KG).clamp(0.0, 1.0)
}

/// Movement speed multiplier for a given weight.
pub fn speed_scale(kilograms: f32) -> f32 {
    let load = load_fraction(kilograms);
    let scale = 1.0 - (1.0 - MIN_SPEED_SCALE) * load;
    if kilograms > CARRY_CAPACITY_KG {
        // Past capacity the curve has already bottomed out, so overload
        // gets its own step down rather than nothing at all -- otherwise
        // there is no feedback for being far too heavy.
        return OVERLOADED_SPEED_SCALE;
    }
    scale
}

/// Fall damage multiplier for a given weight.
///
/// Mass makes a landing worse, which is the other half of why weight is
/// worth tracking: it turns a heavy haul out of a deep mine into a route
/// choice rather than a straight drop down the shaft.
pub fn fall_multiplier(kilograms: f32) -> f32 {
    1.0 + (MAX_FALL_MULTIPLIER - 1.0) * load_fraction(kilograms)
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
        for kg in (0..=600).step_by(50) {
            let speed = speed_scale(kg as f32);
            let fall = fall_multiplier(kg as f32);
            assert!(speed <= last_speed, "speed rose at {kg} kg");
            assert!(fall >= last_fall, "fall damage fell at {kg} kg");
            last_speed = speed;
            last_fall = fall;
        }
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
    fn a_stack_of_stone_is_a_real_fraction_of_what_you_can_carry() {
        // The tuning claim in the docs, kept honest.
        let stack = crate::types::block_weight(crate::types::BLOCK_STONE) * 128.0;
        let fraction = load_fraction(stack);
        assert!(
            (0.35..0.75).contains(&fraction),
            "a stack of stone is {:.0}% of capacity, which makes weight either \
             pointless or unplayable",
            fraction * 100.0
        );
    }
}
