//! What a punch costs and what it can reach.
//!
//! ## Why any of it is shared
//!
//! The two sides ask different questions about the same swing. The
//! client asks "is there anyone under the crosshair worth sending a
//! message about, and has enough time passed since the last one" -- it
//! has to, or every frame with the button held would be a packet. The
//! server asks "was that swing possible", and refuses it otherwise.
//!
//! Both questions are answered from the numbers below, so a swing the
//! client thought was fair is one the server accepts. When they were
//! allowed to drift the failure was silent and horrible: a player at the
//! edge of their own reach swings, sees nothing happen, and has no way
//! to find out that the server measured the distance differently.
//!
//! ## Why a hit is worth so much less than a fall
//!
//! Falling is the thing in this world that kills you. A drop of eighteen
//! blocks is fatal from full health; a punch is a thirteenth of it, so
//! a fight is a long series of decisions and a cliff is one. Making the
//! two comparable would turn every mountain into scenery and every
//! meeting into a duel, and neither is the game this is.
//!
//! ## What the server still decides
//!
//! Everything that matters. The client sends "I swung at that player",
//! carrying no damage figure and no position: the server checks that
//! both are alive, that they are actually within reach *by its own copy
//! of their positions*, and that the attacker is not swinging faster
//! than a person can. A client that lies about any of it is asking for
//! something the server already knows the answer to.

use crate::types::BlockId;

/// How far a swing carries, in blocks, measured between the two players'
/// feet.
///
/// A little longer than the server's block-editing reach, because a
/// player is nearly two blocks tall and their *feet* are what the server
/// tracks: a swing at someone's head from four blocks away is a
/// perfectly ordinary punch, and measuring it foot to foot makes it a
/// longer number than it feels.
pub const MELEE_REACH: f32 = 4.5;

/// The margin the server allows on top of that.
///
/// Two players moving toward each other are each a fraction of a second
/// stale in the other's view, and the whole of that error lands on this
/// measurement. Without a margin the honest swings that get refused are
/// exactly the ones thrown in a chase.
pub const REACH_TOLERANCE: f32 = 1.5;

/// How long fly agaric smeared on a point keeps working, in seconds.
///
/// Five, which is what the player asked for and is also the right
/// number for what it is: long enough that the animal is still losing
/// health while it decides whether to charge again, short enough that
/// nobody stands back and waits for a poisoned boar to fall over. What
/// it takes in that time is `logic::animals::POISON_PER_SECOND` -- about
/// a third of the thrust that delivered it.
///
/// Shared rather than kept on the server because the client has to be
/// able to say what the paste is worth before a player spends two
/// toadstools on it.
pub const POISON_SECONDS: f32 = 5.0;

/// Damage from one bare-handed hit.
///
/// Fourteen swings to kill someone at full health, which is a fight
/// rather than an ambush: long enough that the player being hit has time
/// to run, hit back, or get somewhere the attacker cannot follow.
pub const MELEE_DAMAGE: f32 = 1.5;

/// Seconds between swings.
///
/// The client waits this long before sending another and the server
/// refuses anything faster (less a little slack for jitter), so holding
/// the button down is a steady rhythm rather than as many hits per
/// second as the frame rate allows.
pub const MELEE_COOLDOWN_SECS: f32 = 0.6;

/// How much early the server will still accept a swing.
///
/// The client's clock and the server's do not agree, and the disagreement
/// is one-sided in a way that matters: a swing sent exactly on the
/// cooldown arrives having spent the network latency in flight, so it is
/// *late* by the server's reckoning, never early. This exists for the
/// jitter around that, and is deliberately small.
pub const COOLDOWN_SLACK_SECS: f32 = 0.08;

