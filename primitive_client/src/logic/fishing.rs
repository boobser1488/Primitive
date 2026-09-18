//! The client's half of fishing: a float to draw, and a sentence to say
//! instead of a cast that would come to nothing.
//!
//! **Nothing here decides a catch.** The server rolls every bite and every
//! trap (`primitive_server::logic::fishing`); a fish arrives in the pack in
//! an ordinary inventory message, and the float is how the player sees that
//! they are fishing until it does. What the client does decide is two things
//! the server cannot do well from where it is:
//!
//! * **Speak the player's language.** A cast at a puddle is refused here, in
//!   words from `ui::lang`, from the same `fishing::survey` the server asks,
//!   and never sent. The server's English refusal is left for the cast this
//!   side could not judge.
//! * **Draw the float, and stop drawing it** on exactly the rules the server
//!   ends a line on -- the slot, the reach, the water -- so a player who
//!   walks away sees the float go when the line goes, and not a float left
//!   bobbing over a line the server has already reeled in.
//!
//! **Why the float is told now, where it used to be guessed.** This note
//! used to argue that a `ServerMessage` for the float was two messages and a
//! protocol version for a picture the client could work out. That was true
//! while the only thing a client could not know was *when* a fish takes --
//! and it stopped being true the moment the bite became something to strike
//! at. A dip the player has nine tenths of a second to answer has to be drawn
//! the instant the server decides it, so it is one message
//! (`ServerMessage::Line`), to the one hand holding the rod, carrying the
//! float, the phase, the strain and how lively the water is.
//!
//! **What the client still decides for itself** is everything about the
//! *drawing*: how the float bobs, how often it twitches between bites (off
//! `liveliness`, which is how a player reads a spot without a number on the
//! screen), and when to stop drawing it because the player has walked away --
//! the last from the same `fishing::cast_holds` the server ends the line on,
//! so the float goes when the line goes.

use primitive_shared::body::Water;
use primitive_shared::fishing;
use primitive_shared::types::{block_kind, is_liquid, trap_catch, BlockId, BLOCK_FISHING_ROD, BLOCK_FISH_TRAP};

/// What a line is doing, as the server last said (`ServerMessage::Line`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Phase {
    /// Still landing.
    Settling,
    /// On the water.
    Waiting,
    /// **Under**: strike now.
    Dipping,
    /// A fish is on.
    Fighting,
}

impl Phase {
    /// The byte off the wire. Anything this build does not know is taken as
    /// waiting: a float that sits still is a wrong picture, and a float that
    /// dips at nothing would have the player striking at a newer server's
    /// idea of something.
    pub fn of_code(code: u8) -> Phase {
        match code {
            0 => Phase::Settling,
            2 => Phase::Dipping,
            3 => Phase::Fighting,
            _ => Phase::Waiting,
        }
    }
}

/// A line this client has in the water.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Float {
    /// The cell of water the cast went to.
    pub at: (i32, i32, i32),
    /// The hotbar slot the rod was in.
    pub slot: usize,
    /// How many raw fish the pack held when the line went in: more than this
    /// is the fish coming up, which is what the splash is drawn off.
    pub fish_before: u32,
    /// Seconds since the cast, for the float's bob.
    pub age: f32,
    pub phase: Phase,
    /// How near the line is to parting, 0 to 1. Only meant during
    /// [`Phase::Fighting`].
    pub strain: f32,
    /// How well this water fishes, 0 to 1: what the twitches are counted
    /// off.
    pub liveliness: f32,
    /// Seconds since the phase last changed, for the dip.
    pub since: f32,
}

/// How deep the float goes when a fish has the bait, in blocks.
///
/// A quarter: under, and plainly under, at forty paces and at dusk. It also
/// has to be visible from *above* -- a player standing over a bank sees the
/// top of the float and nothing else -- which is why it is a movement down
/// and not a tilt.
pub const DIP_DEPTH: f32 = 0.25;

/// How much of a block a twitch moves the float.
///
/// A twitch is not a bite and must never be mistaken for one, so it is a
/// fifth of the dip: at a glance it is the float being nudged, and a player
/// who strikes at one loses the cast and learns the difference in one
/// evening.
pub const TWITCH_DEPTH: f32 = DIP_DEPTH / 5.0;

