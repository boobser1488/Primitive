//! Comfort: how well a body is doing *where it is*, and what that is worth
//! to its healing and its breath.
//!
//! **Hidden, and on purpose.** There is no bar. The player asked for a
//! number they never see ("скрытый показатель комфорта"), and the argument
//! for keeping it that way is the design rule itself: a bar is a chore to
//! keep full, a house is a place to come back to. What a player notices is
//! that a wound closes faster at home and the breath comes back quicker by
//! the fire -- and what they decide is where to rest.
//!
//! **Not `body::Comfort`.** That word was already taken, by the band the
//! skin temperature sits in (cold, cool, comfortable...), and it is one
//! input here among nine. The value this module works out is called
//! *comfort* in the prose and `comfort_level` in the code wherever the two
//! could be confused.
//!
//! ## The shape of it
//!
//! A *target* in `LOWEST..=HIGHEST` is added up from the body (warmth, rest,
//! how dirty, how wet, bleeding, smoke) and from the place (a roof and walls,
//! furniture, windows, a fire, and filth nearby). The value itself moves
//! towards the target over tens of seconds (`step`), and what it buys is a
//! multiplier on health regeneration and stamina recovery (`recovery`).
//!
//! The rules are here, with no I/O, so the server that owns the number and
//! the tests that pin it read one copy. `survey` takes a `look` closure the
//! way `wildfire::smoke_room` does, for the same reason.

use crate::types::{
    block_kind, blocks_the_sky, is_carcass, BlockId, BLOCK_BED, BLOCK_BLOOMERY_LIT, BLOCK_BONES,
    BLOCK_BONES_2, BLOCK_CAMPFIRE_LIT, BLOCK_CHAIR, BLOCK_CHEST, BLOCK_DRYING_RACK, BLOCK_DUNG,
    BLOCK_FIREPIT_LIT, BLOCK_KILN_LIT, BLOCK_LEATHER_BENCH, BLOCK_MUD, BLOCK_POTTERS_WHEEL,
    BLOCK_STANDING_TORCH_LIT, BLOCK_STOOL, BLOCK_STRAW_BED, BLOCK_TABLE, BLOCK_TORCH_LIT,
    BLOCK_WORKBENCH,
};
use crate::wildfire::{smoke_room, Room};

/// The worst comfort there is: cold, wet, filthy and bleeding in the smoke.
pub const LOWEST: f32 = -1.0;
/// The best: warm, rested and clean in a furnished house with a fire.
pub const HIGHEST: f32 = 1.0;

/// The slowest a body recovers, as a multiplier, at `LOWEST`.
///
/// **Half, not nothing.** Healing already has hard gates -- an empty
/// stomach, a freezing body, an open wound, illness each stop it outright
/// (`survival::Vitals::regenerate` on the server) -- and a comfort that could
/// reach zero would be a fifth gate that says the same things again, less
/// clearly. This is a *rate*: a miserable camp still mends you, slowly.
pub const SLOWEST_RECOVERY: f32 = 0.5;

/// The fastest, at `HIGHEST`.
///
/// **Half again, not double.** The diet already spans a third to one and
/// sleep past tiredness another factor under that; at two a house would
/// out-weigh the diet, and a hut with a bed in it would be the one correct
/// answer to every wound. Half again is enough to be felt -- a boar's worth
/// of damage back in two minutes rather than three -- without making the
/// meadow a place nobody can recover in.
///
/// Rejected: 0.8..1.2. Tried on paper against the four hundred seconds a
/// near-death body takes to fill: a fifth either way is forty seconds, which
/// nobody standing in a house notices, and a hidden value nobody notices is
/// a value that does not exist.
pub const FASTEST_RECOVERY: f32 = 1.5;

/// Seconds for comfort to close about two thirds of the gap to its target.
///
/// **Tens of seconds, so it does not flicker.** Stepping through a doorway
/// changes the target at once; a value that followed it at once would make
/// healing lurch every time somebody fetched wood. Twenty seconds means a
/// minute indoors is most of the benefit, and a quick trip out costs little
/// -- which is how a home feels.
pub const SETTLE_SECONDS: f32 = 20.0;

