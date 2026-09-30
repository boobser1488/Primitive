//! The woods' and the water's animals, the bear's year, and the two things
//! every animal's body now says before it runs: that it was lying down, and
//! that it has just heard you.
//!
//! ## Why a module of its own
//!
//! `animals.rs` is one mind for twenty species, and it earns that: a deer, a
//! zebra and a horse are one animal with three sets of numbers. What is here
//! is the part that is *not* numbers -- a fox that carries a kill away, a
//! beaver that plugs moving water, a frog that goes quiet, a bear asleep for
//! the winter -- and each of them is a few decisions taken before the shared
//! mind is asked anything (`think_first`), plus a handful of hooks where the
//! shared body has to know (`walk`'s pace, `bolt`'s first step). Written
//! into `think` it would be five more `if species ==` in a function every
//! other animal walks through; here it is one call, and a new reader can see
//! all of a beaver in one place.
//!
//! It is a child of `animals` so it can read an `Animal`'s fields, which
//! stay private to everybody else.
//!
//! ## The two things every body says now
//!
//! * **Lying down costs something.** A herd resting through the middle of
//!   the day or the small hours lies down (`Attitude::Resting`) rather than
//!   dozing on its feet, and a lying animal has to get up before it can run
//!   (`RISING_SECONDS`). That is what makes a bedded herd the one herd a
//!   careful player can reach -- and it is on the wire, so the player can
//!   see which herd that is.
//! * **A fright is a freeze first.** Something noticed at a distance gets
//!   half a second of an animal frozen with its head flung up
//!   (`Attitude::Startled`) before it goes (`freeze`). A player who stops
//!   dead in that half second, and is not seen again, has turned a bolt
//!   into a wary animal that grazes on -- which is the oldest trick in
//!   stalking, and the one decision the old instant bolt left no room for.

use super::*;
use primitive_shared::protocol::Attitude;
use primitive_shared::season::Season;
use primitive_shared::types::BlockId;

/// What the woods' animals need from the tick that nothing else in `think`
/// does, and where they put what they did.
///
/// **A context rather than six more arguments** to a function that already
/// takes ten, and rather than fields on `Animals` read through `&self`: the
/// loop that thinks holds the animals mutably, and these are the parts of
/// the herd's world that loop can lend out beside them.
pub(super) struct Wild<'a> {
    pub season: Season,
    /// Meat lying on the ground, as the tick loop last saw it
    /// (`Animals::meat_lying`).
    pub meat: &'a [(f32, f32, f32)],
    /// Cells where water moved in the last few seconds
    /// (`Animals::hear_running_water`).
    pub running: &'a [(i32, i32, i32)],
    /// What the animals did to the world this tick, for the tick loop to
    /// write (`Animals::take_edits`).
    pub edits: &'a mut Vec<Edit>,
    /// Meat a fox has its jaws on: which fox, and where the meat was
    /// (`Animals::take_meat_taken`).
    pub taken: &'a mut Vec<(EntityId, (f32, f32, f32))>,
}

/// Something an animal did to a cell of the world.
///
/// **Handed out, not written**, for the reason every other change an animal
/// makes is (`take_dung`, `take_hay_eaten`): `set_block` and its broadcast
/// are the tick loop's, and they happen after the animals' lock is let go.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Edit {
    /// A beaver laid a stick of its dam in this cell of moving water.
    Dam((i32, i32, i32)),
    /// A bear ate at a berry bush or a wild hive in this cell.
    Forage((i32, i32, i32)),
    /// A fox ate into a carcass lying in this cell: one cut's worth goes.
    Gnaw((i32, i32, i32)),
}

/// What a fox has in its jaws.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(super) enum Carried {
    /// Something it killed: struck, it drops the body (`Animals::strike`).
    Kill(Species),
    /// A piece of meat it took off the ground: struck, it drops the piece.
    Meat(BlockId),
}

// ---- the body ----

/// How long an animal that was lying down takes to get up, in seconds.
///
/// **Seven tenths**: long enough that a player within a spear's throw of a
/// bedded deer gets the throw, short enough that a herd resting in the open
/// is still a herd a wolf has to be quick about. See the module doc.
pub(super) const RISING_SECONDS: f32 = 0.7;

/// How long a startled animal stands frozen before it decides, in seconds.
const FREEZE_SECONDS: (f32, f32) = (0.45, 0.85);

/// Nearer than this, in blocks, there is no freeze: a person three strides
/// away is not a sound to be placed, it is a person, and the animal goes.
const FREEZE_NEAR: f32 = 6.0;

/// How long after a freeze before an animal will freeze again, in seconds:
/// otherwise a player who froze with it and moved again would be frozen at
/// for ever, one half second at a time, and never bolted from.
const FREEZE_AGAIN: f32 = 6.0;

/// Of the animals resting, how many lie down rather than doze on their feet.
const LIE_CHANCE: f32 = 0.6;

/// Does this animal lie down to rest? The four-legged grazers and hunters.
/// Not a bird, a fish, a crab, a rat or a frog -- which rest as they stand,
/// or have no standing to rest from -- and not the beaver, which rests in
/// its lodge where nobody sees it.
pub(super) fn lies_down(species: Species) -> bool {
    matches!(
        species,
        Species::Hare
            | Species::Deer
            | Species::Boar
            | Species::Wolf
            | Species::Sheep
            | Species::Bear
            | Species::Zebra
            | Species::Antelope
            | Species::Lion
            | Species::Horse
            | Species::Fox
            | Species::Elk
    )
}

/// What a resting animal does with its body: lies down, or dozes on its
/// feet. See `lies_down`.
pub(super) fn rest(species: Species, rng: &mut Rng) -> Attitude {
    if lies_down(species) && rng.chance(LIE_CHANCE) {
        Attitude::Resting
    } else {
        Attitude::Dozing
    }
}

/// The first step of a bolt, for an animal that was lying down: it gets up.
/// Called from `bolt`, which is where every fright ends.
pub(super) fn rise(animal: &mut Animal) {
    if animal.attitude == Attitude::Resting {
        animal.rising_for = RISING_SECONDS;
        animal.attitude = Attitude::Startled;
    }
}

/// **The freeze**: something noticed at a distance, the head flung up and
/// the body still, for half a second before the bolt.
///
/// `true` when it froze, and the caller does not bolt. Only an animal that
/// was at ease -- grazing, walking, drinking -- freezes: one already wary
/// goes at once, and one that froze a moment ago does not freeze again
/// (`FREEZE_AGAIN`). Nothing that fights and nothing that flies: a flushed
/// bird is in the air before it has thought about it.
pub(super) fn freeze(animal: &mut Animal, at: (f32, f32, f32), distance: f32, rng: &mut Rng) -> bool {
    if animal.fights()
        || animal.species.flies()
        || animal.species.swims()
        || animal.freeze_for > 0.0
        || animal.wary_for > 0.0
        || distance < FREEZE_NEAR
        || !matches!(animal.mind, Mind::Idle | Mind::Wander | Mind::Drink)
    {
        return false;
    }
    let seconds = rng.range(FREEZE_SECONDS.0, FREEZE_SECONDS.1);
    animal.freeze_for = seconds + FREEZE_AGAIN;
    animal.mind = Mind::Watch;
    animal.attitude = if animal.attitude == Attitude::Resting {
        // Lying, it lifts its head where it lies; it has to get up anyway
        // before it can go (`rise`).
        Attitude::Resting
    } else {
        Attitude::Startled
    };
    animal.threat_at = Some((at.0, at.2));
    animal.wants_yaw = (at.2 - animal.at().2).atan2(at.0 - animal.at().0);
    animal.next_thought = seconds;
    true
}

/// The timers of this module, run down every tick from the top of `think`.
pub(super) fn tick(animal: &mut Animal, dt: f32) {
    animal.rising_for = (animal.rising_for - dt).max(0.0);
    animal.freeze_for = (animal.freeze_for - dt).max(0.0);
    animal.under_for = (animal.under_for - dt).max(0.0);
    animal.dam_for = (animal.dam_for - dt).max(0.0);
    animal.groggy_for = (animal.groggy_for - dt).max(0.0);
    animal.guard_for = (animal.guard_for - dt).max(0.0);
}

/// How fast it may go this tick, as a fraction of what its mind asked for.
///
/// Nought while it is getting up or asleep in its den; a third while a woken
/// bear is still half asleep (`GROGGY_SECONDS`). Read by `walk`.
pub(super) fn pace(animal: &Animal) -> f32 {
    if animal.rising_for > 0.0 || animal.denned {
        0.0
    } else if animal.groggy_for > 0.0 {
        GROGGY_PACE
    } else {
        1.0
    }
}

