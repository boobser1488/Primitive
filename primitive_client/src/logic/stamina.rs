//! Stamina: what running and carrying a load cost.
//!
//! ## What it is for
//!
//! Sprinting with no cost is not a choice, it is just a faster default
//! speed with an extra key held down. Stamina turns it into something
//! spent and recovered, and gives carried weight somewhere to bite
//! beyond a slower walk.
//!
//! ## Why it lives on the client
//!
//! For the same reason the inventory does: it only ever restrains the
//! player who owns it. A client that ignored stamina entirely would
//! sprint further, and the server's speed limit -- which is what stops
//! anyone actually moving faster than a sprint -- does not care why the
//! player stopped. Nothing here is worth a round trip.
//!
//! ## Exhaustion
//!
//! Running out does not merely stop the sprint; it locks it out until a
//! decent fraction has come back. Without that the player pumps Shift
//! against an empty bar and moves in stutters, which reads as the
//! controls being broken rather than as being out of breath.
//!
//! ## Jumping
//!
//! A jump is charged as a lump rather than as a rate, because that is
//! what it is: one hard push, and the cost does not depend on how long
//! the key was held. It is what stops bunny-hopping being a free second
//! way to travel -- with only the sprint billed, the fastest way across
//! a world was to hop across it with Shift up, which made the whole
//! system a tax on the one movement that was paying it.
//!
//! The tank is sized so that neither cost feels like a leash: it holds
//! twenty-two seconds of flat-out running, or about a dozen jumps, and
//! spending it entirely on either takes a deliberate effort.

/// A full tank, in seconds of flat-out sprinting while empty-handed.
///
/// Nearly doubled from the twelve it used to be, and the reason is that
/// jumps now draw on the same tank: twelve seconds of sprint was already
/// short enough to be felt, and taking a dozen jumps out of it as well
/// would have left a player walking most of the time. A bigger tank with
/// more drawing on it is a budget; a small one with more drawing on it
/// is a leash.
pub const MAX_STAMINA: f32 = 22.0;

/// What one jump costs, in the same seconds-of-sprint units.
///
/// A twelfth of a full tank. Enough that a run of hops across broken
/// ground is a decision, cheap enough that jumping a fence never is.
pub const JUMP_COST: f32 = 1.8;

/// A pull up a tree, in jumps. See `Stamina::climb_cost`.
pub const CLIMB_COST_JUMPS: f32 = 3.0;

/// Seconds between one pull up a tree and the next, unladen; twice that at
/// capacity.
///
/// **This is what makes a climb slow**, and the breath is what makes it
/// short. A pull is still a jump to the physics -- the same push onto the
/// next bough -- so without a rest a climber with a full tank went up at
/// the pace of their key presses. A second and a half a bough puts the top
/// of a tall oak twenty seconds away, which is a climb.
pub const CLIMB_REST: f32 = 1.5;

/// What a second of digging costs, in the same units.
///
/// Digging is the other thing in this game that is *work*, and it was
/// the one thing that cost nothing: a player with an empty bar could
/// not run and could not jump, and could still take a hillside apart at
/// full speed. Billed by the second of work rather than by the block,
/// so a metre of packed earth costs what it takes and brushing a layer
/// of snow off a path costs almost nothing -- the same figure the
/// cracks and the swing are already paced by.
///
/// A shade under what sprinting costs. Digging is heavier work than
/// running, but it is also the thing a player does for minutes at a
/// time, and a rate that empties the bar during one hole would make the
/// early game a series of rests.
pub const DIG_COST_PER_SECOND: f32 = 0.8;

/// How fast an exhausted player digs.
///
/// Not "cannot dig". Being unable to break the block in front of you is
/// how a player ends up stuck in a hole they dug, and there is no way
/// out of that but waiting. Slower is the honest version of tired, and
/// it is self-correcting: digging at half speed also bills at half the
/// rate, so the bar recovers while the work carries on.
pub const EXHAUSTED_DIG_RATE: f32 = 0.5;