/// How often the server looks at a player's surroundings again, in seconds.
///
/// A room changes when somebody opens a wall or lights a fire, and the value
/// the survey feeds is itself settling over `SETTLE_SECONDS`: looking every
/// tick would pay for a flood fill twenty times a second to move a number
/// that cannot follow it anyway. Two of the smoke's steps
/// (`wildfire::SMOKE_STEP_SECONDS`).
pub const SURVEY_SECONDS: f32 = 4.0;

/// How far round the feet the survey looks for filth and fire, in blocks.
///
/// Three: the width of a small hut, so a carcass left by the door is inside
/// it and the midden a dozen steps off is not. A radius is what makes
/// *where* a player leaves things a decision.
pub const NEAR: i32 = 3;

/// What a survey of the place found. Kept by the server between surveys.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct Surroundings {
    /// 0 out of doors, 1 in a shut room; a room with openings between, as
    /// the smoke's own share (`wildfire::smoke_kept`). A doorway keeps about
    /// three quarters; a hole in the roof half.
    pub enclosure: f32,
    /// Furniture in that room, summed (see `furniture_worth`) and capped.
    pub furniture: f32,
    /// Openings in the room's walls at sill height. See `survey`.
    pub windows: u32,
    /// A lit hearth within `NEAR`.
    pub hearth: bool,
    /// Filth within `NEAR`, summed (see `filth_worth`) and capped.
    pub filth: f32,
    /// Standing on mud, which is what dirties a body while it stands still.
    pub on_mud: bool,
}

/// What a body brings to comfort, read off the server's own vitals.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Condition {
    /// Skin temperature, in `body`'s degrees.
    pub body_c: f32,
    /// 0 fresh .. 1 finished.
    pub fatigue: f32,
    /// Lying in a bed. See `target` for why that counts as rested.
    pub asleep: bool,
    /// 0 clean .. 1 caked. See `GRIME_PER_LOOSE_BLOCK`.
    pub grime: f32,
    /// 0 dry .. 1 soaked.
    pub wetness: f32,
    /// A cut still open.
    pub bleeding: bool,
    /// The thickness of the smoke at the eyes, 0..1.
    pub smoke: f32,
}

/// What one block of furniture in the room is worth.
///
/// **A bed first**, because a bed is what makes a shelter a home; a table
/// and a chair next, because they are what a player builds once they mean
/// to stay; a chest and the working stations after, because a place where
/// things are kept and made is lived in. Numbers small enough that the cap
/// (`FURNITURE_CAP`) is reached by a furnished room, not by a warehouse of
/// stools.
pub fn furniture_worth(block: BlockId) -> f32 {
    match block_kind(block) {
        BLOCK_BED => 0.12,
        BLOCK_STRAW_BED => 0.08,
        BLOCK_CHAIR | BLOCK_TABLE => 0.06,
        BLOCK_STOOL | BLOCK_CHEST => 0.04,
        BLOCK_TORCH_LIT | BLOCK_STANDING_TORCH_LIT => 0.04,
        // ...and the anvil, which is a workshop in a room whatever the recipe
        // table calls it. The mason's block is still not here: it is a slab on
        // a stump in a quarry, and a room is not more homely for having rubble
        // in the corner.
        BLOCK_WORKBENCH | BLOCK_LEATHER_BENCH | BLOCK_POTTERS_WHEEL | BLOCK_DRYING_RACK | crate::types::BLOCK_HIDE_FRAME
        | crate::types::BLOCK_ANVIL => 0.03,
        _ => 0.0,
    }
}

/// The most furniture adds.
pub const FURNITURE_CAP: f32 = 0.3;

/// What one block of filth nearby takes away.
///
/// Dung worst, because it is the one a player made and can move; a carcass
/// and bones next, the leftovers of a hunt nobody cleared; mud least, a
/// cell at a time, because a swamp is mud and a whole swamp should be
/// unpleasant rather than unbearable (the cap does that).
pub fn filth_worth(block: BlockId) -> f32 {
    let kind = block_kind(block);
    if kind == BLOCK_DUNG {
        0.15
    } else if is_carcass(block) {
        0.1
    } else if kind == BLOCK_BONES || kind == BLOCK_BONES_2 {
        0.05
    } else if kind == BLOCK_MUD {
        0.02
    } else {
        0.0
    }
}

/// The most filth takes away.
pub const FILTH_CAP: f32 = 0.5;