/// How fast it goes in the water, as a fraction of what it would go on land.
///
/// **The four that take to the water are not wading**, which is what
/// `WADE_SPEED` is for -- a deer that fell in and is floundering for the
/// bank. A duck paddles at its walk, a beaver is quicker in the water than
/// out of it (`Species::Beaver`), a frog kicks, and an elk swims as a horse
/// does: slowly, and without stopping.
pub(super) fn swim_pace(species: Species) -> f32 {
    match species {
        Species::Duck => 1.0,
        Species::Beaver => 1.35,
        Species::Frog => 0.8,
        Species::Elk => 0.55,
        _ => WADE_SPEED,
    }
}

/// How far an animal will stir to take what it wants, in blocks: its
/// temper, as it is today. See `Animal::fights`.
///
/// **One figure a species, with two exceptions that are about where it is
/// and not what it is**:
///
/// * *A cow elk with her calf* (`DEFEND_RANGE`). An elk is not a fighter,
///   and one that came at anybody who walked by would be a bear with
///   antlers; a mother between a player and the young one in the willows is
///   the one elk that does, and a player who sees the calf first has been
///   told.
/// * *A bear at its food in autumn* (`GUARD_RANGE`). A bear is half again
///   as quick to take offence at a berry patch or a hive it is eating from
///   as it is walking through a wood: see `forage`.
pub(super) fn provoke_range(animal: &Animal) -> f32 {
    if animal.denned {
        return 0.0;
    }
    match animal.species {
        Species::Elk if animal.young.is_some() => DEFEND_RANGE,
        Species::Bear if animal.guard_for > 0.0 => GUARD_RANGE,
        species => species.provoke_range(),
    }
}

/// How far a cow elk will let anybody come at her calf, in blocks.
const DEFEND_RANGE: f32 = 8.0;

/// How near a bear at its food lets anybody come, in blocks: twice its
/// temper on the move.
const GUARD_RANGE: f32 = 9.0;

/// How long a bear keeps guarding the food it ate at, in seconds.
const GUARD_SECONDS: f32 = 25.0;

/// What a fox's jaws do to what it catches: a grouse, a duck or a rat in
/// one bite, a hare in two. `Species::damage` is nought for the fox, because
/// it has no blow for a person, and that figure is the one `bite` uses for a
/// wolf's. Taken off the prey's health as it stands (`settle_bites`) -- one
/// bite has to be the whole of a bird, because a grouse that survives the
/// first is in the air before the second.
pub(super) const FOX_BITE: f32 = 16.0;

// ---- the whole of it, asked first ----

/// **What the woods' animals think before anything else is asked.**
///
/// `true` when it took the thought and `think` goes no further. Called once
/// per thought (not per tick), after the shared timers and before the
/// shore, the senses or the grazing.
#[allow(clippy::too_many_arguments)]
pub(super) fn think_first(
    animal: &mut Animal,
    figures: &[Figure],
    seen: &Neighbours,
    world: &dyn BlockWorld,
    rng: &mut Rng,
    time_of_day: f32,
    wild: &mut Wild<'_>,
) -> bool {
    match animal.species {
        Species::Bear => hibernate(animal, figures, wild) || forage(animal, figures, world, rng, wild),
        Species::Fox => fox(animal, figures, seen, world, rng, time_of_day, wild),
        Species::Beaver => beaver(animal, figures, world, rng, wild),
        Species::Duck => duck(animal, figures, rng),
        Species::Frog => frog(animal, figures, seen, world, rng),
        Species::Elk => {
            // **Dawn and dusk at the water.** An elk goes to stand in the bog
            // or the lake at the two ends of the day, and nothing else about
            // it is as reliable: the place to wait for one is the water's
            // edge at first light, with the wind in your face. Through the
            // thirst the herd already has, so it walks there by the same
            // rule a deer does and stands there by the same `Mind::Drink`.
            let t = time_of_day.rem_euclid(1.0);
            let twilight = (0.2..0.32).contains(&t) || (0.68..0.8).contains(&t);
            if twilight && animal.thirst < THIRSTY_AT && animal.wary_for <= 0.0 && rng.chance(0.3) {
                animal.thirst = THIRSTY_AT;
            }
            false
        }
        _ => false,
    }
}

// ---- the bear's year ----

/// How long a bear woken in its den stays half asleep, in seconds, and how
/// fast it moves meanwhile.
///
/// **The mercy in waking one**: three and a half seconds of an animal that
/// can barely stand, which is how long a player has to be somewhere else --
/// and then the angriest thing in the world (`wake`), with a grudge twice
/// its summer one. Whoever woke it had the three seconds to decide.
const GROGGY_SECONDS: f32 = 3.5;
const GROGGY_PACE: f32 = 0.3;

/// How far a sleeping bear hears, in blocks per unit of loudness
/// (`Gait::loudness`).
///
/// **A bear hears as far as the noise carries**, which is the whole rule:
/// creeping (0.3) is under a block and a half, so a player can crawl into a
/// den and out again; walking (1.0) is four and a half, so walking up to
/// look at it wakes it; running or working (2.0 and more) is nine and more,
/// so chopping a tree beside a den is how most people find one.
const WAKES_PER_LOUDNESS: f32 = 4.5;

/// **A bear asleep for the winter.** `true` while it sleeps.
///
/// It lies where the spawner put it -- under a roof of rock or roots
/// (`Animals::populate`) -- and does not move, feed or notice anything it
/// cannot hear. What wakes it is a blow (`struck`) or a person loud enough,
/// near enough (`WAKES_PER_LOUDNESS`), and the spring.
fn hibernate(animal: &mut Animal, figures: &[Figure], wild: &Wild<'_>) -> bool {
    if !animal.denned {
        return false;
    }
    if wild.season != Season::Winter {
        // The spring: it gets up of its own accord, hungry.
        animal.denned = false;
        animal.attitude = Attitude::Easy;
        return false;
    }
    for figure in figures {
        let distance = (figure.at.0 - animal.at().0).hypot(figure.at.2 - animal.at().2);
        if distance <= figure.loudness * WAKES_PER_LOUDNESS {
            wake(animal, figure.at, GROGGY_SECONDS);
            return true;
        }
    }
    animal.mind = Mind::Idle;
    animal.attitude = Attitude::Resting;
    animal.next_thought = 1.0;
    true
}

/// A denned bear woken: slow for `groggy` seconds, and then furious.
pub(super) fn wake(animal: &mut Animal, by: (f32, f32, f32), groggy: f32) {
    animal.denned = false;
    animal.groggy_for = groggy;
    animal.angry_for = animal.species.grudge_seconds() * 2.0;
    animal.threat_at = Some((by.0, by.2));
    animal.wants_yaw = (by.2 - animal.at().2).atan2(by.0 - animal.at().0);
    animal.mind = Mind::Recover;
    animal.attitude = Attitude::Startled;
    animal.next_thought = groggy;
}

/// What a blow does to an animal that this module keeps a state for: a
/// denned bear wakes (quicker than one that was only heard -- it has been
/// hurt), and a fox lets go of what it was carrying. Called from
/// `Animals::strike` before the blow is settled; what the fox dropped comes
/// back for the caller to put on the ground.
pub(super) fn struck(animal: &mut Animal, from: (f32, f32, f32)) -> Option<Carried> {
    if animal.denned {
        wake(animal, from, GROGGY_SECONDS * 0.4);
    }
    animal.carrying.take()
}

/// How far, in blocks, a bear in autumn looks for berries and wild hives.
const FORAGE_REACH: i32 = 9;

/// How often a bear looks for food, in seconds: the scan is a disc of
/// columns, and a bear ambling between two bushes has no need to repeat it
/// every thought.
const FORAGE_SCAN_SECONDS: f32 = 8.0;

/// How often a feeding bear takes a mouthful, in seconds: a berry bush is
/// stripped in one, a full hive in three.
const FORAGE_BITE_SECONDS: f32 = 6.0;