/// Drain per second while sprinting with nothing in your pack.
const SPRINT_DRAIN: f32 = 1.0;
/// Extra drain per second at a full load. Carrying everything you can
/// roughly triples what a sprint costs.
const SPRINT_LOAD_DRAIN: f32 = 2.0;
/// Drain per second from the load alone, standing or walking. Small: a
/// heavy pack is tiring, but standing still has to recover.
const CARRY_DRAIN: f32 = 0.25;

/// Recovery per second when not sprinting and lightly loaded.
///
/// Still faster than a sprint costs -- it has to be, or a chase would be
/// a one-way trip -- but no longer fast enough to refill the bar between
/// two of them. At the 2.2 this replaces, four seconds of standing still
/// put back a third of a tank, so the answer to running out of breath
/// was to pause for a moment and carry on exactly as before, and neither
/// the sprint nor the jump was a cost anyone had to plan around. At 1.3
/// a full tank is the better part of twenty seconds, which is long
/// enough to be a reason to walk.
const RECOVERY: f32 = 1.3;
/// How much of the recovery a full load takes away.
///
/// Tuned against `CARRY_DRAIN` rather than picked: what matters is that
/// `RECOVERY * (1 - this) - CARRY_DRAIN` stays comfortably positive at a
/// full load. At 0.8 it was 0.09 a second, which is technically
/// recovering and practically a lockout -- half a minute of standing
/// still before the sprint came back, with nothing on screen explaining
/// why. There is a test for exactly this.
///
/// Lowered along with `RECOVERY`, because it is a *fraction* of it:
/// leaving it at the 0.55 it was would have taken the laden recovery
/// down with the unladen one and put that lockout straight back.
const RECOVERY_LOAD_PENALTY: f32 = 0.40;

/// Fraction that has to come back before sprinting is allowed again.
const RECOVERED_ENOUGH: f32 = 0.25;

pub struct Stamina {
    current: f32,
    /// Set when the tank empties, cleared once `RECOVERED_ENOUGH` is
    /// back. See the note on exhaustion.
    exhausted: bool,
    /// Seconds before the next pull up a tree may start. See `CLIMB_REST`.
    climb_rest: f32,
    /// What the player's hidden comfort is worth to recovery, as the server
    /// last said (`ServerMessage::Body::recovery`). Multiplies `RECOVERY`
    /// and nothing else: comfort is how fast the breath comes *back*, not
    /// what a sprint costs, so a cold camp is a slower rest rather than a
    /// shorter run.
    recovery_scale: f32,
}

impl Default for Stamina {
    fn default() -> Self {
        Self::new()
    }
}

impl Stamina {
    pub fn new() -> Self {
        Self {
            current: MAX_STAMINA,
            exhausted: false,
            climb_rest: 0.0,
            recovery_scale: 1.0,
        }
    }

    /// Takes the server's comfort multiplier. Sanitised like everything off
    /// a socket, and held to the range the server's own rule allows, so a
    /// hostile number cannot hand anybody an endless sprint.
    pub fn set_recovery(&mut self, scale: f32) {
        use primitive_shared::comfort::{FASTEST_RECOVERY, SLOWEST_RECOVERY};
        self.recovery_scale = if scale.is_finite() { scale.clamp(SLOWEST_RECOVERY, FASTEST_RECOVERY) } else { 1.0 };
    }

    /// What one pull up a tree costs under this load.
    ///
    /// **Three jumps unladen, and up to three times that at capacity.**
    /// Getting up a tree was a run of free hops from bough to bough -- the
    /// crown of an oak in two seconds, with a pack of stone -- and the player
    /// asked for it to be slow, tiring and heavy: "пусть это тратит стамину
    /// довольно сильно и пусть это будет медленно и зависит от веса". A
    /// climb is a pull with the arms, which is why it is dearer than a jump
    /// on the ground, and the load is lifted with every pull, which is why
    /// it multiplies.
    pub fn climb_cost(kilograms: f32) -> f32 {
        JUMP_COST * CLIMB_COST_JUMPS * (1.0 + 2.0 * primitive_shared::load::load_fraction(kilograms))
    }

    /// Whether a pull up a tree may start: breath for it, and the rest after
    /// the last one taken.
    pub fn can_climb(&self, kilograms: f32) -> bool {
        !self.exhausted && self.climb_rest <= 0.0 && self.current >= Self::climb_cost(kilograms)
    }