/// The most twitches a lively spot gives, a second.
///
/// **This is the whole of how a spot is read.** Nothing on the screen says
/// "this water is good"; the float in water where a bite is half a minute
/// away is never still, and the float on a fished-out pond at noon sits like
/// a cork on a table. A player learns the difference by watching, which is
/// the sentence the fishing was written for.
pub const TWITCHES_A_SECOND: f32 = 1.6;

/// What to say instead of casting, or instead of reaching into a trap.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Notice {
    /// Water too small for any fish.
    TooSmall,
    /// Water with fish in it, too shallow for a float.
    TooShallow,
    /// A trap with nothing in it yet, set in the water.
    TrapEmpty,
    /// A trap with nothing in it that is not in the water at all.
    TrapDry,
    /// The throw came down on land, or against something.
    NoWater,
}

impl Notice {
    /// The line of the interface that says it.
    pub fn msg(self) -> crate::ui::lang::Msg {
        use crate::ui::lang::Msg;
        match self {
            Notice::TooSmall => Msg::FishingTooSmall,
            Notice::TooShallow => Msg::FishingTooShallow,
            Notice::TrapEmpty => Msg::FishTrapEmpty,
            Notice::TrapDry => Msg::FishTrapDry,
            Notice::NoWater => Msg::FishingNoWater,
        }
    }
}

/// Why a cast at `at` would take nothing, or `None` if it would be a cast
/// (or is not water at all, which is not a cast and says nothing).
///
/// Asked as fresh water, whatever the water is: the kind decides how long a
/// bite takes, never whether one can come, and the kind is the biome and the
/// flow the server reads (`water_kind`), which this side would only be
/// guessing at.
pub fn cast_refusal(block_at: impl Fn(i32, i32, i32) -> Option<BlockId>, at: (i32, i32, i32)) -> Option<Notice> {
    let spot = fishing::survey(block_at, at)?;
    if !spot.holds_fish() {
        return Some(Notice::TooSmall);
    }
    if fishing::rod_place_factor(spot, Water::Fresh) <= 0.0 {
        return Some(Notice::TooShallow);
    }
    None
}

/// **Where this throw would land, or why it would be wasted.**
///
/// The client walks the same arc the server will (`fishing::cast_target`)
/// through its own chunks, and surveys the same water. A throw it can see
/// is hopeless is answered here, in the player's language, and never sent;
/// a throw it judges good is sent as a power and nothing else, and the
/// server works out the landing again from the transform it already trusts.
///
/// **So the two can disagree, and only in one direction that matters**: the
/// client's chunks are the server's chunks a moment later, so a float that
/// lands somewhere this side did not expect is a cast made in the same
/// instant a block changed. The float the player sees is the one the server
/// sends back (`ServerMessage::Line`), never this guess.
pub fn cast_along(
    eye: (f32, f32, f32),
    look: (f32, f32, f32),
    power: f32,
    block_at: impl Fn(i32, i32, i32) -> Option<BlockId>,
) -> Result<(i32, i32, i32), Notice> {
    let Some(at) = fishing::cast_target(eye, look, power, &block_at) else {
        return Err(Notice::NoWater);
    };
    match cast_refusal(&block_at, at) {
        Some(notice) => Err(notice),
        None => Ok(at),
    }
}

/// What to tell a player reaching into `trap` with nothing to take, or
/// `None` if there are fish in it (and the server empties it).
pub fn trap_notice(
    block_at: impl Fn(i32, i32, i32) -> Option<BlockId>,
    trap: (i32, i32, i32),
    block: BlockId,
) -> Option<Notice> {
    if block_kind(block) != BLOCK_FISH_TRAP || trap_catch(block) > 0 {
        return None;
    }
    match fishing::trap_water(block_at, trap) {
        Some((spot, _)) if spot.holds_fish() => Some(Notice::TrapEmpty),
        _ => Some(Notice::TrapDry),
    }
}