/// **A bear in autumn goes to the berries and the hives, and minds them.**
///
/// What an autumn wood is, and why a player's own honey is not safe in one:
/// a bear that has found a berry patch or a wild hive eats it bare
/// (`Edit::Forage`) and stands its ground over it (`guard_for`, and
/// `GUARD_RANGE` in `provoke_range`). So the berry bushes a player picked all
/// summer are a place to look before walking into in September, and a hive
/// with a bear at it is a hive that can wait.
///
/// The target is kept in `bound_for` -- a bird's field, meaning the same
/// thing, "where it is going" -- and looked for once every
/// `FORAGE_SCAN_SECONDS` (`next_home_scan`, another bird's field for its own
/// scan clock).
fn forage(animal: &mut Animal, figures: &[Figure], world: &dyn BlockWorld, rng: &mut Rng, wild: &mut Wild<'_>) -> bool {
    // **Not "nobody in its head"**: a bear smells a person across a whole
    // clearing (`Species::nose`) and keeps them as its `target` while it
    // watches, and a bear that would not eat while anybody was downwind of it
    // never ate. What stops the meal is a grudge, or somebody inside the guard
    // (below).
    if wild.season != Season::Autumn || animal.angry_for > 0.0 {
        return false;
    }
    // **A person inside the guard comes first**, whether or not it is already
    // eating: the shared mind decides about them (`think_hunter`), and a bear
    // minding its food is quicker to come at them there (`provoke_range`).
    // Asked only of the guard, so it could not be a bear that goes on eating
    // with somebody standing at the hive -- which is what this said when it
    // let a feeding bear keep the thought.
    if figures.iter().any(|f| (f.at.0 - animal.at().0).hypot(f.at.2 - animal.at().2) <= GUARD_RANGE) {
        return false;
    }
    if animal.bound_for.is_none() && animal.next_home_scan <= 0.0 {
        animal.next_home_scan = FORAGE_SCAN_SECONDS;
        animal.bound_for = autumn_food(world, animal.at());
    }
    let Some((fx, fz)) = animal.bound_for else {
        return false;
    };
    let (dx, dz) = (fx - animal.at().0, fz - animal.at().2);
    if dx.hypot(dz) > FEEDING_REACH + 0.6 {
        animal.mind = Mind::Wander;
        animal.wants_yaw = open_heading(world, animal, dz.atan2(dx));
        animal.next_thought = rng.range(0.5, 1.0);
        return true;
    }
    // At the food: head down in it, and minding it.
    animal.mind = Mind::Idle;
    animal.attitude = Attitude::Feeding;
    animal.wants_yaw = dz.atan2(dx);
    animal.guard_for = GUARD_SECONDS;
    animal.next_thought = rng.range(1.0, 2.0);
    if animal.dam_for <= 0.0 {
        animal.dam_for = FORAGE_BITE_SECONDS;
        let cell = (fx.floor() as i32, animal.at().1.floor() as i32, fz.floor() as i32);
        match food_cell(world, cell) {
            Some(cell) => wild.edits.push(Edit::Forage(cell)),
            // Eaten bare (or picked while it walked over): look again.
            None => animal.bound_for = None,
        }
    }
    true
}

/// Is this block a bear's autumn food: a bush with berries on it, or a wild
/// hive with honey in it?
pub(super) fn is_autumn_food(block: BlockId) -> bool {
    use primitive_shared::types::{block_kind, BLOCK_BERRY_BUSH, BLOCK_WILD_HIVE};
    match block_kind(block) {
        BLOCK_BERRY_BUSH => true,
        BLOCK_WILD_HIVE => primitive_shared::bees::honey_in(block) > 0,
        _ => false,
    }
}

/// The food cell at or beside `near`, three high: a hive hangs on a trunk
/// and a bush stands a block up a bank.
fn food_cell(world: &dyn BlockWorld, near: (i32, i32, i32)) -> Option<(i32, i32, i32)> {
    for dy in -1..=2 {
        for (dx, dz) in [(0, 0), (1, 0), (-1, 0), (0, 1), (0, -1)] {
            let cell = (near.0 + dx, near.1 + dy, near.2 + dz);
            if world.block(cell.0, cell.1, cell.2).is_some_and(is_autumn_food) {
                return Some(cell);
            }
        }
    }
    None
}

/// The nearest berry bush or honey hive within `FORAGE_REACH`, as a point on
/// the ground.
fn autumn_food(world: &dyn BlockWorld, at: (f32, f32, f32)) -> Option<(f32, f32)> {
    let (cx, cy, cz) = (at.0.floor() as i32, at.1.floor() as i32, at.2.floor() as i32);
    let mut best: Option<((f32, f32), i32)> = None;
    for dz in -FORAGE_REACH..=FORAGE_REACH {
        for dx in -FORAGE_REACH..=FORAGE_REACH {
            let d2 = dx * dx + dz * dz;
            if d2 > FORAGE_REACH * FORAGE_REACH || best.is_some_and(|(_, b)| b <= d2) {
                continue;
            }
            if (-1..=3).any(|dy| world.block(cx + dx, cy + dy, cz + dz).is_some_and(is_autumn_food)) {
                best = Some((((cx + dx) as f32 + 0.5, (cz + dz) as f32 + 0.5), d2));
            }
        }
    }
    best.map(|(at, _)| at)
}

// ---- the fox ----

/// How long a fox carries a kill before it has eaten it where it was going,
/// in seconds.
const CARRY_SECONDS: f32 = 60.0;

/// How far a fox smells meat left lying, in blocks.
const MEAT_SMELLED: f32 = 18.0;

/// How near a fox has to be to take a piece of meat, in blocks.
const JAWS_REACH: f32 = 1.2;

/// How near a person has to be to a piece of meat for a fox to leave it, in
/// blocks: by day, and at night. **The dark is half the fox's nerve**: a
/// haunch left by the fire at dusk is a haunch a fox walks up to while the
/// cook is ten paces off with their back turned.
const MEAT_GUARDED: (f32, f32) = (12.0, 5.0);

/// **The fox**: carry the kill away, take the meat left out, eat into the
/// carcass nobody came back for, and hunt what is small. See `Species::Fox`
/// for what each of those asks of a player.
///
/// Running from people is not here: a fox runs from a person by the same
/// senses and the same bolt a hare does, which is `think`'s, and this
/// returns `false` whenever there is nothing of its own to do.
#[allow(clippy::too_many_arguments)]
fn fox(
    animal: &mut Animal,
    figures: &[Figure],
    seen: &Neighbours,
    world: &dyn BlockWorld,
    rng: &mut Rng,
    time_of_day: f32,
    wild: &mut Wild<'_>,
) -> bool {
    let nearest_person = figures
        .iter()
        .map(|f| (f.at, (f.at.0 - animal.at().0).hypot(f.at.2 - animal.at().2)))
        .min_by(|a, b| a.1.total_cmp(&b.1));
    // **Where it last stood under the open sky**, kept in `home` -- which
    // nothing else of a fox's uses. Under a roof it is the way it came in:
    // see the carrying branch below.
    let covered = roofed(world, animal.at());
    if !covered {
        animal.home = Some((animal.at().0, animal.at().2));
    }
    // **Carrying: going home**, away from whoever is nearest and into cover,
    // at a trot. The kill is eaten where it was going; nothing of it comes
    // back unless somebody stops the fox on the way (`struck`).
    if animal.carrying.is_some() {
        if animal.fed_for < Species::Fox.fed_seconds() - CARRY_SECONDS {
            animal.carrying = None;
            return false;
        }
        let away = match nearest_person {
            Some((at, _)) => (animal.at().2 - at.2).atan2(animal.at().0 - at.0),
            None => animal.wants_yaw,
        };
        animal.mind = Mind::Wander;
        animal.attitude = Attitude::Easy;
        // **Out of a building the way it came in.** Animals here do not find
        // paths -- one that could find its way out of a pit is one nobody can
        // trap (the parent module's doc) -- and none of `way_out`'s sixteen
        // headings threads a one-cell door from the back of a hen house. What
        // a fox has instead is the last spot it stood on under the sky, which
        // for a coop is the step outside its door, and a line to that goes
        // through the door it came in by.
        animal.wants_yaw = match animal.home.filter(|_| covered) {
            Some((hx, hz)) => (hz - animal.at().2).atan2(hx - animal.at().0),
            None => way_out(world, animal, away),
        };
        animal.next_thought = rng.range(0.6, 1.2);
        return true;
    }
    if animal.fed_for > 0.0 {
        return false;
    }
    // **Something small in sight**: the wolf's chase, at a fox's size.
    if let Some((prey, prey_at)) = seen.quarry {
        animal.quarry = Some(prey);
        let (dx, dz) = (prey_at.0 - animal.at().0, prey_at.2 - animal.at().2);
        animal.mind = if dx.hypot(dz) <= animal.species.awareness() * CHASE_FRACTION {
            Mind::Chase
        } else {
            Mind::Wander
        };
        animal.attitude = Attitude::Stalking;
        animal.wants_yaw = dz.atan2(dx);
        animal.next_thought = rng.range(0.25, 0.5);
        return true;
    }
    animal.quarry = None;
    // **Meat left out**, with nobody near enough and no fire beside it.
    let guarded = if is_night(time_of_day) { MEAT_GUARDED.1 } else { MEAT_GUARDED.0 };
    let meat = wild
        .meat
        .iter()
        .copied()
        .filter(|&m| (m.0 - animal.at().0).hypot(m.2 - animal.at().2) <= MEAT_SMELLED)
        .filter(|&m| !figures.iter().any(|f| (f.at.0 - m.0).hypot(f.at.2 - m.2) <= guarded))
        .filter(|&m| lit_fire_near(world, m, FIRE_RADIUS).is_none())
        .min_by(|a, b| {
            let d = |m: &(f32, f32, f32)| (m.0 - animal.at().0).hypot(m.2 - animal.at().2);
            d(a).total_cmp(&d(b))
        });
    if let Some(m) = meat {
        let (dx, dz) = (m.0 - animal.at().0, m.2 - animal.at().2);
        animal.wants_yaw = dz.atan2(dx);
        if dx.hypot(dz) <= JAWS_REACH {
            wild.taken.push((animal.id, m));
            animal.mind = Mind::Idle;
            animal.next_thought = 0.3;
        } else {
            animal.mind = Mind::Wander;
            animal.attitude = Attitude::Stalking;
            animal.next_thought = rng.range(0.3, 0.6);
        }
        return true;
    }
    // **A carcass nobody came back for**: it eats into it, a cut at a time.
    if let Some(carrion) = carrion_near(world, animal.at(), CARRION_RADIUS) {
        let (dx, dz) = (carrion.0 - animal.at().0, carrion.2 - animal.at().2);
        animal.wants_yaw = dz.atan2(dx);
        if dx.hypot(dz) <= FEEDING_REACH {
            wild.edits.push(Edit::Gnaw((carrion.0.floor() as i32, carrion.1.floor() as i32, carrion.2.floor() as i32)));
            animal.fed_for = Species::Fox.fed_seconds() * 0.5;
            animal.mind = Mind::Idle;
            animal.attitude = Attitude::Feeding;
            animal.next_thought = rng.range(2.0, 4.0);
        } else {
            animal.mind = Mind::Wander;
            animal.wants_yaw = open_heading(world, animal, dz.atan2(dx));
            animal.next_thought = rng.range(0.4, 0.9);
        }
        return true;
    }
    false
}