    /// Bills one pull up a tree, and starts the rest before the next.
    pub fn spend_climb(&mut self, kilograms: f32) {
        self.current = (self.current - Self::climb_cost(kilograms)).max(0.0);
        self.climb_rest = CLIMB_REST * (1.0 + primitive_shared::load::load_fraction(kilograms));
        if self.current <= 0.0 {
            self.exhausted = true;
        }
    }

    /// Seconds of sprint left. The HUD draws `fraction`; this is the
    /// absolute figure the tests reason about.
    #[allow(dead_code)]
    pub fn current(&self) -> f32 {
        self.current
    }

    pub fn fraction(&self) -> f32 {
        (self.current / MAX_STAMINA).clamp(0.0, 1.0)
    }

    pub fn is_exhausted(&self) -> bool {
        self.exhausted
    }

    /// Whether a sprint may start or continue.
    pub fn can_sprint(&self) -> bool {
        !self.exhausted && self.current > 0.0
    }

    /// Whether there is enough left in the tank to push off with.
    ///
    /// A whole jump's worth, rather than any at all: a jump that goes
    /// half as high because the bar was nearly empty would be a way to
    /// end up stuck in a hole, and one charged for at a fraction of the
    /// height is worse. Either the push is there or it is not.
    pub fn can_jump(&self) -> bool {
        !self.exhausted && self.current >= JUMP_COST
    }

    /// Bills one jump.
    ///
    /// Called after physics, and only for a jump that actually left the
    /// ground -- see `Player::jumped`. Running the tank to nothing here
    /// exhausts the player exactly as sprinting to nothing does, so the
    /// bar behaves the same way whichever spent it.
    pub fn spend_jump(&mut self) {
        self.current = (self.current - JUMP_COST).max(0.0);
        if self.current <= 0.0 {
            self.exhausted = true;
        }
    }

    /// Bills the work of breaking one block.
    ///
    /// `seconds` is how long the swing took -- `types::break_seconds`
    /// for the block, which is the same number the progress bar and the
    /// cracks are paced by, so what the player pays matches what they
    /// watched happen.
    ///
    /// Charged on completion rather than continuously, for the reason
    /// jumps are: a swing that was interrupted did not move any earth,
    /// and being billed for one is the sort of thing a player notices
    /// and cannot explain.
    pub fn spend_dig(&mut self, seconds: f32) {
        // NaN included, which is why this is written out rather than
        // negated: a cost nobody can account for is no cost.
        if seconds.is_nan() || seconds <= 0.0 {
            return;
        }
        self.current = (self.current - seconds * DIG_COST_PER_SECOND).max(0.0);
        if self.current <= 0.0 {
            self.exhausted = true;
        }
    }

    /// How fast this player digs, as a multiplier on progress.
    pub fn dig_rate(&self) -> f32 {
        if self.exhausted {
            EXHAUSTED_DIG_RATE
        } else {
            1.0
        }
    }

    /// Back to full. Used on respawn and on joining a world.
    pub fn reset(&mut self) {
        self.current = MAX_STAMINA;
        self.exhausted = false;
    }

    /// Advances one frame.
    ///
    /// `load` is 0..1, the carried weight against what the player can
    /// carry at all. `sprinting` is whether they are actually running --
    /// not whether Shift is down, since a sprint that physics refused
    /// should not be billed for.
    pub fn update(&mut self, dt: f32, load: f32, sprinting: bool) {
        let dt = dt.clamp(0.0, 0.1);
        let load = load.clamp(0.0, 1.0);
        self.climb_rest = (self.climb_rest - dt).max(0.0);

        if sprinting {
            self.current -= (SPRINT_DRAIN + SPRINT_LOAD_DRAIN * load) * dt;
        } else {
            // A load costs something even at rest, but never more than
            // the recovery -- otherwise a heavily laden player can never
            // get their breath back and the game deadlocks.
            //
            // Comfort scales the recovery, and only the recovery: the
            // carry drain is the pack's weight, which a warm room does not
            // lighten. At the slowest (a half) a full load still nets
            // `0.5 * 1.3 * 0.6 - 0.25 = 0.14` a second -- slow, and not
            // the lockout `RECOVERY_LOAD_PENALTY` warns about.
            let recovery = RECOVERY * self.recovery_scale * (1.0 - RECOVERY_LOAD_PENALTY * load);
            self.current += (recovery - CARRY_DRAIN * load) * dt;
        }
        self.current = self.current.clamp(0.0, MAX_STAMINA);

        if self.current <= 0.0 {
            self.exhausted = true;
        } else if self.exhausted && self.fraction() >= RECOVERED_ENOUGH {
            self.exhausted = false;
        }
    }
}