impl Float {
    /// Is this line still in the water? The server's rules for ending one
    /// (`step_fishing`), asked of this side's copy of the same facts.
    pub fn holds(
        &self,
        eye: (f32, f32, f32),
        selected: usize,
        held: Option<BlockId>,
        block_at: impl Fn(i32, i32, i32) -> Option<BlockId>,
    ) -> bool {
        selected == self.slot
            && held.map(block_kind) == Some(BLOCK_FISHING_ROD)
            && fishing::cast_holds(eye, self.at)
            && block_at(self.at.0, self.at.1, self.at.2).is_some_and(is_liquid)
    }

    /// Where the float sits: on the surface of the float's column, bobbing a
    /// finger's width, twitching as often as the water is lively, and under
    /// when a fish has the bait. The top of the water rather than the cell
    /// aimed at, because a cast made from under the surface still floats.
    pub fn position(&self, block_at: impl Fn(i32, i32, i32) -> Option<BlockId>) -> (f32, f32, f32) {
        let mut top = self.at.1;
        while top - self.at.1 < 32 && block_at(self.at.0, top + 1, self.at.2).is_some_and(is_liquid) {
            top += 1;
        }
        let surface = block_at(self.at.0, top, self.at.2).map_or(1.0, primitive_shared::fluid::surface_height);
        let bob = 0.02 * (self.age * 2.6).sin();
        (
            self.at.0 as f32 + 0.5,
            top as f32 + surface + bob - self.sunk(),
            self.at.2 as f32 + 0.5,
        )
    }

    /// How far under the surface the float is, in blocks.
    ///
    /// **Four different movements, and they are told apart on purpose**: a
    /// float that is settling is riding high, a float on a good spot is
    /// twitching, a float with a fish on it is *under*, and a float with a
    /// fish fighting is being pulled about. A player who cannot tell the
    /// third from the second at a glance has no mechanic here, which is why
    /// the dip is five times the twitch.
    pub fn sunk(&self) -> f32 {
        match self.phase {
            // Still landing: riding high on its own splash.
            Phase::Settling => -0.05 * (1.0 - self.since.min(1.0)),
            Phase::Waiting => self.twitch(),
            // Under, and further under the longer it stays: the fish is
            // taking it down.
            Phase::Dipping => DIP_DEPTH * (0.55 + 0.45 * (self.since * 6.0).min(1.0)),
            // Being fought: under, and jerking with the strain.
            Phase::Fighting => DIP_DEPTH * (0.8 + 0.5 * self.strain * (self.age * 11.0).sin().abs()),
        }
    }

    /// The nudge of a fish passing: a short dip, as often as the water is
    /// lively, and never in dead water.
    ///
    /// **Off the float's own age rather than a random number**, so the same
    /// float twitches the same way every frame it is asked -- a rate drawn
    /// per frame would make it shiver constantly instead of twitching.
    fn twitch(&self) -> f32 {
        if self.liveliness <= 0.0 {
            return 0.0;
        }
        let every = 1.0 / (TWITCHES_A_SECOND * self.liveliness).max(0.05);
        let into = self.age.rem_euclid(every);
        // A twitch is a fifth of a second of it, and flat water the rest.
        let shape = (1.0 - (into / 0.2).min(1.0)) * (into * 32.0).sin().abs();
        TWITCH_DEPTH * shape
    }
}

/// What the hand just told the rod to do.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Order {
    /// Throw, with this much power (`fishing::cast_power`).
    Cast(f32),
    /// The float went under and the hand came up.
    Strike,
    /// Pulling, or giving line.
    Reel(bool),
    /// Take the line out of the water, with nothing on it.
    In,
}

/// **The rod, worked with one button.**
///
/// Hold it to wind up and let go to throw; tap it while the line is out to
/// strike; hold it while a fish is on to reel, and let go to give line;
/// hold the sprint key and tap to bring the line in with no strike at all.
///
/// **One button rather than three**, and that is the touch decision made
/// early rather than bolted on: a phone has the use button and the look
/// area, and a mechanic that needed a second and third control would be a
/// mechanic that worked on a desktop and was re-invented for a phone. The
/// cost is that a tap at the wrong moment is a strike at nothing -- which is
/// the rule the fishing wanted anyway.
///
/// **Driven off the button's state once a frame, not off press events.** A
/// wind-up is a duration; and on a phone a tap in the look area arrives as a
/// press with no release at all (`touch::Touch::take_place`), so a throw
/// that waited for the release would never be thrown. What happens instead
/// is that a wind-up that reaches full strength lets go by itself, which is
/// the strongest throw there is: a phone player who taps gets a full cast,
/// and one who holds the on-screen use button gets the whole range.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct Hold {
    /// How long the rod has been wound back, if it is.
    winding: Option<f32>,
    /// The button last frame, for the tap.
    was_down: bool,
    /// The last thing the reel was told, so a held button is one message and
    /// not sixty a second.
    reeling: bool,
}