/// A lit fire to sit by.
fn is_hearth(block: BlockId) -> bool {
    matches!(
        block_kind(block),
        BLOCK_CAMPFIRE_LIT | BLOCK_FIREPIT_LIT | BLOCK_KILN_LIT | BLOCK_BLOOMERY_LIT
    )
}

/// Looks at the place a player's feet are in.
///
/// **The room is the smoke's room** (`wildfire::smoke_room`), started from
/// the feet's cell rather than the cell over a fire. It was the obvious
/// second flood fill to write here, and the reason not to is that the two
/// must agree: a room that smokes like a house and does not comfort like one
/// -- or the other way round -- would be two rules for one building. What
/// that costs is the smoke's own limit: a hall past `ROOM_MAX_CELLS` is out
/// of doors to both.
///
/// **Furniture counts only under a roof.** A bed in a field is not a home,
/// and a player who could carry comfort about as a chest and a stool would
/// never build a wall. The fire is the exception, and counts anywhere near:
/// sitting by a campfire under the stars is the first comfort there is.
///
/// **A window is an opening at sill height**: a cell the room could not
/// follow into (it has no roof), beside a room cell that is not on the
/// floor, with something solid under it. A doorway's upper cell has the
/// doorway's lower cell under it -- open -- and so is not a window. Glass
/// does not exist yet; its draught is already paid for in `enclosure`,
/// and what the window buys back is the light and the view.
pub fn survey(look: impl Fn(i32, i32, i32) -> Option<BlockId>, feet: (i32, i32, i32)) -> Surroundings {
    let mut found = Surroundings::default();
    let (fx, fy, fz) = feet;
    for dx in -NEAR..=NEAR {
        for dz in -NEAR..=NEAR {
            for dy in -1..=2 {
                if let Some(block) = look(fx + dx, fy + dy, fz + dz) {
                    found.filth += filth_worth(block);
                    found.hearth |= is_hearth(block);
                }
            }
        }
    }
    found.filth = found.filth.min(FILTH_CAP);
    found.on_mud = look(fx, fy - 1, fz).is_some_and(|b| block_kind(b) == BLOCK_MUD);

    // `smoke_room` starts one over what it is given.
    let (cells, enclosure) = match smoke_room(&look, (fx, fy - 1, fz)) {
        Room::Vented => return found,
        Room::Closed(cells) => (cells, 1.0),
        Room::Leaky(cells, kept) => (cells, kept),
    };
    found.enclosure = enclosure.clamp(0.0, 1.0);
    let room: std::collections::HashSet<(i32, i32, i32)> = cells.iter().copied().collect();
    let solid = |at: (i32, i32, i32)| look(at.0, at.1, at.2).is_some_and(blocks_the_sky);
    let mut seen = std::collections::HashSet::new();
    let mut openings = std::collections::HashSet::new();
    let mut furniture = 0.0;
    for &(x, y, z) in &cells {
        for (dx, dy, dz) in [(0, 0, 0), (0, -1, 0), (1, 0, 0), (-1, 0, 0), (0, 0, 1), (0, 0, -1)] {
            let at = (x + dx, y + dy, z + dz);
            let Some(block) = look(at.0, at.1, at.2) else {
                continue;
            };
            if seen.insert(at) {
                furniture += furniture_worth(block);
            }
            // An opening above the floor the player stands on, with a sill
            // under it or under the room's cell beside it -- the second for
            // a roof that overhangs the wall, where the gap in the wall is
            // itself under the eaves and so part of the room. A doorway's
            // cells have open air or the floor under them, and are at the
            // floor, and so are never counted.
            let sideways = dy == 0 && (dx != 0 || dz != 0);
            if sideways
                && y > fy
                && !room.contains(&at)
                && !blocks_the_sky(block)
                && (solid((at.0, at.1 - 1, at.2)) || solid((x, y - 1, z)))
            {
                openings.insert(at);
            }
        }
    }
    found.windows = openings.len() as u32;
    found.furniture = furniture.min(FURNITURE_CAP);
    found
}