#[cfg(test)]
mod tests {
    // --- climbing ---

    #[test]
    fn a_climb_is_dearer_than_a_jump_heavier_under_a_load_and_rests_between_pulls() {
        let light = Stamina::climb_cost(0.0);
        let laden = Stamina::climb_cost(primitive_shared::load::CARRY_CAPACITY_KG);
        assert!(light >= JUMP_COST * 2.0, "a pull up a tree costs no more than a hop: {light}");
        assert!(laden > light * 2.0, "a pack of stone made a climb no harder: {laden} against {light}");

        let mut stamina = Stamina::new();
        assert!(stamina.can_climb(0.0));
        stamina.spend_climb(0.0);
        assert!(!stamina.can_climb(0.0), "a second pull came straight after the first");
        stamina.update(CLIMB_REST * 0.5, 0.0, false);
        assert!(!stamina.can_climb(0.0), "the rest between pulls was cut short");
        for _ in 0..20 {
            stamina.update(0.1, 0.0, false);
        }
        assert!(stamina.can_climb(0.0), "the climber never got their breath for the next pull");

        // A full tank is a few pulls, not a tree.
        let mut stamina = Stamina::new();
        let mut pulls = 0;
        while stamina.current() >= Stamina::climb_cost(0.0) && pulls < 100 {
            stamina.spend_climb(0.0);
            pulls += 1;
        }
        assert!(pulls <= 5, "a full tank took {pulls} pulls up a tree");
    }

    // --- digging ---

    #[test]
    fn digging_costs_what_the_swing_took() {
        use primitive_shared::types::{break_seconds, BLOCK_PLANKS, BLOCK_SNOW};
        let mut stamina = Stamina::new();
        let before = stamina.current();
        stamina.spend_dig(break_seconds(BLOCK_PLANKS).unwrap());
        let planks = before - stamina.current();
        assert!(planks > 0.0, "digging was free");

        // Softness, not depth. A drift used to be cheap because it was
        // an eighth of a block; snow is a whole block now and is cheap
        // because it is snow.
        let mut light_work = Stamina::new();
        light_work.spend_dig(break_seconds(BLOCK_SNOW).unwrap());
        let snow = MAX_STAMINA - light_work.current();
        assert!(
            snow < planks,
            "clearing snow ({snow}) cost as much as digging out planks ({planks})"
        );
    }

    #[test]
    fn a_swing_that_broke_nothing_is_not_billed() {
        let mut stamina = Stamina::new();
        stamina.spend_dig(0.0);
        stamina.spend_dig(-1.0);
        stamina.spend_dig(f32::NAN);
        assert_eq!(stamina.current(), MAX_STAMINA);
    }

    #[test]
    fn an_exhausted_player_digs_slower_rather_than_not_at_all() {
        // Being unable to break the block in front of you is how a
        // player ends up stuck in a hole with no way out but waiting.
        let mut stamina = Stamina::new();
        assert_eq!(stamina.dig_rate(), 1.0);
        for _ in 0..40 {
            stamina.spend_dig(2.0);
        }
        assert!(stamina.is_exhausted(), "the tank should be empty by now");
        assert!(stamina.dig_rate() > 0.0, "digging stopped altogether");
        assert!(stamina.dig_rate() < 1.0, "exhaustion cost nothing");
    }

    #[test]
    fn digging_and_jumping_draw_on_the_same_tank() {
        // One bar, and everything that is work comes out of it --
        // otherwise the answer to an empty bar is to stop running and
        // start digging, which is the harder work of the two.
        let mut stamina = Stamina::new();
        stamina.spend_dig(6.0);
        let after_digging = stamina.current();
        assert!(after_digging < MAX_STAMINA);
        stamina.spend_jump();
        assert!(stamina.current() < after_digging);
    }