/// **Which way a fox with a kill goes**: the heading with the most open
/// ground in front of it, with "away from whoever is watching" to settle the
/// ties.
///
/// Two ways were tried first and both left the fox standing in a hen house
/// with the bird in its jaws. *Straight away*, or the nearest cover that way,
/// is the back wall -- stone is cover (`is_cover`). *A flight's weighing*
/// (`escape_heading`) still pays most for away and treats a heading toward
/// the person as a cost, and the door of a coop is on the side the person is
/// watching from. What a fox leaving a coop actually does is go out the way
/// it came in, which is the one way with any room in it: so room counts
/// most, `WAY_OUT_REACH` blocks of it, and away is worth two.
fn way_out(world: &dyn BlockWorld, animal: &Animal, away: f32) -> f32 {
    const HEADINGS: usize = 16;
    let mut best = (away, f32::MIN);
    for i in 0..HEADINGS {
        let yaw = i as f32 / HEADINGS as f32 * std::f32::consts::TAU;
        let room = clear_run(world, animal, yaw, WAY_OUT_REACH);
        let score = room + 2.0 * (yaw - away).cos();
        if score > best.1 {
            best = (yaw, score);
        }
    }
    best.0
}

/// How far ahead `way_out` counts room, in blocks: past the door of a hen
/// house and a stride into the open.
const WAY_OUT_REACH: f32 = 8.0;

/// What a fox did when the tick loop took the meat it had its jaws on off
/// the ground: `Some` block if there was still something there.
pub(super) fn took(animal: &mut Animal, block: Option<BlockId>) {
    if let Some(block) = block {
        animal.carrying = Some(Carried::Meat(block));
        animal.fed_for = Species::Fox.fed_seconds();
    }
}

// ---- the beaver ----

/// How far from its stretch of water a beaver goes, in blocks.
const BEAVER_RANGE: f32 = 14.0;

/// How far a beaver hears moving water, in blocks.
///
/// **It is the sound that sends it**, and that is the animal as it really
/// is: a beaver piles sticks on the noise of water going somewhere, and the
/// only water going anywhere in this world is water somebody or something
/// has let out (`water`). So a drain cut from a pond, a trench led off a
/// river to a field, a lake let out through its bank -- anything that
/// *moves* within twenty blocks of a beaver -- gets a dam on it.
const RUNNING_WATER_HEARD: f32 = 20.0;

/// How near a beaver has to be to lay a stick, in blocks.
const DAM_REACH: f32 = 1.8;

/// How long a beaver takes over each stick, in seconds.
const DAM_SECONDS: f32 = 7.0;

/// How long a beaver stays under after it dives, in seconds: less than a
/// breath (`survival::BREATH_SECONDS`), and long enough that whoever made
/// it dive has walked on.
const DIVE_SECONDS: (f32, f32) = (6.0, 11.0);

/// **The beaver**: keep to its water, dive when anybody comes, and go to the
/// sound of moving water and stop it.
fn beaver(animal: &mut Animal, figures: &[Figure], world: &dyn BlockWorld, rng: &mut Rng, wild: &mut Wild<'_>) -> bool {
    // A person near: the shared mind's bolt, which dives if it is in the
    // water (`plunge`).
    if figures.iter().any(|f| (f.at.0 - animal.at().0).hypot(f.at.2 - animal.at().2) <= animal.species.awareness()) {
        return false;
    }
    // Moving water it can hear: the nearest cell of it, and a stick on it.
    let heard = wild
        .running
        .iter()
        .map(|&c| ((c.0 as f32 + 0.5, c.1 as f32, c.2 as f32 + 0.5), c))
        .filter(|(at, _)| (at.0 - animal.at().0).hypot(at.2 - animal.at().2) <= RUNNING_WATER_HEARD)
        .min_by(|a, b| {
            let d = |at: &(f32, f32, f32)| (at.0 - animal.at().0).hypot(at.2 - animal.at().2);
            d(&a.0).total_cmp(&d(&b.0))
        });
    if let Some((at, cell)) = heard {
        let (dx, dz) = (at.0 - animal.at().0, at.2 - animal.at().2);
        animal.wants_yaw = dz.atan2(dx);
        if dx.hypot(dz) <= DAM_REACH {
            animal.mind = Mind::Idle;
            // Working: head down at the water.
            animal.attitude = Attitude::Feeding;
            animal.next_thought = rng.range(1.0, 2.0);
            if animal.dam_for <= 0.0 {
                animal.dam_for = DAM_SECONDS;
                wild.edits.push(Edit::Dam(cell));
            }
        } else {
            animal.mind = Mind::Wander;
            animal.next_thought = rng.range(0.4, 0.8);
        }
        return true;
    }
    // Keep to its water.
    if let Some((hx, hz)) = animal.home {
        let (dx, dz) = (hx - animal.at().0, hz - animal.at().2);
        if dx.hypot(dz) > BEAVER_RANGE {
            animal.mind = Mind::Wander;
            animal.wants_yaw = open_heading(world, animal, dz.atan2(dx));
            animal.next_thought = rng.range(1.0, 2.0);
            return true;
        }
    }
    false
}

/// **Into the water, or under it**: the first step of a bolt for an animal
/// that dives. `true` when it took the bolt.
///
/// In the water already, it goes under (`under_for`), and `walk` sinks it
/// instead of floating it. On the bank, it makes for the water rather than
/// away from what frightened it -- which is the whole of why a beaver is
/// caught on the bank by somebody between it and the stream, and never by
/// somebody behind it.
pub(super) fn plunge(animal: &mut Animal, world: &dyn BlockWorld, away: f32) -> bool {
    if !animal.species.dives() {
        return false;
    }
    animal.mind = Mind::Flee;
    animal.next_thought = FLEE_SECONDS;
    if animal.afloat || in_liquid(world, animal.position, animal.species) {
        // How long this dive is, from the animal and where it went under:
        // `bolt` has no dice to hand, and two beavers diving off one bank
        // should not come up together.
        let salt = u64::from(animal.at().0.to_bits()) ^ u64::from(animal.at().2.to_bits()).rotate_left(21);
        animal.under_for = DIVE_SECONDS.0 + (DIVE_SECONDS.1 - DIVE_SECONDS.0) * spread(animal.id, salt);
        animal.wants_yaw = away;
        return true;
    }
    match water_near(world, animal) {
        Some((wx, wz)) => {
            animal.wants_yaw = (wz - animal.at().2).atan2(wx - animal.at().0);
            true
        }
        None => false,
    }
}