/// Where comfort is heading, from the body and the place.
///
/// A sum of terms rather than a product, because each is a separate thing a
/// player can fix and each should be worth fixing on its own: a product
/// would make the fire worth nothing to somebody soaked, which is exactly
/// when it is worth most.
pub fn target(body: Condition, place: Surroundings) -> f32 {
    use crate::body::{COMFORT_HIGH, COMFORT_LOW, FREEZING, SCALDING, TIRED_AT};
    let mut sum = 0.0;

    // **Warmth.** Inside the band is a small plus; outside it a minus that
    // grows to its worst at the harm lines, where `regenerate` stops healing
    // outright anyway.
    let c = if body.body_c.is_finite() { body.body_c } else { crate::body::NEUTRAL_C };
    sum += if c < COMFORT_LOW {
        -0.4 * ((COMFORT_LOW - c) / (COMFORT_LOW - FREEZING)).min(1.0)
    } else if c > COMFORT_HIGH {
        -0.4 * ((c - COMFORT_HIGH) / (SCALDING - COMFORT_HIGH)).min(1.0)
    } else {
        0.15
    };

    // **Rest.** Lying in a bed counts as rested whatever the fatigue says:
    // a body going to sleep tired is the case the bed exists for, and
    // scoring it as uncomfortable would slow the healing sleep is meant to
    // bring.
    let fatigue = if body.fatigue.is_finite() { body.fatigue.clamp(0.0, 1.0) } else { 0.0 };
    sum += if body.asleep || fatigue <= TIRED_AT {
        0.1
    } else {
        -0.3 * (fatigue - TIRED_AT) / (1.0 - TIRED_AT)
    };

    // **Cleanliness and dryness.**
    let grime = if body.grime.is_finite() { body.grime.clamp(0.0, 1.0) } else { 0.0 };
    sum += if grime < 0.2 { 0.05 } else { -0.2 * grime };
    let wet = if body.wetness.is_finite() { body.wetness.clamp(0.0, 1.0) } else { 0.0 };
    sum -= 0.25 * wet;
    if body.bleeding {
        sum -= 0.15;
    }
    let smoke = if body.smoke.is_finite() { body.smoke.clamp(0.0, 1.0) } else { 0.0 };
    sum -= 0.4 * smoke;

    // **The place.**
    let enclosure = place.enclosure.clamp(0.0, 1.0);
    sum += 0.25 * enclosure;
    if enclosure > 0.0 {
        sum += place.furniture.clamp(0.0, FURNITURE_CAP);
        sum += 0.05 * place.windows.min(2) as f32;
    }
    if place.hearth {
        sum += 0.1;
    }
    sum -= place.filth.clamp(0.0, FILTH_CAP);

    sum.clamp(LOWEST, HIGHEST)
}

/// One step of comfort towards its target.
///
/// Exponential rather than a fixed rate, so a large change is felt at once
/// and settles, and a small one does not creep for a minute. Written against
/// `dt` so a night slept through in one step lands where the same night
/// ticked would.
pub fn step(current: f32, target: f32, dt: f32) -> f32 {
    let current = if current.is_finite() { current } else { 0.0 };
    let target = if target.is_finite() { target.clamp(LOWEST, HIGHEST) } else { 0.0 };
    if !dt.is_finite() || dt <= 0.0 {
        return current.clamp(LOWEST, HIGHEST);
    }
    let pull = 1.0 - (-dt / SETTLE_SECONDS).exp();
    (current + (target - current) * pull).clamp(LOWEST, HIGHEST)
}

/// What a comfort level does to regeneration and stamina recovery.
pub fn recovery(comfort_level: f32) -> f32 {
    let level = if comfort_level.is_finite() { comfort_level.clamp(LOWEST, HIGHEST) } else { 0.0 };
    if level >= 0.0 {
        1.0 + level * (FASTEST_RECOVERY - 1.0)
    } else {
        1.0 + level * (1.0 - SLOWEST_RECOVERY)
    }
}

// ---- grime ----

/// Grime from breaking one block of loose ground: a hundred holes to caked.
pub const GRIME_PER_LOOSE_BLOCK: f32 = 0.01;
/// ...and from breaking mud, or a dung pat: three times that.
pub const GRIME_PER_FILTHY_BLOCK: f32 = 0.03;
/// Grime a second standing on mud: two minutes in a swamp to caked.
pub const GRIME_ON_MUD_PER_SECOND: f32 = 1.0 / 120.0;
/// Washed off a second in water: five seconds in a river cleans anybody.
pub const WASH_IN_WATER_PER_SECOND: f32 = 1.0 / 5.0;
/// ...and in the rain: a minute and a half of standing in it.
pub const WASH_IN_RAIN_PER_SECOND: f32 = 1.0 / 90.0;