    use super::*;

    fn run_for(stamina: &mut Stamina, seconds: f32, load: f32, sprinting: bool) {
        let step = 1.0 / 60.0;
        for _ in 0..((seconds / step) as usize) {
            stamina.update(step, load, sprinting);
        }
    }

    #[test]
    fn a_fresh_player_can_sprint() {
        let stamina = Stamina::new();
        assert!(stamina.can_sprint());
        assert_eq!(stamina.fraction(), 1.0);
    }

    #[test]
    fn a_comfortable_player_gets_their_breath_back_faster_than_a_miserable_one() {
        let mut snug = Stamina::new();
        let mut cold = Stamina::new();
        snug.set_recovery(primitive_shared::comfort::FASTEST_RECOVERY);
        cold.set_recovery(primitive_shared::comfort::SLOWEST_RECOVERY);
        for stamina in [&mut snug, &mut cold] {
            stamina.current = 0.0;
            for _ in 0..20 {
                stamina.update(0.05, 1.0, false);
            }
        }
        assert!(cold.current() > 0.0, "a fully laden miserable player never recovered");
        assert!(snug.current() > cold.current() * 2.0, "comfort did not speed recovery");
        // ...and a hostile number off the socket is held to the rule's range.
        let mut cheat = Stamina::new();
        cheat.set_recovery(1000.0);
        assert_eq!(cheat.recovery_scale, primitive_shared::comfort::FASTEST_RECOVERY);
        cheat.set_recovery(f32::NAN);
        assert_eq!(cheat.recovery_scale, 1.0);
    }

    #[test]
    fn sprinting_drains_and_resting_recovers() {
        let mut stamina = Stamina::new();
        run_for(&mut stamina, 4.0, 0.0, true);
        let spent = stamina.current();
        assert!(spent < MAX_STAMINA, "sprinting cost nothing");
        assert!(spent > 0.0, "four seconds should not empty the tank");

        run_for(&mut stamina, 4.0, 0.0, false);
        assert!(stamina.current() > spent, "resting did not recover");
    }

    #[test]
    fn a_load_makes_sprinting_dearer_and_recovery_slower() {
        let mut light = Stamina::new();
        let mut heavy = Stamina::new();
        run_for(&mut light, 3.0, 0.0, true);
        run_for(&mut heavy, 3.0, 1.0, true);
        assert!(
            heavy.current() < light.current(),
            "weight did not affect the sprint: {} vs {}",
            heavy.current(),
            light.current()
        );

        // And the same again for getting it back.
        let mut light = Stamina::new();
        let mut heavy = Stamina::new();
        light.current = 1.0;
        heavy.current = 1.0;
        run_for(&mut light, 2.0, 0.0, false);
        run_for(&mut heavy, 2.0, 1.0, false);
        assert!(
            heavy.current() < light.current(),
            "weight did not slow recovery"
        );
    }

    #[test]
    fn even_a_full_load_recovers_eventually() {
        // The deadlock this guards against: if carrying cost more than
        // resting returned, a fully laden player could never sprint
        // again and would have no way to find out why.
        let mut stamina = Stamina::new();
        stamina.current = 0.0;
        stamina.exhausted = true;
        // Fifteen seconds of standing still, up from ten now that
        // recovery is deliberately slower. It is still a *bounded* wait
        // with the worst possible pack on, which is the property that
        // matters: longer than this and the player has no way to tell
        // recovery from a lockout.
        run_for(&mut stamina, 15.0, 1.0, false);
        assert!(
            stamina.can_sprint(),
            "a fully laden player never got their breath back: {}",
            stamina.current()
        );
    }

    #[test]
    fn resting_under_a_full_load_gains_ground_at_a_useful_rate() {
        // The failure mode this guards against is a net recovery that is
        // positive but tiny: technically fine, and indistinguishable
        // from being stuck.
        let net = RECOVERY * (1.0 - RECOVERY_LOAD_PENALTY) - CARRY_DRAIN;
        assert!(
            net > 0.3,
            "a full load recovers at {net:.2} a second, which is a lockout in disguise"
        );
    }