// ---- the duck ----

/// How far from its water a duck walks before it flies back instead, in
/// blocks.
const DUCK_WALKS: f32 = 10.0;

/// **The duck**: on its water, and back to it. The fright is the fowl's --
/// up and away (`bolt` for anything that `flies`) -- and what is here is
/// the coming back: a flock that went up off a pond comes down on the same
/// pond, which is what makes waiting in the reeds for it worth doing.
fn duck(animal: &mut Animal, figures: &[Figure], rng: &mut Rng) -> bool {
    if figures.iter().any(|f| (f.at.0 - animal.at().0).hypot(f.at.2 - animal.at().2) <= animal.species.awareness()) {
        return false;
    }
    if animal.afloat {
        // On the water: paddle about and dabble. The dabble is the feeding
        // attitude on the water (`attitude_now`), drawn tail-up.
        if rng.chance(0.45) {
            animal.mind = Mind::Idle;
            animal.attitude = Attitude::Feeding;
            animal.next_thought = rng.range(2.0, 4.5);
        } else {
            animal.mind = Mind::Wander;
            animal.attitude = Attitude::Easy;
            animal.wants_yaw += rng.range(-1.2, 1.2);
            animal.next_thought = rng.range(1.5, 3.5);
        }
        return true;
    }
    let Some(home) = animal.home else {
        return false;
    };
    let (dx, dz) = (home.0 - animal.at().0, home.1 - animal.at().2);
    let out = dx.hypot(dz);
    if out > DUCK_WALKS {
        // Too far to waddle: fly. The bird's own flight home, landing where
        // it lands -- on the water it was making for.
        animal.bound_for = Some(home);
        animal.flight_ceiling = f32::NEG_INFINITY;
        animal.mind = Mind::Homing;
        animal.wants_yaw = dz.atan2(dx);
        animal.next_thought = rng.range(0.4, 0.8);
        return true;
    }
    if out > 1.0 {
        animal.mind = Mind::Wander;
        animal.attitude = Attitude::Easy;
        animal.wants_yaw = dz.atan2(dx);
        animal.next_thought = rng.range(0.6, 1.2);
        return true;
    }
    false
}

// ---- the frog ----

/// How long a frog stays quiet after something moved near it, in seconds.
///
/// **Long enough to be heard as a silence**: a chorus that dropped out for
/// two seconds is a hiccup, one that is out for a quarter of a minute on
/// one side of the marsh is somebody over there.
const HUSH_SECONDS: f32 = 15.0;

/// How far a frog goes quiet round anything that means harm -- a wolf, a
/// boar, a bear, a fox -- in blocks. Twelve, the old chorus's number: wider
/// than any frog runs from, so the silence travels with whatever is moving
/// along the bank, and under a wolf's own awareness (eighteen), so a player
/// who hears the frogs stop has a few seconds before the wolf has them.
pub(super) const FROG_HUSHES_FOR: f32 = 12.0;

/// How far a frog hears a person, in blocks: four, and six more for every
/// unit of how loud they are (`Gait::loudness`). A creeping player is heard
/// at six, a walking one at ten, a running one at sixteen.
const HUSH_REACH: (f32, f32) = (4.0, 6.0);

/// **The frog**: goes quiet when anything comes near, and keeps to its water.
///
/// The quiet is `Attitude::Alert` held for `HUSH_SECONDS` -- the client's
/// chorus is sung only by frogs at ease (`soundscape::calls`) -- and the frog
/// is still sitting there while it is quiet: it is not afraid yet, it has
/// only stopped. Near enough to be a danger, the shared mind's bolt sends it
/// for the water (`plunge`).
fn frog(animal: &mut Animal, figures: &[Figure], seen: &Neighbours, world: &dyn BlockWorld, rng: &mut Rng) -> bool {
    let heard = figures
        .iter()
        .map(|f| (f.at, (f.at.0 - animal.at().0).hypot(f.at.2 - animal.at().2), f.loudness))
        .find(|&(_, d, loud)| d <= HUSH_REACH.0 + HUSH_REACH.1 * loud)
        .map(|(at, d, _)| (at, d))
        .or_else(|| seen.threat.map(|at| (at, (at.0 - animal.at().0).hypot(at.2 - animal.at().2))));
    if let Some((at, distance)) = heard {
        animal.wary_for = animal.wary_for.max(HUSH_SECONDS);
        animal.threat_at = Some((at.0, at.2));
        if distance <= animal.species.awareness() {
            // Near: the shared mind decides, and the bolt is for the water.
            return false;
        }
        animal.mind = Mind::Idle;
        animal.attitude = Attitude::Alert;
        animal.next_thought = rng.range(0.8, 1.6);
        return true;
    }
    if animal.wary_for > 0.0 {
        // Still quiet, still sitting.
        animal.mind = Mind::Idle;
        animal.attitude = Attitude::Alert;
        animal.next_thought = rng.range(1.0, 2.0);
        return true;
    }
    if let Some((hx, hz)) = animal.home {
        let (dx, dz) = (hx - animal.at().0, hz - animal.at().2);
        if dx.hypot(dz) > 5.0 {
            animal.mind = Mind::Wander;
            animal.wants_yaw = open_heading(world, animal, dz.atan2(dx));
            animal.next_thought = rng.range(0.8, 1.6);
            return true;
        }
    }
    // Sitting, singing: the frog's whole calm.
    animal.mind = if rng.chance(0.15) { Mind::Wander } else { Mind::Idle };
    animal.attitude = Attitude::Easy;
    animal.next_thought = rng.range(2.0, 5.0);
    true
}

// ---- where they come from ----

/// How many of the water's three a player has about them at most.
const BANK_ANIMALS_PER_PLAYER: usize = 4;

/// Does this species come from the bank spawner (`populate_banks`) rather
/// than the meadow's?
pub(super) fn from_the_bank(species: Species) -> bool {
    matches!(species, Species::Beaver | Species::Duck | Species::Frog)
}

/// Is it about in this season? The duck is gone south and the frog is in the
/// mud for the winter; the beaver is under the ice, which is to say gone as
/// far as a player can tell.
fn in_season(species: Species, season: Season) -> bool {
    !(from_the_bank(species) && season == Season::Winter)
}

impl Animals {
    /// Hands in where meat is lying on the ground, for the foxes.
    ///
    /// **Positions and nothing else**: the fox walks to one and the tick
    /// loop, which owns the items, takes what is really there when it
    /// arrives (`take_meat_taken`, `fox_took`) -- so a haunch a player
    /// picked up in the meantime is simply not there to be taken.
    pub fn meat_lying(&mut self, at: Vec<(f32, f32, f32)>) {
        self.meat_lying = at;
    }

    /// Hands in the cells of water the flow simulation just moved, for the
    /// beavers. Remembered for `RUNNING_MEMORY` seconds, because the flow
    /// runs slower than the tick and a stream that moved a moment ago is
    /// still moving as far as an ear is concerned.
    pub fn hear_running_water(&mut self, cells: impl IntoIterator<Item = (i32, i32, i32)>) {
        for cell in cells {
            if let Some(entry) = self.running_water.iter_mut().find(|(c, _)| *c == cell) {
                entry.1 = RUNNING_MEMORY;
            } else if self.running_water.len() < RUNNING_CELLS {
                self.running_water.push((cell, RUNNING_MEMORY));
            }
        }
    }

    /// Runs the memory of moving water down. Called once a tick from `step`.
    pub(super) fn forget_running_water(&mut self, dt: f32) {
        self.running_water.retain_mut(|(_, left)| {
            *left -= dt;
            *left > 0.0
        });
    }

    /// What the animals did to the world, for the tick loop to write.
    pub fn take_edits(&mut self) -> Vec<Edit> {
        std::mem::take(&mut self.edits)
    }

    /// The foxes that have their jaws on meat, and where it was.
    pub fn take_meat_taken(&mut self) -> Vec<(EntityId, (f32, f32, f32))> {
        std::mem::take(&mut self.meat_taken)
    }

    /// What the tick loop found when a fox took the meat: see `took`.
    pub fn fox_took(&mut self, fox: EntityId, block: Option<BlockId>) {
        if let Some(animal) = self.animals.iter_mut().find(|a| a.id == fox) {
            took(animal, block);
        }
    }