/// One step of grime.
///
/// **Water is the answer, and that is the point.** Being clean costs a walk
/// to the river or standing in the rain -- which also makes you wet, which
/// comfort also counts. A player chooses which to be for a while.
pub fn step_grime(grime: f32, dt: f32, in_water: bool, rained_on: bool, on_mud: bool) -> f32 {
    let grime = if grime.is_finite() { grime } else { 0.0 };
    if !dt.is_finite() || dt <= 0.0 {
        return grime.clamp(0.0, 1.0);
    }
    let mut next = grime;
    if in_water {
        next -= WASH_IN_WATER_PER_SECOND * dt;
    } else if rained_on {
        next -= WASH_IN_RAIN_PER_SECOND * dt;
    } else if on_mud {
        next += GRIME_ON_MUD_PER_SECOND * dt;
    }
    next.clamp(0.0, 1.0)
}

// ---- dung ----

/// How much a body has to have eaten, in whole bars of hunger, before it has
/// to go.
///
/// One bar: about as often as a player eats a full day's food, which is
/// "occasionally" and not a chore.
pub const DUNG_PER_BAR: f32 = 1.0;

/// How far past `DUNG_PER_BAR` a body holds on while it is indoors.
///
/// **A body with somewhere to go goes outside.** The pat is left the first
/// time the player stands out of doors once it is due -- so a player who
/// comes and goes never fouls their own floor -- and only one who has not
/// left the house for another half bar of food finds it on the floor. That
/// is the decision: step out now and then, or live with it.
pub const DUNG_HELD_INDOORS: f32 = 0.5;