/// How long a swing with this in hand takes, in seconds.
///
/// **One number for every weapon was one decision the player never
/// made.** A spear and a knife cost the same to swing, so the spear --
/// which hits harder and reaches further -- was simply better, and
/// picking a weapon was picking the newest one. What a knife is *for*
/// is being quick, and that could not be said until this existed.
///
/// The three shapes, and the argument for each:
///
/// * **A spear is slow.** It is a thrust with two metres of haft: the
///   weight that makes it hurt is the weight that makes it late. It is
///   the one weapon where a miss is expensive.
/// * **A knife is fast**, and that is the whole of what it offers in a
///   fight: half again the strokes for well under half the damage. It
///   is what you fight with when the thing is already on top of you.
/// * **An axe is slower than a fist** and hits like the tool it is. It
///   is not a weapon and this is where that is written down.
///
/// Everything else -- a pick, a torch, a handful of berries -- swings at
/// the bare-handed rate, because it is a bare-handed swing with
/// something in the way.
pub fn swing_seconds(held: Option<BlockId>) -> f32 {
    use crate::types::{block_kind, is_weapon};
    let Some(held) = held else {
        return MELEE_COOLDOWN_SECS;
    };
    if is_weapon(held) {
        return SPEAR_COOLDOWN_SECS;
    }
    match block_kind(held) {
        crate::types::BLOCK_FLINT_KNIFE
        | crate::types::BLOCK_COPPER_KNIFE
        | crate::types::BLOCK_BRONZE_KNIFE
        | crate::types::BLOCK_IRON_KNIFE => KNIFE_COOLDOWN_SECS,
        crate::types::BLOCK_STONE_AXE
        | crate::types::BLOCK_COPPER_AXE
        | crate::types::BLOCK_BRONZE_AXE
        | crate::types::BLOCK_IRON_AXE => AXE_COOLDOWN_SECS,
        _ => MELEE_COOLDOWN_SECS,
    }
}

/// How far a swing with this in hand carries, in blocks.
///
/// **The haft is the reach, and the material is not.** All four spears
/// reach the same distance because a spear is a stick with a point on
/// it: a better point goes in deeper (see `hunting_damage`), it does not
/// grow the shaft. A knife is shorter than a fist's swing is generous,
/// and that is the price of its speed.
///
/// This is the number a hunter feels before any other: it is the
/// difference between killing a boar and being reached by one.
pub fn reach_with(held: Option<BlockId>) -> f32 {
    use crate::types::{block_kind, is_weapon};
    let Some(held) = held else {
        return MELEE_REACH;
    };
    if is_weapon(held) {
        return SPEAR_REACH;
    }
    match block_kind(held) {
        crate::types::BLOCK_FLINT_KNIFE
        | crate::types::BLOCK_COPPER_KNIFE
        | crate::types::BLOCK_BRONZE_KNIFE
        | crate::types::BLOCK_IRON_KNIFE => KNIFE_REACH,
        _ => MELEE_REACH,
    }
}

/// A thrust: one whole second, draw, drive and recovery.
///
/// **A second because the player asked for one, and because it is the
/// first number at which a thrust is a commitment.** At 0.9 it was half
/// again a fist, which on screen was a jab: the client's arm used the
/// same third-of-a-second blow for a spear as for a pick, so the point
/// was home and resting for two thirds of the wait and nothing said the
/// spear was slow except that clicking did nothing. Now the hand's
/// thrust is exactly this long (`hand::blow_seconds`), and the wait *is*
/// the animation.
///
/// What it does to the balance, measured against the numbers it meets:
///
/// * **Against a knife** it is two and a half strokes to one, for twice
///   the damage on an animal (`hunting_damage`) and two thirds again the
///   reach. The knife wins a brawl with something already on top of you,
///   which is what it is for.
/// * **Against a boar** (`Species::run_speed` 6.1, four a gore) a boar
///   entering the spear's six blocks is on the hunter in about a second,
///   and the blow lands at the point's full extension, 0.42 s after the
///   click, rather than on it -- see the client's `hand::impact_seconds`.
///   So one thrust lands before it arrives and the second is started
///   with it already goring. The first one has to be aimed, and that is
///   the decision a spear is.
/// * **Against a lion** (three thrusts through its hide, six a bite) it
///   is three seconds of standing your ground, which is what the lion's
///   own comment asks of "a spear and a steady hand". A bear, at under
///   eight thrusts and faster than a sprint, stays the fight you do not
///   pick.
///
/// Fly agaric (`POISON_SECONDS`) is untouched: five seconds of poison is
/// now five thrusts' worth of waiting, which only makes the paste worth
/// more.
pub const SPEAR_COOLDOWN_SECS: f32 = 1.0;
/// A knife: two strokes in the time a fist takes one and a half.
pub const KNIFE_COOLDOWN_SECS: f32 = 0.4;
/// An axe: heavier than a fist and not built for this.
pub const AXE_COOLDOWN_SECS: f32 = 0.85;
/// Two metres of haft, and the reason to carry one.
pub const SPEAR_REACH: f32 = 6.0;
/// An arm and a short blade.
pub const KNIFE_REACH: f32 = 3.6;