    /// **The water's three: beavers on a wooded stream, ducks on still
    /// water, frogs at a marsh's edge** -- put where they live, which is a
    /// bank, rather than on a meadow's turf by `populate`.
    ///
    /// Against a cap of their own (`BANK_ANIMALS_PER_PLAYER`) as well as the
    /// world's, so a lake's ducks do not stop a wood from having deer in it.
    /// Where to look is `populate_shore`'s ring of bearings round a player;
    /// what counts is a water surface in fresh-water country with a bank
    /// beside it, and for the beaver trees near it too.
    pub(super) fn populate_banks(&mut self, world: &dyn BlockWorld, players: &[(PlayerId, (f32, f32, f32))], night: bool) {
        let wild = |animals: &[Animal]| animals.iter().filter(|a| !is_kept(a)).count();
        let banked = |animals: &[Animal]| animals.iter().filter(|a| from_the_bank(a.species)).count();
        if players.is_empty() || wild(&self.animals) >= MAX_ANIMALS {
            return;
        }
        if banked(&self.animals) >= players.len() * BANK_ANIMALS_PER_PLAYER {
            return;
        }
        let Some(&(_, origin)) = self.rng.pick(players) else {
            return;
        };
        let angle = self.rng.range(0.0, std::f32::consts::TAU);
        let distance = self.rng.range(SPAWN_MIN * 0.6, SPAWN_MAX);
        let (x, z) = (origin.0 + angle.cos() * distance, origin.2 + angle.sin() * distance);
        let Some(surface) = water_surface_under(world, x, origin.1 + 16.0, z) else {
            return;
        };
        let biome = world.biome(x.floor() as i32, z.floor() as i32).unwrap_or(UNKNOWN_COUNTRY);
        let season = self.season;
        let choices: Vec<Species> = Species::ALL
            .iter()
            .copied()
            .filter(|&s| from_the_bank(s) && s.lives_in(biome) && in_season(s, season))
            .collect();
        let total: u32 = choices.iter().map(|s| s.spawn_weight(night)).sum();
        if total == 0 {
            return;
        }
        let mut roll = self.rng.range(0.0, total as f32);
        let Some(species) = choices.iter().copied().find(|s| {
            roll -= s.spawn_weight(night) as f32;
            roll <= 0.0
        }) else {
            return;
        };
        let bank = bank_beside(world, (x, surface as f32, z));
        if species == Species::Beaver {
            let Some(bank) = bank else {
                return;
            };
            if !wooded(world, bank.0, bank.1 as i32, bank.2) {
                return;
            }
        }
        let (low, high) = primitive_shared::animals::group_size(species);
        let wanted = low + self.rng.below(high - low + 1);
        for i in 0..wanted {
            if wild(&self.animals) >= MAX_ANIMALS {
                return;
            }
            let (ox, oz) = if i == 0 {
                (0.0, 0.0)
            } else {
                let bearing = self.rng.range(0.0, std::f32::consts::TAU);
                let spread = self.rng.range(1.0, 3.0);
                (bearing.cos() * spread, bearing.sin() * spread)
            };
            let at = match (species, bank) {
                // **On the water**, feet a little under the surface so the
                // body floats where the buoyancy leaves it.
                (Species::Duck, _) => {
                    let (dx, dz) = (x + ox, z + oz);
                    match water_surface_under(world, dx, surface as f32 + 1.0, dz) {
                        Some(y) => (dx, y as f32 + 0.6, dz),
                        None => continue,
                    }
                }
                // **On the bank**, beside the water it keeps to.
                (_, Some(bank)) => {
                    let (bx, bz) = (bank.0 + ox * 0.5, bank.2 + oz * 0.5);
                    match surface_under(world, bx, bank.1 + 2.0, bz) {
                        Some(y) => (bx, y as f32, bz),
                        None => continue,
                    }
                }
                _ => return,
            };
            if !fits(world, (f64::from(at.0), f64::from(at.1), f64::from(at.2)), species) {
                continue;
            }
            if self.spawn(species, at).is_some() {
                if let Some(animal) = self.animals.last_mut() {
                    animal.home = Some((x, z));
                }
            }
        }
    }
}

/// How long a cell of moving water is remembered, in seconds.
const RUNNING_MEMORY: f32 = 6.0;

/// The most cells of moving water remembered at once. A lake let out through
/// its bank moves hundreds of cells, and a beaver needs to hear one.
const RUNNING_CELLS: usize = 96;