/// Whether a body that has eaten `owed` bars since it last went goes now, in
/// a place of this `enclosure`.
pub fn goes_now(owed: f32, enclosure: f32) -> bool {
    owed >= DUNG_PER_BAR && (enclosure <= 0.0 || owed >= DUNG_PER_BAR + DUNG_HELD_INDOORS)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::{BLOCK_AIR, BLOCK_PLANKS, BLOCK_STONE};

    fn calm() -> Condition {
        Condition {
            body_c: crate::body::NEUTRAL_C,
            fatigue: 0.0,
            asleep: false,
            grime: 0.0,
            wetness: 0.0,
            bleeding: false,
            smoke: 0.0,
        }
    }

    /// A 5x5 stone floor at y = 9, plank walls round a 3x3 inside, a plank
    /// roof at y = 12, and whatever `inside` puts in the room.
    fn hut(roofed: bool, inside: &[((i32, i32, i32), BlockId)]) -> impl Fn(i32, i32, i32) -> Option<BlockId> + '_ {
        move |x, y, z| {
            if let Some(&(_, block)) = inside.iter().find(|(at, _)| *at == (x, y, z)) {
                return Some(block);
            }
            if y < 10 {
                return Some(BLOCK_STONE);
            }
            let wall = (x == -2 || x == 2 || z == -2 || z == 2) && (-2..=2).contains(&x) && (-2..=2).contains(&z);
            if wall && y < 12 {
                return Some(BLOCK_PLANKS);
            }
            if roofed && y == 12 && (-2..=2).contains(&x) && (-2..=2).contains(&z) {
                return Some(BLOCK_PLANKS);
            }
            Some(BLOCK_AIR)
        }
    }

    #[test]
    fn a_roofed_hut_with_a_bed_is_enclosed_and_furnished_and_a_field_is_neither() {
        let bed = [((1, 10, 1), BLOCK_BED)];
        let home = survey(hut(true, &bed), (0, 10, 0));
        assert_eq!(home.enclosure, 1.0, "a shut hut was not enclosed: {home:?}");
        assert!(home.furniture > 0.1, "the bed was not found: {home:?}");
        let open = survey(hut(false, &bed), (0, 10, 0));
        assert_eq!(open.enclosure, 0.0, "walls with no roof were a room: {open:?}");
        assert_eq!(open.furniture, 0.0, "a bed under the sky counted as a home");
    }

    #[test]
    fn a_window_at_sill_height_is_found_and_a_doorway_is_not() {
        let window = [((2, 11, 0), BLOCK_AIR)];
        assert_eq!(survey(hut(true, &window), (0, 10, 0)).windows, 1);
        let door = [((2, 10, 0), BLOCK_AIR), ((2, 11, 0), BLOCK_AIR)];
        assert_eq!(survey(hut(true, &door), (0, 10, 0)).windows, 0);
    }

    #[test]
    fn dung_and_carcasses_near_the_feet_are_filth_and_far_ones_are_not() {
        let near = [((1, 10, 0), BLOCK_DUNG)];
        assert!(survey(hut(false, &near), (0, 10, 0)).filth > 0.1);
        let far = [((NEAR + 2, 10, 0), BLOCK_DUNG)];
        assert_eq!(survey(hut(false, &far), (0, 10, 0)).filth, 0.0);
    }

    #[test]
    fn comfort_is_bounded_whatever_it_is_given() {
        let worst = Condition {
            body_c: -50.0,
            fatigue: 1.0,
            asleep: false,
            grime: 1.0,
            wetness: 1.0,
            bleeding: true,
            smoke: 1.0,
        };
        let filthy = Surroundings { filth: 99.0, ..Default::default() };
        assert_eq!(target(worst, filthy), LOWEST);
        let best = Surroundings { enclosure: 1.0, furniture: 9.0, windows: 9, hearth: true, filth: 0.0, on_mud: false };
        let t = target(calm(), best);
        assert!((LOWEST..=HIGHEST).contains(&t));
        let nan = Condition { body_c: f32::NAN, fatigue: f32::NAN, grime: f32::NAN, wetness: f32::NAN, smoke: f32::NAN, ..calm() };
        assert!(target(nan, best).is_finite());
        for level in [-9.0, LOWEST, 0.0, HIGHEST, 9.0, f32::NAN] {
            let r = recovery(level);
            assert!((SLOWEST_RECOVERY..=FASTEST_RECOVERY).contains(&r), "{level} recovers at {r}");
        }
        assert_eq!(step(0.0, 50.0, 1e9), HIGHEST);
    }

    #[test]
    fn comfort_moves_towards_its_target_smoothly_and_not_at_once() {
        let mut level = 0.0;
        let mut last_jump: f32 = 0.0;
        for _ in 0..20 {
            let next = step(level, HIGHEST, 0.05);
            last_jump = last_jump.max(next - level);
            level = next;
        }
        assert!(level < 0.1, "a second indoors was most of the comfort: {level}");
        assert!(last_jump < 0.01, "one tick moved comfort by {last_jump}");
        for _ in 0..(60 * 20) {
            level = step(level, HIGHEST, 0.05);
        }
        assert!(level > 0.9, "a minute indoors did not settle: {level}");
        // A night in one step lands where the ticks do.
        let ticked = (0..200).fold(0.0, |l, _| step(l, 0.8, 0.1));
        assert!((step(0.0, 0.8, 20.0) - ticked).abs() < 1e-3);
    }

    #[test]
    fn a_furnished_warm_house_beats_a_cold_wet_filthy_field() {
        let house = Surroundings { enclosure: 1.0, furniture: 0.2, windows: 1, hearth: true, filth: 0.0, on_mud: false };
        let field = Surroundings { filth: 0.4, ..Default::default() };
        let cold_wet = Condition { body_c: crate::body::CHILLED, wetness: 1.0, grime: 0.6, ..calm() };
        assert!(recovery(target(calm(), house)) > 1.3);
        assert!(recovery(target(cold_wet, field)) < 0.7);
    }

    #[test]
    fn water_washes_grime_off_faster_than_rain_and_mud_puts_it_on() {
        assert!(step_grime(1.0, 5.0, true, false, false) <= 0.0);
        let rained = step_grime(1.0, 5.0, false, true, false);
        assert!(rained < 1.0 && rained > 0.5);
        assert!(step_grime(0.0, 10.0, false, false, true) > 0.0);
    }

    #[test]
    fn a_body_that_is_due_goes_outside_first_and_indoors_only_when_it_cannot_hold() {
        assert!(!goes_now(0.9, 0.0));
        assert!(goes_now(DUNG_PER_BAR, 0.0));
        assert!(!goes_now(DUNG_PER_BAR, 0.8), "went on the floor the moment it was due");
        assert!(goes_now(DUNG_PER_BAR + DUNG_HELD_INDOORS, 0.8));
    }
}