/// Whether one player is close enough to another to have hit them.
///
/// Takes both positions in the same frame of reference -- feet, in world
/// coordinates -- and is what the server actually runs. The tolerance is
/// included here rather than at the call site so there is exactly one
/// definition of "in reach".
pub fn within_reach(
    attacker: (f64, f64, f64),
    target: (f64, f64, f64),
    held: Option<BlockId>,
) -> bool {
    // Apart in `f64`, then narrowed: two feet a million blocks out are each
    // a sixteenth off in `f32`, and a reach is a few blocks.
    let (dx, dy, dz) = (
        (target.0 - attacker.0) as f32,
        (target.1 - attacker.1) as f32,
        (target.2 - attacker.2) as f32,
    );
    let distance_squared = dx * dx + dy * dy + dz * dz;
    if !distance_squared.is_finite() {
        return false;
    }
    let limit = reach_with(held) + REACH_TOLERANCE;
    distance_squared <= limit * limit
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_punch_is_worth_far_less_than_a_cliff() {
        // The whole balance of the thing: a fall is what kills you, and
        // a fight is a series of decisions. Twenty is full health --
        // `survival::MAX_HEALTH`, which lives on the server, because
        // health is the server's business and reach is both sides'.
        let ten_punches = MELEE_DAMAGE * 10.0;
        assert!(ten_punches > 0.0, "a punch is worth nothing at all");
        assert!(ten_punches < 20.0, "ten punches should not be lethal");
    }

    #[test]
    fn reach_is_measured_generously_but_not_infinitely() {
        let here = (0.0, 40.0, 0.0);
        assert!(within_reach(here, (2.0, 40.0, 0.0), None), "a swing at arm's length");
        assert!(
            within_reach(here, (0.0, 42.0, 0.0), None),
            "someone standing on your head is in reach"
        );
        assert!(
            !within_reach(here, (12.0, 40.0, 0.0), None),
            "twelve blocks is not a punch"
        );
        // The margin exists, and it is a margin rather than a licence.
        assert!(within_reach(here, (f64::from(MELEE_REACH + 1.0), 40.0, 0.0), None));
        assert!(!within_reach(here, (f64::from(MELEE_REACH + REACH_TOLERANCE + 0.5), 40.0, 0.0), None));
    }

    #[test]
    fn nonsense_positions_are_out_of_reach_rather_than_a_panic() {
        // These come off the wire on the server's side: a client's own
        // position is whatever it last claimed.
        let here = (0.0, 40.0, 0.0);
        assert!(!within_reach(here, (f64::from(f32::NAN), 40.0, 0.0), None));
        assert!(!within_reach(here, (f64::from(f32::INFINITY), 40.0, 0.0), None));
        assert!(!within_reach((f64::from(f32::NAN), 0.0, 0.0), here, None));
    }

    #[test]
    fn the_slack_on_the_cooldown_cannot_swallow_it() {
        // Slack is for clock jitter. If it ever approached the cooldown
        // itself, the rate limit would stop existing.
        let generous = COOLDOWN_SLACK_SECS * 4.0;
        assert!(
            generous < MELEE_COOLDOWN_SECS,
            "{COOLDOWN_SLACK_SECS}s of slack against a {MELEE_COOLDOWN_SECS}s cooldown"
        );
    }

    #[test]
    fn every_spear_thrust_takes_one_whole_second_poisoned_or_not() {
        // The player's number, and the one the client's arm is timed
        // by. A spear that came out a hair quicker for one material, or
        // for the paste, would be a thrust whose animation ends before
        // the server will take the next one.
        use crate::types::{
            poisoned, BLOCK_BONE_SPEAR, BLOCK_BRONZE_SPEAR, BLOCK_COPPER_SPEAR,
            BLOCK_FLINT_SPEAR, BLOCK_IRON_SPEAR,
        };
        for spear in [
            BLOCK_BONE_SPEAR,
            BLOCK_FLINT_SPEAR,
            BLOCK_COPPER_SPEAR,
            BLOCK_BRONZE_SPEAR,
            BLOCK_IRON_SPEAR,
        ] {
            for held in [spear, poisoned(spear)] {
                assert_eq!(swing_seconds(Some(held)), 1.0, "{held} thrusts in its own time");
            }
        }
        // ...and it is still the slowest thing a hand does, so the
        // spear is not simply better.
        const _: () = assert!(SPEAR_COOLDOWN_SECS > AXE_COOLDOWN_SECS);
        const _: () = assert!(SPEAR_COOLDOWN_SECS > MELEE_COOLDOWN_SECS);
    }
}