/// A bank beside a water surface: the nearest column within three blocks
/// whose top is dry ground within a block of the water's own level.
fn bank_beside(world: &dyn BlockWorld, water: (f32, f32, f32)) -> Option<(f32, f32, f32)> {
    let level = water.1 as i32;
    for reach in 1..=3 {
        for (dx, dz) in [(1, 0), (-1, 0), (0, 1), (0, -1), (1, 1), (-1, -1), (1, -1), (-1, 1)] {
            let (x, z) = (water.0 + (dx * reach) as f32, water.2 + (dz * reach) as f32);
            let Some(ground) = surface_under(world, x, water.1 + 3.0, z) else {
                continue;
            };
            if (ground - 1 - level).abs() > 1 {
                continue;
            }
            let top = world.block(x.floor() as i32, ground - 1, z.floor() as i32);
            if top.is_some_and(|b| !is_liquid(b)) {
                return Some((x, ground as f32, z));
            }
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::logic::falling::tests::TestWorld;
    use primitive_shared::types::{BLOCK_AIR, BLOCK_STONE, BLOCK_WATER};

    const NOON: f32 = 0.5;

    /// A flat world of grass at y = 20, air above, out to `span` either side.
    fn meadow(span: i32) -> TestWorld {
        let world = TestWorld::default();
        for z in -span..=span {
            for x in -span..=span {
                world.put(x, 19, z, BLOCK_STONE);
                world.put(x, 20, z, BLOCK_GRASS);
                for y in 21..32 {
                    world.put(x, y, z, BLOCK_AIR);
                }
            }
        }
        world
    }

    /// The meadow with a pond two deep filling everything from `x = edge`
    /// eastward: its surface (y = 21) level with the ground beside it.
    fn meadow_with_a_pond(span: i32, edge: i32) -> TestWorld {
        let world = meadow(span);
        for z in -span..=span {
            for x in edge..=span {
                world.put(x, 20, z, BLOCK_WATER);
                world.put(x, 21, z, BLOCK_WATER);
            }
        }
        world
    }

    /// Somebody standing far off in a corner of the fixture: an animal with
    /// no player anywhere is forgotten (`forget_the_distant`), and one with a
    /// player too near is only ever running from them.
    fn far_off() -> Vec<(PlayerId, (f32, f32, f32))> {
        vec![(1, (-22.0, 21.0, -22.0))]
    }

    fn figure(at: (f32, f32, f32), loudness: f32) -> Figure {
        Figure { who: 1, at, loudness, visibility: 1.0, facing: None, wounded: false, asleep: false, held: None, reek: 0.0 }
    }

    /// The living animal `id`, to change or to hand to a function.
    fn the(animals: &mut Animals, id: EntityId) -> &mut Animal {
        animals.find_mut_for_test(id).expect("alive")
    }

    fn quiet() -> Neighbours {
        Neighbours {
            centre: None,
            company: 0,
            alarm: None,
            quarry: None,
            threat: None,
            packmate: None,
            leader: None,
            straggler: None,
        }
    }

    #[test]
    fn a_grazing_deer_freezes_at_a_distant_fright_and_then_does_not_freeze_again() {
        let mut animals = Animals::seeded(3);
        let id = animals.spawn(Species::Deer, (0.5, 21.0, 0.5)).expect("deer");
        let mut rng = Rng::seeded(3);
        let deer = animals.find_mut_for_test(id).expect("deer");
        deer.mind = Mind::Idle;
        assert!(freeze(deer, (10.5, 21.0, 0.5), 10.0, &mut rng), "a grazing deer bolted without a look");
        assert_eq!(deer.mind, Mind::Watch);
        assert_eq!(deer.attitude, Attitude::Startled, "the freeze does not show");
        assert!(deer.wants_yaw.abs() < 0.01, "it froze looking away from what it heard");
        // The next thought: still there, and this time it goes.
        deer.mind = Mind::Idle;
        assert!(!freeze(deer, (10.5, 21.0, 0.5), 10.0, &mut rng), "it froze twice at one fright");
        // ...and close up there is no freeze at all.
        let other = animals.spawn(Species::Deer, (0.5, 21.0, 4.5)).expect("deer");
        let near = animals.find_mut_for_test(other).expect("deer");
        near.mind = Mind::Idle;
        assert!(!freeze(near, (3.5, 21.0, 4.5), 3.0, &mut rng), "a deer froze with a person three strides off");
    }

    #[test]
    fn a_lying_deer_gets_up_before_it_can_run() {
        let world = meadow(20);
        let mut animals = Animals::seeded(4);
        let id = animals.spawn(Species::Deer, (0.5, 21.0, 0.5)).expect("deer");
        let start = {
            let deer = animals.find_mut_for_test(id).expect("deer");
            deer.attitude = Attitude::Resting;
            bolt(deer, &world, 0.0, &quiet());
            assert_eq!(deer.attitude, Attitude::Startled, "a lying deer ran without getting up");
            assert!(deer.rising_for > 0.0);
            deer.at()
        };
        // Half the getting up: not a step taken.
        for _ in 0..6 {
            walk(animals.find_mut_for_test(id).expect("deer"), &world, 0.05);
        }
        let risen = animals.find_mut_for_test(id).expect("deer").at();
        assert!((risen.0 - start.0).abs() < 0.05, "it moved {:.2} while still getting up", risen.0 - start.0);
    }

    #[test]
    fn a_cow_elk_with_her_calf_fights_and_one_without_does_not() {
        let mut animals = Animals::seeded(9);
        let cow = animals.spawn(Species::Elk, (0.5, 21.0, 0.5)).expect("cow");
        let calf = animals.spawn(Species::Elk, (2.5, 21.0, 0.5)).expect("calf");
        assert!(!animals.find(cow).expect("cow").fights(), "an elk on its own fights");
        animals.find_mut_for_test(cow).expect("cow").young = Some(calf);
        let mother = animals.find(cow).expect("cow");
        assert!(mother.fights(), "a cow with her calf does not fight");
        assert_eq!(provoke_range(mother), DEFEND_RANGE);
        assert!(Species::Elk.damage() > Species::Wolf.damage(), "an elk's kick is lighter than a wolf's bite");
    }

    #[test]
    fn a_denned_bear_sleeps_through_a_creeper_and_wakes_to_a_walker() {
        let mut animals = Animals::seeded(11);
        let id = animals.spawn(Species::Bear, (0.5, 21.0, 0.5)).expect("bear");
        let (mut edits, mut taken) = (Vec::new(), Vec::new());
        let mut wild = Wild { season: Season::Winter, meat: &[], running: &[], edits: &mut edits, taken: &mut taken };
        let bear = animals.find_mut_for_test(id).expect("bear");
        bear.denned = true;
        assert!(!bear.fights(), "a sleeping bear fights");
        // Creeping past two blocks off: nothing.
        assert!(hibernate(bear, &[figure((2.5, 21.0, 0.5), 0.3)], &wild));
        assert!(bear.denned, "a creeper at arm's length woke a bear");
        assert_eq!(bear.attitude, Attitude::Resting, "a sleeping bear is not lying down");
        // Walking up to look at it from four blocks: awake, slow, then angry.
        hibernate(bear, &[figure((4.5, 21.0, 0.5), 1.0)], &wild);
        assert!(!bear.denned, "a walker four blocks off did not wake it");
        assert!(bear.groggy_for > 0.0 && pace(bear) < 0.5, "a woken bear is on its feet at once");
        assert!(bear.angry_for > Species::Bear.grudge_seconds(), "a woken bear is no angrier than a struck one");
        // And the spring gets it up on its own.
        bear.denned = true;
        wild.season = Season::Spring;
        assert!(!hibernate(bear, &[], &wild));
        assert!(!bear.denned, "a bear slept into the spring");
    }

    #[test]
    fn a_bear_in_autumn_eats_at_a_berry_bush_and_minds_it() {
        use primitive_shared::types::BLOCK_BERRY_BUSH;
        let world = meadow(20);
        world.put(3, 21, 0, BLOCK_BERRY_BUSH);
        let mut animals = Animals::seeded(12);
        let id = animals.spawn(Species::Bear, (0.5, 21.0, 0.5)).expect("bear");
        let (mut edits, mut taken) = (Vec::new(), Vec::new());
        let mut wild = Wild { season: Season::Autumn, meat: &[], running: &[], edits: &mut edits, taken: &mut taken };
        let mut rng = Rng::seeded(12);
        let bear = animals.find_mut_for_test(id).expect("bear");
        // The scan clock is `think`'s to run down; here it has.
        bear.next_home_scan = 0.0;
        let summer_temper = provoke_range(bear);
        for _ in 0..40 {
            forage(bear, &[], &world, &mut rng, &mut wild);
            walk(bear, &world, 0.1);
            tick(bear, 0.1);
            if !wild.edits.is_empty() {
                break;
            }
        }
        assert!(wild.edits.contains(&Edit::Forage((3, 21, 0))), "the bear never ate at the bush: {:?}", wild.edits);
        assert_eq!(bear.attitude, Attitude::Feeding);
        assert!(provoke_range(bear) > summer_temper, "a bear at its food is no quicker to anger than one walking by");
        // In summer it walks past.
        let other = animals.spawn(Species::Bear, (0.5, 21.0, 4.5)).expect("bear");
        wild.season = Season::Summer;
        assert!(!forage(animals.find_mut_for_test(other).expect("bear"), &[], &world, &mut rng, &mut wild));
    }

    #[test]
    fn a_frog_goes_quiet_when_somebody_walks_up_and_sings_again_when_they_have_gone() {
        let world = meadow(20);
        let mut animals = Animals::seeded(13);
        let id = animals.spawn(Species::Frog, (0.5, 21.0, 0.5)).expect("frog");
        let mut rng = Rng::seeded(13);
        let singing = frog(the(&mut animals, id), &[], &quiet(), &world, &mut rng);
        assert!(singing);
        assert_eq!(the(&mut animals, id).attitude, Attitude::Easy, "a frog alone is not singing");
        // A walker nine blocks off: quiet, and still sitting.
        frog(the(&mut animals, id), &[figure((9.5, 21.0, 0.5), 1.0)], &quiet(), &world, &mut rng);
        assert_eq!(the(&mut animals, id).attitude, Attitude::Alert, "the chorus did not stop for a walker");
        assert_eq!(the(&mut animals, id).mind, Mind::Idle, "a hushed frog ran from somebody nine blocks off");
        // ...a creeper at nine is not heard...
        let other = animals.spawn(Species::Frog, (0.5, 21.0, 4.5)).expect("frog");
        let creeper = figure((9.5, 21.0, 4.5), 0.3);
        frog(animals.find_mut_for_test(other).expect("frog"), &[creeper], &quiet(), &world, &mut rng);
        assert_eq!(animals.find(other).expect("frog").attitude, Attitude::Easy, "a frog heard a creeper at nine blocks");
        // ...and the quiet ends when the hush does. (`think` runs `wary_for`
        // down a tick at a time; here it has simply run out.)
        assert!(the(&mut animals, id).wary_for >= HUSH_SECONDS - 0.01, "the hush is shorter than a silence");
        the(&mut animals, id).wary_for = 0.0;
        frog(the(&mut animals, id), &[], &quiet(), &world, &mut rng);
        assert_eq!(the(&mut animals, id).attitude, Attitude::Easy, "the frog never sang again");
    }

    #[test]
    fn a_frog_goes_quiet_for_a_wolf_passing_the_marsh_that_would_never_eat_it() {
        let world = meadow_with_a_pond(30, 4);
        let mut animals = Animals::seeded(22);
        let frog_id = animals.spawn(Species::Frog, (2.5, 21.0, 0.5)).expect("frog");
        animals.spawn(Species::Wolf, (2.5, 21.0, 10.5)).expect("wolf");
        for _ in 0..40 {
            animals.step(&world, &far_off(), 0.05, 0.0);
        }
        let frog = animals.find(frog_id).expect("frog");
        let EntityKind::Animal { attitude, .. } = frog.state().kind else {
            panic!("a frog that is not an animal");
        };
        assert_eq!(attitude, Attitude::Alert, "the chorus sang on with a wolf ten blocks along the bank");
        assert!(!Species::Wolf.hunts(Species::Frog), "the wolf is a frog's hunter now, and this says nothing");
    }

    #[test]
    fn a_beaver_goes_to_moving_water_and_lays_a_stick_in_it() {
        let world = meadow_with_a_pond(24, 6);
        let mut animals = Animals::seeded(14);
        let id = animals.spawn(Species::Beaver, (0.5, 21.0, 0.5)).expect("beaver");
        let moving = (8, 21, 0);
        animals.hear_running_water([moving]);
        let mut laid = Vec::new();
        for _ in 0..600 {
            animals.step(&world, &far_off(), 0.05, NOON);
            laid.extend(animals.take_edits());
            if !laid.is_empty() {
                break;
            }
            // The water keeps moving until something stops it.
            animals.hear_running_water([moving]);
        }
        assert_eq!(laid, [Edit::Dam(moving)], "the beaver did nothing about the moving water");
        let beaver = animals.find(id).expect("beaver");
        assert!(beaver.afloat || beaver.at().0 > 5.0, "the beaver built from the dry bank");
    }

    #[test]
    fn a_beaver_on_the_water_dives_when_frightened_and_on_the_bank_runs_for_the_water() {
        let world = meadow_with_a_pond(24, 6);
        let mut animals = Animals::seeded(15);
        let swimming = animals.spawn(Species::Beaver, (9.5, 20.5, 0.5)).expect("beaver");
        for _ in 0..20 {
            animals.step(&world, &far_off(), 0.05, NOON);
        }
        let beaver = animals.find_mut_for_test(swimming).expect("beaver");
        assert!(beaver.afloat, "a beaver in the pond is not swimming");
        assert!(plunge(beaver, &world, 0.0));
        assert!(beaver.under_for > 0.0, "a frightened beaver in the water stayed on top");
        let surface = beaver.at().1;
        for _ in 0..30 {
            walk(animals.find_mut_for_test(swimming).expect("beaver"), &world, 0.05);
        }
        assert!(animals.find(swimming).expect("beaver").at().1 < surface - 0.3, "the dive did not go down");
        // On the bank, the bolt is toward the water, not away from the fright.
        let banked = animals.spawn(Species::Beaver, (3.5, 21.0, 0.5)).expect("beaver");
        let beaver = animals.find_mut_for_test(banked).expect("beaver");
        // Frightened from the water side: away would be west, the water is east.
        assert!(plunge(beaver, &world, std::f32::consts::PI));
        assert!(beaver.wants_yaw.cos() > 0.5, "a beaver on the bank ran away from the water");
    }

    #[test]
    fn a_duck_on_its_pond_floats_on_it_and_dabbles() {
        let world = meadow_with_a_pond(24, 4);
        let mut animals = Animals::seeded(16);
        let id = animals.spawn(Species::Duck, (10.5, 21.6, 0.5)).expect("duck");
        animals.find_mut_for_test(id).expect("duck").home = Some((10.5, 0.5));
        let mut dabbled = false;
        for _ in 0..800 {
            animals.step(&world, &far_off(), 0.05, NOON);
            let duck = animals.find(id).expect("duck");
            if let EntityKind::Animal { attitude, .. } = duck.state().kind {
                dabbled |= attitude == Attitude::Feeding && duck.afloat;
            }
        }
        let duck = animals.find(id).expect("duck");
        assert!(duck.afloat, "the duck is not on the water");
        assert!(duck.at().0 > 4.0, "the duck walked out of its pond");
        let middle = duck.at().1 + Species::Duck.height() * 0.5;
        assert!((20.8..22.4).contains(&middle), "the duck floats at {middle:.2}, not at the surface");
        assert!(dabbled, "the duck never put its head under");
    }

    #[test]
    fn a_fox_that_kills_a_grouse_carries_it_off_and_drops_it_when_struck() {
        use primitive_shared::horse::TACK_CARRYING;
        let world = meadow(24);
        let mut animals = Animals::seeded(17);
        let grouse = animals.spawn(Species::Fowl, (1.6, 21.0, 0.5)).expect("grouse");
        let fox = animals.spawn(Species::Fox, (0.5, 21.0, 0.5)).expect("fox");
        {
            // The fox thinking now and the grouse not yet: the bite is settled
            // on the tick the fox first looks, before the grouse has seen it.
            let fox = animals.find_mut_for_test(fox).expect("fox");
            fox.yaw = 0.0;
            fox.quarry = Some(grouse);
            fox.next_thought = 0.0;
            fox.fed_for = 0.0;
            animals.find_mut_for_test(grouse).expect("grouse").next_thought = 5.0;
        }
        for _ in 0..20 {
            animals.step(&world, &far_off(), 0.05, NOON);
            if animals.find(grouse).is_none() {
                break;
            }
        }
        assert!(animals.find(grouse).is_none(), "the fox never caught the grouse beside it");
        let carrying = animals.find(fox).expect("fox").carrying;
        assert_eq!(carrying, Some(Carried::Kill(Species::Fowl)), "the fox ate the grouse where it fell");
        let EntityKind::Animal { tack, .. } = animals.find(fox).expect("fox").state().kind else {
            panic!("a fox that is not an animal");
        };
        assert!(tack & TACK_CARRYING != 0, "nobody watching can see the grouse in its jaws");
        // Hit on the way: the grouse goes down where the fox was.
        let at = animals.find(fox).expect("fox").at();
        let _ = animals.take_fallen();
        animals.strike(fox, (at.0 - 1.0, at.1 + 1.0, at.2), 3.0, 0.5);
        let dropped: Vec<Species> = animals.take_fallen().into_iter().map(|d| d.species).collect();
        assert_eq!(dropped, [Species::Fowl], "the fox kept the grouse when it was hit");
        assert_eq!(animals.find(fox).expect("fox").carrying, None);
    }

    #[test]
    fn a_fox_takes_meat_left_out_and_leaves_meat_by_a_fire_or_under_a_nose() {
        let world = meadow(24);
        let mut animals = Animals::seeded(18);
        let id = animals.spawn(Species::Fox, (0.5, 21.0, 0.5)).expect("fox");
        let mut rng = Rng::seeded(18);
        let meat = [(1.0, 21.1, 0.5)];
        let (mut edits, mut taken) = (Vec::new(), Vec::new());
        let mut wild = Wild { season: Season::Summer, meat: &meat, running: &[], edits: &mut edits, taken: &mut taken };
        // Somebody standing beside it by day: left alone.
        fox(the(&mut animals, id), &[figure((3.0, 21.0, 0.5), 0.0)], &quiet(), &world, &mut rng, NOON, &mut wild);
        assert!(wild.taken.is_empty(), "a fox took meat from under a person's nose at noon");
        // Nobody near: taken.
        fox(the(&mut animals, id), &[], &quiet(), &world, &mut rng, NOON, &mut wild);
        assert_eq!(wild.taken.as_slice(), [(id, meat[0])], "a fox left meat nobody was minding");
        // The tick loop found it there: the fox is carrying it.
        animals.fox_took(id, Some(primitive_shared::types::BLOCK_RAW_MEAT));
        assert!(matches!(animals.find(id).expect("fox").carrying, Some(Carried::Meat(_))));
    }

    #[test]
    fn a_winter_bear_is_found_only_asleep_under_a_roof() {
        let world = meadow(80);
        let mut animals = Animals::seeded(19);
        animals.season = Season::Winter;
        // Only bears: every other species' weight is nothing, which is what a
        // spawner asked for a bear and nothing else would see. Done by hand:
        // the meadow has no roof, so a bear drawn here is not put down.
        for _ in 0..400 {
            animals.populate(&world, &[(1, (0.5, 21.0, 0.5))], false);
        }
        assert!(
            animals.animals.iter().all(|a| a.species != Species::Bear || a.denned),
            "a bear walking about in the winter"
        );
        assert!(
            !animals.animals.iter().any(|a| a.species == Species::Bear),
            "a bear denned in an open meadow"
        );
    }

    #[test]
    fn a_resting_herd_lies_down_and_a_bird_never_does() {
        let mut rng = Rng::seeded(7);
        let lying = (0..400).filter(|_| rest(Species::Deer, &mut rng) == Attitude::Resting).count();
        assert!((160..320).contains(&lying), "a deer lay down {lying} times in 400 rests");
        for species in [Species::Fowl, Species::Duck, Species::Gull, Species::Frog, Species::Crab] {
            assert!((0..50).all(|_| rest(species, &mut rng) == Attitude::Dozing), "a {} lay down", species.name());
        }
    }

    #[test]
    fn a_bear_hears_a_creeper_at_arms_length_and_a_walker_across_the_den() {
        // The rule `WAKES_PER_LOUDNESS` is written for, as distances.
        let creeping = 0.3 * WAKES_PER_LOUDNESS;
        let walking = 1.0 * WAKES_PER_LOUDNESS;
        assert!(creeping < 1.6, "a creeping player wakes a bear at {creeping} blocks");
        assert!(walking > 4.0, "a walking player can stand {walking} blocks from a sleeping bear");
    }

    #[test]
    fn only_the_water_three_come_from_the_bank_and_none_of_them_in_winter() {
        let banked: Vec<&str> = Species::ALL.iter().copied().filter(|&s| from_the_bank(s)).map(Species::name).collect();
        assert_eq!(banked, ["beaver", "duck", "frog"]);
        for species in Species::ALL.iter().copied().filter(|&s| from_the_bank(s)) {
            assert!(!in_season(species, Season::Winter), "a {} in winter", species.name());
            assert!(in_season(species, Season::Spring));
        }
    }
}