impl Hold {
    /// How far the wind-up has gone, 0 to 1: what the gauge draws.
    pub fn charge(self) -> Option<f32> {
        self.winding.map(primitive_shared::fishing::cast_power)
    }

    /// **The rod put down without throwing.** A screen opened in the middle
    /// of a wind-up is not a cast: the button state stops arriving while the
    /// pointer belongs to the screen, and a release that is really "the
    /// player pressed Escape" would otherwise throw the line.
    pub fn cancel(&mut self) {
        self.winding = None;
        self.was_down = false;
    }

    /// One frame of it. `down` is the use button, `rod` is a rod in hand,
    /// `line_out` is a cast of this client's in the water, `fighting` is a
    /// fish on it, `giving_way` is the sprint key.
    pub fn step(
        &mut self,
        dt: f32,
        down: bool,
        rod: bool,
        line_out: bool,
        fighting: bool,
        giving_way: bool,
    ) -> Vec<Order> {
        let tapped = down && !self.was_down;
        self.was_down = down;
        let mut orders = Vec::new();
        if !rod {
            self.winding = None;
            return orders;
        }
        if line_out {
            // A line in the water: the button is the strike, and then the
            // reel.
            self.winding = None;
            if fighting {
                if down != self.reeling {
                    self.reeling = down;
                    orders.push(Order::Reel(down));
                }
            } else if tapped {
                self.reeling = false;
                orders.push(if giving_way { Order::In } else { Order::Strike });
            }
            return orders;
        }
        self.reeling = false;
        match (down, self.winding) {
            // Winding up. At full strength it lets go by itself -- see the
            // note on this type.
            (true, Some(held)) => {
                let held = held + dt;
                if held >= primitive_shared::fishing::CAST_CHARGE_SECONDS {
                    self.winding = None;
                    orders.push(Order::Cast(1.0));
                } else {
                    self.winding = Some(held);
                }
            }
            (true, None) if tapped => self.winding = Some(0.0),
            // Held down since before the rod came out, or since the last
            // throw: not a wind-up until it has been let go of.
            (true, None) => {}
            (false, Some(held)) => {
                self.winding = None;
                orders.push(Order::Cast(primitive_shared::fishing::cast_power(held)));
            }
            (false, None) => {}
        }
        orders
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use primitive_shared::types::{trap_holding, BLOCK_AIR, BLOCK_STONE, BLOCK_WATER};

    /// Stone, with a pool `w` across and `depth` deep whose surface is y = 20.
    fn pool(w: i32, depth: i32) -> impl Fn(i32, i32, i32) -> Option<BlockId> {
        move |x, y, z| {
            Some(if (0..w).contains(&x) && (0..w).contains(&z) && y <= 20 && y > 20 - depth {
                BLOCK_WATER
            } else if y > 20 {
                BLOCK_AIR
            } else {
                BLOCK_STONE
            })
        }
    }

    /// A float on `at`, waiting, in water nothing is biting in.
    fn a_float(at: (i32, i32, i32), slot: usize) -> Float {
        Float {
            at,
            slot,
            fish_before: 0,
            age: 0.0,
            phase: Phase::Waiting,
            strain: 0.0,
            liveliness: 0.0,
            since: 0.0,
        }
    }

    #[test]
    fn a_cast_at_a_puddle_is_refused_here_in_words_and_a_cast_at_a_pond_is_not() {
        assert_eq!(cast_refusal(pool(1, 1), (0, 20, 0)), Some(Notice::TooSmall));
        assert_eq!(cast_refusal(pool(9, 1), (4, 20, 4)), Some(Notice::TooShallow));
        assert_eq!(cast_refusal(pool(5, 3), (2, 20, 2)), None);
        // Stone is not a cast at all, and says nothing.
        assert_eq!(cast_refusal(pool(5, 3), (20, 10, 20)), None);
    }

    #[test]
    fn every_fishing_notice_has_a_line_in_every_language() {
        for notice in [Notice::TooSmall, Notice::TooShallow, Notice::TrapEmpty, Notice::TrapDry, Notice::NoWater] {
            for language in crate::ui::lang::Language::ALL {
                assert!(!language.text(notice.msg()).is_empty(), "{notice:?} in {language:?}");
            }
        }
    }

    #[test]
    fn an_empty_trap_says_whether_it_is_in_the_water_and_a_full_one_says_nothing() {
        let world = pool(9, 2);
        let in_pond = |x, y, z| if (x, y, z) == (4, 20, 4) { Some(BLOCK_FISH_TRAP) } else { world(x, y, z) };
        assert_eq!(trap_notice(in_pond, (4, 20, 4), BLOCK_FISH_TRAP), Some(Notice::TrapEmpty));
        assert_eq!(trap_notice(in_pond, (4, 20, 4), trap_holding(1)), None);
        assert_eq!(trap_notice(pool(9, 2), (30, 21, 30), BLOCK_FISH_TRAP), Some(Notice::TrapDry));
    }

    #[test]
    fn the_float_goes_when_the_line_would() {
        let world = pool(9, 3);
        let float = a_float((4, 20, 4), 1);
        let eye = (4.5, 22.6, 0.5);
        assert!(float.holds(eye, 1, Some(BLOCK_FISHING_ROD), &world));
        assert!(!float.holds(eye, 2, Some(BLOCK_FISHING_ROD), &world), "another slot in hand");
        assert!(!float.holds(eye, 1, Some(BLOCK_STONE), &world), "the rod swapped for a stone");
        assert!(!float.holds((40.5, 22.6, 0.5), 1, Some(BLOCK_FISHING_ROD), &world), "walked off");
        assert!(!float.holds(eye, 1, Some(BLOCK_FISHING_ROD), pool(0, 0)), "the pond drained");
    }

    #[test]
    fn how_far_the_throw_goes_follows_how_long_it_was_held() {
        // A pond nine across, fifteen blocks out from where the player
        // stands, so a weak throw falls on the bank and a full one reaches.
        let world = |x: i32, y: i32, z: i32| {
            Some(if (0..24).contains(&x) && (3..20).contains(&z) && y <= 20 && y > 16 {
                BLOCK_WATER
            } else if y > 20 {
                BLOCK_AIR
            } else {
                BLOCK_STONE
            })
        };
        let eye = (12.5, 21.6, 1.5);
        // Level, looking down the z axis at the water.
        let look = (0.0, -0.08, 1.0);
        let reach = |held: f32| {
            let power = primitive_shared::fishing::cast_power(held);
            cast_along(eye, look, power, world).map(|at| (at.2 as f32 - eye.2).abs())
        };
        let short = reach(0.15).expect("a flick reaches the near edge");
        let full = reach(primitive_shared::fishing::CAST_CHARGE_SECONDS).expect("a full throw reaches");
        assert!(full > short + 4.0, "a held throw went {full} blocks against a flick's {short}");
        // ...and the hold is monotone: every step of the wind-up is at
        // least as far as the one before, so a player learns one rule.
        let mut last = 0.0;
        let mut held = 0.05;
        while held <= primitive_shared::fishing::CAST_CHARGE_SECONDS {
            if let Ok(distance) = reach(held) {
                assert!(distance + 0.5 >= last, "holding longer threw shorter at {held}s: {distance} after {last}");
                last = distance;
            }
            held += 0.05;
        }
        // Looking up does not throw further -- the hold does that, and the
        // aim only says which way -- so a throw at the sky with the same
        // bearing lands where the level one did.
        assert_eq!(reach(0.4).ok(), reach(0.4).ok());
        let skyward = cast_along(eye, (0.0, 1.0, 1.0), 1.0, world).expect("a throw aimed high still fishes");
        assert_eq!(skyward, cast_along(eye, look, 1.0, world).expect("a level throw"));
        // Straight up has no bearing at all, and is no cast.
        assert_eq!(cast_along(eye, (0.0, 1.0, 0.0), 1.0, world), Err(Notice::NoWater));
    }

    #[test]
    fn a_float_with_a_fish_on_it_is_plainly_further_under_than_one_being_nudged() {
        let mut lively = a_float((4, 20, 4), 0);
        lively.liveliness = 1.0;
        // The deepest twitch a lively spot ever gives, over a few seconds.
        let mut deepest: f32 = 0.0;
        let mut t = 0.0;
        while t < 8.0 {
            lively.age = t;
            deepest = deepest.max(lively.sunk());
            t += 1.0 / 60.0;
        }
        let mut dipping = lively;
        dipping.phase = Phase::Dipping;
        dipping.since = 0.0;
        assert!(
            dipping.sunk() > deepest * 2.0,
            "a bite ({}) is not plainly deeper than a twitch ({deepest})",
            dipping.sunk()
        );
        // ...and dead water does not twitch at all, which is the other half
        // of reading a spot.
        let mut dead = a_float((4, 20, 4), 0);
        dead.age = 3.3;
        assert_eq!(dead.sunk(), 0.0, "a fished-out pond was still twitching the float");
    }

    #[test]
    fn one_button_throws_strikes_and_reels_and_never_two_of_them_at_once() {
        let mut hold = Hold::default();
        // Held down from a frame where there was no rod: nothing, until it
        // has been let go of. (Otherwise selecting the rod with the button
        // already down would throw it.)
        assert!(hold.step(0.1, true, false, false, false, false).is_empty());
        assert!(hold.step(0.1, true, true, false, false, false).is_empty(), "a rod selected mid-press threw itself");
        assert!(hold.step(0.1, false, true, false, false, false).is_empty());
        // Pressed, held a quarter second, let go: a short throw.
        assert!(hold.step(0.1, true, true, false, false, false).is_empty());
        assert!(hold.step(0.25, true, true, false, false, false).is_empty());
        let thrown = hold.step(0.0, false, true, false, false, false);
        match thrown.as_slice() {
            [Order::Cast(power)] => assert!(*power > 0.0 && *power < 0.5, "a quarter-second throw at {power}"),
            other => panic!("letting go did not throw: {other:?}"),
        }
        // Held past the wind-up: it goes by itself, at full strength, and
        // only once.
        let mut full = Hold::default();
        full.step(0.0, true, true, false, false, false);
        let mut orders = Vec::new();
        for _ in 0..60 {
            orders.extend(full.step(0.05, true, true, false, false, false));
        }
        assert_eq!(orders, vec![Order::Cast(1.0)], "a held button threw {orders:?}");
        // With a line in the water, a tap is a strike and the sprint key
        // makes it a reel-in instead.
        let mut out = Hold::default();
        assert_eq!(out.step(0.05, true, true, true, false, false), vec![Order::Strike]);
        assert!(out.step(0.05, true, true, true, false, false).is_empty(), "a held button struck twice");
        out.step(0.05, false, true, true, false, false);
        assert_eq!(out.step(0.05, true, true, true, false, true), vec![Order::In]);
        // With a fish on, the button is the reel, and it is sent on the
        // change and not every frame.
        let mut fight = Hold::default();
        assert_eq!(fight.step(0.05, true, true, true, true, false), vec![Order::Reel(true)]);
        assert!(fight.step(0.05, true, true, true, true, false).is_empty());
        assert_eq!(fight.step(0.05, false, true, true, true, false), vec![Order::Reel(false)]);
        // A screen opened mid-wind-up is not a throw.
        let mut interrupted = Hold::default();
        interrupted.step(0.0, true, true, false, false, false);
        interrupted.step(0.4, true, true, false, false, false);
        interrupted.cancel();
        assert!(interrupted.step(0.05, false, true, false, false, false).is_empty(), "closing a screen threw the line");
        assert_eq!(interrupted.charge(), None);
    }

    #[test]
    fn the_float_sits_on_the_surface_even_when_cast_from_under_it() {
        let world = pool(9, 4);
        let deep = a_float((4, 18, 4), 0);
        let (_, y, _) = deep.position(&world);
        assert!((20.8..=21.05).contains(&y), "the float is at {y}, not on the water at 21");
    }
}
