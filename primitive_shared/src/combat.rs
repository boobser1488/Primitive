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

/// Whether one player is close enough to another to have hit them.
///
/// Takes both positions in the same frame of reference -- feet, in world
/// coordinates -- and is what the server actually runs. The tolerance is
/// included here rather than at the call site so there is exactly one
/// definition of "in reach".
pub fn within_reach(attacker: (f32, f32, f32), target: (f32, f32, f32)) -> bool {
    let (dx, dy, dz) = (
        target.0 - attacker.0,
        target.1 - attacker.1,
        target.2 - attacker.2,
    );
    let distance_squared = dx * dx + dy * dy + dz * dz;
    if !distance_squared.is_finite() {
        return false;
    }
    let limit = MELEE_REACH + REACH_TOLERANCE;
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
        assert!(within_reach(here, (2.0, 40.0, 0.0)), "a swing at arm's length");
        assert!(
            within_reach(here, (0.0, 42.0, 0.0)),
            "someone standing on your head is in reach"
        );
        assert!(
            !within_reach(here, (12.0, 40.0, 0.0)),
            "twelve blocks is not a punch"
        );
        // The margin exists, and it is a margin rather than a licence.
        assert!(within_reach(here, (MELEE_REACH + 1.0, 40.0, 0.0)));
        assert!(!within_reach(here, (MELEE_REACH + REACH_TOLERANCE + 0.5, 40.0, 0.0)));
    }

    #[test]
    fn nonsense_positions_are_out_of_reach_rather_than_a_panic() {
        // These come off the wire on the server's side: a client's own
        // position is whatever it last claimed.
        let here = (0.0, 40.0, 0.0);
        assert!(!within_reach(here, (f32::NAN, 40.0, 0.0)));
        assert!(!within_reach(here, (f32::INFINITY, 40.0, 0.0)));
        assert!(!within_reach((f32::NAN, 0.0, 0.0), here));
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
}