    #[test]
    fn running_out_locks_the_sprint_until_a_real_recovery() {
        // Otherwise the player pumps Shift against an empty bar and
        // moves in stutters, which reads as broken controls.
        let mut stamina = Stamina::new();
        run_for(&mut stamina, 30.0, 0.0, true);
        assert_eq!(stamina.current(), 0.0);
        assert!(stamina.is_exhausted());
        assert!(!stamina.can_sprint());

        // A sliver back is not enough.
        run_for(&mut stamina, 0.2, 0.0, false);
        assert!(
            !stamina.can_sprint(),
            "sprinting came back at {:.0}%",
            stamina.fraction() * 100.0
        );

        run_for(&mut stamina, 5.0, 0.0, false);
        assert!(stamina.can_sprint(), "never recovered");
        assert!(!stamina.is_exhausted());
    }

    #[test]
    fn stamina_never_leaves_its_range() {
        let mut stamina = Stamina::new();
        run_for(&mut stamina, 60.0, 1.0, true);
        assert!(stamina.current() >= 0.0);
        run_for(&mut stamina, 120.0, 0.0, false);
        assert!(stamina.current() <= MAX_STAMINA);
        assert!(stamina.fraction() <= 1.0 && stamina.fraction() >= 0.0);
    }

    #[test]
    fn nonsense_input_does_not_break_it() {
        let mut stamina = Stamina::new();
        // A long stall must not drain the whole tank in one frame.
        stamina.update(10.0, 0.0, true);
        assert!(stamina.current() > MAX_STAMINA * 0.9);
        // Loads outside 0..1 are clamped rather than trusted.
        stamina.update(1.0 / 60.0, 50.0, true);
        stamina.update(1.0 / 60.0, -50.0, false);
        assert!(stamina.current().is_finite());
    }

    #[test]
    fn a_jump_costs_a_lump_of_the_tank() {
        let mut stamina = Stamina::new();
        assert!(stamina.can_jump());
        stamina.spend_jump();
        assert_eq!(stamina.current(), MAX_STAMINA - JUMP_COST);
        // Standing still puts it back.
        run_for(&mut stamina, 2.0, 0.0, false);
        assert!(stamina.current() > MAX_STAMINA - JUMP_COST);
    }

    #[test]
    fn hopping_across_the_world_runs_you_out_of_breath() {
        // The whole reason jumps are billed: with only the sprint
        // charged, the cheapest way to travel was to hop.
        let mut stamina = Stamina::new();
        let mut jumps = 0;
        while stamina.can_jump() {
            stamina.spend_jump();
            jumps += 1;
            // Hops come about twice a second, and what recovers in
            // between is nothing like what one costs.
            run_for(&mut stamina, 0.5, 0.0, false);
        }
        assert!(jumps >= 8, "only {jumps} hops before the bar gave out");
        assert!(jumps <= 40, "{jumps} hops on one tank is not a cost at all");
        assert!(!stamina.can_jump());
    }

    #[test]
    fn an_empty_bar_refuses_the_jump_rather_than_going_negative() {
        let mut stamina = Stamina::new();
        run_for(&mut stamina, 60.0, 0.0, true);
        assert!(stamina.is_exhausted());
        assert!(!stamina.can_jump(), "an exhausted player jumped");
        stamina.spend_jump();
        assert_eq!(stamina.current(), 0.0, "the tank went past empty");
    }

    #[test]
    fn a_jump_that_empties_the_tank_exhausts_the_player() {
        // Or the bar would sit at zero saying the sprint is available.
        let mut stamina = Stamina::new();
        stamina.current = JUMP_COST;
        stamina.spend_jump();
        assert!(stamina.is_exhausted());
        assert!(!stamina.can_sprint());
    }

    #[test]
    fn respawning_gives_a_clean_slate() {
        let mut stamina = Stamina::new();
        run_for(&mut stamina, 30.0, 1.0, true);
        assert!(stamina.is_exhausted());
        stamina.reset();
        assert!(stamina.can_sprint());
        assert_eq!(stamina.fraction(), 1.0);
    }
}
