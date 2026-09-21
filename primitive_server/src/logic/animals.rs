//! The animals: where they come from, what they do, and what is left
//! when they stop.
//!
//! ## Why they are here and not in a mob framework
//!
//! There are four of them and they do eight things: stand about, walk
//! somewhere, drift back to their own kind, feed where there is
//! something to eat, run away, and -- for the two that fight -- stop and
//! watch, run somebody down, and commit to a charge. A behaviour tree for
//! that would be more code than the behaviour, and every line of it would
//! be about the tree.
//!
//! What is here instead is the same shape the dropped items have, one
//! size up: a list of things with positions, a physics step that is
//! gravity and a box against the world, and a state per entity that is
//! an enum with four values. `items` is worth reading first -- this is
//! that file with a mind.
//!
//! ## The rules, and why each one is a rule
//!
//! * **They spawn near players and are forgotten far from them.** An
//!   animal nobody can see is a position update nobody needs. The cap is
//!   per player rather than per world, so a server with forty people on
//!   it has animals everywhere rather than a herd around whoever logged
//!   in first.
//! * **They spawn on grass, in the open, at any hour.** Not because
//!   night is safe -- it is where the animals are easiest to catch --
//!   but because a deer that appears in the tunnel you are digging is a
//!   deer that came from nowhere, and the whole of what makes an animal
//!   feel like part of a world is that it was already there. (This note
//!   used to say "in daylight" and the spawner never checked the clock;
//!   the note was wrong, not the code, and what the clock *does* reach
//!   is `graze`, where a settled herd keeps different hours from a
//!   grazing one.)
//! * **They look before they run.** A bolt is aimed down a heading the
//!   animal has actually probed -- see `open_heading` -- so a deer with
//!   its back to a cliff swerves along it instead of over it. What it
//!   is deliberately not is a pathfinder: an animal that could find its
//!   way out of a pit is an animal a player cannot trap.
//! * **They notice each other.** Same species only, and two facts
//!   apiece: where the rest of the herd is, and whether one of them has
//!   just bolted. The second is why a meadow empties rather than
//!   surrendering one deer at a time. See `Neighbours`.
//! * **A beaten one runs.** Below a third of its health an animal breaks
//!   off, grudge and all, which is what turns the end of a fight with a
//!   boar into a chase rather than an execution.
//! * **Something hunts something else.** A wolf hunts hares and deer,
//!   and they run from it exactly as they run from a person -- see
//!   `Species::hunts` and `Neighbours::quarry`. Until this existed every
//!   animal here knew about precisely one thing in the world, the
//!   player, so a wolf walked past a deer and a deer grazed beside a
//!   wolf. No amount of cleverness about fleeing *people* covers that
//!   up; it is the thing that makes the animals look like scenery with
//!   legs.
//! * **A kill leaves nothing and feeds the pack.** What the wolf gets is
//!   four minutes of not being hungry (`Species::fed_seconds`), which is
//!   also what stops a pack from clearing every deer in the spawn radius.
//!   See `Animals::settle_bites` for why there is no carcass.
//! * **A chase steers and a charge does not**, and both exist for that
//!   reason. See `Mind::Chase`.
//! * **They never dig, build or open anything.** An animal that
//!   can change the world is an animal that can wreck a player's house
//!   while they are asleep, and there is no version of that which is
//!   worth the deer. (This used to say "swim" as well, and nothing on four
//!   legs does; the two that swim are the next rule.)
//! * **A fish lives in the water and nowhere else.** `Species::swims` sends
//!   it down a second, smaller path: `think_fish` for the mind (drift with
//!   the school, bolt from a swimmer, dive), `swim` for the body (a depth
//!   it is making for instead of gravity, and no step that leaves liquid),
//!   `gasp` for the one hazard (air), and `populate_water` for where it
//!   comes from -- against its own cap, near a player, in water deep
//!   enough for its kind. Two paths rather than a `swims` test inside
//!   every rule of `think` and `walk`, because nearly every rule there is
//!   about ground: pasture, cover, cliffs, thirst, a bank to climb out
//!   over. A fish asked any of those questions gets a wrong answer, and
//!   the first one somebody forgets to guard is a fish that walks.
//! * **Only the boar hits back**, and it hits the player rather than the
//!   world. See `Species::damage`.
//! * **Prey runs for cover, and stops once it is out of sight.** A bolt
//!   is aimed at the nearest wood rather than straight away from you,
//!   and a deer that has been out of your line of sight for a few
//!   seconds gives up running and grazes where it is -- see
//!   `cover_heading` and `sees`. What was there before was a deer that
//!   ran in a straight line across open ground until you caught it,
//!   which is a chase with one correct answer (sprint). Now the wood is
//!   where you lose it and the meadow's edge is where you wait for it.
//! * **They remember where they were hurt.** A place an animal or its
//!   herd was struck is avoided for a few minutes, and a herd struck
//!   twice in the same place moves its range -- see `Animal::dangers`.
//!   A hunter who camps one meadow empties it; moving on is the
//!   decision.
//! * **A pack comes from two sides, and not past a fire.** The second
//!   wolf circles to the side opposite the first before it commits,
//!   nothing in a pack closes on a lit fire after dark, and a wolf below
//!   half its health backs off to twelve blocks and *shadows* rather
//!   than leaving. See `flank`, `lit_fire_near` and `shadow`.
//! * **The night is worse than the day.** A fire is seen from far off after
//!   dark (`FIRE_SEEN_AT_NIGHT`) and a hunter that sees one comes to the edge
//!   of its light and circles there (`FIRE_EDGE`); a sleeper in the dark is
//!   the one person a lone wolf will come at (`think_hunter`'s `sleeper`);
//!   and the night that is skipped while everybody sleeps is asked whether
//!   it would have found them (`Animals::find_the_sleeper`).
//! * **They get thirsty, and water is a place rather than a wall.** An
//!   animal with nothing else to do and a couple of minutes' thirst on
//!   it walks to the nearest open water, stands on the bank and drinks
//!   -- and getting chased makes it thirsty three times as fast. What
//!   that buys is a *reason to be somewhere*: the river is where the
//!   deer will be, so the hunter who fluffed the stalk in the meadow
//!   knows where to wait. Anything at all -- a person, a wolf, a fight
//!   -- outranks it, because it is reached from `graze` and `graze` is
//!   only called when nothing is happening. See `THIRSTY_AT`,
//!   `water_near` and `Mind::Drink`.
//! * **And an animal that is *in* the water heads for the shore.** The
//!   rule above it -- nothing walks into a lake voluntarily -- is what
//!   stops animals floating out of reach, and it said nothing at all
//!   about one that was pushed, spawned or chased in: it wandered on
//!   whatever heading it last had until it happened to drift against a
//!   bank. Now it looks for the nearest dry footing and climbs out over
//!   the bank, which needed both a decision (`shore_heading`) and the
//!   step-up in `walk` to stop insisting the animal be standing on
//!   something first. See the `wading` clause there.
//! * **Nothing sprints for ever.** Every animal has a wind
//!   (`Species::stamina_seconds`), spends it while it runs and gets it
//!   back while it stands; a blown one keeps going at `BLOWN_SPEED`
//!   rather than stopping. That is what turns a chase from a comparison
//!   of two constants -- which always had exactly one answer, decided
//!   before the chase began -- into a race with an end in it. The wolf's
//!   wind is *shorter* than the deer's on purpose, so a hunt is won by
//!   the stalk that got it close and not by arithmetic.
//! * **A bird lives somewhere.** Everything else here is wherever it
//!   wandered to; a bird keeps a nest -- one `worldgen` put in a canopy,
//!   or a crown it has adopted for want of one -- forages within sight
//!   of it, hops between perches round it, and flies back to it when a
//!   fright has carried it off. That is six states over the ones
//!   everything already has and one new one (`Mind::Homing`), and the
//!   only new world lookup is a search a bird pays for at most once every
//!   eight seconds and never again once it has a nest. See the table at
//!   `HOME_RANGE` and the cost at `NEST_SCAN_INTERVAL`.
//! * **A charge is a straight line.** Once a boar is at speed it can
//!   barely turn; it runs through where you were, and the second it
//!   spends turning round is the second its back is to you -- see
//!   `CHARGE_TURN_RATE` and `Animal::exposed_back`.
//! * **They hear, see and smell, and each sense is answered differently.**
//!   Hearing is distance and how loud you are being, through leaves and
//!   round corners; sight needs a line, a cone and light; scent needs the
//!   wind. A sound at the edge of hearing lifts a head before it sends
//!   anything running. See `perceive` and the section above `Gait`.
//! * **A flight looks where it is going and remembers what it ran from.**
//!   Sixteen headings weighed, not the first open one; and a deer that has
//!   lost you grazes warily, away from where you were. See `escape_heading`
//!   and `WARY_SECONDS`.
//! * **A herd has a leader and a pack has a ring.** Followers go with the
//!   oldest of them and keep a body apart; a pack that has found a person
//!   walks round them and comes when their back is turned; a bear keeps its
//!   ground and goes home past the edge of it. See `Neighbours::leader`,
//!   `STALK_RADIUS` and `TERRITORY_RADIUS`.
//! * **A bird banks.** The flight is still a target altitude and a heading,
//!   met through a turn, a flap and a climb that are rate-limited like a
//!   body's -- see the section above `AIR_TURN_ACCEL`.
//!
//! ## What crosses the wire
//!
//! A position, a facing and a species -- the same contract a dropped
//! item has, plus a hurt flash. Everything else is here. A client is
//! never told an animal's health, because a client that knows would draw
//! a health bar over every rabbit, and a world of health bars is a world
//! of interface rather than of animals.

use primitive_shared::animals::{
    blocks_sight, is_cover, Grouping, Species, DESPAWN_DISTANCE, MAX_ANIMALS, MAX_ANIMALS_PER_PLAYER, MAX_KEPT,
    MAX_FISH, MAX_FISH_PER_PLAYER, MAX_SEABIRDS_PER_PLAYER,
};
use primitive_shared::protocol::{
    entity_id, EntityId, EntityKind, EntitySource, EntityState, PlayerId,
};
use primitive_shared::notice::{
    Notice,
};
/// How high an animal stands on this cell.
///
/// **`collision_height`, plus foliage.** Leaves stopped being solid when
/// the player was allowed to push through a thicket
/// (`types::is_collidable`), and that is right for a player and wrong
/// for everything that lives in a tree: a bird came down to perch on the
/// canopy and fell through it to the ground, and a deer that made for
/// the trees walked into the middle of one.
///
/// So the two questions are separated here rather than in the shared
/// crate, because they really are two questions. What a *player* meets
/// is a bush to push through; what an *animal* meets is a branch. The
/// same block, honestly, is both.
///
/// **And a door is a wall while it is shut and a gap while it is open.**
/// An animal meets whole cells (`fits`), and a door is three sixteenths of
/// one: read as its row it would be a wall open or shut, and a wolf could
/// never follow a player through a door somebody forgot to close -- which
/// is the whole of what an open door should cost. Open, the cell is passed
/// through with the boards along its side; shut, the cell is a wall, the
/// sixteenths in front of the boards included. Nothing here opens one.
fn stand_height(id: primitive_shared::types::BlockId) -> f32 {
    if primitive_shared::types::is_leafy(id) {
        return 1.0;
    }
    if primitive_shared::types::door_is_open(id) {
        return 0.0;
    }
    primitive_shared::types::collision_height(id)
}

/// The boxes of a step at `cell`, shaped by its neighbours as the player's
/// collider shapes it (`geometry::step_boxes`), or `None` for anything
/// else -- which an animal still meets as a whole cell `stand_height` tall.
///
/// Only steps, and not every shape `geometry` knows: an animal meets whole
/// cells on purpose (see `stand_height`), and a step is the one partial
/// block an animal is *meant* to walk on -- a flight of them is a way up,
/// and read as a cube each one stood a body on its riser over a tread with
/// half a block of air under it.
fn step_boxes_at(
    world: &dyn BlockWorld,
    block: primitive_shared::types::BlockId,
    (x, y, z): (i32, i32, i32),
) -> Option<primitive_shared::geometry::StepBoxes> {
    primitive_shared::types::is_step(block).then(|| {
        primitive_shared::geometry::step_boxes(block, |dx, dy, dz| {
            world.block(x + dx, y + dy, z + dz).unwrap_or(primitive_shared::types::BLOCK_AIR)
        })
    })
}

use primitive_shared::types::{
    can_grow_on, is_burning, is_liquid, is_lit_torch, BLOCK_GRASS, CHUNK_SIZE_Y,
};

use crate::logic::falling::BlockWorld;
use primitive_shared::horse;
use primitive_shared::husbandry;
use primitive_shared::youth;
use crate::logic::rng::Rng;

const GRAVITY: f32 = -22.0;
const TERMINAL_VELOCITY: f32 = -30.0;

/// How high above the ground a flying animal holds itself, in blocks.
///
/// Three and a half: over a fence, over a hedge, under the canopy of an
/// ordinary tree. High enough that a bird in the air is plainly *in the
/// air* and low enough that it stays a thing in the world rather than a
/// dot in the sky -- and low enough to be speared, which is the whole
/// reason a player cares.
const FLIGHT_HEIGHT: f32 = 3.5;

/// How hard a flier is pulled toward that height, per block of error.
const FLIGHT_CLIMB: f32 = 3.0;

/// The fastest it climbs or drops, in blocks a second. A bird that
/// snapped to its altitude would read as a lift rather than as a bird.
const FLIGHT_SPEED: f32 = 7.0;

// ---- coming down, and going up ----
//
// **A landing is a descent, not a drop.** It used to be the spring above
// and nothing else: a gull flew in at nine blocks, `seabird` called it
// arrived the moment it was over the spot, and with no reason left to be up
// the spring pulled it straight down at `FLIGHT_SPEED` while its forward
// speed bled off in a third of a second. The client draws wings from
// *horizontal* speed (`animal_model::AIRBORNE_SPEED`), so what a player saw
// was a gull standing on nothing, wings folded, falling nine blocks onto the
// sand -- reported as "gulls just fall when they land". A grouse did the
// same from its three and a half.
//
// Three shapes were weighed:
//
// * **A landing state**, with its own physics. Rejected for the reason
//   `walk`'s altitude spring exists: a second flight mode is a second thing
//   to keep in step with the walking one, and every place that asks "is it
//   in the air" would have to learn it.
// * **Tell the client it is flying** and let it hold the wings open.
//   Rejected: it dresses the drop and keeps it. The bird still comes down
//   vertically nine blocks, which is what a falling thing does.
// * **Shape the spring**, which is what is here. The height a homing bird
//   holds falls with the distance left (`GLIDE_SLOPE`), so it is low by the
//   time it is over the spot; nothing comes down steeper than it is flying
//   across (`STEEPEST_GLIDE`); the last block is taken slowly
//   (`LANDING_SINK`); and arriving needs it low as well as near
//   (`ARRIVAL_HEIGHT`). Take-off is the same spring the other way round, at
//   `CALM_CLIMB`.

/// How much height a homing bird holds per block it still has to go: the
/// glide path, down to the spot. Six tenths is a gull coming in off nine
/// blocks over fifteen -- a line a player can follow to where it lands.
const GLIDE_SLOPE: f32 = 0.6;

/// The steepest a bird comes down, as blocks of height per block it flies
/// across, above `FLARE_HEIGHT`. One: never steeper than it is flying, which
/// is the line between a glide and a fall. Where the glide path asks for
/// steeper -- a spot chosen close under it -- the bird overshoots and comes
/// round lower, which is also what a bird does.
const STEEPEST_GLIDE: f32 = 1.0;

/// How far above what it is coming down on a bird starts to flare.
const FLARE_HEIGHT: f32 = 1.0;

/// The fastest a bird sinks through the last `FLARE_HEIGHT`, in blocks a
/// second: touching down, not arriving. It is also the floor under
/// `STEEPEST_GLIDE`, so a bird that has slowed to nothing still comes down.
const LANDING_SINK: f32 = 1.2;

/// How high over the spot a homing bird may be and still call itself
/// arrived. Near is not enough: a bird that is near and six blocks up has
/// not landed, it has flown over, and ending the flight there is the drop
/// this section replaced.
const ARRIVAL_HEIGHT: f32 = 2.5;

/// **A bird coming in to land slows as it comes**, down to this share of its
/// cruise over the spot itself. At a cruise it crossed the arrival disc
/// (`HOME_REACH` round the spot) in well under a second, too fast to sink the
/// last blocks, turned wider than the disc and came round again -- measured on
/// `a_flying_gull_never_falls_through_or_tunnels_into_terrain`, a gull that
/// spent 5218 of 6000 ticks "coming down" and never landed or fished once.
const APPROACH_SLOWEST: f32 = 0.3;

/// ...and a landing that still has not happened after this long is given up
/// for the sky. Whatever shape of shore makes a spot unreachable -- a ledge
/// under an overhang, a column the glide cannot meet -- a bird circling it
/// for ever is a bird that has stopped being a bird.
const LANDING_GIVE_UP_SECONDS: f32 = 20.0;

// ---- a wing, not a rotor ----
//
// **"Birds fly like FPV drones, and sometimes they just fall."** Both were
// the same fact: the flight was a target altitude and a heading, each met as
// fast as the numbers allowed. The heading turned at a walking animal's rate
// (two hundred degrees a second, more for something small and running); the
// vertical speed was *set* from a spring every tick, so it went from a full
// climb to a full sink between two ticks; a take-off rose at seven blocks a
// second with no speed across; a bird that met a branch stopped dead in the
// air; and a bird whose fright ended over a meadow came down at whatever its
// speed allowed -- up to seven a second along a straight line into the
// ground -- or, where the column under it could not be read, under gravity.
//
// Two shapes were weighed. **A proper flight model** -- lift, drag, a pitch
// -- is a second physics, and every rule in this file that asks where a bird
// is going would have to learn to fly it. **Rate limits on the one there
// is**, which is what is here: the same altitude and the same heading, met
// through a banked turn (`AIR_TURN_ACCEL`, `AIR_ROLL`), a flap
// (`WING_ACCEL`), a climb that needs speed (`CLIMB_PER_SPEED`), a vertical
// speed that changes at `LIFT_ACCEL`, and an airspeed that does not fall
// under `MIN_AIRSPEED` until the flare. `a_bird_in_flight_never_jerks_pivots_or_falls`
// samples a flight and holds all of it.

/// The sideways pull a banked wing can make, in blocks a second squared: a
/// turn's rate is this over the airspeed, so a slow bird turns tight and a
/// fast one sweeps. Nine is a grouse at its run turning at about a radian
/// and three quarters a second.
const AIR_TURN_ACCEL: f32 = 9.0;

/// The fastest a bird in the air turns at all, in radians a second, however
/// slow it is going: about a hundred and forty degrees.
const AIR_TURN_MOST: f32 = 2.5;

/// How hard a bird turns toward the heading it wants, per radian off it,
/// before `AIR_TURN_ACCEL` caps it: the difference between rolling out onto
/// a line and swinging past it.
const AIR_TURN_GAIN: f32 = 2.5;

/// How fast the rate of a turn itself can change, in radians a second
/// squared: banking into a turn and out of it. Without it a bird reversed a
/// turn between two ticks, which is exactly the flick that reads as a drone.
const AIR_ROLL: f32 = 6.0;

/// How fast a flapping bird gains or loses airspeed, in blocks a second
/// squared.
const WING_ACCEL: f32 = 6.0;

/// The slowest a bird flies while it is above the flare, in blocks a second.
const MIN_AIRSPEED: f32 = 2.0;

/// The hardest a bird on its feet accelerates into a take-off, in blocks a
/// second squared.
const TAKE_OFF_ACCEL: f32 = 10.0;

/// How fast a bird's climb or sink can change, in blocks a second squared --
/// and a gull's plunge, which is a fold of the wings and far harder.
const LIFT_ACCEL: f32 = 9.0;
const DIVE_ACCEL: f32 = 40.0;

/// The climb a wing makes with no speed across, and what each block a second
/// of airspeed adds to it. A grouse bursting off the ground at two, rising at
/// six once it is going.
const CLIMB_BASE: f32 = 2.0;
const CLIMB_PER_SPEED: f32 = 0.8;

/// How fast a bird climbs when nothing is after it, in blocks a second.
/// Under `FLIGHT_SPEED`, which a fright keeps: a flushed grouse bursts up
/// and that is right, but a gull going up of its own accord at seven blocks
/// a second leaves the sand like a thing on a rope.
const CALM_CLIMB: f32 = 3.0;

// ---- the air is not still, and a bird does not fly a ruler line ----
//
// **"Птицы летают как деревяшки."** Everything above had already stopped the
// bird flying like a drone; what was left was that it flew like a *plank*. A
// gull held nine blocks exactly, at one speed exactly, along a heading it was
// handed twice a second -- so a flock over a beach was a set of identical
// discs sliding round at one altitude, and a grouse crossing a clearing drew
// a ruler line from tree to tree. Nothing in it was wrong and none of it was
// alive.
//
// What a bird actually does is trade height for speed and back again, all
// day: it beats up a little, glides down a little, goes faster on the way
// down and slower on the way up, and lets the air do the rest where the air
// is going up. Three numbers here, all of them reading state that is already
// on the animal, and **no scan, no search and no extra block read** -- the
// slope comes out of the look-ahead column `flight_floor` was already taking.
//
// Rejected: **a wind field**, which is a second world to keep in step with
// the first and a thing a player cannot see the cause of. Rejected too:
// **randomising the altitude per thought**, which is a bird that teleports
// up and down every second rather than one that climbs and glides.

/// How long one climb-and-glide takes, in seconds.
///
/// Seven: slow enough to read as a bird working the air rather than as a
/// bobbing float, and long enough that two birds spread over it
/// (`Animal::air_phase`) are plainly not in step.
const AIR_CYCLE: f32 = 7.0;

/// How far above and below its cruise that cycle carries a bird, in blocks.
///
/// A block and a half. It is deliberately smaller than `AIR_CLIMB_REACH`, so
/// the cycle never argues with the look-ahead that keeps a bird off a canopy:
/// what clears a tree is still the floor, and this rides on top of it.
const CYCLE_LIFT: f32 = 1.5;

/// How much of the rise in the ground ahead a bird gets for nothing, as
/// height held and as blocks a second of extra climb.
///
/// **The slope's updraught, and it costs one subtraction.** `flight_floor`
/// already reads the column under the bird and the column
/// `AIR_LOOK_AHEAD` down its nose; the difference between them is how fast
/// the ground is coming up, and air pushed up a hillside is the one piece of
/// real aerodynamics a voxel world hands over free. What it buys is that a
/// bird crossing a ridge *rises before the ridge does* and tops it with room,
/// instead of scrabbling up the near face at the climb rate the spring
/// allows.
const SLOPE_LIFT: f32 = 0.5;
const SLOPE_CLIMB: f32 = 0.35;

/// How much airspeed a bird gains for each block a second it is sinking, and
/// loses for each block a second it is climbing.
///
/// **Height is speed.** A bird that climbed and sank at one airspeed was the
/// single loudest thing about the old flight: the wings said one thing and
/// the ground under them said another. Just under a half, so a gull dropping
/// at three is going a block and a third a second faster than one holding
/// its height, and one climbing at three is that much slower -- a difference
/// an eye reads at thirty blocks without being able to name it.
const SPEED_PER_SINK: f32 = 0.45;

/// How far a cruising bird wanders either side of the straight line to where
/// it is going, in radians, and how many of those wanders there are to a
/// cycle.
///
/// **A quarter of a radian is a bird, a straight line is a dart.** The weave
/// is faded out on the approach (see `walk`), because a bird that wandered
/// while it was landing would miss the perch -- and it is added to the
/// *wanted* heading rather than to the yaw, so the bank that follows it is
/// the same banked turn everything else uses (`AIR_TURN_ACCEL`) and the
/// obstacle steering still has the last word (`air_heading`).
const WEAVE: f32 = 0.26;
const WEAVES_PER_CYCLE: f32 = 2.5;

/// How far out a cruising bird has to still be for the weave to be at full
/// width, in blocks: inside this it fades to nothing and the bird flies the
/// line in.
const WEAVE_FADES_AT: f32 = 8.0;

// ---- the shore, and what a gull does over it ----
//
// **The fowl's body with a different life.** A grouse lives round a tree
// and flies when it must; a gull lives in the air over a stretch of coast
// and comes down when it chooses. Same collider, same altitude spring, same
// turning and the same fright -- and five states laid over them:
//
// | what it is doing | the state | what moves it on |
// |---|---|---|
// | on the sand, or sitting on the water | `Idle`/`Wander` | `TAKE_OFF_CHANCE`; somebody within `awareness` (`Flee`) |
// | circling over its shore | `Soar`, round `Animal::bound_for` | `LAND_CHANCE` finding a dry spot; nightfall |
// | fishing | `Soar`, with `Animal::dive_for` running | `DIVE_SECONDS` |
// | coming in to land | `Homing` at the spot | arriving: `Idle`, and the spring lets it down |
// | frightened | `Flee`, at `SOAR_HEIGHT` | the fright passing: back to `Soar` over home |
//
// `Animal::home` is the gull's stretch of shore rather than a nest, and it
// is kept for life for the nest's reason. What decides every rule below
// being the gull's alone is `Species::soars`.

/// How high a gull holds itself over the sea or the sand, in blocks.
///
/// **Nine: out of every reach this world has**, and that is the hunt. A
/// spear reaches six (`combat::SPEAR_REACH`), so a gull in the air is
/// scenery and a gull on the sand is dinner, and what a player does about it
/// is watch where the flock comes down. Higher and a flock is specks against
/// the sky nobody can count; lower and it is a grouse over water.
const SOAR_HEIGHT: f32 = 9.0;

/// The radius of the circle a soaring gull flies, in blocks.
///
/// Ten: wide enough that a flock over a beach is several birds sweeping
/// past rather than a ring spinning on the spot, and inside `GULL_RANGE`, so
/// a circle round the middle of its shore stays over its shore.
const SOAR_RADIUS: f32 = 10.0;

/// How far from its shore a gull ranges, in blocks: where it circles, lands
/// and fishes.
///
/// Twenty -- a beach's length -- and not the nest's twelve: a grouse forages
/// round one tree, a gull works a coast.
const GULL_RANGE: f32 = 20.0;

/// How likely a gull on the sand is to go up of its own accord, per
/// thought. By day only: a flock roosts through the dark.
///
/// About one thought in eight, so a gull stands about a quarter of a minute
/// on average -- long enough to be walked up on and counted, short enough
/// that a beach is always busy.
const TAKE_OFF_CHANCE: f32 = 0.12;

/// How likely a soaring gull is to come down, per thought, when there is a
/// dry spot for it to come down on.
const LAND_CHANCE: f32 = 0.18;

/// How likely a gull soaring over the sea is to dive at it, per thought.
const DIVE_CHANCE: f32 = 0.15;

/// How long a dive lasts, in seconds: the plunge and the moment at the top
/// of the water. The climb back out is the spring's, after it.
const DIVE_SECONDS: f32 = 1.6;

/// How fast a diving gull may drop, in blocks a second. Faster than
/// `FLIGHT_SPEED` -- a plunge that eased down at a grouse's pace is a bird
/// being lowered on a string.
const DIVE_SPEED: f32 = 12.0;

/// How far over the water a dive bottoms out, in blocks: at the top of it
/// and not in it. Nothing here swims but a fish, and a gull under the sea
/// would have to be taught to come back out.
const DIVE_SKIM: f32 = 0.35;

/// How much of its steering a gull keeps in the air, against a walking
/// animal's `AIR_CONTROL` of a quarter.
///
/// **A wing is a grip on the air.** A deer that has jumped off a bank
/// cannot steer and should not; a gull that could not was the first shape
/// of the circle, and at a quarter it lagged its own heading by a second
/// and a half and flew a slow spiral out to sea.
const GLIDE_GRIP: f32 = 0.8;

/// What fraction of its awareness a gull on the sand keeps after dark.
///
/// **A half, and it is the hunt's other door.** By day a gull goes up at
/// nine blocks and a spear reaches six; at night it goes up at four and a
/// half, so a hunter who walks the dark beach softly is inside a thrust
/// before the flock notices. It is the one time of day the shore favours the
/// player, which makes it a reason to be there then.
const ROOSTING_WARINESS: f32 = 0.5;

/// How many columns a gull deciding to land looks at for somewhere dry.
///
/// Three, of at most forty-eight reads each (`sea_or_ground_under`), paid
/// only on a thought where it has already decided to come down. A gull that
/// finds only sea keeps circling and asks again next thought.
const LANDING_LOOKS: usize = 3;

/// How many bearings round a player the coast spawner asks the generator
/// about, at each of two distances.
///
/// **Sixteen biome lookups every `SPAWN_INTERVAL` per player, and no block
/// reads and no random draws at all when none of them is coast** -- which is
/// nearly everywhere, and is why a meadow's seeded animals behave exactly as
/// they did before there were gulls.
const COAST_BEARINGS: usize = 8;

// ---- the nest, and the six things a bird does about it ----
//
// **Every other animal in this world is wherever it wandered to.** That
// is right for a deer, which lives in a herd and not in a place, and it
// was the whole of what made the birds look mindless: a covey drifted
// across a meadow at random, took off when you came near, landed
// somewhere else at random, and drifted on. Nothing it did referred to
// anything, so nothing it did could be read.
//
// A bird has a *home* -- the nest `worldgen::place_nests` put in a
// canopy, or, failing that, a canopy it has adopted -- and six states
// laid over the ones every animal already has:
//
// | what it is doing | the state | what moves it on |
// |---|---|---|
// | foraging | `Wander`/`Idle` (through `graze`) | `SETTLE_CHANCE`, or straying past `HOME_RANGE` |
// | at the nest | `Idle` | `HOP_CHANCE` (a hop), or the restless roll in `graze` (back to foraging) |
// | flying | `Homing` | coming within `HOME_REACH` of what it aimed at |
// | landing | the tail of `Homing`: `Idle`, still in the air | the altitude spring in `walk` puts it down |
// | looking for a nest site | no home, `next_home_scan` at zero | `nest_near` finds a nest or a canopy |
// | going home | `Homing` at `Animal::home` | as flying |
//
// and the seventh, which is not new: `Flee`. A frightened bird goes up
// on the state everything else runs on, and when the fright has passed
// it is somewhere it did not choose -- so the first thing it does is fly
// home, which is what "it comes back once you have gone" is made of.

/// How far from its nest a bird will forage before it turns back, in
/// blocks.
///
/// Twelve, which is a herd's width and about as far as one bird can see
/// another. Shorter and the nest is a tether -- a bird pacing a circle
/// six blocks across is a tethered goat; longer and the nest stops
/// meaning anything, because a bird forty blocks away is not at a nest,
/// it is somewhere else with a memory.
const HOME_RANGE: f32 = 12.0;

/// How near its home a bird has to get before it has *arrived*, in
/// blocks.
///
/// A block and a half, the same figure `COVER_REACHED` uses and for the
/// same reason: an approach that has to end on the exact cell is an
/// approach that overshoots, comes back, overshoots, and never lands.
const HOME_REACH: f32 = 1.5;

/// How far a bird without a nest looks for one, in blocks.
///
/// Fifteen: three spokes' worth (see `nest_near`) and about the distance
/// a bird would keep the tree in sight from. Further would find trees on
/// the far side of a valley, which reads as a bird leaving rather than
/// as a bird settling.
const NEST_RANGE: f32 = 15.0;

/// How high above its own feet that search looks, in blocks.
///
/// Ten. A nest sits in a canopy and a canopy is above head height, so a
/// search at the bird's own level finds nothing at all; ten clears the
/// crown of every tree this generator makes that a bird could stand
/// under. It is also the depth of the per-column scan, which is what
/// makes the cost below a number rather than a guess.
const NEST_LIFT: i32 = 10;

/// How often one bird may pay for that search, in seconds.
///
/// **This is the cost argument for the whole mechanic**, and it is the
/// same argument `WATER_SCAN_INTERVAL` makes. The search is three
/// distances on each of eight headings -- twenty-four columns -- and
/// each column is scanned downward from `NEST_LIFT` above the bird's
/// feet to a block below them, stopping at the first cell that is not
/// air. That is at most twelve reads a column and about eleven over open
/// ground (ten cells of sky, then the turf), so **roughly 264 block
/// reads, and never more than 288.**
///
/// It is paid only by a bird that has *no home*, is not frightened, is
/// not thirsty, and has not paid in the last eight seconds. A bird that
/// has found a nest never pays it again. Fowl are a minority of a
/// population now capped at three per player, so the realistic bill is
/// one scan every eight seconds for the first few seconds of a covey's
/// life: call it thirty-odd reads a second while a covey settles and
/// none at all afterwards, against the five thousand a second that
/// scanning once a tick per bird would have cost.
const NEST_SCAN_INTERVAL: f32 = 8.0;

/// How likely a bird foraging inside `HOME_RANGE` is to break off and go
/// and sit on the nest itself, per thought.
///
/// **Without this the nest is a leash rather than a place.** The first
/// shape had one rule -- come back when you are more than twelve blocks
/// off -- and what it produced was a bird that circled its tree at ten
/// or eleven blocks for ever and never once went to it. A third of its
/// thoughts, so the cycle a player sees from the ground is: forage
/// across the clearing, fly up into the tree, sit, come down again.
const SETTLE_CHANCE: f32 = 0.35;

/// How likely a bird that is *at* the nest is to hop to a nearby perch
/// instead of standing still, per thought.
///
/// A quarter. **What this exists for is that a bird should fly between
/// points rather than in one straight line to one point.** A bird that
/// only ever flew when it was frightened and only ever flew home was two
/// journeys; the hop is the third and it is the one a player watching a
/// tree actually sees -- up, across, down, again, all inside the nest's
/// own radius. Higher and the bird never settles; lower and it may as
/// well not have wings.
const HOP_CHANCE: f32 = 0.25;

/// How far one hop goes, in blocks.
///
/// Six, comfortably inside `HOME_RANGE`, so a hop is never the thing
/// that strands a bird away from its nest.
const HOP_RANGE: f32 = 6.0;

/// How far ahead of itself a flying bird reads the ground, in blocks.
///
/// **This is the fix for "они врезаются в листву".** The altitude spring
/// flies a bird `FLIGHT_HEIGHT` over whatever is *under* it, and a bird
/// crossing a meadow toward a wood is over the meadow right up until the
/// moment it is over the canopy -- so the first thing that told it a tree
/// was there was the tree. What a player saw was a grouse flying into the
/// side of a crown and scrabbling along it.
///
/// Five blocks is a second and a bit of a cruise at `CRUISE_FRACTION` of a
/// bird's run, which is time for the spring (`FLIGHT_CLIMB`, capped at
/// `CALM_CLIMB`) to lift it a canopy's worth. It costs one extra column
/// read a tick per bird in the air and nothing at all for everything that
/// walks -- see `flight_floor`, which is also where the "not while it is
/// landing" clause is argued.
const AIR_LOOK_AHEAD: f32 = 5.0;

/// How many boxes the air ahead is tested at when a bird has to steer
/// round something, and how far apart they stand.
///
/// Three at a block and a half: four and a half blocks of clearance, which
/// is the same look-ahead a walking animal gets from `LOOK_AHEAD` and the
/// distance a cruising bird covers in a second. See `air_is_clear`.
const AIR_PROBES: usize = 3;
const AIR_PROBE_STEP: f32 = 1.5;

/// How much higher than itself a bird will climb for what is ahead, in
/// blocks.
///
/// Four: a canopy over a meadow, a wall, a roof. **A bird tops a tree, not a
/// hill.** Without a limit the look-ahead would hand the spring a cliff face
/// twenty blocks up as the floor it must clear, and what that produces is a
/// bird climbing at `CALM_CLIMB` for six seconds in a straight line -- a
/// balloon, and one that leaves its own wood. Anything taller than this is
/// terrain to go round, which is what `air_is_clear` and `steer_around`
/// are for.
const AIR_CLIMB_REACH: f32 = 4.0;

/// How much height a bird gains the moment something stops it in mid-air,
/// in blocks a second.
///
/// A bird that meets a branch goes *over* it: that is what wings are for,
/// and the alternative -- the sideways slide `steer_around` gives a deer
/// against a rock -- is a bird crabbing along the edge of a crown, which
/// is the thing the player was looking at. Capped by the same
/// `CALM_CLIMB` the spring is, so a bird bumping a trunk does not shoot
/// up like a firework.
const BUMP_CLIMB: f32 = CALM_CLIMB;

/// What fraction of its run a bird flies at when it is going somewhere
/// on purpose rather than getting away from something.
///
/// Seven tenths. A cruise is visibly not a bolt, which is what tells a
/// player at thirty blocks whether the bird they can see has been
/// startled by something they cannot.
const CRUISE_FRACTION: f32 = 0.7;

/// How high an animal will climb without jumping, in blocks.
///
/// The same as a player's reach, and for the same reason: terrain is
/// full of single-block benches, and anything that has to path around
/// them spends its life walking into walls. Anything taller is a wall to
/// an animal, which is what makes a fenced-in pen possible without a
/// fence block existing.
const STEP_HEIGHT: f32 = 1.05;

/// The shortest way round from one angle to another, capped.
///
/// Shortest, which is the whole of it: an animal told to face the
/// opposite way must turn through a half circle in *some* direction and
/// not wind the long way round through five quarters of one, and an
/// animal at 359 degrees told to face 1 must move two degrees rather
/// than 358.
fn turn_towards(from: f32, to: f32, most: f32) -> f32 {
    use std::f32::consts::{PI, TAU};
    let delta = (to - from).rem_euclid(TAU);
    let delta = if delta > PI { delta - TAU } else { delta };
    // **Wrapped, not just turned.** `from` used to be carried forward
    // unwrapped -- every call added a small delta to whatever the
    // previous call returned, for as long as the animal lived. A yaw
    // that only ever turns toward fresh, bounded targets stays small in
    // practice, but an animal that keeps the same heading for a long
    // time (a wander that happens to bend the same way for minutes, or
    // a wolf that circles a kill) drives it past the point where an f32
    // can tell one radian from the next: past roughly eight hundred
    // thousand, adding `TURN_RATE * dt` stops changing the value by a
    // predictable amount and starts changing it by whatever the rounding
    // does, which reads as an animal that spins in place at a speed
    // nothing asked for. Wrapping the *result* into a single lap keeps
    // every future call working from a small number, so the error never
    // has anywhere to accumulate. See
    // `a_yaw_that_has_turned_the_same_way_for_a_long_time_does_not_lose_its_bearings`.
    (from + delta.clamp(-most, most)).rem_euclid(TAU)
}

/// How fast an animal turns, in radians per second.
///
/// About two hundred degrees a second: a quarter turn takes a fifth of a
/// second. Fast enough to look decisive, slow enough that a decision
/// which reverses the facing reads as the animal turning round rather
/// than as it being replaced by one pointing the other way.
const TURN_RATE: f32 = 3.5;

/// How fast it climbs one, in blocks per second.
///
/// **A step used to be instant** -- the whole block, in the tick the
/// animal met it -- and at twenty ticks a second with a client
/// interpolating between them, that is not a step, it is a deer
/// appearing a metre higher up. Four blocks a second spreads a full
/// block over about a fifth of a second, which reads as scrambling up.
///
/// It is deliberately faster than gravity is at that height, or an
/// animal would spend the climb falling back down it.
const CLIMB_SPEED: f32 = 4.0;

/// How often an animal reconsiders what it is doing, in seconds.
///
/// Once a second, not once a tick. The whole cost of the mind is paid
/// here, and a deer that re-derives its opinion of the world twenty
/// times a second is nineteen wasted opinions -- and, visibly, a deer
/// that jitters between two decisions instead of committing to one.
const THINK_INTERVAL: f32 = 1.0;

/// How far apart two animals of a kind still count as together, in
/// blocks.
///
/// Twelve, which is about as far as one deer is visible to another
/// across a meadow and comfortably inside the distance a player can see
/// both of them at once -- a herd that only forms at forty blocks is a
/// herd nobody ever sees form.
const HERD_RADIUS: f32 = 12.0;

/// How close to the middle of its herd an animal is content, in blocks.
///
/// Inside this it wanders as it likes; outside it drifts back. Five,
/// so a herd is a loose scatter of animals in the same field rather
/// than a stack of them standing on one another -- the failure mode of
/// making this small is not a herd, it is a heap.
const HERD_COMFORT: f32 = 5.0;

/// How far a bolting animal's panic carries to its own kind, in blocks.
///
/// **The whole reason a herd is worth having.** One deer sees the
/// player, and the field empties: the others never saw anything, they
/// saw the first one leave. It is shorter than `HERD_RADIUS`, so a herd
/// scatters from the middle outward rather than every animal in it
/// starting at once.
const ALARM_RADIUS: f32 = 9.0;

/// How far ahead an animal looks before committing to a heading, in
/// blocks.
///
/// Three, which at a run is about half a second: far enough to see the
/// cliff or the lake it was about to bolt into, near enough that the
/// probe is a handful of block lookups rather than a pathfinder. See
/// `open_heading`.
const LOOK_AHEAD: f32 = 3.0;

/// The headings an animal will settle for, either side of the one it
/// wants, in radians.
///
/// Nearest first, and it gives up at a right angle: past that it is no
/// longer running away from anything, and an animal that would rather
/// run *toward* the player than into a wall is worse than one that is
/// cornered. Being cornered is a real outcome here.
const SWERVES: [f32; 4] = [0.45, 0.9, 1.25, 1.57];

/// How soon an animal that walked into something reconsiders, in
/// seconds.
///
/// A third, because the alternative is what it used to do: the collision
/// zeroes the velocity, and the animal then stands with its nose against
/// the rock until its next scheduled thought -- which, in the middle of
/// a bolt, is two and a half seconds of a deer pressed against a wall
/// while a player walks up to it.
const BLOCKED_RETHINK: f32 = 0.33;

/// Below this fraction of its health, an animal stops fighting.
///
/// A third. What it buys is the end of the one thing a boar did that
/// nothing alive does: fight to the death, every time, whatever it
/// costs. A wounded one breaks off and runs, which turns the fight into
/// something a player can *win* rather than merely survive -- and turns
/// the last of it into a chase, which is the same verb the rest of the
/// hunting in this world uses.
const BREAKS_OFF_BELOW: f32 = 0.34;

/// How hard a landed blow shoves an animal, in blocks per second.
///
/// Enough to be seen and not enough to move it out of reach: a boar
/// knocked a metre back is a boar you have to step toward again, which
/// is a fight rather than a queue. It is applied to the velocity rather
/// than to the position, so the collider still decides where it ends up
/// -- a shove must not put anything inside a wall.
const KNOCKBACK: f32 = 3.5;

/// How long a bolt lasts before an animal looks up again.
///
/// The hare's whole design: it is faster than a sprinting player and it
/// stops. Without the stop it is uncatchable and therefore pointless;
/// with it, catching one is cutting it off rather than outrunning it.
const FLEE_SECONDS: f32 = 2.5;

/// How close a boar has to be to land a blow, in blocks.
const GORE_RANGE: f32 = 1.8;

/// ...and how nearly in front of it the player has to be, as the cosine
/// of the angle off its nose.
///
/// A half is sixty degrees either side: wide enough that a boar does not
/// have to be aimed like a rifle, narrow enough that standing behind one
/// is standing behind it.
const GORE_ARC: f32 = 0.5;

/// ...and how long between blows.
///
/// Two seconds, so a player caught by one is losing health steadily
/// rather than instantly -- long enough to decide to run, which is the
/// decision a boar exists to force.
const GORE_INTERVAL: f32 = 2.0;

/// How long the red flash on a struck animal lasts, in seconds.
const HURT_SECONDS: f32 = 0.4;

/// How long a killed animal takes to go down, in seconds: the time between
/// the blow and the carcass.
///
/// **The body is kept for this long, and it has to be.** A killed deer used to
/// be removed in the tick that killed it and a carcass block written into the
/// cell under it -- so on screen a running animal became a block lying on its
/// side between two frames, which reads as the game swapping one thing for
/// another rather than as anything dying. Kept as an entity for a little
/// under a second (`Animals::falling`), still carried by its last shove and by
/// gravity, with `Attitude::Dying` on the wire so every client rolls it over
/// (`animal_model::Motion::fallen`); and *then* the carcass is laid, in the
/// cell the body came to rest in rather than the one it was struck in.
///
/// Under a second, because it is a moment and not a scene: long enough for
/// the roll to read, short enough that a player walking up to their kill
/// never has to wait for it.
pub const FALL_SECONDS: f32 = primitive_shared::protocol::DEATH_FALL_SECONDS;

/// How fast a falling body's last shove bleeds off, a second: a knocked-down
/// deer slides a little and stops, rather than skating to where its run was
/// taking it.
const FALL_DRAG: f32 = 6.0;

/// How long one charge runs for: the ends of the range it is rolled in.
///
/// Under two seconds, which at a boar's speed is about nine metres --
/// far enough to cross the distance it starts one at, short enough that
/// a player who steps aside is out of it rather than merely delaying it.
const CHARGE_SECONDS: (f32, f32) = (1.2, 1.8);

/// ...and how long it stands blown afterwards.
///
/// The window the whole fight happens in: this is when you hit it. A
/// boar with no recovery is a boar you can only run from.
const RECOVER_SECONDS: (f32, f32) = (0.9, 1.6);

/// How long a hostile animal flinches for after a blow.
///
/// A third of a second: long enough to read as a flinch, short enough
/// that the boar is coming back before the player has finished
/// congratulating themselves. Everything that runs uses `FLEE_SECONDS`
/// instead, which is a bolt rather than a flinch.
const FLINCH_SECONDS: f32 = 0.35;

/// How nearly a charging animal has to be facing its target before it
/// runs, in radians.
///
/// A quarter of a radian is about fifteen degrees. See the wind-up in
/// `walk` for what it is for.
const CHARGE_AIM: f32 = 0.25;

/// How far past its target a charge is aimed, in blocks.
const CHARGE_OVERSHOOT: f32 = 3.0;

/// How quickly an animal reaches the speed it is asking for, per second.
///
/// Three, so a charge takes about a third of a second to wind up and as
/// long again to bleed off. That delay is what a sidestep is spent
/// against.
const ACCELERATION: f32 = 3.0;

/// How much of a running body's speed goes into turning it across its own
/// momentum, at a quarter turn off.
///
/// A third, and not more: the number is bounded above by the hunt. A deer
/// flees in bursts that bend round cover (`escape_heading`) and a wolf chases
/// by re-aiming every tick, so the cost is paid more often by the animal
/// running away than by the one behind it -- push it past a third and a wolf
/// catches everything, which is the opposite of the failure it was added to
/// fix. Held by `a_running_animal_that_turns_hard_loses_speed_doing_it` and by
/// the hunt tests either side of it.
const TURN_COST: f32 = 0.33;

/// How quick a body is: how fast it comes round, and how fast it gets going.
///
/// From its width, because width is the cheapest honest stand-in for how much
/// of the animal there is -- a hare is a third of a block across and a bear
/// is most of one, and the mass goes as the cube of that. Clamped at both
/// ends: below eight tenths a bear is a barge nobody can escape by walking
/// away from, and above one and four fifths a hare is a mosquito.
///
/// Rejected: **a table per species**, which is the same eleven numbers
/// written down where they can go stale against the sizes they describe.
/// Rejected too: **deriving it from the mass** -- `Species` has no mass, and
/// inventing one to feed a steering coefficient is a second body model.
fn nimbleness(species: Species) -> f32 {
    // **A sidler turns as fast as it likes, and its width is beside the
    // point.** Everything else here has to swing a body round to go a new
    // way, which is why the number is read off how wide that body is; a crab
    // is already pointing every way at once (`Species::sidles`), so the
    // question does not apply to it. Without this the widest animal in the
    // world would have been the slowest to change its mind, which is the
    // opposite of what a crab on a beach does -- and what a player would
    // have seen is a thing that took half a second to decide which rock to
    // go under, standing still in a hand's reach.
    if species.sidles() {
        return 1.8;
    }
    (0.6 / species.width()).clamp(0.8, 1.8)
}

/// Below this, in blocks per second, a stopping animal is stopped.
///
/// The exponential decay in `walk` never reaches zero on its own. Half a
/// block a second is under a tenth of a walk and well below anything a
/// player could see as motion -- but it is *not* below what the client's
/// walk cycle can see, which is why this is here rather than being left
/// as a rounding detail.
const STANDING_STILL: f32 = 0.5;

/// How much of that it has in the air. A quarter: an animal that steers
/// freely while falling is a helicopter.
const AIR_CONTROL: f32 = 0.25;

/// How far an animal will step down without minding, in blocks.
///
/// Three, which is exactly the drop a *player* takes for free. An animal
/// that refused to walk down anything a player walks down would pen
/// itself into the terrain.
const SAFE_STEP_DOWN: f32 = 3.0;

/// Upward pull on a body in water, and the fastest it rises.
///
/// None of the three swims. What this buys is that they do not drown
/// standing up either: an animal that falls in floats back to the
/// surface and walks out, instead of sinking to the bed and pacing about
/// down there where nobody can see it.
const BUOYANCY: f32 = 26.0;
const SWIM_RISE: f32 = 2.2;

/// What fraction of its speed an animal keeps while it is in water.
///
/// A third. An animal that wandered in anyway -- or was driven in --
/// should visibly struggle to get out rather than skating across the
/// surface at a run, which is what it did when buoyancy was the only
/// thing water changed.
const WADE_SPEED: f32 = 0.35;

// ---- thirst, the water's edge, and the way out of it ----
//
// Water used to be one thing to an animal: a wall it stopped at (see
// `footing`) and, if it ever got in, a fluid that pushed it upward
// (`BUOYANCY`). Both are still true and both were half the story. An
// animal has a *reason* to walk to a lake and a *reason* to get out of
// one, and neither existed: a herd grazed past a river for an hour
// without ever looking at it, and one that was driven in milled about
// in the shallows until the player lost interest.

/// How long an animal goes before it wants a drink, in seconds.
///
/// Two minutes of play at a standstill, and a good deal less if it has
/// been running (`RUNNING_THIRST`). Long enough that drinking is
/// something a player notices a herd doing rather than something the
/// herd is always doing -- at thirty seconds every animal in sight
/// would be walking to water at any given moment, and the meadow would
/// read as a queue at a trough. Short enough that a river bank is a
/// place worth waiting at, which is the whole point of the mechanic:
/// it tells a hunter *where the animals will be*.
const THIRSTY_AT: f32 = 120.0;

/// How much faster thirst grows while an animal is actually running.
///
/// Three times. What this buys is the one interesting interaction the
/// mechanic has: a herd that has been chased across a meadow goes to
/// water sooner than one nobody bothered, so the hunter who failed the
/// first stalk knows where to be for the second.
const RUNNING_THIRST: f32 = 3.0;

/// How fast drinking puts thirst back, in seconds of thirst per second.
///
/// Forty, so a full drink is three seconds and change from the
/// threshold. Counted rather than cleared on arrival on purpose: an
/// animal startled off the water halfway through has had *half* a
/// drink and comes back for the rest, which is what makes standing
/// between a herd and the river work at all.
const SLAKED_PER_SECOND: f32 = 40.0;

/// The longest thirst an animal accumulates, as a multiple of
/// `THIRSTY_AT`.
///
/// Twice. Without a cap an animal penned away from water for ten
/// minutes arrives with six hundred seconds of thirst and stands at the
/// edge drinking for a quarter of a minute, which reads as a stuck
/// animal rather than a thirsty one.
const THIRST_CAP: f32 = THIRSTY_AT * 2.0;

/// How long one pull at the water lasts, in seconds.
///
/// **A pull, not the whole drink.** It was the whole drink -- three and a
/// half to five and a half seconds of head-down with no thought in between --
/// and that is exactly as long as the animal could not look up for. A thought
/// is what reads `Neighbours::threat`, so a deer at the bank did not know a
/// wolf existed until its drink was over; what a player saw was a deer a wolf
/// walked the length of a meadow up to and bit, with the deer's nose in the
/// water throughout. `a_thirsty_animal_runs_from_a_wolf_rather_than_finishing_its_drink`
/// was passing on the timing of one seed.
///
/// A second or so is a mouthful, and the drink goes on across as many of them
/// as the thirst needs: `graze` walks straight back into `Mind::Drink` while
/// there is thirst left and the water is still in reach, so what changes is
/// not how long the animal stands there -- `SLAKED_PER_SECOND` and the thirst
/// decide that -- but that its head comes up between gulps. Which is what an
/// animal at a waterhole does, and the same shape the feeding bout has.
const DRINK_SECONDS: (f32, f32) = (0.8, 1.4);

/// How near the water an animal has to be to be drinking from it, in
/// blocks, measured to the middle of the water cell.
///
/// One and a half, which is a body's length short of the edge: the
/// animal stands on the bank with its nose over the water. Nearer and
/// the approach would have to end *inside* the cell it is aiming at,
/// which `walk` refuses to walk into (and rightly -- see the liquid
/// check there), so the animal would circle the pond it means to drink
/// from for ever.
const DRINK_REACH: f32 = 1.5;

/// How far a thirsty animal looks for water, in blocks.
///
/// Twelve: about a deer's awareness, so the water an animal goes to is
/// water it could plausibly have known was there. Further and a thirsty
/// herd sets off on a cross-country march to a lake the player never
/// saw, which reads as animals leaving rather than animals drinking.
const WATER_RANGE: f32 = 12.0;

/// How often one animal may pay for a water search, in seconds.
///
/// **This is the whole cost argument for the mechanic.** The search is
/// twelve spokes sampled every block and a half out to `WATER_RANGE`
/// -- ninety-six columns of three lookups, so about three hundred block
/// reads, the same order as `lit_fire_near` -- and it is paid only by
/// an animal that is *already thirsty* and has no water in mind, at
/// most once every four seconds. At the cap of a hundred and twenty
/// animals, all of them thirsty at once, that is thirty scans a second:
/// nine thousand block reads, against the six hundred and ninety
/// thousand a scan on every tick for every animal would have cost. A
/// lake scan per animal per tick was never affordable and this is why
/// there is a timer rather than a flag.
const WATER_SCAN_INTERVAL: f32 = 4.0;

/// How far an animal that has ended up in water looks for a way out, in
/// blocks.
///
/// Eight, and shorter than `WATER_RANGE` because the question is
/// different: the nearest dry ground, not the nicest. An animal in the
/// middle of an ocean finds nothing and goes on floating, which is the
/// honest answer -- nothing here swims.
const SHORE_RANGE: f32 = 8.0;

/// What fraction of its run a blown animal has left.
///
/// Three fifths. Two numbers decide it. A blown deer has to be
/// *catchable* -- three and three quarters against a player's walk of
/// five and a half, so the chase that went on long enough ends with the
/// player closing rather than with a deer that is merely slower. And it
/// has to stay above `COMMITTED_FRACTION`, or a blown boar would never
/// count as committed to its own charge: the turn cap would come off
/// mid-run and the overshoot that ends a burst would stop firing, so a
/// tired boar would quietly become the homing missile the charge was
/// rebuilt to stop being.
const BLOWN_SPEED: f32 = 0.6;

/// How fast wind comes back, in seconds of run per second of not
/// running.
///
/// Half. An animal that recovers as fast as it spends can chase in
/// alternate bursts for ever, which is a chase with no end in it; at a
/// half a wolf that has run its dinner down and lost it has to spend
/// twice as long walking before it can try that again.
const STAMINA_RECOVERS: f32 = 0.5;

/// How far from a player an animal may appear.
///
/// Beyond the near distance so nothing pops into existence in front of
/// somebody, inside the interest radius so it is replicated the moment
/// it exists rather than wandering unseen.
///
/// **Pushed out from twenty-four when the animals were made rare.** The
/// two go together and the reason is what a player sees rather than what
/// the numbers are: at twenty-four blocks a fresh deer is inside the
/// distance you can pick one out at, so a rationed world would have
/// spent its ration arriving in plain view -- three animals a player
/// watched appear. Thirty-six is past that, and seventy-two is still
/// well inside `DESPAWN_DISTANCE`, so nothing is spawned only to be
/// forgotten a few seconds later. What this buys is that the three
/// animals a player is allowed were *already there* when they walked
/// over the rise, which is the whole of what makes one an encounter.
const SPAWN_MIN: f32 = 36.0;
const SPAWN_MAX: f32 = 72.0;

/// The country a world that cannot say which biome it is gets taken for.
///
/// **A temperate meadow**, because that is what every such world has always
/// spawned: the test meadows, and any host a mod builds on `BlockWorld`
/// without a generator behind it. Taking "cannot say" as "anywhere" would
/// put lions in all of them; taking it as "nowhere" would empty them.
const UNKNOWN_COUNTRY: primitive_shared::worldgen::Biome = primitive_shared::worldgen::Biome::Plains;

/// How far from the first of a group the rest of it stands.
///
/// Close enough that they can see each other -- the alarm that makes a
/// herd a herd reaches about this far (see `Neighbours`) -- and far
/// enough that they read as several animals rather than as one animal
/// drawn four times.
const GROUP_SPREAD: f32 = 6.0;

/// How often the spawner tries at all, in seconds.
///
/// Every twelve seconds rather than every tick, and it was every four.
///
/// **This is the number that decides whether an animal is an event.**
/// The cap (`MAX_ANIMALS_PER_PLAYER`) says how many may stand near a
/// player at once; this says how quickly the world refills the one that
/// was just eaten. At four seconds a hunter who took a deer had another
/// group within half a minute, so the cap read as a queue rather than as
/// scarcity -- rarity you can wait out is not rarity. At twelve, a
/// cleared valley stays cleared for a few minutes, and going *somewhere
/// else* is the answer to having hunted here.
///
/// It is not longer than that because an attempt is not a spawn: it
/// fails on unloaded ground, on rock, on a bear that wanted a wood and
/// found a meadow. Twelve seconds of failures in a stony place is
/// already several minutes of nothing, and a player who has walked into
/// a valley should not have to camp it before it has anything alive in
/// it.
const SPAWN_INTERVAL: f32 = 12.0;

// ---- cover, memory, the pack and the charge ----
//
// The numbers behind the four behaviours the module doc lists last.
// Each is a distance or a time a player can *feel*, and each doc says
// which way it fails if it is moved.

/// How far a fleeing animal looks for somewhere to hide, in blocks.
///
/// Sixteen: a bit further than a deer notices a person, so a deer
/// startled at the edge of a meadow can see the wood on the far side of
/// it. Much further and every bolt is a cross-country run to a forest
/// the player never saw; much nearer and cover is only ever a thing the
/// deer was already standing in.
const COVER_RANGE: f32 = 16.0;

/// How near a remembered patch of cover an animal has to be to count as
/// having reached it, in blocks.
///
/// Once there it is *in* the trees, and the next bolt looks for the next
/// trees rather than steering at the trunk it is standing beside --
/// which is what a deer did in the first version: it arrived, aimed at
/// the same column again, and ran into it.
const COVER_REACHED: f32 = 1.5;

/// How long a fleeing animal has to be out of the threat's sight before
/// it stops running, in seconds.
///
/// Three. A deer that relaxed the instant a trunk passed between it and
/// you would stop every time you blinked, and one that needed thirty
/// seconds would be out of your awareness long before it mattered. Three
/// is about a burst and a bit -- long enough that a player who keeps
/// coming through the trees keeps the deer moving, short enough that
/// one who stops at the edge of the wood gets a deer that stops too.
const HIDDEN_SECONDS: f32 = 3.0;

/// How often a fleeing animal checks whether it can still be seen, in
/// ticks.
///
/// Every tenth tick, so a line-of-sight ray -- a couple of dozen block
/// lookups -- is paid twice a second per *fleeing* animal and never for
/// a grazing one. Once a tick would be twenty rays a second per animal
/// for a fact that changes at the pace a person walks. The count is
/// staggered by id (see `spawn`) so a herd that bolts together does not
/// all look on the same tick.
const LOOK_EVERY: u8 = 10;

/// How far off the flight line a hare's burst is aimed, in radians,
/// alternating sides every burst.
///
/// About thirty-five degrees. The hare's whole design is that it cannot
/// be run down and it stops; what this adds is that where it *goes* is
/// not the straight line a player can pre-aim down. Wider and the
/// zig-zag costs it enough ground that a sprinting player closes on it
/// anyway, which would take away the one thing the animal is.
const HARE_DODGE: f32 = 0.6;

/// How long an animal remembers a place it was hurt, in seconds.
///
/// Five minutes of play. Long enough that a hunter who stands in one
/// spot and takes one deer after another finds the meadow empty, short
/// enough that the meadow is a meadow again by the time they come back
/// with the second hide cured. Struck twice in the same place, the
/// memory is held twice as long -- see `Animal::remember_danger`.
const DANGER_MEMORY: f32 = 300.0;

/// How near a remembered danger a grazing animal will not stay, in
/// blocks.
///
/// Ten, which is about a herd's width: inside it the animal walks out,
/// and the herd-centre pull (see `graze`) then brings the rest after it.
/// Smaller and the herd shuffles one step sideways and carries on being
/// shot at; larger and one strike clears a whole valley, which is a
/// hunter who cannot hunt.
const DANGER_RADIUS: f32 = 10.0;

/// Two blows nearer together than this, in blocks, are the *same*
/// place, and the second lengthens the memory rather than adding one.
const SAME_PLACE: f32 = 6.0;

/// How many places one animal keeps in mind.
///
/// Four, and the freshest win. It is a `Vec` so that an animal nobody
/// has ever hit -- nearly all of them -- carries a null pointer and not
/// an array; the cap is what keeps a long fight from growing it.
const DANGERS_REMEMBERED: usize = 4;

/// How near a lit fire a wolf will not come after dark, in blocks.
///
/// Five. Inside a campfire's own light; outside it a pack still stands
/// in the dark and watches, which is the thing a fire at night is *for*:
/// not safety, but a circle you can see the edge of. Held torches count
/// through `Animals::carrying_fire`, so walking home is walking a circle
/// of five blocks through the wood.
const FIRE_RADIUS: f32 = 5.0;

/// Where a night hunter that has come to a fire waits: the ring it walks
/// round the fire, in blocks from it.
///
/// **Three past the circle it will not enter**, which is about where a
/// campfire's light has fallen off to a glow: near enough that a player at
/// the fire sees shapes moving at the edge of it, far enough that the
/// shapes are never inside a spear's reach of somebody sitting by the
/// flames. It used to stand wherever it happened to notice the camp from --
/// thirty blocks off, in the dark, where nobody could see it -- and a pack
/// that is kept at bay and never seen is not a pack at all, it is a rule.
/// Now it comes to the edge and circles, which is the picture: the fire, and
/// the dark round it with something walking in it.
const FIRE_EDGE: f32 = FIRE_RADIUS + 3.0;

/// The share of its thoughts at the edge of a fire that a hunter spends
/// stopped and looking in, rather than walking the ring. A third: enough
/// that a player at the fire catches one standing and staring, not so much
/// that the ring stops moving.
const FIRE_EDGE_PAUSE: f32 = 0.3;

/// How much further a person beside a lit fire is seen after dark than a
/// person on a walk by day.
///
/// **Two: a fire is the brightest thing in the night**, brighter than the
/// torch in a hand (`TORCH_AT_NIGHT`, 1.3), and it does not move with you.
/// A player sitting still by a campfire is seen by a wolf from about
/// twenty blocks (its eighteen, times two, times a stand's 0.6) against
/// about five in the dark with no fire at all (times `DARKNESS`). **That is
/// the price of the fire and the whole of the decision it makes**: lit, the
/// night knows where you are and comes to look, and cannot come in; dark,
/// you are nearly invisible and nothing keeps anything off you.
const FIRE_SEEN_AT_NIGHT: f32 = 2.0;

/// How often, in seconds, whether a player is sitting by a fire is looked
/// at again. The look is `lit_fire_near`'s disc -- a few hundred block
/// reads -- and a fire does not light or go out twenty times a second, so
/// a second's staleness costs nothing a player could notice.
const FIRE_LOOK_EVERY: f32 = 1.0;

/// How far from a bed the pack that finds a sleeper is put down, in blocks.
///
/// **Two seconds of a wolf's run.** They come in at once (`FOUND_GRUDGE`),
/// so the distance *is* the warning: at ten to twelve blocks and a run of
/// six a second, the waking player has about the time it takes to read the
/// notice, stand, and choose -- the torch in the hand, the spear, a wall at
/// the back. Put down in a bite, the waking would be the death; put down at
/// twenty, it would be a howl nobody had to answer.
const SLEEPER_FOUND_DISTANCE: (f32, f32) = (10.0, 12.0);

/// How long the pack that finds a sleeper comes on regardless, in seconds.
///
/// **They came for somebody, and they know where.** Without it the pair was
/// put down at ten blocks from a person standing still in the dark -- whom
/// a wolf sees at five (`DARKNESS`) -- and half the time they lost the
/// player they had just found and wandered off grazing: a night that woke
/// you for nothing. With it they come in as a wolf that has been struck
/// comes in (`angry_for`, which reaches past noticing), and it runs out:
/// twelve seconds is the run-in and a bite or two, and after that the pair
/// is an ordinary pack with the ordinary rules -- a player who broke away
/// and kept moving has lost them. **A fire still wins**: the fire rule in
/// `think_hunter` is asked before any grudge, so a torch lit in the first
/// second of waking is the circle they stop at.
const FOUND_GRUDGE: f32 = 12.0;

/// The radius the second wolf circles at while it works round to the
/// far side of you, in blocks.
///
/// Five: inside its own provoking range, so it is committed, and outside
/// a bite, so it is not a bite yet. See `flank`.
///
/// **The ring is not what the wolf's first leg holds to**, and it is
/// worth knowing which way that fails. A circling wolf begins pointing
/// wherever it happened to be pointing and turns onto its aim while it
/// is already accelerating, so the ground it covers before it is
/// actually on the arc is drift straight across the circle -- and that
/// drift is a fraction of its run. Widening this does not buy it back
/// (tried at five and a half: the dip moved by a tenth of a block,
/// because the drift is set by the turn and not by the radius). What
/// bounds it is the wolf's own bite range, which the first leg stays a
/// body clear of; `the_second_wolf_comes_from_the_other_side` states it
/// that way rather than as a fraction of this number.
const CIRCLE_RADIUS: f32 = 5.0;

/// How nearly opposite its packmate a circling wolf has to be before it
/// comes in, in radians.
///
/// Half a radian is about thirty degrees off exactly behind you -- close
/// enough that facing one wolf has the other out of the corner of your
/// eye, loose enough that it does not orbit for ever chasing a mate that
/// keeps moving.
const FLANK_TOLERANCE: f32 = 0.5;

/// Below this fraction of its health a wolf stops closing and shadows.
///
/// A half, which is above the third at which everything else breaks
/// off, because a wolf that is losing does not leave: it falls back and
/// waits for you to bleed, be tired, or turn your back. See `shadow`.
const WOLF_SHADOWS_BELOW: f32 = 0.5;

/// How far off a wounded wolf keeps while it shadows, in blocks.
///
/// Twelve: inside its awareness so it never loses you, outside a
/// spear's throw so you cannot finish it, and near enough to be seen
/// at the edge of a torch's light, which is the point.
const SHADOW_DISTANCE: f32 = 12.0;

/// How fast an animal that is already *at speed* in a charge can turn,
/// in radians per second.
///
/// About thirty-five degrees a second, against two hundred for anything
/// on its feet. A charge used to be aimed at a point and then steered
/// toward that point at the ordinary rate, which is nearly straight but
/// not quite: a boar that ran past you turned back within half a second.
/// This is the rest of the commitment -- once running, it goes where it
/// is pointed, and a sidestep at the last moment is a clean miss with a
/// second of the boar's back to look at. The wind-up before the run is
/// not capped (see `walk`), or the boar could never aim in the first
/// place.
const CHARGE_TURN_RATE: f32 = 0.6;

/// How much of its run speed a charging animal has to be doing before
/// it counts as committed and the turn cap applies.
const COMMITTED_FRACTION: f32 = 0.5;

/// How much harder a blow lands on a hostile animal's back while it is
/// turning round -- the second after a charge has passed you.
///
/// Half again. The sidestep is the skill, and this is what it pays: a
/// player who reads the charge gets the boar's flank and a better blow;
/// one who stands and swings gets the tusks. See `Animal::exposed_back`.
const BACKSTAB: f32 = 1.5;

// ---- the senses: what a person is doing, and how far off it tells ----
//
// **Awareness used to be a radius.** A deer noticed a person at twelve
// blocks if a ray reached them, and a wolf at eighteen whether or not
// anything did -- so a player standing still in the grass at dusk and one
// sprinting across the meadow at noon were the same fact to every animal
// alive, and the stalk was a line of sight and nothing else. The numbers
// below are what replaced it; the species' own numbers are in the shared
// crate (`Species::awareness`, `hearing`, `nose`, `view_cone`), and the
// arithmetic that combines them is `perceive`.
//
// Three shapes were weighed:
//
// * **A single radius scaled by the player's speed.** Cheap, and it fixes
//   the sprint -- but it has no *direction*: a person behind a wall would be
//   noticed exactly as one in the open, a wind would mean nothing, and the
//   deer grazing with its back to you would see you as well as one looking
//   at you. Every choice a hunter makes about *where* would still be
//   invisible.
// * **A stimulus field** -- noise and scent spread through the blocks by a
//   flood fill, the way light is. Right in principle and absurd in cost: a
//   flood per player per tick for a question sixty animals ask once a
//   second.
// * **Three senses with three reaches, each answered by a different
//   choice**, which is what is here. Hearing is distance only (through
//   leaves, round corners) and scales with what the person is doing;
//   sight needs a ray, a cone and light; scent needs the wind. The ray is
//   the only expensive part, so it is cast last, only when the cheap senses
//   have not already answered, and against a per-tick budget
//   (`RAYS_PER_TICK`).

/// How the person is moving, as an ear and an eye take it.
///
/// **Read off where they are, not off a flag they send.** The server
/// already has every player's position every tick; the gait is how far it
/// moved over the last `GAIT_WINDOW`, which is a fact a client cannot leave
/// unset. There is no crouch key in this game (see `physics`), so the
/// quietest a person can be is *slow* -- a thumb eased onto the stick, or
/// stop-start steps -- and still, which is quieter than slow and easier to
/// see.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Gait {
    Still,
    Creeping,
    Walking,
    Running,
}

impl Gait {
    /// From a speed over the ground, in blocks a second.
    ///
    /// The lines sit between the speeds a player actually has: under
    /// `CREEPING_BELOW` is a stalk, the walk is 4.3 and the sprint 6.45
    /// (`animals::NOMINAL_SPRINT_SPEED`), and `RUNNING_ABOVE` splits them.
    pub fn of_speed(speed: f32) -> Gait {
        if speed < STILL_BELOW {
            Gait::Still
        } else if speed < CREEPING_BELOW {
            Gait::Creeping
        } else if speed <= RUNNING_ABOVE {
            Gait::Walking
        } else {
            Gait::Running
        }
    }

    /// How far a person moving like this carries, as a share of a walk.
    ///
    /// **A sprint is twice a walk and a creep under a third of one**, and
    /// the ratio is the mechanic --
    /// `a_running_player_is_noticed_at_more_than_twice_the_distance_of_a_creeping_one`.
    /// Standing still makes no sound at all, which is why the way to get
    /// near a deer that has lifted its head is to stop.
    fn loudness(self) -> f32 {
        match self {
            Gait::Still => 0.0,
            Gait::Creeping => 0.3,
            Gait::Walking => 1.0,
            Gait::Running => 2.0,
        }
    }

    /// How easy a person moving like this is to pick out, as a share of a
    /// walk in plain view.
    ///
    /// **Low and slow, or not at all.** Movement is what catches an eye, so a
    /// creep and a stand are both seen at six tenths of a walk -- and no less,
    /// because a person standing in a meadow is still a person in a meadow.
    /// It was seven and a half tenths for a creep at first, and that made the
    /// eye the sense that decided every stalk: a lion saw a creeper at eleven
    /// blocks and heard a sprinter at twenty-two, so going slowly bought less
    /// than half. The sprint is a little more than a walk and no more:
    /// running is loud, not large.
    fn visibility(self) -> f32 {
        match self {
            Gait::Still => 0.6,
            Gait::Creeping => 0.6,
            Gait::Walking => 1.0,
            Gait::Running => 1.15,
        }
    }
}

/// What the tick loop knows about a player that the animals may use: which
/// way they face, whether they are working, in the air, low, or hurt.
///
/// **Handed in, like `carrying_fire`, rather than looked up**, for the reason
/// `step` takes positions instead of the registry: this file must not be
/// able to open a player's inventory or health. Five facts, each a choice a
/// player makes that an animal can answer. A player the loop said nothing
/// about is facing nowhere in particular, idle, standing and whole.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PlayerSign {
    pub who: PlayerId,
    /// The player's yaw, in the same convention as an animal's (x = cos,
    /// z = sin) -- `Camera::forward`'s.
    pub facing: f32,
    /// Swinging at a block, or has just broken, placed or struck something.
    pub working: bool,
    /// Off the ground on their own legs: a jump, and the landing after it.
    pub airborne: bool,
    /// Sitting or lying down.
    pub low: bool,
    /// Under half their health. What a wolf pack waits for.
    pub wounded: bool,
    /// In a bed. **Not the same as `low`**, which a stool also sets: a
    /// sleeper cannot see anything coming, and after dark that is what a
    /// lone wolf waits for (`think_hunter`).
    pub asleep: bool,
    /// What is in their hand, if anything: feed held out is a lure to an
    /// animal that eats it (`lure`).
    pub held: Option<primitive_shared::types::BlockId>,
    /// How far their smell carries against a person's, from what they wear
    /// (`equipment::reek`): one, or a tarred coat's half again.
    pub reek: f32,
}

/// Under this, in blocks a second, a person is standing still.
const STILL_BELOW: f32 = 0.35;
/// Under this, creeping: about half a walk.
const CREEPING_BELOW: f32 = 2.2;
/// Over this, running: between the walk's 4.3 and the sprint's 6.45.
const RUNNING_ABOVE: f32 = 5.0;

/// How long a player's movement is averaged over before its gait is read,
/// in seconds.
///
/// **Not one tick.** A client sends its position at its own rate, not the
/// server's, so the per-tick distance is a sawtooth of nothing and a jump --
/// a walking player read one tick at a time is alternately still and
/// sprinting. Four tenths of a second holds several updates and is still
/// quicker than anybody goes from a walk to a creep.
const GAIT_WINDOW: f32 = 0.4;

/// How loud work is, against a walk: breaking stone, chopping, a swing. It
/// rings out further than a sprint, which is what makes felling a tree in a
/// wood with a pack in it a decision.
const WORKING_LOUDNESS: f32 = 2.2;

/// How loud a jump is, against a walk: the thump of the landing.
const AIRBORNE_LOUDNESS: f32 = 1.5;

/// How much sitting or lying down takes off being seen and heard.
const LOW_POSTURE: f32 = 0.6;

/// How much standing in tall grass, a bush or a canopy takes off being
/// seen. A half: cover shortens the stalk, it does not end it.
const IN_COVER: f32 = 0.5;

/// How much the dark takes off being seen -- and what a lit torch in a hand
/// makes of it instead: the brightest thing in a dark wood is the person
/// carrying the fire, which is the price of the circle it keeps the wolves
/// out of (`FIRE_RADIUS`).
const DARKNESS: f32 = 0.5;
const TORCH_AT_NIGHT: f32 = 1.3;

/// Anything this near is noticed, whatever it is doing and wherever the
/// animal is looking, in blocks. Breath, the ground giving under a foot, a
/// shadow: nothing stands at arm's length from a wild animal unnoticed.
const PRESENCE: f32 = 2.5;

/// How much sharper a wary animal's senses are -- one that has lifted its
/// head at a sound, is running, or has lately been run off -- and it looks
/// all the way round while it is.
const KEEN: f32 = 1.25;

/// How long an animal stays wary after it has lost what it fled from, in
/// seconds.
///
/// **What "grazes warily" is made of.** A deer that has run and stopped is
/// back at the grass, and for forty seconds it hears and sees a quarter
/// further (`KEEN`), stands with its head up for shorter spells, and drifts
/// away from where the threat last was. The player who follows straight in
/// finds a deer that is harder to reach than the first time; the one who
/// waits a minute finds the first deer again.
const WARY_SECONDS: f32 = 40.0;

/// The outer share of a sound's or a smell's reach in which a cue is a
/// *maybe*.
///
/// **Out there, prey lifts its head rather than bolting** (`Mind::Watch`,
/// facing the cue): it has heard something. If it is still there at the next
/// thought, it goes. This is the stalker's warning -- the head coming up --
/// and the answer to it is to stop, which silences a sound (a still person
/// makes none) and does not silence a scent (the wind decides that). A sight
/// or a touch is never a maybe.
const ALERT_BAND: f32 = 0.6;

/// How much of a nose's reach still air leaves, how much a wind adds or
/// takes per unit of its strength along the line from the person, and the
/// least and most any wind makes of it.
///
/// **Still air under half, and the wind moves it both ways**: a gale from
/// the person to the animal carries the scent the whole way and a little
/// more, and the same gale the other way leaves a tenth -- the upwind
/// approach, which is how a hunter gets within a spear of a deer.
const SCENT_STILL: f32 = 0.45;
const SCENT_CARRY: f32 = 0.75;
const SCENT_RANGE: (f32, f32) = (0.1, 1.2);

/// How many line-of-sight rays the whole population may cast in one tick.
///
/// **The one expensive sense, rationed.** A ray is up to forty block reads
/// (`sees`), and it is cast only when touch, hearing and scent have not
/// already settled the matter and the person is inside the eye's reach and
/// cone. Sixteen a tick is three hundred and twenty a second: every one of
/// sixty animals looking twice a second, with room over. An animal whose
/// look the budget refuses asks again next tick (`Perception::Deferred`)
/// rather than deciding it saw nothing.
const RAYS_PER_TICK: u32 = 16;

/// What an animal made of a person this thought.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Perception {
    Nothing,
    /// Something is out there, not sure what: prey lifts its head.
    Maybe,
    /// Seen, touched, or heard or smelled close.
    Sure,
    /// Would have needed a ray, and the tick has none left.
    Deferred,
}

/// A player as the animals take them this tick.
#[derive(Debug, Clone, Copy)]
struct Figure {
    who: PlayerId,
    at: (f32, f32, f32),
    /// Loudness against a walk, everything folded in.
    loudness: f32,
    /// Visibility against a walk in daylight in the open, everything folded
    /// in: gait, cover, darkness, a torch, posture.
    visibility: f32,
    /// Which way they face, if the tick loop said.
    facing: Option<f32>,
    wounded: bool,
    /// In a bed: see `PlayerSign::asleep`.
    asleep: bool,
    /// What is in their hand: see `PlayerSign::held`.
    held: Option<primitive_shared::types::BlockId>,
    /// How far their smell carries: see `PlayerSign::reek`.
    reek: f32,
}

/// One player's movement over the running `GAIT_WINDOW`.
#[derive(Debug, Clone, Copy)]
struct Track {
    who: PlayerId,
    anchor: (f32, f32, f32),
    age: f32,
    speed: f32,
    /// Sitting by a lit fire, as of the last look (`FIRE_LOOK_EVERY`).
    by_fire: bool,
    /// Seconds to the next look.
    fire_look_in: f32,
}

/// How far a player may move in one tick and still be moving rather than
/// put somewhere -- a respawn, a bed, a command -- in blocks. A teleport is
/// not a sprint, and a deer that bolted from a respawn two hundred blocks
/// off would be a deer that heard a number.
const TELEPORT: f32 = 8.0;

// ---- fleeing that looks where it is going ----

/// How far down each heading an escape looks, in blocks.
///
/// **Eight, where the old probe looked three.** Three blocks is half a
/// second at a run: enough to see a cliff edge and not that the gap along a
/// wall is a dead end, so a deer swerved along a wall into its corner and
/// stood there. Eight sees round a short wall and past a pond's end. It is
/// still a probe down a line and not a search -- see `open_heading` for why
/// an animal must not find its way out of a pit.
const ESCAPE_PROBE: f32 = 8.0;

/// How many headings an escape weighs, evenly round: one every twenty-two
/// and a half degrees, fine enough to find the end of a wall eight off.
const ESCAPE_HEADINGS: usize = 16;

/// The weights an escape heading is scored on.
///
/// * `AWAY` -- straight away from the threat. Headings pointing back toward
///   it are not taken at all (`ESCAPE_TOWARD`).
/// * `WANTED` -- toward what the animal was making for: its cover, or its
///   zig-zag. Heavier than `AWAY`, so a deer that has picked a wood runs for
///   the wood.
/// * `KEEP` -- the heading it is already running on. **What stops the
///   circle**: re-scored every burst from a threat that keeps moving, the
///   best heading shifted a little each time and the bursts bent round into
///   a loop back past the player.
/// * `HERD` -- toward its own herd, for a herd animal, when the herd is not
///   the way the threat is: the middle of a herd is where a zebra is safe.
/// * `CLEAR` -- the share of `ESCAPE_PROBE` it could actually run. The
///   heaviest, because a heading into a wall is not an escape however well
///   it scores on the rest.
const ESCAPE_AWAY: f32 = 1.0;
const ESCAPE_WANTED: f32 = 1.2;
const ESCAPE_KEEP: f32 = 0.5;
const ESCAPE_HERD: f32 = 0.5;
const ESCAPE_CLEAR: f32 = 3.0;

/// The most a heading may point back toward the threat and still be taken,
/// as a cosine: a little past a right angle. Sideways along a wall is an
/// escape; back past the person is not, and an animal with nothing else
/// open is cornered, which is a real outcome here.
const ESCAPE_TOWARD: f32 = -0.15;

// ---- herds and packs ----

/// How close to another of its herd an animal stands before it steps away,
/// in blocks: a body and a half. A herd, not a heap.
const PERSONAL_SPACE: f32 = 1.6;

/// How far behind its leader a herd animal lets itself fall while the
/// leader is on the move, in blocks -- and a pack animal, which ranges
/// wider.
const HERD_FOLLOW: f32 = 4.0;
const PACK_FOLLOW: f32 = 5.0;

/// How near the middle of its kind a loose group keeps, in blocks. Twice a
/// herd's: two hares in one meadow are not a herd, and pulling them together
/// as hard as deer made them one.
const LOOSE_COMFORT: f32 = 10.0;

/// ...and a pack, between the two.
const PACK_COMFORT: f32 = 6.0;

/// The ring a pack that has found a person walks round them on while it
/// waits for an opening, in blocks.
///
/// **Nine: out of a spear's reach and well inside a wolf's hearing.** A pack
/// used to stand and watch until somebody came within six blocks and then
/// come all at once, which is two outcomes and no decision. Now it circles,
/// and what it waits for is on the player's side of the ring: turn your back
/// on a wolf and it comes (`BACK_TURNED`), be hurt and they all do. Keeping
/// them in front of you, a wall at your back, is the answer.
const STALK_RADIUS: f32 = 9.0;

/// How far a stalking pack comes from to take an opening, as a share of a
/// wolf's sight.
const OPENING_FRACTION: f32 = 0.65;

/// When a player has their back to an animal: the cosine between where they
/// face and where the animal is. Under this it is behind their shoulders.
const BACK_TURNED: f32 = -0.2;

/// How far a bear's ground reaches from its den, in blocks. Walk inside it
/// and a bear that knows you are there comes, after one look
/// (`Mind::Watch`) -- however far off it was.
const TERRITORY_RADIUS: f32 = 10.0;

/// How far a bear follows anybody from its den before it turns back, in
/// blocks -- a grudge included.
///
/// **The bear's way out.** It is faster than a sprint and its grudge is the
/// longest in the world, so the only answer to a bear used to be never to
/// have met one. Now it is to get off its ground: a bear drives you out and
/// goes home.
const TERRITORY_LEASH: f32 = 24.0;

/// What an animal is doing.
///
/// Seven states. The three the boar added are the reason it stopped
/// being a homing missile. `Charge` used to be entered the moment a
/// player came within seven metres and left only when they left --
/// which meant a boar tracked you like a cursor, hit you every two
/// seconds whatever you did, and made the wood it stood in a place you
/// simply could not go.
///
/// What a boar actually does is *notice*, *commit* and *recover*, and
/// those are the three states: it stops and watches you at a distance,
/// it runs at where you were and cannot correct mid-run, and then it has
/// to turn round. All three are what make it dodgeable, and being
/// dodgeable is what makes it worth fighting.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Mind {
    /// Standing about. Most of an animal's life.
    Idle,
    /// Walking somewhere for no particular reason.
    Wander,
    /// Running away from something.
    Flee,
    /// Stopped, facing somebody, deciding nothing. What a boar does at
    /// six metres, and the state a player should be able to walk away
    /// from.
    Watch,
    /// Committed to a run at a point on the ground. **The point, not
    /// the player**: a charge that steers is a charge nobody can
    /// sidestep. See `Animal::charge_at`.
    Charge,
    /// Running *after* something, re-aiming as it goes.
    ///
    /// **The opposite of `Charge`, and the difference is the whole
    /// reason both exist.** A charge is a commitment: it is aimed once,
    /// it cannot be corrected, and stepping out of the way beats it --
    /// which is what makes a boar fair. A chase is the other thing:
    /// it steers, so it cannot be dodged, only outrun.
    ///
    /// Only a hunter after an animal ever enters it. Aimed at a player,
    /// a state that steers and runs at full speed would be the homing
    /// missile the boar was rebuilt to stop being; aimed at a deer, it is
    /// the only way a wolf ever catches anything, because a deer that has
    /// seen it runs at six metres a second and a stalk does not.
    ///
    /// **...and the monkey, which is the one thing here that chases a
    /// person** (`raid`). It is allowed the homing missile's steering for
    /// the reason the boar is not: what arrives is not a blow. A monkey that
    /// reaches you takes what is in your hand and runs, and every one of the
    /// things that make a charge fair -- see it coming, step aside, back
    /// away -- are answers to it as well; the difference is that ignoring it
    /// costs a dinner rather than a quarter of your health. It is also
    /// slower than a sprinting player (`Species::run_speed`), which a wolf
    /// is not.
    Chase,
    /// Blown, turning round, deciding whether to go again.
    Recover,
    /// Standing at the water's edge with its head down.
    ///
    /// **Standing**, and that is the whole of what the state does to the
    /// body: `walk` gives it no speed, so an animal that has reached the
    /// bank stays on the bank. Nothing of it crosses the wire -- a client
    /// is told a position and a facing (see the module doc) -- so what a
    /// player sees is an animal that walked to the river, stopped there
    /// for four seconds facing the water, and went back to grazing, which
    /// is what drinking looks like from thirty blocks away.
    ///
    /// It is entered only out of `graze`, which is to say only when
    /// nothing else is happening: a thirsty deer with a wolf in sight is
    /// a deer that runs. See `THIRSTY_AT`.
    Drink,
    /// **Flying somewhere on purpose.** The bird's state, and the only
    /// one in this list that is about *going* rather than about
    /// reacting.
    ///
    /// It is `Flee` with the fright taken out, and the three differences
    /// are the whole reason it is not `Flee`. It spends no wind, because
    /// a bird crossing a meadow to its own tree is not bolting and a bird
    /// that arrived home blown would then be caught on the ground. It
    /// costs a cruise rather than a run (`CRUISE_FRACTION`), so the
    /// flight reads as travel. And -- the one that would have been a bug
    /// -- it does not set off the alarm: `survey` startles a covey off
    /// any neighbour in `Flee`, so a bird flying home on `Flee` would
    /// have scattered every bird that could see it, every time, for no
    /// reason any of them could have named.
    ///
    /// Where it is going is `Animal::home`. It ends by *landing*: the
    /// state drops to `Idle` on arrival and `walk`'s altitude spring
    /// (see `FLIGHT_HEIGHT`) puts the bird down on whatever is under it
    /// -- the ground, a canopy, a roof somebody built. Only a species
    /// that `flies` is ever put in it.
    Homing,
    /// **Circling high over a shore.** The gull's state, and the one thing
    /// in this list that is neither going somewhere nor reacting: it is
    /// being somewhere, in the air.
    ///
    /// `Homing` with no destination, and the difference is the steering:
    /// `walk` flies a circle of `SOAR_RADIUS` round `Animal::bound_for`
    /// every tick (`circling`) at a cruise, `SOAR_HEIGHT` over the sand or
    /// the sea. Like `Homing` it spends no wind and sets off no alarm. It
    /// ends when `seabird` picks somewhere to land, or when a dive
    /// (`Animal::dive_for`) takes the bird down to the water and back.
    Soar,
}

/// Health a second while fly agaric is working.
///
/// Two thirds of a point, for the five seconds the paste lasts: about
/// three and a half points in all, which is roughly a third of what
/// the thrust that delivered it does. That is the shape of the thing
/// -- poison shortens a fight, it does not win one -- and it is why
/// two toadstools are worth spending rather than hoarding.
const POISON_PER_SECOND: f32 = 0.66;

/// What one animal can see of its own kind, as of the start of the tick.
///
/// Two facts, and each is one behaviour: where the herd is, so an animal
/// that has drifted out of it comes back, and whether anything nearby
/// has bolted, so a field of deer empties when the first one sees you
/// rather than one deer at a time as the player walks closer.
///
/// Same species only. A hare that scattered because a boar did would be
/// a hare that can read minds across the species barrier, and a boar
/// that drifted toward a herd of deer would be a boar in a herd of deer.
struct Neighbours {
    /// The middle of the animals of this kind within `HERD_RADIUS`, if
    /// there are any.
    centre: Option<(f32, f32)>,
    /// The nearest animal this one *hunts*, and where it is.
    ///
    /// `Species::hunts` decides what counts. This is the whole of the
    /// ecosystem: without it a wolf walks past a deer, because the only
    /// thing any animal here ever knew about was the player.
    quarry: Option<(EntityId, (f32, f32, f32))>,
    /// The nearest animal that hunts *this* one, if it is close enough
    /// to matter -- which is a shorter distance than the hunter's own
    /// reach, because prey notices a predator late. That is what being
    /// prey is.
    threat: Option<(f32, f32, f32)>,
    /// How many of them there are, not counting this one.
    ///
    /// Only the wolf reads it, and for the wolf it is the whole animal:
    /// one alone keeps its distance and three come at you. See
    /// `think_hunter`.
    company: usize,
    /// Which way the nearest bolting neighbour inside `ALARM_RADIUS` is
    /// running, if one is.
    ///
    /// The *heading*, not the threat: a herd runs the same way, and the
    /// animal at the back never has to have seen what the one at the
    /// front saw. Which is also the truth about herds.
    alarm: Option<f32>,
    /// The nearest animal of its own kind inside `HERD_RADIUS`, if
    /// there is one: who it is and where.
    ///
    /// Only the wolf reads it, and only to decide which side of you to
    /// come from -- see `flank`. `centre` is not enough for that: the
    /// middle of a pack of two is halfway between them, which is on
    /// nobody's side.
    packmate: Option<(EntityId, (f32, f32, f32))>,
    /// The one this animal follows, if it is in a herd or a pack and is not
    /// the one in front: where it is, which way it is heading, and whether it
    /// is on the move.
    ///
    /// **The lowest id within `HERD_RADIUS`**, which is the oldest of them --
    /// the first of a spawned group, and the next one along when that one is
    /// eaten. Chosen rather than elected so that every animal in a herd
    /// agrees who leads without asking each other, and so that it never
    /// changes while the herd stands together. What it replaced is a herd
    /// whose every member drifted toward the middle of all the others, which
    /// is a herd that moves only by accident and never *somewhere*.
    leader: Option<Leader>,
    /// For a wild herd's stallion only: the mare furthest out past `STRAY`,
    /// on the ground. See `Animal::stallion`.
    straggler: Option<(f32, f32)>,
}

/// What a follower reads off its leader. See `Neighbours::leader`.
#[derive(Debug, Clone, Copy, PartialEq)]
struct Leader {
    at: (f32, f32, f32),
    heading: f32,
    moving: bool,
}

/// A place an animal was hurt, and how long it will go on minding.
///
/// Two coordinates rather than three because the memory is of a
/// *meadow*, not a cell: a deer shot from the top of a bank remembers
/// the bank, and a deer that only remembered the exact block would walk
/// back to the block beside it.
#[derive(Debug, Clone, Copy, PartialEq)]
struct Danger {
    at: (f32, f32),
    /// Seconds until it is forgotten. Counts down every tick -- see
    /// `think` -- and the entry goes when it reaches zero.
    left: f32,
}

pub struct Animal {
    pub id: EntityId,
    pub species: Species,
    /// Feet, in world space, at the centre of the collider's base.
    ///
    /// **`f64`, and only moved in `f64`.** The feet were an `f32`, and a deer a
    /// million blocks out walked in sixteenths: it stood still for a tick and
    /// jumped the next, and at ten million it moved a whole block or nothing
    /// and could be put inside the wall it was walking along. Where it goes
    /// and what it bumps into are worked out on this (`walk`, `swim`, `fits`);
    /// what it *decides* reads [`Animal::at`], because a choice of where to
    /// run is not changed by a sixteenth.
    pub position: (f64, f64, f64),
    velocity: (f32, f32, f32),
    /// Which way it is facing, in radians. Kept rather than derived from
    /// velocity, because an animal that has stopped is still looking
    /// somewhere and a box that snaps to face north when it halts reads
    /// as a bug.
    yaw: f32,
    /// Which way it *wants* to be facing.
    ///
    /// Two fields rather than one because a turn takes time: decisions
    /// write this one, and `walk` moves `yaw` toward it at `TURN_RATE`.
    /// Writing the facing directly is what made an animal change
    /// orientation in a single frame.
    wants_yaw: f32,
    health: f32,
    /// This animal's own pace, as a multiplier on its species' speed.
    ///
    /// **A herd that moves in lockstep is a herd of one animal drawn
    /// several times.** Every deer walked at exactly 1.9 blocks a
    /// second, turned at exactly the same rate and stopped at the same
    /// moment; what a player saw was a formation. A tenth either way is
    /// enough to break that up and small enough that nothing about
    /// catching one changes.
    pace: f32,
    /// How much its heading wanders while it walks, in radians a second.
    ///
    /// Re-rolled at every thought. A wander used to be a dead straight
    /// line held for up to five seconds, which reads as a thing on
    /// rails; a line that bends slightly reads as an animal picking its
    /// way.
    drift: f32,
    mind: Mind,
    /// Seconds until it thinks again.
    next_thought: f32,
    /// Whoever it is running from or at, and how sure it is.
    target: Option<primitive_shared::protocol::PlayerId>,
    /// Seconds since it was struck, for the flash the client draws.
    hurt_for: f32,
    /// Seconds until it can gore somebody again.
    gore_cooldown: f32,
    /// Where a charge is aimed, on the ground.
    ///
    /// Fixed at the moment the charge starts and not touched again,
    /// which is the whole mechanic: the boar runs *through* the place
    /// you were standing, and stepping out of the way works.
    charge_at: Option<(f32, f32)>,
    /// Seconds of anger left. Set by being hit, and counted down
    /// wherever the boar happens to be -- see `Species::grudge_seconds`.
    ///
    /// **A monkey spends it the other way round**, staying away instead of
    /// coming at you, and it is the same fact about the animal: this person
    /// and I have had words. See `raid`.
    angry_for: f32,
    /// Whose dinner it has just taken, for the tick loop to settle against
    /// their pack (`Animals::take_thefts`).
    ///
    /// **Here rather than a blow.** A theft is not damage and must not go
    /// through `Blow`: what it touches is an inventory, which the animals
    /// module has never been given and is not being given now (see the note
    /// on `Neighbours::centre` about mechanics that can reach everything).
    /// It is drained the way dung and a dead horse's bags are, which is the
    /// pattern this module already has for "something happened that the
    /// server has to finish".
    stole_from: Option<PlayerId>,
    /// Seconds until it is hungry again. Only a predator ever has any.
    ///
    /// A stomach rather than an appetite: a wolf that has eaten stops
    /// hunting outright -- it does not hunt *less* -- which is what
    /// keeps a pack from clearing every deer inside the spawn radius and
    /// then standing about in an empty field. See
    /// `Species::fed_seconds`.
    fed_for: f32,
    /// What it is hunting, when that is an animal rather than a person.
    ///
    /// Separate from `target`, which is a `PlayerId`. Both can be set --
    /// a wolf that has been speared while chasing a deer has a grudge
    /// and a dinner -- and the player wins, because the thing that is
    /// hitting you is more urgent than the thing you were eating.
    quarry: Option<EntityId>,
    on_ground: bool,
    /// Highest point reached since it was last on the ground. `None`
    /// while standing on something -- the same shape `Vitals::fall_peak_y`
    /// carries for a player, and read the same way: a landing with a
    /// peak on record is a fall, and one without is just a step.
    fall_peak_y: Option<f32>,
    /// Seconds of breath left, counted down while its head is under
    /// water. The animal counterpart of `Vitals::breath` -- these
    /// species do not swim, so buoyancy alone gets them back to the
    /// surface after almost any dip, and this is what charges the ones
    /// that cannot get there: a body of water with no air pocket over
    /// it, or a ceiling that traps them under the surface.
    breath: f32,
    /// The patch of cover a fleeing animal is making for, on the
    /// ground, if it has picked one. See `bolt` and `cover_heading`.
    ///
    /// Kept between bursts rather than re-derived at each, so a deer
    /// that chose a wood keeps running at *that* wood -- a search that
    /// re-ran every two and a half seconds picked a different, nearer
    /// tree each time and the deer described an arc through the meadow
    /// instead of leaving it. Cleared when it arrives, when it stops,
    /// and when it is hit (a blow means the plan was wrong).
    cover: Option<(f32, f32)>,
    /// How long it has been out of its pursuer's sight while fleeing,
    /// in seconds. Zero whenever it has just been seen. See `sees` and
    /// `HIDDEN_SECONDS`.
    hidden_for: f32,
    /// Ticks, for rate-limiting the line-of-sight check -- see
    /// `LOOK_EVERY`. Seeded from the id so a herd does not all look on
    /// the same tick. Wraps; only ever compared modulo `LOOK_EVERY`.
    looks: u8,
    /// Which side of the flight line the next burst is aimed at: plus
    /// or minus one. Flipped every bolt. Only a hare reads it -- see
    /// `HARE_DODGE`.
    dodge: f32,
    /// Where it, or its herd, was hurt, and for how much longer it
    /// minds. Empty for nearly every animal that ever lives; see
    /// `Danger` and `remember_danger`.
    dangers: Vec<Danger>,
    /// Seconds of thirst it has built up. Grows every tick, faster while
    /// it is running (`RUNNING_THIRST`), and falls while it drinks.
    ///
    /// A meter rather than a flag, for the reason `fed_for` is one: a
    /// yes-or-no thirst is an animal that is either at the water or
    /// walking away from it, and the interesting state is the one in
    /// between -- half slaked, startled off the bank, and coming back.
    thirst: f32,
    /// The water it is making for, on the ground, if it has found any.
    ///
    /// Kept between thoughts rather than searched for at each, exactly as
    /// `cover` is and for the same reason: a search re-run every second
    /// picks a slightly different cell of the same lake each time, and
    /// the animal walks a curve to a pond that is straight ahead of it.
    water: Option<(f32, f32)>,
    /// Seconds until it may pay for a water search again. See
    /// `WATER_SCAN_INTERVAL`, which is where the cost of the whole
    /// mechanic is argued.
    next_water_scan: f32,
    /// Where its nest is, on the ground. Only a bird ever has one.
    ///
    /// Two coordinates, exactly as `cover` and `water` are, and for the
    /// same reason: how high it is is not a decision, it is whatever
    /// `walk`'s altitude spring finds under that column. So a nest in a
    /// canopy and a perch on the turf are one field.
    ///
    /// **Kept for life once it is found.** A nest a player has chopped
    /// down is a place the bird still flies back to, sits at, and forages
    /// round -- which is a bird returning to where its nest was, and is
    /// both what a bird does and cheaper than re-checking the column
    /// every thought. What it is *not* allowed to be is stale in a way
    /// that costs anything: nothing reads it but `homing`, and `homing`
    /// only ever steers.
    home: Option<(f32, f32)>,
    /// Where the flight it is on is aimed, if it is on one.
    ///
    /// **Separate from `home`, and the hop is why.** The nest is the
    /// anchor -- what the bird comes back to and forages round -- and
    /// this is the destination of whichever leg it is flying now: the
    /// nest itself, a nest it has just found, or a perch a few blocks
    /// off (`HOP_RANGE`). Folding the two into one field was the first
    /// shape, and it made hops impossible: the bird set off on a hop, and
    /// its very next thought found it was away from home and turned it
    /// round, so every hop was a flight of one thought that ended where
    /// it started.
    bound_for: Option<(f32, f32)>,
    /// The surface the current flight is *aimed at the top of*, in
    /// blocks, taken once when the flight is planned.
    ///
    /// **A bird has to be above the tree before it gets to the tree.**
    /// Flight height is otherwise measured from whatever is directly
    /// under the animal (`FLIGHT_HEIGHT` over `surface_under`), which is
    /// the meadow -- so a bird whose nest was in a crown five blocks up
    /// crossed the field at three and a half, arrived *underneath* its
    /// own tree, could not climb through the leaves, and came down on the
    /// turf in its shade. Every trip. Holding the destination's height
    /// for the length of the flight makes the bird climb over the wood
    /// while it is still over open ground, which is also what a bird
    /// does.
    ///
    /// One `surface_under` at the destination column per flight -- a
    /// couple of dozen block reads every few seconds for a bird that is
    /// actually going somewhere, which is a rounding error beside the
    /// search at `NEST_SCAN_INTERVAL`. It is only read while
    /// `Mind::Homing`; a
    /// fright still climbs to `FLIGHT_HEIGHT` over whatever is below,
    /// because a flushed bird has no destination to be above.
    flight_ceiling: f32,
    /// Seconds until it may pay for a nest search again. See
    /// `NEST_SCAN_INTERVAL`, which is where that cost is argued.
    next_home_scan: f32,
    /// Seconds of running it has left before it is blown.
    ///
    /// Counted down while it is actually running and back up while it is
    /// not (`STAMINA_RECOVERS`); at zero it keeps running at
    /// `BLOWN_SPEED` rather than stopping, because an animal that halts
    /// dead in front of a wolf is a corpse and an animal that slows is a
    /// chase with an end to it. See `Species::stamina_seconds`.
    stamina: f32,
    /// The bearing round the player a circling wolf is working toward,
    /// in radians, while it is circling. See `flank`.
    ///
    /// Chosen once, when the engagement starts, and held until it
    /// charges. The first version re-derived it every thought from
    /// wherever the packmate was *now* -- and the packmate is a wolf
    /// that charges through you and out the other side every three
    /// seconds, so "opposite it" flipped sides every three seconds and
    /// the follower ran back and forth along an arc and never came in.
    flank_to: Option<f32>,
    /// Seconds of fly agaric still working, and how much of it is left
    /// to give.
    ///
    /// **A countdown rather than a wound**, because that is what a
    /// poison is: the spear did its damage when it landed and this is
    /// the part that arrives afterwards. Ticked in `think`, which every
    /// animal runs anyway, so a poisoned animal costs one subtract a
    /// tick and an unpoisoned one costs one compare.
    poison_for: f32,
    /// Seconds left of a gull's dive at the water. Only a species that
    /// `soars` is ever given any; `walk` reads it to aim the altitude spring
    /// at the top of the sea instead of `SOAR_HEIGHT` over it.
    dive_for: f32,
    /// Seconds spent coming in to land, for `LANDING_GIVE_UP_SECONDS`.
    /// Counted in the tick, not in a thought, and zero in any other state.
    homing_for: f32,
    /// The height a swimming body is making for, as the y of its feet.
    /// Only a species that `swims` reads it.
    ///
    /// **A target rather than gravity**, the bird's trick under water: a
    /// fish neither sinks nor floats, it holds a depth, and a depth it is
    /// pulled toward at a capped rate is a whole vertical behaviour in one
    /// number -- a school drifting up over a reef, diving when a swimmer
    /// comes down on it -- without a buoyancy model to keep in step with the
    /// player's. Chosen by `think_fish`, inside the water column the fish is
    /// in, and reset to where it is whenever the floor or the surface
    /// refuses the move (`swim`).
    swim_depth: f32,
    /// Seconds of wariness left: see `WARY_SECONDS`. Set when a flight ends
    /// and when prey lifts its head at a maybe; while it lasts the senses are
    /// `KEEN` and the eyes look all the way round.
    wary_for: f32,
    /// Where it last knew the thing it ran from to be, on the ground.
    ///
    /// **Remembered, because running away from where the threat is *now* is
    /// a thing only an animal that can still see it can do.** A deer that had
    /// lost you used to have no idea which way was away; its next wander was
    /// as likely to walk it back to you as not. Kept through the wary spell,
    /// so grazing drifts off from here.
    threat_at: Option<(f32, f32)>,
    /// How fast its yaw is turning, in radians a second, for a bird in the
    /// air. See `AIR_ROLL`: a wing banks into a turn and out of it, so the
    /// rate itself cannot jump.
    turn_rate: f32,
    /// Where this bird is in its own climb-and-glide, in seconds, wrapped
    /// into `AIR_CYCLE`. Advanced only while it is in the air.
    ///
    /// **Per bird and not per world**, and that is the whole of what makes a
    /// flock read as a flock rather than as one bird drawn six times: a clock
    /// everything shared would have every gull over the beach topping its
    /// climb on the same tick. Seeded from the ordinal the way `thirst` and
    /// `next_home_scan` are, so a seeded world still plays out the same.
    ///
    /// Only while it is up: a bird standing on the sand does not resume its
    /// glide half way through when it takes off again, it starts a fresh
    /// climb, which is what a take-off is.
    air_phase: f32,
    /// What its body is doing, for everybody who can see it. See
    /// [`primitive_shared::protocol::Attitude`], which carries the argument.
    ///
    /// **A field rather than a function of `mind`**, because the two are not
    /// the same question and the cases that differ are the interesting ones:
    /// `Mind::Idle` is a deer with its head in the grass, a deer with its head
    /// up between mouthfuls, and a deer asleep at three in the morning, and
    /// those are three pictures. Written where the decision is made -- `graze`
    /// knows what it is standing in, `think_hunter` knows it is stalking --
    /// and read once a tick by `state`.
    attitude: primitive_shared::protocol::Attitude,
    /// How grown it is: nought at birth, `youth::GROWN` for an adult, which
    /// is what the spawner makes. Everything a young animal does differently
    /// reads this through `primitive_shared::youth` -- see that module for
    /// why a fawn is a deer with a number rather than a species.
    growth: f32,
    /// Its mother, while it is young. See `keep_family`.
    ///
    /// **Both ends of the link are kept**, the young's here and the mother's
    /// in `young`, because each end asks a different question every tick --
    /// "where is she" and "where is it" -- and a search of the whole list for
    /// the other end is what the link exists to avoid. A link to somebody who
    /// is gone (killed, forgotten) simply resolves to nobody in `family`, and
    /// is cut when it next grows or gives birth.
    mother: Option<EntityId>,
    /// Her young, while it is young.
    young: Option<EntityId>,
    /// In-game days before she can give birth again: `youth::BIRTH_REST_DAYS`
    /// after each birth.
    birth_rest: f32,
    /// The most it may run this tick, in blocks a second: infinite except for
    /// a mother keeping to her young's pace. Written by `keep_family` every
    /// tick, read by `walk`.
    speed_cap: f32,
    /// Seconds of its fall left, once it is dead. See `FALL_SECONDS`; only a
    /// body in `Animals::falling` has any.
    dying_for: f32,
    /// What people have done to it: `None` for every wild animal nobody has
    /// fed. See `husbandry::Keeping` -- and `forget_the_distant`, which parks
    /// an animal with one of these instead of forgetting it.
    keep: Option<husbandry::Keeping>,
    /// **The herd's stallion**: one of each wild herd of horses, the last of
    /// the group to be spawned. It fetches back a mare that has strayed
    /// (`Neighbours::straggler`), which is what "a stallion keeps the herd"
    /// means on the ground -- a herd that drifts apart over a quarter of an
    /// hour is a herd with no stallion. Meaningless once it is tamed.
    stallion: bool,
    /// A kept horse's saddle, bags and breaking (`horse::Gear`). Boxed because
    /// one animal in a hundred has any, and the bags are an inventory.
    gear: Option<Box<horse::Gear>>,
    /// Somebody on its back, and the body their reins move. See `carry`.
    ride: Option<Ride>,
    /// Seconds before a gentled horse that has just thrown somebody lets
    /// anybody try its back again (`husbandry::SETTLE_SECONDS`).
    settle_for: f32,
}

/// A horse with a rider: the body `horse::step` moves, and what the reins
/// last said.
///
/// **The body is its own and not the animal's `position`**, for the raft's
/// reason: it is the thing the rider's client predicts with the same
/// numbers, and `walk`'s collider -- the animals' own, with its step and its
/// swim -- is not. So a ridden horse is moved by `carry` alone, and its
/// position is copied out of this every tick.
#[derive(Debug, Clone, Copy)]
struct Ride {
    rider: PlayerId,
    body: horse::Mount,
    reins: horse::Reins,
    /// Seconds since the reins were last sent. See `REINS_TIMEOUT`.
    reins_age: f32,
    /// Seconds a jump asked for is still waiting to be taken.
    ///
    /// **Latched, and not read off the latest reins**, because a jump is a
    /// press and the reins are a state: the client sends "jump" on the frame
    /// the key goes down and "no jump" on the next, and both arrive inside one
    /// tick -- so a server that read the reins as they stood at its tick never
    /// jumped at all, while the rider's own horse cleared the ditch and the
    /// server's fell into it. Held for `JUMP_LATCH` and spent by the jump.
    jump_for: f32,
}

/// How long a jump asked for waits for the horse's feet to be under it.
///
/// A little over two ticks: long enough that a press between two ticks is
/// taken, short enough that a jump asked in mid-air is not a second jump on
/// landing that nobody asked for then.
const JUMP_LATCH: f32 = 0.12;

/// What a dead horse left, and where: see `Animals::take_spilled`.
pub type Spilled = ((f64, f64, f64), Vec<(primitive_shared::types::BlockId, u32, u32)>);

/// How long a rider's last reins keep asking without another.
///
/// The client sends them every quarter second while the horse is asked to
/// move. A second of silence is a client that went away mid-gallop, and a
/// horse that galloped on across the plain with nobody asking would be the
/// raft's ghost at the oars (`rafts::OARS_TIMEOUT`) with legs.
const REINS_TIMEOUT: f32 = 1.0;

/// How far off a stallion notices one of its mares has strayed, in blocks.
const STALLION_RANGE: f32 = 26.0;

/// How far from the stallion a mare is before it goes and fetches her.
///
/// **Past the herd's own comfort and short of its radius**, so the rules
/// hand over rather than fight: inside `HERD_COMFORT` the mare's own herd
/// rules keep her, and a mare wandering out toward `HERD_RADIUS` -- where
/// no herd rule reaches -- is the one the stallion goes for.
const STRAY: f32 = 9.0;

/// What a horse brings to a ride, off its keeping and its gear: see
/// `horse::Fettle`.
fn fettle_of(keep: Option<&husbandry::Keeping>, gear: Option<&horse::Gear>) -> horse::Fettle {
    horse::Fettle {
        most_wind: keep.map_or(horse::GALLOP_SECONDS, |k| k.most_wind()),
        // **Saddled and fed.** Bareback is a walk and a trot (`Gear::saddle`),
        // and a hungry horse is the same (`Keeping::will_gallop`).
        will_gallop: gear.is_some_and(|g| g.saddle) && keep.is_none_or(|k| k.will_gallop()),
        load_kg: gear.map_or(0.0, |g| g.load_kg()),
    }
}

/// Where one of an animal's own family is, as of the start of the tick.
///
/// Worked out for everybody before anything moves, for `Neighbours`' reason:
/// a mother read while the list is being walked is a mother in two places.
#[derive(Debug, Clone, Copy)]
struct Kin {
    at: (f32, f32, f32),
    /// Running from something: see `keep_family`.
    fleeing: bool,
    /// Charging, chasing, squaring up -- a mother in a fight, whose young
    /// looks after itself rather than following her into it.
    fighting: bool,
    /// Which way it is going, in radians, off its velocity.
    heading: f32,
    /// How fast it can run, in blocks a second, with its youth and its own
    /// pace in it.
    run: f32,
}

/// An animal's family: its mother if it is young, its young if it is a
/// mother. Both `None` for almost every animal alive.
#[derive(Debug, Clone, Copy, Default)]
struct Family {
    mother: Option<Kin>,
    young: Option<Kin>,
}

/// The size of the box an animal is, for the collision tests.
///
/// **A size rather than a species**, because a young animal is its species at
/// `youth::size` of it: `fits` asked the species and every fawn collided as a
/// full-grown deer -- it could not go under anything its mother could not,
/// and it stood a full body's width off its neighbours. `From<Species>` keeps
/// every caller that has only a species (the spawner, the tests) as it was.
#[derive(Debug, Clone, Copy)]
struct Frame {
    width: f32,
    height: f32,
}

impl From<Species> for Frame {
    fn from(species: Species) -> Frame {
        Frame { width: species.width(), height: species.height() }
    }
}

/// One animal's bite landing on another.
///
/// Collected during the tick and applied after it, for the reason the
/// blows on players are: the loop holds `&mut` on the animal doing the
/// biting, and the one being bitten is in the same list.
#[derive(Debug, Clone, Copy)]
struct Bite {
    at: EntityId,
    damage: f32,
    /// Where it came from, so the bitten animal knows which way to run.
    from: (f32, f32, f32),
}



impl Animal {
    /// Where the feet are, narrowed for the decisions: see `position`.
    #[inline]
    pub fn at(&self) -> (f32, f32, f32) {
        primitive_shared::geometry::narrow(self.position)
    }

    /// How big it is, as a fraction of its species: `youth::size`.
    #[inline]
    fn size(&self) -> f32 {
        youth::size(self.growth)
    }

    /// The box it collides as: its species', at its size. See `Frame`.
    #[inline]
    fn frame(&self) -> Frame {
        Frame { width: self.species.width() * self.size(), height: self.species.height() * self.size() }
    }

    /// Does it fight, rather than run?
    ///
    /// **Its species, unless it is young.** A piglet is a boar's young and
    /// not a boar: it has no tusks to speak of and no business charging a
    /// person, so it runs like a deer (and its mother is the one who fights
    /// -- see `strike`). Everything in `think` that used to ask the species
    /// asks this, so a young boar takes the prey's path through every rule.
    #[inline]
    fn fights(&self) -> bool {
        self.species.is_hostile() && !youth::is_young(self.growth)
    }

    pub fn state(&self) -> EntityState {
        EntityState {
            id: self.id,
            kind: EntityKind::Animal {
                species: self.species,
                yaw: self.yaw,
                hurt: (self.hurt_for / HURT_SECONDS).clamp(0.0, 1.0),
                attitude: self.attitude_now(),
                growth: youth::to_wire(self.growth),
                tack: self.tack(),
            },
            x: f64::from(self.at().0),
            // The *centre* of the animal, which is what the client draws
            // a box around -- the same convention a dropped item uses.
            // Sending the feet would mean every client had to know every
            // species' height to put the box in the right place, which is
            // a second copy of a number that already crosses the wire as
            // the species.
            y: f64::from(self.at().1 + self.frame().height * 0.5),
            z: f64::from(self.at().2),
        }
    }

    /// The `horse::TACK_*` bits for everybody who can see it.
    fn tack(&self) -> u8 {
        let tame = self.keep.is_some_and(|k| k.tame);
        self.gear.as_ref().map_or(0, |g| g.tack())
            | if self.ride.is_some() { horse::TACK_RIDDEN } else { 0 }
            | if tame { horse::TACK_HALTER } else { 0 }
            | if self.stallion && !tame { horse::TACK_STALLION } else { 0 }
            | if self.shorn() { horse::TACK_SHORN } else { 0 }
    }

    /// A sheep with its fleece off and not yet grown back: what the model
    /// draws close-cropped (`horse::TACK_SHORN`) and what a carcass gives no
    /// wool for (`Animals::kill`'s drops).
    fn shorn(&self) -> bool {
        self.species == Species::Sheep && self.keep.is_some_and(|k| !k.fleece_ready())
    }

    /// What its body is doing right now, as [`Animal::attitude`] says, with
    /// the one override that cannot be left to the decision sites.
    ///
    /// **An animal that is moving is not grazing, whatever its last thought
    /// said.** `graze` writes `Feeding` and then something startles the herd
    /// two ticks later through `bolt`, which is reached from five places and
    /// would have had to clear the field in every one of them; forget it in
    /// one and a deer runs across the meadow with its nose in the grass. The
    /// test is speed rather than `mind`, for the reason the charge's commit
    /// test is speed: it is the physical fact, and it cannot be out of step
    /// with a state list somebody adds to later.
    ///
    /// The floor is the client's own (`animal_model::STANDING`): below it the
    /// legs are not swinging either, so the two agree about what standing is.
    fn attitude_now(&self) -> primitive_shared::protocol::Attitude {
        use primitive_shared::protocol::Attitude;
        let moving = self.velocity.0.hypot(self.velocity.2) > STANDING_STILL;
        match self.attitude {
            Attitude::Feeding | Attitude::Drinking | Attitude::Dozing if moving => Attitude::Easy,
            other => other,
        }
    }

    /// Where a swing at this animal has to land -- centre and radius.
    pub fn hit_box(&self) -> ((f32, f32, f32), f32) {
        (
            (
                self.at().0,
                self.at().1 + self.frame().height * 0.5,
                self.at().2,
            ),
            self.frame().height.max(self.frame().width) * 0.5,
        )
    }

    /// How far a point is from this animal's *box*, in its own frame.
    ///
    /// Zero inside it. **The box rather than a sphere, and the animal's
    /// frame rather than the world's**, because these creatures are all
    /// about twice as long as they are wide: a sphere big enough to hold
    /// a deer end to end is also that wide, and one sized to its width
    /// leaves its head and tail outside. The client aims at the drawn
    /// silhouette (see `entities::aimed_at`), so a swing at a deer's
    /// flank connected on screen and missed on the wire.
    ///
    /// The box comes from `Species::half_extents`, which is the shared
    /// answer both sides use.
    fn distance_to_box(&self, from: (f32, f32, f32)) -> f32 {
        let (half_x, half_y, half_z) = self.species.half_extents();
        // ...at its size: a fawn is not hit a foot beside itself.
        let (half_x, half_y, half_z) = (half_x * self.size(), half_y * self.size(), half_z * self.size());
        // Centred where the client draws it: on the position the
        // snapshot carries, which is the animal's own middle in the
        // model's frame (see `animal_model::append_part`). The two have
        // to be the same box or a swing lands on one side and not the
        // other.
        let centre = (self.at().0, self.at().1, self.at().2);
        let (dx, dy, dz) = (from.0 - centre.0, from.1 - centre.1, from.2 - centre.2);
        // Into the animal's frame: turn the offset by minus its yaw, so
        // "along" is always the same axis whichever way it is facing.
        let (sin, cos) = (-self.yaw).sin_cos();
        let along = dx * cos - dz * sin;
        let across = dx * sin + dz * cos;
        let outside = (
            (along.abs() - half_z).max(0.0),
            (dy.abs() - half_y).max(0.0),
            (across.abs() - half_x).max(0.0),
        );
        (outside.0 * outside.0 + outside.1 * outside.1 + outside.2 * outside.2).sqrt()
    }

    /// Where it looks from: nine tenths of the way up, in the middle.
    ///
    /// Used at both ends of a line of sight -- what an animal sees from,
    /// and (for a player, with `EYE_HEIGHT` instead) what it looks at.
    /// The feet would put the ray through every tuft of grass on the
    /// meadow; the top of the head would see over a wall the animal's
    /// eyes are below.
    fn eye(&self) -> (f32, f32, f32) {
        (
            self.at().0,
            self.at().1 + self.frame().height * 0.9,
            self.at().2,
        )
    }

    /// Writes down a place it was hurt.
    ///
    /// **A second blow near an old one lengthens the memory rather than
    /// adding a second entry.** That is what "struck twice at the same
    /// water" means to the herd: one strike is bad luck and the meadow
    /// is worth coming back to in five minutes; two is a hunter who has
    /// found a spot, and the herd moves its range for twice as long.
    /// The `min` keeps a long fight from making a place taboo for an
    /// hour -- an animal that never forgets is one the player has to
    /// kill to be rid of (compare `Species::grudge_seconds`).
    fn remember_danger(&mut self, at: (f32, f32)) {
        let same = self
            .dangers
            .iter_mut()
            .find(|d| (d.at.0 - at.0).hypot(d.at.1 - at.1) <= SAME_PLACE);
        if let Some(danger) = same {
            danger.left = (danger.left + DANGER_MEMORY).min(DANGER_MEMORY * 2.0);
            return;
        }
        if self.dangers.len() >= DANGERS_REMEMBERED {
            // The one it minds least makes room.
            if let Some((index, _)) = self
                .dangers
                .iter()
                .enumerate()
                .min_by(|a, b| a.1.left.total_cmp(&b.1.left))
            {
                self.dangers.swap_remove(index);
            }
        }
        self.dangers.push(Danger { at, left: DANGER_MEMORY });
    }

    /// The remembered danger nearest to where it is standing, as an
    /// offset from it, if one is inside `DANGER_RADIUS`.
    fn danger_underfoot(&self) -> Option<(f32, f32)> {
        self.dangers
            .iter()
            .map(|d| (d.at.0 - self.at().0, d.at.1 - self.at().2))
            .filter(|(dx, dz)| dx * dx + dz * dz <= DANGER_RADIUS * DANGER_RADIUS)
            .min_by(|a, b| (a.0 * a.0 + a.1 * a.1).total_cmp(&(b.0 * b.0 + b.1 * b.1)))
    }

    /// Is this a hostile animal in the second after a charge -- blown,
    /// and turning round?
    ///
    /// **Public, and pure**, so that whoever works out what a blow is
    /// worth (`lib::hunting_damage` and its caller) can ask the same
    /// question `strike` already answers for itself through
    /// `exposed_back`. `Recover` is the only state it is true in: a
    /// charge that is still running is a straight line moving away from
    /// you at speed, and a watching boar is facing you.
    pub fn is_turning(&self) -> bool {
        self.species.is_hostile() && self.mind == Mind::Recover
    }

    /// Is `from` behind this animal while it is turning round?
    ///
    /// Behind is the mirror of `GORE_ARC`: more than a hundred and
    /// twenty degrees off the nose, so the sixty degrees either side of
    /// its tail. The moment it exists for is the one after a sidestep --
    /// the boar has run through where you were and has its back to
    /// where you are -- and `strike` charges `BACKSTAB` for it. A player
    /// standing beside a turning boar is not behind it; a player it has
    /// already turned to face is not either, which is why the window is
    /// about a second long and not the whole of `Recover`.
    pub fn exposed_back(&self, from: (f32, f32, f32)) -> bool {
        if !self.is_turning() {
            return false;
        }
        let (dx, dz) = (from.0 - self.at().0, from.2 - self.at().2);
        let flat = (dx * dx + dz * dz).sqrt();
        if flat < 1e-4 {
            return false;
        }
        let (sin, cos) = self.yaw.sin_cos();
        (dx * cos + dz * sin) / flat < -GORE_ARC
    }
}

/// A blow an animal landed on somebody.
///
/// Carries the species because the message a player reads has to name
/// the animal that killed them, and the tick loop has no other way to
/// find out which one it was: by the time it reads this list the
/// simulation has moved on.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Blow {
    pub victim: primitive_shared::protocol::PlayerId,
    pub damage: f32,
    pub species: Species,
}

/// What happened to an animal that was struck.
#[derive(Debug, Clone, PartialEq)]
pub enum Struck {
    /// Nothing there, or out of reach.
    Missed,
    /// Hurt, and still standing.
    Hurt,
    /// Dead. Carries *what* died and where, and nothing about what to
    /// leave on the ground: that is decided by whoever handles the death
    /// (`lib::lay_carcass`), because it depends on the cell it fell
    /// in -- a carcass where there is ground for one, the old heap of
    /// meat over water. This used to carry `Species::drops` directly,
    /// which made the heap the only possible outcome.
    Killed { species: Species, at: (f32, f32, f32) },
}

/// Everything alive in the world.
pub struct Animals {
    animals: Vec<Animal>,
    /// How many animals have ever been born here. **Not the id they are
    /// replicated under** -- see `protocol::EntitySource` -- and
    /// deliberately still the number `spread` is hashed from, so that
    /// giving ids a source did not quietly re-roll the pace and the
    /// drift of every animal in every seeded world.
    next_ordinal: u64,
    /// Seconds until the spawner tries again.
    next_spawn: f32,
    spawned: u64,
    killed: u64,
    despawned: u64,
    rng: Rng,
    /// Who appeared and who went, since the tick loop last asked.
    ///
    /// **A queue rather than a callback**, and that is the whole of why
    /// it exists. Spawning and despawning both happen inside `step`,
    /// with this structure's mutex held, and nothing may call into a mod
    /// with a lock held (see `logic::mods`). So the ids are written down
    /// here and the tick loop drains them once the lock is gone, which
    /// is the same shape the blows already use.
    births: Vec<(EntityId, (f32, f32, f32))>,
    deaths: Vec<EntityId>,
    /// Deaths the world dealt rather than anybody's blow: a fall, a fire,
    /// held breath, the last of a poisoned spear's paste. See
    /// `take_fallen`.
    fallen: Vec<Death>,
    /// Where a body was driven onto sharpened stakes this step, for the tick
    /// loop to tell everybody near (`ServerMessage::Staked`) -- drained like
    /// `births`, and for their reason: the step holds this lock, and the
    /// telling takes the registry's.
    staked: Vec<(f64, f64, f64)>,
    /// The dead, going down: bodies kept for `FALL_SECONDS` after the blow,
    /// moved by nothing but gravity and their last shove, and sent with
    /// `Attitude::Dying`. **A list of their own**, and that is what makes a
    /// dying body inert without a test in every rule: nothing that walks the
    /// living -- `survey`, `strike`, `within`, the spawner's ceiling -- ever
    /// sees one. When the fall is over the body goes into `fallen` with the
    /// place it came to rest, which is where its carcass is laid.
    falling: Vec<Animal>,
    /// The world's day count as the tick loop last gave it (`calendar`), and
    /// the days that have gone by since the step last used them. `None` until
    /// the loop says: an `Animals` nobody gives a calendar to -- most tests --
    /// has no young growing and none born.
    calendar: Option<f32>,
    days_pending: f32,
    /// The season, off the same calendar: young come in the spring.
    season: primitive_shared::season::Season,
    /// Who is holding a lit torch, as of the last time the tick loop
    /// said. See `carrying_fire`.
    fire_bearers: Vec<PlayerId>,
    /// What the tick loop last said about each player -- facing, working,
    /// posture, health. See `PlayerSign` and `player_signs`.
    signs: Vec<PlayerSign>,
    /// Every player's movement, for their gait. See `Track` and `GAIT_WINDOW`.
    tracks: Vec<Track>,
    /// The wind, as (x, z) scaled by its strength -- `raft::Wind::vector`.
    /// Calm until the tick loop says otherwise (`feel_wind`).
    wind: (f32, f32),
    /// **Kept animals nobody is near**, with the world day each was left on.
    ///
    /// Out of the simulation, off the wire, and not forgotten: see
    /// `forget_the_distant`, which puts them here, and `unpark`, which brings
    /// them back when somebody comes within `UNPARK_DISTANCE` -- with the
    /// days they were alone in them (`Keeping::pass_days`), so a flock left
    /// for a week is a flock that was not fed for a week.
    parked: Vec<(Animal, f32)>,
    /// Where a kept animal left a pat of dung this step, for the tick loop to
    /// write into the world -- this file cannot (see `staked` for the shape).
    dung: Vec<(f64, f64, f64)>,
    /// A bite out of the haystack in this cell, one entry a bite, for the
    /// tick loop to take out of the world (see `dung` for why this file
    /// cannot). See `Keeping::winter_through`.
    hay_eaten: Vec<(i32, i32, i32)>,
    /// Counts `keep_the_kept`'s calls, so a hungry animal with no stack in
    /// reach looks for one every `MANGER_LOOK_EVERY` of them and not every
    /// tick. See `stacks_in_reach`.
    manger_clock: u32,
    /// The night a raid on the pens was last rolled for: see `raid_the_pens`.
    raided: Option<i64>,
    /// The night a sleeper was last found: see `find_the_sleeper`.
    found_sleeper: Option<i64>,
    /// What `find_the_sleeper`'s dice show instead of a roll, for a
    /// scenario that has to know what the night will do. **The dice, not the
    /// odds**: a rigged roll of nought still finds nobody by a fire, whose
    /// odds are nought, so a test that rigs it is still asking the bed. A
    /// hook, not a setting: nothing but a test sets it.
    sleeper_dice: Option<f32>,
    /// What dead horses left on the ground this step -- saddle, bags and the
    /// load in them (`horse::Gear::left_behind`) -- for the tick loop to throw
    /// down as items, which this file cannot do (see `staked`).
    spilled: Vec<Spilled>,
    /// Who a monkey robbed this step, and which monkey did it, for the tick
    /// loop to take out of their pack -- this file has no inventories. See
    /// `raid` and `take_thefts`.
    thefts: Vec<(PlayerId, EntityId)>,
    /// Whether it is raining on the world, for the kept horses standing out
    /// in it (`husbandry::EXPOSED_CONDITION_PER_DAY`). Told by the tick loop.
    raining: bool,
    /// Line-of-sight rays cast since the world started. Test-only, for the
    /// budget test.
    #[cfg(test)]
    rays_cast: u64,
    /// How many times `survey` has done its full O(n) neighbour scan --
    /// herd centre, quarry, threat -- since the world started, rather
    /// than just the alarm check every animal gets every tick.
    ///
    /// Test-only, and it exists to prove that scan is actually being
    /// skipped for animals that are not about to think this tick (see
    /// `survey`'s doc comment) rather than merely being documented to.
    #[cfg(test)]
    full_neighbour_scans: u64,
}

impl Default for Animals {
    fn default() -> Self {
        Self::new()
    }
}

/// A death nobody swung for: what died, and where it fell.
///
/// The same two facts `Struck::Killed` carries, for the same reason:
/// what a death leaves behind is the caller's decision, not the
/// animal's. It used to be a list of stacks with a position each,
/// which baked "a heap of meat" into the only place a mod could kill
/// something from.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Death {
    pub species: Species,
    pub at: (f32, f32, f32),
    /// How grown it was: a young one leaves a small heap rather than its
    /// species' carcass (`youth::leaves_carcass`).
    pub growth: f32,
    /// A sheep killed with its fleece off (`Animal::shorn`): its carcass is
    /// laid with the fleece already taken, so it gives no wool.
    ///
    /// **The wool came off the knife twice before.** A ewe sheared this
    /// morning and butchered this afternoon gave the two of the shearing and
    /// then three more off the carcass -- which made shearing a sheep and
    /// then eating it strictly better than either, and the regrowth that is
    /// the whole reason to keep her (`husbandry`) worth nothing.
    pub shorn: bool,
}

impl Animals {
    pub fn new() -> Self {
        Self {
            animals: Vec::new(),
            next_ordinal: 0,
            next_spawn: SPAWN_INTERVAL,
            spawned: 0,
            killed: 0,
            despawned: 0,
            rng: Rng::from_clock(),
            births: Vec::new(),
            deaths: Vec::new(),
            fallen: Vec::new(),
            staked: Vec::new(),
            falling: Vec::new(),
            calendar: None,
            days_pending: 0.0,
            season: primitive_shared::season::Season::Spring,
            fire_bearers: Vec::new(),
            signs: Vec::new(),
            tracks: Vec::new(),
            wind: (0.0, 0.0),
            parked: Vec::new(),
            dung: Vec::new(),
            hay_eaten: Vec::new(),
            manger_clock: 0,
            raided: None,
            found_sleeper: None,
            sleeper_dice: None,
            spilled: Vec::new(),
            thefts: Vec::new(),
            raining: false,
            #[cfg(test)]
            rays_cast: 0,
            #[cfg(test)]
            full_neighbour_scans: 0,
        }
    }

    /// Tells the animals who is carrying a lit torch.
    ///
    /// **A setter rather than a lookup**, for the reason `step` takes a
    /// list of positions instead of the registry: this file must not be
    /// able to open a player's inventory. The tick loop, which already
    /// holds every player's state to build `where_everyone_is`, calls
    /// this with the ids whose held slot is `BLOCK_TORCH_LIT` (see
    /// `types::is_lit_torch`) just before `step`; a placed fire needs no
    /// telling, because it is in the world and `lit_fire_near` finds it
    /// there. Until the loop calls this, held torches are simply not
    /// known about and only placed fires keep the wolves off.
    ///
    /// The list is replaced rather than merged, so a torch that goes
    /// out or is put away stops counting the tick it does.
    pub fn carrying_fire(&mut self, bearers: Vec<PlayerId>) {
        self.fire_bearers = bearers;
    }

    /// Tells the animals what each player is doing that an ear or an eye
    /// would notice: which way they face, whether they are working, in the
    /// air, sitting or lying, hurt. Replaced, not merged, every tick. See
    /// `PlayerSign`.
    pub fn player_signs(&mut self, signs: Vec<PlayerSign>) {
        self.signs = signs;
    }

    /// Tells the animals the wind, as `raft::Wind::vector` gives it: (x, z)
    /// toward where it blows, as long as it is strong.
    ///
    /// **The rafts' wind and the wildfire's, not a third one**: a hunter who
    /// has watched the smoke lean knows which way to come at a deer from.
    pub fn feel_wind(&mut self, wind: (f32, f32)) {
        self.wind = if wind.0.is_finite() && wind.1.is_finite() { wind } else { (0.0, 0.0) };
    }

    /// Tells the animals the world's day count (`Clock::world_days`), which is
    /// what the young grow by and the season births come in.
    ///
    /// **Days rather than the tick's seconds**, because the day length is a
    /// server setting and "a fawn is grown in four days" has to be four days
    /// on a server with twenty-minute days and on one with an hour. Only
    /// forward time counts: a clock set back (`/time`) grows nobody younger.
    /// A night slept through is days that went by, and the young grow over it.
    pub fn calendar(&mut self, world_days: f32) {
        if !world_days.is_finite() {
            return;
        }
        if let Some(last) = self.calendar {
            self.days_pending += (world_days - last).max(0.0);
        }
        self.calendar = Some(world_days);
        self.season = primitive_shared::season::Season::at(world_days);
    }

    pub fn seeded(seed: u64) -> Self {
        Self {
            rng: Rng::seeded(seed),
            ..Self::new()
        }
    }

    pub fn len(&self) -> usize {
        self.animals.len()
    }

    pub fn is_empty(&self) -> bool {
        self.animals.is_empty()
    }

    /// Spawned, killed, forgotten -- for `/stats`.
    pub fn stats(&self) -> (u64, u64, u64) {
        (self.spawned, self.killed, self.despawned)
    }

    /// Everything that appeared since this was last asked, and where.
    ///
    /// Drained rather than read, so a tick with nobody listening costs
    /// one `mem::take` of an empty vector.
    pub fn take_births(&mut self) -> Vec<(EntityId, (f32, f32, f32))> {
        std::mem::take(&mut self.births)
    }

    /// ...and everything that went, killed or forgotten alike. A mod
    /// that wants to tell the two apart watches what lands on the
    /// ground.
    pub fn take_deaths(&mut self) -> Vec<EntityId> {
        std::mem::take(&mut self.deaths)
    }

    /// What died this step of something nobody swung -- and so what the
    /// tick loop has to lay a carcass for, exactly as a spear's kill gets
    /// one (`lay_carcass`). Drained like the other two lists, and for the
    /// same reason: laying a body takes the item lock and broadcasts, and
    /// this structure's lock is held while the step runs.
    pub fn take_fallen(&mut self) -> Vec<Death> {
        std::mem::take(&mut self.fallen)
    }

    /// Where bodies were driven onto sharpened stakes since this was last
    /// asked. See the field.
    pub fn take_staked(&mut self) -> Vec<(f64, f64, f64)> {
        std::mem::take(&mut self.staked)
    }

    // ---- what a mod may ask about one animal ----
    //
    // Four accessors and two verbs, added for `logic::api_impl`. They
    // are here rather than as a `pub` field because the whole reason the
    // mod API exists is that a mod should not be holding this
    // structure's internals -- it asks by id and is told, and the day
    // the storage changes shape nothing outside has to move.

    /// Where one is, if it is still alive.
    pub fn position(&self, id: EntityId) -> Option<(f32, f32, f32)> {
        self.animals.iter().find(|a| a.id == id).map(|a| a.at())
    }

    /// How much of it is left.
    pub fn health(&self, id: EntityId) -> Option<f32> {
        self.animals.iter().find(|a| a.id == id).map(|a| a.health)
    }

    /// What it is, as an index into `Species::ALL` -- which is what
    /// crosses the mod boundary, because an enum discriminant is not a
    /// stable thing to promise across a compiler version.
    pub fn species_index(&self, id: EntityId) -> Option<u32> {
        let animal = self.animals.iter().find(|a| a.id == id)?;
        primitive_shared::animals::Species::ALL
            .iter()
            .position(|&s| s == animal.species)
            .map(|i| i as u32)
    }

    /// Every animal alive, in the order they were spawned.
    pub fn ids(&self) -> Vec<EntityId> {
        self.animals.iter().map(|a| a.id).collect()
    }

    /// Puts health back, never above what the species has.
    ///
    /// **Clamped rather than trusted.** There is no `set_health` beside
    /// it for the same reason: a boar with forty health is not a boar,
    /// it is a number two mods would disagree about and a fight the
    /// player cannot win with the weapon the game gave them for it.
    pub fn heal(&mut self, id: EntityId, amount: f32) -> bool {
        let Some(animal) = self.animals.iter_mut().find(|a| a.id == id) else {
            return false;
        };
        animal.health = (animal.health + amount).min(animal.species.health());
        true
    }

    /// Everything within `radius` of a point.
    pub fn within(&self, of: (f32, f32, f32), radius: f32) -> Vec<EntityId> {
        let squared = radius * radius;
        self.animals
            .iter()
            .filter(|a| {
                let (dx, dy, dz) = (
                    a.at().0 - of.0,
                    a.at().1 - of.1,
                    a.at().2 - of.2,
                );
                dx * dx + dy * dy + dz * dz <= squared
            })
            .map(|a| a.id)
            .collect()
    }

    /// Hurts one without anybody having swung at it.
    ///
    /// `Some(death)` when the blow killed it, and the caller decides
    /// what it leaves -- the same contract `strike` has, and for the
    /// same reason: what a death *does* is the tick loop's business and
    /// not the animal's.
    pub fn hurt(&mut self, id: EntityId, amount: f32) -> Option<Death> {
        let index = self.animals.iter().position(|a| a.id == id)?;
        let animal = &mut self.animals[index];
        animal.health -= amount;
        if animal.health > 0.0 {
            return None;
        }
        Some(self.fell(index))
    }

    /// Takes a killed animal off the living and starts it falling: see
    /// `FALL_SECONDS`. Answers with the death as it stands at the blow -- what
    /// died and where it was hit -- for the caller that wants to know; the body
    /// itself, and where it ends up, arrive through `take_fallen` when the fall
    /// is done, and *that* is what the carcass is laid by.
    ///
    /// **One function for every way to die**, so a spear, a fall, a fire, a
    /// mod's `entity_kill` and a collapsing roof all fall the same way. A
    /// swimmer does not fall -- it has nowhere to fall to, and a fish rolling
    /// over in the water for a second before it is a heap on the surface is a
    /// fish dying twice -- so it goes straight to `fallen`.
    fn fell(&mut self, index: usize) -> Death {
        let mut body = self.animals.remove(index);
        self.killed += 1;
        let death = Death { species: body.species, at: body.at(), growth: body.growth, shorn: body.shorn() };
        // **A horse's saddle and load go down with it**, where it fell, for
        // the tick loop to throw on the ground: see `horse::Gear::left_behind`
        // for why all of it. The rider is put off by the tick loop, which
        // finds the horse gone (`horses::tick`).
        body.ride = None;
        if let Some(gear) = body.gear.take() {
            let left = gear.left_behind();
            if !left.is_empty() {
                self.spilled.push((body.position, left));
            }
        }
        if body.species.swims() {
            self.deaths.push(body.id);
            self.fallen.push(death);
            return death;
        }
        body.health = 0.0;
        body.dying_for = FALL_SECONDS;
        body.mind = Mind::Idle;
        body.attitude = primitive_shared::protocol::Attitude::Dying;
        body.hurt_for = HURT_SECONDS;
        self.falling.push(body);
        death
    }

    /// The bodies going down, a tick on; the ones that have landed leave the
    /// list, and their deaths go into `fallen` where they came to rest.
    fn settle_the_falling(&mut self, world: &dyn BlockWorld, dt: f32) {
        if self.falling.is_empty() {
            return;
        }
        for body in &mut self.falling {
            lie_still(body, world, dt);
            body.dying_for -= dt;
            body.hurt_for = (body.hurt_for - dt).max(0.0);
        }
        let (down, going): (Vec<Animal>, Vec<Animal>) =
            std::mem::take(&mut self.falling).into_iter().partition(|body| body.dying_for <= 0.0);
        self.falling = going;
        for body in down {
            self.deaths.push(body.id);
            self.fallen.push(Death { species: body.species, at: body.at(), growth: body.growth, shorn: body.shorn() });
        }
    }

    /// How many bodies are still going down. Tests and `/stats`.
    pub fn falling_count(&self) -> usize {
        self.falling.len()
    }

    /// Everybody's mother and young, where they are this tick. `None` when
    /// nobody alive has either -- almost every tick of a world with no young
    /// in it -- so the price of the mechanic to such a world is one pass over
    /// the list looking at two fields.
    fn family(&self) -> Option<Vec<Family>> {
        if !self.animals.iter().any(|a| a.mother.is_some() || a.young.is_some()) {
            return None;
        }
        let kin = |id: Option<EntityId>| {
            let other = self.animals.iter().find(|a| Some(a.id) == id)?;
            Some(Kin {
                at: other.at(),
                fleeing: other.mind == Mind::Flee,
                fighting: matches!(other.mind, Mind::Charge | Mind::Chase | Mind::Watch | Mind::Recover)
                    && other.fights(),
                heading: other.velocity.2.atan2(other.velocity.0),
                run: other.species.run_speed() * youth::speed(other.growth) * other.pace,
            })
        };
        Some(
            self.animals
                .iter()
                .map(|a| Family { mother: kin(a.mother), young: kin(a.young) })
                .collect(),
        )
    }

    /// The young grow by the days that have gone by, and the calm adults
    /// give birth: see `primitive_shared::youth`.
    fn raise_young(&mut self, days: f32) {
        if days <= 0.0 {
            return;
        }
        // **The milk is the lamb's.** A lamb whose mother was milked within
        // the day grows at `husbandry::MILKED_LAMB_GROWTH`: see
        // `husbandry::MILK_EVERY_DAYS` for the decision that is.
        let short: Vec<EntityId> = self
            .animals
            .iter()
            .filter(|a| a.keep.is_some_and(|k| k.lamb_goes_short()))
            .filter_map(|a| a.young)
            .collect();
        let mut grown_up = Vec::new();
        for animal in &mut self.animals {
            animal.birth_rest = (animal.birth_rest - days).max(0.0);
            if !youth::is_young(animal.growth) {
                continue;
            }
            let before = youth::strength(animal.growth);
            let rate = if short.contains(&animal.id) { husbandry::MILKED_LAMB_GROWTH } else { 1.0 };
            animal.growth = (animal.growth + days * rate / youth::GROWN_DAYS).min(youth::GROWN);
            // A bigger body has more to lose: the health grows with it, by
            // the difference, so a fawn that was hurt is still that much hurt.
            animal.health += animal.species.health() * (youth::strength(animal.growth) - before);
            if !youth::is_young(animal.growth) {
                animal.mother = None;
                grown_up.push(animal.id);
            }
        }
        // Grown is its own animal, and the mother is free of it.
        for animal in &mut self.animals {
            if animal.young.is_some_and(|young| grown_up.contains(&young)) {
                animal.young = None;
            }
        }

        // **Births.** A chance a day, per calm adult, drawn against the days
        // that went by -- so a server with long days and one with short ones
        // fill out their herds at the same rate per day.
        let mut mothers = Vec::new();
        for animal in &self.animals {
            if !youth::breeds(animal.species)
                || youth::is_young(animal.growth)
                || animal.birth_rest > 0.0
                || self.animals.iter().any(|a| Some(a.id) == animal.young)
            {
                continue;
            }
            // **A kept animal breeds on its keeper's terms, not the
            // meadow's**: both of a pair tame and thriving -- fed, on a rich
            // ration, in condition (`Keeping::thriving`) -- and the season's
            // rate with a partner. Grass alone keeps a flock; grain grows it.
            if let Some(keep) = animal.keep.filter(|k| k.tame) {
                let partner = keep.thriving()
                    && self.animals.iter().any(|other| {
                        other.id != animal.id
                            && other.species == animal.species
                            && !youth::is_young(other.growth)
                            && other.keep.is_some_and(|k| k.thriving())
                            && (other.at().0 - animal.at().0).hypot(other.at().2 - animal.at().2)
                                <= youth::PARTNER_RANGE
                    });
                if partner && self.rng.range(0.0, 1.0) < youth::births_per_day(self.season, true) * days {
                    mothers.push(animal.id);
                }
                continue;
            }
            // Safe and fed: nothing it is running from or remembers being hurt
            // near, not on edge, and not parched.
            let calm = matches!(animal.mind, Mind::Idle | Mind::Wander | Mind::Drink)
                && animal.dangers.is_empty()
                && animal.wary_for <= 0.0
                && animal.thirst < THIRSTY_AT;
            if !calm {
                continue;
            }
            let partner = self.animals.iter().any(|other| {
                other.id != animal.id
                    && other.species == animal.species
                    && !youth::is_young(other.growth)
                    && (other.at().0 - animal.at().0).hypot(other.at().2 - animal.at().2) <= youth::PARTNER_RANGE
            });
            let chance = youth::births_per_day(self.season, partner) * days;
            if self.rng.range(0.0, 1.0) < chance {
                mothers.push(animal.id);
            }
        }
        for mother in mothers {
            let _ = self.bear_young(mother);
        }
    }

    /// A young one for the animal `mother`, beside her, if she can have one:
    /// an adult of a species that breeds, with no young already, and room
    /// under the ceiling. What the births in the world go through, and what a
    /// test or a mod asks for directly.
    pub fn bear_young(&mut self, mother: EntityId) -> Option<EntityId> {
        let parent = self.find(mother)?;
        if !youth::breeds(parent.species)
            || youth::is_young(parent.growth)
            || self.animals.iter().any(|a| Some(a.id) == parent.young)
        {
            return None;
        }
        let (species, at, yaw, parent_keep) = (parent.species, parent.at(), parent.yaw, parent.keep);
        // At her flank, a body's width to the side: a newborn put *in* her
        // would be two animals in one cell, which `unstack` would then shove
        // apart on the next tick anyway.
        let side = yaw + std::f32::consts::FRAC_PI_2;
        let beside = (at.0 + side.cos() * species.width(), at.1, at.2 + side.sin() * species.width());
        let kept = parent_keep.is_some_and(|k| k.tame);
        let id = self.spawn_as(species, beside, kept)?;
        let young = self.animals.last_mut().expect("just spawned");
        young.growth = 0.0;
        young.health = species.health() * youth::strength(0.0);
        young.yaw = yaw;
        young.wants_yaw = yaw;
        young.mother = Some(mother);
        // Born to a kept mother, kept: see `Keeping::born_to`.
        young.keep = parent_keep.filter(|k| k.tame).map(|k| husbandry::Keeping::born_to(&k));
        if let Some(parent) = self.animals.iter_mut().find(|a| a.id == mother) {
            parent.young = Some(id);
            parent.birth_rest = youth::BIRTH_REST_DAYS;
        }
        Some(id)
    }

    /// How grown one is, 0 to `youth::GROWN`. Tests and mods.
    pub fn growth(&self, id: EntityId) -> Option<f32> {
        self.find(id).map(|a| a.growth)
    }

    /// Its mother, if it is young and she is alive.
    pub fn mother_of(&self, id: EntityId) -> Option<EntityId> {
        let mother = self.find(id)?.mother?;
        self.find(mother).map(|m| m.id)
    }

    /// Takes one out of the world without killing it.
    ///
    /// What a mod uses to clean up something it spawned. Distinct from
    /// `hurt` on purpose: this leaves nothing behind, and a mod tidying
    /// up its own entities should not be showering the ground in meat.
    pub fn forget(&mut self, id: EntityId) -> bool {
        let Some(index) = self.animals.iter().position(|a| a.id == id) else {
            return false;
        };
        self.animals.remove(index);
        self.despawned += 1;
        self.deaths.push(id);
        true
    }

    /// Puts one in the world at a point, ignoring the spawn rules.
    ///
    /// `spawn` is the world's own path and applies the conditions --
    /// light, ground, distance from a player. This is the mod's, and it
    /// applies only the cap: a mod that asked for an animal somewhere
    /// specific meant it, and second-guessing that would make the call
    /// useless for the thing anybody wants it for.
    pub fn spawn_at(
        &mut self,
        species: primitive_shared::animals::Species,
        at: (f32, f32, f32),
    ) -> Option<EntityId> {
        if !at.0.is_finite() || !at.1.is_finite() || !at.2.is_finite() {
            return None;
        }
        self.spawn(species, at)
    }

    /// Where every animal of one species is, and how many there are.
    ///
    /// **Off the live list rather than a tally kept beside it**, for the
    /// reason `spawn` counts its own ceiling that way: a number somebody
    /// remembers to increment is a number that goes wrong the first time
    /// something else kills one. Written for `logic::vermin`, which needs
    /// both -- where the rats are, to raid the chests beside them, and how
    /// many, to know whether to make another.
    pub fn where_the(&self, species: Species) -> Vec<(f32, f32, f32)> {
        self.animals
            .iter()
            .filter(|a| a.species == species)
            .map(|a| primitive_shared::geometry::narrow(a.position))
            .collect()
    }

    /// Where every living animal heavy enough to break a pit's cover is
    /// standing (`pitfall::breaks_through`). Asked once a tick, after the
    /// step, by the server's `collapse_pit_covers`.
    pub fn heavy_feet(&self) -> Vec<(f32, f32, f32)> {
        self.animals
            .iter()
            .filter(|a| primitive_shared::pitfall::breaks_through(a.species))
            .map(|a| primitive_shared::geometry::narrow(a.position))
            .collect()
    }

    /// Everything to draw: the living, and the dead still going down.
    pub fn states(&self) -> Vec<EntityState> {
        self.animals.iter().chain(&self.falling).map(|a| a.state()).collect()
    }

    /// Turns an animal to a stated heading. Tests only: which way one
    /// is facing is the simulation's business, and every test that cares
    /// about the hit box has to be able to say which way it is pointing.
    ///
    /// Not test-only: a scenario on the client stands a horse along its
    /// strip of field with it (`Server::face_animal`).
    pub fn face_for_test(&mut self, id: EntityId, yaw: f32) {
        if let Some(animal) = self.animals.iter_mut().find(|a| a.id == id) {
            animal.yaw = yaw;
            animal.wants_yaw = yaw;
        }
    }

    /// A new animal of `species` standing at `at`, with its own id, and not
    /// yet anywhere: `spawn` puts it in the world, and `load_herd` puts a
    /// kept one back into `parked` with what the save file says of it.
    fn make(&mut self, species: Species, at: (f32, f32, f32)) -> Animal {
        self.next_ordinal += 1;
        self.spawned += 1;
        let id = entity_id(EntitySource::Animal, self.next_ordinal);
        let yaw = self.rng.range(0.0, std::f32::consts::TAU);
        Animal {
            id,
            species,
            position: primitive_shared::geometry::wide(at),
            velocity: (0.0, 0.0, 0.0),
            yaw,
            wants_yaw: yaw,
            health: species.health(),
            // **From the id, not from the generator.** Drawing them
            // here would consume two numbers per spawn out of the same
            // stream every other decision comes from, which changes what
            // a seeded world does -- the animals are deterministic on
            // purpose, and a fixture that hunts a particular deer should
            // keep hunting it. A hash of the id gives every animal its
            // own pace and its own bend, repeatably.
            // Seven per cent either way. Wider was tried at fifteen and
            // it is too much: the margin between a wolf's run and a
            // deer's is a *designed* number, and a spread that can
            // invert it turns a hunt into a procession. This is enough
            // to break the lockstep and not enough to change who
            // catches whom.
            pace: 0.93 + spread(self.next_ordinal, 0x51ED_2701) * 0.14,
            drift: (spread(self.next_ordinal, 0x9E37_79B9) - 0.5) * 0.5,
            mind: Mind::Idle,
            // Nothing on it yet: an animal is poisoned by a spear, and
            // a spawned one has not met a hunter.
            poison_for: 0.0,
            next_thought: self.rng.range(0.0, THINK_INTERVAL),
            target: None,
            hurt_for: 0.0,
            gore_cooldown: 0.0,
            stole_from: None,
            charge_at: None,
            angry_for: 0.0,
            fed_for: 0.0,
            quarry: None,
            on_ground: false,
            fall_peak_y: None,
            breath: crate::logic::survival::BREATH_SECONDS,
            cover: None,
            hidden_for: 0.0,
            // Staggered from the id, not the generator, for the reason
            // `pace` is: it must not move the decision stream.
            looks: (self.next_ordinal % LOOK_EVERY as u64) as u8,
            dodge: 1.0,
            dangers: Vec::new(),
            // **Staggered, from the id, for the reason `pace` is.** A
            // herd is spawned in one call; if every animal in it started
            // at zero thirst they would all cross `THIRSTY_AT` in the
            // same second and walk to the river shoulder to shoulder,
            // which reads as a scripted event rather than as six animals
            // that each got thirsty. Up to a full interval of head start,
            // drawn from the ordinal so it does not move the decision
            // stream a seeded world depends on.
            thirst: spread(self.next_ordinal, 0x2545_F491) * THIRSTY_AT,
            water: None,
            next_water_scan: 0.0,
            home: None,
            bound_for: None,
            flight_ceiling: f32::NEG_INFINITY,
            // **Staggered from the ordinal, for the reason `thirst` is.**
            // A covey is spawned in one call; if every bird in it looked
            // for a nest on the same tick they would all find the same
            // tree in the same second and fly to it in formation, which
            // is a flock of one bird drawn three times. Up to a full
            // interval of head start, drawn from the id so it does not
            // move the decision stream a seeded world depends on.
            next_home_scan: spread(self.next_ordinal, 0x1B87_3593) * NEST_SCAN_INTERVAL,
            stamina: species.stamina_seconds(),
            flank_to: None,
            dive_for: 0.0,
            homing_for: 0.0,
            // Where it was put: a fish spawned mid-water holds that depth
            // until its first thought picks another.
            swim_depth: at.1,
            wary_for: 0.0,
            threat_at: None,
            turn_rate: 0.0,
            // Spread over the cycle from the ordinal, for the reason
            // `next_home_scan` is: a covey spawned in one call must not beat
            // and glide in unison. See `Animal::air_phase`.
            air_phase: spread(self.next_ordinal, 0x9E37_79B9) * AIR_CYCLE,
            attitude: primitive_shared::protocol::Attitude::Easy,
            growth: youth::GROWN,
            mother: None,
            young: None,
            birth_rest: 0.0,
            speed_cap: f32::INFINITY,
            dying_for: 0.0,
            keep: None,
            stallion: false,
            gear: None,
            ride: None,
            settle_for: 0.0,
        }
    }

    /// Puts one in the world at a stated place. What the spawner uses,
    /// and what a test or a plugin can call directly.
    pub fn spawn(&mut self, species: Species, at: (f32, f32, f32)) -> Option<EntityId> {
        self.spawn_as(species, at, false)
    }

    /// `spawn`, for one that will be kept (`kept`: a tame mother's young) or
    /// wild.
    fn spawn_as(&mut self, species: Species, at: (f32, f32, f32), kept: bool) -> Option<EntityId> {
        // **Each class against its own ceiling.** The sea's animals and the
        // land's are capped separately (`MAX_FISH`, `MAX_ANIMALS`): one
        // list, one wire, two budgets -- a coast full of schools must not be
        // why a mod's deer refuses to appear, and a crowded meadow must not
        // empty the river. The kept are a third (`MAX_KEPT`), and the land's
        // cap counts only the wild: see `MAX_KEPT` for the rule and the herd
        // that used to empty the world.
        let (class, ceiling) = if kept {
            (self.animals.iter().filter(|a| is_kept(a)).count(), MAX_KEPT)
        } else if species.swims() {
            (self.animals.iter().filter(|a| a.species.swims()).count(), MAX_FISH)
        } else {
            (self.animals.iter().filter(|a| !a.species.swims() && !is_kept(a)).count(), MAX_ANIMALS)
        };
        if class >= ceiling {
            return None;
        }
        let animal = self.make(species, at);
        let id = animal.id;
        self.births.push((id, at));
        self.animals.push(animal);
        // **A bear's den is where it was first found.** See
        // `TERRITORY_RADIUS`: the ground it keeps is round this.
        if species.keeps_territory() {
            if let Some(bear) = self.animals.last_mut() {
                bear.home = Some((at.0, at.2));
            }
        }
        Some(id)
    }

    /// One tick of everything alive.
    ///
    /// `players` is where everybody is, sampled once by the tick loop --
    /// the same list the interest grid is built from. Handing it in
    /// rather than reaching for the registry keeps this file unable to
    /// touch a player's inventory, health or connection, which is worth
    /// more than the parameter costs.
    ///
    /// Returns the blows landed on players: who was hit, how hard, and
    /// **what hit them** -- for the caller to apply through the same
    /// path a fall does.
    ///
    /// The species is carried rather than assumed. It was assumed while
    /// the boar was the only thing that could hit anybody, and the death
    /// message was the literal string "was gored by a boar"; the first
    /// animal to join it would have killed players by goring them with
    /// tusks it does not have.
    pub fn step(
        &mut self,
        world: &dyn BlockWorld,
        players: &[(primitive_shared::protocol::PlayerId, (f32, f32, f32))],
        dt: f32,
        time_of_day: f32,
    ) -> Vec<Blow> {
        let mut blows = Vec::new();
        // A kept flock somebody has just walked back to is in the world
        // before anything looks at it. See `parked`.
        self.unpark(world, players);
        // What each animal can see of its own kind, worked out before
        // anything moves.
        //
        // A snapshot rather than a look at the live list, because a herd
        // read while it is being walked is a herd where the first deer
        // sees where everyone was and the last sees where half of them
        // now are -- which is a difference nobody can debug and
        // everybody can see, as a line of animals that drifts. See
        // `Neighbours`.
        let (seen, _full_scans) = self.survey(dt);
        // ...and every mother and young, the same way. See `keep_family`.
        let family = self.family();
        #[cfg(test)]
        {
            self.full_neighbour_scans += _full_scans;
        }
        // How every player looks and sounds this tick, worked out once for
        // everybody rather than once per animal that asks.
        let figures = self.figures(world, players, dt, is_night(time_of_day));
        let wind = self.wind;
        let mut rays = RAYS_PER_TICK;
        let mut bites = Vec::new();
        // Gathered here and appended after the loop, for `bites`' reason:
        // nothing may touch `self` while the animals are being walked.
        let mut thefts: Vec<(PlayerId, EntityId)> = Vec::new();
        // **What the world itself does to an animal, this tick.** Three
        // things a player already answers to (`logic::survival`) and
        // nothing here ever did: a fall from height, standing in fire,
        // and a head that stays under water too long. Collected here
        // rather than applied in place for the same reason bites are --
        // `hurt`, which does the actual damage-and-maybe-death, needs
        // `&mut self` and this loop is already holding `&mut animal`
        // out of `self.animals`.
        let mut hazards: Vec<(EntityId, f32)> = Vec::new();
        for (index, animal) in self.animals.iter_mut().enumerate() {
            // **A swimmer thinks and moves down its own path** -- see the
            // module doc for why that is two functions rather than a
            // `swims` test in every rule of `think` and `walk`. What the
            // world does to it is one thing, and it is the opposite of what
            // it does to everything else: air.
            // **Ridden, it goes where the reins say** and thinks nothing of its
            // own: see `Ride`. What the world does to a body still does it --
            // a fire, a roof falling in -- and a wolf can still bite it.
            let hazard = if animal.ride.is_some() {
                carry(animal, world, dt);
                burn(animal, world, dt) + suffocate(animal, world, dt)
            } else if animal.species.swims() {
                think_fish(animal, players, &seen[index], world, &mut self.rng, dt);
                swim(animal, world, dt);
                gasp(animal, world, dt)
            } else {
                think(
                    animal,
                    players,
                    &mut Senses { figures: &figures, wind, rays: &mut rays },
                    &self.fire_bearers,
                    &seen[index],
                    world,
                    &mut self.rng,
                    dt,
                    time_of_day,
                );
                keep_family(animal, family.as_ref().map(|f| f[index]).unwrap_or_default());
                let fallen = walk(animal, world, dt);
                let burned = burn(animal, world, dt);
                let staked = stakes(animal, world);
                if staked > 0.0 {
                    // The flinch `stakes` waits out, started here. `hurt`,
                    // which the damage goes through, does not start one --
                    // so a deer pushed along a palisade was cut, and heard
                    // being cut, twenty times a second.
                    animal.hurt_for = HURT_SECONDS;
                    // Heard at the flank, where the points went in.
                    let (x, y, z) = animal.position;
                    self.staked.push((x, y + f64::from(animal.species.half_extents().1), z));
                }
                let suffocated = suffocate(animal, world, dt);
                fallen + burned + suffocated + staked
            };
            // ...and a body the poison has already taken below nothing, with
            // no damage of its own to add: `think` wears the paste down and
            // asks no question about death, and `hurt` is where that
            // question lives. Without this a beaten lion that the paste
            // finished walked on at minus three and was forgotten, carcass
            // and all.
            if hazard > 0.0 || animal.health <= 0.0 {
                hazards.push((animal.id, hazard));
            }
            if let Some(blow) = gore(animal, players, dt) {
                blows.push(blow);
            }
            if let Some(bite) = bite(animal, &seen[index]) {
                bites.push(bite);
            }
            animal.hurt_for = (animal.hurt_for - dt).max(0.0);
            animal.settle_for = (animal.settle_for - dt).max(0.0);
            if let Some(who) = animal.stole_from.take() {
                thefts.push((who, animal.id));
            }
        }
        self.thefts.append(&mut thefts);
        #[cfg(test)]
        {
            self.rays_cast += u64::from(RAYS_PER_TICK - rays);
        }
        self.unstack(world);
        self.settle_bites(bites);
        self.settle_hazards(hazards);
        // The dead from earlier ticks go on down -- after this tick's deaths,
        // so a body killed this tick has its whole `FALL_SECONDS` still ahead
        // of it less one tick.
        self.settle_the_falling(world, dt);
        // The days that went by, once, for both uses: the kept animals'
        // belly and fleece, and the young growing and being born.
        let days = std::mem::take(&mut self.days_pending);
        self.keep_the_kept(world, days);
        self.raise_young(days);
        self.forget_the_distant(players);
        self.next_spawn -= dt;
        if self.next_spawn <= 0.0 {
            self.next_spawn = SPAWN_INTERVAL;
            self.populate(world, players, is_night(time_of_day));
            // ...and, on a night, whatever comes for the pens.
            self.raid_the_pens(world, is_night(time_of_day));
            // ...and the water, on the same clock and its own cap. One
            // attempt each, for the reason `populate` gives: a player in the
            // middle of a desert costs one failed search for water and no
            // more.
            self.populate_water(world, players);
            // ...and the coast, the same way: a player nowhere near the sea
            // pays a few biome lookups and nothing else (`COAST_BEARINGS`).
            self.populate_shore(world, players, is_night(time_of_day));
        }
        blows
    }

    /// Every player as the animals take them this tick: their gait off the
    /// last `GAIT_WINDOW` of movement, and everything `PlayerSign`, the torch
    /// list and the ground under them add to how loud and how plain they are.
    ///
    /// Two block reads a player for the cover, and nothing else touches the
    /// world: the per-animal part of seeing is `perceive`.
    fn figures(
        &mut self,
        world: &dyn BlockWorld,
        players: &[(PlayerId, (f32, f32, f32))],
        dt: f32,
        night: bool,
    ) -> Vec<Figure> {
        self.tracks.retain(|t| players.iter().any(|&(id, _)| id == t.who));
        let mut figures = Vec::with_capacity(players.len());
        for &(who, at) in players {
            let track = match self.tracks.iter_mut().find(|t| t.who == who) {
                Some(track) => track,
                None => {
                    self.tracks.push(Track { who, anchor: at, age: 0.0, speed: 0.0, by_fire: false, fire_look_in: 0.0 });
                    self.tracks.last_mut().expect("just pushed")
                }
            };
            track.age += dt;
            // **Is there a fire lit beside them?** Only asked after dark,
            // which is the only time the answer changes anything, and only
            // once a `FIRE_LOOK_EVERY`. By day the answer is dropped, so the
            // first look of the evening is a fresh one.
            if night {
                track.fire_look_in -= dt;
                if track.fire_look_in <= 0.0 {
                    track.fire_look_in = FIRE_LOOK_EVERY;
                    track.by_fire = lit_fire_near(world, at, FIRE_RADIUS).is_some();
                }
            } else {
                track.by_fire = false;
                track.fire_look_in = 0.0;
            }
            let moved = (at.0 - track.anchor.0).hypot(at.2 - track.anchor.2);
            if moved > TELEPORT + RUNNING_ABOVE * track.age {
                // Put somewhere, not moving: the window starts again from
                // here -- and the fire is looked for again, since the
                // person is somewhere else now.
                *track = Track { who, anchor: at, age: 0.0, speed: 0.0, by_fire: false, fire_look_in: 0.0 };
            } else if track.age >= GAIT_WINDOW {
                track.speed = moved / track.age;
                track.anchor = at;
                track.age = 0.0;
            }
            let gait = Gait::of_speed(track.speed);
            let sign = self.signs.iter().find(|s| s.who == who);
            let mut loudness = gait.loudness();
            let mut visibility = gait.visibility();
            if let Some(sign) = sign {
                if sign.working {
                    loudness = loudness.max(WORKING_LOUDNESS);
                }
                if sign.airborne {
                    loudness = loudness.max(AIRBORNE_LOUDNESS);
                }
                if sign.low {
                    loudness *= LOW_POSTURE;
                    visibility *= LOW_POSTURE;
                }
            }
            // In cover: the cell the feet are in or the one over it -- a tuft
            // round the legs, a bush, the edge of a canopy.
            let (x, y, z) = (at.0.floor() as i32, at.1.floor() as i32, at.2.floor() as i32);
            let hidden = (y..=y + 1).any(|y| {
                world.block(x, y, z).is_some_and(|b| is_cover(b) || primitive_shared::types::is_foliage(b))
            });
            if hidden {
                visibility *= IN_COVER;
            }
            if night {
                // The brightest light they are in decides it: a fire on the
                // ground outshines a torch in the hand, and either outshines
                // the dark. See `FIRE_SEEN_AT_NIGHT` for what that costs.
                visibility *= if track.by_fire {
                    FIRE_SEEN_AT_NIGHT
                } else if self.fire_bearers.contains(&who) {
                    TORCH_AT_NIGHT
                } else {
                    DARKNESS
                };
            }
            figures.push(Figure {
                who,
                at,
                loudness,
                visibility,
                facing: sign.map(|s| s.facing),
                wounded: sign.is_some_and(|s| s.wounded),
                asleep: sign.is_some_and(|s| s.asleep),
                held: sign.and_then(|s| s.held),
                reek: sign.map_or(1.0, |s| s.reek.clamp(1.0, primitive_shared::equipment::TAR_REEK)),
            });
        }
        figures
    }

    /// A player swung at an entity. Works out whether they hit, and what
    /// that did.
    ///
    /// The reach check is the caller's -- it is the same one a punch
    /// against a player uses, and it belongs where the other one is. What
    /// is checked here is that the entity exists and that the swing
    /// actually reached the animal's box, which is a question only this
    /// file can answer.
    /// The same blow, with fly agaric on the point.
    ///
    /// Split from `strike` rather than folded into it because every
    /// other caller of `strike` -- a falling block, a mod, a test --
    /// has nothing to say about poison, and a fifth parameter on the
    /// common path would be `0.0` at all of them.
    pub fn strike_poisoned(
        &mut self,
        id: EntityId,
        from: (f32, f32, f32),
        reach: f32,
        damage: f32,
        poison_seconds: f32,
    ) -> Struck {
        let struck = self.strike(id, from, reach, damage);
        // **Only a blow that landed poisons anything.** A miss that
        // still smeared the paste onto an animal would be the one way
        // to poison something from across a field.
        if !matches!(struck, Struck::Missed) && poison_seconds > 0.0 {
            if let Some(animal) = self.animals.iter_mut().find(|a| a.id == id) {
                // The longer of the two rather than the sum, on the
                // argument sickness makes: being poisoned twice is
                // still being poisoned, and a sum would make a second
                // thrust worth more than the first.
                animal.poison_for = animal.poison_for.max(poison_seconds);
            }
        }
        struck
    }


    pub fn strike(&mut self, id: EntityId, from: (f32, f32, f32), reach: f32, damage: f32) -> Struck {
        let Some(index) = self.animals.iter().position(|a| a.id == id) else {
            return Struck::Missed;
        };
        let animal = &mut self.animals[index];
        // Reach is measured to the animal's *box*, not to a point inside
        // it: a swing that lands on the far end of a deer is a swing
        // that landed. See `distance_to_box`.
        if animal.distance_to_box(from) > reach {
            return Struck::Missed;
        }

        // **A blow on a turning boar's back lands harder.** Applied
        // here rather than where the damage is worked out, because only
        // this file knows which way the animal is facing and what it is
        // in the middle of doing -- and because it has to be read
        // *before* the flinch below turns the animal to face the blow,
        // or the back would never be exposed to the hit that found it.
        // See `Animal::exposed_back` and `BACKSTAB`.
        let damage = if animal.exposed_back(from) {
            damage.max(0.0) * BACKSTAB
        } else {
            damage.max(0.0)
        };
        // **And then the hide takes its share**, which is what stops a
        // player beating a boar to death with their fists. Applied
        // after the backstab, because a blow on the shoulder of a
        // turning animal is still a blow on its shoulder: the bonus is
        // about where you hit and the armour is about what you hit it
        // through. See `Species::hide_armour` for the arithmetic and
        // the reason it is a subtraction.
        let damage = animal.species.hurt_by(damage);
        animal.health -= damage;
        animal.hurt_for = HURT_SECONDS;
        // Whatever it was making for, the plan was wrong: the next bolt
        // picks cover afresh, from where it now is.
        animal.cover = None;
        // **A hit is what starts a fight.** Everything flinches -- the
        // boar included, for one thought -- and what separates the two
        // is what happens at the thought after: a deer keeps running,
        // and a boar has a grudge to work off. See
        // `Species::grudge_seconds`.
        animal.angry_for = animal.species.grudge_seconds();
        animal.charge_at = None;

        // **What being hit does depends on what was hit.**
        //
        // It used to turn *everything* round: `Flee`, facing away, for a
        // third of a second, after which a boar thought again, found it
        // was angry, and turned back. Hit it again and it did the whole
        // thing over -- so a fight with a boar was watching it pirouette
        // between every blow, which is the single most stupid-looking
        // thing an animal in this world could do and was exactly what
        // "the animals are stupid" meant.
        //
        // A boar that is not beaten does what a boar does: it takes the
        // blow, is shoved back by it, and turns *toward* whoever landed
        // it. Prey runs, and so does anything that has had enough (see
        // `BREAKS_OFF_BELOW`) -- a fight to the death every time is the
        // other way to make them stupid.
        let (ax, az) = (animal.at().0 - from.0, animal.at().2 - from.2);
        let away = if ax != 0.0 || az != 0.0 {
            az.atan2(ax)
        } else {
            animal.yaw
        };
        // **A mother with young does not break off.** Everything else that
        // fights gives up at `BREAKS_OFF_BELOW` and runs, which is what makes
        // a boar fair; a sow with a piglet at her side stands, because running
        // would be leaving it. See `primitive_shared::youth` for why that is
        // the decision the young are for.
        let beaten = animal.health <= animal.species.health() * youth::strength(animal.growth) * BREAKS_OFF_BELOW
            && animal.young.is_none();
        if animal.fights() && !beaten {
            // Rocked, not routed: it stands where it is for the length
            // of the flinch, facing the blow, and comes again on the
            // thought after. `Recover` is the state a charge already
            // ends into, so nothing new has to know about this.
            animal.mind = Mind::Recover;
            animal.next_thought = FLINCH_SECONDS;
            animal.wants_yaw = away + std::f32::consts::PI;
            animal.yaw = animal.wants_yaw;
            // ...and shoved. The knock is what the old spin was standing
            // in for: a blow that moved nothing read as a blow that
            // missed, and this is that feedback without the animal
            // giving up its ground.
            let (sin, cos) = away.sin_cos();
            animal.velocity.0 += cos * KNOCKBACK;
            animal.velocity.2 += sin * KNOCKBACK;
        } else {
            animal.mind = Mind::Flee;
            // **A beaten wolf hops; everything else bolts.** A full
            // bolt is two and a half seconds, which at a wolf's run is
            // nineteen blocks -- past its own awareness, so the wolf
            // `shadow` was written for had lost you before it ever got
            // to shadow you. A short run puts a few blocks between you
            // and hands the decision to the next thought, which is where
            // the falling-back lives.
            // (`Species::falls_back_when_hurt`: the wolf's alone -- a beaten
            // lion leaves, like a boar.)
            animal.next_thought = if animal.species.falls_back_when_hurt() {
                0.6
            } else {
                FLEE_SECONDS
            };
            // Snapped rather than turned: a flinch is not a considered
            // turn, and an animal that eased round after being speared
            // reads as one that did not notice.
            animal.wants_yaw = away;
            animal.yaw = away;
        }
        let species = animal.species;
        let at = animal.at();

        // **The herd remembers where this happened.** Everything of the
        // same kind within sight of the blow -- the struck animal
        // included -- writes the place down, so it is the *meadow* that
        // empties and not one deer that learns. Before the death check
        // on purpose: a kill is the strongest reason of all to avoid the
        // spot, and the animal that could have remembered is about to
        // be removed. See `Animal::remember_danger` and `graze`.
        let here = (at.0, at.2);
        for other in &mut self.animals {
            if other.species != species {
                continue;
            }
            let (dx, dz) = (other.at().0 - at.0, other.at().2 - at.2);
            if dx * dx + dz * dz <= HERD_RADIUS * HERD_RADIUS {
                other.remember_danger(here);
            }
        }

        // **...and a young one's mother comes for whoever hit it**, if she is
        // one that fights: angry, as though she had been struck herself, which
        // sends her at the nearest person (`think_hunter`'s grudge). A doe is
        // not told anything here -- she was already told by the herd's memory
        // above, and what a doe does about it is run, with the fawn.
        if let Some(mother) = self.animals[index].mother {
            if let Some(mother) = self.animals.iter_mut().find(|a| a.id == mother) {
                let (dx, dz) = (mother.at().0 - at.0, mother.at().2 - at.2);
                if mother.fights() && dx * dx + dz * dz <= HERD_RADIUS * HERD_RADIUS {
                    mother.angry_for = mother.species.grudge_seconds();
                    mother.next_thought = 0.0;
                }
            }
        }

        if self.animals[index].health > 0.0 {
            return Struck::Hurt;
        }
        self.fell(index);
        Struck::Killed { species, at }
    }

    /// What every animal can see of its own kind, one entry per animal
    /// in the order they are stored.
    ///
    /// **Two different costs, split apart.** `alarm` has to be current
    /// for *every* animal on *every* tick -- `think` reads it before it
    /// even asks whether this is a tick it gets to think on, because a
    /// panic has to be noticed the instant it happens (see
    /// `Neighbours::alarm`). The other three -- herd centre, quarry,
    /// threat -- are read nowhere else: `think` returns before touching
    /// them unless `next_thought` says this animal's thought is due,
    /// which happens on one tick in about twenty (`THINK_INTERVAL` is a
    /// second; the server ticks a good deal faster than that). This used
    /// to compute all four for every animal on every tick regardless,
    /// which made the full O(n) neighbour scan -- not just the *n* of
    /// it, the *whole* inner loop, hunts-checks and all -- the price of
    /// a single alarm lookup, paid nineteen times out of twenty for
    /// nothing that was ever read. Gating the expensive three on the
    /// same condition `think` itself uses (computed here *before*
    /// `think` subtracts `dt` from `next_thought`, so the two agree
    /// exactly) turns most of a tick's cost from `O(animals^2)` into
    /// `O(animals * awake)`, where `awake` is the handful about to
    /// think.
    ///
    /// `alarm` still has to look at every animal to be current every
    /// tick -- but only ever against the ones that are actually
    /// fleeing, gathered once up front, which is the empty list on
    /// almost every ordinary tick and never more than the population.
    ///
    /// Still no spatial grid: `MAX_ANIMALS` is a hundred and twenty, the
    /// inner test is two subtractions and a compare, and a grid
    /// maintained every tick to save less time than it costs was never
    /// the fix -- doing the expensive part less *often* was. See
    /// `a_crowded_flock_only_pays_the_full_neighbour_scan_when_it_actually_thinks`.
    fn survey(&self, dt: f32) -> (Vec<Neighbours>, u64) {
        // Gathered once rather than filtered out of the full population
        // inside every animal's own pass: on the ordinary tick where
        // nothing is fleeing this is empty, and the loop below costs one
        // pointer chase per animal instead of one per pair.
        let fleeing: Vec<(EntityId, Species, (f32, f32), f32)> = self
            .animals
            .iter()
            .filter(|a| a.mind == Mind::Flee && a.target.is_some())
            .map(|a| (a.id, a.species, (a.at().0, a.at().2), a.wants_yaw))
            .collect();

        let mut full_scans = 0u64;
        let mut seen = Vec::with_capacity(self.animals.len());
        for animal in &self.animals {
            let mut alarm: Option<(f32, f32)> = None;
            // **As far as it can hear one of its own go.** A bolting animal is
            // a running one, and a herd hears that through the trees and round
            // the rock the rest of it is grazing behind -- which is how a deer
            // that never saw you and could not have leaves with the one that
            // did. Never less than `ALARM_RADIUS`, for the animals whose
            // hearing is short because they are busy eating.
            //
            // **A herd of horses goes as one, stallion and all**: as far as
            // the stallion ranges, so a mare grazing at the edge of the herd
            // is off with the rest of it rather than left standing to be
            // walked up to.
            let alarm_reach = if animal.species == Species::Horse {
                STALLION_RANGE
            } else {
                animal.species.hearing().max(ALARM_RADIUS)
            };
            for &(id, species, (px, pz), heading) in &fleeing {
                if id == animal.id || species != animal.species {
                    continue;
                }
                let (dx, dz) = (px - animal.at().0, pz - animal.at().2);
                let distance_sq = dx * dx + dz * dz;
                if distance_sq <= alarm_reach * alarm_reach {
                    match alarm {
                        Some((_, best)) if best <= distance_sq => {}
                        _ => alarm = Some((heading, distance_sq)),
                    }
                }
            }

            // Exactly `think`'s own gate (`next_thought -= dt; if
            // next_thought > 0.0 { return }`), checked here before that
            // subtraction happens rather than after, so this predicts
            // precisely the tick on which `think` would have gone on to
            // read `centre`/`quarry`/`threat`/`company` -- not
            // approximately, since both read the same `dt` and nothing
            // between this call and that animal's own `think` call
            // touches its `next_thought`.
            let (centre, quarry, threat, company, packmate, leader, straggler) = if animal.next_thought <= dt {
                full_scans += 1;
                let mut sum = (0.0f32, 0.0f32);
                let mut count = 0usize;
                let mut quarry: Option<(EntityId, (f32, f32, f32), f32)> = None;
                let mut threat: Option<((f32, f32, f32), f32)> = None;
                let mut packmate: Option<(EntityId, (f32, f32, f32), f32)> = None;
                let mut leader: Option<(EntityId, Leader)> = None;
                let mut straggler: Option<((f32, f32), f32)> = None;
                let keeps_herd = animal.stallion && !animal.keep.is_some_and(|k| k.tame);
                let follows = matches!(animal.species.grouping(), Grouping::Herd | Grouping::Pack);
                // A fed wolf is not hunting, so it does not need to be
                // told where dinner is -- and, more to the point, the
                // deer beside it should be able to graze.
                let hungry = animal.fed_for <= 0.0;
                for other in &self.animals {
                    if other.id == animal.id {
                        continue;
                    }
                    let (dx, dz) = (
                        other.at().0 - animal.at().0,
                        other.at().2 - animal.at().2,
                    );
                    let distance_sq = dx * dx + dz * dz;

                    // ---- something to eat, and something to run from ----
                    if hungry
                        && animal.species.hunts(other.species)
                        && distance_sq <= animal.species.awareness() * animal.species.awareness()
                    {
                        match quarry {
                            Some((_, _, best)) if best <= distance_sq => {}
                            _ => quarry = Some((other.id, other.at(), distance_sq)),
                        }
                    }
                    // Prey notices a hunter at its *own* awareness rather
                    // than at the hunter's: a wolf that could be seen as
                    // far as it can see is a wolf that never gets near
                    // anything. And it only counts a hunter that is
                    // actually hunting.
                    //
                    // **...and a bird goes up off anything that means harm,
                    // hunting it or not.** Nothing here hunts a grouse or a
                    // gull, so until this a covey sat in the grass while a
                    // wolf walked through it -- and a bird that tells nobody
                    // anything is a bird that is scenery. A flock going up
                    // with no person near it is now something coming: a
                    // warning a player can read from across a valley, and
                    // one that costs a compare, because this loop already
                    // looks at every animal.
                    let flushes = animal.species.flies() && other.species.is_hostile();
                    if ((other.species.hunts(animal.species) && other.fed_for <= 0.0) || flushes)
                        && distance_sq <= animal.species.awareness() * animal.species.awareness()
                    {
                        match threat {
                            Some((_, best)) if best <= distance_sq => {}
                            _ => threat = Some((other.at(), distance_sq)),
                        }
                    }

                    if other.species != animal.species {
                        continue;
                    }
                    // The mare furthest out, for a stallion, if one is out
                    // past `STRAY` -- and not one somebody has tamed, which is
                    // no longer his.
                    if keeps_herd
                        && distance_sq > STRAY * STRAY
                        && distance_sq <= STALLION_RANGE * STALLION_RANGE
                        && !other.keep.is_some_and(|k| k.tame)
                        && other.ride.is_none()
                    {
                        match straggler {
                            Some((_, far)) if far >= distance_sq => {}
                            _ => straggler = Some(((other.at().0, other.at().2), distance_sq)),
                        }
                    }
                    if distance_sq <= HERD_RADIUS * HERD_RADIUS {
                        sum.0 += other.at().0;
                        sum.1 += other.at().2;
                        count += 1;
                        match packmate {
                            Some((_, _, best)) if best <= distance_sq => {}
                            _ => packmate = Some((other.id, other.at(), distance_sq)),
                        }
                        if follows && other.id < animal.id && leader.is_none_or(|(best, _)| other.id < best) {
                            leader = Some((
                                other.id,
                                Leader {
                                    at: other.at(),
                                    heading: other.wants_yaw,
                                    moving: other.mind == Mind::Wander,
                                },
                            ));
                        }
                    }
                }
                (
                    (count > 0).then(|| (sum.0 / count as f32, sum.1 / count as f32)),
                    quarry.map(|(id, at, _)| (id, at)),
                    threat.map(|(at, _)| at),
                    count,
                    packmate.map(|(id, at, _)| (id, at)),
                    leader.map(|(_, leader)| leader),
                    straggler.map(|(at, _)| at),
                )
            } else {
                (None, None, None, 0, None, None, None)
            };

            seen.push(Neighbours {
                centre,
                company,
                alarm: alarm.map(|(yaw, _)| yaw),
                quarry,
                threat,
                packmate,
                leader,
                straggler,
            });
        }
        (seen, full_scans)
    }

    /// Pushes apart any two animals standing in each other.
    ///
    /// **Two deer could occupy the same block, and they did.** Nothing in
    /// this file ever made one body take up room from another's point of
    /// view: `fits` asks the *world* whether an animal can be somewhere and
    /// has never heard of other animals, and the herd's `PERSONAL_SPACE` is a
    /// steering rule on a thought -- so an idle deer with six seconds to run
    /// on its next thought simply stood there while another walked into it
    /// and through it. Measured on
    /// `a_herd_left_alone_for_five_minutes_stays_together_and_never_stands_in_itself`:
    /// five deer, five minutes, closest approach four hundredths of a block.
    /// What a player saw was one animal drawn twice.
    ///
    /// **A position correction, not a force.** A repulsion added to the
    /// velocity is a spring, and a spring between five animals that are also
    /// steering toward each other is a herd that hums; moving the overlap out
    /// resolves it in the tick it happened and adds nothing to what the
    /// animal thinks it is doing. Each of the pair takes half, so neither is
    /// privileged by its position in the list.
    ///
    /// **`O(n*n)`, and that is the cheap part of this file.** The inner test
    /// is two subtractions and a compare -- the same argument `survey` makes
    /// for not keeping a spatial grid -- and it only ever touches animals
    /// with their feet on the ground: a bird in the air has the whole sky to
    /// be in and a fish already schools by steering, and neither wants a
    /// solid body.
    ///
    /// Nothing moves into a place `fits` refuses, so an animal in a doorway
    /// is crowded rather than shoved through the wall.
    fn unstack(&mut self, world: &dyn BlockWorld) {
        for i in 0..self.animals.len() {
            for j in (i + 1)..self.animals.len() {
                let (a, b) = (&self.animals[i], &self.animals[j]);
                // A fish schools by steering and a bird in the air has the
                // whole sky; neither wants a solid body. Everything with its
                // feet on something does -- and the test is "is it flying"
                // rather than `on_ground`, because an animal half way up a
                // step is off the ground for a tick and a body that stops
                // being solid while it scrambles is a body two others walk
                // into.
                let airborne = |x: &Animal| x.species.flies() && !x.on_ground;
                if a.species.swims() || b.species.swims() || airborne(a) || airborne(b) {
                    continue;
                }
                let want = (a.species.width() + b.species.width()) * 0.5;
                let (dx, dz) = (b.position.0 - a.position.0, b.position.2 - a.position.2);
                let gap = (dx * dx + dz * dz).sqrt() as f32;
                if gap >= want {
                    continue;
                }
                // Exactly on top of each other: no line to push along, so one
                // is nudged along its own nose and the pair sort themselves
                // out on the next tick. It happens when two are spawned on
                // one spot and essentially never otherwise.
                let (ux, uz) = if gap > 1e-4 {
                    (dx / f64::from(gap), dz / f64::from(gap))
                } else {
                    let (sin, cos) = self.animals[i].yaw.sin_cos();
                    (f64::from(cos), f64::from(sin))
                };
                let push = f64::from((want - gap) * 0.5);
                let apart = [(i, -push), (j, push)];
                for (index, by) in apart {
                    let animal = &self.animals[index];
                    let moved = (
                        animal.position.0 + ux * by,
                        animal.position.1,
                        animal.position.2 + uz * by,
                    );
                    if fits(world, moved, animal.frame()) {
                        self.animals[index].position = moved;
                    }
                }
            }
        }
    }

    /// Applies what the hunters landed on the hunted.
    ///
    /// After the loop rather than inside it, because the animal being
    /// bitten is in the same list as the one biting -- see `Bite`.
    ///
    /// **What dies here leaves nothing.** A deer a wolf pulled down is a
    /// deer the wolf is eating, and dropping its meat and hide on the
    /// ground would mean the best way to hunt in this world is to follow
    /// a pack about and rob it. That is a fine mechanic and it is not
    /// this one: what the player gets out of the ecosystem is that the
    /// world has one, and what the wolf gets is four minutes of not
    /// being hungry.
    fn settle_bites(&mut self, bites: Vec<Bite>) {
        for bite in bites {
            let Some(index) = self.animals.iter().position(|a| a.id == bite.at) else {
                continue; // already eaten by something else this tick
            };
            let animal = &mut self.animals[index];
            animal.health -= bite.damage;
            animal.hurt_for = HURT_SECONDS;
            if animal.health > 0.0 {
                // Bolt, and *keep* bolting: prey that stood still after
                // the first bite would be prey that dies to one wolf
                // standing next to it.
                let (ax, az) = (
                    animal.at().0 - bite.from.0,
                    animal.at().2 - bite.from.2,
                );
                if ax != 0.0 || az != 0.0 {
                    animal.wants_yaw = az.atan2(ax);
                    animal.yaw = animal.wants_yaw;
                }
                animal.mind = Mind::Flee;
                animal.next_thought = FLEE_SECONDS;
                // A wolf's teeth are a reason to graze elsewhere, the
                // same as a spear is. Only the bitten animal, not its
                // herd: the herd saw the wolf, and the wolf moves on.
                animal.remember_danger((animal.at().0, animal.at().2));
                continue;
            }
            self.animals.remove(index);
            self.killed += 1;
            // Everything that was hunting it stops, and whatever was
            // near enough to have made the kill is fed. Fed by proximity
            // rather than by whose bite landed last, because a pack eats
            // together -- and because otherwise two of three wolves stay
            // hungry and immediately start on the next deer.
            let at = bite.from;
            for other in &mut self.animals {
                if other.quarry == Some(bite.at) {
                    other.quarry = None;
                }
                let (dx, dz) = (other.at().0 - at.0, other.at().2 - at.2);
                if other.species.is_predator() && dx * dx + dz * dz <= HERD_RADIUS * HERD_RADIUS {
                    other.fed_for = other.species.fed_seconds();
                    other.mind = Mind::Idle;
                    other.next_thought = 1.0;
                }
            }
        }
    }

    /// Applies what the world did to an animal this tick -- a fall, a
    /// fire, a held breath -- through the same path a mod's `hurt` call
    /// uses. After the loop for the reason `settle_bites` is: `hurt`
    /// needs `&mut self` and the loop that found these is holding `&mut
    /// Animal` borrowed out of it.
    fn settle_hazards(&mut self, hazards: Vec<(EntityId, f32)>) {
        for (id, amount) in hazards {
            // **The death is kept, not thrown away.** `hurt` answers with
            // it so that whoever dealt the damage can lay the body -- a mod
            // does, a spear does -- and this was the one caller that
            // dropped the answer, so a deer run off a cliff or into a fire
            // simply stopped existing. See `take_fallen`.
            //
            // ...and now it is the *fall* that keeps it: `hurt` starts the body
            // going down (`fell`), and `settle_the_falling` hands the death on
            // from where it came to rest. Pushing it here as well was a second
            // carcass.
            let _ = self.hurt(id, amount);
        }
    }

    /// Forgets anything nobody is near.
    ///
    /// The counterpart of the spawner, and the reason the world does not
    /// fill up behind a walking player. Comfortably past the interest
    /// radius, so nothing is despawned while somebody can still see it --
    /// an animal that pops out of existence at the edge of vision is
    /// worse than one that was never there.
    fn forget_the_distant(&mut self, players: &[(primitive_shared::protocol::PlayerId, (f32, f32, f32))]) {
        if self.animals.is_empty() {
            return;
        }
        // **The rule, written down: a wild animal nobody is near is forgotten;
        // a kept one is parked.** "Kept" is anything with a `Keeping` -- fed
        // once, tamed, penned, or born to a kept mother -- and a parked animal
        // is saved with the world (`save_herd`) and comes back when somebody
        // does (`unpark`). A wild deer eighty blocks off is still simply let
        // go: the meadow makes another, and saving every animal ever seen
        // would be a save file that grows as the player explores.
        let day = self.calendar.unwrap_or(0.0);
        let far = |a: &Animal| {
            let at = a.at();
            !players.iter().any(|&(_, p)| (p.0 - at.0).hypot(p.2 - at.2) <= DESPAWN_DISTANCE)
        };
        // Taken apart only when there is somebody to park: this runs every
        // tick, and a world with no flock in it should not pay for one.
        let (kept, wild): (Vec<Animal>, Vec<Animal>) = if self.animals.iter().any(|a| a.keep.is_some() && far(a)) {
            std::mem::take(&mut self.animals).into_iter().partition(|a| a.keep.is_some())
        } else {
            (Vec::new(), std::mem::take(&mut self.animals))
        };
        self.animals = wild;
        for animal in kept {
            let at = animal.at();
            let near = players
                .iter()
                .any(|&(_, p)| (p.0 - at.0).hypot(p.2 - at.2) <= DESPAWN_DISTANCE);
            if near {
                self.animals.push(animal);
            } else {
                self.deaths.push(animal.id);
                self.parked.push((animal, day));
            }
        }
        if self.animals.is_empty() {
            return;
        }
        let before = self.animals.len();
        // Nobody online: everything goes. A world ticking over with no
        // players in it should not be simulating a herd.
        if players.is_empty() {
            self.deaths.extend(self.animals.iter().map(|a| a.id));
            self.animals.clear();
            self.despawned += before as u64;
            return;
        }
        let deaths = &mut self.deaths;
        self.animals.retain(|animal| {
            // Kept animals were sorted out above, and are all near somebody.
            if animal.keep.is_some() {
                return true;
            }
            // A fish is forgotten nearer, because it is born nearer
            // (`FISH_SPAWN_MAX`) and seen nearer: the underwater fog closes
            // at eighteen blocks, so a school sixty blocks off is a school
            // nobody can see from anywhere they could be.
            let limit = if animal.species.swims() {
                FISH_DESPAWN_DISTANCE
            } else {
                DESPAWN_DISTANCE
            };
            let limit_sq = limit * limit;
            let kept = players.iter().any(|&(_, at)| {
                let (dx, dz) = (animal.at().0 - at.0, animal.at().2 - at.2);
                dx * dx + dz * dz <= limit_sq
            });
            if !kept {
                deaths.push(animal.id);
            }
            kept
        });
        self.despawned += (before - self.animals.len()) as u64;
    }

    /// Tries to put something new in the world near somebody.
    /// One species, drawn against `Species::spawn_weight`.
    ///
    /// A ladder of thresholds rather than a shuffled bag: four species
    /// and one roll, which is as much machinery as a weighted pick of
    /// this size is worth. `None` only if every weight is zero, which no
    /// table this code ships has -- but a plugin could, and a zero total
    /// must be "nothing spawns" rather than a division by it.
    ///
    /// **Drawn among the animals that live in `biome` and no others**
    /// (`Species::lives_in`). The other way -- draw from all ten, and give
    /// up when the answer does not live here -- is how the bear is kept to
    /// the woods (`needs_trees`), and it is cheap only because the bear is
    /// one in twenty. Across two countries it throws away a third of every
    /// meadow's attempts on lions and zebra and most of a savanna's on deer
    /// and sheep, so the country with fewer species of its own would fill at
    /// a fraction of the other's rate, for a reason no player could see.
    ///
    /// The draw is one roll either way, so a meadow's sequence is the one it
    /// always was: its animals and their weights are exactly the old table.
    fn pick_species(&mut self, night: bool, biome: primitive_shared::worldgen::Biome) -> Option<Species> {
        // The land's animals only: a fish "lives" in every biome that has
        // water in it, and drawn here it would be a school spawned on turf.
        // Not the gull either: it lives on a beach, and a beach can have turf
        // on it, but it arrives from `populate_shore` or not at all.
        let here = |species: &&Species| !species.swims() && !species.soars() && species.lives_in(biome);
        let total: u32 = Species::ALL.iter().filter(here).map(|s| s.spawn_weight_in(night, biome)).sum();
        if total == 0 {
            return None;
        }
        let mut roll = self.rng.range(0.0, total as f32);
        let mut last = None;
        for &species in Species::ALL.iter().filter(here) {
            roll -= species.spawn_weight_in(night, biome) as f32;
            last = Some(species);
            if roll <= 0.0 {
                return Some(species);
            }
        }
        last
    }

    fn populate(
        &mut self,
        world: &dyn BlockWorld,
        players: &[(primitive_shared::protocol::PlayerId, (f32, f32, f32))],
        night: bool,
    ) {
        // The land's animals against the land's cap: a school in the river
        // beside the meadow is not a deer the meadow has already had.
        // Neither the sea's nor the shore's: a flock of gulls over the beach
        // is counted against its own allowance (`populate_shore`), and must
        // not use up the three a player meets in the meadow behind it.
        // ...and never the kept: a flock in a pen is not the meadow's three
        // (`MAX_KEPT`).
        let on_land = self
            .animals
            .iter()
            .filter(|a| !a.species.swims() && !a.species.soars() && !is_kept(a))
            .count();
        if players.is_empty() || on_land >= MAX_ANIMALS {
            return;
        }
        let cap = (players.len() * MAX_ANIMALS_PER_PLAYER).min(MAX_ANIMALS);
        if on_land >= cap {
            return;
        }
        let Some(&(_, origin)) = self.rng.pick(players) else {
            return;
        };

        // One attempt per call, not a loop until it succeeds. A loop is
        // what turns a spawner into a hang the first time somebody
        // stands in a place where nothing can spawn -- the middle of an
        // ocean, the bottom of a mine -- and the cost of failing is
        // simply that the world fills up a little more slowly there,
        // which is also the truth about those places.
        let angle = self.rng.range(0.0, std::f32::consts::TAU);
        let distance = self.rng.range(SPAWN_MIN, SPAWN_MAX);
        let x = origin.0 + angle.cos() * distance;
        let z = origin.2 + angle.sin() * distance;

        let Some(ground) = surface_under(world, x, origin.1 + 16.0, z) else {
            return;
        };
        // Grass, or the shore's own sand: an animal standing on a rock
        // face, in a desert or on a cave floor is one that came from
        // nowhere. `can_grow_on` is asked with a tuft, because "would a
        // plant grow here" is exactly the question -- and it is one function
        // rather than a second list of what counts as pasture.
        //
        // **Asked twice, cheaply then properly.** Which floors count depends
        // on the species (`spawn_ground`), and the species is not picked
        // until the country is known a dozen lines below; so this first ask
        // is "could *anything* stand here", which throws out the rock face
        // and the cave without doing the work, and the species' own answer
        // comes after it.
        let Some(under) = world.block(x.floor() as i32, ground - 1, z.floor() as i32) else {
            return;
        };
        if !Species::ALL.iter().any(|&species| spawn_ground(species, under)) {
            return;
        }

        // **Which country this is**, asked of the world's generator rather
        // than read off the ground: a savanna's turf is a meadow's turf (see
        // `Species::lives_in`, and `BlockWorld::biome` for a world that
        // cannot say).
        let country = |x: f32, z: f32| world.biome(x.floor() as i32, z.floor() as i32).unwrap_or(UNKNOWN_COUNTRY);
        let Some(species) = self.pick_species(night, country(x, z)) else {
            return;
        };
        // **A bear belongs in the woods.** Asked of the world rather
        // than of the generator, because this file has a `BlockWorld`
        // and nothing else -- and because a wood a player planted is a
        // wood. See `Species::needs_trees`.
        if !spawn_ground(species, under) {
            return;
        }
        if species.needs_trees() && !wooded(world, x, ground, z) {
            return;
        }
        // Room to stand up in. An animal spawned into a one-block gap is
        // an animal wedged in the ceiling.
        if !fits(world, (f64::from(x), f64::from(ground as f32), f64::from(z)), species) {
            return;
        }
        self.spawn(species, (x, ground as f32, z));

        // **The rest of the group.** Everything these animals do
        // together -- a herd that bolts as one, a pack that will not
        // come in until there are two of them -- needs there to *be* a
        // group, and one spawn at a time from a random bearing is how
        // you get a world of solitary deer with herd code behind them.
        //
        // The companions are placed around the first one and each is
        // checked exactly as it was: same ground, same headroom, same
        // caps. A spot that will not take one is simply skipped, so a
        // group at the edge of a wood comes out as however many of it
        // fit in the wood.
        //
        // **Whole, against the hard cap only.** The per-player allowance was
        // checked before every companion, so no group was ever larger than
        // three and a wolf pack was always a pair. It is checked once, above,
        // for whether a group may *start*; the group is then what it is
        // (`animals::MAX_GROUP`), and nothing more arrives until the player's
        // allowance is free again. One herd is one meeting.
        let (low, high) = primitive_shared::animals::group_size(species);
        let wanted = low + self.rng.below(high - low + 1);
        // The last of a herd of horses to arrive is its stallion: see
        // `Animal::stallion`. Marked when the loop is done, however it ends.
        let mut last = None;
        for _ in 1..wanted {
            if self.animals.iter().filter(|a| !is_kept(a)).count() >= MAX_ANIMALS {
                break;
            }
            let bearing = self.rng.range(0.0, std::f32::consts::TAU);
            let spread = self.rng.range(1.5, GROUP_SPREAD);
            let (cx, cz) = (x + bearing.cos() * spread, z + bearing.sin() * spread);
            let Some(ground) = surface_under(world, cx, origin.1 + 16.0, cz) else {
                continue;
            };
            let Some(under) = world.block(cx.floor() as i32, ground - 1, cz.floor() as i32) else {
                continue;
            };
            // ...and the same country: a herd of zebra spawned on the edge
            // of the savanna stops at the edge, rather than putting its last
            // animal among the deer a few blocks over the line.
            if !spawn_ground(species, under)
                || !species.lives_in(country(cx, cz))
                || !fits(world, (f64::from(cx), f64::from(ground as f32), f64::from(cz)), species)
            {
                continue;
            }
            last = self.spawn(species, (cx, ground as f32, cz)).or(last);
        }
        if species == Species::Horse {
            if let Some(stallion) = last.and_then(|id| self.animals.iter_mut().find(|a| a.id == id)) {
                stallion.stallion = true;
            }
        }
    }

    /// Tries to put a school -- or a cod -- in the water near somebody.
    ///
    /// **`populate` for the sea, and every rule of that one turned round.**
    /// Not on grass but in liquid, and in liquid *deep enough for the kind*
    /// (`needs_depth`), because a school in a puddle is a school lying on the
    /// bottom of one. Nearer the player (`FISH_SPAWN_MIN`..`FISH_SPAWN_MAX`),
    /// because what can be seen under water ends at the fog. At any depth in
    /// the column rather than on its floor, and the cod in the bottom third
    /// of it. Against the sea's own cap. And a group placed round the first
    /// is checked exactly as the first was, so a school at the edge of a
    /// pond is however many of it fit in the pond.
    ///
    /// **Asked of the blocks, not of the generator**, for the bear's reason:
    /// a pond a player dug and flooded is water, and fish in it are right.
    /// What the generator is asked is only which sea this is
    /// (`BlockWorld::biome`), because a cod belongs to the open ocean and a
    /// deep lake is not one.
    fn populate_water(
        &mut self,
        world: &dyn BlockWorld,
        players: &[(primitive_shared::protocol::PlayerId, (f32, f32, f32))],
    ) {
        if players.is_empty() {
            return;
        }
        let cap = (players.len() * MAX_FISH_PER_PLAYER).min(MAX_FISH);
        let swimming = |animals: &[Animal]| animals.iter().filter(|a| a.species.swims()).count();
        if swimming(&self.animals) >= cap {
            return;
        }
        let Some(&(_, origin)) = self.rng.pick(players) else {
            return;
        };
        // **A few looks for water, not one.** Land animals take one attempt
        // per call because nearly everywhere a player stands is land; water
        // is the rarer thing, and a pond fifteen blocks across inside a ring
        // of thirty-six fills one attempt in thirty -- a pond that is empty
        // for six minutes after a player sits down beside it. A failed look
        // is at most forty-eight block reads straight down, so four of them
        // every `SPAWN_INTERVAL` is nothing a desert notices.
        let found = (0..WATER_LOOKS).find_map(|_| {
            let angle = self.rng.range(0.0, std::f32::consts::TAU);
            let distance = self.rng.range(FISH_SPAWN_MIN, FISH_SPAWN_MAX);
            let x = origin.0 + angle.cos() * distance;
            let z = origin.2 + angle.sin() * distance;
            water_surface_under(world, x, origin.1 + 16.0, z).map(|cell| (x, z, cell))
        });
        let Some((x, z, surface_cell)) = found else {
            return;
        };
        let Some((floor, surface)) = water_column(world, x, surface_cell as f32 + 0.5, z) else {
            return;
        };
        let country = world.biome(x.floor() as i32, z.floor() as i32).unwrap_or(UNKNOWN_COUNTRY);
        let Some(species) = self.pick_swimmer(country, surface - floor) else {
            return;
        };
        let (low, high) = primitive_shared::animals::group_size(species);
        let wanted = low + self.rng.below(high - low + 1);
        for i in 0..wanted {
            if swimming(&self.animals) >= cap {
                return;
            }
            let (cx, cz) = if i == 0 {
                (x, z)
            } else {
                let bearing = self.rng.range(0.0, std::f32::consts::TAU);
                let spread = self.rng.range(0.8, SCHOOL_SPREAD);
                (x + bearing.cos() * spread, z + bearing.sin() * spread)
            };
            let Some((floor, surface)) = water_column(world, cx, surface_cell as f32 + 0.5, cz) else {
                continue;
            };
            if surface - floor < needs_depth(species) {
                continue;
            }
            let (bottom, top) = swimming_band(species, floor, surface);
            if top < bottom {
                continue;
            }
            let y = self.rng.range(bottom, top);
            let at = (cx, y, cz);
            if !fits(world, primitive_shared::geometry::wide(at), species) || !in_water(world, primitive_shared::geometry::wide(at), species) {
                continue;
            }
            self.spawn(species, at);
        }
    }

    /// One swimmer, drawn against `Species::spawn_weight` among the kinds
    /// that live in this water and fit in this depth of it.
    fn pick_swimmer(&mut self, biome: primitive_shared::worldgen::Biome, depth: f32) -> Option<Species> {
        let here = |species: &&Species| species.swims() && species.lives_in(biome) && needs_depth(**species) <= depth;
        let total: u32 = Species::ALL.iter().filter(here).map(|s| s.spawn_weight(false)).sum();
        if total == 0 {
            return None;
        }
        let mut roll = self.rng.range(0.0, total as f32);
        let mut last = None;
        for &species in Species::ALL.iter().filter(here) {
            roll -= species.spawn_weight(false) as f32;
            last = Some(species);
            if roll <= 0.0 {
                return Some(species);
            }
        }
        last
    }

    /// Puts a flock of gulls over the coast near somebody, if there is coast
    /// near anybody.
    ///
    /// **The coast is found by asking the generator at fixed bearings before
    /// anything is drawn**, and the three shapes weighed were these. Rolling
    /// a spot and checking it, as `populate` does, spends random draws on
    /// every player everywhere -- and a meadow's seeded animals would then do
    /// something different because gulls exist, for a reason no test about
    /// deer could name. Reading the blocks for sand and sea is the bear's way
    /// (`wooded`) and fails here: a beach's sand is a desert's sand, and a
    /// lake is water. The biome at `COAST_BEARINGS` bearings is sixteen noise
    /// lookups, no block read, and no draw at all unless one of them is
    /// coast.
    ///
    /// Then one attempt, as `populate` makes: over the water a flock arrives
    /// soaring, `SOAR_HEIGHT` up; on the shore it arrives standing, never in
    /// a canopy. Against its own allowance (`MAX_SEABIRDS_PER_PLAYER`) and the
    /// land's hard ceiling.
    fn populate_shore(
        &mut self,
        world: &dyn BlockWorld,
        players: &[(PlayerId, (f32, f32, f32))],
        night: bool,
    ) {
        let wild = |animals: &[Animal]| animals.iter().filter(|a| !is_kept(a)).count();
        if players.is_empty() || wild(&self.animals) >= MAX_ANIMALS {
            return;
        }
        let Some(species) = Species::ALL.iter().copied().find(|s| s.soars()) else {
            return;
        };
        let cap = (players.len() * MAX_SEABIRDS_PER_PLAYER).min(MAX_ANIMALS);
        let soaring = |animals: &[Animal]| animals.iter().filter(|a| a.species.soars()).count();
        if soaring(&self.animals) >= cap {
            return;
        }
        let mut coast: Vec<(f32, f32, f32)> = Vec::new();
        for &(_, at) in players {
            for bearing in 0..COAST_BEARINGS {
                let angle = bearing as f32 / COAST_BEARINGS as f32 * std::f32::consts::TAU;
                for distance in [SPAWN_MIN, SPAWN_MAX] {
                    let (x, z) = (at.0 + angle.cos() * distance, at.2 + angle.sin() * distance);
                    if world
                        .biome(x.floor() as i32, z.floor() as i32)
                        .is_some_and(|biome| species.lives_in(biome))
                    {
                        coast.push((x, z, at.1));
                    }
                }
            }
        }
        if coast.is_empty() {
            return;
        }
        // Fewer arrive after dark (`Species::spawn_weight`), out of the
        // gull's own table of four.
        if !self.rng.chance(species.spawn_weight(night) as f32 / 4.0) {
            return;
        }
        let Some(&(bx, bz, from_y)) = self.rng.pick(&coast) else {
            return;
        };
        let (x, z) = (bx + self.rng.range(-6.0, 6.0), bz + self.rng.range(-6.0, 6.0));
        let Some(floor) = sea_or_ground_under(world, x, from_y + 24.0, z) else {
            return;
        };
        let Some(under) = world.block(x.floor() as i32, floor - 1, z.floor() as i32) else {
            return;
        };
        if !species.lives_in(world.biome(x.floor() as i32, z.floor() as i32).unwrap_or(UNKNOWN_COUNTRY))
            || primitive_shared::types::is_leafy(under)
        {
            return;
        }
        let over_the_sea = is_liquid(under);
        let (low, high) = primitive_shared::animals::group_size(species);
        let wanted = low + self.rng.below(high - low + 1);
        for i in 0..wanted {
            if soaring(&self.animals) >= cap || wild(&self.animals) >= MAX_ANIMALS {
                return;
            }
            let (cx, cz) = if i == 0 {
                (x, z)
            } else {
                let bearing = self.rng.range(0.0, std::f32::consts::TAU);
                let spread = self.rng.range(1.5, GROUP_SPREAD);
                (x + bearing.cos() * spread, z + bearing.sin() * spread)
            };
            let Some(floor) = sea_or_ground_under(world, cx, from_y + 24.0, cz) else {
                continue;
            };
            let y = if over_the_sea { floor as f32 + SOAR_HEIGHT } else { floor as f32 };
            if !fits(world, (f64::from(cx), f64::from(y), f64::from(cz)), species) {
                continue;
            }
            if self.spawn(species, (cx, y, cz)).is_some() {
                if let Some(gull) = self.animals.last_mut() {
                    // One shore for the flock, so it circles and lands as one.
                    gull.home = Some((x, z));
                    if over_the_sea {
                        gull.mind = Mind::Soar;
                        gull.bound_for = Some((x, z));
                    }
                }
            }
        }
    }

    #[cfg(test)]
    /// The same, mutable, **for tests only**.
    ///
    /// Nothing in the game moves an animal by hand -- that is what
    /// `step` is for -- but a test that wants a hare *here* rather than
    /// wherever two seconds of wandering put it has no other way to say
    /// so, and a fixture that strikes at where the animal used to be is
    /// a test that fails for a reason nobody can read.
    #[cfg(test)]
    pub fn find_mut_for_test(&mut self, id: EntityId) -> Option<&mut Animal> {
        self.animals.iter_mut().find(|a| a.id == id)
    }

    /// One animal by id, read-only.
    ///
    /// Only tests asked, once: everything in the game that wanted an animal
    /// wanted to change it. The young are the first thing that reads one by
    /// id -- a mother, a newborn's parent -- so it is no longer test-only.
    fn find(&self, id: EntityId) -> Option<&Animal> {
        self.animals.iter().find(|a| a.id == id)
    }
}

/// How near somebody has to come to a parked flock for it to be put back in
/// the world, in blocks. **Well inside `DESPAWN_DISTANCE`**, so an animal
/// at the edge is not parked and unparked on alternate ticks as a player
/// paces about -- each of which is an entity appearing and vanishing on
/// every screen near it.
const UNPARK_DISTANCE: f32 = 64.0;

/// How far off a raid on the pens starts, in blocks: outside the pen and
/// out of the lamplight, inside what a wolf can smell (`Species::awareness`).
const RAID_DISTANCE: (f32, f32) = (14.0, 20.0);

/// The chance, at each of the spawner's tries through a night, that the
/// night's raid comes now. Rolled until it comes once, then not again until
/// the next night (`Animals::raided`): most nights, at a time nobody can
/// wait up for.
const RAID_CHANCE: f32 = 0.05;

/// Near enough to put a hand on it, in blocks from the eye to the animal's
/// feet: a block reach and a body's width more, because an animal is not a
/// cell and a sheep's back is a metre from its feet.
const TEND_REACH: f32 = 3.5;

/// Tame: a kept animal, counted against `MAX_KEPT` and never against the
/// wild's caps.
fn is_kept(animal: &Animal) -> bool {
    animal.keep.is_some_and(|k| k.tame)
}

/// What a right click with something in hand did to a kept animal. See
/// `Animals::tend`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Tended {
    /// It took the feed -- `tamed` if that was the feed that did it --
    /// `gentled` if it was the feed that made a horse willing to be tried
    /// (`husbandry::needs_breaking`).
    Fed { tamed: bool, gentled: bool },
    /// A saddle went on its back: the one in the hand is spent.
    Saddled,
    /// Saddlebags went on: the pair in the hand is spent.
    Bagged,
    /// Sheared: this much wool.
    Shorn(u32),
    /// A bowl of milk.
    Milked,
    /// Nothing happened, and this is why, as the code the player's client
    /// says in their language (`notice`).
    Refused(Notice),
}

/// What a knife on a horse's buckles came to. See `Animals::unbuckle`.
#[derive(Debug, Clone)]
pub enum Unbuckled {
    /// Its saddlebags came off, with this in them.
    Bags(primitive_shared::inventory::Inventory),
    /// Its saddle came off.
    Saddle,
    /// Nothing came off, and why.
    Refused(Notice),
}

/// A kept animal as the save file holds it: what it is, where, and what
/// people have made of it. **Not the whole `Animal`** -- a wolf's grudge, a
/// deer's thirst and a bird's nest are this minute's business and are born
/// fresh after a restart; what a player did to it is what has to last.
#[derive(serde::Serialize, serde::Deserialize)]
struct KeptRecord {
    species: Species,
    position: (f64, f64, f64),
    yaw: f32,
    health: f32,
    growth: f32,
    birth_rest: f32,
    /// Its mother, as an index into the same file's list.
    mother: Option<u32>,
    keep: husbandry::Keeping,
    /// The world day it was parked on, if it was.
    parked_on: Option<f32>,
    /// A horse's saddle, bags and load, and how far its breaking got. See
    /// `HERD_FORMAT_VERSION` for why this is at the end.
    gear: Option<horse::Gear>,
}

/// **Two: the horse's gear went on the end of every record.** bincode writes
/// a struct field by field with no names, so a version-one file read as the
/// new shape would take the next animal's species for this one's gear. The
/// old shape is read and comes in with no gear, which is what every animal
/// in it had -- nothing was ever saddled before this.
const HERD_FORMAT_VERSION: u32 = 2;

#[derive(serde::Serialize, serde::Deserialize)]
struct HerdFile {
    version: u32,
    /// The world day it was written on: what an animal that was in the world
    /// at the time counts its absence from.
    day: f32,
    animals: Vec<KeptRecord>,
}

/// Version one's record: `KeptRecord` before the horse. See
/// `HERD_FORMAT_VERSION`.
#[derive(serde::Deserialize)]
struct KeptRecordV1 {
    species: Species,
    position: (f64, f64, f64),
    yaw: f32,
    health: f32,
    growth: f32,
    birth_rest: f32,
    mother: Option<u32>,
    keep: husbandry::Keeping,
    parked_on: Option<f32>,
}

#[derive(serde::Deserialize)]
struct HerdFileV1 {
    #[allow(dead_code)]
    version: u32,
    day: f32,
    animals: Vec<KeptRecordV1>,
}

#[derive(serde::Deserialize)]
struct HerdVersion {
    version: u32,
}

/// What a right click on a horse's back came to. See `Animals::mount`.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Mounting {
    /// On, and riding: where the horse is, and what it has in it.
    Riding { body: horse::Mount, fettle: horse::Fettle, broke: bool },
    /// A gentled horse had them off (`husbandry::thrown`): where it stood.
    Thrown { at: (f64, f64, f64) },
    /// Not on, and why (`notice`).
    Refused(Notice),
}

impl Animals {
    /// A right click on animal `id` from a player whose eye is at `from`,
    /// with `held` in hand: feed, knife or bowl. What the item does to the
    /// pack is the caller's -- this file never sees a pack -- and it does it
    /// only on the answer this gives.
    pub fn tend(
        &mut self,
        id: EntityId,
        from: (f32, f32, f32),
        held: Option<primitive_shared::types::BlockId>,
    ) -> Tended {
        use primitive_shared::types::{block_kind, is_knife, BLOCK_BOWL};
        let Some(held) = held else {
            return Tended::Refused(Notice::NothingInHand);
        };
        if primitive_shared::types::is_tack(held) {
            return self.saddle_up(id, from, held);
        }
        // Her lamb, while it is still one: asked before the mutable borrow.
        let has_lamb = self
            .find(id)
            .and_then(|a| a.young)
            .and_then(|young| self.find(young))
            .is_some_and(|young| youth::is_young(young.growth));
        let Some(animal) = self.animals.iter_mut().find(|a| a.id == id) else {
            return Tended::Refused(Notice::AnimalGone);
        };
        let at = animal.at();
        let (dx, dy, dz) = (at.0 - from.0, at.1 - from.1, at.2 - from.2);
        if (dx * dx + dy * dy + dz * dz).sqrt() > TEND_REACH + animal.species.width() * 0.5 {
            return Tended::Refused(Notice::TooFarAway);
        }
        if !husbandry::tameable(animal.species) {
            return Tended::Refused(Notice::CannotBeKept);
        }
        let tame = animal.keep.is_some_and(|k| k.tame);
        if is_knife(held) {
            if animal.species != Species::Sheep {
                return Tended::Refused(Notice::NothingToShear);
            }
            if !tame {
                return Tended::Refused(Notice::TameItFirst);
            }
            return match animal.keep.as_mut().and_then(|k| k.shear()) {
                Some(wool) => Tended::Shorn(wool),
                None => Tended::Refused(Notice::FleeceNotGrown),
            };
        }
        if block_kind(held) == BLOCK_BOWL {
            if animal.species != Species::Sheep || youth::is_young(animal.growth) || !tame || !has_lamb {
                return Tended::Refused(Notice::OnlyEweGivesMilk);
            }
            return if animal.keep.as_mut().is_some_and(|k| k.milk(has_lamb)) {
                Tended::Milked
            } else {
                Tended::Refused(Notice::NoMilkYet)
            };
        }
        // **A wild horse will not eat from a hand it is running from.** The
        // approach is the taming: a horse sees a walker at fourteen blocks
        // and bolts (`Species::awareness`), and the player who reaches one
        // is the player who crept (`Gait`). A horse caught at the end of a
        // bolt by somebody who sprinted after it is still bolting.
        let wild = !animal.keep.is_some_and(|k| k.tame);
        if animal.species == Species::Horse && wild && animal.mind == Mind::Flee {
            return Tended::Refused(Notice::ShiesAway);
        }
        let fresh = animal.keep.is_none();
        let was_gentled = animal.keep.is_some_and(|k| k.gentled());
        let keep = animal.keep.get_or_insert_with(husbandry::Keeping::wild);
        match keep.feed(animal.species, held, at) {
            Ok(tamed) => {
                let gentled = keep.gentled() && !was_gentled;
                // Its head is in your hand: whatever it was wary of, it is
                // not now.
                animal.wary_for = 0.0;
                animal.threat_at = None;
                animal.target = None;
                if animal.mind != Mind::Flee {
                    animal.mind = Mind::Idle;
                }
                Tended::Fed { tamed, gentled }
            }
            Err(refused) => {
                // Offered the wrong thing, a wild animal is still nobody's:
                // a keeping put on it only to be refused would be an animal
                // parked and saved for having been shown a plank.
                if fresh {
                    animal.keep = None;
                }
                Tended::Refused(match refused {
                    husbandry::Refused::Sated => Notice::NotHungry,
                    husbandry::Refused::NotItsFood => Notice::DoesNotEatThat,
                })
            }
        }
    }

    /// Where kept animals left dung since the tick loop last asked.
    pub fn take_dung(&mut self) -> Vec<(f64, f64, f64)> {
        std::mem::take(&mut self.dung)
    }

    /// The haystacks kept animals ate from since the tick loop last asked,
    /// a cell a bite.
    pub fn take_hay_eaten(&mut self) -> Vec<(i32, i32, i32)> {
        std::mem::take(&mut self.hay_eaten)
    }

    /// What dead horses left on the ground since the tick loop last asked:
    /// see `Animals::spilled`.
    pub fn take_spilled(&mut self) -> Vec<Spilled> {
        std::mem::take(&mut self.spilled)
    }

    /// Who the monkeys robbed since the tick loop last asked, and which
    /// monkey did each: see `Animals::thefts`.
    pub fn take_thefts(&mut self) -> Vec<(PlayerId, EntityId)> {
        std::mem::take(&mut self.thefts)
    }

    /// Tells the animals whether it is raining, for the kept horses standing
    /// out in it. A setter for `carrying_fire`'s reason.
    pub fn rain(&mut self, raining: bool) {
        self.raining = raining;
    }

    /// A saddle or saddlebags held out to animal `id`: on, if it is a broken
    /// horse and not already wearing one.
    ///
    /// **Only on a tame horse.** A gentled one throws its rider still, and a
    /// saddle on a horse nobody can sit is a saddle a player has to take the
    /// word of the game for; the breaking is bareback, as it is.
    fn saddle_up(&mut self, id: EntityId, from: (f32, f32, f32), held: primitive_shared::types::BlockId) -> Tended {
        let Some(animal) = self.animals.iter_mut().find(|a| a.id == id) else {
            return Tended::Refused(Notice::AnimalGone);
        };
        let at = animal.at();
        let (dx, dy, dz) = (at.0 - from.0, at.1 - from.1, at.2 - from.2);
        if (dx * dx + dy * dy + dz * dz).sqrt() > TEND_REACH + animal.species.width() * 0.5 {
            return Tended::Refused(Notice::TooFarAway);
        }
        if animal.species != Species::Horse {
            return Tended::Refused(Notice::GoesOnAHorse);
        }
        if !animal.keep.is_some_and(|k| k.tame) {
            return Tended::Refused(Notice::BreakItFirst);
        }
        if youth::is_young(animal.growth) {
            return Tended::Refused(Notice::TooYoungToCarry);
        }
        let gear = animal.gear.get_or_insert_with(Default::default);
        if primitive_shared::types::block_kind(held) == primitive_shared::types::BLOCK_SADDLE {
            if gear.saddle {
                return Tended::Refused(Notice::AlreadySaddled);
            }
            gear.saddle = true;
            Tended::Saddled
        } else {
            if gear.bags.is_some() {
                return Tended::Refused(Notice::AlreadyBagged);
            }
            gear.bags = Some(primitive_shared::inventory::Inventory::new());
            Tended::Bagged
        }
    }

    /// **A knife on a horse's buckles**: its saddlebags off, or -- when it
    /// wears none -- its saddle. What the item does to the pack is the
    /// caller's; `takes` says whether the pack has room for the bags and
    /// everything in them, and is asked only when there are bags to take.
    ///
    /// ## Why a knife, and why the bags first
    ///
    /// The knife is already the tool for taking something *off* an animal --
    /// the fleece -- and a horse had nothing for it to do. A gesture of its
    /// own (the rein key and a click is the bags' door, `OpenBags`) would be
    /// one more thing to be told. The bags come first because they hang over
    /// the saddle: two clicks strip a horse, and one takes off only the load.
    ///
    /// ## Where the load goes
    ///
    /// **Into the player's pack with the bags, or the bags stay on.** Three
    /// ways were weighed. Bags that come off only when empty make unloading a
    /// chore of dragging every stack out first, for a rule with no decision
    /// in it. Bags that spill what will not fit on the ground would lose a
    /// load in the grass to a mis-click. So the bags come off whole when the
    /// pack can take them and all they hold, and otherwise they stay on and
    /// the player is told: nothing is lost, and nothing is a chore when there
    /// is room.
    pub fn unbuckle(
        &mut self,
        id: EntityId,
        from: (f32, f32, f32),
        takes: impl FnOnce(&primitive_shared::inventory::Inventory) -> bool,
    ) -> Unbuckled {
        let Some(animal) = self.animals.iter_mut().find(|a| a.id == id) else {
            return Unbuckled::Refused(Notice::AnimalGone);
        };
        let at = animal.at();
        let (dx, dy, dz) = (at.0 - from.0, at.1 - from.1, at.2 - from.2);
        if (dx * dx + dy * dy + dz * dz).sqrt() > TEND_REACH + animal.species.width() * 0.5 {
            return Unbuckled::Refused(Notice::TooFarAway);
        }
        if animal.ride.is_some() {
            return Unbuckled::Refused(Notice::SomebodyOnIt);
        }
        let Some(gear) = animal.gear.as_mut() else {
            return Unbuckled::Refused(Notice::NothingToUnbuckle);
        };
        if let Some(bags) = &gear.bags {
            if !takes(bags) {
                return Unbuckled::Refused(Notice::PackCannotTakeBags);
            }
            return Unbuckled::Bags(gear.bags.take().expect("the bags were just there"));
        }
        if gear.saddle {
            gear.saddle = false;
            return Unbuckled::Saddle;
        }
        Unbuckled::Refused(Notice::NothingToUnbuckle)
    }

    /// A player at `from` getting on horse `id`.
    ///
    /// **Three answers, and the middle one is the breaking.** A tame horse
    /// stands for its rider. A gentled one -- fed enough, never sat on
    /// (`Keeping::gentled`) -- is tried: `husbandry::thrown` on the times it
    /// has been got on already, and a throw puts the rider on the ground and
    /// the horse off a few strides and unwilling for `SETTLE_SECONDS`. A horse
    /// that stays still under them is broken on the spot, and home is where
    /// it happened. Anything else refuses to be got near.
    pub fn mount(&mut self, id: EntityId, rider: PlayerId, from: (f32, f32, f32)) -> Mounting {
        if self.animals.iter().any(|a| a.ride.is_some_and(|r| r.rider == rider)) {
            return Mounting::Refused(Notice::AlreadyRiding);
        }
        let roll = self.rng.range(0.0, 1.0);
        let Some(animal) = self.animals.iter_mut().find(|a| a.id == id) else {
            return Mounting::Refused(Notice::AnimalGone);
        };
        if animal.species != Species::Horse {
            return Mounting::Refused(Notice::CannotRideThat);
        }
        let at = animal.at();
        if (at.0 - from.0).hypot(at.2 - from.2) > horse::MOUNT_REACH + animal.species.width() * 0.5
            || (at.1 - from.1).abs() > 2.5
        {
            return Mounting::Refused(Notice::TooFarAway);
        }
        if animal.ride.is_some() {
            return Mounting::Refused(Notice::SomebodyOnIt);
        }
        if youth::is_young(animal.growth) {
            return Mounting::Refused(Notice::TooYoungToRide);
        }
        let Some(keep) = animal.keep.as_mut() else {
            return Mounting::Refused(Notice::GentleItFirst);
        };
        let mut broke = false;
        if !keep.tame {
            if !keep.gentled() {
                return Mounting::Refused(Notice::GentleItFirst);
            }
            if animal.settle_for > 0.0 {
                return Mounting::Refused(Notice::LetItSettle);
            }
            let gear = animal.gear.get_or_insert_with(Default::default);
            let attempt = gear.rides;
            gear.rides = gear.rides.saturating_add(1);
            if husbandry::thrown(attempt, roll) {
                animal.settle_for = husbandry::SETTLE_SECONDS;
                // Off a few strides, the way from the rider it threw, and
                // then it stands: a thrown rider is looking at a horse that
                // has not gone anywhere, which is what makes the next try
                // a decision rather than a chase.
                let away = (at.2 - from.2).atan2(at.0 - from.0);
                animal.mind = Mind::Flee;
                animal.wants_yaw = away;
                animal.next_thought = 1.2;
                return Mounting::Thrown { at: animal.position };
            }
            keep.break_in(at);
            broke = true;
        }
        let fettle = fettle_of(animal.keep.as_ref(), animal.gear.as_deref());
        let (x, y, z) = animal.position;
        let body = horse::Mount::standing(x, y, z, animal.yaw, fettle.most_wind);
        animal.ride = Some(Ride { rider, body, reins: horse::Reins::SLACK, reins_age: 0.0, jump_for: 0.0 });
        animal.mind = Mind::Idle;
        animal.target = None;
        animal.velocity = (0.0, 0.0, 0.0);
        Mounting::Riding { body, fettle, broke }
    }

    /// Off horse `id`, if `rider` is the one on it. Answers where the horse
    /// stood.
    pub fn dismount(&mut self, id: EntityId, rider: PlayerId) -> Option<horse::Mount> {
        let animal = self.animals.iter_mut().find(|a| a.id == id)?;
        let ride = animal.ride.filter(|r| r.rider == rider)?;
        animal.ride = None;
        animal.velocity = (0.0, 0.0, 0.0);
        animal.mind = Mind::Idle;
        animal.next_thought = 2.0;
        Some(ride.body)
    }

    /// The reins, from `rider`, for horse `id`. Ignored unless they are the
    /// one on it -- a passer-by's reins steer nothing.
    pub fn rein(&mut self, id: EntityId, rider: PlayerId, reins: horse::Reins) -> bool {
        let Some(ride) = self.animals.iter_mut().find(|a| a.id == id).and_then(|a| a.ride.as_mut()) else {
            return false;
        };
        if ride.rider != rider {
            return false;
        }
        ride.reins = reins.clamped();
        ride.reins_age = 0.0;
        if reins.jump {
            ride.jump_for = JUMP_LATCH;
        }
        true
    }

    /// The horse `rider` is on, where it is, and what it has in it.
    pub fn ridden_by(&self, rider: PlayerId) -> Option<(EntityId, horse::Mount, horse::Fettle)> {
        self.animals.iter().find_map(|a| {
            let ride = a.ride.filter(|r| r.rider == rider)?;
            Some((a.id, ride.body, fettle_of(a.keep.as_ref(), a.gear.as_deref())))
        })
    }

    /// Everybody on a horse, and which: the tick loop's list, for putting the
    /// riders on their saddles (`horses::tick`).
    pub fn riders(&self) -> Vec<(PlayerId, EntityId, horse::Mount)> {
        self.animals.iter().filter_map(|a| a.ride.map(|r| (r.rider, a.id, r.body))).collect()
    }

    /// A player went away: off whatever they were riding.
    pub fn forget_rider(&mut self, rider: PlayerId) {
        for animal in &mut self.animals {
            if animal.ride.is_some_and(|r| r.rider == rider) {
                animal.ride = None;
                animal.velocity = (0.0, 0.0, 0.0);
            }
        }
    }

    /// The saddlebags on horse `id`, if it wears a pair and is within reach
    /// of an eye at `from`: what `OpenBags` and every gesture at them ask.
    pub fn bags_within(&mut self, id: EntityId, from: (f32, f32, f32), reach: f32) -> Option<&mut primitive_shared::inventory::Inventory> {
        let animal = self.animals.iter_mut().find(|a| a.id == id)?;
        let at = animal.at();
        if (at.0 - from.0).hypot(at.2 - from.2) > reach + animal.species.width() * 0.5 || (at.1 - from.1).abs() > 3.0 {
            return None;
        }
        animal.gear.as_mut()?.bags.as_mut()
    }

    /// Every horse in the world whose bags hold something `changing` would
    /// answer yes for, and where its bags are: the rot clock's list
    /// (`rot::Rot::pass`). Parked horses are not on it, as a chest in a chunk
    /// nobody has loaded is not.
    pub fn bags_to_age(
        &self,
        changing: impl Fn(&primitive_shared::inventory::Inventory) -> bool,
    ) -> Vec<(EntityId, (f32, f32, f32))> {
        self.animals
            .iter()
            .filter(|a| a.gear.as_ref().and_then(|g| g.bags.as_ref()).is_some_and(&changing))
            .map(|a| {
                let at = a.at();
                (a.id, (at.0, at.1 + 1.0, at.2))
            })
            .collect()
    }

    /// `edit` over the bags of each horse named, with what was sampled for
    /// it. Answers the horses whose bags changed.
    pub fn edit_bags<A>(
        &mut self,
        which: &[(EntityId, A)],
        mut edit: impl FnMut(&mut primitive_shared::inventory::Inventory, &A) -> bool,
    ) -> Vec<EntityId> {
        let mut changed = Vec::new();
        for (id, sampled) in which {
            let bags = self.animals.iter_mut().find(|a| a.id == *id).and_then(|a| a.gear.as_mut()).and_then(|g| g.bags.as_mut());
            if bags.is_some_and(|bags| edit(bags, sampled)) {
                changed.push(*id);
            }
        }
        changed
    }

    /// Puts a keeping and gear on an animal outright: scenarios and mods, for
    /// an animal somebody is meant to have kept already.
    pub fn put_keeping(&mut self, id: EntityId, keep: husbandry::Keeping, gear: Option<horse::Gear>) {
        if let Some(animal) = self.animals.iter_mut().find(|a| a.id == id) {
            animal.keep = Some(keep);
            animal.gear = gear.map(Box::new);
        }
    }

    /// What a horse wears and carries.
    pub fn gear(&self, id: EntityId) -> Option<horse::Gear> {
        self.find(id).and_then(|a| a.gear.as_deref().cloned())
    }

    /// Where horse `id` is. A dead or forgotten horse is nowhere.
    pub fn horse_at(&self, id: EntityId) -> Option<(f64, f64, f64)> {
        self.find(id).filter(|a| a.species == Species::Horse).map(|a| a.position)
    }

    /// What a horse is wearing, for tests.
    #[cfg(test)]
    pub fn gear_for_test(&mut self, id: EntityId) -> Option<&mut horse::Gear> {
        let animal = self.animals.iter_mut().find(|a| a.id == id)?;
        Some(animal.gear.get_or_insert_with(Default::default))
    }

    /// Whether this animal is its herd's stallion. Tests.
    #[cfg(test)]
    pub fn is_stallion(&self, id: EntityId) -> bool {
        self.find(id).is_some_and(|a| a.stallion)
    }

    /// Makes one its herd's stallion. Tests.
    #[cfg(test)]
    pub fn make_stallion_for_test(&mut self, id: EntityId) {
        if let Some(animal) = self.animals.iter_mut().find(|a| a.id == id) {
            animal.stallion = true;
        }
    }

    /// The days that went by, for every kept animal in the world: hunger,
    /// fleece, trust, condition and dung (`Keeping::winter_through`).
    /// **Grazing is asked of the ground under it and of the season**, so a
    /// pen on turf is half a flock's keep in summer and none in winter, and a
    /// pen on bare earth is none at all -- and **a haystack in reach is eaten
    /// from** when an animal is getting hungry, which is what a flock lives on
    /// in winter while nobody is there to feed it.
    fn keep_the_kept(&mut self, world: &dyn BlockWorld, days: f32) {
        if days <= 0.0 {
            return;
        }
        self.manger_clock = self.manger_clock.wrapping_add(1);
        // A lump of days (a night slept through) always looks: the whole of
        // it is decided in this one call.
        let look = days > 0.01 || self.manger_clock.is_multiple_of(MANGER_LOOK_EVERY);
        let from_day = self.calendar.map(|today| today - days);
        let mut taken = std::collections::HashMap::new();
        for animal in &mut self.animals {
            let Some(keep) = animal.keep.as_mut() else {
                continue;
            };
            let (x, y, z) = animal.position;
            let grazing = world
                .block(x.floor() as i32, y.floor() as i32 - 1, z.floor() as i32)
                .is_some_and(is_pasture);
            let wants_hay = look
                && keep.tame
                && husbandry::eats_hay(animal.species)
                && keep.hunger + days >= husbandry::STACK_AFTER_DAYS;
            let stacks = if wants_hay { stacks_in_reach(world, animal.position, &taken) } else { Vec::new() };
            let mut hay = stacks.iter().map(|&(_, left)| left).sum();
            let (dung, eaten) = keep.winter_through(animal.species, days, from_day, grazing, &mut hay);
            take_bites(&stacks, eaten, &mut taken, &mut self.hay_eaten);
            for _ in 0..dung {
                self.dung.push(animal.position);
            }
            // **A kept horse out in the rain** loses condition: see
            // `husbandry::EXPOSED_CONDITION_PER_DAY`. The sky over its head,
            // read from the cell its head is in; a roof anywhere above it is
            // shelter, a tree included -- which is the lean-to's argument for
            // a stable, not a rule against a wood.
            if self.raining && animal.species == Species::Horse && keep.tame {
                let head = (x.floor() as i32, y.floor() as i32 + 1, z.floor() as i32);
                if primitive_shared::pit::open_to_the_sky(|bx, by, bz| world.block(bx, by, bz), head) {
                    keep.exposed(days);
                }
            }
            if keep.forgotten() {
                animal.keep = None;
            }
        }
    }

    /// Puts parked animals back in the world when somebody comes near, with
    /// the days they were alone passed over them.
    ///
    /// **The absence counts as bare ground unless it is turf**, read off the
    /// cell it was left standing on: a flock parked in a meadow pen grazed
    /// while nobody watched. No dung is laid for the absence -- a week of
    /// pats landing in one tick is a pen buried the moment a player looks at
    /// it -- and that is the one thing a parked flock does differently from
    /// a watched one.
    fn unpark(&mut self, world: &dyn BlockWorld, players: &[(PlayerId, (f32, f32, f32))]) {
        if self.parked.is_empty() || players.is_empty() {
            return;
        }
        // Bites already promised out of each stack this call, so two sheep
        // met again together do not both eat the stack's last bite.
        let mut parked_bites = std::collections::HashMap::new();
        let mut index = 0;
        while index < self.parked.len() {
            let at = self.parked[index].0.at();
            let (bx, by, bz) = (at.0.floor() as i32, at.1.floor() as i32, at.2.floor() as i32);
            let near = players.iter().any(|&(_, p)| (p.0 - at.0).hypot(p.2 - at.2) <= UNPARK_DISTANCE);
            // **Only onto ground that is there.** Put back over a chunk the
            // server has not loaded yet, it would fall through the world.
            let under = world.block(bx, by - 1, bz);
            if !near || under.is_none() || world.block(bx, by, bz).is_none() {
                index += 1;
                continue;
            }
            let (mut animal, since) = self.parked.swap_remove(index);
            if let Some(today) = self.calendar {
                // **Up to where the step's own days begin, and not to
                // today.** The days still pending (`days_pending`) are passed
                // over every animal in the world by `keep_the_kept` and
                // `raise_young` later in this same step -- this one included,
                // now that it is back -- so an absence counted to today was
                // the last of it lived twice: a flock met again after a jump
                // of the calendar came back doubly hungry, and with a stack
                // beside it ate that stack twice over.
                let away = (today - self.days_pending - since).max(0.0);
                let grazing = under.is_some_and(is_pasture);
                if let Some(keep) = animal.keep.as_mut() {
                    // **The stack by the pen fed it while nobody was there**,
                    // bite by bite through the absence exactly as a watched
                    // pen's would have (`Keeping::winter_through`): a flock
                    // left for the winter with hay beside it is a flock that
                    // was fed, and the stack is lower by what it ate.
                    let stacks = if keep.tame && husbandry::eats_hay(animal.species) {
                        stacks_in_reach(world, animal.position, &parked_bites)
                    } else {
                        Vec::new()
                    };
                    let mut hay = stacks.iter().map(|&(_, left)| left).sum();
                    let (_pats_nobody_saw, eaten) =
                        keep.winter_through(animal.species, away, Some(since), grazing, &mut hay);
                    take_bites(&stacks, eaten, &mut parked_bites, &mut self.hay_eaten);
                    if keep.forgotten() {
                        animal.keep = None;
                    }
                }
                if youth::is_young(animal.growth) {
                    animal.growth = (animal.growth + away / youth::GROWN_DAYS).min(youth::GROWN);
                }
                animal.birth_rest = (animal.birth_rest - away).max(0.0);
            }
            self.births.push((animal.id, at));
            self.animals.push(animal);
        }
    }

    /// **Something comes for the pens at night.** Once a night at most, at a
    /// random try (`RAID_CHANCE`), a hunter that lives in that country and
    /// eats what is kept is put down `RAID_DISTANCE` from one kept animal's
    /// home -- and from there it is an ordinary hungry wolf: it finds the
    /// flock by `survey`'s quarry, and it gets in or it does not.
    ///
    /// **What keeps it out is the pen, not a rule here.** Two blocks of wall
    /// is more than anything steps (`STEP_HEIGHT`), a fire inside keeps a
    /// wolf off as it does in the open (`FIRE_RADIUS`), and a flock grazing
    /// loose on its leash has no wall at all. A one-block wall is a step for
    /// the wolf and the sheep alike; a ring of stakes cuts the flock every
    /// time one of them is shoved against it (`stakes`), which makes it a pen
    /// that butchers what it keeps.
    ///
    /// Rejected: **a raid that always comes.** Then the pen is a tax paid in
    /// walls, and there is no night on which leaving the flock out is a bet
    /// that might come off.
    fn raid_the_pens(&mut self, world: &dyn BlockWorld, night: bool) {
        let Some(today) = self.calendar else {
            return;
        };
        // Noon to noon: one number for the whole of one night.
        let this_night = (today + 0.5).floor() as i64;
        if !night || self.raided == Some(this_night) || self.rng.range(0.0, 1.0) >= RAID_CHANCE {
            return;
        }
        let homes: Vec<(Species, (f32, f32, f32))> = self
            .animals
            .iter()
            .filter_map(|a| a.keep.filter(|k| k.tame).and_then(|k| k.home).map(|home| (a.species, home)))
            .collect();
        let Some(&(prey, home)) = self.rng.pick(&homes) else {
            return;
        };
        self.raided = Some(this_night);
        self.raid(world, prey, home);
    }

    /// The raid itself: hunters of `prey` round `home`, as many as the
    /// smallest group of their kind. Split out so a test can send one.
    fn raid(&mut self, world: &dyn BlockWorld, prey: Species, home: (f32, f32, f32)) -> Vec<EntityId> {
        let country = world.biome(home.0.floor() as i32, home.2.floor() as i32).unwrap_or(UNKNOWN_COUNTRY);
        let hunters: Vec<Species> = Species::ALL
            .iter()
            .copied()
            .filter(|h| h.is_predator() && h.hunts(prey) && h.lives_in(country) && !h.needs_trees())
            .collect();
        let Some(&hunter) = self.rng.pick(&hunters) else {
            return Vec::new();
        };
        let bearing = self.rng.range(0.0, std::f32::consts::TAU);
        let distance = self.rng.range(RAID_DISTANCE.0, RAID_DISTANCE.1);
        let (low, _) = primitive_shared::animals::group_size(hunter);
        let mut sent = Vec::new();
        for n in 0..low.max(1) {
            let heading = bearing + n as f32 * 0.3;
            let (x, z) = (home.0 + heading.cos() * distance, home.2 + heading.sin() * distance);
            let Some(ground) = surface_under(world, x, home.1 + 16.0, z) else {
                continue;
            };
            if fits(world, (f64::from(x), f64::from(ground as f32), f64::from(z)), hunter) {
                sent.extend(self.spawn(hunter, (x, ground as f32, z)));
            }
        }
        sent
    }

    /// **The night that is skipped, asked whether it would have found a
    /// sleeper.** `odds` is `animals::found_asleep_odds` for the place round
    /// the bed; rolled once, and if it comes up, the night's hunters are put
    /// down `SLEEPER_FOUND_DISTANCE` from the bed and their ids come back --
    /// an empty list is a night that passed quietly.
    ///
    /// **Why the night has to be asked at all.** It passes the moment
    /// everybody is asleep (`night_may_pass`), which in singleplayer is two
    /// and a half seconds after lying down, and no wolf walking about in the
    /// world can find anybody in that. Without this, the rule that a pack
    /// comes for a sleeper after dark (`think_hunter`'s `sleeper`) would only
    /// ever apply on a server where somebody else was still awake -- and
    /// sleeping in the open would be exactly as safe as sleeping behind a
    /// door for everyone playing alone.
    ///
    /// **They come in at once** (`FOUND_GRUDGE`), from two seconds' run
    /// away (`SLEEPER_FOUND_DISTANCE`): this is the attack the sleeper wakes
    /// to, not a pack that might wander up later.
    ///
    /// **What comes is whatever hunts at night here** (`shies_from_fire`: a
    /// wolf in the woods and meadows, a lion on the savanna), and in the
    /// smallest number that has the nerve to come at a standing person: two
    /// wolves (`needs_company`), one lion. Not the three a pen raid sends
    /// (`raid`): this is somebody who has just been woken with nothing in
    /// their hand, and a pair is a fight a spear can win or a fire can end.
    /// Where nothing hunts at night -- a beach, a desert, the high ground --
    /// nothing comes, and the night passes; the country is part of the bet.
    ///
    /// **Once a night.** A sleeper who was found, dealt with it and lay down
    /// again is not rolled for twice: whatever is still out there is out
    /// there for real, and comes or does not by its own rules.
    ///
    /// Rejected: *a pack spawned in the world when somebody lies down, left to
    /// find them on its own.* The night would have passed before it had
    /// taken a step. *Holding the night back while a sleeper is exposed*
    /// makes the bed stop working in the open, which is a rule and not a
    /// risk.
    pub fn find_the_sleeper(&mut self, world: &dyn BlockWorld, bed: (f32, f32, f32), odds: f32) -> Vec<EntityId> {
        let this_night = self.calendar.map(|today| (today + 0.5).floor() as i64);
        if this_night.is_some() && self.found_sleeper == this_night {
            return Vec::new();
        }
        if odds <= 0.0 {
            return Vec::new();
        }
        let roll = match self.sleeper_dice {
            Some(rigged) => rigged,
            None => self.rng.range(0.0, 1.0),
        };
        if roll >= odds {
            return Vec::new();
        }
        let country = world.biome(bed.0.floor() as i32, bed.2.floor() as i32).unwrap_or(UNKNOWN_COUNTRY);
        let hunters: Vec<Species> = Species::ALL
            .iter()
            .copied()
            .filter(|h| h.shies_from_fire() && h.lives_in(country) && !h.needs_trees())
            .collect();
        let Some(&hunter) = self.rng.pick(&hunters) else {
            return Vec::new();
        };
        let count = if hunter.needs_company() { 2 } else { 1 };
        let bearing = self.rng.range(0.0, std::f32::consts::TAU);
        let mut sent = Vec::new();
        for n in 0..count {
            // The second a little round from the first, so the pair is a
            // pair and not one animal drawn twice -- and so `flank` has two
            // sides to work from.
            let heading = bearing + n as f32 * 0.7;
            let distance = self.rng.range(SLEEPER_FOUND_DISTANCE.0, SLEEPER_FOUND_DISTANCE.1);
            let (x, z) = (bed.0 + heading.cos() * distance, bed.2 + heading.sin() * distance);
            let Some(ground) = surface_under(world, x, bed.1 + 8.0, z) else {
                continue;
            };
            if fits(world, (f64::from(x), f64::from(ground as f32), f64::from(z)), hunter) {
                if let Some(id) = self.spawn(hunter, (x, ground as f32, z)) {
                    // Facing the bed, and coming: they came *for* it.
                    if let Some(animal) = self.animals.iter_mut().find(|a| a.id == id) {
                        animal.yaw = (bed.2 - z).atan2(bed.0 - x);
                        animal.wants_yaw = animal.yaw;
                        animal.angry_for = FOUND_GRUDGE;
                    }
                    sent.push(id);
                }
            }
        }
        if !sent.is_empty() {
            self.found_sleeper = this_night;
        }
        sent
    }

    /// Rigs `find_the_sleeper`'s roll to `dice` -- `None` rolls again. For
    /// a scenario: see `sleeper_dice`.
    pub fn set_sleeper_dice(&mut self, dice: Option<f32>) {
        self.sleeper_dice = dice;
    }

    /// Where every living one of `species` is. For a scenario.
    pub fn positions_of(&self, species: Species) -> Vec<(f32, f32, f32)> {
        self.animals.iter().filter(|a| a.species == species).map(|a| a.at()).collect()
    }

    /// Writes every kept animal -- in the world and parked -- to `herd.bin`
    /// beside the other saves, atomically, the way the carcasses are written.
    pub fn save_herd(&self, dir: &std::path::Path) -> std::io::Result<usize> {
        let day = self.calendar.unwrap_or(0.0);
        let kept: Vec<(&Animal, Option<f32>)> = self
            .animals
            .iter()
            .filter(|a| a.keep.is_some())
            .map(|a| (a, None))
            .chain(self.parked.iter().map(|(a, since)| (a, Some(*since))))
            .collect();
        let index_of = |id: EntityId| kept.iter().position(|(a, _)| a.id == id).map(|i| i as u32);
        let animals: Vec<KeptRecord> = kept
            .iter()
            .filter_map(|&(a, parked_on)| {
                Some(KeptRecord {
                    species: a.species,
                    position: a.position,
                    yaw: a.yaw,
                    health: a.health,
                    growth: a.growth,
                    birth_rest: a.birth_rest,
                    mother: a.mother.and_then(index_of),
                    keep: a.keep?,
                    parked_on,
                    gear: a.gear.as_deref().cloned(),
                })
            })
            .collect();
        let count = animals.len();
        let bytes = bincode::serialize(&HerdFile { version: HERD_FORMAT_VERSION, day, animals })
            .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))?;
        std::fs::create_dir_all(dir)?;
        let final_path = dir.join("herd.bin");
        let tmp_path = final_path.with_extension("bin.tmp");
        std::fs::write(&tmp_path, &bytes)?;
        std::fs::rename(&tmp_path, &final_path)?;
        Ok(count)
    }

    /// Reads the kept animals back, **parked**: each comes into the world the
    /// first time somebody is near it (`unpark`), with the days since the
    /// save passed over it. A missing, unreadable or older file is a world
    /// with no kept animals -- the carcasses' bargain: losing a flock is bad,
    /// and refusing to open the world over it is worse.
    pub fn load_herd(&mut self, dir: &std::path::Path) -> std::io::Result<usize> {
        let bytes = match std::fs::read(dir.join("herd.bin")) {
            Ok(bytes) => bytes,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(0),
            Err(e) => return Err(e),
        };
        let Ok(version) = bincode::deserialize::<HerdVersion>(&bytes) else {
            return Ok(0);
        };
        let file = match version.version {
            HERD_FORMAT_VERSION => match bincode::deserialize::<HerdFile>(&bytes) {
                Ok(file) => file,
                Err(_) => return Ok(0),
            },
            // Before the horse: the same records with no gear on them.
            1 => match bincode::deserialize::<HerdFileV1>(&bytes) {
                Ok(old) => HerdFile {
                    version: HERD_FORMAT_VERSION,
                    day: old.day,
                    animals: old
                        .animals
                        .into_iter()
                        .map(|r| KeptRecord {
                            species: r.species,
                            position: r.position,
                            yaw: r.yaw,
                            health: r.health,
                            growth: r.growth,
                            birth_rest: r.birth_rest,
                            mother: r.mother,
                            keep: r.keep,
                            parked_on: r.parked_on,
                            gear: None,
                        })
                        .collect(),
                },
                Err(_) => return Ok(0),
            },
            _ => return Ok(0),
        };
        // Index in the file to the animal made for it, so the family can be
        // tied back up below. A record with a bad position is skipped, and
        // its lamb is simply motherless.
        let mut made: Vec<Option<EntityId>> = Vec::with_capacity(file.animals.len());
        for record in &file.animals {
            let (x, y, z) = record.position;
            if !x.is_finite() || !y.is_finite() || !z.is_finite() {
                made.push(None);
                continue;
            }
            let mut animal = self.make(record.species, primitive_shared::geometry::narrow(record.position));
            animal.position = record.position;
            animal.yaw = record.yaw;
            animal.wants_yaw = record.yaw;
            animal.health = record.health.clamp(0.1, record.species.health());
            animal.growth = if record.growth.is_finite() { record.growth.clamp(0.0, youth::GROWN) } else { youth::GROWN };
            animal.birth_rest = record.birth_rest.max(0.0);
            animal.keep = Some(record.keep);
            animal.gear = record.gear.clone().map(Box::new);
            made.push(Some(animal.id));
            self.parked.push((animal, record.parked_on.unwrap_or(file.day)));
        }
        for (n, record) in file.animals.iter().enumerate() {
            let (Some(young), Some(Some(mother))) =
                (made[n], record.mother.and_then(|m| made.get(m as usize).copied()))
            else {
                continue;
            };
            for (animal, _) in &mut self.parked {
                if animal.id == young {
                    animal.mother = Some(mother);
                } else if animal.id == mother {
                    animal.young = Some(young);
                }
            }
        }
        Ok(made.iter().flatten().count())
    }

    /// Kept animals out of the world just now.
    pub fn parked_count(&self) -> usize {
        self.parked.len()
    }

    /// What people have made of one animal. Tests and mods.
    pub fn keeping(&self, id: EntityId) -> Option<husbandry::Keeping> {
        self.find(id).and_then(|a| a.keep)
    }

    /// Every kept animal in the world, and what it is. Tests.
    #[cfg(test)]
    pub fn kept(&self) -> Vec<(EntityId, Species, husbandry::Keeping)> {
        self.animals.iter().filter_map(|a| a.keep.map(|k| (a.id, a.species, k))).collect()
    }

    /// Puts a keeping on an animal outright. Tests only: a test about a
    /// pen should not have to spend a day and a half taming its flock.
    #[cfg(test)]
    pub fn keep_for_test(&mut self, id: EntityId, keep: husbandry::Keeping) {
        if let Some(animal) = self.animals.iter_mut().find(|a| a.id == id) {
            animal.keep = Some(keep);
        }
    }
}

/// Decides what an animal is doing, at most once a second.
///
/// Split in two because the two halves have nothing in common beyond the
/// word "animal": everything that runs answers one question (is anybody
/// near?), and the boar answers four.
#[allow(clippy::too_many_arguments)]
fn think(
    animal: &mut Animal,
    players: &[(PlayerId, (f32, f32, f32))],
    senses: &mut Senses<'_>,
    fire_bearers: &[PlayerId],
    seen: &Neighbours,
    world: &dyn BlockWorld,
    rng: &mut Rng,
    dt: f32,
    time_of_day: f32,
) {
    animal.angry_for = (animal.angry_for - dt).max(0.0);
    animal.next_thought -= dt;

    // **What the fly agaric is still doing.** A poisoned point does
    // its damage twice: the thrust, and then a few seconds of this.
    // The rate is small on purpose (see `POISON_PER_SECOND`) -- the
    // paste is one thrust long, and a poison that killed on its own
    // would make the spear behind it beside the point.
    if animal.poison_for > 0.0 {
        let spent = dt.min(animal.poison_for);
        animal.poison_for -= spent;
        animal.health -= POISON_PER_SECOND * spent;
    }

    // ---- the meters a body keeps, whether or not it thinks this tick ----
    //
    // Wind and thirst, in one place because one fact decides both: is it
    // running? Splitting them -- thirst here and stamina down in `walk`
    // -- was the first shape, and it meant two copies of that question
    // which could disagree the day a state was added to one list and not
    // the other. `walk` reads `stamina` and `graze` reads `thirst`;
    // neither writes them.
    let running = matches!(animal.mind, Mind::Flee | Mind::Charge | Mind::Chase);
    if running {
        animal.stamina = (animal.stamina - dt).max(0.0);
    } else {
        animal.stamina = (animal.stamina + dt * STAMINA_RECOVERS).min(animal.species.stamina_seconds());
    }
    animal.thirst = if animal.mind == Mind::Drink {
        // Counted down rather than cleared, so an interrupted drink is a
        // partial one -- see `SLAKED_PER_SECOND`.
        (animal.thirst - SLAKED_PER_SECOND * dt).max(0.0)
    } else {
        let rate = if running { RUNNING_THIRST } else { 1.0 };
        (animal.thirst + dt * rate).min(THIRST_CAP)
    };
    if animal.thirst <= 0.0 {
        // Slaked: whatever water it had in mind is nothing to it now, and
        // the next thirst looks for water from wherever it has got to.
        //
        // **Empty, not merely under `THIRSTY_AT`.** It used to be the
        // threshold, which was the same line as "start being thirsty" -- so
        // once the mouthfuls got short (see `DRINK_SECONDS`) an animal drank
        // down to exactly the threshold, forgot where the water was, stood
        // there getting thirsty again at a second a second, and walked back
        // to the same bank it was standing on. A drink is finished, not
        // abandoned as soon as it stops being urgent.
        animal.water = None;
    }
    animal.next_water_scan = (animal.next_water_scan - dt).max(0.0);
    // The bird's counterpart, counted here beside the water one so the
    // two rationed searches in this file are visibly the same mechanism.
    // It is only ever *spent* by a bird with no nest -- see `homing`.
    animal.next_home_scan = (animal.next_home_scan - dt).max(0.0);
    // A gull's dive runs on the tick, not the thought: a plunge that waited a
    // second to end would be a gull sitting on the sea.
    animal.dive_for = (animal.dive_for - dt).max(0.0);
    animal.homing_for = if animal.mind == Mind::Homing { animal.homing_for + dt } else { 0.0 };
    // Wariness wears off, and with it the memory of which way the danger was.
    if animal.wary_for > 0.0 {
        animal.wary_for = (animal.wary_for - dt).max(0.0);
        if animal.wary_for == 0.0 && animal.mind != Mind::Flee {
            animal.threat_at = None;
        }
    }

    // Memory fades. Guarded on `is_empty` so the animal that has never
    // been hurt -- nearly all of them -- pays one branch here and not a
    // retain over nothing.
    if !animal.dangers.is_empty() {
        animal.dangers.retain_mut(|d| {
            d.left -= dt;
            d.left > 0.0
        });
    }

    // **Is it still being watched?** Only while fleeing a *person*, and
    // only on one tick in `LOOK_EVERY`: a line of sight is a couple of
    // dozen block lookups, and a herd that cast one every tick apiece
    // would spend more of the frame on rays than on running. The time
    // credited is the whole interval, so `hidden_for` counts real
    // seconds however often the ray is actually cast. See `sees` and
    // the thought below, which is where `hidden_for` is read.
    //
    // **Still being noticed, not only still in sight.** It used to ask for a
    // ray and nothing else, so a player sprinting after a deer through a
    // wood lost it at the first trunk: the deer stopped, three seconds
    // later, with a person crashing through the undergrowth four blocks
    // behind it. Now it is the same `perceive` that started the run -- heard
    // is as good as seen -- with the ray only paid for when the ears have
    // not already answered.
    if animal.mind == Mind::Flee && !animal.fights() {
        if let Some(target) = animal.target {
            animal.looks = animal.looks.wrapping_add(1);
            if animal.looks.is_multiple_of(LOOK_EVERY) {
                if let Some(figure) = senses.figures.iter().find(|f| f.who == target) {
                    let acuity = seabird_reach(animal, 1.0, world, time_of_day);
                    match perceive(world, animal, figure, senses.wind, acuity, senses.rays) {
                        Perception::Sure | Perception::Maybe => {
                            animal.hidden_for = 0.0;
                            animal.threat_at = Some((figure.at.0, figure.at.2));
                        }
                        Perception::Nothing => animal.hidden_for += dt * LOOK_EVERY as f32,
                        // Look again next tick rather than wait a whole
                        // interval on a question the budget would not ask.
                        Perception::Deferred => animal.looks = animal.looks.wrapping_sub(1),
                    }
                }
            }
        }
    }

    // **Panic does not wait its turn.** Everything else in this
    // function happens once a second, which is right for deciding
    // whether to walk somewhere and wrong for noticing that the animal
    // beside you has just left: a deer that took up to three seconds to
    // look up is a deer standing alone in a field its herd has already
    // cleared out of.
    //
    // Only for the things that run, and only from a standing start --
    // an animal already fleeing has somewhere to be, and a boar does
    // not take its lead from other boars.
    if !animal.fights() && animal.mind != Mind::Flee {
        if let Some(heading) = seen.alarm {
            bolt(animal, world, heading, seen);
            return;
        }
    }

    // **A sound does not wait its turn either.** An animal standing over
    // grass thinks every four to seven seconds (`FEEDING_SECONDS`), and the
    // senses used to be asked only then -- so a person could sprint thirty
    // blocks up to a grazing deer between two of its thoughts. Twice a
    // second, between thoughts, it listens and sniffs: touch, hearing and
    // scent only, with no ray (the eyes wait for the thought), and anything
    // at all brings the next thought forward to the next tick -- the next,
    // not this one, because the neighbour scan a thought reads is only paid
    // for on the tick `survey` sees one is due. Not while it is running,
    // charging or chasing, which re-think often anyway, and not while it is
    // already watching something, which is the look this would cut short.
    // ...not for a tame animal, which has nothing to decide about a person:
    // woken early by every passer-by, its grazing would be re-rolled ten
    // times a second and it would stand twitching in the pen.
    if animal.next_thought > dt
        && !matches!(animal.mind, Mind::Flee | Mind::Charge | Mind::Chase | Mind::Watch)
        && animal.angry_for <= 0.0
        && !animal.keep.is_some_and(|k| k.tame)
    {
        animal.looks = animal.looks.wrapping_add(1);
        if animal.looks.is_multiple_of(LOOK_EVERY) {
            let acuity = seabird_reach(animal, 1.0, world, time_of_day);
            let mut no_rays = 0;
            let mut ears = Senses { figures: senses.figures, wind: senses.wind, rays: &mut no_rays };
            if matches!(notice(world, animal, &mut ears, acuity), Some((_, _, _, Perception::Sure | Perception::Maybe))) {
                animal.next_thought = 0.0;
            }
        }
    }

    if animal.next_thought > 0.0 {
        return;
    }
    animal.next_thought = THINK_INTERVAL;
    // **Re-rolled here, not fixed for life.** `Animal::drift`'s own doc
    // comment has always said "re-rolled at every thought" and the code
    // never did it -- `drift` was set once, from the id, at `spawn` and
    // never touched again. A bend that never changes is not an animal
    // picking its way, it is an animal walking a slow circle, and it is
    // the *same* circle every time this one wanders: whichever way its
    // id happened to hash is the way it always curves. That is most of
    // what "the whole herd turns together" turned out to be -- not one
    // shared random source, but several animals each individually stuck
    // with a single, permanent bend, so a herd that spawned facing
    // roughly the same way kept curving the same way for as long as it
    // lived. Drawing a fresh value here, once a second, gives each
    // animal its own changing phase instead of a fixed one. See
    // `two_sheep_side_by_side_do_not_walk_the_same_path`.
    animal.drift = rng.range(-0.25, 0.25);

    animal.fed_for = (animal.fed_for - dt).max(0.0);

    // **In the water: get out of it.**
    //
    // Before every other decision, because an animal that is swimming is
    // not doing anything else -- and because the decisions below are all
    // about ground it is not standing on. What was here before was
    // nothing at all: `footing` stops an animal *walking* into a lake
    // (and that rule stays, it is what keeps animals from floating out
    // of reach), but an animal that was pushed in, spawned in, or chased
    // in had no idea it was in water. It wandered on whatever heading it
    // last picked, at `WADE_SPEED`, and buoyancy held it at the surface
    // -- so what a player saw was an animal milling about in a pond
    // until it happened to drift against the bank.
    //
    // The heading only. A fleeing or chasing animal keeps its state, so
    // the alarm still spreads from a deer that is in the river and a
    // startled bird still climbs; what changes is that it makes for dry
    // land while it does it. The shore it picks is the *nearest*, not
    // the one furthest from whatever frightened it -- a deer that swam
    // the long way across a lake to keep a player at its back would
    // drown a good deal more often than it would escape, and the water
    // is not where that decision belongs.
    //
    // A charge is exempt, exactly as it is exempt from the rule about
    // not walking into water in the first place: it has committed, and
    // what happens to it is the player's doing.
    // The *feet*, not the body. `in_liquid` -- the middle of the animal,
    // which is what decides whether it is wading -- goes false the
    // moment buoyancy lifts a floating body's back above the surface,
    // and it does that every few ticks: a bobbing animal would have
    // looked for the shore on half its thoughts and wandered off on the
    // other half. Feet in water is the question being asked anyway, and
    // it is one lookup.
    // A gull is exempt: it sits on the water it comes down on, and gets off
    // it the way it gets off the sand -- by going up (`seabird`). Making for
    // the bank on foot is a grouse's answer.
    if animal.mind != Mind::Charge && !animal.species.soars() && enters_liquid(world, animal.position) {
        if let Some(heading) = shore_heading(world, animal) {
            animal.wants_yaw = heading;
            if !matches!(animal.mind, Mind::Flee | Mind::Chase) {
                animal.mind = Mind::Wander;
            }
            // Sooner than a full thought: a wading animal covers a third
            // of a block a second, and a heading held for a whole second
            // while the bank slides past is how it misses the bank.
            animal.next_thought = 0.5;
            return;
        }
    }

    // **Food held out, and a home to go back to**, before any question of
    // who to run from or charge: see `tend_thought`.
    if tend_thought(animal, senses.figures, seen, world, rng, time_of_day) {
        return;
    }

    // How keen its senses are just now: everything's own, except a gull's,
    // which is nothing in the air and a half on a dark beach.
    let acuity = seabird_reach(animal, 1.0, world, time_of_day);

    if animal.fights() {
        // **An angry animal does not have to notice anybody**: it keeps its
        // quarry well past the distance it would have noticed them at. That
        // is what a grudge *is*, and without it backing off four metres
        // cancels a fight you started.
        let nearest = if animal.angry_for > 0.0 {
            players
                .iter()
                .map(|&(id, at)| (id, at, (at.0 - animal.at().0).hypot(at.2 - animal.at().2)))
                .filter(|&(_, _, distance)| distance <= animal.species.awareness() * 2.0)
                .min_by(|a, b| a.2.total_cmp(&b.2))
        } else {
            // A hunter does not lift its head at a maybe the way prey does:
            // a sound it is not sure of is a sound it goes to look at, which
            // is what `think_hunter`'s watching already is.
            match notice(world, animal, senses, acuity) {
                Some((id, at, distance, Perception::Sure | Perception::Maybe)) => Some((id, at, distance)),
                Some((_, _, _, Perception::Deferred)) => {
                    animal.next_thought = 0.05;
                    return;
                }
                _ => None,
            }
        };
        let figure = nearest.and_then(|(id, _, _)| senses.figures.iter().find(|f| f.who == id).copied());
        think_hunter(animal, nearest, figure, fire_bearers, seen, world, rng, time_of_day);
        return;
    }

    // **A person it cannot see is a person it does not know about.**
    //
    // Awareness used to be a radius and nothing else, so a deer twelve
    // blocks away behind a hill knew exactly where you were -- which is
    // why running into the trees never worked: the trees hid you from
    // the deer and not the deer from a radius. Now a grazing animal has
    // to *see* you to start, and a fleeing one keeps running only until
    // it has been out of your sight for `HIDDEN_SECONDS` (kept up by the
    // per-tick check above). A deer that is fleeing and can still see
    // you does not pay a second ray here: `hidden_for` is that answer.
    //
    // The ray is cast once a second per prey animal with a player in
    // range, which is the cheapest place it could be -- see `sees`.
    //
    // ...and now: a person it has not *noticed* is a person it does not know
    // about. Seen, heard or smelled -- see `perceive` -- and a sound or a
    // scent from the far edge of its reach is a maybe, which lifts its head
    // rather than sending it off (`ALERT_BAND`).
    let running_from = match animal.target {
        Some(target) if animal.mind == Mind::Flee && animal.hidden_for < HIDDEN_SECONDS => senses
            .figures
            .iter()
            .find(|f| f.who == target)
            .map(|f| (Some(target), f.at, (f.at.0 - animal.at().0).hypot(f.at.2 - animal.at().2)))
            // Not past anything any of its senses could reach, however
            // recently it was running: a person who has been carried off
            // (a bed, a death) is not a person it is still running from.
            .filter(|&(_, _, distance)| distance <= furthest_sense(animal.species, acuity)),
        _ => None,
    };
    let mut deferred = false;
    let mut maybe = None;
    let from_player = running_from.or_else(|| match notice(world, animal, senses, acuity) {
        Some((id, at, distance, Perception::Sure)) => Some((Some(id), at, distance)),
        // **A second maybe in a row is enough.** It looked up at the last one
        // and the thing is still there: it goes. A player who froze when the
        // head came up is a player the sound went with.
        Some((id, at, distance, Perception::Maybe)) if animal.mind == Mind::Watch => Some((Some(id), at, distance)),
        Some((id, at, _, Perception::Maybe)) => {
            maybe = Some((id, at));
            None
        }
        Some((_, _, _, Perception::Deferred)) => {
            deferred = true;
            None
        }
        _ => None,
    });
    match from_player {
        // It has lost you, or been lost. Either way the run is over and
        // the next one starts from scratch.
        None if animal.mind == Mind::Flee && animal.target.is_some() => {
            animal.hidden_for = 0.0;
            animal.cover = None;
            // **And it stays wary.** See `WARY_SECONDS`: the run is over, the
            // fright is not.
            animal.wary_for = WARY_SECONDS;
        }
        // Somebody new, or somebody seen afresh: the clock on being
        // hidden from them starts now, not where the last chase left it.
        Some((who, _, _)) if who != animal.target => animal.hidden_for = 0.0,
        _ => {}
    }

    // **A wolf is as frightening as a person, and usually nearer.**
    //
    // Whichever danger is closer wins. Before this the only thing prey
    // knew about was the player: a deer grazed beside a hunting wolf
    // until it was bitten, which is the single most stupid-looking thing
    // an animal in this world could do.
    let from_beast = seen.threat.map(|at| {
        let (dx, dz) = (at.0 - animal.at().0, at.2 - animal.at().2);
        (None, at, (dx * dx + dz * dz).sqrt())
    });
    let danger = match (from_player, from_beast) {
        (Some(a), Some(b)) if b.2 < a.2 => Some(b),
        (Some(a), _) => Some(a),
        (None, b) => b,
    };
    // ...and a gull in the air minds a wolf on the dunes no more than it
    // minds a person there: see `seabird_reach`.
    let danger = if seabird_reach(animal, 1.0, world, time_of_day) < 0.0 {
        None
    } else {
        danger
    };

    // **A maybe, and nothing worse: head up.** It stops, faces the sound or
    // the scent, and is keen for a while -- which is all a stalker gets by
    // way of warning, and the moment to stop moving.
    if danger.is_none() {
        if let Some((_, at)) = maybe {
            animal.mind = Mind::Watch;
            animal.target = None;
            animal.wants_yaw = (at.2 - animal.at().2).atan2(at.0 - animal.at().0);
            // Head up, and everybody watching can see that it is: this is the
            // warning a stalker gets, and a warning nothing draws is not one.
            animal.attitude = primitive_shared::protocol::Attitude::Alert;
            animal.wary_for = animal.wary_for.max(WARY_SECONDS * 0.25);
            animal.threat_at = Some((at.0, at.2));
            animal.next_thought = rng.range(1.0, 2.0);
            return;
        }
    }

    // **A troop does not run from a person, it comes for what they have.**
    // Before the flight, because for a monkey the sight of somebody is the
    // *opportunity* rather than the fright -- and after `maybe`, because a
    // half-noticed figure is not something to steal from. A monkey that has
    // been hit goes back to running like everything else: see `raid`, which
    // reads the grudge.
    if animal.species.pilfers() {
        if let Some((Some(who), at, distance)) = danger {
            let held = senses.figures.iter().find(|f| f.who == who).and_then(|f| f.held);
            if raid(animal, Mark { who, at, distance, held }, seen, world, rng) {
                return;
            }
        }
    }

    match danger {
        Some((who, at, _)) => {
            // A `target` only exists for people. What it is *for* is the
            // grudge and the alarm, and neither has any meaning aimed at
            // an animal -- but the alarm does have to spread, so a deer
            // running from a wolf must still count as running from
            // something it can see. See `Neighbours::alarm`, which reads
            // `target`, and note that a beast-driven bolt sets it to
            // `None`: the deer that saw the wolf startles its neighbours
            // through the wolf's own `quarry` chase instead, one ring at
            // a time, because a herd that all bolted from an unseen wolf
            // would be a herd with one mind.
            animal.target = who;
            animal.threat_at = Some((at.0, at.2));
            let (dx, dz) = (at.0 - animal.at().0, at.2 - animal.at().2);
            bolt(animal, world, (-dz).atan2(-dx), seen);
        }
        None => {
            animal.target = None;
            // **A bird has somewhere to be.** Everything else in this
            // world grazes where it happens to be standing, which is
            // what `graze` is; a bird has a nest, and the first thing it
            // does with nothing else going on is decide whether it is at
            // it. `homing` hands back `false` when the answer is "it is
            // home and has nothing to do about it", and then the bird
            // forages, drinks and idles exactly as a deer does -- which
            // is why the flying is a layer over `graze` and not a second
            // copy of it.
            // ...except the gull, which has a shore rather than a nest and
            // does not graze at all: everything it does with nothing in sight
            // is in `seabird`.
            if animal.species.soars() {
                seabird(animal, players, seen, world, rng, time_of_day);
                return;
            }
            if animal.species.flies() && homing(animal, world, rng, time_of_day) {
                return;
            }
            graze(animal, seen, world, rng, time_of_day);
            if deferred {
                // It wanted a look the tick had no ray for: look next tick.
                animal.next_thought = animal.next_thought.min(0.05);
            }
        }
    }
}

/// What a bird does about its nest, and `false` when the answer is
/// "nothing" -- at which point the caller lets it graze like anything
/// else.
///
/// The state machine the module comment tabulates, in the order the
/// decisions are actually made. Reached only from `think`'s "nothing in
/// sight" branch, which is what makes every rule below outranked by a
/// person, a wolf and a fright without a line of code saying so.
/// How far off feed held out is noticed by an animal nobody has tamed, in
/// blocks -- and only from somebody creeping or standing (`LURE_LOUDNESS`).
///
/// **Inside the distance it would bolt at**, so the lure is a thing done
/// slowly: a player who walks up to a flock with grain in hand scatters it
/// as anybody would, and one who creeps the last ten blocks and stands has
/// them come to the hand.
const LURE_RANGE: f32 = 10.0;

/// ...and by a tame one, from anybody walking or running: a flock follows
/// the sack. **This is how animals are moved**, into a pen or across a
/// valley, and there is no lead rope because the grain is the rope.
///
/// Rejected: following only the person who tamed it. A flock is then
/// unstealable, and a server where a neighbour's sheep can be walked off
/// with a handful of grain is a server with a reason to build a gate.
const FOLLOW_RANGE: f32 = 16.0;

/// Loudest a person may be, against a walk, and still lure a wild animal:
/// creeping (0.3) or standing (0). See `Gait::loudness`.
const LURE_LOUDNESS: f32 = 0.5;

/// Near enough to the feed: it stops and waits at the hand rather than
/// walking into the player.
const LURE_CLOSE: f32 = 1.6;

/// **How far a tame animal lets itself graze from home** before it turns
/// back, in blocks. A pen is a few blocks across, so inside it this never
/// bites and the walls do the holding; outside one, it is what keeps a
/// flock from drifting off across the map while nobody watches it.
const KEPT_LEASH: f32 = 5.0;

/// The part of a thought that is about people as keepers rather than as
/// danger. `true` if it decided what the animal does.
///
/// Three things, in order. **A predator beats everything**: a sheep with a
/// wolf in sight runs whatever is held out to it -- and a boar, which does
/// not run from wolves, is let through to its own rules. **Feed beats
/// home**: a tame animal follows food from `FOLLOW_RANGE`, a wild one comes
/// to a still hand from `LURE_RANGE`. And **a tame animal with nothing to
/// follow grazes near home**, and never flees or charges a person at all --
/// the rest of `think` is not asked, so a kept boar is not a boar that
/// gores whoever opens the pen.
fn tend_thought(
    animal: &mut Animal,
    figures: &[Figure],
    seen: &Neighbours,
    world: &dyn BlockWorld,
    rng: &mut Rng,
    time_of_day: f32,
) -> bool {
    if !husbandry::tameable(animal.species) || animal.angry_for > 0.0 || animal.mind == Mind::Flee {
        return false;
    }
    if seen.threat.is_some() && !animal.fights() {
        return false;
    }
    let tame = animal.keep.is_some_and(|k| k.tame);
    let reach = if tame { FOLLOW_RANGE } else { LURE_RANGE };
    let at = animal.at();
    let lure = figures
        .iter()
        .filter(|f| f.held.is_some_and(|held| husbandry::ration(animal.species, held).is_some()))
        .filter(|f| tame || f.loudness <= LURE_LOUDNESS)
        .map(|f| (f.at, (f.at.0 - at.0).hypot(f.at.2 - at.2)))
        .filter(|&(_, distance)| distance <= reach)
        .min_by(|a, b| a.1.total_cmp(&b.1));
    if let Some((to, distance)) = lure {
        animal.target = None;
        animal.charge_at = None;
        animal.attitude = primitive_shared::protocol::Attitude::Easy;
        animal.wants_yaw = (to.2 - at.2).atan2(to.0 - at.0);
        animal.mind = if distance > LURE_CLOSE { Mind::Wander } else { Mind::Idle };
        animal.next_thought = 0.5;
        return true;
    }
    if !tame {
        return false;
    }
    animal.target = None;
    animal.charge_at = None;
    if let Some(home) = animal.keep.and_then(|k| k.home) {
        let (dx, dz) = (home.0 - at.0, home.2 - at.2);
        if dx.hypot(dz) > KEPT_LEASH {
            animal.mind = Mind::Wander;
            animal.attitude = primitive_shared::protocol::Attitude::Easy;
            animal.wants_yaw = open_heading(world, animal, dz.atan2(dx));
            animal.next_thought = rng.range(1.0, 2.0);
            return true;
        }
    }
    graze(animal, seen, world, rng, time_of_day);
    // **Asked again soon**, whatever `graze` chose: a wander held for five
    // seconds carried a tame sheep that far past its leash before the leash
    // was next looked at, and a flock left in the open drifted off by
    // whole wanders at a time.
    animal.next_thought = animal.next_thought.min(1.5);
    true
}

fn homing(animal: &mut Animal, world: &dyn BlockWorld, rng: &mut Rng, time_of_day: f32) -> bool {
    // **Thirst first, and it belongs to `graze`.** A bird that flew home
    // rather than drinking would be a bird that never drinks, because
    // being home is the state it spends its life in -- and the water is a
    // *place*, which is the one thing the nest and the river have in
    // common and the reason neither may swallow the other. See
    // `THIRSTY_AT`.
    //
    // **Only when there is water to go to**, and that clause is the
    // whole of the rule. Thirst never falls on its own -- only drinking
    // takes it down -- so a bird in a wood with no pond reached
    // `THIRSTY_AT` about two minutes into its life, handed every thought
    // after that to `graze`, and never went near its nest again: a
    // mechanic that switched itself off permanently the first time it
    // could not be satisfied. Now a bird that has looked and found
    // nothing gets on with living where it lives, and `graze` -- which
    // still runs on every thought this function declines -- goes on
    // paying for the water search on its own timer (`water_near`,
    // `WATER_SCAN_INTERVAL`) until there is somewhere to go.
    if animal.thirst >= THIRSTY_AT && animal.water.is_some() {
        animal.bound_for = None;
        return false;
    }

    /// Sets a bird flying at a point, and answers `true` because that is
    /// a decision.
    ///
    /// The heading is deliberately *not* passed through `open_heading`:
    /// that probe asks whether the animal could walk the next three
    /// blocks, and a bird three metres up does not care what is on the
    /// ground under it. Running the flight through the walker's opinion
    /// of the terrain was the first shape, and it turned a bird crossing
    /// a pond into a bird circling one.
    ///
    /// Re-aimed often -- sooner than a full thought -- for the reason the
    /// approach to water is: a cruise covers four blocks a second and
    /// `HOME_REACH` is a block and a half, so a heading held for a whole
    /// second flies past the tree it was aimed at and has to come back.
    fn fly_to(
        animal: &mut Animal,
        at: (f32, f32),
        world: &dyn BlockWorld,
        rng: &mut Rng,
    ) -> bool {
        animal.bound_for = Some(at);
        // How high the thing it is flying to stands. Taken once, here,
        // and held for the flight -- see `Animal::flight_ceiling` for the
        // bird-under-its-own-tree this stops.
        // **`perch_over` and not `surface_under`**: the destination of this
        // flight is very often a nest, and a nest is the one thing in the
        // world that hides the canopy it sits on from a scan that wants air
        // over footing. See `perch_over` for the bird that spent its life
        // standing under its own tree.
        animal.flight_ceiling = perch_over(
            world,
            at.0,
            animal.at().1 + NEST_LIFT as f32,
            at.1,
        )
        .map(|y| y as f32)
        .unwrap_or(f32::NEG_INFINITY);
        animal.mind = Mind::Homing;
        animal.wants_yaw = (at.1 - animal.at().2).atan2(at.0 - animal.at().0);
        animal.next_thought = rng.range(0.4, 0.8);
        true
    }

    /// How far, squared, an animal is from a point on the ground.
    fn short_of(animal: &Animal, at: (f32, f32)) -> f32 {
        let (dx, dz) = (at.0 - animal.at().0, at.1 - animal.at().2);
        dx * dx + dz * dz
    }

    // **In the air on a leg of a journey: hold it.** Before everything
    // else, so a hop is not turned round by the range test below on the
    // very next thought -- which is exactly what happened while `home`
    // and the destination were one field.
    if let Some(at) = animal.bound_for {
        // Near is not arrived while it is still well up over the spot: see
        // `still_up`. Asked second, so the column is only read once near.
        if animal.mind == Mind::Homing
            && (short_of(animal, at) > HOME_REACH * HOME_REACH || still_up(animal, world))
        {
            return fly_to(animal, at, world, rng);
        }
        // Arrived, or knocked out of the flight by something that
        // outranks it. Either way this leg is over.
        //
        // **Landing is not a state.** Dropping out of `Homing` is the
        // whole of it: `walk`'s altitude spring aims a bird with no
        // reason to be up at whatever is under it and lets it down the
        // last block at `LANDING_SINK`, having already glided most of the
        // way on `GLIDE_SLOPE`. What is under it may be the turf, the canopy
        // it nests in, or a roof somebody built -- `surface_under` does
        // not care which, which is why "it perches on things" needed no
        // code of its own.
        animal.bound_for = None;
    }

    let Some(home) = animal.home else {
        // **No nest: look for one, rarely.** The cost of this line is
        // argued at `NEST_SCAN_INTERVAL`; what matters here is that the
        // timer is charged whether or not anything is found, so a bird
        // over open steppe -- where there is no canopy for miles -- pays
        // once every eight seconds and not once a second.
        if animal.next_home_scan <= 0.0 {
            animal.next_home_scan = NEST_SCAN_INTERVAL;
            animal.home = nest_near(world, animal);
            if let Some(found) = animal.home {
                // It goes there now. A bird that noted a tree and then
                // wandered off would have found a nest it never used.
                return fly_to(animal, found, world, rng);
            }
        }
        // Still nothing. A bird with nowhere to go forages where it is,
        // which is also how it gets somewhere new to look from: the
        // search is re-run from wherever the wander left it.
        return false;
    };

    let out = short_of(animal, home);

    // **After dark a bird goes to roost, and then it stays there.**
    //
    // The player's "бездельничают" was two complaints in one sentence, and
    // this is the half about the night: a bird kept the same hours at
    // midnight as at noon -- hop, forage, hop -- so there was no hour at
    // which watching a tree told you anything. A roost is the cheapest
    // possible mechanic for it (one clock test, no state) and it is what
    // makes the dusk flight home worth seeing: the wood fills up at sunset
    // and empties at dawn, and a player who wants a bird after dark knows
    // exactly where every bird is.
    //
    // The leash is dropped for the trip home -- a bird caught out at dusk
    // flies back from wherever it is, not only from inside `HOME_RANGE` --
    // and `graze` is never reached at all while it is roosting, which is
    // what stops a roosting bird from wandering off its own branch.
    // Everything that outranks the nest still outranks the roost: this
    // function is only reached with nothing in sight (see `think`), so a
    // fright takes a bird off its perch at any hour.
    if is_night(time_of_day) {
        if out > HOME_REACH * HOME_REACH {
            return fly_to(animal, home, world, rng);
        }
        animal.mind = Mind::Idle;
        // Half-asleep: a look round now and then and nothing else. The
        // range is `graze`'s night pause, so a roosting bird and a settled
        // herd keep the same hours.
        if rng.chance(0.25) {
            animal.wants_yaw += rng.range(-0.8, 0.8);
        }
        animal.next_thought = rng.range(6.0, 12.0);
        return true;
    }

    if out > HOME_RANGE * HOME_RANGE {
        // **Going home.** The state a fright ends in: a bird flushed
        // across a meadow finds itself somewhere it did not choose, and
        // the first thing it does once nothing is in sight is fly back.
        return fly_to(animal, home, world, rng);
    }

    if out > HOME_REACH * HOME_REACH {
        // Near the nest and not on it: foraging. Sometimes it goes and
        // sits on it instead, and `SETTLE_CHANCE` is where that number
        // is argued -- without it the leash above is the only rule and a
        // bird orbits its own tree without ever landing in it.
        if rng.chance(SETTLE_CHANCE) {
            return fly_to(animal, home, world, rng);
        }
        return false;
    }

    if rng.chance(HOP_CHANCE) {
        // **A hop.** The bird picks another perch within sight of the
        // nest and flies to it, so what a player watching a tree sees is
        // a bird moving between points rather than a bird glued to one.
        // The target is a point, not a search: a hop that had to *find*
        // somewhere better would be the per-tick world scan this whole
        // design exists to avoid, and a hop that lands somewhere
        // uninteresting is still a hop.
        //
        // **Toward something to eat when there is any in sight**, which is
        // the other half of "бездельничают": a hop aimed at nothing is a
        // bird shuffling round its tree, and the same hop aimed at the
        // tussocks across the clearing is a bird going to feed -- and it
        // lands standing in food, where `graze` already keeps it with its
        // head down for `FEEDING_SECONDS`. It costs one `food_heading` on
        // the quarter of thoughts that hop, and the answer is only a
        // bearing: where it comes down is still a point and not a search.
        let bearing = food_heading(world, animal).unwrap_or_else(|| rng.range(0.0, std::f32::consts::TAU));
        let reach = rng.range(HOME_REACH * 2.0, HOP_RANGE);
        let (sin, cos) = bearing.sin_cos();
        let to = (home.0 + cos * reach, home.1 + sin * reach);
        return fly_to(animal, to, world, rng);
    }

    // Standing at the nest. Not `true`: a bird that is home with nothing
    // to do is an animal with nothing to do, and what an animal with
    // nothing to do does is forage, look about and stand -- which is
    // `graze`, written once, for everything that lives here.
    false
}

/// What a gull does with nothing in sight: the shore's `homing` and `graze`
/// in one. See the table at `SOAR_HEIGHT`.
///
/// Reached only from `think`'s "nothing in sight" branch, so a person on the
/// sand and a wolf on the dunes outrank every line of it, as they outrank the
/// nest -- and from nowhere else, so no other animal pays for any of it.
fn seabird(
    animal: &mut Animal,
    players: &[(PlayerId, (f32, f32, f32))],
    seen: &Neighbours,
    world: &dyn BlockWorld,
    rng: &mut Rng,
    time_of_day: f32,
) {
    let night = is_night(time_of_day);
    // Its stretch of shore: where the flock was put, kept for life for the
    // reason a nest is (`Animal::home`). A gull a mod put down takes wherever
    // it first thinks as its shore.
    let home = *animal.home.get_or_insert((animal.at().0, animal.at().2));
    // **Where the rest of them are, when any of them are in sight.** A gull
    // comes down where the other gulls are, which is the whole of what makes
    // a roost on the sand a *flock* rather than a dozen birds who happen to
    // have been spawned together and have drifted apart ever since. Free:
    // `survey` has already worked the centre out for the herd rules, and a
    // landing is still `LANDING_LOOKS` columns round it rather than a point
    // every bird aims at -- a flock that converged on one cell would be a
    // heap.
    let about = seen.centre.unwrap_or(home);
    match animal.mind {
        Mind::Homing => {
            // A landing that will not happen is given up for the sky. See
            // `LANDING_GIVE_UP_SECONDS`.
            if animal.homing_for > LANDING_GIVE_UP_SECONDS {
                animal.mind = Mind::Soar;
                animal.bound_for = Some(home);
                animal.next_thought = rng.range(1.0, 2.5);
                return;
            }
            if let Some((tx, tz)) = animal.bound_for {
                let (dx, dz) = (tx - animal.at().0, tz - animal.at().2);
                // Over the spot and still high is a pass, not a landing: it
                // comes round and in again, lower. See `still_up`.
                if dx * dx + dz * dz > HOME_REACH * HOME_REACH || still_up(animal, world) {
                    // Re-aimed often, for `homing`'s reason: a cruise
                    // overshoots a block and a half in well under a second.
                    animal.wants_yaw = dz.atan2(dx);
                    animal.next_thought = rng.range(0.3, 0.6);
                    return;
                }
            }
            // Over the spot. **Landing is not a state** here either: out of
            // `Homing`, and `walk`'s spring lets it down onto whatever is
            // under it.
            animal.bound_for = None;
            animal.mind = Mind::Idle;
            animal.next_thought = rng.range(2.0, 5.0);
        }
        Mind::Soar | Mind::Flee => {
            // A fright that has passed, or a circle that has wandered off its
            // shore: back over home, and circling.
            let centre = animal.bound_for.unwrap_or(home);
            if animal.mind == Mind::Flee || (centre.0 - home.0).hypot(centre.1 - home.1) > GULL_RANGE {
                animal.mind = Mind::Soar;
                animal.bound_for = Some(home);
            }
            animal.next_thought = rng.range(1.0, 2.5);
            // **Fishing**: over the sea, by day, now and then. The draw first,
            // so the column is only read on a thought that would dive.
            if !night && animal.dive_for <= 0.0 && rng.chance(DIVE_CHANCE) && over_water(world, animal) {
                animal.dive_for = DIVE_SECONDS;
                return;
            }
            // **Down**: the dark brings a flock in, and by day it comes down
            // now and then -- where there is somewhere dry and nobody near.
            if night || rng.chance(LAND_CHANCE) {
                if let Some((x, y, z)) = landing_spot(world, animal, about, players, rng) {
                    animal.mind = Mind::Homing;
                    animal.bound_for = Some((x, z));
                    // Above what it lands on for the whole approach, for the
                    // reason `Animal::flight_ceiling` gives the grouse.
                    animal.flight_ceiling = y;
                    animal.wants_yaw = (z - animal.at().2).atan2(x - animal.at().0);
                    animal.next_thought = rng.range(0.3, 0.6);
                    return;
                }
            }
            // Otherwise the circle drifts along the shore a little, so a
            // flock works the coast rather than one patch of sky over it.
            //
            // **Drifting toward where the others are, not round the nest.**
            // The centre used to be drawn round `home` for every bird, so
            // six gulls held six independent circles that happened to share
            // a middle -- a flock by arithmetic and not by sight. Half way
            // to where it can see the rest of them (`about`, which `survey`
            // has already worked out and nothing here pays for) and a drift
            // on top puts their circles over one another without putting
            // them on one line: each bird still flies its own radius
            // (`circling`) and its own hand, which is what keeps the
            // formation loose. Held inside `GULL_RANGE` of home, or a flock
            // that kept following its own centre would walk itself off the
            // coast a few blocks a minute.
            if rng.chance(0.2) {
                let centre = (centre.0 * 0.35 + about.0 * 0.65, centre.1 * 0.35 + about.1 * 0.65);
                let bearing = rng.range(0.0, std::f32::consts::TAU);
                let reach = rng.range(0.0, GULL_RANGE * 0.3);
                let to = (centre.0 + bearing.cos() * reach, centre.1 + bearing.sin() * reach);
                let (out_x, out_z) = (to.0 - home.0, to.1 - home.1);
                let out = out_x.hypot(out_z);
                animal.bound_for = Some(if out > GULL_RANGE {
                    (home.0 + out_x / out * GULL_RANGE, home.1 + out_z / out * GULL_RANGE)
                } else {
                    to
                });
            }
        }
        _ => {
            // On the sand, or sitting on the water.
            if !night && rng.chance(TAKE_OFF_CHANCE) {
                animal.mind = Mind::Soar;
                animal.bound_for = Some(home);
                animal.next_thought = rng.range(4.0, 10.0);
                return;
            }
            if !night && animal.on_ground && rng.chance(0.3) {
                // A few steps along the tideline.
                animal.mind = Mind::Wander;
                animal.wants_yaw = open_heading(world, animal, rng.range(0.0, std::f32::consts::TAU));
                animal.next_thought = rng.range(0.8, 2.0);
            } else {
                animal.mind = Mind::Idle;
                if rng.chance(0.5) {
                    animal.wants_yaw += rng.range(-1.2, 1.2);
                }
                animal.next_thought = if night { rng.range(6.0, 12.0) } else { rng.range(1.5, 4.0) };
            }
        }
    }
}

/// How far off a gull notices a person, given where it is and the hour:
/// `reach` unchanged for everything else.
///
/// **Negative -- nobody at all -- while it is in the air**, because up there
/// it is out of everybody's reach and it knows it: a gull that bolted from a
/// person walking the beach nine blocks under it would spend its life fleeing
/// the people it circles over. What moves it in the air is its own thoughts
/// and its flock's alarm. On the sand or the water after dark, a half
/// (`ROOSTING_WARINESS`).
fn seabird_reach(animal: &Animal, reach: f32, world: &dyn BlockWorld, time_of_day: f32) -> f32 {
    if !animal.species.soars() {
        return reach;
    }
    if !animal.on_ground && !in_liquid(world, animal.position, animal.species) {
        return -1.0;
    }
    if is_night(time_of_day) {
        reach * ROOSTING_WARINESS
    } else {
        reach
    }
}

/// Whether a homing bird near its spot is still too high to call itself
/// arrived: more than `ARRIVAL_HEIGHT` over the higher of what is under it
/// and what it is going to land on -- the floor `walk` flies it over.
///
/// **That floor and not the spot's height alone**, and the difference was a
/// gull that never landed again. Measured against the spot, a gull coming in
/// to the sand at the foot of a cliff passed the spot over the cliff top,
/// where it cannot sink lower than the rock, was "too high" on every pass, and
/// circled in `Homing` for the rest of its life without fishing once
/// (`a_flying_gull_never_falls_through_or_tunnels_into_terrain`). Against the
/// floor it has arrived there, and settles on the cliff top -- which is a
/// place a gull stands.
///
/// Never while it is standing on something, which is the one thing that
/// holds a bird up for good; and never for a spot whose height nobody could
/// measure (ground not loaded).
fn still_up(animal: &Animal, world: &dyn BlockWorld) -> bool {
    if animal.on_ground || !animal.flight_ceiling.is_finite() {
        return false;
    }
    let (x, y, z) = animal.at();
    let under = if animal.species.soars() {
        sea_or_ground_under(world, x, y + 2.0, z)
    } else {
        perch_over(world, x, y + 2.0, z)
    };
    let floor = under.map_or(animal.flight_ceiling, |ground| (ground as f32).max(animal.flight_ceiling));
    y - floor > ARRIVAL_HEIGHT
}

/// Somewhere dry on its shore for a gull to come down, if one of
/// `LANDING_LOOKS` columns has it: sand, shingle, rock or turf -- not the
/// sea, not a canopy -- with room to stand, and further from every player
/// than the gull's own awareness. Nearer than that and it lands and goes
/// straight back up, which is a bird bouncing on the beach.
fn landing_spot(
    world: &dyn BlockWorld,
    animal: &Animal,
    home: (f32, f32),
    players: &[(PlayerId, (f32, f32, f32))],
    rng: &mut Rng,
) -> Option<(f32, f32, f32)> {
    let wary = animal.species.awareness();
    for _ in 0..LANDING_LOOKS {
        let bearing = rng.range(0.0, std::f32::consts::TAU);
        let reach = rng.range(2.0, GULL_RANGE);
        let (x, z) = (home.0 + bearing.cos() * reach, home.1 + bearing.sin() * reach);
        let Some(y) = sea_or_ground_under(world, x, animal.at().1 + 4.0, z) else {
            continue;
        };
        let Some(under) = world.block(x.floor() as i32, y - 1, z.floor() as i32) else {
            continue;
        };
        if is_liquid(under) || primitive_shared::types::is_leafy(under) {
            continue;
        }
        if players.iter().any(|&(_, at)| (at.0 - x).hypot(at.2 - z) <= wary) {
            continue;
        }
        if !fits(world, (f64::from(x), f64::from(y as f32), f64::from(z)), animal.frame()) {
            continue;
        }
        return Some((x, y as f32, z));
    }
    None
}

/// The heading that flies a circle of `SOAR_RADIUS` round a point, from
/// wherever the bird is: along the circle, bent in or out by how far off it
/// the bird has drifted.
///
/// Which way round is the bird's own, from its id, so a flock does not all
/// wheel one way like a mobile over a cot.
/// **Each bird on its own circle, not on one ring.** The radius is the
/// bird's own -- `SOAR_RADIUS` give or take a third, from the id -- which is
/// what turns a flock sharing a centre into a *loose* formation: they hold
/// together over the same stretch of shore and no two of them fly the same
/// line, which is the difference between a flock and a fairground ride. It
/// costs one more term out of the same id the hand already comes from.
fn circling(animal: &Animal, centre: (f32, f32)) -> f32 {
    let (dx, dz) = (animal.at().0 - centre.0, animal.at().2 - centre.1);
    let distance = (dx * dx + dz * dz).sqrt();
    let around = dz.atan2(dx);
    let hand = if animal.id.is_multiple_of(2) { 1.0 } else { -1.0 };
    let radius = SOAR_RADIUS * (0.7 + 0.6 * spread(animal.id, 0x27D4_EB2F));
    // Past the circle, turned further in than a tangent; inside it, less.
    let bend = ((distance - radius) / radius).clamp(-0.8, 1.2);
    around + hand * (std::f32::consts::FRAC_PI_2 + bend)
}

/// Is the first thing under a flying bird the sea?
fn over_water(world: &dyn BlockWorld, animal: &Animal) -> bool {
    let (x, z) = (animal.at().0.floor() as i32, animal.at().2.floor() as i32);
    sea_or_ground_under(world, animal.at().0, animal.at().1, animal.at().2)
        .and_then(|y| world.block(x, y - 1, z))
        .is_some_and(is_liquid)
}

/// The cell a body over this column comes to rest in -- on the ground or on
/// the top of the water -- looking down from `from_y`.
///
/// **`surface_under` with the sea counted as a floor**, which is the one
/// thing a gull needs that a grouse does not. `surface_under` asks for air
/// over something standable, so over a lake it looks straight through the
/// water, finds the cell over the bed full of water rather than air, and
/// answers nothing -- and a flier with nothing under it holds whatever height
/// it had, so a bird that flew off a cliff over the sea stayed at the
/// cliff's height for ever. A gull flies at its height *over the sea*, and
/// dives to the top of it.
///
/// Anything with no box -- a tuft, a flower -- is looked through, as air is.
/// Forty-eight cells at most, like `water_surface_under`, and `None` on
/// ground that has not loaded.
fn sea_or_ground_under(world: &dyn BlockWorld, x: f32, from_y: f32, z: f32) -> Option<i32> {
    let (bx, bz) = (x.floor() as i32, z.floor() as i32);
    let start = (from_y.floor() as i32).min(CHUNK_SIZE_Y as i32 - 1);
    for y in ((start - 48).max(1)..=start).rev() {
        let here = world.block(bx, y, bz)?;
        if is_liquid(here) || stand_height(here) > 0.0 {
            return Some(y + 1);
        }
    }
    None
}

/// What a flying bird holds its height over: the higher of the ground
/// under it and the ground it is about to be over.
///
/// **A bird that only reads the column under itself flies into things**,
/// and that is exactly what the player saw: the spring is a target
/// altitude over what is below, a meadow is below a bird right up to the
/// moment a crown is, and by then the crown is in the way. One column
/// `AIR_LOOK_AHEAD` down the nose turns "it is over a tree" into "there is
/// a tree coming", which the climb has a second to do something about.
///
/// **Not while it is coming in to land**, and that clause is as
/// load-bearing as the look-ahead. A bird landing at a perch beside a tall
/// fir would read the fir as the floor it must clear, hold the fir's
/// height over the spot it wanted, and circle it for ever -- which is the
/// bug `still_up` records under a different name. So the probe is only
/// taken while the bird still has further to go than it can see: inside
/// that, the spot it is making for is the only ground that matters.
///
/// `None` only when there is nothing under the bird at all -- unloaded
/// ground -- which `walk` reads as "hold the flight level" rather than as
/// a reason to fall.
///
/// **The second number is the slope's updraught**: how far the ground ahead
/// stands above the ground below, in blocks, and never less than nought.
/// It is the subtraction between the two columns this function was already
/// reading, which is why `SLOPE_LIFT` costs nothing -- and it is nought
/// whenever the look-ahead is not taken at all, so a landing bird gets no
/// free lift from a hill it is not flying at.
fn flight_floor(world: &dyn BlockWorld, animal: &Animal) -> Option<(f32, f32)> {
    // Over the sea a gull's floor is the sea (`sea_or_ground_under`); a
    // grouse's is still whatever it could stand on.
    let column = |x: f32, z: f32| {
        if animal.species.soars() {
            sea_or_ground_under(world, x, animal.at().1 + 2.0, z)
        } else {
            perch_over(world, x, animal.at().1 + 2.0, z)
        }
    };
    let (x, y, z) = animal.at();
    let under = column(x, z).map(|cell| cell as f32);
    // Landing, or standing about: what is under it is the whole answer.
    let landing = match animal.bound_for {
        Some((tx, tz)) => (tx - x).hypot(tz - z) <= AIR_LOOK_AHEAD,
        None => !matches!(animal.mind, Mind::Flee | Mind::Soar),
    };
    if landing || animal.on_ground {
        return under.map(|under| (under, 0.0));
    }
    // Down the nose rather than down the velocity: the nose is where the
    // wings are taking it (see the flight in `walk`), and a bird part-way
    // through a bank has a velocity pointing at where it has been.
    let (sin, cos) = animal.yaw.sin_cos();
    let ahead = column(x + cos * AIR_LOOK_AHEAD, z + sin * AIR_LOOK_AHEAD).map(|cell| cell as f32);
    // The higher of the two, and never higher than the bird could climb to
    // anyway -- a cliff twenty blocks up ahead is not a reason to fly at
    // twenty blocks now. Its own height plus a climb's worth is the limit,
    // which lets a bird top a canopy and not a mountain.
    let reach = y + AIR_CLIMB_REACH;
    match (under, ahead) {
        (Some(under), Some(ahead)) => Some((under.max(ahead.min(reach)), (ahead - under).max(0.0))),
        (under, _) => under.map(|under| (under, 0.0)),
    }
}

/// Could a bird fly `AIR_PROBES` steps along that heading without putting
/// its box inside anything?
///
/// **The flier's `way_is_open`.** The walker's asks `footing` -- is there
/// somewhere to put its feet a block along -- which is the wrong question
/// three blocks up: over a lake the answer is no and over a crown it is
/// yes, so a bird steered by it turns away from open water and into the
/// tree. This one asks the only question a bird has: is the air there.
fn air_is_clear(world: &dyn BlockWorld, animal: &Animal, yaw: f32) -> bool {
    let (sin, cos) = yaw.sin_cos();
    let (x, y, z) = animal.at();
    (1..=AIR_PROBES).all(|step| {
        let reach = step as f32 * AIR_PROBE_STEP;
        let at = (
            f64::from(x + cos * reach),
            f64::from(y),
            f64::from(z + sin * reach),
        );
        fits(world, at, animal.frame())
    })
}

/// `open_heading` for a bird in the air: the nearest heading to the one it
/// wants with clear air down it.
fn air_heading(world: &dyn BlockWorld, animal: &Animal, wanted: f32) -> f32 {
    if air_is_clear(world, animal, wanted) {
        return wanted;
    }
    for swerve in SWERVES {
        for side in [swerve, -swerve] {
            if air_is_clear(world, animal, wanted + side) {
                return wanted + side;
            }
        }
    }
    wanted
}

/// The nest, or failing that the canopy, a bird would take for a home --
/// if there is one within `NEST_RANGE`.
///
/// **A nest if it can find one and a tree if it cannot**, which is the
/// player's "creates or uses nests" answered with the half this file is
/// allowed to give. It does not *place* a `BLOCK_NEST`: the module's
/// oldest rule is that animals never change the world (see the doc at
/// the top -- an animal that can build is an animal that can wreck a
/// house while its owner sleeps), and a bird that adopts a crown and sits
/// in it is the same behaviour from the outside. The nests themselves
/// come from `worldgen::place_nests`, which is where a thing that writes
/// blocks belongs.
///
/// Eight headings by three distances, each column scanned downward from
/// `NEST_LIFT` above the bird's feet and stopped at the first cell that
/// is not air: at most twelve reads a column, twenty-four columns, so
/// **288 block reads at the very worst and about 264 over open ground**
/// -- paid at most once every `NEST_SCAN_INTERVAL` seconds by a bird
/// that has no nest at all.
///
/// A nest beats a bare canopy at any distance inside the range, and
/// among equals the nearer wins. That ordering is the mechanic: a bird
/// will cross the whole search radius to sit at a real nest rather than
/// settle for the tree it is already under.
fn nest_near(world: &dyn BlockWorld, animal: &Animal) -> Option<(f32, f32)> {
    use primitive_shared::types::{
        block_kind, is_air, BLOCK_ACACIA_LEAVES, BLOCK_APPLE_LEAVES, BLOCK_APPLE_LEAVES_FRUIT,
        BLOCK_BIRCH_LEAVES, BLOCK_LEAVES, BLOCK_NEST, BLOCK_NEST_EGGS,
    };
    const HEADINGS: usize = 8;
    const STEP: f32 = 5.0;

    let feet = animal.at().1.floor() as i32;
    // Rank, then distance: a nest anywhere in range beats the crown the
    // bird is standing under. Lower is better in both.
    let mut best: Option<(u8, f32, (f32, f32))> = None;
    for point in 0..HEADINGS {
        let yaw = point as f32 / HEADINGS as f32 * std::f32::consts::TAU;
        let (sin, cos) = yaw.sin_cos();
        let mut distance = STEP;
        while distance <= NEST_RANGE {
            let (cx, cz) = (
                (animal.at().0 + cos * distance).floor() as i32,
                (animal.at().2 + sin * distance).floor() as i32,
            );
            // Down the column from the top of the window. The first thing
            // that is not air is the only thing in it worth knowing
            // about: a nest sits *on* a canopy, so a canopy with a nest
            // in it answers "nest" and one without answers "leaves".
            for y in (feet - 1..=feet + NEST_LIFT).rev() {
                let Some(block) = world.block(cx, y, cz) else {
                    break;
                };
                if is_air(block) {
                    continue;
                }
                let rank = match block_kind(block) {
                    BLOCK_NEST | BLOCK_NEST_EGGS => 0,
                    BLOCK_LEAVES | BLOCK_BIRCH_LEAVES | BLOCK_APPLE_LEAVES
                    | BLOCK_APPLE_LEAVES_FRUIT
                    // A fir and a saxaul are trees a bird nests in as well.
                    | primitive_shared::types::BLOCK_FIR_NEEDLES
                    | primitive_shared::types::BLOCK_SAXAUL_LEAVES
                    | primitive_shared::types::BLOCK_PINE_NEEDLES
                    | primitive_shared::types::BLOCK_WILLOW_LEAVES
                    // An acacia is the only tree on a plain, and a bird
                    // on a plain nests in the only tree.
                    | BLOCK_ACACIA_LEAVES
                    | primitive_shared::types::BLOCK_MAPLE_LEAVES => 1,
                    // Rock, turf, a wall: not a tree, and a bird that
                    // nested on the ground would be a bird a player
                    // trips over.
                    _ => break,
                };
                let here = (cx as f32 + 0.5, cz as f32 + 0.5);
                match best {
                    Some((best_rank, near, _))
                        if (best_rank, near) <= (rank, distance) => {}
                    _ => best = Some((rank, distance, here)),
                }
                break;
            }
            distance += STEP;
        }
    }
    best.map(|(_, _, at)| at)
}

/// Runs, in the best direction available rather than in the one asked
/// for.
///
/// **The heading is checked against the world.** A bolt used to be set
/// straight away from the player and nothing else, which is right in a
/// field and wrong everywhere a field is not: a deer with its back to a
/// cliff ran off it, a deer against a rock face ran into it and stood
/// there vibrating, and a hare cornered against a lake waded in and
/// floated away. All three read as an animal that cannot see. See
/// `open_heading`.
///
/// **And, for prey, it is aimed at cover rather than at open ground.**
/// Straight away from the threat is the right answer for one burst and
/// the wrong answer for a chase: a deer that runs down the line you are
/// already on across a meadow is caught by anyone who can sprint, and
/// there was nothing else a player could *do* about a deer. Now the
/// first burst picks the nearest wood in the half of the world that is
/// not toward you, and keeps it between bursts (see `Animal::cover`),
/// so a chase has terrain in it: the player who follows into the trees
/// loses the deer to a trunk (see `sees`), and the one who waits at the
/// edge gets it back when it wanders out.
///
/// A hare or an antelope adds a swerve on top, alternating sides every
/// burst -- see `HARE_DODGE` and `Species::zigzags`. Hostile animals get none of this: a beaten boar runs
/// straight, which is the one time a boar is easy to follow.
///
/// **And it runs the heading it has weighed, not the first one that was
/// open.** See `escape_heading`: sixteen headings scored on getting away,
/// making for its cover or its herd, keeping to the line it is already on,
/// and how far each can actually be run -- which is what takes a deer round
/// the end of a wall instead of into the corner beside it.
/// How fast a climber goes up a trunk, in blocks a second.
///
/// A little under its own walk, which is what climbing is: the same effort
/// turned through a right angle. Fast enough that a monkey with a player
/// under it is out of a spear's reach in a second and a half, which is the
/// number the whole animal turns on.
const TRUNK_CLIMB_SPEED: f32 = 1.6;

/// ...and how fast it comes back down: quicker, because coming down a trunk
/// is a controlled fall and every animal that climbs does it that way.
const TRUNK_DESCEND_SPEED: f32 = 2.4;

/// **What holds a climber up, and which way it is going.**
///
/// `None` for anything that is not a climber, and for a climber with no
/// timber against it -- both of which fall like everything else, which is the
/// half of this that makes it a climb rather than a flight.
///
/// The rule is the whole animal in four lines: a climber whose own cell or
/// the cell it faces is a trunk or a crown (`animals::is_cover`, which is
/// already this game's word for "standing timber") is *held*, and it goes up
/// while it has a reason to be up and down when it has not. The reason is
/// `Mind::Flee`: a monkey that has been frightened goes up the nearest thing,
/// and a monkey with nothing to fear comes down to the fruit. Nothing else
/// about it is special -- the collider, the turning, the speed across the
/// ground and the fall when it lets go are every other animal's.
///
/// **Rejected: a target altitude, the bird's way** (`FLIGHT_HEIGHT`). It is
/// three lines fewer and wrong in all of them: a height is a place in the
/// *air*, so a monkey would have held it over open sand with nothing under
/// it, and the palms would have been scenery it happened to hover among
/// rather than the thing it was in. The whole point of a troop is that the
/// grove is where they are safe and the beach is where they are not.
///
/// **Rejected: climbing only while a player is looking.** It is cheaper and
/// it is the kind of cheat that is found the first time somebody watches a
/// grove through the dark with a torch out.
fn climbing(animal: &Animal, world: &dyn BlockWorld) -> Option<f32> {
    if !animal.species.climbs() {
        return None;
    }
    let (x, y, z) = animal.at();
    // Its own middle and the cell its nose is in: a monkey against a trunk
    // is not standing inside it. Half a block ahead is inside the next cell
    // for anything this size.
    let (sin, cos) = animal.yaw.sin_cos();
    let ahead = (x + cos * 0.6, z + sin * 0.6);
    let holds = [(x, z), ahead].into_iter().any(|(cx, cz)| {
        world
            .block(cx.floor() as i32, y.floor() as i32, cz.floor() as i32)
            .is_some_and(is_cover)
    });
    if !holds {
        return None;
    }
    Some(if animal.mind == Mind::Flee { TRUNK_CLIMB_SPEED } else { -TRUNK_DESCEND_SPEED })
}

/// How close a monkey has to get to a person to have their dinner off them,
/// in blocks.
///
/// **An arm's length, and it is the whole of what makes the theft fair.** A
/// troop that lifted things from six blocks away would be a tax; at an arm's
/// length the player can see it coming, can back away, can throw a stone
/// (`Species::grudge_seconds`), and can simply not stand there. What it costs
/// is attention -- which is exactly the thing a camp with stores in it is
/// asking for.
const STEAL_REACH: f32 = 1.5;

/// How far off a monkey will start a raid, in blocks.
///
/// Well inside its `awareness` of twenty: a monkey sees a person across the
/// grove and does nothing about it, and comes in when they are close enough
/// that crossing the ground is a dash rather than a march. A troop that set
/// off at twenty blocks would be at the camp before the player got back to
/// it, which is a theft nobody watched happen -- and a theft nobody watched
/// is the rat's mechanic, indoors, at night (`Species::Rat`), not this one.
const RAID_RANGE: f32 = 9.0;

/// **What a monkey does about a person instead of running from one.**
///
/// `true` when it decided the thought. Three states, in order, and they are
/// the three halves of a theft anybody has ever watched happen:
///
/// * **Nothing, if it has been hit.** `angry_for` is the grudge every other
///   animal spends *coming at you*; a monkey spends it staying away, which is
///   the same field meaning the same thing -- "this person and I have had
///   words" -- read by an animal that was never going to fight. A stone
///   thrown at a monkey buys two minutes (`Species::grudge_seconds`), and the
///   player who works that out has the tool the mechanic is for.
/// * **Come, while there is something to come for.** Food in the hand is the
///   whole of what it wants, and that is deliberately the *only* thing it
///   reads: a monkey that raided a pack it could not see would be a monkey
///   with x-ray eyes, and a player would never learn what drew it. What draws
///   it is what they are holding, and they can see that too.
/// * **Take it and go.** Inside `STEAL_REACH` it marks the theft for the tick
///   loop to settle against the pack (`Animals::take_thefts`) and bolts, with
///   the same grudge set on itself: a monkey that has just robbed somebody
///   does not come back for seconds.
///
/// Rejected: **a mind that went for the *chest*** rather than for the hand. It
/// is what the player asked for in so many words ("steals food from a
/// player's camp"), and it is a worse mechanic: a container the animals could
/// open is a larder that empties while nobody is there, which is the thing
/// the rat's own note says is a bad feeling, and the defence against it is a
/// wall -- built once, thought about never. The hand is the version that
/// happens *in front of you*, and a camp is guarded by being at it.
/// Who a monkey has decided to rob and what it can see of them: the three
/// facts `raid` reads about a person, together, so the function takes a
/// person rather than three of a person's parts.
#[derive(Debug, Clone, Copy)]
struct Mark {
    who: PlayerId,
    at: (f32, f32, f32),
    distance: f32,
    /// What is in their hand: see `PlayerSign::held`.
    held: Option<primitive_shared::types::BlockId>,
}

fn raid(animal: &mut Animal, mark: Mark, seen: &Neighbours, world: &dyn BlockWorld, rng: &mut Rng) -> bool {
    let Mark { who, at, distance, held } = mark;
    if animal.angry_for > 0.0 || distance > RAID_RANGE {
        return false;
    }
    if !held.is_some_and(primitive_shared::food::is_food) {
        return false;
    }
    animal.target = Some(who);
    animal.threat_at = None;
    let toward = (at.2 - animal.at().2).atan2(at.0 - animal.at().0);
    if distance <= STEAL_REACH {
        animal.stole_from = Some(who);
        // Its own grudge, set on itself: see the doc above.
        animal.angry_for = animal.species.grudge_seconds();
        bolt(animal, world, toward + std::f32::consts::PI, seen);
        return true;
    }
    animal.mind = Mind::Chase;
    animal.wants_yaw = toward;
    animal.attitude = primitive_shared::protocol::Attitude::Alert;
    animal.next_thought = rng.range(0.15, 0.35);
    true
}

fn bolt(animal: &mut Animal, world: &dyn BlockWorld, away: f32, seen: &Neighbours) {
    animal.mind = Mind::Flee;
    animal.next_thought = FLEE_SECONDS;
    let mut wanted = away;
    // **A gull flies straight away, over whatever is there.** Cover is a
    // wood to run into and `open_heading` asks whether the ground ahead can
    // be walked -- and a gull going up off the tideline would otherwise be
    // steered away from the sea, which is exactly where it is safest.
    let soars = animal.species.soars();
    // Only what hides in a wood runs for one (`Species::hides_in_cover`): a
    // zebra's safety is its herd on open ground, which `escape_heading`
    // weighs instead.
    if !animal.fights() && !soars && animal.species.hides_in_cover() {
        if let Some((cx, cz)) = animal.cover {
            let (dx, dz) = (cx - animal.at().0, cz - animal.at().2);
            if dx * dx + dz * dz <= COVER_REACHED * COVER_REACHED {
                // Arrived. The next patch is looked for from here.
                animal.cover = None;
            }
        }
        if animal.cover.is_none() {
            animal.cover = cover_heading(world, animal, away);
        }
        if let Some((cx, cz)) = animal.cover {
            wanted = (cz - animal.at().2).atan2(cx - animal.at().0);
        }
    }
    if !animal.fights() && !soars {
        // Asked of the species rather than written as the hare's: the
        // antelope swerves too, and one figure serves both -- the same
        // fraction of a right angle is the same trick at either size.
        if animal.species.zigzags() {
            animal.dodge = -animal.dodge;
            wanted += animal.dodge * HARE_DODGE;
        }
    }
    // A bird goes up and over whatever is there (see the gull above, and
    // `homing` on why a flight is not steered by what the ground allows);
    // everything on legs weighs its way out.
    animal.wants_yaw = if soars {
        wanted
    } else if animal.species.flies() || plain_swerve() {
        open_heading(world, animal, wanted)
    } else {
        escape_heading(world, animal, away, wanted, seen)
    };
}

#[cfg(test)]
thread_local! {
    /// The flee as it was before `escape_heading`, for measuring the two in
    /// one binary (`fleeing_round_an_obstacle_course`). Per thread, because
    /// tests run side by side.
    static PLAIN_SWERVE: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
}

#[cfg(test)]
fn plain_swerve() -> bool {
    PLAIN_SWERVE.with(|flag| flag.get())
}

#[cfg(not(test))]
#[inline]
fn plain_swerve() -> bool {
    false
}

/// The heading a fleeing animal actually takes: the best of
/// `ESCAPE_HEADINGS` round the compass, never back toward the threat.
///
/// **Why a score and not the old nearest-open-swerve.** `open_heading` asks
/// one question -- can I run three blocks this way? -- and takes the first
/// yes nearest the heading it wanted. Against a wall that is a swerve along
/// the wall, whichever side happened to be tried first, into whatever is at
/// the end of that side; and every burst asked afresh from wherever the
/// threat now was, so a deer whose best line kept shifting ran a curve that
/// brought it back past the player. Here each heading is weighed on all of
/// it at once (see the weights at `ESCAPE_AWAY`), and the run is probed
/// `ESCAPE_PROBE` blocks out.
///
/// **Bounded, not exhaustive.** Every term but the probe is arithmetic, so
/// headings are tried best-first and the probing stops as soon as no
/// remaining heading could beat the best even with a clear run. In the open
/// that is one probe -- straight away is clear and nothing can beat it --
/// and against a wall a handful: eight footings each, a couple of hundred
/// block reads a heading, once a burst.
fn escape_heading(world: &dyn BlockWorld, animal: &Animal, away: f32, wanted: f32, seen: &Neighbours) -> f32 {
    use std::f32::consts::TAU;
    // Toward the middle of its herd, for a herd animal, when that is not
    // back the way the threat is.
    let herd = match (animal.species.grouping(), seen.centre) {
        (Grouping::Herd, Some((cx, cz))) if !animal.species.zigzags() => {
            let (dx, dz) = (cx - animal.at().0, cz - animal.at().2);
            let toward = dz.atan2(dx);
            (dx.hypot(dz) > HERD_COMFORT * 0.5 && (toward - away).cos() >= 0.0).then_some(toward)
        }
        _ => None,
    };
    // The line it is already running, except for a zig-zagger, whose whole
    // trick is not keeping it.
    let keep = (animal.mind == Mind::Flee && !animal.species.zigzags()).then_some(animal.wants_yaw);
    let partial = |yaw: f32| {
        ESCAPE_AWAY * (yaw - away).cos()
            + ESCAPE_WANTED * (yaw - wanted).cos()
            + keep.map_or(0.0, |k| ESCAPE_KEEP * (yaw - k).cos())
            + herd.map_or(0.0, |h| ESCAPE_HERD * (yaw - h).cos())
    };
    let mut candidates = [(0.0f32, f32::NEG_INFINITY); ESCAPE_HEADINGS + 1];
    let mut count = 0;
    for i in 0..=ESCAPE_HEADINGS {
        let yaw = if i == ESCAPE_HEADINGS {
            wanted
        } else {
            away + i as f32 / ESCAPE_HEADINGS as f32 * TAU
        };
        if (yaw - away).cos() < ESCAPE_TOWARD {
            continue;
        }
        candidates[count] = (yaw, partial(yaw));
        count += 1;
    }
    let candidates = &mut candidates[..count];
    candidates.sort_by(|a, b| b.1.total_cmp(&a.1));
    let mut best: Option<(f32, f32)> = None;
    // **What it was making for, if it can run it.** The cover it chose and
    // the zig-zag are decisions already, and in open ground re-weighing them
    // against a compass of headings only bent them: a hare's swerve came out
    // at twenty-two degrees instead of thirty-five because that was where
    // the nearest heading on the grid scored. So the wanted line is probed
    // first, and taken as it is when the whole probe is clear -- unless a
    // herd animal's herd is there to be weighed.
    if herd.is_none() && clear_run(world, animal, wanted, ESCAPE_PROBE) >= ESCAPE_PROBE {
        return wanted.rem_euclid(TAU);
    }
    for &(yaw, part) in candidates.iter() {
        if best.is_some_and(|(_, score)| part + ESCAPE_CLEAR <= score) {
            break;
        }
        let score = part + ESCAPE_CLEAR * clear_run(world, animal, yaw, ESCAPE_PROBE) / ESCAPE_PROBE;
        if best.is_none_or(|(_, b)| score > b) {
            best = Some((yaw, score));
        }
    }
    best.map_or(wanted, |(yaw, _)| yaw.rem_euclid(TAU))
}

/// How far, up to `most` blocks, this animal could run along a heading
/// before the ground refuses it: a wall, a drop, water, the edge of what is
/// loaded. `way_is_open` asks the same question as a yes or no.
fn clear_run(world: &dyn BlockWorld, animal: &Animal, yaw: f32, most: f32) -> f32 {
    let (sin, cos) = yaw.sin_cos();
    let mut at = animal.at();
    let mut run = 0.0;
    while run < most {
        let ahead = (at.0 + cos, at.1, at.2 + sin);
        let Some(y) = footing(world, ahead, animal.species) else {
            break;
        };
        at = (ahead.0, y, ahead.2);
        run += 1.0;
    }
    run
}

/// What the per-tick part of `step` hands every mind: how every player looks
/// and sounds, the wind, and what is left of this tick's rays.
struct Senses<'a> {
    figures: &'a [Figure],
    wind: (f32, f32),
    rays: &'a mut u32,
}

/// How much of a nose's reach the wind leaves between a person at `from` and
/// an animal at `to`. See `SCENT_STILL`.
fn scent_carry(wind: (f32, f32), from: (f32, f32, f32), to: (f32, f32, f32)) -> f32 {
    let (dx, dz) = (to.0 - from.0, to.2 - from.2);
    let length = dx.hypot(dz);
    let along = if length > 1e-3 { (wind.0 * dx + wind.1 * dz) / length } else { 0.0 };
    (SCENT_STILL + SCENT_CARRY * along).clamp(SCENT_RANGE.0, SCENT_RANGE.1)
}

/// Is this animal keyed up -- wary, running, watching, or already taken up
/// with somebody? Then its senses are `KEEN` and it looks all the way round.
fn is_keen(animal: &Animal) -> bool {
    animal.wary_for > 0.0 || animal.target.is_some() || matches!(animal.mind, Mind::Flee | Mind::Watch)
}

/// What one animal makes of one person: touch, hearing and scent first, and
/// the ray last and only if it could change the answer.
///
/// `acuity` scales every reach: a roosting gull's half, or nothing at all
/// for a gull in the air (`seabird_reach`).
fn perceive(
    world: &dyn BlockWorld,
    animal: &Animal,
    figure: &Figure,
    wind: (f32, f32),
    acuity: f32,
    rays: &mut u32,
) -> Perception {
    if acuity <= 0.0 {
        return Perception::Nothing;
    }
    let (dx, dz) = (figure.at.0 - animal.at().0, figure.at.2 - animal.at().2);
    let distance = dx.hypot(dz);
    if distance <= PRESENCE && (figure.at.1 - animal.at().1).abs() < 3.0 {
        return Perception::Sure;
    }
    let species = animal.species;
    let keen = if is_keen(animal) { KEEN } else { 1.0 };
    let mut maybe = false;
    // Hearing and scent: distance only, and a maybe in the outer band.
    for reach in [
        species.hearing() * figure.loudness * keen * acuity,
        species.nose() * scent_carry(wind, figure.at, animal.at()) * figure.reek * acuity,
    ] {
        if distance <= reach {
            if distance <= reach * ALERT_BAND {
                return Perception::Sure;
            }
            maybe = true;
        }
    }
    // Sight: the reach, then the cone, and only then the ray.
    let sight = species.awareness() * figure.visibility * keen * acuity;
    if distance <= sight {
        let (sin, cos) = animal.yaw.sin_cos();
        let looking = keen > 1.0 || (dx * cos + dz * sin) / distance.max(1e-3) >= species.view_cone();
        if looking {
            if *rays == 0 {
                // No ray to spare. A maybe is still a maybe; otherwise it
                // looks again next tick rather than deciding it saw nothing.
                return if maybe { Perception::Maybe } else { Perception::Deferred };
            }
            *rays -= 1;
            if sees(world, animal, player_eye(figure.at)) {
                return Perception::Sure;
            }
        }
    }
    if maybe {
        Perception::Maybe
    } else {
        Perception::Nothing
    }
}

/// The furthest any sense of this animal could possibly reach, so people
/// nowhere near are skipped without asking.
fn furthest_sense(species: Species, acuity: f32) -> f32 {
    let hear = species.hearing() * WORKING_LOUDNESS;
    // ...the smell as far as the rankest coat carries it (`equipment::reek`).
    let smell = species.nose() * SCENT_RANGE.1 * primitive_shared::equipment::TAR_REEK;
    // The brightest a person can be is sitting by a fire at night: a scan
    // cut at the torch's reach would never find the camp the fire lit.
    let see = species.awareness() * TORCH_AT_NIGHT.max(FIRE_SEEN_AT_NIGHT) * Gait::Running.visibility();
    hear.max(smell).max(see) * KEEN * acuity.max(0.0)
}

/// Who was noticed, where they are, how far off, and how surely.
type Noticed = (PlayerId, (f32, f32, f32), f32, Perception);

/// The person this animal has noticed, if any: the nearest it is sure of, or
/// failing that the nearest it has a maybe about -- or `Deferred` if the only
/// answer was a look the tick had no ray left for.
fn notice(
    world: &dyn BlockWorld,
    animal: &Animal,
    senses: &mut Senses<'_>,
    acuity: f32,
) -> Option<Noticed> {
    let furthest = furthest_sense(animal.species, acuity);
    // A handful of people at most, nearest first, so the first sure answer
    // is the right one and the rays go on the nearest.
    let mut near: Vec<(usize, f32)> = senses
        .figures
        .iter()
        .enumerate()
        .map(|(i, f)| (i, (f.at.0 - animal.at().0).hypot(f.at.2 - animal.at().2)))
        .filter(|&(_, d)| d <= furthest)
        .collect();
    near.sort_by(|a, b| a.1.total_cmp(&b.1));
    let mut fallback: Option<Noticed> = None;
    for (index, distance) in near {
        let figure = senses.figures[index];
        match perceive(world, animal, &figure, senses.wind, acuity, senses.rays) {
            Perception::Sure => return Some((figure.who, figure.at, distance, Perception::Sure)),
            Perception::Maybe if !matches!(fallback, Some((_, _, _, Perception::Maybe))) => {
                fallback = Some((figure.who, figure.at, distance, Perception::Maybe));
            }
            Perception::Deferred if fallback.is_none() => {
                fallback = Some((figure.who, figure.at, distance, Perception::Deferred));
            }
            _ => {}
        }
    }
    fallback
}

/// Where a player looks from, given where they stand.
///
/// `EYE_HEIGHT` is the client's own number for the same thing, so a
/// deer decides whether it can see you from the point you see it from.
fn player_eye(feet: (f32, f32, f32)) -> (f32, f32, f32) {
    (
        feet.0,
        feet.1 + primitive_shared::geometry::EYE_HEIGHT,
        feet.2,
    )
}

/// Can this animal see that point from where it stands?
///
/// A ray from its eye to the point, sampled a quarter of a block at a
/// time and asked of each new cell it enters whether `blocks_sight` --
/// so leaves and trunks stop it and water and grass (for anything taller
/// than a hare) do not. The cell the animal's own eye is in and the cell
/// the target is in are skipped: a deer standing *in* tall grass is not
/// blind, and a player whose head is in a bush is still a player.
///
/// **Sampled rather than a proper voxel walk**, and the trade is
/// deliberate: a quarter-block step can miss a cell the ray only clips
/// the corner of, which is a leaf the deer sees round -- and nobody will
/// ever notice that. What they would notice is the cost, and this is
/// forty lookups at the outside for a ray the length of a deer's
/// awareness, cast twice a second per fleeing animal (see `LOOK_EVERY`)
/// and once a second per grazing one with a person in range. A cell the
/// world has not loaded is treated as something in the way: an animal
/// must not act on a person in terrain that has not arrived.
fn sees(world: &dyn BlockWorld, animal: &Animal, to: (f32, f32, f32)) -> bool {
    let from = animal.eye();
    let eye_height = from.1 - animal.at().1;
    let (dx, dy, dz) = (to.0 - from.0, to.1 - from.1, to.2 - from.2);
    let length = (dx * dx + dy * dy + dz * dz).sqrt();
    if length < 1e-3 {
        return true;
    }
    let cell_of = |p: (f32, f32, f32)| (p.0.floor() as i32, p.1.floor() as i32, p.2.floor() as i32);
    let (start, end) = (cell_of(from), cell_of(to));
    let steps = (length * 4.0).ceil().max(1.0) as i32;
    let mut last = start;
    for i in 1..steps {
        let t = i as f32 / steps as f32;
        let cell = cell_of((from.0 + dx * t, from.1 + dy * t, from.2 + dz * t));
        if cell == last || cell == start || cell == end {
            continue;
        }
        last = cell;
        match world.block(cell.0, cell.1, cell.2) {
            None => return false,
            Some(block) if blocks_sight(block, eye_height) => return false,
            Some(_) => {}
        }
    }
    true
}

/// The nearest patch of cover a fleeing animal can make for, on the
/// ground, if there is one within `COVER_RANGE` that is not back toward
/// the threat.
///
/// Twelve headings, thirty degrees apart, sampled outward two blocks at
/// a time; a heading is only considered if it is not *toward* `away`'s
/// opposite -- sideways is allowed, because a wood beside you is a wood,
/// and straight back at the player is not. A column counts as cover if
/// anything from a block under the animal's feet to four over them is
/// `is_cover` (a trunk at foot level, a canopy overhead, grass around
/// the legs); it is not asked whether the animal can *stand* there,
/// because `open_heading` answers that on the way, and a deer aimed at
/// a tree it has to go round is exactly the deer that is wanted.
///
/// The nearest wins, which sends the deer to the edge of the wood and
/// not its middle -- the next burst, from the edge, finds the middle. At
/// most about forty columns of six lookups, once per bolt that does not
/// already have cover in mind (see `Animal::cover`), which is the same
/// order of cost as `food_heading` and paid a good deal less often.
fn cover_heading(world: &dyn BlockWorld, animal: &Animal, away: f32) -> Option<(f32, f32)> {
    const HEADINGS: usize = 12;
    let (away_sin, away_cos) = away.sin_cos();
    let foot = animal.at().1.floor() as i32;
    let mut best: Option<((f32, f32), f32)> = None;
    for point in 0..HEADINGS {
        let yaw = point as f32 / HEADINGS as f32 * std::f32::consts::TAU;
        let (sin, cos) = yaw.sin_cos();
        // Not back toward whatever it is running from. `-0.05` rather
        // than zero so a heading at exactly a right angle is not lost
        // to rounding; that heading is sideways, and sideways is safe.
        if cos * away_cos + sin * away_sin < -0.05 {
            continue;
        }
        let mut distance = 2.0;
        while distance <= COVER_RANGE {
            if best.is_some_and(|(_, near)| near <= distance) {
                break;
            }
            let (x, z) = (
                animal.at().0 + cos * distance,
                animal.at().2 + sin * distance,
            );
            let (cx, cz) = (x.floor() as i32, z.floor() as i32);
            let covered = (foot - 1..=foot + 4)
                .any(|y| world.block(cx, y, cz).is_some_and(is_cover));
            if covered {
                best = Some(((x, z), distance));
                break;
            }
            distance += 2.0;
        }
    }
    best.map(|(at, _)| at)
}

/// Is there a fire lit near enough to `at` to keep a night hunter off it?
///
/// The same circle a pack will not step into (`FIRE_RADIUS`), asked by the
/// server of a bed before the night is let pass (`find_the_sleeper`) -- one
/// answer to "does this fire keep them off", so the bed and the wolf can
/// never disagree about whether a fire counted.
pub fn fire_keeps_the_night_off(world: &dyn BlockWorld, at: (f32, f32, f32)) -> bool {
    lit_fire_near(world, at, FIRE_RADIUS).is_some()
}

/// The nearest lit fire within `radius` of a point, if there is one:
/// a burning hearth (`is_burning`) or a torch left standing lit.
///
/// A disc of columns, four cells tall from a block under the feet to
/// two over the head -- a fire on the ground, a torch in a wall at eye
/// level. About three hundred lookups at `FIRE_RADIUS`, which is why
/// it is asked only by a wolf, only after dark, and only on the thought
/// at which it would otherwise have come in (see `think_hunter`); a
/// scan every tick for every animal would be the most expensive thing
/// in the file for a fact that changes once an evening.
///
/// Cells that are not loaded are skipped rather than treated as fire:
/// a wolf must not be kept off you by a chunk that has not arrived.
fn lit_fire_near(world: &dyn BlockWorld, at: (f32, f32, f32), radius: f32) -> Option<(f32, f32, f32)> {
    let (cx, cy, cz) = (at.0.floor() as i32, at.1.floor() as i32, at.2.floor() as i32);
    let reach = radius.ceil() as i32;
    let mut best: Option<((f32, f32, f32), f32)> = None;
    for dz in -reach..=reach {
        for dx in -reach..=reach {
            if dx * dx + dz * dz > reach * reach {
                continue;
            }
            for dy in -1..=2 {
                let Some(block) = world.block(cx + dx, cy + dy, cz + dz) else {
                    continue;
                };
                if !is_burning(block) && !is_lit_torch(block) {
                    continue;
                }
                let fire = (
                    (cx + dx) as f32 + 0.5,
                    (cy + dy) as f32,
                    (cz + dz) as f32 + 0.5,
                );
                let distance_sq = (fire.0 - at.0).powi(2) + (fire.2 - at.2).powi(2);
                if distance_sq > radius * radius {
                    continue;
                }
                match best {
                    Some((_, near)) if near <= distance_sq => {}
                    _ => best = Some((fire, distance_sq)),
                }
            }
        }
    }
    best.map(|(fire, _)| fire)
}

/// How far off a hungry scavenger smells carrion, in blocks.
///
/// Ten: shorter than a wolf's awareness of a *person* (eighteen), so a
/// player in the open still outranks a kill in the grass, and long
/// enough that a carcass left near a camp draws whatever is in the wood
/// behind it. The scan it pays for is the fire scan's shape at half the
/// radius, and it is asked only when there is nobody to hunt.
///
/// One radius for a dead animal and a dead player alike. A second number
/// for bodies would be a second thing to tune and a second thing to
/// explain, and there is nothing about a person lying in a wood that
/// carries further than a deer lying in the same wood.
const CARRION_RADIUS: f32 = 10.0;

/// How near a scavenger has to be to carrion to be eating it.
const FEEDING_REACH: f32 = 1.8;

/// The nearest thing worth eating within `radius`, if there is one.
///
/// `lit_fire_near`'s disc, one band shallower: carrion lies on the floor
/// and there is no such thing as a carcass in a wall. Cells that are not
/// loaded are skipped rather than guessed at, which is the rule
/// everywhere in this file.
///
/// **A dead player counts** (`types::draws_scavengers`), and the nearest
/// wins whichever it is -- a wolf choosing a deer over a body because one
/// of them is a player would be the world knowing something it has no way
/// of knowing.
fn carrion_near(
    world: &dyn BlockWorld,
    at: (f32, f32, f32),
    radius: f32,
) -> Option<(f32, f32, f32)> {
    let (cx, cy, cz) = (at.0.floor() as i32, at.1.floor() as i32, at.2.floor() as i32);
    let reach = radius.ceil() as i32;
    let mut best: Option<((f32, f32, f32), f32)> = None;
    for dz in -reach..=reach {
        for dx in -reach..=reach {
            if dx * dx + dz * dz > reach * reach {
                continue;
            }
            for dy in -1..=1 {
                let Some(block) = world.block(cx + dx, cy + dy, cz + dz) else {
                    continue;
                };
                if !primitive_shared::types::draws_scavengers(block) {
                    continue;
                }
                let kill = (
                    (cx + dx) as f32 + 0.5,
                    (cy + dy) as f32,
                    (cz + dz) as f32 + 0.5,
                );
                let distance_sq = (kill.0 - at.0).powi(2) + (kill.2 - at.2).powi(2);
                if distance_sq > radius * radius {
                    continue;
                }
                match best {
                    Some((_, near)) if near <= distance_sq => {}
                    _ => best = Some((kill, distance_sq)),
                }
            }
        }
    }
    best.map(|(kill, _)| kill)
}

/// Is it dark out?
///
/// The sun is up between a quarter past and three quarters through the
/// day -- see `Sky::sun_elevation`, which is the same sine. This is that
/// comparison and nothing more, so the two cannot drift apart into a
/// world where the animals are asleep in the sunshine.
pub fn is_night(time_of_day: f32) -> bool {
    let t = time_of_day.rem_euclid(1.0);
    !(0.25..0.75).contains(&t)
}

/// Is the sun near the top of the sky?
///
/// The four hours of a sixteen-hour day either side of noon, on the same
/// clock `is_night` reads so the two cannot describe different days. Read by
/// one thing: the animal that would rather be lying up in the shade than
/// standing in a meadow being looked at (see `MIDDAY_REST`).
fn is_midday(time_of_day: f32) -> bool {
    let t = time_of_day.rem_euclid(1.0);
    (0.42..0.58).contains(&t)
}

/// How often a grazing animal in the shade at noon settles instead of
/// carrying on, and for how long.
///
/// **A herd that feeds at the same rate at noon as at six in the morning is a
/// herd on a timer.** The hours an animal is actually up and eating are the
/// ends of the day; the middle of it is spent standing in whatever shade
/// there is, which is why a hunter who walks a meadow at midday finds it
/// empty and one who walks the wood's edge finds everything in it. That is a
/// decision -- where to look, and when -- which is the bar a mechanic has to
/// clear here.
///
/// Only in shade, and shade is what `wooded` already answers for the birds.
/// An animal caught in the open at noon does not lie down in it: it goes on
/// grazing, uncomfortably, which is also what one does.
const MIDDAY_REST: (f32, f32) = (12.0, 22.0);

/// ...and the odds it takes the chance when it is standing in the shade with
/// nothing happening. Not one, because a herd that all settled on the same
/// thought would settle in unison -- the lockstep `Animal::pace` exists to
/// break up, arriving by another door.
///
/// **Rolled last of the three conditions, and that is not a style choice.**
/// `Rng` is one stream shared by every animal on the server, so a draw taken
/// on a branch that then decides nothing still moves every later draw along --
/// which is a change of behaviour everywhere, reproducible and impossible to
/// attribute. The cheap certain tests come first and the die is rolled only
/// when it can actually decide something.
const MIDDAY_CHANCE: f32 = 0.45;

/// How far up an animal looks for something over its head.
///
/// Eight blocks: a canopy, a rock overhang, a roof somebody built. Everything
/// that reads as shade from underneath, which is the question being asked --
/// `wooded` answers a different and much more expensive one (a hundred and
/// twenty columns), and what it is for is deciding whether a *place* is a
/// wood, not whether this animal is out of the sun.
const SHADE_REACH: i32 = 8;

/// Is there anything over this animal's head?
///
/// One column, at most `SHADE_REACH` reads, and it stops at the first thing it
/// finds. Cheap enough to ask on an idle thought without a die roll in front
/// of it -- see `MIDDAY_CHANCE` for why that matters more than it looks.
fn in_shade(world: &dyn BlockWorld, animal: &Animal) -> bool {
    let at = animal.at();
    let (bx, bz) = (at.0.floor() as i32, at.2.floor() as i32);
    let head = (at.1 + animal.species.height()).floor() as i32;
    (1..=SHADE_REACH).any(|dy| {
        world
            .block(bx, head + dy, bz)
            .is_some_and(|block| block != primitive_shared::types::BLOCK_AIR)
    })
}

/// How much of a settled night an animal spends with its head down.
///
/// **Not sleep**, for the reason the night's own `restless` is not sleep:
/// everything above `graze` ran first, so a dozing deer still startles, still
/// hears and still runs. What it is is the difference between a field of
/// boxes standing to attention at three in the morning and a field of animals
/// -- and it is the thing that makes a torch at night worth carrying, because
/// what the light finds is an animal that has not got its head up yet.
const NIGHT_DOZE: f32 = 0.7;

/// Standing about, or wandering off: the nine tenths of an animal's life
/// nobody is watching.
///
/// Mostly standing. An animal that is always walking is a thing on
/// rails, and the pauses are most of what makes a grazing herd read as
/// one.
fn graze(
    animal: &mut Animal,
    seen: &Neighbours,
    world: &dyn BlockWorld,
    rng: &mut Rng,
    time_of_day: f32,
) {
    // **Nothing much happens at night.** An animal that wanders the
    // hours of darkness at the same rate it wanders noon is a thing on
    // a timer rather than a thing that is alive -- and a world where
    // the deer stand still after dark is a world with a reason to hunt
    // then, which is the one time a player with a torch has the
    // advantage. It is a settled herd rather than sleep: they still
    // startle, because everything above this ran first.
    let restless = if is_night(time_of_day) { 0.1 } else { 0.35 };

    // **Not here.** Before the herd, before the grass: an animal
    // standing where it or its herd was hurt walks out of the place
    // first, whatever else it wanted. This is checked ahead of the
    // herd-centre pull on purpose -- the centre of a herd that was shot
    // at is *in* the danger, and a deer that went back to its herd
    // before leaving the meadow would never leave the meadow. The herd
    // follows instead: each animal walks out on its own, the centre
    // moves with them, and the stragglers are pulled after it. See
    // `Animal::dangers` and `DANGER_RADIUS`.
    // **Head up until something puts it down.** Every path out of this
    // function that is not a mouthful, a drink or a doze is an animal walking
    // or standing with its head where a head normally is, and writing that
    // once here is cheaper -- and much harder to forget -- than writing it in
    // the eleven places below that return. What it *was* is kept, because the
    // feeding bout below is an alternation and an alternation needs to know
    // which half it is in.
    let was = animal.attitude;
    animal.attitude = primitive_shared::protocol::Attitude::Easy;

    if let Some((dx, dz)) = animal.danger_underfoot() {
        animal.mind = Mind::Wander;
        animal.wants_yaw = open_heading(world, animal, (-dz).atan2(-dx));
        animal.next_thought = rng.range(1.5, 3.5);
        return;
    }

    // **Thirsty: go to the water.**
    //
    // After the remembered danger and before the herd, and both
    // positions are decisions. A place the herd was shot at is worth
    // leaving whatever else the animal wanted -- thirst is a need and a
    // hunter is a reason to be somewhere else entirely. But thirst beats
    // the herd pull, because an animal that went back to the middle of
    // its herd before every drink would never reach a river the herd is
    // not standing on, and what a watering place *is* is the thing that
    // pulls a herd off its own centre for a minute.
    //
    // Nothing here is reached with anything at all in sight: `graze` is
    // only ever called when there is no player, no threat and no quarry
    // (see `think` and `think_hunter`), which is what makes "fleeing
    // beats thirst" true without a line of code saying so.
    // Thirsty enough to go looking, or already at the water and not yet
    // done: see the note on `Animal::water` being cleared only when the
    // thirst is spent.
    if animal.thirst >= THIRSTY_AT || (animal.water.is_some() && animal.thirst > 0.0) {
        if animal.water.is_none() && animal.next_water_scan <= 0.0 {
            // Charged whether or not anything is found: a dry meadow
            // must not cost a scan a second. See `WATER_SCAN_INTERVAL`.
            animal.next_water_scan = WATER_SCAN_INTERVAL;
            animal.water = water_near(world, animal);
        }
        if let Some((wx, wz)) = animal.water {
            let (dx, dz) = (wx - animal.at().0, wz - animal.at().2);
            if dx * dx + dz * dz <= DRINK_REACH * DRINK_REACH {
                animal.mind = Mind::Drink;
                // Facing the water, because that is where its head is.
                animal.wants_yaw = dz.atan2(dx);
                animal.attitude = primitive_shared::protocol::Attitude::Drinking;
                animal.next_thought = rng.range(DRINK_SECONDS.0, DRINK_SECONDS.1);
            } else {
                animal.mind = Mind::Wander;
                animal.wants_yaw = open_heading(world, animal, dz.atan2(dx));
                // Often, and this is the whole of what keeps an animal
                // from swimming in the pond it came to drink from: a
                // deer walks two blocks in a second, `DRINK_REACH` is
                // one and a half, and an approach re-aimed once a second
                // therefore steps straight past the point at which it
                // should have stopped -- into the water, where `walk`
                // turns it away, and round the pond for ever.
                animal.next_thought = rng.range(0.3, 0.6);
            }
            return;
        }
    }

    // **Wary: keep drifting off from where the danger was**, and do not
    // stand long anywhere. See `WARY_SECONDS`.
    let mut restless: f32 = restless;
    if let (true, Some((tx, tz))) = (animal.wary_for > 0.0, animal.threat_at) {
        let (dx, dz) = (animal.at().0 - tx, animal.at().2 - tz);
        if dx.hypot(dz) < animal.species.awareness() * 1.5 && rng.chance(0.6) {
            animal.mind = Mind::Wander;
            animal.wants_yaw = open_heading(world, animal, dz.atan2(dx) + rng.range(-0.6, 0.6));
            animal.next_thought = rng.range(1.0, 2.5);
            return;
        }
        restless = restless.max(0.5);
    }

    // **A rat keeps to the walls.** Everything below is a grazer's day --
    // a herd, a tuft to walk to, a random heading across open ground --
    // and a rat that took it crossed the middle of the storeroom like a
    // hare crossing a meadow. See `skulk`.
    if animal.species == Species::Rat {
        skulk(animal, world, rng);
        return;
    }

    // ---- the herd and the pack ----
    //
    // **A herd follows somebody.** What was here was one rule -- drift back
    // to the middle of the others when you have strayed -- and a herd made of
    // only that moves when one animal happens to wander and the rest are
    // pulled after it, a few seconds late, one at a time: a scatter that
    // reassembles rather than a herd going somewhere. Now there is a leader
    // (`Neighbours::leader`), and three rules in order: do not stand on top
    // of one another (`PERSONAL_SPACE`), go with the leader when it moves
    // (`HERD_FOLLOW`), and graze near it when it does not -- more patiently
    // than it does, so the leader is the one that sets the herd off.
    let grouping = animal.species.grouping();
    // **The stallion goes for the one that has strayed**, before any rule
    // about his own place in the herd: out past her, so the herd's own rules
    // -- which put her back toward the middle of whoever is near -- have him
    // on the far side to put her back toward.
    if let Some((sx, sz)) = seen.straggler {
        let (dx, dz) = (sx - animal.at().0, sz - animal.at().2);
        animal.mind = Mind::Wander;
        animal.wants_yaw = open_heading(world, animal, dz.atan2(dx));
        animal.next_thought = rng.range(0.8, 1.6);
        return;
    }
    if matches!(grouping, Grouping::Herd | Grouping::Pack) {
        if let Some((_, mate)) = seen.packmate {
            let (dx, dz) = (animal.at().0 - mate.0, animal.at().2 - mate.2);
            if dx.hypot(dz) < PERSONAL_SPACE {
                animal.mind = Mind::Wander;
                animal.wants_yaw = open_heading(world, animal, dz.atan2(dx) + rng.range(-0.5, 0.5));
                animal.next_thought = rng.range(0.4, 0.8);
                return;
            }
        }
        if let Some(leader) = seen.leader {
            let gap = if grouping == Grouping::Pack { PACK_FOLLOW } else { HERD_FOLLOW };
            let (dx, dz) = (leader.at.0 - animal.at().0, leader.at.2 - animal.at().2);
            let behind = dx.hypot(dz);
            if (leader.moving && behind > gap * 0.6) || behind > gap * 1.2 {
                // Along the leader's line, bent toward the leader the further
                // back it is: a herd moving in file, not converging on a point.
                let toward = dz.atan2(dx);
                let pull = ((behind - gap * 0.6) / gap).clamp(0.0, 1.0);
                let heading = if leader.moving {
                    let (sin, cos) = (
                        leader.heading.sin() * (1.0 - pull) + toward.sin() * pull,
                        leader.heading.cos() * (1.0 - pull) + toward.cos() * pull,
                    );
                    sin.atan2(cos)
                } else {
                    toward
                };
                animal.mind = Mind::Wander;
                animal.wants_yaw = open_heading(world, animal, heading);
                animal.next_thought = rng.range(0.6, 1.4);
                return;
            }
            // Near a leader that is standing: stand too, mostly.
            restless *= 0.4;
        }
    }

    // Drifting back to the others. Only when it has actually strayed:
    // an animal that steered at the middle of its herd from inside it
    // would pile the whole herd onto one block, which is not a herd, it
    // is a heap. How far is straying is the kind's own (`Grouping`): a
    // hare two meadows' width from the next is still where hares are.
    let comfort = match grouping {
        Grouping::Herd => HERD_COMFORT,
        Grouping::Pack => PACK_COMFORT,
        _ => LOOSE_COMFORT,
    };
    if let Some((cx, cz)) = seen.centre {
        let (dx, dz) = (cx - animal.at().0, cz - animal.at().2);
        if dx * dx + dz * dz > comfort * comfort {
            animal.mind = Mind::Wander;
            animal.wants_yaw = open_heading(world, animal, dz.atan2(dx));
            animal.next_thought = rng.range(1.5, 3.5);
            return;
        }
    }

    if rng.chance(restless) {
        animal.mind = Mind::Wander;
        // Toward something worth walking to, if there is any. A wander
        // aimed at nothing is a random walk with an animal drawn on it;
        // a wander aimed at the nearest tuft is the same cost and reads
        // as foraging. Failing that, somewhere it can at least walk --
        // see `open_heading`, without which a wander into a wall is
        // three seconds of an animal pressed against it.
        let wanted = if animal.species.is_predator() {
            None
        } else {
            food_heading(world, animal)
        };
        animal.wants_yaw = match wanted {
            Some(yaw) => yaw,
            None => open_heading(world, animal, rng.range(0.0, std::f32::consts::TAU)),
        };
        animal.next_thought = rng.range(1.5, 5.0);
        // **In company, a short wander and not away from the one in front.**
        // Five seconds at a walk is ten blocks, and a herd animal that took
        // one of those the wrong way was past `HERD_RADIUS` of everything --
        // out of sight of its herd, with no herd rule left that could reach
        // it, for the rest of its life. Measured on
        // `wolves_of_one_pack_stay_within_sixteen_blocks_of_each_other`: one
        // wolf of five was fifty blocks off after two minutes.
        if matches!(grouping, Grouping::Herd | Grouping::Pack) && seen.company > 0 {
            animal.next_thought = rng.range(1.0, 2.5);
            if let Some(leader) = seen.leader {
                let gap = if grouping == Grouping::Pack { PACK_FOLLOW } else { HERD_FOLLOW };
                let (dx, dz) = (leader.at.0 - animal.at().0, leader.at.2 - animal.at().2);
                if dx.hypot(dz) > gap * 0.8 {
                    animal.wants_yaw = open_heading(world, animal, dz.atan2(dx) + rng.range(-0.8, 0.8));
                }
            }
        }
    } else {
        // **What it is standing over decides how long it stands there.**
        //
        // Not *whether* -- that was the first shape of this and it was
        // wrong in a way the tests caught: on grassland `standing_in_food`
        // is true everywhere, so an animal that fed instead of deciding
        // fed for ever and a meadow full of deer never moved again.
        //
        // Lengthening the pause is the whole effect that was wanted. An
        // animal that stops for the same second and a half wherever it
        // happens to be is a random walk with a drawing on it; one that
        // stops for six seconds where the grass is has a reason to be
        // standing there, and that reading costs one field lookup.
        animal.mind = Mind::Idle;
        // **Standing is not being switched off.** An idle animal used to
        // be perfectly still for up to ten seconds, which is a statue
        // with a walk cycle; half the time it now looks somewhere else
        // while it stands, which costs one number and is most of what
        // makes a field of grazing deer read as alive.
        if rng.chance(0.5) {
            animal.wants_yaw += rng.range(-1.1, 1.1);
        }
        use primitive_shared::protocol::Attitude;
        let feet = primitive_shared::geometry::narrow(animal.position);
        let grass = !animal.species.is_predator() && standing_in_food(world, feet);
        animal.next_thought = if animal.wary_for > 0.0 {
            // Head up again soon: a wary animal feeds in snatches.
            //
            // **And the head is actually up.** This branch is the one an
            // animal takes when something has frightened it and gone: the
            // pause is short because it will not settle, and it spends the
            // pause looking, which is the difference between a wary animal
            // and a hungry one standing in the same spot.
            animal.attitude = Attitude::Alert;
            rng.range(0.8, 2.0)
        } else if is_night(time_of_day) {
            if rng.chance(NIGHT_DOZE) {
                animal.attitude = Attitude::Dozing;
            }
            rng.range(4.0, 10.0)
        } else if is_midday(time_of_day) && in_shade(world, animal) && rng.chance(MIDDAY_CHANCE) {
            // **Lying up in the shade through the middle of the day.** See
            // `MIDDAY_REST`. Charged one `wooded` scan, and only on the
            // thought that is already standing still, at noon, on the
            // fraction of thoughts `MIDDAY_CHANCE` lets through -- which is
            // a few block reads a minute for an animal in a wood and none at
            // all for one in a meadow, because `is_midday` is a compare and
            // it is tested first.
            animal.attitude = Attitude::Dozing;
            rng.range(MIDDAY_REST.0, MIDDAY_REST.1)
        } else if grass {
            // **Mouthfuls, with the head coming up between them.** A herd
            // that grazed with every head down for six seconds at a time
            // looked like a herd of animals looking for something they had
            // dropped; what one actually does is take two or three bites and
            // then stand up and look round, because a head in the grass
            // cannot see a wolf. Alternated on the thought rather than run on
            // a clock of its own, so a bout is a whole number of decisions
            // and the two never fall out of step.
            if was == Attitude::Feeding {
                animal.attitude = Attitude::Alert;
                rng.range(1.2, 2.6)
            } else {
                animal.attitude = Attitude::Feeding;
                rng.range(FEEDING_SECONDS.0, FEEDING_SECONDS.1)
            }
        } else {
            rng.range(1.0, 4.0)
        };
    }
}

/// Is there something to eat in the cell an animal is standing in, or in
/// the one under its feet?
///
/// Two lookups. What counts is what a grazing animal would actually put
/// its head down for: a tuft of grass, a berry bush, reeds, a flower --
/// anything the world calls foliage -- or plain turf, which is grass and
/// is the commonest thing under an animal's feet in this world.
fn standing_in_food(world: &dyn BlockWorld, feet: (f32, f32, f32)) -> bool {
    let (x, z) = (feet.0.floor() as i32, feet.2.floor() as i32);
    let y = feet.1.floor() as i32;
    let here = world
        .block(x, y, z)
        .is_some_and(primitive_shared::types::is_foliage);
    let under = world.block(x, y - 1, z).is_some_and(is_pasture);
    here || under
}

/// Ground a herd stands on and grazes: turf, and the savanna's dry turf.
///
/// **One answer for spawning and for grazing**, because the savanna's floor
/// stopped being `BLOCK_GRASS` the day it became dry turf
/// (`types::BLOCK_DRY_TURF`) -- and three separate "is it grass" checks
/// meant a savanna whose zebra and antelope could neither arrive nor find
/// anything to eat on the one ground they are allowed to live on.
///
/// Asked as "would that ground's own grass grow here", with the tuft that
/// belongs on it, so a field of turf under a roof or on a cave ledge is
/// still refused exactly as it was.
/// Is this a floor the spawner will put `species` down on?
///
/// **Grass for everything that grazes or hunts, and the shore's own floor
/// for the shore's two** (`Species::walks_on_sand`, which carries the whole
/// argument). `is_pasture` on its own was the answer for eighteen species
/// and would have meant a world with no crabs and no monkeys in it: a beach
/// is sand, and sand grows nothing.
///
/// The shore's list is what a tideline is made of and deliberately not "any
/// solid block": an animal standing on a rock face, on a cave floor or on
/// somebody's roof is still an animal that came from nowhere, which is what
/// the pasture test was protecting.
fn spawn_ground(species: Species, under: primitive_shared::types::BlockId) -> bool {
    use primitive_shared::types::{block_kind, BLOCK_COBBLESTONE, BLOCK_GRAVEL, BLOCK_SAND};
    is_pasture(under)
        || (species.walks_on_sand()
            && matches!(block_kind(under), BLOCK_SAND | BLOCK_GRAVEL | BLOCK_COBBLESTONE))
}

/// How many of `keep_the_kept`'s calls pass between two looks for a stack by
/// an animal that wants one: forty ticks, two seconds.
///
/// **Not every tick**, because the look is six hundred cells (`MANGER_REACH`
/// round and `MANGER_RISE` up and down) and an animal with no stack in reach
/// wants one every tick from the moment it passes `STACK_AFTER_DAYS` until
/// somebody feeds it. Two seconds late to a stack is nothing to an animal
/// that has half a day before it is hungry.
const MANGER_LOOK_EVERY: u32 = 40;

/// The haystacks within reach of an animal's feet, nearest first, with the
/// hay each still holds once `taken` (bites already promised this step) is
/// counted off.
fn stacks_in_reach(
    world: &dyn BlockWorld,
    feet: (f64, f64, f64),
    taken: &std::collections::HashMap<(i32, i32, i32), u32>,
) -> Vec<((i32, i32, i32), u32)> {
    use husbandry::{MANGER_REACH, MANGER_RISE};
    let (fx, fy, fz) = (feet.0.floor() as i32, feet.1.floor() as i32, feet.2.floor() as i32);
    let mut found = Vec::new();
    for dy in -MANGER_RISE..=MANGER_RISE {
        for dz in -MANGER_REACH..=MANGER_REACH {
            for dx in -MANGER_REACH..=MANGER_REACH {
                let cell = (fx + dx, fy + dy, fz + dz);
                let Some(left) = world.block(cell.0, cell.1, cell.2).and_then(primitive_shared::types::hay_in_stack) else {
                    continue;
                };
                let left = u32::from(left).saturating_sub(taken.get(&cell).copied().unwrap_or(0));
                if left > 0 {
                    found.push((cell, left, dx * dx + dy * dy + dz * dz));
                }
            }
        }
    }
    found.sort_by_key(|&(_, _, far)| far);
    found.into_iter().map(|(cell, left, _)| (cell, left)).collect()
}

/// `eaten` bites out of `stacks`, nearest first, written down both as bites
/// promised (`taken`) and as bites for the tick loop to take out of the world.
fn take_bites(
    stacks: &[((i32, i32, i32), u32)],
    mut eaten: u32,
    taken: &mut std::collections::HashMap<(i32, i32, i32), u32>,
    bites: &mut Vec<(i32, i32, i32)>,
) {
    for &(cell, left) in stacks {
        if eaten == 0 {
            break;
        }
        let here = eaten.min(left);
        eaten -= here;
        *taken.entry(cell).or_insert(0) += here;
        bites.extend(std::iter::repeat_n(cell, here as usize));
    }
}

fn is_pasture(under: primitive_shared::types::BlockId) -> bool {
    use primitive_shared::types::{block_kind, BLOCK_DRY_GRASS, BLOCK_DRY_TURF, BLOCK_TALL_GRASS};
    match block_kind(under) {
        BLOCK_GRASS => can_grow_on(BLOCK_TALL_GRASS, under),
        BLOCK_DRY_TURF => can_grow_on(BLOCK_DRY_GRASS, under),
        _ => false,
    }
}

/// A heading toward the nearest thing worth eating, if there is one
/// within `FORAGE_RANGE`.
///
/// Eight compass points, sampled outward. Not a search of every cell in
/// the radius -- that is two hundred lookups for an animal that only has
/// to end up *facing* roughly the right way, and it thinks again in a
/// second and a half. The heading is passed through `open_heading`, so
/// the animal never sets off toward a tuft on the far side of a lake.
fn food_heading(world: &dyn BlockWorld, animal: &Animal) -> Option<f32> {
    const POINTS: usize = 8;
    let mut best: Option<(f32, f32)> = None;
    for point in 0..POINTS {
        let yaw = point as f32 / POINTS as f32 * std::f32::consts::TAU;
        let (sin, cos) = yaw.sin_cos();
        let mut distance = 2.0;
        while distance <= FORAGE_RANGE {
            let at = (
                animal.at().0 + cos * distance,
                animal.at().1,
                animal.at().2 + sin * distance,
            );
            // Only where the animal could stand, so a tuft on a ledge
            // over its head is not food.
            if let Some(y) = footing(world, at, animal.species) {
                if standing_in_food(world, (at.0, y, at.2)) {
                    match best {
                        Some((_, near)) if near <= distance => {}
                        _ => best = Some((yaw, distance)),
                    }
                    break;
                }
            }
            distance += 2.0;
        }
    }
    best.map(|(yaw, _)| open_heading(world, animal, yaw))
}

/// The nearest heading to the one wanted that the animal could actually
/// take, looking `LOOK_AHEAD` blocks down each.
///
/// Not a pathfinder and not trying to be. It answers one question --
/// "can I run that way?" -- and when the answer is no it asks again a
/// few degrees round, nearest first. That is enough for every case the
/// terrain here produces: a wall to swerve along, a ledge to turn from,
/// a pond to go round. What it deliberately will not do is find its way
/// out of a pit, because an animal that could is an animal a player
/// cannot trap, and trapping one is a legitimate way to catch dinner.
///
/// If nothing is open it returns the heading it was given: cornered is a
/// real answer, and one the player has earned.
fn open_heading(world: &dyn BlockWorld, animal: &Animal, wanted: f32) -> f32 {
    if way_is_open(world, animal, wanted) {
        return wanted;
    }
    for swerve in SWERVES {
        for side in [swerve, -swerve] {
            if way_is_open(world, animal, wanted + side) {
                return wanted + side;
            }
        }
    }
    wanted
}

/// The four sides of a cell, in the order `walls_beside` answers them.
const SIDES: [(i32, i32); 4] = [(1, 0), (0, 1), (-1, 0), (0, -1)];

/// How far a rat in the open looks for a wall to run to, in blocks.
///
/// Six: the middle of any room a player builds is within six of a wall,
/// so inside a house the answer is always found; out in a field it is
/// not, and a rat there wanders -- which is right, a field is not where
/// it lives.
const SKULK_LOOK: f32 = 6.0;

/// Which sides of the cell `at` stands in have something solid at body
/// height beside it: a wall, a chest, the leg of a bench.
fn walls_beside(world: &dyn BlockWorld, at: (f32, f32, f32)) -> [bool; 4] {
    let (x, y, z) = (at.0.floor() as i32, at.1.floor() as i32, at.2.floor() as i32);
    SIDES.map(|(dx, dz)| world.block(x + dx, y, z + dz).is_some_and(|b| stand_height(b) > 0.0))
}

/// Whether something with a box hangs over the cell a step or two up --
/// a table, a shelf, a low roof: the "under the furniture" a rat hides in.
fn roofed(world: &dyn BlockWorld, at: (f32, f32, f32)) -> bool {
    let (x, y, z) = (at.0.floor() as i32, at.1.floor() as i32, at.2.floor() as i32);
    (1..=2).any(|up| world.block(x, y + up, z).is_some_and(|b| stand_height(b) > 0.0))
}

/// A rat's day with nothing in sight: along a wall, into a corner, still.
///
/// **Steering and not a search.** It reads the four cells beside it once
/// per thought (a second or less), plus at most eight short runs of
/// `clear_run` when it is out in the open -- a few dozen block reads a
/// second for a house that holds four rats at most (`vermin::MOST_RATS`).
/// Three shapes were weighed:
///
/// * **A wall-following heading, re-aimed often** (this): with a wall
///   beside it, it runs along it, keeping the way it was going; in a
///   corner or under something it stops and stays a while; in the open
///   it makes for the nearest wall. Cheap, and it reads as a rat.
/// * **A path to the nearest dark corner** (`pathfinder`-style): the
///   right answer for "where would it go", at the price of a search per
///   rat per decision -- and the walk it produces is a beeline across
///   the floor to the corner, which is exactly the thing not wanted.
/// * **A bias on `open_heading`** toward headings with a wall beside
///   them: one line, but it only ever touches the moment of choosing,
///   and a rat that chose well then ran four seconds straight out of
///   the corner into the middle of the room anyway.
///
/// The thoughts are short (under a second and a half) for the same
/// reason the last one fails: a heading along a wall is only along it
/// until the wall ends, and a rat that is not asked again at the corner
/// runs on into the room.
fn skulk(animal: &mut Animal, world: &dyn BlockWorld, rng: &mut Rng) {
    let at = animal.at();
    let walls = walls_beside(world, at);
    let beside = walls.iter().filter(|&&w| w).count();
    // Hidden: a corner, or something over it. It stays -- most of a rat's
    // night in a house is spent not being seen -- but not for ever, or
    // the first corner it found would be the only place it ever was.
    if (beside >= 2 || (beside >= 1 && roofed(world, at))) && rng.chance(0.6) {
        animal.mind = Mind::Idle;
        animal.next_thought = rng.range(2.0, 6.0);
        return;
    }
    animal.mind = Mind::Wander;
    if beside > 0 {
        // Along the wall, the way it was already facing if that is one of
        // the two ways along it: a rat that picked afresh each second
        // would shuttle back and forth over the same block.
        let (sin, cos) = animal.yaw.sin_cos();
        let mut best: Option<(f32, f32)> = None;
        for (side, &wall) in SIDES.iter().zip(walls.iter()) {
            if !wall {
                continue;
            }
            for along in [(-side.1, side.0), (side.1, -side.0)] {
                let yaw = (along.1 as f32).atan2(along.0 as f32);
                if !way_is_open(world, animal, yaw) {
                    continue;
                }
                // Now and then it turns back: a rat pacing one wall to
                // its end and back is a rat, one circling the room for
                // ever is a clockwork toy.
                let keep = along.0 as f32 * cos + along.1 as f32 * sin + rng.range(-0.6, 0.6);
                if best.is_none_or(|(_, k)| keep > k) {
                    best = Some((yaw, keep));
                }
            }
        }
        if let Some((yaw, _)) = best {
            animal.wants_yaw = yaw;
            animal.next_thought = rng.range(0.4, 1.2);
            return;
        }
    }
    // In the open (or boxed in): make for the nearest wall it can see.
    // A run that stops short of `SKULK_LOOK` stopped at something.
    let nearest = (0..8)
        .map(|k| k as f32 * std::f32::consts::FRAC_PI_4 + animal.yaw)
        .map(|yaw| (yaw, clear_run(world, animal, yaw, SKULK_LOOK)))
        .filter(|&(_, run)| run < SKULK_LOOK)
        .min_by(|a, b| a.1.total_cmp(&b.1));
    animal.wants_yaw = match nearest {
        // Nearly there: the run is to the last open block, so go straight.
        Some((yaw, _)) => yaw,
        None => open_heading(world, animal, rng.range(0.0, std::f32::consts::TAU)),
    };
    animal.next_thought = rng.range(0.4, 1.0);
}

/// Could this animal walk `LOOK_AHEAD` blocks along that heading?
fn way_is_open(world: &dyn BlockWorld, animal: &Animal, yaw: f32) -> bool {
    let (sin, cos) = yaw.sin_cos();
    let mut at = animal.at();
    let mut left = LOOK_AHEAD;
    while left > 0.0 {
        let ahead = (at.0 + cos, at.1, at.2 + sin);
        let Some(y) = footing(world, ahead, animal.species) else {
            return false;
        };
        at = (ahead.0, y, ahead.2);
        left -= 1.0;
    }
    true
}

/// Where an animal's feet would end up one block along, or `None` if
/// there is nowhere to put them.
///
/// The same three rules `walk` enforces a tick at a time -- climb what
/// is a step high, drop what is a safe step down, do not go in the water
/// -- asked a block *before* walking into them instead of after. That is
/// the whole of the difference between an animal that looks where it is
/// going and one that finds out on arrival.
fn footing(world: &dyn BlockWorld, at: (f32, f32, f32), species: Species) -> Option<f32> {
    let highest = (at.1 + STEP_HEIGHT).floor() as i32;
    let lowest = (at.1 - SAFE_STEP_DOWN).floor() as i32;
    for y in (lowest.max(1)..=highest).rev() {
        let feet = (at.0, y as f32, at.2);
        // Something to stand on. Unloaded ground is not footing: an
        // animal must not plan a route through terrain that has not
        // arrived.
        let under = world.block(at.0.floor() as i32, y - 1, at.2.floor() as i32)?;
        if stand_height(under) <= 0.0 {
            continue;
        }
        if !fits(world, primitive_shared::geometry::wide(feet), species) {
            continue;
        }
        // Water is the far side of a wall, not a floor. Nothing here
        // swims, so a route into a lake is a route to floating about in
        // the middle of one where no player can reach it.
        if enters_liquid(world, primitive_shared::geometry::wide(feet)) {
            return None;
        }
        return Some(y as f32);
    }
    None
}

/// What fraction of its awareness a hunter breaks into a run at.
///
/// Two thirds. Beyond it a wolf walks -- a stalk, and the deer may not
/// have noticed it yet; inside it the walk is pointless, because anything
/// worth hunting has seen it by then and is already leaving.
const CHASE_FRACTION: f32 = 0.66;

/// How close a hunter has to get before it bites, in blocks.
///
/// The same reach it gores a player with. A bite is a gore with a
/// different noun.
const BITE_RANGE: f32 = GORE_RANGE;

/// How far off the line of a charge a hunter aims, in blocks, to leave
/// room for the rest of the pack.
///
/// Two, which at a wolf's width is a body and a half. Without it every
/// wolf in a pack runs down the same line at the same point and they
/// arrive as a queue -- which looks like one animal with a rendering
/// bug, and which a player can hold off in a doorway forever. The offset
/// is fixed per animal rather than rolled, so a given wolf always takes
/// the same side and the pack fans out instead of shuffling.
const PACK_SPREAD: f32 = 2.0;

/// How far a grazing animal will look for something to eat, in blocks.
///
/// Eight. Far enough that there is usually something in range on
/// grassland, near enough that the search is a handful of block lookups
/// on a path taken once a second. See `food_heading`.
const FORAGE_RANGE: f32 = 8.0;

/// How long an animal stands over food, in seconds.
///
/// The longest pause in the file, and it is the one that reads as
/// *eating* rather than as idling: an animal that stops for the same
/// second and a half everywhere is an animal wandering at random with
/// pauses in it, and one that stops for six seconds where the grass is
/// has a reason to be standing there.
const FEEDING_SECONDS: (f32, f32) = (4.0, 7.0);

/// How many of its own kind a wolf needs beside it before it will come
/// at a person.
///
/// One. Two wolves are a pack and a wolf on its own is a coward, which
/// is both true of wolves and the thing that keeps this animal from
/// making the whole map hostile: a player can *count* what is in front
/// of them and know whether it is a problem.
const PACK: usize = 1;

/// The boar and the wolf, which are the animals with a decision to make.
///
/// Four states in a ring, and every arrow between them is a distance or
/// a timer rather than a die roll:
///
/// ```text
///   nobody near ─► graze
///        ▲            │ somebody at seven metres
///        │            ▼
///     give up ◄──── watch ◄──── they came to three metres,
///        ▲            ▲          or they hit me
///        │            │              │
///     recover ◄──── charge ◄─────────┘
/// ```
///
/// `Watch` is the state that matters. A boar that charged everything it
/// could see made the wood it stood in impassable; one that stops and
/// looks at you gives the player the choice the whole animal exists to
/// offer -- leave it alone, or start something.
#[allow(clippy::too_many_arguments)]
fn think_hunter(
    animal: &mut Animal,
    nearest: Option<(PlayerId, (f32, f32, f32), f32)>,
    figure: Option<Figure>,
    fire_bearers: &[PlayerId],
    seen: &Neighbours,
    world: &dyn BlockWorld,
    rng: &mut Rng,
    time_of_day: f32,
) {
    // **Head up until something below puts it down**, the same way `graze`
    // does it and for the same reason: this function has a dozen ways out,
    // and a crouch left set in one of them is a wolf that walks home from a
    // fight still stalking.
    animal.attitude = primitive_shared::protocol::Attitude::Easy;

    // **A bear goes home.** Before anything else, a grudge included: past
    // `TERRITORY_LEASH` from its den it has driven whoever it was after off
    // its ground, and that is all a bear wanted. See `Species::keeps_territory`.
    if let (true, Some(den)) = (animal.species.keeps_territory(), animal.home) {
        let (hx, hz) = (den.0 - animal.at().0, den.1 - animal.at().2);
        if hx.hypot(hz) > TERRITORY_LEASH {
            animal.angry_for = 0.0;
            animal.target = None;
            animal.charge_at = None;
            animal.flank_to = None;
            animal.mind = Mind::Wander;
            animal.wants_yaw = open_heading(world, animal, hz.atan2(hx));
            animal.next_thought = rng.range(1.0, 2.0);
            return;
        }
    }
    let Some((id, at, distance)) = nearest else {
        animal.target = None;
        animal.flank_to = None;
        // **Nobody about: go and find dinner.**
        //
        // Checked before the fall back to grazing, because a hungry
        // predator with a deer in sight is not grazing. A charge is a
        // charge whether it is aimed at a person or at an animal, so
        // this reuses the same three states: watch, commit, recover.
        if let Some((prey, prey_at)) = seen.quarry {
            animal.quarry = Some(prey);
            let (dx, dz) = (prey_at.0 - animal.at().0, prey_at.2 - animal.at().2);
            let distance_sq = dx * dx + dz * dz;
            let toward = dz.atan2(dx);
            match animal.mind {
                // The burst is over; every charge ends in a pause.
                Mind::Charge => {
                    animal.mind = Mind::Recover;
                    animal.charge_at = None;
                    animal.next_thought = rng.range(RECOVER_SECONDS.0, RECOVER_SECONDS.1);
                    animal.wants_yaw = toward;
                }
                // Near enough to pounce.
                _ if distance_sq <= animal.species.provoke_range().powi(2) => {
                    start_charge(animal, prey_at, pack_lane(animal, seen), rng)
                }
                // **Inside the run: chase it.**
                //
                // A stalk cannot close this gap and never could -- a deer
                // that has seen a wolf leaves at six metres a second and
                // a walk is two. The first shape of this hunt walked the
                // whole way in and the wolf simply followed a fleeing
                // deer across the map for ever, never gaining a metre.
                _ if distance_sq <= (animal.species.awareness() * CHASE_FRACTION).powi(2) => {
                    animal.mind = Mind::Chase;
                    animal.wants_yaw = toward;
                    // Often, because a chase that re-aims once a second
                    // is a chase that runs where the deer *was*.
                    animal.next_thought = rng.range(0.25, 0.5);
                }
                // Further off than that: walk, and do not be seen doing
                // it. Sprinting at a deer from eighteen metres only
                // teaches the deer to leave.
                _ => {
                    animal.mind = Mind::Wander;
                    animal.wants_yaw = open_heading(world, animal, toward);
                    animal.next_thought = rng.range(0.5, 1.2);
                }
            }
            return;
        }
        animal.quarry = None;
        animal.charge_at = None;

        // **A kill left lying is dinner, and the wolves know it.**
        //
        // Checked only when there is nobody about and nothing alive to
        // chase, and only by a hungry wolf, which is where the scan is
        // affordable -- the same argument `lit_fire_near` makes. What it
        // buys is the answer to a question the carcass mechanic asked
        // and had no reply to: what happens to a deer you shot and did
        // not butcher. It gets eaten, and the thing that eats it is
        // standing over it when you come back.
        //
        // The wolf does not remove the carcass; it feeds beside it
        // (`Species::fed_seconds`) and the carcass goes off on its own
        // clock (`logic::carrion`). A wolf that ate the block would be
        // a wolf that deletes a player's kill in the time it takes them
        // to fetch a knife -- and being *followed home* by a fed pack
        // is the better story anyway.
        //
        // ---- and a dead player is meat too ----
        //
        // `types::draws_scavengers` says yes to `BLOCK_CORPSE`, so a body
        // in a wood draws whatever is hungry in it, exactly as a deer
        // does. That is the whole of the change, and everything it does
        // *not* do was chosen as carefully as what it does.
        //
        // **What happens when the wolf arrives: it eats, and it stays.**
        // Nothing else. The body is not removed, nothing comes out of it,
        // and its two-day clock is untouched. What the mechanic produces
        // is a picture at the moment of return -- you come back over the
        // ridge for your things and there is a fed wolf standing on your
        // grave -- and a choice that goes with it: go in, wait it out, or
        // come round the other side. That is a decision made with
        // everything visible, which is the shape this game wants
        // (`BLOCK_REMAINS` argues the same point at length).
        //
        // Three louder versions were weighed and all three fail the same
        // test, which is that the player cannot see them coming and so
        // cannot decide anything about them:
        //
        // * **The scavenger takes the soft half.** "I came back and my
        //   furs are gone" is a punishment, not a choice: nothing told
        //   the player it was happening, nothing they could have done
        //   would have changed it, and the loss is indistinguishable
        //   from the rot that was going to take the same things anyway.
        //   It is also a second, hidden clock beside the one the body
        //   already has.
        // * **The scavenger hurries the rot.** The same fault at one
        //   remove. Two days is the number a player plans the trip back
        //   against; a two-day clock that secretly ran in one because
        //   something walked past is a world that lies about its own
        //   rules.
        // * **The body becomes a carcass.** A corpse you can open with a
        //   knife for meat and hide is a different game, and it is the
        //   reason `is_carcass` says no to `BLOCK_CORPSE` and must go on
        //   saying no (`types::BLOCK_CORPSE`).
        //
        // The bones are not in `draws_scavengers` either: there is
        // nothing left on them to smell.
        if animal.species.scavenges() && animal.fed_for <= 0.0 {
            if let Some(carrion) = carrion_near(world, animal.at(), CARRION_RADIUS) {
                let (dx, dz) = (
                    carrion.0 - animal.at().0,
                    carrion.2 - animal.at().2,
                );
                if dx * dx + dz * dz <= FEEDING_REACH * FEEDING_REACH {
                    animal.fed_for = animal.species.fed_seconds();
                    animal.mind = Mind::Idle;
                    animal.next_thought = rng.range(1.0, 2.0);
                } else {
                    animal.mind = Mind::Wander;
                    animal.wants_yaw = open_heading(world, animal, dz.atan2(dx));
                    animal.next_thought = rng.range(0.4, 0.9);
                }
                return;
            }
        }

        // Nobody in reach. A boar mid-charge does not stop dead -- it is
        // running at a place rather than at a person, and a charge that
        // evaporates because its target stepped behind a hill is a
        // charge that never had any weight -- but *this is the thought
        // that ends the burst*, so it ends here into the same recovery
        // any other charge ends into.
        //
        // Leaving it in `Charge` was the first shape and it is a boar
        // that runs in a straight line for ever: the burst is timed by
        // `next_thought`, and a branch that returns without spending one
        // is a timer that never fires.
        if animal.mind == Mind::Charge {
            animal.mind = Mind::Recover;
            animal.next_thought = rng.range(RECOVER_SECONDS.0, RECOVER_SECONDS.1);
        } else {
            graze(animal, seen, world, rng, time_of_day);
        }
        return;
    };

    animal.target = Some(id);
    // A person in reach outranks whatever it was eating.
    animal.quarry = None;
    let (dx, dz) = (at.0 - animal.at().0, at.2 - animal.at().2);
    let facing = if dx != 0.0 || dz != 0.0 {
        dz.atan2(dx)
    } else {
        animal.yaw
    };

    // **A beaten animal runs.** Nothing alive fights to the death every
    // time, and a boar that did was the one thing in this world with no
    // way out of a fight it was losing -- which made every fight with
    // one a fight to somebody's death, and usually the player's if they
    // had opened it with a flint knife.
    //
    // Below a third it breaks off, and the grudge goes with it: an
    // animal that ran and then remembered it was angry would turn round
    // four metres later, which is not breaking off, it is a feint.
    //
    // **A wolf does not break off; it falls back.** Below half -- above
    // the third everything else runs at, because a wolf gives up on the
    // *fight* sooner and on *you* never -- it backs off to twelve blocks
    // and keeps you in sight from there. What a player sees is a wolf at
    // the edge of the torchlight that will not come and will not go,
    // which is what a wounded wolf is, and which turns "I hit it and it
    // ran" into "it is still out there". See `shadow`.
    //
    // The lion does not (`Species::falls_back_when_hurt`): it is the one
    // hostile animal that comes alone, and one that also followed you home
    // when it lost would make every fight with it a fight to the end.
    if animal.species.falls_back_when_hurt()
        && animal.health <= animal.species.health() * WOLF_SHADOWS_BELOW
    {
        shadow(animal, world, facing, distance, rng);
        return;
    }
    if animal.health <= animal.species.health() * BREAKS_OFF_BELOW {
        animal.angry_for = 0.0;
        animal.charge_at = None;
        bolt(animal, world, facing + std::f32::consts::PI, seen);
        return;
    }

    // **Fire is the answer to the night.** A wolf will not come inside
    // `FIRE_RADIUS` of a lit hearth, a standing torch, or a person
    // carrying one, after dark -- grudge or no grudge, pack or no pack.
    // It stands outside the light and watches, and if it finds itself
    // inside the circle (you walked at it with the torch) it backs out.
    // After dark only: by day a wolf is shy of *you*, not of a campfire
    // it can see is a campfire, and a rule that kept wolves off every
    // hearth at noon would make the campfire a fence rather than a
    // decision about when to be out. See `lit_fire_near`, and
    // `Animals::carrying_fire` for how a held torch is known about.
    //
    // A lion keeps the same rule (`Species::shies_from_fire`): it is the
    // savanna's night, and a fire has to be the answer to that night too.
    if animal.species.shies_from_fire() && is_night(time_of_day) {
        let fire = if fire_bearers.contains(&id) {
            Some(at)
        } else {
            lit_fire_near(world, at, FIRE_RADIUS)
        };
        if let Some(fire) = fire {
            animal.charge_at = None;
            animal.flank_to = None;
            let (fx, fz) = (fire.0 - animal.at().0, fire.2 - animal.at().2);
            let off = fx.hypot(fz);
            if off < FIRE_RADIUS + 1.0 {
                // Inside the light: out, at a walk. Not a bolt -- it
                // is not frightened of you, it is declining the fire.
                animal.mind = Mind::Wander;
                animal.wants_yaw = open_heading(world, animal, (-fz).atan2(-fx));
                animal.next_thought = rng.range(0.5, 1.0);
            } else if off > FIRE_EDGE + 1.0 {
                // **Drawn to it from the dark**, at a stalk: the fire is
                // what it saw (`FIRE_SEEN_AT_NIGHT`), and it comes to the
                // edge of the light to look. Not a chase -- nothing here
                // is going to be run down -- so it walks, low.
                animal.mind = Mind::Wander;
                animal.attitude = primitive_shared::protocol::Attitude::Stalking;
                animal.wants_yaw = open_heading(world, animal, fz.atan2(fx));
                animal.next_thought = rng.range(0.5, 1.0);
            } else if rng.chance(FIRE_EDGE_PAUSE) {
                // Stops, and looks in at you.
                animal.mind = Mind::Watch;
                animal.attitude = primitive_shared::protocol::Attitude::Alert;
                animal.wants_yaw = facing;
                animal.next_thought = rng.range(0.8, 1.6);
            } else {
                // **At the edge: round it.** The ring `FIRE_EDGE` out,
                // the way a pack walks round a person (`ring_round`), so the
                // shapes out there are moving -- which is what an eye
                // picks out of the dark, and what makes them read as
                // something waiting rather than as scenery.
                animal.mind = Mind::Wander;
                animal.attitude = primitive_shared::protocol::Attitude::Stalking;
                animal.wants_yaw = open_heading(world, animal, ring_round(animal, fire, FIRE_EDGE));
                animal.next_thought = rng.range(0.4, 0.8);
            }
            return;
        }
    }

    // **A sleeper after dark is the one opening a lone wolf takes.** Lying
    // in a bed with the eyes shut is somebody who cannot see anything
    // coming, and a wolf that will not face a standing person alone
    // (`needs_company`) does not have to face this one. Night only: by day a
    // sleeper in a meadow is a sleeper in plain view of everything, and the
    // day is when the wolves are few and fed (`spawn_weight`). The fire
    // above has already had its say, so a sleeper by a lit fire is never
    // this -- which is the whole of why a fire is worth its fuel.
    let sleeper = figure.is_some_and(|f| f.asleep) && is_night(time_of_day);

    // **Nerve.** A boar has it always: it is defending the patch of wood
    // it is standing in, and it does not care how many of it there are.
    // A wolf is doing something else entirely -- it is hunting, and
    // nothing hunts a thing its own size on its own. So a lone wolf
    // watches from six metres and follows you about, and the moment
    // there is a second one nearby they both come.
    //
    // Being hit is nerve of its own for either of them. An animal that
    // took a spear and then decided it was outnumbered would be an
    // animal you could farm.
    //
    // The lion is the boar's side of this and not the wolf's: it hunts
    // alone, and a lone lion five blocks off is a lion that comes. What
    // keeps that fair is its speed, not its nerve (`Species::needs_company`).
    //
    // **A hurt person is nerve enough on their own.** What a pack waits for
    // is somebody bleeding, and a lone wolf that finds one does not wait for
    // company either.
    let wounded = figure.is_some_and(|f| f.wounded);
    let pack = animal.species.needs_company() && seen.company >= PACK;
    let bold = !animal.species.needs_company() || pack || wounded || sleeper;
    // A wolf that is already working round you is committed: the
    // circle it runs is wider than its provoking range at the far side
    // of a chord, and a wolf that dropped the engagement every time the
    // arc carried it a block outside six stood there and watched a
    // player who had not moved. See `flank`.
    let circling = animal.flank_to.is_some();
    // **An opening.** A hunter that hunts with others comes in from
    // further than its provoking range when the person has their back to it
    // or is hurt -- see `STALK_RADIUS` for the ring it waits on and
    // `BACK_TURNED` for what counts. A boar and a bear do not stalk: they
    // defend where they stand.
    let back_turned = figure.and_then(|f| f.facing).is_some_and(|facing| {
        let (dx, dz) = (animal.at().0 - at.0, animal.at().2 - at.2);
        let length = dx.hypot(dz).max(1e-3);
        (facing.cos() * dx + facing.sin() * dz) / length < BACK_TURNED
    });
    let opening = animal.species.flanks()
        && bold
        && (back_turned || wounded || sleeper)
        && distance <= animal.species.awareness() * OPENING_FRACTION;
    // **Trespass.** A bear that has had one look at somebody on its ground
    // comes, from wherever it is. The look first (`Mind::Watch`), so there
    // is always a moment in which the thing to do is back off the way you
    // came.
    let trespass = animal.species.keeps_territory()
        && animal.mind == Mind::Watch
        && animal.target == Some(id)
        && animal.home.is_some_and(|den| (at.0 - den.0).hypot(at.2 - den.1) <= TERRITORY_RADIUS);
    let provoked = animal.angry_for > 0.0
        || opening
        || trespass
        || (bold && (circling || distance <= animal.species.provoke_range()));

    match animal.mind {
        // Mid-run: the burst is over, because `next_thought` is what
        // ends one. Whatever happens next, it has to stop first.
        Mind::Charge => {
            animal.mind = Mind::Recover;
            animal.charge_at = None;
            animal.next_thought = rng.range(RECOVER_SECONDS.0, RECOVER_SECONDS.1);
            animal.wants_yaw = facing;
        }
        // Blown, and deciding. Angry or crowded means going again.
        Mind::Recover if provoked => engage(animal, at, seen, world, rng),
        // Anything else: watch, unless there is a reason not to.
        _ if provoked => engage(animal, at, seen, world, rng),
        // **A pack that has found somebody walks round them**, on a ring out
        // of reach, waiting for an opening -- see `STALK_RADIUS`. Only a
        // pack: a lone wolf watches and follows, which is the rule a player
        // counts wolves by.
        _ if pack && animal.species.flanks() => {
            animal.target = Some(id);
            animal.flank_to = None;
            animal.mind = Mind::Wander;
            // Low, and everybody can see it. The ring has been walked since
            // `STALK_RADIUS` existed and nothing about the body said so: a
            // wolf circling a camp at nine blocks was drawn exactly as a wolf
            // trotting home, and the one warning a player got was the count
            // of wolves. See `protocol::Attitude::Stalking`.
            animal.attitude = primitive_shared::protocol::Attitude::Stalking;
            animal.wants_yaw = open_heading(world, animal, stalking(animal, at));
            // Often, for the reason the gull's circle is re-aimed every
            // tick: a ring made of long chords is a polygon.
            animal.next_thought = rng.range(0.4, 0.8);
        }
        _ => {
            animal.mind = Mind::Watch;
            animal.flank_to = None;
            animal.attitude = primitive_shared::protocol::Attitude::Alert;
            animal.next_thought = rng.range(0.6, 1.4);
            animal.wants_yaw = facing;
        }
    }
}

/// The heading that walks a ring of `STALK_RADIUS` round a person: along
/// it, bent in or out by how far off it the wolf is. Which way round is the
/// wolf's own, from its id, so a pack spreads round both sides rather than
/// filing round one.
fn stalking(animal: &Animal, at: (f32, f32, f32)) -> f32 {
    ring_round(animal, at, STALK_RADIUS)
}

/// The heading that walks a ring of `radius` round `centre`: `stalking`'s
/// ring, and the one a night hunter walks round the edge of a fire's light
/// (`FIRE_EDGE`). One function, so the two rings bend in and out the same
/// way and a wolf does not change its gait between a person and a camp.
fn ring_round(animal: &Animal, centre: (f32, f32, f32), radius: f32) -> f32 {
    let (dx, dz) = (animal.at().0 - centre.0, animal.at().2 - centre.2);
    let distance = dx.hypot(dz);
    let around = dz.atan2(dx);
    let hand = if animal.id.is_multiple_of(2) { 1.0 } else { -1.0 };
    let bend = ((distance - radius) / radius).clamp(-0.9, 0.9);
    around + hand * (std::f32::consts::FRAC_PI_2 + bend)
}

/// Comes at a person: straight in, or round the side first.
///
/// A boar, a lone wolf, a wolf that has been hit, and the *first* wolf
/// of a pack all charge (see `start_charge`). The second wolf of a pack
/// -- the one with the higher id, so the choice is stable and nobody
/// waits for anybody -- works round to the side opposite its packmate
/// before it does, one arc per thought, and commits once it is there.
/// See `flank` for what that buys.
fn engage(
    animal: &mut Animal,
    at: (f32, f32, f32),
    seen: &Neighbours,
    world: &dyn BlockWorld,
    rng: &mut Rng,
) {
    if let Some((heading, toward)) = flank(animal, at, seen) {
        animal.flank_to = Some(toward);
        animal.charge_at = None;
        animal.mind = Mind::Chase;
        animal.wants_yaw = open_heading(world, animal, heading);
        // Often, for the reason a chase re-aims often: an arc that was
        // corrected once a second would be a polygon.
        animal.next_thought = rng.range(0.25, 0.4);
        return;
    }
    start_charge(animal, at, pack_lane(animal, seen), rng);
}

/// The heading a circling wolf takes next and the bearing it is working
/// round to, or `None` if it is in position and should come in.
///
/// **So that facing one wolf puts your back to the other.** A pack that
/// arrived from one direction, however well fanned out, was a pack you
/// could back against a rock and swing at; the whole of what makes two
/// wolves worse than one is that they are not in the same place. So the
/// second wolf's target is the point on a circle of `CIRCLE_RADIUS`
/// round you that is opposite where its packmate is, and it gets there
/// by going *round* -- at most a radian of arc per thought, always the
/// short way -- rather than through you, which is what steering straight
/// at the far point would have meant.
///
/// Only the follower flanks (the lower id leads and charges straight),
/// only while the mate is in sight, and never once it has been hit: a
/// wolf that is angry comes straight, which is nerve, and a pair that
/// each tried to get behind the other would orbit you for ever, which
/// is the failure the id rule exists to prevent. The side is chosen
/// once per engagement -- see `Animal::flank_to` for the oscillation
/// that choosing it every thought produced.
fn flank(animal: &Animal, at: (f32, f32, f32), seen: &Neighbours) -> Option<(f32, f32)> {
    use std::f32::consts::PI;
    // A pair of lions works round you the way a pair of wolves does
    // (`Species::flanks`); two boars are two charges.
    if !animal.species.flanks() || animal.angry_for > 0.0 {
        return None;
    }
    let (mate, mate_at) = seen.packmate?;
    if animal.id < mate {
        return None;
    }
    let mine = (animal.at().2 - at.2).atan2(animal.at().0 - at.0);
    let opposite = animal.flank_to.unwrap_or_else(|| {
        (mate_at.2 - at.2).atan2(mate_at.0 - at.0) + PI
    });
    let delta = (opposite - mine).rem_euclid(2.0 * PI);
    let delta = if delta > PI { delta - 2.0 * PI } else { delta };
    if delta.abs() <= FLANK_TOLERANCE {
        return None;
    }
    // A short arc per thought, and a shorter one from inside the
    // circle. The chord of a long arc passes well inside the circle --
    // a radian at five blocks dips to four and a half, and a wolf still
    // turning from a random facing dips further -- and a wolf that cut
    // to three blocks was a wolf that ran through the player it was
    // meant to be going round. From inside, most of the step is spent
    // getting back out to the ring.
    let (dx, dz) = (animal.at().0 - at.0, animal.at().2 - at.2);
    let inside = (dx * dx + dz * dz).sqrt() < CIRCLE_RADIUS - 1.0;
    let arc = if inside { 0.3 } else { 0.6 };
    let bearing = mine + delta.clamp(-arc, arc);
    let (sin, cos) = bearing.sin_cos();
    let aim = (at.0 + cos * CIRCLE_RADIUS, at.2 + sin * CIRCLE_RADIUS);
    Some((
        (aim.1 - animal.at().2).atan2(aim.0 - animal.at().0),
        opposite,
    ))
}

/// A wounded wolf keeps its distance and keeps you in sight.
///
/// Three bands round `SHADOW_DISTANCE`: too near and it runs -- a short
/// run, not a bolt, because it is putting a few blocks between you and
/// not leaving; too far and it walks after you; in between it stands
/// and faces you. The grudge and any charge in progress go, so a wolf
/// at five health is a wolf that has stopped coming, and the quarry
/// goes too, because a wolf that is watching you is not eating.
///
/// What this replaces is `bolt`, which is what a beaten boar does and
/// what a beaten wolf did: it ran, was out of awareness in three
/// seconds, and the night was over. A wolf that leaves is a wolf you
/// beat; one that shadows is one you have to *deal* with -- go home,
/// light a fire, or turn and finish it while it is slow.
fn shadow(animal: &mut Animal, world: &dyn BlockWorld, facing: f32, distance: f32, rng: &mut Rng) {
    animal.angry_for = 0.0;
    animal.charge_at = None;
    animal.flank_to = None;
    animal.quarry = None;
    if distance < SHADOW_DISTANCE * 0.75 {
        animal.mind = Mind::Flee;
        animal.wants_yaw = open_heading(world, animal, facing + std::f32::consts::PI);
        animal.next_thought = 0.6;
    } else if distance > SHADOW_DISTANCE * 1.33 {
        animal.mind = Mind::Wander;
        animal.wants_yaw = open_heading(world, animal, facing);
        animal.next_thought = rng.range(0.5, 1.2);
    } else {
        animal.mind = Mind::Watch;
        animal.wants_yaw = facing;
        animal.next_thought = rng.range(0.6, 1.4);
    }
}

/// Which side of the line this animal takes when it charges.
///
/// **Zero unless it is in a pack.** Three wolves that all run down the
/// same line arrive nose to tail, which looks like one animal with a
/// rendering fault and which a player can hold off in a doorway for
/// ever; so each takes a fixed side, from its own id, and they fan out
/// across the front.
///
/// A boar gets nothing, and neither does a wolf on its own. The offset
/// is a *miss* when there is nobody else to make it worth having: the
/// gore arc is sixty degrees off the nose, and a charge aimed two metres
/// to one side of a person arrives with its flank forward. That is
/// exactly the bug this function exists to have prevented -- the first
/// version gave every charge a lane and the boar stopped being able to
/// hit anybody at all.
fn pack_lane(animal: &Animal, seen: &Neighbours) -> f32 {
    if !animal.species.is_predator() || seen.company == 0 {
        return 0.0;
    }
    match animal.id % 3 {
        0 => 0.0,
        1 => PACK_SPREAD,
        _ => -PACK_SPREAD,
    }
}

/// Commits to a run at a patch of ground.
///
/// **Past** the player rather than at them, because that is what a
/// charge is: an animal that stops exactly where you were standing has
/// aimed at you, and one that runs through the spot has aimed at the
/// spot. The overshoot is also what gives the player the half second on
/// the far side of it in which nothing at all is coming at them.
fn start_charge(animal: &mut Animal, at: (f32, f32, f32), lane: f32, rng: &mut Rng) {
    let (dx, dz) = (at.0 - animal.at().0, at.2 - animal.at().2);
    let length = (dx * dx + dz * dz).sqrt().max(0.001);
    // Sideways is the line turned a quarter, which is `(-dz, dx)`
    // normalised. See `pack_lane` for who gets a non-zero one -- and note
    // that a lone animal gets zero, because a charge aimed two metres to
    // the left of a player is a charge that misses.
    let (side_x, side_z) = (-dz / length * lane, dx / length * lane);
    // In position, or never needed one: the next engagement picks its
    // own side.
    animal.flank_to = None;
    animal.charge_at = Some((
        at.0 + dx / length * CHARGE_OVERSHOOT + side_x,
        at.2 + dz / length * CHARGE_OVERSHOOT + side_z,
    ));
    animal.wants_yaw = dz.atan2(dx);
    animal.mind = Mind::Charge;
    animal.next_thought = rng.range(CHARGE_SECONDS.0, CHARGE_SECONDS.1);
}

/// Moves an animal, one axis at a time, against the world.
///
/// Momentum rather than teleportation: what an animal *wants* is a
/// direction and a speed, and what it gets is an acceleration toward
/// that. Three things fall out of the change and all three are the boar
/// -- a charge takes a moment to wind up, it cannot turn on the spot at
/// full tilt, and it carries on past where it was aimed. The shape this
/// replaced set the velocity directly, which made a charging boar a
/// cursor that happened to have a speed limit.
///
/// Returns fall damage, if this tick's landing carried any -- the one
/// piece of this function's own business that the tick loop still needs
/// afterwards, to fold into the same hazard list fire and held breath
/// use. See `Animals::step`.
fn walk(animal: &mut Animal, world: &dyn BlockWorld, dt: f32) -> f32 {
    let mut wanted = match animal.mind {
        Mind::Idle | Mind::Watch | Mind::Recover | Mind::Drink => 0.0,
        Mind::Wander => animal.species.walk_speed(),
        // A cruise, not a bolt -- see `CRUISE_FRACTION`. It is also the
        // one moving state `think` does not count as running, so a bird
        // that has flown home still has its wind for the fright that
        // finds it there.
        Mind::Homing | Mind::Soar => animal.species.run_speed() * CRUISE_FRACTION,
        Mind::Flee | Mind::Charge | Mind::Chase => animal.species.run_speed(),
    };
    // Slowing on the final approach -- see `APPROACH_SLOWEST`. Four arrival
    // radii out it is still the cruise; over the spot, the slowest.
    if animal.mind == Mind::Homing && animal.species.flies() {
        if let Some((tx, tz)) = animal.bound_for {
            let left = (tx - animal.at().0).hypot(tz - animal.at().2);
            wanted *= (left / (HOME_REACH * 4.0)).clamp(APPROACH_SLOWEST, 1.0);
        }
    }

    // **A blown animal is still running, and it is slower.**
    //
    // The meter is counted in `think`, which is the one place that
    // decides whether this animal is running at all; what happens here
    // is the only thing that reads it. Applied to `wanted` before the
    // wade and the pace, so it is a fact about the animal's legs rather
    // than about the ground: a tired deer in a river is tired and
    // wading, and the two multiply.
    //
    // It is not applied to a walk. An animal that could not *walk*
    // properly after a chase would be an animal that limps for a minute
    // every time anything startles it, and nobody would read that as
    // tiredness -- they would read it as an animal stuck on something.
    // See `Species::stamina_seconds` for what the numbers buy and
    // `BLOWN_SPEED` for why it is three fifths and not a stop.
    if animal.stamina <= 0.0 {
        wanted *= BLOWN_SPEED;
    }

    // In the water it *wades*, and this has to be decided here rather
    // than beside the buoyancy below: what follows turns `wanted` into a
    // velocity, and a slowdown applied after that is a slowdown applied
    // to nothing -- which is what it was until the compiler pointed out
    // that the assignment was never read.
    //
    // Buoyancy alone floats an animal to the surface and leaves it
    // walking across the top of a lake at full speed, because nothing
    // horizontal is in its way. From the shore that looks exactly like
    // an animal that cannot tell water from ground.
    let wading = in_liquid(world, animal.position, animal.species);
    if wading {
        wanted *= WADE_SPEED;
    }
    // ...and this animal's own pace, so a herd is several animals
    // rather than one animal drawn four times. See `Animal::pace`.
    wanted *= animal.pace;
    // A young one's legs, and a mother keeping to them. See `keep_family`.
    wanted *= youth::speed(animal.growth);
    wanted = wanted.min(animal.speed_cap);

    // A walk bends. The drift is re-rolled at every thought and applied
    // while it is actually going somewhere -- a standing animal that
    // slowly rotated would be a weathervane.
    // **Only while wandering.** A fleeing animal that curved as well
    // read beautifully and quietly broke the hunt: the margin between a
    // wolf's run and a deer's is a designed number, and a deer that
    // spends part of its speed on turning is a deer that is never
    // caught. A run is a straight line for a reason.
    if animal.mind == Mind::Wander {
        // Wrapped for the same reason `turn_towards` wraps its own
        // result: an accumulator that only ever grows loses precision
        // once it is large enough, and this one used to grow for as
        // long as the animal kept wandering the same way.
        animal.wants_yaw = (animal.wants_yaw + animal.drift * dt).rem_euclid(std::f32::consts::TAU);
    }

    // **A soaring gull flies a circle**, re-aimed every tick rather than every
    // thought: a heading held for a second at a cruise is five blocks of
    // straight line, and a circle of ten made of five-block chords is a
    // pentagon.
    if animal.mind == Mind::Soar {
        if let Some(centre) = animal.bound_for {
            animal.wants_yaw = circling(animal, centre);
        }
    }

    // A charge steers toward the patch of ground it was aimed at, not
    // toward the player. Everything else simply goes where it is facing.
    if let (Mind::Charge, Some((tx, tz))) = (animal.mind, animal.charge_at) {
        let (dx, dz) = (tx - animal.at().0, tz - animal.at().2);
        if dx * dx + dz * dz > 0.25 {
            animal.wants_yaw = dz.atan2(dx);
        }
    }

    // Turn toward where it wants to be pointing rather than snapping
    // there. A thought lands once a second and can reverse the facing,
    // and a body that reversed with it was the "it keeps changing which
    // way round it is" -- an animal spinning on the spot between frames.
    //
    // The client draws the yaw it is sent and interpolates only
    // position, so this is the only place a turn can be made to take
    // time.
    // **How fast it comes round is a fact about the animal.** One rate
    // for everything turned a hare and a deer at the same speed, which
    // is most of what made them read as the same object in different
    // skins: a hare pivots, a deer sweeps. Small things turn faster, and
    // anything running turns faster than anything grazing.
    let agility = nimbleness(animal.species);
    let urgency = match animal.mind {
        Mind::Flee | Mind::Chase => 1.4,
        // Not slower. A charge already commits by aiming at a patch of
        // ground rather than at the target (see `charge_at`), and
        // slowing the turn on top of that stretched the wind-up far
        // enough that a wolf's pounce stopped landing at all.
        _ => 1.0,
    };
    // **A charge at speed cannot turn.** The wind-up is exempt -- the
    // animal is standing still while it comes round to face its mark,
    // and capping *that* is the mistake the comment above records --
    // but once it is doing better than half its run it is committed:
    // the mark it was aimed at is where it goes, and the mark is a
    // point on the ground three blocks past where you were. A sidestep
    // at the last moment is therefore a clean miss, and the second the
    // boar spends in `Recover` turning round is the second its back is
    // to you (see `Animal::exposed_back`). The speed test rather than a
    // flag, because speed is the physical fact: a boar that has hit a
    // wall and stopped can turn, whatever state it is in.
    let speed_sq = animal.velocity.0 * animal.velocity.0 + animal.velocity.2 * animal.velocity.2;
    let committed_at = animal.species.run_speed() * animal.pace * COMMITTED_FRACTION;
    let committed = animal.mind == Mind::Charge && speed_sq > committed_at * committed_at;
    let most = if committed {
        CHARGE_TURN_RATE
    } else {
        TURN_RATE * agility * urgency
    };
    // **A bird in the air banks.** See `AIR_TURN_ACCEL` for what the
    // drone was: two hundred degrees a second of instant turn, which on the
    // ground is a hare pivoting and in the air is a thing with rotors.
    let airborne_bird = animal.species.flies() && !animal.on_ground && !wading;
    // **A bird does not fly a ruler line.** See `WEAVE`: a wander on the
    // heading it *wants*, so what comes of it is the ordinary banked turn and
    // not a body sliding sideways -- and faded out over the last
    // `WEAVE_FADES_AT` blocks of an approach, because a bird that weaved
    // while it was landing would never find the branch. A circle
    // (`Mind::Soar`) already bends every tick and is left alone.
    let aim = if airborne_bird && matches!(animal.mind, Mind::Homing | Mind::Flee) {
        let close = match animal.bound_for {
            Some((tx, tz)) => ((tx - animal.at().0).hypot(tz - animal.at().2) / WEAVE_FADES_AT).min(1.0),
            None => 1.0,
        };
        let turns = animal.air_phase / AIR_CYCLE * WEAVES_PER_CYCLE * std::f32::consts::TAU;
        animal.wants_yaw + WEAVE * close * turns.sin()
    } else {
        animal.wants_yaw
    };
    if airborne_bird {
        let speed = animal.velocity.0.hypot(animal.velocity.2);
        let rate_cap = (AIR_TURN_ACCEL / speed.max(MIN_AIRSPEED)).min(AIR_TURN_MOST);
        let delta = (aim - animal.yaw).rem_euclid(std::f32::consts::TAU);
        let delta = if delta > std::f32::consts::PI { delta - std::f32::consts::TAU } else { delta };
        let desired = (delta * AIR_TURN_GAIN).clamp(-rate_cap, rate_cap);
        let roll = AIR_ROLL * dt;
        animal.turn_rate = (animal.turn_rate + (desired - animal.turn_rate).clamp(-roll, roll)).clamp(-rate_cap, rate_cap);
        animal.yaw = (animal.yaw + animal.turn_rate * dt).rem_euclid(std::f32::consts::TAU);
    } else {
        animal.turn_rate = 0.0;
        animal.yaw = turn_towards(animal.yaw, animal.wants_yaw, most * dt);
        // **A body cannot be turned and driven at once.**
        //
        // Four legs push along the body's own length, so a body pointing
        // across the line it is already travelling on has to spend its grip
        // redirecting what it has before any of it goes into going faster.
        // Nothing here said so, and what that looked like was an animal at a
        // full run changing direction without losing a block an hour -- a
        // thing steered rather than a thing running. A deer that has to slow
        // to come round a rock is a deer a player can cut the corner on,
        // which is what a chase through a wood is about.
        //
        // **Charged on the angle between the nose and the momentum**, not on
        // how fast the yaw is moving. That distinction is the whole of
        // whether this behaves: a rate charge bills an animal for holding a
        // steady curve, so anything swerving down a corridor paid a third of
        // its speed every tick for as long as it was in the corridor and
        // never got it back. This one is self-clearing -- the velocity swings
        // round to the new heading over the next few ticks and the charge
        // falls away with it -- and it is the physical statement besides.
        //
        // A quarter turn is the whole cost; anything stood still pays
        // nothing, because it has no momentum to redirect.
        //
        // **Only what is running.** A walk is slow enough that the grip is
        // never the limit -- a deer ambling to a river is not at the edge of
        // what its feet can do, and billing it a third of a walking pace for
        // every bend in its wander is a deer that takes half again as long to
        // cross a meadow for a reason nobody can see. It is also, measured,
        // the difference between a suite that passes and one that does not:
        // `a_thirsty_deer_walks_to_the_lake_and_not_into_it` and the herd's
        // own follow test are both about an animal getting somewhere at a
        // walk in the time a test gives it.
        let speed = animal.velocity.0.hypot(animal.velocity.2);
        let running = matches!(animal.mind, Mind::Flee | Mind::Chase | Mind::Charge);
        if running && speed > STANDING_STILL {
            let heading = animal.velocity.2.atan2(animal.velocity.0);
            let off = (animal.yaw - heading).rem_euclid(std::f32::consts::TAU);
            let off = off.min(std::f32::consts::TAU - off);
            wanted *= 1.0 - TURN_COST * (off / std::f32::consts::FRAC_PI_2).clamp(0.0, 1.0);
        }
    }

    // **A charge that has run through its mark is over.** The timer in
    // `CHARGE_SECONDS` is the upper bound -- a mark off a cliff or in a
    // lake is one the animal never reaches -- but the mark itself is
    // the end of the run: three blocks past where you stood, and then
    // it digs its hooves in and comes round. This has to be here, and
    // not left to the timer, *because* of the turn cap above. Uncapped,
    // an animal past its mark turned back toward it and ended the burst
    // circling the spot; capped, it kept straight on for whatever was
    // left of the timer -- a boar provoked at three blocks ran seven
    // past you, and a pack's second pass carried both wolves out of
    // their own provoking range, where they stood and watched a player
    // who had not moved. Judged by whether the mark is behind the nose,
    // and only while committed: during the wind-up the mark is often
    // behind an animal that has not yet turned to face it.
    //
    // The velocity is cut rather than zeroed. Zero is the "cursor" a
    // charge was rebuilt not to be; two fifths reads as an animal
    // braking, and stops it about a block on -- which keeps a wolf's
    // overshoot inside the range it re-engages from.
    if committed {
        if let Some((tx, tz)) = animal.charge_at {
            let (dx, dz) = (tx - animal.at().0, tz - animal.at().2);
            let (sin, cos) = animal.yaw.sin_cos();
            if dx * cos + dz * sin < 0.0 {
                animal.mind = Mind::Recover;
                animal.charge_at = None;
                animal.next_thought = RECOVER_SECONDS.0;
                animal.velocity.0 *= 0.4;
                animal.velocity.2 *= 0.4;
                wanted = 0.0;
            }
        }
    }

    // **A charge winds up.** Until it is roughly facing where it is
    // going, a charging animal turns on the spot instead of running.
    //
    // Without this it accelerates while it is still coming round, which
    // is a car and not a boar: it curves past its target sideways,
    // arrives with its flank forward, and -- because a boar gores with
    // the end that has tusks on it -- cannot land the blow the whole
    // charge was for. It also gives the player the half second of
    // warning that makes a charge fair rather than merely fast.
    if animal.mind == Mind::Charge {
        let off = (animal.wants_yaw - animal.yaw).abs();
        let off = off.min(std::f32::consts::TAU - off);
        if off > CHARGE_AIM {
            wanted = 0.0;
        }
    }

    let (sin, cos) = animal.yaw.sin_cos();
    let (want_x, want_z) = (cos * wanted, sin * wanted);
    let wanted_speed = wanted;
    // Only while there is ground under them, so nothing steers in
    // mid-air.
    // **A gull grips the air only while it is flying somewhere.** In the
    // last block of a landing it has nowhere to be, and at `GLIDE_GRIP` it
    // braked to a standstill in a third of a second and came down with its
    // wings folded -- the client opens them on horizontal speed
    // (`animal_model::AIRBORNE_SPEED`). At `AIR_CONTROL` it carries on across
    // on what it had, and the wings stay open until its feet are down.
    let flying_somewhere = matches!(animal.mind, Mind::Flee | Mind::Homing | Mind::Soar);
    let grip = if animal.on_ground {
        1.0
    } else if animal.species.soars() && flying_somewhere {
        GLIDE_GRIP
    } else {
        AIR_CONTROL
    };
    // **A bear does not start like a hare.** One acceleration for every
    // species meant the heaviest animal in the world reaching its run in the
    // same third of a second a hare does, and stopping in it: what a player
    // saw was a heavy animal that moved like a light one, and a charge with
    // no weight behind it. The same number that decides how fast a body comes
    // round decides how fast it gets going, because both are one fact about
    // how much of it there is to move -- and it gives the stopping distance
    // for nothing, since the approach to a wanted speed of zero is the same
    // exponential run backwards.
    //
    // **Four legs only.** A bird's thrust is its wings, and they have their
    // own numbers already (`WING_ACCEL` in the air, `TAKE_OFF_ACCEL` off the
    // ground) which were tuned against this constant as it stood; a gull is
    // narrow, so `nimbleness` reads it as light and nearly doubled the punch
    // of a take-off run, which put the bird into the air on a different arc
    // and out of the reach of everything that was measured against the old
    // one. Width is a fact about a body that walks.
    let heft = if animal.species.flies() { 1.0 } else { agility };
    let rate = (ACCELERATION * heft * grip * dt).min(1.0);
    if airborne_bird {
        // Left for the wings: see the flight below, which knows how high it is.
    } else if animal.species.flies() {
        // **A take-off is a run and a lift, not a launch.** On its feet a
        // bird accelerates as anything does, but never faster than
        // `TAKE_OFF_ACCEL`: the exponential grip reached a grouse's full
        // speed in a fifth of a second from a standstill, which is the
        // punch of a drone's throttle.
        let most = TAKE_OFF_ACCEL * dt;
        let (dx, dz) = ((want_x - animal.velocity.0) * rate, (want_z - animal.velocity.2) * rate);
        let length = dx.hypot(dz);
        let scale = if length > most { most / length } else { 1.0 };
        animal.velocity.0 += dx * scale;
        animal.velocity.2 += dz * scale;
    } else {
        animal.velocity.0 += (want_x - animal.velocity.0) * rate;
        animal.velocity.2 += (want_z - animal.velocity.2) * rate;
    }

    // **An animal that has decided to stand still stands still.**
    //
    // The decay above is exponential, so it never actually reaches zero:
    // an idling deer drifts at a hundredth of a block a second for ever,
    // which is invisible on its own and is exactly what made the legs
    // fidget -- the client swings them from distance covered, and an
    // animal that never stops covering distance never stops walking.
    if wanted == 0.0 && !airborne_bird {
        let creep = animal.velocity.0 * animal.velocity.0 + animal.velocity.2 * animal.velocity.2;
        if creep < STANDING_STILL * STANDING_STILL {
            animal.velocity.0 = 0.0;
            animal.velocity.2 = 0.0;
        }
    }

    // Water holds an animal up rather than swallowing it. None of the
    // three swims, and none of them should sink to the bed of a lake and
    // pace about down there either -- so a body in water floats, slowly,
    // until it is at the surface and can walk out.
    // **A bird in the air.** The one animal in this world that leaves
    // the ground, and it leaves it for exactly as long as it has a
    // reason to be up: `Mind::Flee` -- something frightened it -- or
    // `Mind::Homing`, which is the same altitude with a destination
    // instead of a fright (see `homing`). Both come down on the timers
    // every other animal already uses; nothing here hovers.
    //
    // Two states rather than one is the whole of what turned a bird from
    // a thing that jumps when poked into a thing that lives somewhere.
    //
    // Written as a *target altitude* rather than a jump and a glide:
    // what a flushed grouse does is get three or four blocks up, hold
    // it, and go -- and an altitude the flier is pulled toward gives
    // that in one line, without a second physics mode to keep in step
    // with the walking one. Everything else about it -- the collision,
    // the turning, the speed -- is the same code every animal uses.
    // **A bird on its feet is not half way through a glide.** The cycle
    // clock is reset while it is standing, so the next take-off begins at
    // the bottom of a climb rather than wherever the last flight left off --
    // a gull that leapt off the sand already sinking is a gull that falls
    // back onto it. See `Animal::air_phase`.
    if animal.on_ground {
        animal.air_phase = 0.0;
    }
    let ground_below = animal.species.flies().then(|| {
        // The ground under it and, while it is travelling, the ground it is
        // about to be over: see `flight_floor` for the canopy this stops it
        // flying into.
        //
        // **A column it cannot read is not a reason to fall.** This used to
        // answer "where it is", which made a bird with no reason to be up
        // exactly at ground level -- not flying -- and handed it to gravity:
        // twenty-two blocks a second squared from the height of a crown,
        // whenever the ground under it was out of the scan's reach or not
        // loaded. A cruise under where it is holds a flight level and lets a
        // bird with nowhere to be glide down.
        let cruise = if animal.species.soars() { SOAR_HEIGHT } else { FLIGHT_HEIGHT };
        flight_floor(world, animal).unwrap_or((animal.at().1 - cruise, 0.0))
    });
    // Airborne while it is frightened, and **still flying while it comes
    // down**: the target drops to the ground when the fright passes and
    // the bird descends under the same spring. Switching gravity back on
    // instead was the first shape and it killed the bird -- three and a
    // half blocks of fall on three points of health -- which is a bird
    // that dies of having been startled.
    let aloft = matches!(animal.mind, Mind::Flee | Mind::Homing | Mind::Soar);
    let flying = ground_below.is_some_and(|(ground, _)| aloft || animal.at().1 > ground + 0.2);
    if let (true, Some((ground, slope))) = (flying, ground_below) {
        // A journey is flown over the higher of what is under the bird
        // and what it is going to land on -- see `Animal::flight_ceiling`
        // for the bird that arrived under its own tree every time. A
        // fright has no destination and simply clears what is below it.
        let floor = if animal.mind == Mind::Homing {
            ground.max(animal.flight_ceiling)
        } else {
            ground
        };
        let cruise = if animal.species.soars() { SOAR_HEIGHT } else { FLIGHT_HEIGHT };
        let diving = aloft && animal.dive_for > 0.0;
        // **The glide path.** A homing bird holds less height the less way
        // it has left, reaching what it lands on as it reaches the spot --
        // see `GLIDE_SLOPE`. Anything else aloft cruises.
        let glide = match (animal.mind, animal.bound_for) {
            (Mind::Homing, Some((tx, tz))) => {
                let left = (tx - animal.at().0).hypot(tz - animal.at().2);
                (left - HOME_REACH).max(0.0) * GLIDE_SLOPE
            }
            _ => f32::INFINITY,
        };
        // **The climb and the glide, and what the hill gives.** See
        // `AIR_CYCLE`: the height a bird holds breathes about its cruise, so
        // it is always either going up or coming down, and it is handed the
        // rise in the ground ahead for nothing. The clock runs only while it
        // is up here, which is what makes a take-off the start of a climb.
        //
        // The cycle is kept off a bird that is coming in to land -- `glide`
        // is finite only then -- because a landing approach that bobbed
        // would be a bird that could not find the branch.
        animal.air_phase = (animal.air_phase + dt).rem_euclid(AIR_CYCLE);
        let ride = SLOPE_LIFT * slope
            + if glide.is_finite() {
                0.0
            } else {
                CYCLE_LIFT * (animal.air_phase / AIR_CYCLE * std::f32::consts::TAU).sin()
            };
        let (wanted, landing_on) = if diving {
            (floor + DIVE_SKIM, floor)
        } else if aloft {
            (floor + cruise.min(glide) + ride, floor)
        } else {
            (ground, ground)
        };
        // A spring toward the height, clamped: fast enough to clear a
        // fence in the first second, slow enough that a bird does not
        // shoot into the sky like a thrown block. A dive is let fall faster.
        //
        // **Up and down are clamped apart.** Up: a fright at `FLIGHT_SPEED`,
        // anything calmer at `CALM_CLIMB`. Down: never steeper than the bird
        // is flying across (`STEEPEST_GLIDE`), and through the last block at
        // `LANDING_SINK` -- one limit for both ways was the gull that fell
        // nine blocks onto the sand. See the section above `GLIDE_SLOPE`.
        let height = animal.at().1 - landing_on;
        // **The wings, along the nose.** In the air a bird goes where it
        // points and changes speed by flapping, at `WING_ACCEL`: no sideways
        // slide, no stop in mid-air. Above the flare it never drops under
        // `MIN_AIRSPEED` -- a bird that has lost its reason to be up glides
        // down on a line rather than being lowered on a string -- and
        // through the flare it bleeds off toward whatever its mind wants,
        // which on a landing is nothing.
        if airborne_bird {
            let speed = animal.velocity.0.hypot(animal.velocity.2);
            // **Height is speed, and it is traded both ways**: see
            // `SPEED_PER_SINK`. A sinking bird is going faster than its mind
            // asked for and a climbing one slower, which is why the same
            // bird crossing the same bay is never at the same speed twice.
            // Not through the flare, where what the wings are doing is
            // stopping.
            let traded = -animal.velocity.1 * SPEED_PER_SINK;
            let target = if height > FLARE_HEIGHT { (wanted_speed + traded).max(MIN_AIRSPEED) } else { wanted_speed };
            let speed = (speed + (target - speed).clamp(-WING_ACCEL * dt, WING_ACCEL * dt)).max(0.0);
            let (sin, cos) = animal.yaw.sin_cos();
            // Toward that, by no more than a wing and a bank can pull in a
            // tick: a bird that left the ground running one way and pointing
            // another swings onto its heading rather than snapping onto it.
            let (dx, dz) = (cos * speed - animal.velocity.0, sin * speed - animal.velocity.2);
            let length = dx.hypot(dz);
            let most = (WING_ACCEL + AIR_TURN_ACCEL) * dt;
            let scale = if length > most { most / length } else { 1.0 };
            animal.velocity.0 += dx * scale;
            animal.velocity.2 += dz * scale;
        }
        let across = animal.velocity.0.hypot(animal.velocity.2);
        let (rise, sink) = if diving {
            (DIVE_SPEED, DIVE_SPEED)
        } else {
            let rise = if animal.mind == Mind::Flee { FLIGHT_SPEED } else { CALM_CLIMB };
            // **Lift comes from going.** A wing climbs in proportion to the
            // air going over it; a bird that rose at seven blocks a second
            // from a standstill went straight up like a lift.
            // ...and the air going up a hillside is a climb the wings do not
            // have to pay for, which is what tops a ridge with room. See
            // `SLOPE_CLIMB`. Held under `FLIGHT_SPEED`, which is the fastest
            // anything in this world goes up.
            let rise = (rise.min(CLIMB_BASE + across * CLIMB_PER_SPEED) + slope * SLOPE_CLIMB).min(FLIGHT_SPEED);
            let sink = if height < FLARE_HEIGHT {
                LANDING_SINK
            } else {
                // Never steeper than it flies across, and never faster than it
                // could still slow to a touch-down by the flare: a stopping
                // distance, at seven tenths of what the wings can take off, so
                // the last block is `LANDING_SINK` without a jolt.
                let stopping = (LANDING_SINK * LANDING_SINK + 2.0 * LIFT_ACCEL * 0.7 * (height - FLARE_HEIGHT)).sqrt();
                (across * STEEPEST_GLIDE).clamp(LANDING_SINK, FLIGHT_SPEED).min(stopping)
            };
            (rise, sink)
        };
        let target = ((wanted - animal.at().1) * FLIGHT_CLIMB).clamp(-sink, rise);
        // **And the climb and the sink change at a rate a body can.** The
        // spring used to *be* the vertical speed, so a bird went from
        // climbing at seven to sinking at seven between one tick and the
        // next. A plunge is allowed its own, much harder, pull.
        let pull = if diving { DIVE_ACCEL } else { LIFT_ACCEL } * dt;
        animal.velocity.1 += (target - animal.velocity.1).clamp(-pull, pull);
        // ...except that nothing may keep sinking faster than the flare allows
        // once it is in it: a limit it is already past is a limit it obeys now.
        if !diving && height < FLARE_HEIGHT {
            animal.velocity.1 = animal.velocity.1.max(-LANDING_SINK);
        }
    } else if animal.species.flies() && !animal.on_ground && !wading {
        // **The last fifth of a block, or a step down, under its wings.** A
        // bird below the spring's reach but not yet standing used to be
        // handed to gravity, and for the ticks until its feet met something
        // it fell -- which on a ledge or the lip of a roof was a bird dropping
        // like a stone off the edge it had been standing on.
        let pull = LIFT_ACCEL * dt;
        animal.velocity.1 += (-LANDING_SINK - animal.velocity.1).clamp(-pull, pull);
    } else if wading {
        animal.velocity.1 = (animal.velocity.1 + BUOYANCY * dt).min(SWIM_RISE);
    } else if let Some(up) = climbing(animal, world) {
        // **A monkey holding on to a tree.** See `climbing` for the whole
        // of it; what happens here is only that the timber replaces gravity
        // while a hand is on it, which is what a climb is.
        animal.velocity.1 = up;
    } else {
        animal.velocity.1 = (animal.velocity.1 + GRAVITY * dt).max(TERMINAL_VELOCITY);
    }

    let step = (
        animal.velocity.0 * dt,
        animal.velocity.1 * dt,
        animal.velocity.2 * dt,
    );

    // **Read before the vertical move, and used by everything below.**
    //
    // The vertical move clears `on_ground` the instant the animal leaves
    // the floor, and a scramble up a step leaves the floor by
    // definition -- so a climb gated on the *current* flag made its own
    // second tick impossible. The animal rose a fifth of a block, fell
    // back, and did it again for as long as anybody watched.
    //
    // That is a deer penned in by a kerb, and it is why the world read
    // as a set of corridors. The client's own physics has had this
    // exactly right since step-up was added to it -- see
    // `physics::Player::update` and its `was_grounded` -- and this is
    // the same fix on the other side of the wire.
    let was_on_ground = animal.on_ground;

    // Vertical first, so an animal that has just walked off a ledge is
    // falling rather than hovering into the next horizontal test.
    let try_y = (
        animal.position.0,
        animal.position.1 + f64::from(step.1),
        animal.position.2,
    );
    if fits(world, try_y, animal.frame()) {
        animal.position = try_y;
        animal.on_ground = false;
    } else {
        if animal.velocity.1 < 0.0 {
            animal.on_ground = true;
            // Settle exactly on top of whatever stopped it, rather than
            // a fraction of a block inside it -- a herd that sinks into
            // the ground over a few minutes is what happens without
            // this.
            //
            // **This used to be `animal.at().1.floor()`, which is
            // the right answer only when the thing that stopped the fall
            // fills its whole cell.** A full block's top always sits on
            // a whole number, so flooring the pre-collision height and
            // the true landing height agree by coincidence. A campfire's
            // top does not -- it is a quarter of a block up -- and
            // flooring drove the animal's feet straight through it into
            // the block itself. Once embedded, the climb in `walk` (which
            // rises *in place* to clear whatever is ahead) had nothing
            // open to rise into, because the cell it was standing in was
            // now the obstruction: every further step failed the same
            // `fits` check landing had just failed, and the animal stood
            // there, shoved to a dead stop, for good. See
            // `resting_height` and
            // `an_animal_can_walk_over_a_campfire_the_way_a_player_can`.
            animal.position.1 = resting_height(world, animal.position, try_y.1, animal.frame());
        }
        animal.velocity.1 = 0.0;
    }

    // **A fall is judged the instant the ground catches it**, the same
    // moment `Vitals::on_transform` judges a player's landing.
    // `fall_peak_y` is that function's `fall_peak_y` again: the highest
    // point reached since the animal was last standing on something,
    // `None` while it still is. Landing turns the gap between that peak
    // and where it came down into a distance, and asks
    // `survival::fall_damage` -- the player's own formula, not a second
    // copy of it -- the same question a player's landing does. Zero
    // kilograms throughout: nothing here carries a pack.
    let mut fall_damage = 0.0f32;
    if flying {
        // **A bird coming down under its own wings is not falling.**
        //
        // The descent from `FLIGHT_HEIGHT` is a spring, not gravity --
        // it is capped at `FLIGHT_SPEED` and it is the animal's own
        // decision -- but the bookkeeping below could not tell the two
        // apart: it measured the drop from the highest point since the
        // feet last touched anything and charged the player's own fall
        // formula for it. Three and a half blocks is on the edge of what
        // that formula forgives, so a bird that had merely been
        // *startled* landed hurt, and one that had flown over a wood --
        // which is a longer way down the other side -- landed at half
        // health and died of the third fright of its life. Nobody would
        // read that as a fall; they would read it as birds dying for no
        // reason.
        //
        // Cleared rather than skipped, so the tick the bird actually
        // touches down has no peak on record and is a landing rather
        // than an arrival.
        animal.fall_peak_y = None;
    } else if !animal.on_ground {
        animal.fall_peak_y = Some(match animal.fall_peak_y {
            Some(peak) => peak.max(animal.at().1),
            None => animal.at().1,
        });
    } else if let Some(peak) = animal.fall_peak_y.take() {
        let landed_in_liquid = in_liquid(world, animal.position, animal.species);
        fall_damage =
            crate::logic::survival::fall_damage(peak - animal.at().1, landed_in_liquid, 0.0);
    }

    if animal.velocity.0 == 0.0 && animal.velocity.2 == 0.0 {
        return fall_damage;
    }

    // **A charging animal commits; everything else looks where it is
    // putting its feet.** A boar that runs off a cliff after you is
    // characterful, and is also the player's doing; a deer that grazes
    // its way over one is a deer nobody put there.
    //
    // A *chase* is careful, which is the one place the two hunting states
    // differ besides steering: a wolf that ran off every ledge its dinner
    // jumped down would be a wolf that dies of the ecosystem.
    // Same flag, same reason: an animal a fifth of the way up a step is
    // not airborne in any sense a cliff check cares about, and treating
    // it as such is how a scrambling deer stops minding the drop behind
    // it.
    //
    // A bird setting off on a flight is exempt too, and for a plainer
    // reason: it is about to be three blocks up. The first tick of a
    // take-off is still on the ground, so without this a bird whose nest
    // is across a pond stops at the water's edge and never leaves.
    let careful = was_on_ground && !matches!(animal.mind, Mind::Charge | Mind::Homing | Mind::Soar);

    for (dx, dz) in [(step.0, 0.0), (0.0, step.2)] {
        if dx == 0.0 && dz == 0.0 {
            continue;
        }
        let want = (
            animal.position.0 + f64::from(dx),
            animal.position.1,
            animal.position.2 + f64::from(dz),
        );
        if fits(world, want, animal.frame()) {
            // **It does not walk into the water.**
            //
            // Nothing here swims, and an animal that strolls into a lake
            // is an animal that floats out into the middle of it and
            // paddles about where no player can reach it. So water is
            // treated the way a cliff is: something to stop at.
            //
            // A charging boar goes in anyway, for the same reason it
            // goes off a cliff -- it has committed, and what happens
            // next is the player's doing.
            if careful && !wading && enters_liquid(world, want) {
                steer_around(animal, world);
                continue;
            }
            if careful && drop_below(world, want) > SAFE_STEP_DOWN {
                // A ledge: turn along it rather than stand on the lip of
                // it. `steer_around` keeps the heading when there is
                // nothing better, so an animal cornered against a cliff
                // still stops -- what it will not do any more is stand
                // there twitching at the drop for as long as it takes to
                // have another idea.
                steer_around(animal, world);
                continue;
            }
            animal.position = want;
            continue;
        }
        // Blocked. Climb, which is what makes terrain walkable: the
        // world is full of single-block benches, and anything that has
        // to path around them spends its life walking into walls.
        //
        // A little at a time, and *without* the horizontal move: the
        // animal rises where it stands until the way ahead is clear and
        // then walks on, which is a scramble. Taking the whole block in
        // one tick -- which is what this did -- is a teleport, and it is
        // the thing that makes an animal look like it is skipping over
        // the ground rather than walking on it.
        // **Or wading, which is the other kind of not being in mid-air.**
        //
        // The `was_on_ground` gate is there to stop an animal climbing
        // the air while it falls, and an animal in water is not falling:
        // buoyancy is holding it at the surface. Without this clause the
        // bank of any pond whose water sits below the ground beside it
        // was a two-block wall to anything floating in it -- the animal
        // pressed against it, failed `fits`, failed the climb for want
        // of a flag, and was steered along the shore instead of up it.
        // That is the second half of getting out of the water, and the
        // first half (`shore_heading`, in `think`) is worth nothing
        // without it: an animal that knows exactly where the bank is and
        // cannot climb it is a worse sight than one that never knew.
        let stepped = (want.0, want.1 + f64::from(STEP_HEIGHT), want.2);
        if (was_on_ground || wading) && fits(world, stepped, animal.frame()) {
            let rise = (CLIMB_SPEED * dt).min(STEP_HEIGHT);
            let climbing = (animal.position.0, animal.position.1 + f64::from(rise), animal.position.2);
            if fits(world, climbing, animal.frame()) {
                animal.position = climbing;
                animal.velocity.1 = animal.velocity.1.max(0.0);
                // **Still on the ground.** An animal scrambling up a
                // step is in contact with the step -- it is not falling
                // and it is not jumping. Saying so is what lets the
                // climb carry on to the next tick, and it is also the
                // truth about what the animal is doing.
                animal.on_ground = true;
                continue;
            }
        }
        // Ran into something. A charge that hits a wall is over -- the
        // recovery is the animal shaking its head, and without it a boar
        // grinds against the rock behind you for as long as you stand
        // there.
        if animal.mind == Mind::Charge {
            animal.velocity.0 = 0.0;
            animal.velocity.2 = 0.0;
            animal.mind = Mind::Recover;
            animal.charge_at = None;
            animal.next_thought = RECOVER_SECONDS.0;
        } else {
            steer_around(animal, world);
        }
    }
    fall_damage
}

/// A number in 0..1 from an id and a salt.
///
/// What gives each animal its own pace and its own bend without touching
/// the decision stream -- see `Animal::pace`.
fn spread(id: EntityId, salt: u64) -> f32 {
    let mut h = id.wrapping_mul(0x9E37_79B9_7F4A_7C15) ^ salt;
    h ^= h >> 29;
    h = h.wrapping_mul(0xBF58_476D_1CE4_E5B9);
    h ^= h >> 32;
    (h % 10_000) as f32 / 10_000.0
}

/// An animal that has just walked into something gets to think about it
/// soon, rather than at whatever time it had already scheduled.
///
/// Only ever brings a thought *forward*. Pushing one back would let a
/// bumpy hillside postpone a deer's next decision indefinitely, one
/// scrape at a time.
fn rethink(animal: &mut Animal) {
    animal.next_thought = animal.next_thought.min(BLOCKED_RETHINK);
}

/// An animal that has run into something turns to where it *can* go,
/// now, rather than standing there until its next thought.
///
/// **This is the difference between an animal and a thing that walks
/// into walls.** Stopping and rescheduling a thought was the whole of
/// the old answer: what a player saw was a deer with its nose against a
/// rock, twitching, for up to half a second at a time -- and then
/// choosing a heading at random, which is as likely to be into the rock
/// again. Now it looks for a way past before it has stopped: the same
/// `open_heading` a bolt uses, from where it is standing, so it slides
/// along the obstacle instead of pounding on it.
///
/// The heading it wants is kept if nothing better is found, so an animal
/// in a dead end still stops -- and being stopped is then a fact about
/// the terrain rather than about the code.
fn steer_around(animal: &mut Animal, world: &dyn BlockWorld) {
    // A bird in the air that meets a trunk is turned, not stopped dead: a
    // flier that halted in mid-air against a branch and then accelerated off
    // at right angles was the other half of the drone.
    let airborne_bird = animal.species.flies() && !animal.on_ground;
    if !airborne_bird {
        animal.velocity.0 = 0.0;
        animal.velocity.2 = 0.0;
    }
    // **A bird goes over what stopped it, and picks its way round by the
    // air rather than by the ground.** Both halves are the same bug: a
    // flier steered with `open_heading` is steered by whether it could
    // *walk* three blocks along a heading, which over a crown says yes and
    // over a lake says no -- so a grouse blocked by leaves was turned into
    // the wood and a gull blocked by a rock was turned inland. See
    // `air_is_clear` and `BUMP_CLIMB`.
    if airborne_bird {
        animal.velocity.1 = animal.velocity.1.max(BUMP_CLIMB);
        let open = air_heading(world, animal, animal.wants_yaw);
        if open != animal.wants_yaw {
            animal.wants_yaw = open;
            animal.next_thought = animal.next_thought.min(BLOCKED_RETHINK * 0.25);
            return;
        }
        rethink(animal);
        return;
    }
    let open = open_heading(world, animal, animal.wants_yaw);
    if open != animal.wants_yaw {
        animal.wants_yaw = open;
        // Its next step is the new way, rather than a thought away.
        animal.next_thought = animal.next_thought.min(BLOCKED_RETHINK * 0.25);
        return;
    }
    rethink(animal);
}

/// How far the ground falls away from a cell an animal is about to step
/// into, in blocks.
///
/// Bounded, because "there is no bottom" and "it is twenty deep" are the
/// same answer to the only question being asked.
fn drop_below(world: &dyn BlockWorld, at: (f64, f64, f64)) -> f32 {
    let (x, z) = (at.0.floor() as i32, at.2.floor() as i32);
    let foot = at.1.floor() as i32;
    for step in 1..=(SAFE_STEP_DOWN as i32 + 2) {
        let y = foot - step;
        if y < 0 {
            break;
        }
        match world.block(x, y, z) {
            // Unloaded ground reads as solid: an animal must not walk
            // off the edge of the world because the chunk beyond it has
            // not arrived yet.
            None => return 0.0,
            Some(block) if stand_height(block) > 0.0 => {
                return (step - 1) as f32;
            }
            Some(_) => {}
        }
    }
    SAFE_STEP_DOWN + 2.0
}

/// Would an animal standing here have its feet in water?
///
/// The *feet*, not the body: what this is asked about is a step it has
/// not taken yet, and the question is whether taking it puts the animal
/// in the water at all -- not whether it would be swimming.
fn enters_liquid(world: &dyn BlockWorld, feet: (f64, f64, f64)) -> bool {
    world
        .block(
            feet.0.floor() as i32,
            feet.1.floor() as i32,
            feet.2.floor() as i32,
        )
        .is_some_and(primitive_shared::types::is_liquid)
}

/// Is this animal's body in water?
///
/// Its middle, which is the one sample that gives the answer a player
/// would give: a boar standing in a puddle is not swimming, and one with
/// its back under is.
fn in_liquid(world: &dyn BlockWorld, feet: (f64, f64, f64), species: Species) -> bool {
    let middle = feet.1 + f64::from(species.height() * 0.5);
    world
        .block(
            feet.0.floor() as i32,
            middle.floor() as i32,
            feet.2.floor() as i32,
        )
        .is_some_and(primitive_shared::types::is_liquid)
}

/// The nearest open water an animal could walk to and drink from, as a
/// point on the ground.
///
/// **Spokes, not a disc**, and that is the whole reason the mechanic is
/// affordable. `lit_fire_near` sweeps every column in its radius because
/// a fire five blocks away that the wolf misses is a fire that stops
/// working; a pond eleven blocks away that a deer misses is a deer that
/// looks again in four seconds, from somewhere else, and finds it. So
/// this is `cover_heading`'s shape: twelve headings thirty degrees
/// apart, sampled outward every block and a half, nearest wins. Ninety-
/// six columns of three lookups against the disc's five hundred of the
/// same, and the cost is argued in full at `WATER_SCAN_INTERVAL`.
///
/// What counts is a *surface*: a liquid cell with something other than
/// liquid above it. The cell under a lake is water an animal could never
/// put its head in, and aiming at one would send a deer walking at the
/// middle of a pond.
///
/// Three bands -- a block under its feet to a block over them -- because
/// a bank is not level with the water it holds and a river cut into a
/// meadow sits a block down. Wider than that is a waterfall or a lake on
/// a shelf, and neither is somewhere an animal can stand and drink.
///
/// It is not asked whether the animal can *get* there: the heading goes
/// through `open_heading` at the call site, which is the same trade
/// `food_heading` makes, so an animal never sets off toward water on the
/// far side of a cliff and is never clever enough to find its way round
/// one either.
fn water_near(world: &dyn BlockWorld, animal: &Animal) -> Option<(f32, f32)> {
    const HEADINGS: usize = 12;
    const STEP: f32 = 1.5;
    let foot = animal.at().1.floor() as i32;
    let surface = |cx: i32, cz: i32| {
        (foot - 1..=foot + 1).any(|y| {
            world.block(cx, y, cz).is_some_and(is_liquid)
                && !world.block(cx, y + 1, cz).is_some_and(is_liquid)
        })
    };
    let cell = |yaw: f32, distance: f32| {
        let (sin, cos) = yaw.sin_cos();
        (
            (animal.at().0 + cos * distance).floor() as i32,
            (animal.at().2 + sin * distance).floor() as i32,
        )
    };

    let mut best: Option<(f32, f32)> = None;
    for point in 0..HEADINGS {
        let yaw = point as f32 / HEADINGS as f32 * std::f32::consts::TAU;
        let mut distance = STEP;
        while distance <= WATER_RANGE {
            if best.is_some_and(|(_, near)| near <= distance) {
                break;
            }
            let (cx, cz) = cell(yaw, distance);
            if surface(cx, cz) {
                best = Some((yaw, distance));
                break;
            }
            distance += STEP;
        }
    }
    let (yaw, coarse) = best?;

    // **And then walk the winning spoke back in to the bank.**
    //
    // A stride of a block and a half means the cell the search *found*
    // can be that much out into the lake, and the animal is going to
    // stop `DRINK_REACH` short of whatever it is given -- so aiming at
    // the found cell puts the stopping point in the shallows, where
    // `walk`'s no-water rule turns the animal away, and the deer walks
    // round the pond it came to drink from. Twenty-odd lookups on one
    // heading buys the near edge instead: the first surface cell along
    // the ray, which is the bit of water the animal is actually going to
    // put its nose in.
    let mut distance = 0.5;
    while distance < coarse {
        let (cx, cz) = cell(yaw, distance);
        if surface(cx, cz) {
            // The middle of the cell, so `DRINK_REACH` measures from the
            // water rather than from the corner of its column.
            return Some((cx as f32 + 0.5, cz as f32 + 0.5));
        }
        distance += 0.5;
    }
    let (cx, cz) = cell(yaw, coarse);
    Some((cx as f32 + 0.5, cz as f32 + 0.5))
}

/// The heading toward the nearest dry ground an animal in the water
/// could stand on, if there is any within `SHORE_RANGE`.
///
/// `water_near`'s search read backwards, and it asks `footing` -- the
/// file's own answer to "could I stand there", which already refuses
/// liquid, refuses a drop it would not take and refuses ground that has
/// not loaded. Reusing it rather than writing a second opinion is what
/// keeps a shore the animal swims to from being a shore it cannot climb
/// out onto.
///
/// Eight headings rather than twelve, and eight blocks rather than
/// twelve: this is a search for the *nearest* dry cell and a coarse fan
/// finds one wherever there is one to find. Paid once a second by an
/// animal that is actually in water, which in an ordinary world is none
/// of them.
fn shore_heading(world: &dyn BlockWorld, animal: &Animal) -> Option<f32> {
    const HEADINGS: usize = 8;
    const STEP: f32 = 1.0;
    let mut best: Option<(f32, f32)> = None;
    for point in 0..HEADINGS {
        let yaw = point as f32 / HEADINGS as f32 * std::f32::consts::TAU;
        let (sin, cos) = yaw.sin_cos();
        let mut distance = STEP;
        while distance <= SHORE_RANGE {
            if best.is_some_and(|(_, near)| near <= distance) {
                break;
            }
            let at = (
                animal.at().0 + cos * distance,
                animal.at().1,
                animal.at().2 + sin * distance,
            );
            if footing(world, at, animal.species).is_some() {
                best = Some((yaw, distance));
                break;
            }
            distance += STEP;
        }
    }
    best.map(|(yaw, _)| yaw)
}

// ---- the water's animals ----

/// Nearest a school is put down from a player, in blocks.
///
/// Close enough to be inside the underwater fog's eighteen blocks at the
/// far end of the range, far enough that nothing appears in front of a
/// swimmer's mask.
const FISH_SPAWN_MIN: f32 = 10.0;
/// ...and furthest. Well inside `SPAWN_MIN` of the land's, because the
/// land's is chosen against a horizon and this one against a fog.
const FISH_SPAWN_MAX: f32 = 36.0;
/// Beyond this from every player a swimmer is forgotten. See
/// `forget_the_distant`.
const FISH_DESPAWN_DISTANCE: f32 = 64.0;
/// How many places `populate_water` looks for water on one call. See the
/// note there.
const WATER_LOOKS: usize = 4;
/// How far round the first fish the rest of a school is put.
const SCHOOL_SPREAD: f32 = 2.5;
/// How far from the middle of its school a fish drifts before it turns
/// back toward it. Tighter than a herd's `HERD_COMFORT`: a school is a
/// knot, and one spread across a reef reads as fish that happen to be near
/// each other.
const SCHOOL_COMFORT: f32 = 2.5;
/// How far above the floor and below the surface a swimming body keeps.
const SWIM_MARGIN: f32 = 0.15;
/// How fast a fish changes depth, in blocks a second, at most.
const SWIM_CLIMB: f32 = 1.2;

/// How many blocks of water a kind wants over its floor before it will be
/// put in it.
///
/// Two for a school, which is a pond or a river; eight for a cod, which is
/// the sea past the surf -- and the reason a cod is worth diving for is
/// that it is down there.
fn needs_depth(species: Species) -> f32 {
    match species {
        Species::Cod => 8.0,
        // **A pike wants a lake and not a ditch.** Three blocks is the
        // difference between water a player waded across on the way here
        // and water they have to swim in, and the best single catch in
        // fresh water should be in the second kind. It is also what stops
        // the pond somebody dug behind their house from being a pike farm:
        // dig it deep and it is a pike farm they earned.
        Species::Pike => 3.0,
        // Everything else swims in anything it fits in: a trout in a river
        // two deep is a trout where trout are.
        _ => 2.0,
    }
}

/// The band of heights a swimmer's feet may be at in a column of water,
/// floor to surface. A cod keeps to the bottom third of it.
fn swimming_band(species: Species, floor: f32, surface: f32) -> (f32, f32) {
    let bottom = floor + SWIM_MARGIN;
    let top = surface - species.height() - SWIM_MARGIN;
    let third = (surface - floor) / 3.0;
    match species {
        Species::Cod => (bottom, top.min(bottom + third)),
        // **A shoal is at the top of the water and that is how it is
        // caught.** The cod's rule upside down: a herring within a spear's
        // length of the surface is a fish somebody standing chest-deep in
        // the sea can reach, which is the whole reason the shallow sea is
        // worth wading into on the first evening. A pike lies near the bed
        // among the weed, and is found by going down for it.
        Species::Herring => (bottom.max(top - third), top),
        Species::Pike => (bottom, top.min(bottom + third * 1.5)),
        _ => (bottom, top),
    }
}

/// Is every cell this body overlaps water, and is its top under the surface?
///
/// **The swimmer's `fits`.** `swim` refuses any move that makes this false,
/// which is the whole of why a fish never leaves the water: not a decision
/// it makes, a thing its body cannot do. Unloaded cells are not water, for
/// `fits`'s safe-way-round reason.
fn in_water(world: &dyn BlockWorld, feet: (f64, f64, f64), species: Species) -> bool {
    let half = f64::from(species.width() * 0.5);
    let top = feet.1 as f32 + species.height();
    let (x0, x1) = ((feet.0 - half).floor() as i32, (feet.0 + half).floor() as i32);
    let (z0, z1) = ((feet.2 - half).floor() as i32, (feet.2 + half).floor() as i32);
    let (y0, y1) = ((feet.1 as f32).floor() as i32, (top - 1e-3).floor() as i32);
    if y0 < 0 || y1 + 1 >= CHUNK_SIZE_Y as i32 {
        return false;
    }
    for z in z0..=z1 {
        for x in x0..=x1 {
            for y in y0..=y1 {
                if !world.block(x, y, z).is_some_and(is_liquid) {
                    return false;
                }
            }
            // The top of the body under the water's own surface, which in
            // the top cell of a column is short of the top of the cell
            // (`fluid::SURFACE_DROP`): a fin standing out of the sea is a
            // fish on the surface, and the next step is a fish in the air.
            let (Some(cell), Some(above)) = (world.block(x, y1, z), world.block(x, y1 + 1, z)) else {
                return false;
            };
            if !primitive_shared::fluid::covers_with_above(cell, above, top - y1 as f32) {
                return false;
            }
        }
    }
    true
}

/// The water a point is in, as (floor, surface): the y of the first cell
/// of it and the height its top is drawn at. `None` if the point is not in
/// water or the column runs into an unloaded cell.
fn water_column(world: &dyn BlockWorld, x: f32, y: f32, z: f32) -> Option<(f32, f32)> {
    let (bx, by, bz) = (x.floor() as i32, y.floor() as i32, z.floor() as i32);
    if !is_liquid(world.block(bx, by, bz)?) {
        return None;
    }
    let mut floor = by;
    while floor > 0 && is_liquid(world.block(bx, floor - 1, bz)?) {
        floor -= 1;
    }
    let mut top = by;
    while top + 1 < CHUNK_SIZE_Y as i32 && is_liquid(world.block(bx, top + 1, bz)?) {
        top += 1;
    }
    let above = world.block(bx, top + 1, bz).unwrap_or(primitive_shared::types::BLOCK_AIR);
    let surface = top as f32 + primitive_shared::fluid::surface_height_with_above(world.block(bx, top, bz)?, above);
    Some((floor as f32, surface))
}

/// The top cell of the water under a point, looking down through air.
///
/// Stops at the first cell that is neither air nor water: a school is not
/// put in a cave pool under a hill because the spawner saw through the
/// hill. Forty-eight cells at most, for the reason `surface_under` is
/// bounded.
fn water_surface_under(world: &dyn BlockWorld, x: f32, from_y: f32, z: f32) -> Option<i32> {
    let (bx, bz) = (x.floor() as i32, z.floor() as i32);
    let start = (from_y.floor() as i32).min(CHUNK_SIZE_Y as i32 - 1);
    for y in ((start - 48).max(1)..=start).rev() {
        let here = world.block(bx, y, bz)?;
        if is_liquid(here) {
            return Some(y);
        }
        if !primitive_shared::types::is_air(here) {
            return None;
        }
    }
    None
}

/// The nearest heading to `wanted` a swimmer can take and still be in water
/// a body's length on, if there is one.
///
/// **The shore is a wall to a fish**, and this is `steer_around` for it:
/// eight probes rather than `open_heading`'s footing test, because what a
/// fish asks of the way ahead is not "could I stand there" but "is it sea".
fn open_water_heading(world: &dyn BlockWorld, animal: &Animal, wanted: f32) -> Option<f32> {
    const HEADINGS: usize = 8;
    let reach = animal.species.length().max(1.0);
    let mut best: Option<(f32, f32)> = None;
    for point in 0..HEADINGS {
        let yaw = wanted + point as f32 / HEADINGS as f32 * std::f32::consts::TAU;
        let (sin, cos) = yaw.sin_cos();
        let at = (animal.at().0 + cos * reach, animal.at().1, animal.at().2 + sin * reach);
        if !fits(world, primitive_shared::geometry::wide(at), animal.frame()) || !in_water(world, primitive_shared::geometry::wide(at), animal.species) {
            continue;
        }
        let off = (yaw - wanted).rem_euclid(std::f32::consts::TAU);
        let off = off.min(std::f32::consts::TAU - off);
        if best.is_none_or(|(_, least)| off < least) {
            best = Some((yaw, off));
        }
    }
    best.map(|(yaw, _)| yaw.rem_euclid(std::f32::consts::TAU))
}

/// What a fish decides, at most every second or so.
///
/// **A school is three rules**, and each is one a herd already has, said
/// under water:
///
/// * *Somebody close is somebody to leave.* No line of sight: water is as
///   murky one way as the other, and `Species::awareness` is already short
///   enough to be the fog. The bolt sets `target`, which is what makes the
///   rest of the school see it bolt (`survey`'s alarm), so a school
///   scatters as one rather than a fish at a time.
/// * *A fish away from its school turns back to it* (`SCHOOL_COMFORT`).
/// * *Otherwise it drifts*, and now and then picks another depth in the
///   column it is in -- which is what makes a school over a reef rise and
///   fall rather than slide along one plane.
///
/// What it deliberately does not do is feed, drink, rest at night or
/// remember where it was speared. Each is a rule the land animals earned by
/// making a decision for a player; a fish that did them would be a longer
/// file and the same fish.
fn think_fish(
    animal: &mut Animal,
    players: &[(PlayerId, (f32, f32, f32))],
    seen: &Neighbours,
    world: &dyn BlockWorld,
    rng: &mut Rng,
    dt: f32,
) {
    let fleeing = animal.mind == Mind::Flee;
    if fleeing {
        animal.stamina = (animal.stamina - dt).max(0.0);
    } else {
        animal.stamina = (animal.stamina + dt * STAMINA_RECOVERS).min(animal.species.stamina_seconds());
    }
    // A school's panic does not wait for a thought, for the herd's reason.
    if !fleeing {
        if let Some(heading) = seen.alarm {
            animal.mind = Mind::Flee;
            animal.next_thought = FLEE_SECONDS * 0.6;
            animal.wants_yaw = open_water_heading(world, animal, heading).unwrap_or(heading);
            return;
        }
    }
    animal.next_thought -= dt;
    if animal.next_thought > 0.0 {
        return;
    }
    animal.next_thought = rng.range(0.6, 1.4);
    animal.drift = rng.range(-0.4, 0.4);

    let middle = (
        animal.at().0,
        animal.at().1 + animal.species.height() * 0.5,
        animal.at().2,
    );
    let nearest = players
        .iter()
        .map(|&(id, at)| {
            // The player's middle, not their feet: a swimmer is a body in
            // the water, and a fish below one is near its chest.
            let body = (at.0, at.1 + 0.9, at.2);
            let (dx, dy, dz) = (body.0 - middle.0, body.1 - middle.1, body.2 - middle.2);
            (id, body, (dx * dx + dy * dy + dz * dz).sqrt())
        })
        .min_by(|a, b| a.2.total_cmp(&b.2))
        .filter(|&(_, _, distance)| distance <= animal.species.awareness());
    let column = water_column(world, middle.0, middle.1, middle.2);

    if let Some((id, at, _)) = nearest {
        animal.target = Some(id);
        animal.mind = Mind::Flee;
        animal.next_thought = FLEE_SECONDS * 0.6;
        let away = (animal.at().2 - at.2).atan2(animal.at().0 - at.0);
        animal.wants_yaw = open_water_heading(world, animal, away).unwrap_or(away);
        // ...and away in depth as well: down from somebody above it, up
        // from somebody on the bottom. The band keeps it in the water.
        if let Some((floor, surface)) = column {
            let (bottom, top) = swimming_band(animal.species, floor, surface);
            let wanted = if at.1 > middle.1 { animal.at().1 - 1.5 } else { animal.at().1 + 1.5 };
            animal.swim_depth = wanted.clamp(bottom, top.max(bottom));
        }
        return;
    }

    animal.target = None;
    let to_school = seen.centre.and_then(|(cx, cz)| {
        let (dx, dz) = (cx - animal.at().0, cz - animal.at().2);
        (dx * dx + dz * dz > SCHOOL_COMFORT * SCHOOL_COMFORT).then(|| dz.atan2(dx))
    });
    match to_school {
        Some(heading) => {
            animal.mind = Mind::Wander;
            animal.wants_yaw = heading;
        }
        None if rng.chance(0.2) => animal.mind = Mind::Idle,
        None => {
            animal.mind = Mind::Wander;
            animal.wants_yaw = (animal.wants_yaw + rng.range(-0.9, 0.9)).rem_euclid(std::f32::consts::TAU);
        }
    }
    if let Some((floor, surface)) = column {
        let (bottom, top) = swimming_band(animal.species, floor, surface);
        if top >= bottom && rng.chance(0.35) {
            animal.swim_depth = rng.range(bottom, top);
        }
    }
}

/// One tick of a swimming body.
///
/// `walk` with the ground taken out: the same turn rate, the same easing
/// toward a wanted velocity, the same collider -- and in place of gravity a
/// depth it is pulled toward (`Animal::swim_depth`), in place of a cliff
/// check the edge of the water (`in_water`). A move that would leave the
/// water is not taken, and the fish turns along the shore instead.
///
/// **Out of the water, gravity**, and no swimming: a fish on a bank lies
/// where it fell. See `gasp` for what that costs it.
fn swim(animal: &mut Animal, world: &dyn BlockWorld, dt: f32) {
    let wet = in_water(world, animal.position, animal.species);
    let mut wanted = match animal.mind {
        Mind::Flee => animal.species.run_speed(),
        Mind::Wander => animal.species.walk_speed(),
        _ => 0.0,
    };
    if animal.stamina <= 0.0 {
        wanted *= BLOWN_SPEED;
    }
    wanted *= animal.pace;
    if !wet {
        wanted = 0.0;
    }

    if animal.mind == Mind::Wander {
        animal.wants_yaw = (animal.wants_yaw + animal.drift * dt).rem_euclid(std::f32::consts::TAU);
    }
    let agility = nimbleness(animal.species);
    let urgency = if animal.mind == Mind::Flee { 1.4 } else { 1.0 };
    animal.yaw = turn_towards(animal.yaw, animal.wants_yaw, TURN_RATE * agility * urgency * dt);

    let (sin, cos) = animal.yaw.sin_cos();
    let rate = (ACCELERATION * dt).min(1.0);
    animal.velocity.0 += (cos * wanted - animal.velocity.0) * rate;
    animal.velocity.2 += (sin * wanted - animal.velocity.2) * rate;
    if wanted == 0.0 {
        let creep = animal.velocity.0 * animal.velocity.0 + animal.velocity.2 * animal.velocity.2;
        if creep < STANDING_STILL * STANDING_STILL {
            animal.velocity.0 = 0.0;
            animal.velocity.2 = 0.0;
        }
    }
    if wet {
        let toward = ((animal.swim_depth - animal.at().1) * 2.0).clamp(-SWIM_CLIMB, SWIM_CLIMB);
        animal.velocity.1 += (toward - animal.velocity.1) * rate;
    } else {
        animal.velocity.1 = (animal.velocity.1 + GRAVITY * dt).max(TERMINAL_VELOCITY);
    }

    // Vertical first, as `walk` does it.
    if animal.velocity.1 != 0.0 {
        let try_y = (animal.position.0, animal.position.1 + f64::from(animal.velocity.1 * dt), animal.position.2);
        let allowed = fits(world, try_y, animal.frame()) && (!wet || in_water(world, try_y, animal.species));
        if allowed {
            animal.position = try_y;
            animal.on_ground = false;
        } else {
            if !wet && animal.velocity.1 < 0.0 {
                animal.on_ground = true;
                animal.position.1 = resting_height(world, animal.position, try_y.1, animal.frame());
            }
            animal.velocity.1 = 0.0;
            // The floor or the surface said no: the depth it was making for
            // is on the other side of it, and holding it would push against
            // the edge for ever.
            if wet {
                animal.swim_depth = animal.at().1;
            }
        }
    }

    for (dx, dz) in [(animal.velocity.0 * dt, 0.0), (0.0, animal.velocity.2 * dt)] {
        if dx == 0.0 && dz == 0.0 {
            continue;
        }
        let want = (animal.position.0 + f64::from(dx), animal.position.1, animal.position.2 + f64::from(dz));
        if fits(world, want, animal.frame()) && in_water(world, want, animal.species) {
            animal.position = want;
            continue;
        }
        // The edge of the water or a rock in it. Turn along it now rather
        // than nosing at it until the next thought -- `steer_around`'s
        // lesson, for the same reason.
        animal.velocity.0 = 0.0;
        animal.velocity.2 = 0.0;
        if let Some(open) = open_water_heading(world, animal, animal.wants_yaw) {
            animal.wants_yaw = open;
        } else {
            animal.wants_yaw = (animal.wants_yaw + std::f32::consts::PI).rem_euclid(std::f32::consts::TAU);
        }
        break;
    }
    animal.fall_peak_y = None;
}

/// A tick a fish spends out of the water, as damage -- once it has run out
/// of what it can stand.
///
/// **`suffocate` turned inside out**, and on the same numbers
/// (`BREATH_SECONDS`, `DROWNING_PER_SECOND`), because they are the same
/// thing: a body in the medium it cannot breathe. Fifteen seconds is long
/// enough that a fish flipped onto a bank by a player's spear can be picked
/// up alive -- which it cannot, but the death is not instant -- and short
/// enough that a drained pond is a pond of dead fish by the time anybody
/// walks back to it.
fn gasp(animal: &mut Animal, world: &dyn BlockWorld, dt: f32) -> f32 {
    if in_water(world, animal.position, animal.species) {
        animal.breath = crate::logic::survival::BREATH_SECONDS;
        return 0.0;
    }
    animal.breath -= dt;
    if animal.breath > 0.0 {
        return 0.0;
    }
    crate::logic::survival::DROWNING_PER_SECOND * dt.min(-animal.breath)
}

/// A tick spent standing in a fire, as damage.
///
/// **The same rate a player burns at, read off `logic::survival`
/// rather than written down a second time.** A player who builds a
/// campfire under a hostile animal to be rid of it, or corners a deer
/// against a burning wall, should get the fire they lit -- and a second
/// copy of `BURNING_PER_SECOND` here is a number that can quietly stop
/// agreeing with the player's own the next time somebody tunes it.
///
/// Checked by the rule a player is checked by (`survival::touches_fire`),
/// at the feet and at the middle of the body: a fire an animal is standing
/// *in* reaches its feet, a wider one reaches into its flank, and a burning
/// pit it stands *on* reaches it from under its feet.
/// A ridden horse, a tick on: `horse::step` with the rider's reins, and the
/// animal put where the body went.
///
/// **The same step the rider's client predicts with** (`horse`'s module note):
/// what the server adds is the authority -- a stale rein runs out
/// (`REINS_TIMEOUT`), and the wind, hunger and load are the server's own
/// keeping and gear (`fettle_of`), never the client's word.
fn carry(animal: &mut Animal, world: &dyn BlockWorld, dt: f32) {
    let fettle = fettle_of(animal.keep.as_ref(), animal.gear.as_deref());
    let Some(ride) = animal.ride.as_mut() else {
        return;
    };
    ride.reins_age += dt;
    let mut reins = if ride.reins_age > REINS_TIMEOUT { horse::Reins::SLACK } else { ride.reins };
    reins.jump = ride.jump_for > 0.0;
    let was_up = !ride.body.on_ground;
    horse::step(&mut ride.body, reins, fettle, &|x, y, z| world.block(x, y, z), dt);
    // Spent by the jump it made, or run out: see `Ride::jump_for`.
    ride.jump_for = if !was_up && !ride.body.on_ground && ride.body.vy > 0.0 { 0.0 } else { (ride.jump_for - dt).max(0.0) };
    let body = ride.body;
    animal.position = (body.x, body.y, body.z);
    animal.yaw = body.yaw;
    animal.wants_yaw = body.yaw;
    animal.velocity = (body.vx, body.vy, body.vz);
    animal.on_ground = body.on_ground;
    animal.mind = Mind::Idle;
    animal.next_thought = 1.0;
    animal.attitude = primitive_shared::protocol::Attitude::Easy;
    animal.fall_peak_y = None;
}

fn burn(animal: &Animal, world: &dyn BlockWorld, dt: f32) -> f32 {
    let middle = animal.species.height() * 0.5;
    if !crate::logic::survival::touches_fire(animal.at(), &[middle], |x, y, z| world.block(x, y, z)) {
        return 0.0;
    }
    crate::logic::survival::BURNING_PER_SECOND * dt
}

/// A push through sharpened stakes, as damage (`spikes`).
///
/// **What a ring of stakes is for.** An animal was never cut by them, so the
/// palisade a player planted round a camp was a picture: a boar ran through
/// it as through grass. The same rule the player is held to, at the speed the
/// body is actually going, and not again while it is still flinching from
/// the last blow (`hurt_for`) -- which is also what keeps a body stood among
/// the points from being cut every tick. `hurt` is what makes it bolt.
fn stakes(animal: &Animal, world: &dyn BlockWorld) -> f32 {
    if animal.hurt_for > 0.0 {
        return 0.0;
    }
    let (hx, hy, hz) = animal.species.half_extents();
    let (x, y, z) = animal.position;
    let min = [x - f64::from(hx), y, z - f64::from(hz)];
    let max = [x + f64::from(hx), y + 2.0 * f64::from(hy), z + f64::from(hz)];
    if !primitive_shared::spikes::touches(min, max, |bx, by, bz| world.block(bx, by, bz).unwrap_or(primitive_shared::types::BLOCK_AIR)) {
        return 0.0;
    }
    let speed = animal.velocity.0.hypot(animal.velocity.2);
    primitive_shared::spikes::harm(speed).map_or(0.0, |(damage, _)| damage)
}

/// A tick spent with its head under water, as damage -- once its breath
/// has run out.
///
/// **The same shape `Vitals::breathe` is, reimplemented rather than
/// shared**, and that is a deliberate line rather than a shortcut: a
/// `Vitals` carries hunger, body temperature and wetness along with the
/// breath meter, and giving every animal a full one to use two fields
/// of would drag three player-only systems into a file that does not
/// have them. What *is* shared is the part that would otherwise drift
/// -- `BREATH_SECONDS` and `DROWNING_PER_SECOND`, both read off
/// `logic::survival` -- so the two meters run out at the same rate even
/// though they are counted in different places.
///
/// **Why an animal needs this at all, given none of them swim.**
/// Buoyancy (`walk`'s `BUOYANCY`/`SWIM_RISE`) gets a body back to the
/// surface of almost anything, which is why nothing here has ever had
/// to hold its breath before. What it cannot do is get a body out from
/// under a ceiling: a cave passage that floods, or a pen a player built
/// with the roof underwater, traps an animal at the surface of a space
/// with no surface in it. That is the one case this exists for.
fn suffocate(animal: &mut Animal, world: &dyn BlockWorld, dt: f32) -> f32 {
    // The top of the body rather than its middle -- `in_liquid` (which
    // decides whether an animal is *wading*, and answers to the middle)
    // is asking a different question. This is asking whether its head
    // is under, which is the top of it minus a hair, so a body floating
    // with its back just breaking the surface is not treated as
    // drowning.
    let head = (
        animal.at().0,
        animal.at().1 + animal.species.height() - 0.05,
        animal.at().2,
    );
    let head_under = world
        .block(head.0.floor() as i32, head.1.floor() as i32, head.2.floor() as i32)
        .is_some_and(is_liquid);
    if !head_under {
        animal.breath = crate::logic::survival::BREATH_SECONDS;
        return 0.0;
    }
    animal.breath -= dt;
    if animal.breath > 0.0 {
        return 0.0;
    }
    // Billed for the part of *this* tick spent out of air, not the
    // whole of it -- the same overshoot-only charge `Vitals::breathe`
    // uses, so a tick that crosses zero mid-way is not charged for the
    // half of it that still had air.
    let seconds = dt.min(-animal.breath);
    crate::logic::survival::DROWNING_PER_SECOND * seconds
}

/// A boar's blow, if it is close enough and has waited long enough.
/// What an animal's family does to what it has just decided, this tick.
///
/// **After `think`, every tick, and last word**, because the family is the one
/// thing that outranks a thought: a fawn that has decided to graze where it
/// stands, with its mother running, runs. Written as an override rather than
/// as a rule inside `think`, `graze` and `bolt` each, because those are five
/// places a young one's mother would have had to be remembered in, and the
/// first one forgotten is a fawn left standing in a field its herd has left.
///
/// * **The young follows.** It runs with a running mother, aimed a little
///   ahead of her along the way she is going, so it comes up beside her rather
///   than into her back. Otherwise it keeps within `youth::MOTHER_LEASH`,
///   walking back to her when it has drifted -- unless she is in a fight,
///   when it is left to its own fright (which, being young, is running).
/// * **The mother waits.** Running, she keeps to `youth::MOTHER_PACE` of the
///   young's pace, and slows further when it has fallen `FLEE_LEASH` behind,
///   so she never leaves it: the doe is the slower deer, and that is the whole
///   cost of the fawn. Grazing, she stops when it has strayed past the leash
///   and turns to face it until it comes back -- the head up and watching.
///
/// Rejected: **a herd rule** -- young keeping to the herd's centre, as the
/// herd already does (`Neighbours::centre`). A boar is not a herd animal, and
/// "keeps near the herd" is exactly the fawn that ends the day ten deer away
/// from its mother because the herd's middle moved.
fn keep_family(animal: &mut Animal, family: Family) {
    animal.speed_cap = f32::INFINITY;
    if let Some(mother) = family.mother {
        let (dx, dz) = (mother.at.0 - animal.at().0, mother.at.2 - animal.at().2);
        let apart = dx.hypot(dz);
        if mother.fleeing {
            // Where she will be in a moment, which is where to run to.
            let (sin, cos) = mother.heading.sin_cos();
            let (ax, az) = (dx + cos * 1.5, dz + sin * 1.5);
            animal.mind = Mind::Flee;
            animal.wants_yaw = az.atan2(ax);
            animal.next_thought = animal.next_thought.max(0.25);
        } else if !mother.fighting && apart > youth::MOTHER_LEASH && animal.mind != Mind::Flee {
            animal.mind = Mind::Wander;
            animal.wants_yaw = dz.atan2(dx);
            animal.attitude = primitive_shared::protocol::Attitude::Easy;
        }
    }
    if let Some(young) = family.young {
        let (dx, dz) = (young.at.0 - animal.at().0, young.at.2 - animal.at().2);
        let apart = dx.hypot(dz);
        if animal.mind == Mind::Flee {
            // Never faster than it can follow, and a good deal slower when it
            // has fallen behind: it closes, and then they go on together.
            let pace = if apart > youth::FLEE_LEASH { 0.4 } else { youth::MOTHER_PACE };
            animal.speed_cap = young.run * pace;
        } else if matches!(animal.mind, Mind::Idle | Mind::Wander) && apart > youth::MOTHER_LEASH {
            animal.mind = Mind::Idle;
            animal.wants_yaw = dz.atan2(dx);
            animal.attitude = primitive_shared::protocol::Attitude::Alert;
        }
    }
}

/// A dead body going down: its last shove bleeding off, gravity, and nothing
/// else. See `FALL_SECONDS`.
///
/// **Not `walk`**, which is a living animal's legs: it steers toward a wanted
/// heading, refuses ledges (`SAFE_STEP_DOWN`), climbs steps and holds a bird
/// at its flight height. A body does none of it -- a deer killed at the edge of
/// a bank goes over it, and a grouse shot out of the air comes down. The two
/// things it shares with `walk` are the ones that must agree with the living:
/// what `fits` and where it comes to rest (`resting_height`).
fn lie_still(body: &mut Animal, world: &dyn BlockWorld, dt: f32) {
    let frame = body.frame();
    let drag = (1.0 - FALL_DRAG * dt).max(0.0);
    body.velocity.0 *= drag;
    body.velocity.2 *= drag;
    body.velocity.1 = (body.velocity.1 + GRAVITY * dt).max(TERMINAL_VELOCITY);
    let try_y = (body.position.0, body.position.1 + f64::from(body.velocity.1 * dt), body.position.2);
    if fits(world, try_y, frame) {
        body.position = try_y;
        body.on_ground = false;
    } else {
        if body.velocity.1 < 0.0 {
            body.on_ground = true;
            body.position.1 = resting_height(world, body.position, try_y.1, frame);
        }
        body.velocity.1 = 0.0;
    }
    for (dx, dz) in [(body.velocity.0 * dt, 0.0), (0.0, body.velocity.2 * dt)] {
        let want = (body.position.0 + f64::from(dx), body.position.1, body.position.2 + f64::from(dz));
        if (dx != 0.0 || dz != 0.0) && fits(world, want, frame) {
            body.position = want;
        }
    }
}

fn gore(
    animal: &mut Animal,
    players: &[(primitive_shared::protocol::PlayerId, (f32, f32, f32))],
    dt: f32,
) -> Option<Blow> {
    animal.gore_cooldown = (animal.gore_cooldown - dt).max(0.0);
    if !animal.fights() || animal.gore_cooldown > 0.0 {
        return None;
    }
    let target = animal.target?;
    let (_, at) = players.iter().find(|&&(id, _)| id == target)?;
    let (dx, dy, dz) = (
        at.0 - animal.at().0,
        at.1 - animal.at().1,
        at.2 - animal.at().2,
    );
    if dx * dx + dy * dy + dz * dz > GORE_RANGE * GORE_RANGE {
        return None;
    }
    // **In front of it.** A boar gores with its tusks, and the tusks are
    // at the end with the face on it.
    //
    // Without this a boar that has charged past you keeps hitting you
    // with its backside for as long as you stand behind it, which is
    // both absurd to watch and impossible to answer: the whole reason a
    // charge overshoots is to give the player a moment on the far side
    // of it, and a rear-facing hitbox takes that moment away again.
    let (facing_x, facing_z) = animal.yaw.sin_cos();
    let flat = (dx * dx + dz * dz).sqrt().max(1e-4);
    let ahead = (dx * facing_z + dz * facing_x) / flat;
    if ahead < GORE_ARC {
        return None;
    }
    animal.gore_cooldown = GORE_INTERVAL;
    Some(Blow {
        victim: target,
        damage: animal.species.damage() * youth::strength(animal.growth),
        species: animal.species,
    })
}

/// A hunter's bite on the animal it is chasing, if it has caught up.
///
/// The same three questions `gore` asks about a player -- in range, in
/// front, and off cooldown -- against the animal it is hunting instead.
/// It shares the cooldown with `gore` deliberately: a wolf has one set
/// of teeth, and one that could bite a deer and a person in the same
/// tick would be two wolves.
fn bite(animal: &mut Animal, seen: &Neighbours) -> Option<Bite> {
    if animal.gore_cooldown > 0.0 {
        return None;
    }
    let quarry = animal.quarry?;
    let (id, at) = seen.quarry?;
    if id != quarry {
        return None; // it has lost the one it was after
    }
    let (dx, dy, dz) = (
        at.0 - animal.at().0,
        at.1 - animal.at().1,
        at.2 - animal.at().2,
    );
    if dx * dx + dy * dy + dz * dz > BITE_RANGE * BITE_RANGE {
        return None;
    }
    // In front of it, for the reason a gore has to be: an animal that
    // bites with its hindquarters is an animal that cannot be escaped by
    // getting behind it.
    let (facing_x, facing_z) = animal.yaw.sin_cos();
    let flat = (dx * dx + dz * dz).sqrt().max(1e-4);
    if (dx * facing_z + dz * facing_x) / flat < GORE_ARC {
        return None;
    }
    animal.gore_cooldown = GORE_INTERVAL;
    Some(Bite {
        at: quarry,
        damage: animal.species.damage(),
        from: animal.at(),
    })
}

/// Would an animal of this species standing here be clear of the world?
///
/// The whole collider, in one function, because every caller wants the
/// same question: the physics step asks it three times a tick and the
/// spawner asks it once. Cells nobody has loaded count as **blocked** --
/// the safe way round, since the alternative is animals walking off into
/// terrain that has not arrived and falling through it.
fn fits(world: &dyn BlockWorld, feet: (f64, f64, f64), frame: impl Into<Frame>) -> bool {
    let frame = frame.into();
    // Across in `f64`, which is where the cells a body spans are decided a
    // long way from zero; up in `f32`, which never leaves a few hundred.
    let half = f64::from(frame.width * 0.5);
    let height = frame.height;
    let (x0, x1) = ((feet.0 - half).floor() as i32, (feet.0 + half).floor() as i32);
    let (z0, z1) = ((feet.2 - half).floor() as i32, (feet.2 + half).floor() as i32);
    let feet = (feet.0, feet.1 as f32, feet.2);
    let y0 = feet.1.floor() as i32;
    let y1 = (feet.1 + height - 1e-3).floor() as i32;
    if y0 < 0 || y1 >= CHUNK_SIZE_Y as i32 {
        return false;
    }
    for z in z0..=z1 {
        for x in x0..=x1 {
            for y in y0..=y1 {
                let Some(block) = world.block(x, y, z) else {
                    return false;
                };
                // Water is not a wall -- an animal that treats a puddle
                // as one is an animal that cannot cross a stream -- but
                // nothing here swims either, so deep water is somewhere
                // they simply sink through and then walk along the
                // bottom of. That is a poor outcome and a rare one, and
                // the alternative is buoyancy for three species.
                if is_liquid(block) {
                    continue;
                }
                // Anything with no box at all -- air, a tuft of grass, a
                // stone lying on the ground. **This guard is the whole
                // function.** Without it `collision_height` answers zero
                // for air, `top` comes out as the cell's own floor, and
                // every cell above the animal's feet reads as
                // overlapping it: an animal so surrounded by walls that
                // it can neither walk nor fall, standing wherever it
                // spawned forever.
                // **A step is its tread and its riser**, not the cube its
                // row is: read as a whole cell it stood a sheep on the
                // top of the riser over the tread, half a block of air
                // under its front hooves.
                if let Some(boxes) = step_boxes_at(world, block, (x, y, z)) {
                    let clear = boxes.iter().all(|(min, max)| {
                        let across = f64::from(x) + f64::from(min[0]) < feet.0 + half
                            && f64::from(x) + f64::from(max[0]) > feet.0 - half
                            && f64::from(z) + f64::from(min[2]) < feet.2 + half
                            && f64::from(z) + f64::from(max[2]) > feet.2 - half;
                        !(across && y as f32 + max[1] > feet.1 && (y as f32 + min[1]) < feet.1 + height)
                    });
                    if !clear {
                        return false;
                    }
                    continue;
                }
                let solid = stand_height(block);
                if solid <= 0.0 {
                    continue;
                }
                let top = y as f32 + solid;
                if top > feet.1 && (y as f32) < feet.1 + height {
                    return false;
                }
            }
            // **Nor on the cope stones of a finished dry stone wall**
            // (`build::bars_animals`): a row of stones on edge is not a place
            // an animal puts its feet, so it neither climbs onto one nor
            // plans a route over it -- which is how a wall a player steps
            // over holds a flock at one cell high. Asked here, of the cells
            // under the feet, because this is the one question both the
            // climb and the route (`footing`) put.
            let resting = feet.1 - (y0 as f32) < 0.5;
            if resting && world.block(x, y0 - 1, z).is_some_and(primitive_shared::build::bars_animals) {
                return false;
            }
        }
    }
    true
}

/// The height to rest at once a fall has been stopped by something
/// below `was_at`, worked out from the actual top of whatever is in the
/// way rather than assumed to be a whole number.
///
/// **Read `fits`'s footprint and take the highest surface that does not
/// put the animal above where it already was.** `was_at` is known good
/// -- it is the position the previous tick left the animal standing in,
/// and `fits` passed there -- so the true landing height is somewhere
/// between the failed `low` and `was_at`, and it is exactly the top of
/// whichever colliding cell reaches furthest up without passing
/// `was_at`. For a full block that is the same number `.floor()` used
/// to give, because a full block's top always sits on a whole number;
/// for anything shorter -- a campfire, a drying rack, a future slab --
/// it is not, and `.floor()` drove the animal through the top it had
/// just failed to clear. See the call site in `walk` and
/// `an_animal_can_walk_over_a_campfire_the_way_a_player_can`.
fn resting_height(world: &dyn BlockWorld, at: (f64, f64, f64), low: f64, frame: impl Into<Frame>) -> f64 {
    let frame = frame.into();
    let (was_at, low) = (at.1 as f32, low as f32);
    let half = f64::from(frame.width * 0.5);
    let (x0, x1) = ((at.0 - half).floor() as i32, (at.0 + half).floor() as i32);
    let (z0, z1) = ((at.2 - half).floor() as i32, (at.2 + half).floor() as i32);
    // The fall that triggered this call cannot have covered more than a
    // tick's worth of height, so the cells worth asking about are the
    // handful between where it started and where it was stopped --
    // never the whole column down to bedrock.
    let (y0, y1) = (low.floor() as i32, was_at.floor() as i32);
    let mut highest = was_at.floor();
    for z in z0..=z1 {
        for x in x0..=x1 {
            for y in y0..=y1 {
                let Some(block) = world.block(x, y, z) else {
                    continue;
                };
                if is_liquid(block) {
                    continue;
                }
                // A step's boxes under the footprint, for `fits`'s reason.
                if let Some(boxes) = step_boxes_at(world, block, (x, y, z)) {
                    for (min, max) in boxes.iter() {
                        let across = f64::from(x) + f64::from(min[0]) < at.0 + half
                            && f64::from(x) + f64::from(max[0]) > at.0 - half
                            && f64::from(z) + f64::from(min[2]) < at.2 + half
                            && f64::from(z) + f64::from(max[2]) > at.2 - half;
                        let top = y as f32 + max[1];
                        if across && top <= was_at {
                            highest = highest.max(top);
                        }
                    }
                    continue;
                }
                let solid = stand_height(block);
                if solid <= 0.0 {
                    continue;
                }
                let top = y as f32 + solid;
                // Never above `was_at`: a cell whose top is higher than
                // where the animal already stood a moment ago is not
                // what stopped this fall, and resting on it would lift
                // the animal through ground it had not yet reached.
                if top <= was_at {
                    highest = highest.max(top);
                }
            }
        }
    }
    f64::from(highest)
}

/// The first solid surface below a point, as the y of the cell to stand
/// *in*.
///
/// `None` if there is nothing under it within the world, which is what a
/// spawn attempt over a chasm or in an unloaded chunk gets -- and the
/// correct answer to both is to give up rather than to guess.
/// Is there enough standing timber round here to call it a wood?
///
/// **The spawner's answer to "which biome is this".** It has a
/// `BlockWorld` and no generator, so the question is asked of what is
/// actually there: trunks and canopy within a few blocks. That is true
/// of a forest and a taiga, false of a meadow, a steppe, a desert and a
/// tundra -- and true of a wood somebody planted, which is the right
/// answer and one a biome lookup would have got wrong.
///
/// Twelve cells of a box eleven across and six high: about one tree.
/// Counted rather than "any", because a single log lying in a field is
/// deadfall and not a forest.
fn wooded(world: &dyn BlockWorld, x: f32, ground: i32, z: f32) -> bool {
    use primitive_shared::types::{
        block_kind, BLOCK_ACACIA_LEAVES, BLOCK_APPLE_LEAVES, BLOCK_APPLE_LEAVES_FRUIT,
        BLOCK_BIRCH_LEAVES, BLOCK_BIRCH_LOG, BLOCK_LEAVES, BLOCK_LOG,
    };
    let (bx, bz) = (x.floor() as i32, z.floor() as i32);
    let mut wood = 0;
    for dz in -5..=5 {
        for dx in -5..=5 {
            for dy in 0..6 {
                let Some(block) = world.block(bx + dx, ground + dy, bz + dz) else {
                    continue;
                };
                if primitive_shared::wood::is_log(block)
                    || primitive_shared::wood::is_wood_leaves(block)
                    || matches!(
                    block_kind(block),
                    BLOCK_LOG
                        | BLOCK_BIRCH_LOG
                        | BLOCK_LEAVES
                        | BLOCK_BIRCH_LEAVES
                        | BLOCK_APPLE_LEAVES
                        | BLOCK_APPLE_LEAVES_FRUIT
                        | BLOCK_ACACIA_LEAVES
                        | primitive_shared::types::BLOCK_MAPLE_LEAVES
                        // **...and a palm**, whose trunk is a *branch* to
                        // every rule in the game (`types::is_branch`) and
                        // whose crown is fronds rather than leaves -- so
                        // neither of the two lists above had ever heard of
                        // it. A grove on a hot shore was "not wooded", and
                        // the one animal that lives in one (`Species::Monkey`)
                        // would never have been put in a world at all. A bear
                        // reads the same line and is none the worse for it:
                        // there are no bears on a tropical beach
                        // (`Species::lives_in`).
                        | primitive_shared::types::BLOCK_PALM_TRUNK
                        | primitive_shared::types::BLOCK_PALM_FRONDS
                        | primitive_shared::types::BLOCK_PALM_COCONUTS
                ) {
                    wood += 1;
                    if wood >= 12 {
                        return true;
                    }
                }
            }
        }
    }
    false
}

fn surface_under(world: &dyn BlockWorld, x: f32, from_y: f32, z: f32) -> Option<i32> {
    let (bx, bz) = (x.floor() as i32, z.floor() as i32);
    let start = (from_y.floor() as i32).min(CHUNK_SIZE_Y as i32 - 1);
    for y in (1..=start).rev() {
        let here = world.block(bx, y, bz)?;
        let below = world.block(bx, y - 1, bz)?;
        // Leaves count, and that is the same split `stand_height`
        // makes: a canopy is a thicket to a player and a branch to a
        // bird. `has_full_top` asks the *player's* question, so asking
        // it here dropped every perching bird through the crown it was
        // aiming at and onto the turf below.
        let footing = primitive_shared::types::has_full_top(below)
            || primitive_shared::types::is_leafy(below);
        if primitive_shared::types::is_air(here) && footing {
            return Some(y);
        }
    }
    None
}

/// The cell a **bird** comes to rest in over this column: one above the
/// first thing with a box in it, looking down from `from_y`.
///
/// **`surface_under` asks for air over footing, and there is exactly one
/// column in this world where that never happens: the one a nest is in.**
/// A nest sits *on* a canopy and fills the only air cell over it, so the
/// scan walks past the crown, past the trunk, and on down into the ground
/// -- and in a solid world it reaches bedrock and answers nothing at all.
/// That answer went into `Animal::flight_ceiling` as "no ceiling", the
/// glide then aimed at whatever was under the bird instead, and a grouse
/// flying home to its nest came down in the *shade of its own tree* and
/// stood at the foot of the trunk for the rest of the day. That is what
/// the player was looking at when they said birds бездельничают: not a
/// bird with nothing to do, a bird that never got up into the tree it
/// lives in.
///
/// The rule here is the one a falling body obeys, and it is the same one
/// `sea_or_ground_under` gives a gull with the sea taken out: the first
/// cell with a collision box is the thing you land on, whatever is
/// stacked on it. Anything with no box -- a tuft, a flower, the air --
/// is looked through, which is also the right answer for the grass a
/// bird pecks in. Forty-eight cells at most, and `None` on ground that
/// has not loaded.
fn perch_over(world: &dyn BlockWorld, x: f32, from_y: f32, z: f32) -> Option<i32> {
    let (bx, bz) = (x.floor() as i32, z.floor() as i32);
    let start = (from_y.floor() as i32).min(CHUNK_SIZE_Y as i32 - 1);
    for y in ((start - 48).max(1)..=start).rev() {
        if stand_height(world.block(bx, y, bz)?) > 0.0 {
            return Some(y + 1);
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Midday, for the tests that do not care what time it is.
    ///
    /// Most of them: the clock only reaches the part of the mind that
    /// decides how much an unbothered animal moves about. Anything being
    /// chased behaves the same at any hour, which is the point of it
    /// being a hunt rather than a schedule.
    const NOON: f32 = 0.5;
    use crate::logic::falling::tests::TestWorld;
    use primitive_shared::types::{BLOCK_AIR, BLOCK_STONE};

    /// A flat world of grass at y = 20, air above, out to `span` either
    /// side of the origin.
    fn meadow(span: i32) -> TestWorld {
        let world = TestWorld::default();
        for z in -span..=span {
            for x in -span..=span {
                world.put(x, 20, z, BLOCK_GRASS);
                for y in 21..30 {
                    world.put(x, y, z, BLOCK_AIR);
                }
            }
        }
        world
    }

    fn player(at: (f32, f32, f32)) -> Vec<(primitive_shared::protocol::PlayerId, (f32, f32, f32))> {
        vec![(1, at)]
    }

    #[test]
    fn a_frightened_monkey_goes_up_the_trunk_it_is_against_and_a_calm_one_comes_down() {
        // **The whole of the climb, as the two answers `climbing` gives.**
        // What makes a troop safe in the palms is not a speed and not an
        // altitude: it is that the timber it is touching holds it up while
        // it has a reason to be up, and lets it down when it has not. See
        // `climbing` for why this is not the bird's target altitude.
        let world = meadow(8);
        for y in 21..27 {
            world.put(2, y, 0, primitive_shared::types::BLOCK_LOG);
        }
        let mut animals = Animals::seeded(5);
        let id = animals.spawn(Species::Monkey, (1.55, 21.0, 0.5)).expect("a monkey");
        let up = {
            let monkey = animals.find_mut_for_test(id).expect("the monkey");
            // Facing the trunk, half a stride off it: the hand is on the wood.
            monkey.yaw = 0.0;
            monkey.mind = Mind::Flee;
            climbing(monkey, &world)
        };
        assert_eq!(up, Some(TRUNK_CLIMB_SPEED), "a frightened monkey against a trunk did not climb it");
        let down = {
            let monkey = animals.find_mut_for_test(id).expect("the monkey");
            monkey.mind = Mind::Idle;
            climbing(monkey, &world)
        };
        assert_eq!(down, Some(-TRUNK_DESCEND_SPEED), "a calm monkey stayed up the tree");
        // ...and out on the grass, with nothing to hold, it falls like
        // everything else -- which is what makes the grove a place rather
        // than a height.
        let loose = {
            let monkey = animals.find_mut_for_test(id).expect("the monkey");
            monkey.position = (-4.0, 21.0, -4.0);
            monkey.mind = Mind::Flee;
            climbing(monkey, &world)
        };
        assert_eq!(loose, None, "a monkey climbed thin air");
        // Nothing else in the world does any of this.
        for &species in Species::ALL.iter().filter(|s| !s.climbs()) {
            let other = animals.spawn(species, (1.55, 21.0, 0.5));
            let Some(other) = other else { continue };
            let animal = animals.find_mut_for_test(other).expect("the animal");
            animal.yaw = 0.0;
            animal.mind = Mind::Flee;
            assert_eq!(climbing(animal, &world), None, "a {} climbed a tree", species.name());
        }
    }

    /// The distance between two points on the ground.
    fn apart(a: (f32, f32, f32), b: (f32, f32, f32)) -> f32 {
        ((a.0 - b.0).powi(2) + (a.2 - b.2).powi(2)).sqrt()
    }

    /// A meadow with a one-block bench across it at `x = at`.
    ///
    /// The commonest shape in the world: worldgen is full of
    /// single-block terraces, and anything that cannot get up one spends
    /// its life walking into them.
    fn meadow_with_a_step(span: i32, at: i32) -> TestWorld {
        let world = meadow(span);
        for z in -span..=span {
            for x in at..=span {
                world.put(x, 21, z, BLOCK_GRASS);
            }
        }
        world
    }

    /// **A death the world deals leaves a body**, the same as one a spear
    /// deals. The poison tick took health below nothing and nothing asked
    /// whether that was a death, so a lion beaten to half a point ran off,
    /// "died" of the paste, walked on at minus three and was forgotten
    /// with no carcass anywhere -- and a fall, a fire or held breath killed
    /// through `hurt` and threw the death away.
    #[test]
    fn an_animal_the_poison_finishes_off_leaves_a_body_to_find() {
        let world = meadow(20);
        let mut animals = Animals::seeded(5);
        let id = animals.spawn(Species::Deer, (0.5, 21.0, 0.5)).expect("deer");
        if let Some(deer) = animals.animals.iter_mut().find(|a| a.id == id) {
            deer.health = 0.2;
            deer.poison_for = primitive_shared::combat::POISON_SECONDS;
        }
        for _ in 0..100 {
            animals.step(&world, &player((15.0, 21.0, 15.0)), 0.05, NOON);
        }
        assert!(animals.find(id).is_none(), "a deer poisoned past nothing is still walking about");
        let fallen = animals.take_fallen();
        assert!(
            fallen.iter().any(|death| death.species == Species::Deer),
            "it died and nobody was told to lay a body: {fallen:?}"
        );
    }

    #[test]
    fn an_animal_that_dies_in_a_fire_leaves_a_body_to_find() {
        let world = meadow(20);
        let mut animals = Animals::seeded(5);
        let id = animals.spawn(Species::Deer, (0.5, 21.0, 0.5)).expect("deer");
        world.put(0, 21, 0, primitive_shared::types::BLOCK_CAMPFIRE_LIT);
        if let Some(deer) = animals.animals.iter_mut().find(|a| a.id == id) {
            deer.health = 0.1;
        }
        for _ in 0..20 {
            animals.step(&world, &player((15.0, 21.0, 15.0)), 0.05, NOON);
        }
        assert!(animals.find(id).is_none(), "the deer stood in the fire and lived");
        let fallen = animals.take_fallen();
        assert!(
            fallen.iter().any(|death| death.species == Species::Deer),
            "it burned and nobody was told to lay a body: {fallen:?}"
        );
    }

    /// **The bug this test was written for.** An animal walked into a
    /// one-block bench, rose a fifth of a block, fell back, and did it
    /// again for as long as anybody watched -- so a deer could be penned
    /// in by a kerb and the world read as a set of corridors.
    ///
    /// The cause was one line: the vertical move clears `on_ground` the
    /// instant the animal leaves the floor, and the climb below it was
    /// gated on `on_ground`. So the first tick of a scramble made the
    /// second tick impossible.
    #[test]
    fn an_animal_gets_up_a_one_block_step() {
        let world = meadow_with_a_step(20, 4);
        let mut animals = Animals::seeded(3);
        let id = animals
            .spawn(Species::Deer, (0.5, 21.0, 0.5))
            .expect("deer");
        // Pointed at the step and told to run at it: a player behind it
        // is the least contrived way to get a deer to go somewhere.
        for _ in 0..400 {
            animals.step(&world, &player((-6.0, 21.0, 0.5)), 0.05, NOON);
            if animals.find(id).is_some_and(|a| a.at().1 >= 22.0) {
                break;
            }
        }
        let deer = animals.find(id).expect("the deer vanished");
        assert!(
            deer.at().1 >= 22.0,
            "it never got up the step -- still at y={:.2}, x={:.2}",
            deer.at().1,
            deer.at().0
        );
    }

    /// **A field wall of dry stone holds a flock at one cell high**, where a
    /// bench of earth the same height is a step (the test above): its cope
    /// stones are not a place an animal puts its feet (`build::bars_animals`).
    #[test]
    fn a_finished_dry_stone_wall_one_cell_high_is_not_climbed() {
        use primitive_shared::build;
        use primitive_shared::types::{BLOCK_PEBBLE, BLOCK_STONE};
        let mut wall = BLOCK_PEBBLE;
        for _ in 0..4 {
            wall = build::lay(wall, BLOCK_PEBBLE, false, BLOCK_STONE, false).expect("a course").result;
        }
        assert!(build::bars_animals(wall));
        let world = meadow(20);
        for z in -20..=20 {
            world.put(4, 21, z, wall);
        }
        let mut animals = Animals::seeded(3);
        let id = animals.spawn(Species::Deer, (0.5, 21.0, 0.5)).expect("deer");
        for _ in 0..400 {
            animals.step(&world, &player((-6.0, 21.0, 0.5)), 0.05, NOON);
        }
        let deer = animals.find(id).expect("the deer vanished");
        assert!(deer.at().1 < 22.0 && deer.at().0 < 4.0, "the deer got over the wall to {:?}", deer.at());
    }

    /// ...and it does it as a scramble rather than a teleport. A whole
    /// block in one tick, with the client interpolating between them, is
    /// an animal appearing a metre higher up.
    #[test]
    fn getting_up_a_step_takes_more_than_one_tick() {
        let world = meadow_with_a_step(20, 4);
        let mut animals = Animals::seeded(3);
        let id = animals
            .spawn(Species::Deer, (0.5, 21.0, 0.5))
            .expect("deer");
        let mut ticks_off_the_floor = 0;
        for _ in 0..400 {
            animals.step(&world, &player((-6.0, 21.0, 0.5)), 0.05, NOON);
            let Some(deer) = animals.find(id) else { break };
            if deer.at().1 > 21.05 && deer.at().1 < 21.95 {
                ticks_off_the_floor += 1;
            }
            if deer.at().1 >= 22.0 {
                break;
            }
        }
        assert!(
            ticks_off_the_floor >= 3,
            "the climb took {ticks_off_the_floor} tick(s) -- that is a teleport, not a step"
        );
    }

    /// Two blocks is a wall, and has to stay one: it is what makes a pen
    /// possible without a fence block existing.
    #[test]
    fn two_blocks_is_still_a_wall() {
        let world = meadow(20);
        for z in -20..=20 {
            for x in 4..=20 {
                world.put(x, 21, z, BLOCK_GRASS);
                world.put(x, 22, z, BLOCK_GRASS);
            }
        }
        let mut animals = Animals::seeded(3);
        let id = animals
            .spawn(Species::Deer, (0.5, 21.0, 0.5))
            .expect("deer");
        for _ in 0..400 {
            animals.step(&world, &player((-6.0, 21.0, 0.5)), 0.05, NOON);
        }
        let deer = animals.find(id).expect("the deer vanished");
        assert!(
            deer.at().1 < 22.0,
            "it climbed a two-block wall to y={:.2}",
            deer.at().1
        );
    }

    #[test]
    fn a_bolting_animal_does_not_run_off_the_edge_of_the_world() {
        // A flee heading used to be "directly away from the player" and
        // nothing else, which is right in a field and wrong at the edge
        // of one. Here the player stands between the deer and the middle
        // of the meadow, so straight away from them is over the drop.
        let world = meadow(20);
        let mut animals = Animals::seeded(7);
        let id = animals.spawn(Species::Deer, (17.5, 21.0, 0.5)).expect("deer");
        for _ in 0..200 {
            animals.step(&world, &player((14.0, 21.0, 0.5)), 0.05, NOON);
        }
        let deer = animals.find(id).expect("it fell out of the world");
        assert!(
            deer.at().1 > 20.0,
            "it bolted off the cliff and is at y={}",
            deer.at().1
        );
        assert!(
            deer.at().0 < 21.0,
            "it ran past the edge at x=21: x={}",
            deer.at().0
        );
        // ...and it did actually run, rather than being saved by
        // standing still.
        assert!(
            apart(deer.at(), (17.5, 21.0, 0.5)) > 3.0,
            "it never left: {:?}",
            deer.at()
        );
    }

    #[test]
    fn a_bolting_animal_goes_round_a_wall_rather_than_into_it() {
        let world = meadow(30);
        // A wall across the deer's escape, two blocks high so it cannot
        // be stepped over, with the ends left open.
        for z in -8..=8 {
            for y in 21..=22 {
                world.put(12, y, z, BLOCK_STONE);
            }
        }
        let mut animals = Animals::seeded(8);
        let id = animals.spawn(Species::Deer, (8.5, 21.0, 0.5)).expect("deer");
        for _ in 0..120 {
            animals.step(&world, &player((4.0, 21.0, 0.5)), 0.05, NOON);
        }
        let deer = animals.find(id).expect("alive");
        // It cannot get through the wall, so what it must not do is
        // spend the whole bolt with its nose against it.
        assert!(
            deer.at().2.abs() > 2.0,
            "it ran straight at the wall and stayed there: {:?}",
            deer.at()
        );
    }

    #[test]
    fn one_deer_bolting_takes_the_rest_of_the_herd_with_it() {
        // What a herd is *for*. The third deer never sees the player --
        // it is fifteen metres off, past its own awareness -- and it
        // leaves anyway, because the second one did.
        let world = meadow(60);
        let mut animals = Animals::seeded(9);
        animals.spawn(Species::Deer, (0.5, 21.0, 0.5)).expect("deer");
        animals.spawn(Species::Deer, (6.5, 21.0, 0.5)).expect("deer");
        let far = animals.spawn(Species::Deer, (12.5, 21.0, 0.5)).expect("deer");

        let mut startled = false;
        for _ in 0..60 {
            animals.step(&world, &player((-3.0, 21.0, 0.5)), 0.05, NOON);
            if animals.find(far).expect("alive").mind == Mind::Flee {
                startled = true;
                break;
            }
        }
        assert!(startled, "the herd stood and watched one of its own bolt");
    }

    #[test]
    fn a_herd_that_has_spread_out_comes_back_together() {
        let world = meadow(60);
        let mut animals = Animals::seeded(10);
        let a = animals.spawn(Species::Deer, (0.5, 21.0, 0.5)).expect("deer");
        let b = animals.spawn(Species::Deer, (10.5, 21.0, 0.5)).expect("deer");
        // Somebody has to be online or the world forgets them, and they
        // must not be able to see him.
        let watcher = player((60.0, 21.0, 60.0));
        // The closest they ever get, rather than where they happen to be
        // at the end: a herd is a pair that keeps coming back to each
        // other, and the instant a run finishes on is a coin toss.
        let mut closest = f32::INFINITY;
        for _ in 0..600 {
            animals.step(&world, &watcher, 0.05, NOON);
            closest = closest.min(apart(
                animals.find(a).expect("alive").at(),
                animals.find(b).expect("alive").at(),
            ));
        }
        assert!(
            closest <= HERD_COMFORT,
            "ten metres apart in an empty field and neither went to look: closest {closest}"
        );
    }

    #[test]
    fn a_beaten_boar_runs_rather_than_dying_where_it_stands() {
        // The one thing a boar used to do that nothing alive does.
        let world = meadow(60);
        let mut animals = Animals::seeded(11);
        let id = animals.spawn(Species::Boar, (4.0, 21.0, 0.5)).expect("boar");
        // Seven tenths of it off: under a third left, which is where
        // `BREAKS_OFF_BELOW` says a fight ends. The armour on a boar's
        // shoulders takes its share of every blow, so what is swung is
        // what has to *land* plus that -- see `Species::hide_armour`.
        //
        // **A fraction of its health rather than ten of its fourteen.**
        // The rule this is about is a fraction (`BREAKS_OFF_BELOW`), and
        // a fixed ten stopped meaning "beaten" the day
        // `animals::TOUGHNESS` multiplied every row in the table: ten off
        // a boar of seventy is a scratch, and the test reported a boar
        // coming back for more at a quarter health when it was standing
        // at six sevenths of it. `blow_taking` is what turns the fraction
        // into a swing, so the hide and `animals::WEAPON_BITE` in front of
        // it are stated once and not here.
        let taken = Species::Boar.blow_taking(Species::Boar.health() * 0.7);
        animals.strike(id, (1.0, 21.6, 0.5), 8.0, taken);

        let at = (1.0, 21.0, 0.5);
        let before = apart(animals.find(id).expect("alive").at(), at);
        let mut charged = false;
        for _ in 0..80 {
            animals.step(&world, &player(at), 0.05, NOON);
            charged |= animals.find(id).expect("alive").mind == Mind::Charge;
        }
        assert!(!charged, "it came back for more at a quarter health");
        let boar = animals.find(id).expect("alive");
        assert!(
            apart(boar.at(), at) > before + 2.0,
            "it broke off and stayed: {before} -> {}",
            apart(boar.at(), at)
        );
    }

    #[test]
    fn a_boar_with_most_of_its_health_still_fights() {
        // The other side of the rule above: breaking off must be what a
        // beaten animal does, not what every animal does the moment it
        // is touched.
        let world = meadow(60);
        let mut animals = Animals::seeded(12);
        let id = animals.spawn(Species::Boar, (4.0, 21.0, 0.5)).expect("boar");
        animals.strike(id, (1.0, 21.6, 0.5), 8.0, 2.0);
        let mut came = false;
        for _ in 0..80 {
            animals.step(&world, &player((1.0, 21.0, 0.5)), 0.05, NOON);
            if animals.find(id).expect("alive").mind == Mind::Charge {
                came = true;
                break;
            }
        }
        assert!(came, "a scratch sent it home");
    }

    #[test]
    fn the_herd_settles_after_dark() {
        // Not sleep -- they still startle -- but a night with the same
        // amount of wandering in it as noon is a clock nothing in the
        // world can tell the time by.
        const MIDNIGHT: f32 = 0.0;
        let idling = |hour: f32| {
            // **The spawner is held off, rather than dodged.** What has
            // to be kept out of this measurement is arrivals -- what the
            // spawner added over the minute was, at night, wolves: they
            // hunted, the deer they found ran, and a test about deer
            // standing still was measuring how well a pack hunts.
            //
            // That used to be arranged geometrically, by standing the
            // watcher 88 blocks off a 32-block meadow so that the whole
            // spawn ring fell on unloaded ground. Two numbers moved when
            // the animals were made rare and it stopped working: the
            // ring is 36 to 72 blocks now (`SPAWN_MIN`), which reaches
            // the meadow from there, and deer walk a fifth faster, so
            // one of them crossed `DESPAWN_DISTANCE` inside the minute
            // and the herd came back five. There is no watcher position
            // that satisfies both any more.
            //
            // So the fixture says what it means: no spawns, and a
            // watcher close enough that nothing is ever forgotten and
            // far enough (fifty blocks, against a deer's awareness of
            // twelve) that nothing is ever frightened.
            let world = meadow(34);
            let mut animals = Animals::seeded(13);
            animals.next_spawn = f32::INFINITY;
            // Twelve and a half apart: outside `HERD_RADIUS`, so six
            // separate deer rather than one herd being pulled together.
            let herd: Vec<EntityId> = (0..6)
                .map(|i| {
                    animals
                        .spawn(Species::Deer, (i as f32 * 12.5 - 31.25, 21.0, 0.5))
                        .expect("deer")
                })
                .collect();
            let watcher = player((0.5, 21.0, 50.0));
            let (mut idle, mut total) = (0u32, 0u32);
            for _ in 0..1200 {
                animals.step(&world, &watcher, 0.05, hour);
                assert_eq!(animals.len(), 6, "something spawned or something was forgotten");
                for animal in animals.animals.iter().filter(|a| herd.contains(&a.id)) {
                    total += 1;
                    idle += u32::from(animal.mind == Mind::Idle);
                }
            }
            f64::from(idle) / f64::from(total)
        };
        let night = idling(MIDNIGHT);
        let day = idling(NOON);
        assert!(
            night > day + 0.15,
            "the herd keeps the same hours all round the clock: {night} at night, {day} by day"
        );
    }

    #[test]
    fn an_animal_that_walks_into_something_thinks_again_soon() {
        // It used to stand against whatever stopped it until its next
        // scheduled thought, which in the middle of a bolt is most of
        // three seconds of a deer pressed into a rock face.
        let world = meadow(20);
        for z in -6..=6 {
            for y in 21..=22 {
                world.put(4, y, z, BLOCK_STONE);
            }
        }
        let mut animals = Animals::seeded(14);
        let id = animals.spawn(Species::Deer, (2.5, 21.0, 0.5)).expect("deer");
        // Point it at the wall and let it walk.
        {
            let deer = animals.animals.iter_mut().find(|a| a.id == id).expect("alive");
            deer.mind = Mind::Wander;
            deer.yaw = 0.0;
            deer.wants_yaw = 0.0;
            deer.next_thought = 5.0;
        }
        for _ in 0..40 {
            animals.step(&world, &player((0.5, 21.0, 60.0)), 0.05, NOON);
        }
        // Two seconds. Without the rethink its heading is whatever it
        // was told to face five seconds ago, because nothing else can
        // change it.
        let deer = animals.find(id).expect("alive");
        assert!(
            !(deer.mind == Mind::Wander && deer.wants_yaw == 0.0),
            "it is still walking into the wall, waiting for a thought three seconds off"
        );
    }

    #[test]
    fn a_wolf_hunts_a_deer_with_no_player_anywhere_near() {
        // **The change that makes these animals a world.** Nobody is
        // watching, nobody is being chased, and the wolf still has
        // something to do -- which is the whole difference between an
        // ecosystem and four kinds of furniture standing in a field.
        let world = meadow(80);
        let mut animals = Animals::seeded(41);
        let wolf = animals.spawn(Species::Wolf, (0.5, 21.0, 0.5)).expect("wolf");
        let deer = animals.spawn(Species::Deer, (12.5, 21.0, 0.5)).expect("deer");
        // Far off, and out of everyone's awareness.
        let watcher = player((0.5, 21.0, 80.0));

        let start = apart(
            animals.find(wolf).expect("alive").at(),
            animals.find(deer).expect("alive").at(),
        );
        let mut hunted = false;
        let mut chased = false;
        let mut deer_ran = false;
        for _ in 0..400 {
            animals.step(&world, &watcher, 0.05, NOON);
            let Some(w) = animals.find(wolf) else { break };
            hunted |= w.quarry == Some(deer);
            chased |= matches!(w.mind, Mind::Chase | Mind::Charge);
            deer_ran |= animals.find(deer).is_none_or(|d| d.mind == Mind::Flee);
        }
        assert!(hunted, "the wolf never noticed the deer");
        assert!(deer_ran, "the deer grazed beside a hunting wolf");
        // ...and it ran rather than only thinking about it. Whether it
        // *caught* the deer is not this test's business -- a deer that
        // gets clean away is a legitimate end to a chase, and which one
        // happens depends on where the two of them started.
        let _ = start;
        assert!(chased, "the wolf never broke into a run after it");
    }

    #[test]
    fn a_deer_runs_from_a_wolf_the_way_it_runs_from_a_person() {
        let world = meadow(80);
        let mut animals = Animals::seeded(42);
        let deer = animals.spawn(Species::Deer, (0.5, 21.0, 0.5)).expect("deer");
        animals.spawn(Species::Wolf, (8.5, 21.0, 0.5)).expect("wolf");
        let watcher = player((0.5, 21.0, 80.0));
        let mut fled = false;
        for _ in 0..60 {
            animals.step(&world, &watcher, 0.05, NOON);
            fled |= animals.find(deer).is_some_and(|d| d.mind == Mind::Flee);
        }
        assert!(fled, "a deer stood still while a wolf walked up to it");
    }

    #[test]
    fn a_fed_wolf_leaves_the_deer_alone() {
        // The stomach, and it is population control as much as realism: a
        // pack that hunted without pause would clear the spawn radius and
        // leave the player in an empty world for reasons they cannot see.
        let world = meadow(80);
        let mut animals = Animals::seeded(43);
        let wolf = animals.spawn(Species::Wolf, (0.5, 21.0, 0.5)).expect("wolf");
        let deer = animals.spawn(Species::Deer, (6.5, 21.0, 0.5)).expect("deer");
        {
            let w = animals.animals.iter_mut().find(|a| a.id == wolf).expect("alive");
            w.fed_for = Species::Wolf.fed_seconds();
        }
        let watcher = player((0.5, 21.0, 80.0));
        for _ in 0..200 {
            animals.step(&world, &watcher, 0.05, NOON);
            assert!(
                animals.find(wolf).is_some_and(|w| w.quarry.is_none()),
                "a wolf that had just eaten went hunting"
            );
            assert!(animals.find(deer).is_some(), "it ate a second deer on a full stomach");
        }
        // ...and the deer is not spending its life running from something
        // that is not coming.
        assert_ne!(
            animals.find(deer).expect("alive").mind,
            Mind::Flee,
            "the deer is fleeing a wolf that is asleep"
        );
    }

    #[test]
    fn a_hunt_that_lands_kills_the_deer_and_fills_the_wolf() {
        // Set up the moment of the catch directly: the chase itself is
        // the two tests above, and what this one is about is what
        // happens when the teeth arrive.
        //
        // **A meadow the deer cannot outrun the despawner across.** It
        // was eighty blocks with the watcher at the far edge of it, and
        // once the deer ran at 6.9 instead of 6.2 the chase carried it
        // past `DESPAWN_DISTANCE` from the only player -- so the deer
        // vanished, `find` said it was gone, and the test read that as a
        // kill. Forty blocks with the watcher halfway out keeps every
        // corner of it inside ninety-six, and the spawner is held off so
        // nothing else wanders into the chase.
        let world = meadow(40);
        let mut animals = Animals::seeded(44);
        animals.next_spawn = f32::INFINITY;
        let wolf = animals.spawn(Species::Wolf, (0.5, 21.0, 0.5)).expect("wolf");
        let deer = animals.spawn(Species::Deer, (1.6, 21.0, 0.5)).expect("deer");
        {
            let d = animals.animals.iter_mut().find(|a| a.id == deer).expect("alive");
            d.health = 1.0; // one bite from the end
        }
        {
            let w = animals.animals.iter_mut().find(|a| a.id == wolf).expect("alive");
            w.next_thought = 0.0;
        }
        animals.face_for_test(wolf, 0.0); // east, which is where the deer is
        // Outside a wolf's awareness (eighteen) and near enough that
        // nothing in the meadow is ever forgotten.
        let watcher = player((0.5, 21.0, 30.0));
        // **The wolf is pointed at its dinner and allowed to think on the
        // first tick**, which is the difference between staging the
        // moment of the catch and staging a chase and hoping.
        //
        // `bite` needs three things at once -- the quarry inside
        // `BITE_RANGE`, inside `GORE_ARC` of the wolf's nose, and a tick
        // on which this wolf actually thinks, because `seen.quarry` is
        // only gathered on those. A wolf spawned facing a random bearing
        // met all three by luck, and once the deer ran at 6.9 instead of
        // 6.2 the luck ran out: the first pass missed, the chase went on
        // for twenty seconds, and it ended with both of them off the far
        // edge of the meadow. Setting the facing is not weakening the
        // test -- which way an animal happens to have been spawned
        // pointing is exactly what it is not about.
        for _ in 0..120 {
            animals.step(&world, &watcher, 0.05, NOON);
            if animals.find(deer).is_none() {
                break;
            }
        }
        assert!(animals.find(deer).is_none(), "the wolf never closed its jaws");
        let wolf = animals.find(wolf).expect("alive");
        assert!(wolf.fed_for > 0.0, "it made a kill and stayed hungry");
        assert_eq!(wolf.quarry, None, "it is still chasing something that is gone");
        // The kill counts, and it leaves nothing on the ground -- see
        // `settle_bites`.
        let (_, killed, _) = animals.stats();
        assert_eq!(killed, 1);
    }

    #[test]
    fn a_pack_fans_out_and_a_lone_animal_aims_straight() {
        // The bug this test exists for: the first version of the lane
        // gave *every* charge a sideways offset, including a boar's, and
        // a charge aimed two metres to the left of a person arrives with
        // its flank forward and cannot land a blow at all.
        let alone = Neighbours {
            centre: None,
            company: 0,
            alarm: None,
            quarry: None,
            threat: None,
            packmate: None,
            leader: None,
            straggler: None,
        };
        let crowded = Neighbours { company: 2, ..alone };
        let mut animals = Animals::seeded(45);
        let boar = animals.spawn(Species::Boar, (0.5, 21.0, 0.5)).expect("boar");
        let wolf = animals.spawn(Species::Wolf, (0.5, 21.0, 4.5)).expect("wolf");
        let lane_of = |animals: &Animals, id, seen: &Neighbours| {
            pack_lane(animals.find(id).expect("alive"), seen)
        };
        assert_eq!(lane_of(&animals, boar, &crowded), 0.0, "a boar took a lane");
        assert_eq!(lane_of(&animals, wolf, &alone), 0.0, "a lone wolf took a lane");
        // In company it takes one -- which side does not matter, only
        // that it is off the line.
        assert_ne!(lane_of(&animals, wolf, &crowded), 0.0, "a pack ran single file");
    }

    #[test]
    fn a_grazing_animal_walks_toward_something_to_eat() {
        // A wander aimed at nothing is a random walk with an animal drawn
        // on it. This is the same cost and reads as foraging.
        let world = TestWorld::default();
        // Bare stone, so nothing counts as food except the one patch.
        for z in -20..=20 {
            for x in -20..=20 {
                world.put(x, 20, z, BLOCK_STONE);
                for y in 21..30 {
                    world.put(x, y, z, BLOCK_AIR);
                }
            }
        }
        // A stand of grass six blocks off, along +X and nowhere else.
        for z in -1..=1 {
            for x in 5..=7 {
                world.put(x, 20, z, BLOCK_GRASS);
                world.put(x, 21, z, primitive_shared::types::BLOCK_TALL_GRASS);
            }
        }
        let mut animals = Animals::seeded(46);
        let id = animals.spawn(Species::Deer, (0.5, 21.0, 0.5)).expect("deer");
        let heading = {
            let deer = animals.find(id).expect("alive");
            food_heading(&world, deer)
        };
        let heading = heading.expect("it could not find a meadow six metres away");
        // +X is a yaw of zero; the grass is that way and nothing else is.
        assert!(
            heading.cos() > 0.7,
            "it set off away from the only food on the map: yaw {heading}"
        );
    }

    #[test]
    fn a_lone_wolf_follows_and_does_not_bite() {
        // The rule that keeps a predator from making the whole map
        // hostile: what a player sees is one animal, and one animal is
        // something they can walk past.
        let world = meadow(60);
        let mut animals = Animals::seeded(31);
        let id = animals.spawn(Species::Wolf, (4.0, 21.0, 0.5)).expect("wolf");
        let mut blows = Vec::new();
        let mut charged = false;
        for _ in 0..400 {
            blows.extend(animals.step(&world, &player((1.0, 21.0, 0.5)), 0.05, NOON));
            charged |= animals.find(id).expect("alive").mind == Mind::Charge;
        }
        assert!(!charged, "one wolf took on a person by itself");
        assert!(blows.is_empty(), "it bit somebody: {blows:?}");
        // ...and it has not simply wandered off either. It is watching.
        let wolf = animals.find(id).expect("alive");
        assert!(
            apart(wolf.at(), (1.0, 21.0, 0.5)) < Species::Wolf.awareness(),
            "it lost interest entirely: {:?}",
            wolf.at()
        );
    }

    #[test]
    fn two_wolves_are_a_pack_and_a_pack_comes() {
        // The other half. Nothing about either wolf changed -- what
        // changed is that there are two of them.
        let world = meadow(60);
        let mut animals = Animals::seeded(32);
        let id = animals.spawn(Species::Wolf, (4.0, 21.0, 0.5)).expect("wolf");
        animals.spawn(Species::Wolf, (4.0, 21.0, 3.5)).expect("wolf");

        let mut charged = false;
        for _ in 0..200 {
            animals.step(&world, &player((1.0, 21.0, 0.5)), 0.05, NOON);
            if animals.find(id).expect("alive").mind == Mind::Charge {
                charged = true;
                break;
            }
        }
        assert!(charged, "two wolves stood and watched");
    }

    #[test]
    fn a_wolf_that_has_been_hit_does_not_wait_for_help() {
        // Nerve from a grudge rather than from numbers. Without this a
        // lone wolf is something a player can hit for free, which is a
        // predator you farm rather than one you avoid.
        let world = meadow(60);
        let mut animals = Animals::seeded(33);
        let id = animals.spawn(Species::Wolf, (5.0, 21.0, 0.5)).expect("wolf");
        animals.strike(id, (1.0, 21.6, 0.5), 8.0, 1.0);
        let mut charged = false;
        for _ in 0..200 {
            animals.step(&world, &player((1.0, 21.0, 0.5)), 0.05, NOON);
            if animals.find(id).expect("alive").mind == Mind::Charge {
                charged = true;
                break;
            }
        }
        assert!(charged, "a wolf that was speared went back to grazing");
    }

    #[test]
    fn the_spawner_makes_mostly_prey_and_more_wolves_after_dark() {
        let counts = |hour: f32, seed: u64| {
            let mut animals = Animals::seeded(seed);
            let night = is_night(hour);
            let mut wolves = 0;
            for _ in 0..400 {
                // In a meadow: the wolf's country. The savanna's own version
                // of this is `the_savanna_spawns_zebra_antelope_and_lions_and_nothing_from_the_woods`.
                match animals.pick_species(night, primitive_shared::worldgen::Biome::Plains) {
                    Some(Species::Wolf) => wolves += 1,
                    Some(_) => {}
                    None => panic!("nothing spawns"),
                }
            }
            wolves
        };
        let by_day = counts(NOON, 34);
        let by_night = counts(0.0, 34);
        assert!(
            by_night > by_day,
            "the night is no different: {by_day} wolves by day, {by_night} by night"
        );
        assert!(by_night < 200, "more than half of everything alive is a wolf");
    }

    #[test]
    fn a_wood_has_more_wolves_and_bears_than_a_meadow_and_is_still_mostly_prey() {
        // "медведи и волки в лесах очень редкие или их нету вовсе".
        let predators = |biome, night: bool| {
            let mut animals = Animals::seeded(77);
            let mut hunters = 0;
            for _ in 0..2000 {
                if matches!(animals.pick_species(night, biome), Some(Species::Wolf | Species::Bear)) {
                    hunters += 1;
                }
            }
            hunters
        };
        use primitive_shared::worldgen::Biome;
        let (wood, meadow) = (predators(Biome::Forest, false), predators(Biome::Plains, false));
        assert!(wood > meadow * 2, "a wood by day has {wood} predators in 2000 against a meadow's {meadow}");
        assert!(wood > 400, "a wood by day is still nearly safe: {wood} predators in 2000");
        let night = predators(Biome::Forest, true);
        assert!(night < 800, "a wood at night is {night} predators in 2000: a siege, not a night");
    }

    #[test]
    fn an_animal_stands_on_the_ground_rather_than_in_it_or_over_it() {
        let world = meadow(20);
        let mut animals = Animals::seeded(1);
        let id = animals.spawn(Species::Deer, (0.5, 25.0, 0.5)).expect("spawned");
        for _ in 0..200 {
            animals.step(&world, &player((0.5, 21.0, 30.0)), 0.05, NOON);
        }
        let deer = animals.find(id).expect("still there");
        assert!(
            (deer.at().1 - 21.0).abs() < 0.01,
            "it settled at y={} rather than on the turf at 21",
            deer.at().1
        );
    }

    #[test]
    fn a_boar_watches_from_a_distance_and_is_left_alone() {
        // **The rule the whole animal turns on.** A boar that charged
        // everything it could see made the wood it stood in impassable:
        // there was no distance at which a player could decide to leave
        // one be. At seven metres it notices you and stops; at three it
        // takes offence.
        let world = meadow(40);
        let mut animals = Animals::seeded(21);
        let id = animals.spawn(Species::Boar, (6.0, 21.0, 0.5)).expect("boar");

        // Five metres away: inside its awareness, outside its patience.
        for _ in 0..200 {
            animals.step(&world, &player((1.0, 21.0, 0.5)), 0.05, NOON);
        }
        let boar = animals.find(id).expect("alive");
        assert_eq!(boar.mind, Mind::Watch, "it charged somebody who kept their distance");
        // ...and it is looking at them rather than wandering off.
        assert!(boar.yaw.cos() < 0.0, "it is watching in the wrong direction");

        // Now walk into it.
        for _ in 0..40 {
            let at = animals.find(id).expect("alive").at();
            animals.step(&world, &player((at.0 - 1.5, at.1, at.2)), 0.05, NOON);
        }
        assert!(
            matches!(
                animals.find(id).expect("alive").mind,
                Mind::Charge | Mind::Recover
            ),
            "it ignored somebody standing on top of it"
        );
    }

    #[test]
    fn a_charge_runs_at_the_ground_rather_than_at_the_player() {
        // What makes it dodgeable: the boar commits to a patch of earth
        // and cannot steer once it has. A charge that tracked the player
        // is a charge nobody can do anything about but run.
        let world = meadow(60);
        let mut animals = Animals::seeded(22);
        let id = animals.spawn(Species::Boar, (10.0, 21.0, 0.5)).expect("boar");

        // Provoke it from close range, then step aside.
        for _ in 0..30 {
            animals.step(&world, &player((9.0, 21.0, 0.5)), 0.05, NOON);
        }
        assert_eq!(animals.find(id).expect("alive").mind, Mind::Charge);

        // The player is now well off the line of the charge. Half a
        // second of it: the boar should carry on the way it was going
        // and not swing round after them.
        let before = animals.find(id).expect("alive").at();
        for _ in 0..10 {
            animals.step(&world, &player((9.0, 21.0, 12.0)), 0.05, NOON);
        }
        let after = animals.find(id).expect("alive").at();
        // It may *curve* -- a charge steers toward the ground it was
        // aimed at, and an animal that had not finished turning when the
        // charge began still has some of that turn left in it. What it
        // must not do is follow: the player is twelve blocks off the
        // line, and the boar should not have gone appreciably that way.
        assert!(
            (after.2 - before.2).abs() < 3.0,
            "it turned after the player mid-charge: z went from {} to {}",
            before.2,
            after.2
        );
        assert!(
            after.0 < before.0 - 0.5,
            "it did not press on with the charge: x went from {} to {}",
            before.0,
            after.0
        );
    }

    #[test]
    fn a_charge_ends_in_a_pause_that_can_be_hit() {
        // The window the fight happens in. A boar with no recovery is a
        // boar you can only run from.
        let world = meadow(40);
        let mut animals = Animals::seeded(23);
        let id = animals.spawn(Species::Boar, (3.0, 21.0, 0.5)).expect("boar");

        let mut recovered = false;
        for _ in 0..200 {
            animals.step(&world, &player((1.0, 21.0, 0.5)), 0.05, NOON);
            if animals.find(id).expect("alive").mind == Mind::Recover {
                recovered = true;
                break;
            }
        }
        assert!(recovered, "it charged for ever without drawing breath");
    }

    #[test]
    fn hitting_one_starts_a_fight_it_eventually_drops() {
        // A grudge, and an end to it. Without the first, backing off
        // four metres cancels a fight you started; without the second,
        // the only way out of one is to kill the animal.
        let world = meadow(80);
        let mut animals = Animals::seeded(24);
        let id = animals.spawn(Species::Boar, (6.0, 21.0, 0.5)).expect("boar");
        animals.strike(id, (1.0, 21.6, 0.5), 8.0, 1.0);
        assert!(animals.find(id).expect("alive").angry_for > 0.0);

        // Well outside its awareness, and it comes anyway.
        let mut came = false;
        for _ in 0..60 {
            animals.step(&world, &player((1.0, 21.0, 0.5)), 0.05, NOON);
            if animals.find(id).expect("alive").mind == Mind::Charge {
                came = true;
                break;
            }
        }
        assert!(came, "a wounded boar shrugged it off");

        // ...and once the grudge runs out it goes back to being an
        // animal in a field. Fifty metres, not three hundred: past
        // three hundred it is not calm, it is *forgotten* -- see
        // `forget_the_distant` -- and a despawned boar would pass this
        // test for the wrong reason.
        for _ in 0..((Species::Boar.grudge_seconds() / 0.05) as usize + 200) {
            animals.step(&world, &player((50.0, 21.0, 0.5)), 0.05, NOON);
        }
        let boar = animals.find(id).expect("alive");
        assert!(
            matches!(boar.mind, Mind::Idle | Mind::Wander),
            "it is still angry at somebody three hundred metres away: {:?}",
            boar.mind
        );
        assert_eq!(boar.angry_for, 0.0);
    }

    #[test]
    fn a_deer_runs_from_a_player_and_a_boar_comes_at_one() {
        let world = meadow(40);
        let watcher = player((0.5, 21.0, 0.5));

        let mut running = Animals::seeded(2);
        let deer = running.spawn(Species::Deer, (6.5, 21.0, 0.5)).expect("deer");
        // The boar starts inside its own provoking range, because a boar
        // at six metres now watches rather than charges -- which is the
        // point of `a_boar_watches_from_a_distance_and_is_left_alone`.
        let mut charging = Animals::seeded(2);
        let boar = charging.spawn(Species::Boar, (2.6, 21.0, 0.5)).expect("boar");

        let distance = |a: &Animals, id: EntityId| {
            let at = a.find(id).expect("alive").at();
            ((at.0 - 0.5).powi(2) + (at.2 - 0.5).powi(2)).sqrt()
        };
        let deer_before = distance(&running, deer);
        let boar_before = distance(&charging, boar);

        // The *closest* the boar got, not where it ended up: a charge
        // overshoots by three metres, so an animal that ran straight
        // over the player finishes further away than it started. See
        // `start_charge`.
        let mut boar_closest = boar_before;
        for _ in 0..80 {
            running.step(&world, &watcher, 0.05, NOON);
            charging.step(&world, &watcher, 0.05, NOON);
            boar_closest = boar_closest.min(distance(&charging, boar));
        }

        assert!(
            distance(&running, deer) > deer_before + 1.0,
            "the deer did not run"
        );
        assert!(
            boar_closest < boar_before - 1.0,
            "the boar did not come: it got no nearer than {boar_closest} of {boar_before}"
        );
    }

    #[test]
    fn a_boar_that_reaches_you_hurts_you_and_then_waits() {
        let world = meadow(20);
        let mut animals = Animals::seeded(3);
        animals.spawn(Species::Boar, (1.0, 21.0, 0.5)).expect("boar");
        let victim = player((0.5, 21.0, 0.5));

        let mut blows = Vec::new();
        for _ in 0..40 {
            blows.extend(animals.step(&world, &victim, 0.05, NOON));
        }
        assert!(!blows.is_empty(), "a boar standing on somebody did nothing");
        assert_eq!(blows[0].victim, 1);
        assert_eq!(blows[0].damage, Species::Boar.damage());
        // ...and it says what did it, or the death message names the
        // wrong animal.
        assert_eq!(blows[0].species, Species::Boar);

        // ...and then it waits. Two seconds of ticks at a two-second
        // interval is one more blow at the very most -- not forty.
        let mut again = Vec::new();
        for _ in 0..40 {
            again.extend(animals.step(&world, &victim, 0.05, NOON));
        }
        assert!(
            again.len() <= 1,
            "it landed {} more blows in two seconds",
            again.len()
        );
    }

    #[test]
    fn nothing_that_runs_ever_hits_anybody() {
        let world = meadow(20);
        for species in [Species::Hare, Species::Deer] {
            let mut animals = Animals::seeded(4);
            animals.spawn(species, (0.6, 21.0, 0.5)).expect("spawned");
            let victim = player((0.5, 21.0, 0.5));
            let mut blows = Vec::new();
            for _ in 0..200 {
                blows.extend(animals.step(&world, &victim, 0.05, NOON));
            }
            assert!(blows.is_empty(), "a {} attacked somebody", species.name());
        }
    }

    #[test]
    fn hitting_one_enough_times_kills_it_and_leaves_something() {
        let mut animals = Animals::seeded(5);
        let id = animals.spawn(Species::Hare, (0.5, 21.0, 0.5)).expect("hare");
        let from = (0.5, 21.6, 0.5);

        assert_eq!(animals.strike(id, from, 3.0, 1.0), Struck::Hurt);
        assert_eq!(animals.len(), 1);

        // One point a blow until it goes, and the bound is the hare's own
        // health rather than ten: `animals::TOUGHNESS` made a hare twenty
        // points, and ten blows of one left it standing with the test
        // reporting that it never died.
        let mut killed = None;
        for _ in 0..(Species::Hare.health().ceil() as usize + 2) {
            if let Struck::Killed { species, at } = animals.strike(id, from, 3.0, 1.0) {
                killed = Some((species, at));
                break;
            }
        }
        let (species, at) = killed.expect("the hare never died");
        // What it says died is what died, and where it says so is where
        // it stood -- the carcass goes in that cell (`lib::lay_carcass`).
        assert_eq!(species, Species::Hare);
        assert!((at.0 - 0.5).abs() < 1.0 && (at.2 - 0.5).abs() < 1.0);
        assert!(animals.is_empty());
        assert_eq!(animals.stats().1, 1);
        // ...and a swing at a corpse is a miss rather than a second kill.
        assert_eq!(animals.strike(id, from, 3.0, 1.0), Struck::Missed);
    }

    #[test]
    fn a_swing_from_across_the_field_misses() {
        let mut animals = Animals::seeded(6);
        let id = animals.spawn(Species::Deer, (0.5, 21.0, 0.5)).expect("deer");
        assert_eq!(animals.strike(id, (40.0, 21.6, 40.0), 4.0, 5.0), Struck::Missed);
        assert_eq!(animals.len(), 1, "a missed swing killed it anyway");
    }

    #[test]
    fn a_struck_boar_faces_the_blow_rather_than_turning_its_back() {
        // **What "the animals are stupid" was.** A hit turned everything
        // round -- flee, facing away -- so a boar spent a fight
        // pirouetting: hit, spin away, think, spin back, gore, hit, spin
        // away. It stands its ground now: rocked back, facing whoever
        // landed it, and coming again on the thought after.
        let world = meadow(30);
        let mut animals = Animals::seeded(7);
        let id = animals.spawn(Species::Boar, (4.5, 21.0, 0.5)).expect("boar");
        animals.strike(id, (0.5, 21.6, 0.5), 6.0, 1.0);
        let struck = animals.find(id).expect("alive");
        assert_eq!(struck.mind, Mind::Recover, "it ran from one blow");
        // Facing the player at (0.5, 0.5) from (4.5, 0.5) is facing
        // along -x, which is a yaw of pi.
        let facing = (struck.yaw.cos(), struck.yaw.sin());
        assert!(
            facing.0 < -0.7,
            "it is facing ({:.2}, {:.2}) rather than at the player",
            facing.0,
            facing.1
        );
        // ...and it was shoved, which is what a landed blow has to look
        // like now that it does not spin.
        assert!(struck.velocity.0 > 0.5, "the blow moved nothing");

        // ...and once the flinch is over, a player still in front of it
        // is a player it comes at again -- and a boar it has been hit by
        // is one it comes at wherever they go, for as long as the grudge
        // lasts.
        for _ in 0..((FLINCH_SECONDS / 0.05) as usize + 40) {
            let at = animals.find(id).expect("alive").at();
            animals.step(&world, &player((at.0 - 2.0, at.1, at.2)), 0.05, NOON);
        }
        assert!(
            matches!(
                animals.find(id).expect("alive").mind,
                Mind::Charge | Mind::Recover
            ),
            "a wounded boar did not come back"
        );
    }

    #[test]
    fn a_struck_deer_runs_and_a_beaten_boar_runs_too() {
        // The other half of the rule. Prey never stands, and neither
        // does anything that has had enough: a boar that fought to the
        // death every time would be a boar with no way out of a fight it
        // is losing -- see `BREAKS_OFF_BELOW`.
        let mut animals = Animals::seeded(8);
        let deer = animals.spawn(Species::Deer, (4.5, 21.0, 0.5)).expect("deer");
        animals.strike(deer, (0.5, 21.6, 0.5), 6.0, 1.0);
        assert_eq!(animals.find(deer).expect("alive").mind, Mind::Flee);

        let boar = animals.spawn(Species::Boar, (4.5, 21.0, 8.5)).expect("boar");
        // `blow_taking` puts back what the blow loses on the way in: the
        // armour every hit pays first (`Species::hide_armour`) and the
        // bite it is multiplied by (`animals::WEAPON_BITE`).
        let nearly =
            Species::Boar.blow_taking(Species::Boar.health() * (1.0 - BREAKS_OFF_BELOW) + 0.1);
        animals.strike(boar, (0.5, 21.6, 8.5), 6.0, nearly);
        assert_eq!(
            animals.find(boar).expect("alive").mind,
            Mind::Flee,
            "a boar down to its last third stood and fought"
        );
    }

    #[test]
    fn an_animal_stops_at_the_water_rather_than_walking_over_it() {
        // Buoyancy on its own floats an animal to the surface and leaves
        // it strolling across the top of a lake, because nothing
        // horizontal is in its way -- which from the shore looks exactly
        // like an animal that cannot tell water from ground.
        use primitive_shared::types::BLOCK_WATER;
        let world = meadow(40);
        // A lake filling everything past x = 4.
        for z in -40..=40 {
            for x in 5..40 {
                world.put(x, 20, z, BLOCK_WATER);
                world.put(x, 21, z, BLOCK_WATER);
            }
        }

        let mut animals = Animals::seeded(31);
        let id = animals.spawn(Species::Deer, (0.5, 21.0, 0.5)).expect("deer");
        // Driven at the water by a player standing on the dry side.
        for _ in 0..400 {
            animals.step(&world, &player((-6.0, 21.0, 0.5)), 0.05, NOON);
        }
        let at = animals.find(id).expect("alive").at();
        assert!(
            at.0 < 6.0,
            "it walked {:.1} blocks out into the lake",
            at.0 - 5.0
        );
    }

    /// A meadow with a lake filling everything from `edge` eastward.
    ///
    /// Two blocks of water on a stone bed, so its surface (y = 21) is
    /// level with the ground an animal stands on beside it -- which is
    /// what a pond in a meadow looks like and what the drinking tests
    /// want. `meadow_with_a_banked_lake` is the awkward version.
    fn meadow_with_a_lake(span: i32, edge: i32) -> TestWorld {
        use primitive_shared::types::BLOCK_WATER;
        let world = meadow(span);
        for z in -span..=span {
            for x in edge..=span {
                world.put(x, 19, z, BLOCK_STONE);
                world.put(x, 20, z, BLOCK_WATER);
                world.put(x, 21, z, BLOCK_WATER);
            }
        }
        world
    }

    #[test]
    fn a_thirsty_deer_walks_to_the_lake_and_not_into_it() {
        // **Both halves of the mechanic in one statement.** A deer that
        // never went to water is scenery that happens to be hungry; a
        // deer that walked into the lake to reach the water is the bug
        // the no-swimming rule exists to prevent, arriving through the
        // front door. So: it must get to the bank, it must stop there,
        // and its head must go down (`Mind::Drink`).
        let world = meadow_with_a_lake(40, 5);
        let mut animals = Animals::seeded(71);
        let id = animals.spawn(Species::Deer, (-8.5, 21.0, 0.5)).expect("deer");
        {
            let deer = animals.animals.iter_mut().find(|a| a.id == id).expect("alive");
            deer.thirst = THIRSTY_AT + 5.0;
        }
        // Somebody has to be online or the world forgets the deer, and
        // he must be too far off to be worth running from -- thirst is
        // only ever a thing an unbothered animal does.
        let watcher = player((-8.5, 21.0, 80.0));

        let mut drank = false;
        for _ in 0..600 {
            animals.step(&world, &watcher, 0.05, NOON);
            let deer = animals.find(id).expect("alive");
            assert!(
                !enters_liquid(&world, primitive_shared::geometry::wide(deer.at())),
                "it waded in to drink: {:?}",
                deer.at()
            );
            drank |= deer.mind == Mind::Drink;
        }
        assert!(drank, "a thirsty deer twelve blocks from a lake never went to it");
        let deer = animals.find(id).expect("alive");
        assert!(
            deer.at().0 > 2.0,
            "it never reached the bank at x = 5: x = {:.1}",
            deer.at().0
        );
        // ...and the drink was worth taking: it is no longer thirsty.
        assert!(
            deer.thirst < THIRSTY_AT,
            "it stood at the water for ten seconds and is as thirsty as it was: {:.0}",
            deer.thirst
        );
    }

    #[test]
    fn a_thirsty_animal_runs_from_a_wolf_rather_than_finishing_its_drink() {
        // Thirst must be the *last* thing an animal thinks about. It is
        // reached from `graze` alone, which is only called when there is
        // nothing to run from -- this is the test that says so, because
        // "it is only called from there" is a fact about today's code
        // and the rule is about the animal.
        let world = meadow_with_a_lake(60, 5);
        let mut animals = Animals::seeded(72);
        let deer = animals.spawn(Species::Deer, (2.5, 21.0, 0.5)).expect("deer");
        {
            let d = animals.animals.iter_mut().find(|a| a.id == deer).expect("alive");
            d.thirst = THIRST_CAP;
        }
        let watcher = player((2.5, 21.0, 80.0));
        // Let it get its head down first, so what the wolf interrupts is
        // an actual drink.
        let mut drank = false;
        for _ in 0..200 {
            animals.step(&world, &watcher, 0.05, NOON);
            drank |= animals.find(deer).expect("alive").mind == Mind::Drink;
            if drank {
                break;
            }
        }
        assert!(drank, "it never started drinking, so there is nothing to interrupt");
        animals.spawn(Species::Wolf, (-4.5, 21.0, 0.5)).expect("wolf");
        let mut fled = false;
        for _ in 0..100 {
            animals.step(&world, &watcher, 0.05, NOON);
            let Some(d) = animals.find(deer) else { break };
            fled |= d.mind == Mind::Flee;
        }
        assert!(fled, "it went on drinking with a wolf walking up to it");
    }

    #[test]
    fn an_animal_that_ends_up_in_a_lake_makes_for_the_shore() {
        // **What water used to be to an animal that was in it: nothing
        // at all.** Buoyancy held it at the surface and it went on
        // wandering on whatever heading it last picked, at a third of
        // its speed -- so a deer that was pushed, spawned or chased into
        // a pond milled about in the middle of it until it happened to
        // drift against a bank. It now looks for the nearest dry ground
        // and goes there.
        let world = meadow_with_a_lake(40, 5);
        let mut animals = Animals::seeded(73);
        // Nine blocks out into the water, which is well past drifting
        // distance.
        let id = animals.spawn(Species::Deer, (14.5, 21.0, 0.5)).expect("deer");
        let watcher = player((-8.5, 21.0, 60.0));
        for _ in 0..600 {
            animals.step(&world, &watcher, 0.05, NOON);
            if animals.find(id).is_some_and(|d| d.at().0 < 4.5) {
                break;
            }
        }
        let deer = animals.find(id).expect("it drowned");
        // Dry land, judged by where it is standing rather than by
        // whether the water has it at this instant: a floating body bobs
        // in and out of `enters_liquid` several times a second.
        assert!(
            deer.at().0 < 5.0,
            "it is still out in the lake at {:?} after half a minute",
            deer.at()
        );
        assert!(
            !enters_liquid(&world, primitive_shared::geometry::wide(deer.at())),
            "it got to the shore and stood in the shallows: {:?}",
            deer.at()
        );
    }

    #[test]
    fn an_animal_in_a_pond_climbs_the_bank_to_get_out() {
        // The other half of getting out, and the half that is physics
        // rather than a decision: the bank of a pond whose water sits
        // below the ground beside it is a step, and the step-up in
        // `walk` used to be gated on being *on the ground* -- which a
        // floating animal is not. It knew exactly where the shore was
        // and could not climb it.
        use primitive_shared::types::BLOCK_WATER;
        let world = meadow(24);
        // A bank a block high everywhere west of x = 0...
        for z in -24..=24 {
            for x in -24..0 {
                world.put(x, 21, z, BLOCK_GRASS);
            }
        }
        // ...and a pond east of it, its surface a block below the top of
        // the bank.
        for z in -24..=24 {
            for x in 0..=24 {
                world.put(x, 19, z, BLOCK_STONE);
                world.put(x, 20, z, BLOCK_WATER);
                world.put(x, 21, z, BLOCK_WATER);
            }
        }
        let mut animals = Animals::seeded(74);
        let id = animals.spawn(Species::Deer, (6.5, 21.0, 0.5)).expect("deer");
        let watcher = player((-8.5, 22.0, 60.0));
        for _ in 0..800 {
            animals.step(&world, &watcher, 0.05, NOON);
            if animals
                .find(id)
                .is_some_and(|d| d.at().1 >= 21.9 && d.at().0 < -0.5)
            {
                break;
            }
        }
        let deer = animals.find(id).expect("it drowned");
        assert!(
            deer.at().1 >= 21.9 && deer.at().0 < -0.5,
            "it never got up the bank: {:?}",
            deer.at()
        );
    }

    #[test]
    fn a_blown_animal_runs_slower_than_a_fresh_one() {
        // The point of the wind, stated as the thing a player feels: a
        // chase that has gone on long enough ends with them closing on
        // the animal instead of trailing it at a fixed distance for as
        // long as the map lasts.
        let ran = |stamina: f32| {
            let world = meadow(40);
            let mut animals = Animals::seeded(75);
            let id = animals.spawn(Species::Deer, (0.5, 21.0, 0.5)).expect("deer");
            {
                let deer = animals.animals.iter_mut().find(|a| a.id == id).expect("alive");
                deer.stamina = stamina;
            }
            let from = (-4.0, 21.0, 0.5);
            let start = animals.find(id).expect("alive").at();
            for _ in 0..40 {
                animals.step(&world, &player(from), 0.05, NOON);
            }
            apart(animals.find(id).expect("alive").at(), start)
        };
        let fresh = ran(Species::Deer.stamina_seconds());
        let blown = ran(0.0);
        assert!(fresh > 1.0, "the fresh deer never ran at all: {fresh:.1} blocks");
        assert!(
            blown < fresh * 0.8,
            "blown it covered {blown:.1} blocks in two seconds and fresh {fresh:.1} -- \
             which is not a difference anybody could see"
        );
    }

    #[test]
    fn running_costs_an_animal_its_wind_and_standing_about_gives_it_back() {
        // A meter that only ever goes down is an animal that is blown
        // for the rest of its life after one fright, and a meter that
        // comes back as fast as it goes is no meter at all. See
        // `STAMINA_RECOVERS`.
        let world = meadow(40);
        let mut animals = Animals::seeded(76);
        let id = animals.spawn(Species::Deer, (0.5, 21.0, 0.5)).expect("deer");
        let full = Species::Deer.stamina_seconds();
        let mut lowest = full;
        for _ in 0..60 {
            animals.step(&world, &player((-3.0, 21.0, 0.5)), 0.05, NOON);
            lowest = lowest.min(animals.find(id).expect("alive").stamina);
        }
        assert!(
            lowest < full - 1.0,
            "it bolted for three seconds and spent {:.1} seconds of wind",
            full - lowest
        );
        // ...and then it is left alone, from far enough away that it has
        // nothing to run from.
        for _ in 0..600 {
            animals.step(&world, &player((0.5, 21.0, 80.0)), 0.05, NOON);
        }
        assert_eq!(
            animals.find(id).expect("alive").stamina,
            full,
            "half a minute in an empty field and it has not got its breath back"
        );
    }

    #[test]
    fn a_chase_makes_an_animal_thirsty_faster_than_a_quiet_afternoon() {
        // Why the two meters are the same mechanism: a herd that has
        // been run across a meadow goes to the water sooner than one
        // nobody bothered, which is what tells a hunter who fluffed the
        // stalk where to wait. See `RUNNING_THIRST`.
        let thirst_after = |chased: bool| {
            let world = meadow(40);
            let mut animals = Animals::seeded(77);
            let id = animals.spawn(Species::Deer, (0.5, 21.0, 0.5)).expect("deer");
            {
                let deer = animals.animals.iter_mut().find(|a| a.id == id).expect("alive");
                deer.thirst = 0.0;
            }
            let at = if chased {
                (-3.0, 21.0, 0.5)
            } else {
                (0.5, 21.0, 80.0)
            };
            for _ in 0..40 {
                animals.step(&world, &player(at), 0.05, NOON);
            }
            animals.find(id).expect("alive").thirst
        };
        let quiet = thirst_after(false);
        let chased = thirst_after(true);
        assert!(
            chased > quiet * 1.5,
            "two seconds of standing about cost {quiet:.1} seconds of thirst and two \
             seconds of running cost {chased:.1}"
        );
    }

    #[test]
    fn animals_walk_up_a_step_and_not_through_a_wall() {
        let world = meadow(20);
        // A wall two blocks high at x = 4, and a single bench at x = -4.
        for z in -20..=20 {
            world.put(4, 21, z, BLOCK_STONE);
            world.put(4, 22, z, BLOCK_STONE);
            world.put(-4, 21, z, BLOCK_STONE);
        }

        // Driven by fleeing a player standing on the other side, which
        // is the most determined an animal ever gets.
        let mut into_the_wall = Animals::seeded(8);
        let id = into_the_wall.spawn(Species::Deer, (2.5, 21.0, 0.5)).expect("deer");
        for _ in 0..300 {
            into_the_wall.step(&world, &player((0.5, 21.0, 0.5)), 0.05, NOON);
        }
        let at = into_the_wall.find(id).expect("alive").at();
        assert!(at.0 < 4.0, "it walked through a wall to x={}", at.0);

        // **And the half this test's name promised and never checked.**
        //
        // It asserted the wall and nothing else for as long as it has
        // existed, which is exactly how an animal that could not get up
        // a one-block bench shipped: the test that was supposed to
        // notice was only ever looking the other way. See
        // `an_animal_gets_up_a_one_block_step`, which is the same
        // statement made on its own so it cannot be half-forgotten
        // again.
        let mut up_the_bench = Animals::seeded(8);
        let id = up_the_bench
            .spawn(Species::Deer, (-2.5, 21.0, 0.5))
            .expect("deer");
        for _ in 0..300 {
            up_the_bench.step(&world, &player((0.5, 21.0, 0.5)), 0.05, NOON);
            if up_the_bench.find(id).is_some_and(|a| a.at().0 < -4.0) {
                break;
            }
        }
        let at = up_the_bench.find(id).expect("alive").at();
        assert!(
            at.0 < -4.0,
            "it never got over the one-block bench -- stopped at x={:.2}, y={:.2}",
            at.0,
            at.1
        );
    }

    #[test]
    fn an_animal_stands_in_an_open_doorway_and_never_in_a_shut_one() {
        // Nothing here opens a door; a door left open is a hole a wolf
        // follows a player through. See `stand_height`.
        use primitive_shared::types::{door_partner, door_swung, faced, Facing, BLOCK_DOOR};
        let world = meadow(20);
        let lower = faced(BLOCK_DOOR, Facing::North);
        let (top_at, top) = door_partner((0, 21, 0), lower).unwrap();
        for (lower, top, open) in [(lower, top, false), (door_swung(lower), door_swung(top), true)] {
            world.put(0, 21, 0, lower);
            world.put(top_at.0, top_at.1, top_at.2, top);
            for species in [Species::Hare, Species::Wolf] {
                assert_eq!(
                    fits(&world, (0.5, 21.0, 0.5), species),
                    open,
                    "a {species:?} in a doorway whose door is {}",
                    if open { "open" } else { "shut" }
                );
            }
        }
    }

    #[test]
    fn an_animal_can_walk_over_a_campfire_the_way_a_player_can() {
        // **The bug this test was written for.** A gravity-collision that
        // stops a fall used to snap the animal's feet to
        // `position.1.floor()` -- correct for a full block, whose top
        // always sits on a whole number, and wrong for anything shorter.
        // Climbing past a campfire (a quarter of a block tall) lifted the
        // animal to a height that cleared it, but the very next tick's
        // fall caught the campfire's *actual* top (a quarter-block up)
        // and floored the position straight through it, embedding the
        // animal in the campfire's own cell. From inside a block, the
        // climb that had just worked cannot run again -- it rises "in
        // place", and "in place" was now the obstruction -- so the
        // animal stood there, shoved to a dead stop, for good. See
        // `resting_height`.
        use primitive_shared::types::BLOCK_CAMPFIRE;
        let world = meadow(20);
        for z in -20..=20 {
            world.put(-4, 21, z, BLOCK_CAMPFIRE);
        }
        let mut animals = Animals::seeded(8);
        let id = animals.spawn(Species::Deer, (-2.5, 21.0, 0.5)).expect("deer");
        for i in 0..300 {
            animals.step(&world, &player((0.5, 21.0, 0.5)), 0.05, NOON);
            if i > 20 && animals.find(id).is_some_and(|a| a.at().0 < -4.0) {
                break;
            }
        }
        let at = animals.find(id).expect("alive").at();
        assert!(
            at.0 < -4.0,
            "it never got past the campfire row -- stopped at x={:.2}, y={:.2}",
            at.0,
            at.1
        );
    }

    #[test]
    fn an_animal_on_the_tread_of_a_step_stands_on_the_tread() {
        // A step's row is a whole cube, and an animal read it as one: a
        // body over the tread was held at the top of the riser, half a
        // block of air under its hooves, and one coming down onto the tread
        // stopped there. The tread and the riser are what it stands on now,
        // as they are for a player (`geometry::step_boxes`).
        use primitive_shared::geometry::STEP_TREAD;
        use primitive_shared::types::{faced, Facing, BLOCK_PLANK_STAIRS};
        let world = meadow(4);
        // North-facing: the low side is -z, the riser at the back, +z.
        world.put(0, 21, 0, faced(BLOCK_PLANK_STAIRS, Facing::North));
        let frame = Frame { width: 0.4, height: 0.8 };
        let over_tread = (0.5, 0.0, (STEP_TREAD * 0.5) as f64);
        let at = |y: f64| (over_tread.0, y, over_tread.2);
        assert!(fits(&world, at(21.5), frame), "a small body on the tread does not fit on it");
        assert!(!fits(&world, at(21.45), frame), "a body sunk into the tread fits");
        let rest = resting_height(&world, at(21.9), 21.4, frame);
        assert!((rest - 21.5).abs() < 1e-6, "a fall onto the tread came to rest at {rest}");
        // ...and over the riser, the riser's top.
        let over_riser = (0.5, 22.3, 0.5 + (STEP_TREAD * 0.5) as f64);
        assert!(!fits(&world, (over_riser.0, 21.5, over_riser.2), frame), "the riser is walked through");
        let rest = resting_height(&world, over_riser, 21.9, frame);
        assert!((rest - 22.0).abs() < 1e-6, "a fall onto the riser came to rest at {rest}");
    }

    #[test]
    fn the_world_fills_up_near_a_player_and_stops() {
        let world = meadow(100);
        let mut animals = Animals::seeded(9);
        let standing = player((0.5, 21.0, 0.5));
        // Ten minutes of standing about. It used to be three, against a
        // spawner that tried every four seconds; `SPAWN_INTERVAL` is
        // twelve now -- which is most of what "rare" means -- so the
        // same number of attempts takes three times as long. The meadow
        // grew with `SPAWN_MAX` for the same reason.
        for _ in 0..12_000 {
            animals.step(&world, &standing, 0.05, NOON);
        }
        assert!(!animals.is_empty(), "a meadow with nothing in it");
        // **A group arrives whole** (`animals::MAX_GROUP`), so the most one
        // player can have is an allowance one short of full and then the
        // biggest group there is.
        assert!(
            animals.len() < MAX_ANIMALS_PER_PLAYER + primitive_shared::animals::MAX_GROUP,
            "{} animals around one player",
            animals.len()
        );
    }

    #[test]
    fn nothing_spawns_where_a_player_could_not_have_expected_it() {
        // Bare stone, no turf: an animal appearing on a rock face or in
        // a cave is an animal that came from nowhere.
        let world = TestWorld::default();
        for z in -80..=80 {
            for x in -80..=80 {
                world.put(x, 20, z, BLOCK_STONE);
            }
        }
        let mut animals = Animals::seeded(10);
        for _ in 0..4000 {
            animals.step(&world, &player((0.5, 21.0, 0.5)), 0.05, NOON);
        }
        assert!(animals.is_empty(), "{} animals spawned on bare rock", animals.len());
    }

    #[test]
    fn a_herd_does_not_move_in_lockstep() {
        // **What "too scripted" looked like.** Every deer walked at
        // exactly its species' speed, turned at exactly the same rate
        // and bent not at all, so four of them crossing a meadow were
        // one animal drawn four times in formation.
        let mut animals = Animals::seeded(51);
        let mut paces = Vec::new();
        for i in 0..6 {
            let id = animals
                .spawn(Species::Deer, (i as f32 * 3.0 + 0.5, 21.0, 0.5))
                .expect("deer");
            let animal = animals.find(id).expect("alive");
            paces.push(animal.pace);
            assert!(
                (0.9..=1.1).contains(&animal.pace),
                "a deer at {:.2} of its species' speed",
                animal.pace
            );
            assert!(animal.drift.abs() <= 0.25, "it wanders in circles");
        }
        // A spread rather than six identical numbers. Not "all
        // different": two of six landing on the same thousandth is a
        // hash doing its job, and a test that forbade it would be a test
        // about the hash rather than about the herd.
        let distinct: std::collections::HashSet<i32> =
            paces.iter().map(|pace| (pace * 1000.0) as i32).collect();
        assert!(
            distinct.len() >= 4,
            "six deer have {} paces between them",
            distinct.len()
        );
    }

    #[test]
    fn an_idle_animal_looks_about_rather_than_freezing() {
        // A statue with a walk cycle is what an animal that stands
        // perfectly still for ten seconds is.
        let world = meadow(30);
        let mut animals = Animals::seeded(52);
        let id = animals.spawn(Species::Deer, (0.5, 21.0, 0.5)).expect("deer");
        let watcher = player((0.5, 21.0, 40.0));
        let mut headings = std::collections::HashSet::new();
        for _ in 0..600 {
            animals.step(&world, &watcher, 0.05, NOON);
            if let Some(animal) = animals.find(id) {
                headings.insert((animal.wants_yaw * 20.0) as i32);
            }
        }
        assert!(
            headings.len() > 1,
            "it faced one direction for half a minute"
        );
    }

    #[test]
    fn an_animal_against_a_wall_walks_round_it() {
        // **What "the AI is stupid" looked like.** Blocked meant stop
        // and reschedule a thought, and the thought picked a heading at
        // random -- so an animal that met a wall stood with its nose
        // against it, twitching, and had an even chance of choosing the
        // wall again. Now it looks for a way past before it stops.
        let world = meadow(40);
        // A wall across the animal's path, two blocks high so nothing
        // steps over it.
        for z in -6..=6 {
            for y in 21..=22 {
                world.put(4, y, z, BLOCK_STONE);
            }
        }
        let mut animals = Animals::seeded(41);
        let id = animals.spawn(Species::Deer, (0.5, 21.0, 0.5)).expect("spawned");
        animals.face_for_test(id, 0.0); // straight at the wall

        let mut walked = 0.0f32;
        for _ in 0..200 {
            animals.step(&world, &player((0.5, 21.0, 0.5)), 0.05, NOON);
            if let Some(state) = animals.states().first() {
                walked = walked.max((state.z as f32 - 0.5).abs());
            }
        }
        assert!(
            walked > 1.0,
            "it never went round the wall: {walked:.2} blocks along it"
        );
    }

    #[test]
    fn a_blow_at_the_flank_of_a_long_animal_lands() {
        // **The bug the oriented box fixes.** A deer is 1.7 long and
        // 0.85 wide; the sphere this replaced was sized to its height,
        // so a player standing at the animal's tail -- with the whole of
        // it drawn under their crosshair -- swung at a point the server
        // said was out of reach. What connected on screen missed on the
        // wire, which is exactly what "the hitboxes are wrong" is.
        let mut animals = Animals::seeded(31);
        let id = animals
            .spawn(Species::Deer, (0.0, 21.0, 0.0))
            .expect("spawned");
        animals.face_for_test(id, 0.0); // nose along +x

        // A metre and a half down the animal's own length, at chest
        // height: inside the deer, however you look at it.
        let along = (Species::Deer.length() * 0.5, 21.7, 0.0);
        assert!(
            !matches!(animals.strike(id, along, 0.2, 1.0), Struck::Missed),
            "a blow inside the animal missed"
        );

        // ...and the same distance out to the side is *not* inside it,
        // because the box is the silhouette rather than a bubble.
        let mut animals = Animals::seeded(31);
        let id = animals
            .spawn(Species::Deer, (0.0, 21.0, 0.0))
            .expect("spawned");
        animals.face_for_test(id, 0.0);
        let beside = (0.0, 21.7, Species::Deer.length() * 0.5);
        assert!(
            matches!(animals.strike(id, beside, 0.2, 1.0), Struck::Missed),
            "a blow a metre out to the side connected"
        );
    }

    #[test]
    fn animals_arrive_in_groups_rather_than_one_at_a_time() {
        // **The mechanics were in the code and not in the world.** Deer
        // take their lead from the deer beside them and a wolf will not
        // come in alone -- and none of that could ever happen while the
        // spawner put down one animal at a time on a random bearing.
        //
        // Measured as "how many of this species are standing within
        // sight of another of it", which is what the herd rules actually
        // ask, rather than as a count of spawn calls.
        // Long enough for a few groups to arrive and no longer: given
        // an hour they would wander apart, which is what animals do and
        // not what this is asking about.
        //
        // **Four people standing on the same spot**, and that is the
        // rarity cap rather than a contrivance: one player is allowed
        // three animals (`MAX_ANIMALS_PER_PLAYER`), which a single group
        // can fill on its own, and a sample of one group cannot say
        // anything about whether spawns arrive in groups. Four players'
        // worth of allowance buys three or four groups, which can. It
        // used to say `while animals.len() < 4` against a cap of six;
        // under the new cap that loop never ends.
        let world = meadow(120);
        let mut animals = Animals::seeded(21);
        let crowd: Vec<(primitive_shared::protocol::PlayerId, (f32, f32, f32))> =
            (0..4).map(|i| (i as u64, (0.5, 21.0, 0.5))).collect();
        while animals.len() < 8 {
            animals.step(&world, &crowd, 0.05, NOON);
        }
        animals.step(&world, &crowd, 0.05, NOON);

        // By species, not by `EntityKind`: the kind carries the animal's
        // facing and its hurt flash as well, so comparing kinds compares
        // two deer looking in different directions and finds them
        // different.
        let standing: Vec<(Species, (f32, f32, f32))> = animals
            .states()
            .iter()
            .filter_map(|state| match state.kind {
                primitive_shared::protocol::EntityKind::Animal { species, .. } => {
                    Some((species, (state.x as f32, state.y as f32, state.z as f32)))
                }
                _ => None,
            })
            .collect();
        let with_company = standing
            .iter()
            .filter(|(species, at)| {
                standing.iter().any(|(other, other_at)| {
                    other == species
                        && other_at != at
                        && (other_at.0 - at.0).hypot(other_at.2 - at.2) < 24.0
                })
            })
            .count();
        assert!(
            with_company * 2 >= standing.len(),
            "only {with_company} of {} animals had one of their own kind anywhere near",
            standing.len()
        );
    }

    #[test]
    fn walking_away_forgets_them() {
        let world = meadow(80);
        let mut animals = Animals::seeded(11);
        animals.spawn(Species::Deer, (0.5, 21.0, 0.5));
        animals.spawn(Species::Hare, (2.5, 21.0, 0.5));
        assert_eq!(animals.len(), 2);

        // The player is now half a world away.
        animals.step(
            &world,
            &player((DESPAWN_DISTANCE * 2.0, 21.0, 0.0)),
            0.05,
            NOON,
        );
        assert!(animals.is_empty());
        assert_eq!(animals.stats().2, 2);
    }

    #[test]
    fn an_empty_server_simulates_nothing() {
        let world = meadow(40);
        let mut animals = Animals::seeded(12);
        animals.spawn(Species::Deer, (0.5, 21.0, 0.5));
        animals.step(&world, &[], 0.05, NOON);
        assert!(animals.is_empty(), "a herd wandering an empty world");
    }

    #[test]
    fn the_count_is_bounded_however_many_people_are_playing() {
        let world = meadow(80);
        let mut animals = Animals::seeded(13);
        // Far more players than the hard cap allows animals for.
        let crowd: Vec<(primitive_shared::protocol::PlayerId, (f32, f32, f32))> = (0..200)
            .map(|i| (i as u64, (0.5, 21.0, 0.5)))
            .collect();
        for _ in 0..20_000 {
            animals.step(&world, &crowd, 0.05, NOON);
        }
        assert!(animals.len() <= MAX_ANIMALS, "{} animals", animals.len());
    }

    #[test]
    fn every_animal_has_its_own_id() {
        let mut animals = Animals::seeded(14);
        for _ in 0..50 {
            animals.spawn(Species::Hare, (0.5, 21.0, 0.5));
        }
        let mut ids: Vec<EntityId> = animals.states().iter().map(|s| s.id).collect();
        ids.sort_unstable();
        let before = ids.len();
        ids.dedup();
        assert_eq!(ids.len(), before, "two animals share an id");
    }

    #[test]
    fn what_crosses_the_wire_is_the_middle_of_the_animal() {
        // The client draws a box around this point, so sending the feet
        // would mean every client had to know every species' height to
        // put it in the right place.
        let mut animals = Animals::seeded(15);
        let id = animals.spawn(Species::Deer, (1.0, 21.0, 2.0)).expect("deer");
        let state = animals.states().into_iter().find(|s| s.id == id).expect("state");
        assert_eq!(state.y as f32, 21.0 + Species::Deer.height() * 0.5);
        match state.kind {
            EntityKind::Animal { species, hurt, .. } => {
                assert_eq!(species, Species::Deer);
                assert_eq!(hurt, 0.0, "an untouched animal is flashing red");
            }
            other => panic!("wrong kind: {other:?}"),
        }
    }

    #[test]
    fn a_struck_animal_flashes_and_stops_flashing() {
        let world = meadow(20);
        let mut animals = Animals::seeded(16);
        let id = animals.spawn(Species::Deer, (0.5, 21.0, 0.5)).expect("deer");
        animals.strike(id, (0.5, 21.6, 0.5), 3.0, 1.0);
        let flashing = |a: &Animals| match a.states()[0].kind {
            EntityKind::Animal { hurt, .. } => hurt,
            _ => unreachable!(),
        };
        assert!(flashing(&animals) > 0.5, "a struck deer did not flash");
        for _ in 0..40 {
            animals.step(&world, &player((0.5, 21.0, 40.0)), 0.05, NOON);
        }
        assert_eq!(flashing(&animals), 0.0, "it is still flashing a second later");
    }

    // ---- what the world does to an animal ----

    #[test]
    fn a_deer_driven_onto_the_stakes_is_heard_where_it_happened_and_only_once_a_flinch() {
        use primitive_shared::types::{placed, BLOCK_STAKE};
        let world = meadow(5);
        world.put(0, 21, 0, placed(BLOCK_STAKE, 0.0, (0, 1, 0)));
        let mut animals = Animals::seeded(30);
        let id = animals.spawn(Species::Deer, (0.5, 21.0, 0.5)).expect("deer");
        let far = player((0.5, 21.0, 60.0));
        let mut heard = Vec::new();
        for _ in 0..4 {
            if let Some(deer) = animals.find_mut_for_test(id) {
                deer.position = (0.5, 21.0, 0.5);
                deer.velocity = (5.0, 0.0, 0.0);
            }
            animals.step(&world, &far, 0.05, NOON);
            heard.extend(animals.take_staked());
        }
        // Once: the flinch (`hurt_for`) keeps a body among the points from
        // being cut, and so heard, every tick.
        assert_eq!(heard.len(), 1, "a deer run onto the stakes was heard {} times", heard.len());
        let (x, _, z) = heard[0];
        assert!((x - 0.5).abs() < 1.0 && (z - 0.5).abs() < 1.0, "heard at {:?}, not at the stakes", heard[0]);
        assert!(animals.take_staked().is_empty(), "the list was read and not drained");
    }

    #[test]
    fn standing_in_a_campfire_burns_an_animal_the_way_it_burns_a_player() {
        // Measured against `survival::BURNING_PER_SECOND` rather than a
        // number written down here a second time, so the two can never
        // quietly disagree.
        use primitive_shared::types::BLOCK_CAMPFIRE_LIT;
        let world = meadow(5);
        world.put(0, 21, 0, BLOCK_CAMPFIRE_LIT);
        let mut animals = Animals::seeded(30);
        let id = animals.spawn(Species::Deer, (0.5, 21.25, 0.5)).expect("deer");
        let before = animals.find(id).expect("alive").health;
        let far = player((0.5, 21.0, 60.0));
        const TICK: f32 = 0.05;
        const TICKS: u32 = 20;
        for _ in 0..TICKS {
            animals.step(&world, &far, TICK, NOON);
        }
        let after = animals.find(id).expect("a second in a campfire should not kill a deer").health;
        let expected = crate::logic::survival::BURNING_PER_SECOND * (TICKS as f32 * TICK);
        let taken = before - after;
        assert!(
            taken > expected * 0.5,
            "a second standing in a lit campfire cost {taken:.2} health, expected about {expected:.2}"
        );
    }

    #[test]
    fn an_animal_standing_on_a_burning_pit_kiln_burns_the_way_a_player_does() {
        // A burning pit fills its cell, so a deer on it has its feet in the
        // air cell over the fire -- which is exactly where a player on one
        // was never burned either. See `survival::touches_fire`.
        use primitive_shared::types::BLOCK_PIT_KILN_LIT;
        let world = meadow(5);
        world.put(0, 20, 0, BLOCK_PIT_KILN_LIT);
        let mut animals = Animals::seeded(30);
        let id = animals.spawn(Species::Deer, (0.5, 21.0, 0.5)).expect("deer");
        let before = animals.find(id).expect("alive").health;
        let far = player((0.5, 21.0, 60.0));
        const TICK: f32 = 0.05;
        const TICKS: u32 = 20;
        for _ in 0..TICKS {
            animals.step(&world, &far, TICK, NOON);
        }
        let after = animals.find(id).expect("a second on a burning pit should not kill a deer").health;
        let expected = crate::logic::survival::BURNING_PER_SECOND * (TICKS as f32 * TICK);
        let taken = before - after;
        assert!(
            taken > expected * 0.5,
            "a second standing on a burning pit kiln cost {taken:.2} health, expected about {expected:.2}"
        );
    }

    #[test]
    fn falling_a_long_way_hurts_an_animal_and_a_short_hop_does_not() {
        let world = meadow(5);
        for y in 30..35 {
            for x in -5..=5 {
                for z in -5..=5 {
                    world.put(x, y, z, BLOCK_AIR);
                }
            }
        }
        let mut animals = Animals::seeded(31);
        let far = player((0.5, 21.0, 60.0));

        // A ten-block drop: comfortably past `SAFE_FALL_BLOCKS` and
        // short of what would kill a deer outright, so the same test
        // can check both that it lost health and that it is still there
        // to have lost it.
        let hurt = animals.spawn(Species::Deer, (0.5, 31.0, 0.5)).expect("deer");
        // A two-block step down: under `SAFE_FALL_BLOCKS`, and should
        // cost nothing at all -- an animal that flinched at every ledge
        // a player walks off without noticing would be unplayable
        // scenery.
        let safe = animals.spawn(Species::Deer, (5.5, 23.0, 4.5)).expect("deer");

        for _ in 0..200 {
            animals.step(&world, &far, 0.05, NOON);
        }

        let after = animals.find(hurt).expect("a ten-block fall should not have killed it");
        assert!(after.health < Species::Deer.health(), "a ten-block fall cost no health");
        assert!(
            (after.at().1 - 21.0).abs() < 0.5,
            "did not land on the floor: y={:.2}",
            after.at().1
        );

        let untouched = animals.find(safe).expect("alive");
        assert_eq!(
            untouched.health,
            Species::Deer.health(),
            "a two-block step down cost health"
        );
    }

    #[test]
    fn an_animal_sealed_under_a_ceiling_of_water_eventually_drowns() {
        // **None of these species swim** (see `walk`'s `BUOYANCY`), so
        // an animal that ends up under water almost always has
        // somewhere to float up to -- except when it genuinely does
        // not: a chamber flooded right up to its own roof has no
        // surface anywhere inside it.
        use primitive_shared::types::BLOCK_WATER;
        let world = TestWorld::default();
        for x in -2..=2 {
            for z in -2..=2 {
                world.put(x, 19, z, BLOCK_STONE);
                for y in 20..25 {
                    world.put(x, y, z, BLOCK_WATER);
                }
                world.put(x, 25, z, BLOCK_STONE);
            }
        }
        let mut animals = Animals::seeded(32);
        let id = animals.spawn(Species::Deer, (0.5, 21.0, 0.5)).expect("deer");
        let before = animals.find(id).expect("alive").health;
        let far = player((0.5, 19.0, 60.0));
        // A minute of game time: `BREATH_SECONDS` to use up its held
        // breath and then some to actually take the damage.
        for _ in 0..(60 * 20) {
            animals.step(&world, &far, 0.05, NOON);
            if animals.find(id).is_none() {
                break; // drowned outright, which also proves the point
            }
        }
        match animals.find(id) {
            None => {} // it did not survive being sealed in
            Some(a) => assert!(
                a.health < before,
                "an animal sealed in a flooded chamber for a minute of game time took \
                 no suffocation damage"
            ),
        }
    }

    // ---- an individual bend, not a shared one ----

    #[test]
    fn a_wandering_animals_drift_changes_from_one_thought_to_the_next() {
        // **The bug this test was written for.** `Animal::drift`'s own
        // doc comment has said "re-rolled at every thought" since the
        // field was added; the code set it once, from a hash of the
        // id, at `spawn`, and never touched it again. An animal whose
        // bend never changes is not picking its way -- it is walking a
        // slow, permanent circle, the same one every time it wanders --
        // and that is most of what "the whole herd turns together"
        // turned out to be: not one shared random source, but several
        // animals each individually stuck with a single, lifelong bend
        // that happened to point the same general way because they all
        // spawned facing roughly the same direction.
        let world = meadow(20);
        let mut animals = Animals::seeded(33);
        let id = animals.spawn(Species::Deer, (0.5, 21.0, 0.5)).expect("deer");
        let far = player((0.5, 21.0, 60.0));
        let mut seen = std::collections::HashSet::new();
        for _ in 0..400 {
            animals.step(&world, &far, 0.05, NOON);
            let drift = animals.find(id).expect("alive").drift;
            seen.insert((drift * 1000.0) as i32);
        }
        assert!(
            seen.len() >= 3,
            "this animal's drift took only {} distinct value(s) over twenty seconds of \
             wandering -- it is not being re-rolled",
            seen.len()
        );
    }

    // ---- what a tick of `survey` costs ----

    #[test]
    fn a_crowded_flock_only_pays_the_full_neighbour_scan_when_it_actually_thinks() {
        // **The measurement.** `survey` used to work out the herd
        // centre, the nearest quarry and the nearest threat for *every*
        // animal on *every* tick, even though `think` only ever reads
        // them on the one tick in about twenty its own thought is due.
        // A hundred animals close enough together that every one of
        // them has same-species neighbours (so the old code never
        // short-circuited on "nobody nearby") ticked for two seconds --
        // forty ticks at the server's own 0.05s -- would have done the
        // full scan `ANIMALS * TICKS = 4,000` times under the old code.
        //
        // The ceiling here is comfortably above what the fix actually
        // does (about a hundred -- each animal's staggered thought comes
        // due roughly once in these two seconds) and comfortably below
        // what the old code did, so a regression back to "always full"
        // fails it while ordinary variance in when each animal's thought
        // lands does not.
        //
        // The player has to be far enough that nobody is provoked into
        // fleeing: a bolt short-circuits `think` through the panic check
        // *before* the once-a-second gate (see `survey`'s doc comment on
        // `alarm`), which would make this measure how rarely a startled
        // herd thinks rather than how rarely a calm one does.
        let world = meadow(20);
        let mut animals = Animals::seeded(50);
        const ANIMALS: usize = 100;
        for i in 0..ANIMALS {
            // A tight cluster: every deer starts within `HERD_RADIUS` of
            // every other, so `centre` is never `None` for lack of a
            // neighbour -- only ever because the full scan that would
            // have found one was skipped.
            let x = (i % 10) as f32 * 0.6;
            let z = (i / 10) as f32 * 0.6;
            animals.spawn(Species::Deer, (x, 21.0, z));
        }
        let players = player((50.0, 21.0, 50.0));
        const TICK: f32 = 0.05;
        const TICKS: u32 = 40;
        for _ in 0..TICKS {
            animals.step(&world, &players, TICK, NOON);
        }
        let scans = animals.full_neighbour_scans;
        assert!(
            scans > 0,
            "no animal ever got a full neighbour scan -- `centre` can never be read"
        );
        assert!(
            scans < (ANIMALS as u64 * TICKS as u64) / 3,
            "{scans} full scans over {TICKS} ticks of {ANIMALS} animals -- survey is no \
             longer skipping animals that are not about to think (the old code did \
             {} of them)",
            ANIMALS as u64 * TICKS as u64
        );
    }

    #[test]
    fn turning_toward_an_ever_advancing_target_never_loses_its_bearings() {
        // **The bug this test guards against.** `turn_towards` used to
        // return `from + delta`, unwrapped -- so an animal that kept
        // turning the same way, tick after tick, carried its heading
        // further from a single lap every time it did. Combined with
        // `drift` never being re-rolled (see
        // `a_wandering_animals_drift_changes_from_one_thought_to_the_next`),
        // an animal that wandered long enough drove its own heading
        // past the point an f32 can add one tick's turn to it and have
        // the result mean anything -- which reads as an animal spinning
        // in place at a speed nothing asked for.
        //
        // Simulated directly against `turn_towards` rather than through
        // a simulated animal, because the failure is a property of the
        // function's own arithmetic and does not need a world, a
        // species or a clock to show up.
        let mut yaw = 0.0f32;
        let mut target = 0.0f32; // an ever-advancing target, as an unwrapped `wants_yaw` was
        for _ in 0..500_000 {
            target += 0.1; // roughly a full-speed drift's contribution per tick
            yaw = turn_towards(yaw, target, 0.2);
            assert!(
                (0.0..std::f32::consts::TAU).contains(&yaw),
                "yaw escaped a single lap after a long chase: {yaw}"
            );
        }
    }

    // ---- cover, memory, the pack and the charge ----

    /// A stand of leaves three high, five by five, at `x0..=x0+4`,
    /// `z0..=z0+4`. Solid: a deer cannot get into it, which is what
    /// makes it a thing to run *to* and then *round*.
    fn copse(world: &TestWorld, x0: i32, z0: i32) {
        use primitive_shared::types::BLOCK_LEAVES;
        for z in z0..=z0 + 4 {
            for x in x0..=x0 + 4 {
                for y in 21..=23 {
                    world.put(x, y, z, BLOCK_LEAVES);
                }
            }
        }
    }

    /// The shortest signed turn from `from` to `to`, in radians.
    fn swing(from: f32, to: f32) -> f32 {
        use std::f32::consts::{PI, TAU};
        let delta = (to - from).rem_euclid(TAU);
        if delta > PI {
            delta - TAU
        } else {
            delta
        }
    }

    #[test]
    fn a_fleeing_deer_makes_for_the_trees_and_stops_once_it_is_out_of_sight() {
        // **Part one: where it runs.** A bolt used to be straight away
        // from the player and nothing else. Here the player is due west
        // of the deer, so straight away is due east across open grass --
        // and there is a copse off to the north-east. The deer should
        // set off for the copse, which is a heading a player cannot
        // pre-aim down.
        let world = meadow(40);
        copse(&world, 4, 5);
        let mut animals = Animals::seeded(61);
        let id = animals.spawn(Species::Deer, (3.5, 21.0, 0.5)).expect("deer");
        let hunter = player((0.5, 21.0, 0.5));
        let mut ticks = 0;
        while animals.find(id).expect("alive").mind != Mind::Flee {
            animals.step(&world, &hunter, 0.05, NOON);
            ticks += 1;
            assert!(ticks < 60, "it never noticed a person three blocks away");
        }
        let deer = animals.find(id).expect("alive");
        assert!(deer.cover.is_some(), "it saw no cover in a copse five blocks off");
        for _ in 0..20 {
            animals.step(&world, &hunter, 0.05, NOON);
        }
        let deer = animals.find(id).expect("alive");
        assert!(
            deer.at().2 > 2.0,
            "it ran straight down the line instead of for the trees: {:?}",
            deer.at()
        );

        // **Part two: when it stops.** A deer that cannot be seen
        // through a wall of leaves gives up running after
        // `HIDDEN_SECONDS`, even with the player well inside its
        // awareness. The same pen, the same distances, and a player who
        // stands on top of the wall -- and so can see over it -- keeps
        // it running: the difference between the two is the line of
        // sight and nothing else.
        let penned = |player_at: (f32, f32, f32)| -> bool {
            use primitive_shared::types::BLOCK_LEAVES;
            let world = meadow(40);
            // A leafy pen, eight by eight inside, walls two high.
            for i in -1..=8 {
                for y in 21..=22 {
                    world.put(i, y, -1, BLOCK_LEAVES);
                    world.put(i, y, 8, BLOCK_LEAVES);
                    world.put(-1, y, i, BLOCK_LEAVES);
                    world.put(8, y, i, BLOCK_LEAVES);
                }
            }
            let mut animals = Animals::seeded(62);
            let id = animals.spawn(Species::Deer, (4.0, 21.0, 4.0)).expect("deer");
            {
                let deer = animals.animals.iter_mut().find(|a| a.id == id).expect("alive");
                deer.mind = Mind::Flee;
                deer.target = Some(1);
                deer.next_thought = 0.5;
            }
            let hunter = player(player_at);
            // Ten seconds: three to be hidden for, and time for the
            // thought that notices.
            for _ in 0..200 {
                animals.step(&world, &hunter, 0.05, NOON);
                if animals.find(id).expect("alive").mind != Mind::Flee {
                    return true;
                }
            }
            false
        };
        // Six blocks off, on the ground, behind the leaves: inside a
        // deer's awareness, outside its sight.
        assert!(
            penned((-4.0, 21.0, 4.0)),
            "a deer that could not be seen for ten seconds never stopped running"
        );
        // ...and standing on the wall, looking down into the pen.
        assert!(
            !penned((-1.5, 23.0, 4.0)),
            "a deer in plain view of somebody six blocks away gave up and grazed"
        );
    }


    /// A leafy canopy at y = 24 over the square from `(x0, z0)` to
    /// `(x0 + 8, z0 + 8)`, with nothing under it: shade to stand in, and
    /// nothing an animal has to walk round to get there.
    fn awning(world: &TestWorld, x0: i32, z0: i32) {
        use primitive_shared::types::BLOCK_LEAVES;
        for z in z0..=z0 + 8 {
            for x in x0..=x0 + 8 {
                world.put(x, 24, z, BLOCK_LEAVES);
            }
        }
    }

    #[test]
    fn a_grazing_animal_puts_its_head_down_and_lifts_it_again() {
        // The bout. A deer standing in grass is not a deer with its nose in
        // the ground for the rest of the afternoon: it takes a few mouthfuls
        // and then stands up and looks round, because a head in the grass
        // cannot see a wolf. See `graze`'s feeding branch.
        let world = meadow(40);
        let mut animals = Animals::seeded(91);
        let id = animals.spawn(Species::Deer, (0.5, 21.0, 0.5)).expect("deer");
        let far = player((0.5, 21.0, 90.0));
        let mut fed = 0;
        let mut looked = 0;
        for _ in 0..4000 {
            animals.step(&world, &far, 0.05, NOON);
            match animals.find(id).expect("alive").attitude_now() {
                primitive_shared::protocol::Attitude::Feeding => fed += 1,
                primitive_shared::protocol::Attitude::Alert => looked += 1,
                _ => {}
            }
        }
        assert!(fed > 0, "it never put its head down in three minutes of grass");
        assert!(looked > 0, "it fed for three minutes without once looking up");
    }

    #[test]
    fn a_running_animal_never_has_its_head_in_the_grass() {
        // `Animal::attitude_now`'s whole job. The decision sites write the
        // attitude and `bolt` is reached from five of them; what this asks is
        // the rule rather than the bookkeeping, so it startles a grazing deer
        // and watches the body rather than the mind.
        let world = meadow(60);
        let mut animals = Animals::seeded(92);
        let id = animals.spawn(Species::Deer, (0.5, 21.0, 0.5)).expect("deer");
        let hunter = player((6.5, 21.0, 0.5));
        for _ in 0..600 {
            animals.step(&world, &hunter, 0.05, NOON);
            let deer = animals.find(id).expect("alive");
            let speed = deer.velocity.0.hypot(deer.velocity.2);
            if speed > STANDING_STILL {
                assert!(
                    !matches!(
                        deer.attitude_now(),
                        primitive_shared::protocol::Attitude::Feeding
                            | primitive_shared::protocol::Attitude::Drinking
                            | primitive_shared::protocol::Attitude::Dozing
                    ),
                    "it was moving at {speed:.2} blocks a second with its head down"
                );
            }
        }
    }

    #[test]
    fn a_herd_in_the_shade_at_noon_settles_and_the_same_herd_in_the_open_does_not() {
        // See `MIDDAY_REST`: the middle of the day is spent lying up, and
        // only where there is shade to lie up in. The two runs differ by the
        // canopy and by nothing else -- same seed, same ground, same hour --
        // which is what makes the shade the cause rather than the weather.
        let dozing = |shaded: bool| {
            let world = meadow(40);
            if shaded {
                awning(&world, -4, -4);
            }
            let mut animals = Animals::seeded(93);
            let mut ids = Vec::new();
            for (x, z) in [(0.5, 0.5), (2.5, 1.5), (-1.5, 2.5), (1.5, -2.5)] {
                ids.push(animals.spawn(Species::Deer, (x, 21.0, z)).expect("deer"));
            }
            let far = player((0.5, 21.0, 90.0));
            let mut ticks = 0;
            for _ in 0..4000 {
                animals.step(&world, &far, 0.05, NOON);
                ticks += ids
                    .iter()
                    .filter(|&&id| {
                        animals.find(id).map(Animal::attitude_now)
                            == Some(primitive_shared::protocol::Attitude::Dozing)
                    })
                    .count();
            }
            ticks
        };
        let under_the_trees = dozing(true);
        let in_the_open = dozing(false);
        assert_eq!(in_the_open, 0, "deer in an open meadow lay up at noon anyway");
        assert!(
            under_the_trees > 0,
            "four deer under a canopy spent three minutes of noon without one of them resting"
        );
    }

    #[test]
    fn a_running_animal_that_turns_hard_loses_speed_doing_it() {
        // See `TURN_COST`. Two deer, both fleeing, one down a straight line
        // and one sent round a right angle half way: the one that turned has
        // to be slower over the same stretch of time, and it has to get its
        // speed back once it is pointing the way it is going again.
        let run = |turn: bool| {
            let world = meadow(80);
            let mut animals = Animals::seeded(94);
            let id = animals.spawn(Species::Deer, (0.5, 21.0, 0.5)).expect("deer");
            let far = player((0.5, 21.0, 70.0));
            let mut slowest = f32::MAX;
            for tick in 0..60 {
                if let Some(deer) = animals.find_mut_for_test(id) {
                    deer.mind = Mind::Flee;
                    deer.next_thought = 1.0;
                    // East for the first second and a half, then north --
                    // or east the whole way.
                    deer.wants_yaw = if turn && tick >= 30 {
                        std::f32::consts::FRAC_PI_2
                    } else {
                        0.0
                    };
                }
                animals.step(&world, &far, 0.05, NOON);
                if tick >= 30 {
                    let deer = animals.find(id).expect("alive");
                    slowest = slowest.min(deer.velocity.0.hypot(deer.velocity.2));
                }
            }
            let deer = animals.find(id).expect("alive");
            (slowest, deer.velocity.0.hypot(deer.velocity.2))
        };
        let (straight_slowest, _) = run(false);
        let (turned_slowest, turned_end) = run(true);
        assert!(
            turned_slowest < straight_slowest * 0.9,
            "the one that turned never dropped below {turned_slowest:.2} against {straight_slowest:.2} for the one that did not"
        );
        assert!(
            turned_end > turned_slowest * 1.2,
            "it came out of the turn at {turned_end:.2} and never got its speed back"
        );
    }

    #[test]
    fn a_heavy_animal_takes_longer_to_get_going_than_a_light_one() {
        // See `nimbleness`. Both are set running from a standstill down the
        // same open meadow; what is compared is how much of its own top speed
        // each has after half a second, so the answer is about the body and
        // not about which of them is faster.
        let fraction_after = |species: Species| {
            let world = meadow(60);
            let mut animals = Animals::seeded(95);
            let id = animals.spawn(species, (0.5, 21.0, 0.5)).expect("spawned");
            let far = player((0.5, 21.0, 90.0));
            for _ in 0..10 {
                if let Some(beast) = animals.find_mut_for_test(id) {
                    beast.mind = Mind::Flee;
                    beast.wants_yaw = 0.0;
                    beast.yaw = 0.0;
                    beast.next_thought = 1.0;
                }
                animals.step(&world, &far, 0.05, NOON);
            }
            let beast = animals.find(id).expect("alive");
            beast.velocity.0.hypot(beast.velocity.2) / (species.run_speed() * beast.pace)
        };
        let hare = fraction_after(Species::Hare);
        let bear = fraction_after(Species::Bear);
        assert!(
            hare > bear * 1.15,
            "a hare was {hare:.2} of the way to its run in half a second and a bear {bear:.2}"
        );
    }

    #[test]
    fn a_herd_left_alone_for_five_minutes_stays_together_and_never_stands_in_itself() {
        // The two halves of what a herd is, and they pull against each other:
        // cohesion without spacing is a heap on one block, spacing without
        // cohesion is four animals that wander off in four directions. Both,
        // for five minutes of nobody watching. See `HERD_COMFORT` and
        // `PERSONAL_SPACE`.
        let world = meadow(80);
        let mut animals = Animals::seeded(96);
        let ids: Vec<EntityId> = [(0.5, 0.5), (3.5, 1.5), (-2.5, 2.5), (1.5, -3.5), (-3.5, -1.5)]
            .iter()
            .map(|&(x, z)| animals.spawn(Species::Deer, (x, 21.0, z)).expect("deer"))
            .collect();
        // Forty blocks off: three times a deer's awareness, so nothing here
        // has ever heard of them, and well inside `DESPAWN_DISTANCE`, so a
        // herd that drifts a few dozen blocks over five minutes is still a
        // herd rather than an empty list.
        let far = player((0.5, 21.0, 40.0));
        let mut widest: f32 = 0.0;
        let mut closest = f32::MAX;
        for _ in 0..6000 {
            animals.step(&world, &far, 0.05, NOON);
            let at: Vec<(f32, f32, f32)> = ids.iter().map(|&id| animals.find(id).expect("alive").at()).collect();
            for (i, a) in at.iter().enumerate() {
                for b in &at[i + 1..] {
                    widest = widest.max(apart(*a, *b));
                    closest = closest.min(apart(*a, *b));
                }
            }
        }
        assert!(
            widest < HERD_RADIUS * 2.0,
            "the herd spread to {widest:.1} blocks across, which is not a herd"
        );
        // The bodies are the floor, not the steering: see `Animals::unstack`.
        // Half a block is inside `PERSONAL_SPACE`, which is the rule about
        // where a deer *wants* to stand, and outside the sum of two half
        // widths, which is the rule about where one *can*.
        assert!(
            closest > 0.6,
            "two deer got within {closest:.2} blocks of each other, which is one deer drawn twice"
        );
    }

    #[test]
    fn nothing_alive_ever_ends_up_inside_a_block() {
        // The promise every other rule here is written on top of: five
        // minutes of a mixed field going about its business -- walking,
        // grazing, drinking, fleeing a person who keeps moving -- and at no
        // tick is any animal standing in something solid. Broken ground with
        // steps, a pond and a copse to run into, because the ways into a
        // block are the climb, the water and the swerve.
        let world = meadow_with_a_lake(60, 34);
        for z in -8..=8 {
            for x in 10..=20 {
                world.put(x, 21, z, BLOCK_GRASS);
            }
        }
        copse(&world, -20, -14);
        let mut animals = Animals::seeded(97);
        for (species, x, z) in [
            (Species::Deer, 0.5, 0.5),
            (Species::Deer, 2.5, 2.5),
            (Species::Sheep, -4.5, 1.5),
            (Species::Hare, 5.5, -3.5),
            (Species::Boar, -8.5, -4.5),
            (Species::Wolf, 12.5, 6.5),
        ] {
            let feet = surface_under(&world, x, 40.0, z).expect("ground") as f32;
            animals.spawn(species, (x, feet, z)).expect("spawned");
        }
        for tick in 0..6000 {
            // A player walking a slow circle, so something is always being
            // startled and something is always going back to grazing.
            let angle = tick as f32 * 0.004;
            let walker = player((angle.cos() * 14.0, 21.0, angle.sin() * 14.0));
            animals.step(&world, &walker, 0.05, tick as f32 / 2000.0);
            for animal in &animals.animals {
                assert!(
                    fits(&world, animal.position, animal.species),
                    "a {} was inside the world at {:?} on tick {tick}",
                    animal.species.name(),
                    animal.position
                );
            }
        }
    }

    #[test]
    fn a_wolf_catches_a_deer_it_surprises_and_never_one_that_saw_it_coming() {
        // The hunt has to be winnable and it has to be losable, and the fact
        // that decides which is the one every other rule about wolves is
        // built on: **a deer at a full run is faster than a wolf at a full
        // run** (7.2 against 6.3), so the only hunt that ever lands is one
        // the deer did not see starting. A wolf that has closed to two blocks
        // eats; two animals both flat out down the same line have a gap that
        // grows, whatever the turn cost and the acceleration do to either of
        // them.
        let world = meadow(90);
        let mut animals = Animals::seeded(98);
        let deer = animals.spawn(Species::Deer, (0.5, 21.0, 0.5)).expect("deer");
        animals.spawn(Species::Wolf, (-1.5, 21.0, 0.5)).expect("wolf");
        let far = player((0.5, 21.0, 70.0));
        let mut eaten = false;
        for _ in 0..4000 {
            animals.step(&world, &far, 0.05, NOON);
            if animals.find(deer).is_none() {
                eaten = true;
                break;
            }
        }
        assert!(eaten, "a wolf two blocks behind a deer never caught it");

        // ...and the chase it does not win. Both running east, neither with
        // anything to decide, which is what the last stretch of every chase
        // comes down to.
        let world = meadow(90);
        let mut animals = Animals::seeded(99);
        let deer = animals.spawn(Species::Deer, (4.5, 21.0, 0.5)).expect("deer");
        let wolf = animals.spawn(Species::Wolf, (0.5, 21.0, 0.5)).expect("wolf");
        let far = player((0.5, 21.0, 70.0));
        let gap = |animals: &Animals| {
            apart(animals.find(deer).expect("alive").at(), animals.find(wolf).expect("alive").at())
        };
        let mut opened = gap(&animals);
        for _ in 0..100 {
            for (id, mind) in [(deer, Mind::Flee), (wolf, Mind::Chase)] {
                if let Some(beast) = animals.find_mut_for_test(id) {
                    beast.mind = mind;
                    beast.wants_yaw = 0.0;
                    beast.next_thought = 1.0;
                }
            }
            animals.step(&world, &far, 0.05, NOON);
            opened = gap(&animals);
        }
        assert!(
            opened > 4.0,
            "five seconds of a straight chase left the wolf {opened:.1} blocks back, having started four"
        );
    }

    #[test]
    fn a_hares_bursts_zigzag() {
        // The hare is caught by cutting it off, and cutting it off
        // means knowing where it will be. Each burst is aimed
        // `HARE_DODGE` off the straight-away line, alternating sides,
        // so the line to cut is never the one it just ran down.
        assert_bursts_zigzag(Species::Hare, 63, 40);
    }

    /// **The antelope swerves as the hare does**, through the one predicate
    /// (`Species::zigzags`) rather than a second `== Hare` beside the first.
    /// A wider meadow, because an antelope's burst covers half as much ground
    /// again as a hare's.
    #[test]
    fn an_antelopes_bursts_zigzag_as_a_hares_do() {
        assert_bursts_zigzag(Species::Antelope, 64, 60);
    }

    fn assert_bursts_zigzag(species: Species, seed: u64, span: i32) {
        // The player is kept five blocks *outside* the animal -- on the
        // far side of it from the origin -- so it flees back and forth
        // through the middle of the meadow and never off the edge.
        let world = meadow(span);
        let mut animals = Animals::seeded(seed);
        let id = animals.spawn(species, (0.5, 21.0, 0.5)).expect("spawned");
        let mut offsets: Vec<f32> = Vec::new();
        let mut last_heading = f32::NAN;
        for _ in 0..400 {
            let at = animals.find(id).expect("alive").at();
            let out = (at.0 * at.0 + at.2 * at.2).sqrt().max(0.01);
            let hunter = (at.0 + at.0 / out * 5.0, 21.0, at.2 + at.2 / out * 5.0);
            animals.step(&world, &player(hunter), 0.05, NOON);
            let hare = animals.find(id).expect("alive");
            if hare.mind == Mind::Flee
                && (last_heading.is_nan() || (hare.wants_yaw - last_heading).abs() > 1e-3)
            {
                last_heading = hare.wants_yaw;
                let away = (hare.at().2 - hunter.2).atan2(hare.at().0 - hunter.0);
                offsets.push(swing(away, hare.wants_yaw));
            }
        }
        assert!(offsets.len() >= 4, "only {} bursts in twenty seconds: {offsets:?}", offsets.len());
        for pair in offsets.windows(2) {
            assert!(
                pair[0].signum() != pair[1].signum(),
                "two bursts in a row swerved the same way: {offsets:?}"
            );
        }
        for offset in &offsets {
            assert!(
                (offset.abs() - HARE_DODGE).abs() < 0.2,
                "a burst was {offset:.2} rad off the line rather than about {HARE_DODGE}: {offsets:?}"
            );
        }
    }

    #[test]
    fn a_herd_struck_twice_at_the_same_water_grazes_somewhere_else() {
        // **The decision, on its own.** An animal standing where it was
        // hurt walks away from the spot rather than standing over it,
        // ahead of the herd and ahead of the grass.
        let world = meadow(60);
        let mut animals = Animals::seeded(64);
        let id = animals.spawn(Species::Deer, (0.5, 21.0, 0.5)).expect("deer");
        {
            let deer = animals.animals.iter_mut().find(|a| a.id == id).expect("alive");
            deer.remember_danger((4.5, 0.5)); // four blocks east
            deer.mind = Mind::Idle;
            deer.next_thought = 0.0;
        }
        animals.step(&world, &player((0.5, 21.0, 70.0)), 0.05, NOON);
        let deer = animals.find(id).expect("alive");
        assert_eq!(deer.mind, Mind::Wander, "it stood where it had been hurt");
        assert!(
            deer.wants_yaw.cos() < -0.7,
            "it set off toward the place it was hurt: yaw {}",
            deer.wants_yaw
        );

        // **The herd.** Three deer at a watering place, one of them
        // struck twice from the same spot. All three remember, the two
        // blows are one memory held twice as long, and over the next
        // half minute the herd's middle never comes back to the water.
        let world = meadow(60);
        let mut animals = Animals::seeded(65);
        let herd = [
            animals.spawn(Species::Deer, (0.5, 21.0, 0.5)).expect("deer"),
            animals.spawn(Species::Deer, (2.5, 21.0, 0.5)).expect("deer"),
            animals.spawn(Species::Deer, (0.5, 21.0, 2.5)).expect("deer"),
        ];
        let from = (-2.0, 21.6, 0.5);
        assert_eq!(animals.strike(herd[0], from, 8.0, 1.0), Struck::Hurt);
        assert_eq!(animals.strike(herd[0], from, 8.0, 1.0), Struck::Hurt);
        for id in herd {
            let deer = animals.find(id).expect("alive");
            assert_eq!(deer.dangers.len(), 1, "two blows in one place are two memories");
            assert!(
                deer.dangers[0].left > DANGER_MEMORY,
                "struck twice, remembered no longer than once: {}",
                deer.dangers[0].left
            );
        }
        // Nobody about -- past a deer's awareness, inside despawn range.
        let watcher = player((0.5, 21.0, 70.0));
        let water = (0.5, 21.0, 0.5);
        let mut nearest = f32::INFINITY;
        for tick in 0..(40 * 20) {
            animals.step(&world, &watcher, 0.05, NOON);
            // The first ten seconds are the bolt and the regathering;
            // what is being asked is where they settle.
            if tick < 10 * 20 {
                continue;
            }
            let (mut sx, mut sz) = (0.0, 0.0);
            for id in herd {
                let at = animals.find(id).expect("alive").at();
                sx += at.0;
                sz += at.2;
            }
            nearest = nearest.min(apart((sx / 3.0, 21.0, sz / 3.0), water));
        }
        assert!(
            nearest > DANGER_RADIUS * 0.4,
            "the herd came back to graze where it was shot at: its middle came within {nearest:.1}"
        );
    }

    #[test]
    fn the_second_wolf_comes_from_the_other_side() {
        // Two wolves, both east of the player and both inside their
        // provoking range. The first (lower id) comes straight in; the
        // second should go *round* -- staying off the player while it
        // does -- and commit from the far side, so that facing one puts
        // your back to the other.
        let world = meadow(60);
        let mut animals = Animals::seeded(66);
        let leader = animals.spawn(Species::Wolf, (5.5, 21.0, 0.5)).expect("wolf");
        let follower = animals.spawn(Species::Wolf, (5.5, 21.0, 2.5)).expect("wolf");
        assert!(follower > leader, "ids are meant to grow with the ordinal");
        let at = (0.5, 21.0, 0.5);
        let bearing = |p: (f32, f32, f32)| (p.2 - at.2).atan2(p.0 - at.0);
        let start = bearing(animals.find(follower).expect("alive").at());

        let mut came_in_from = None;
        let mut closest_on_the_way = f32::INFINITY;
        for _ in 0..400 {
            animals.step(&world, &player(at), 0.05, NOON);
            let wolf = animals.find(follower).expect("alive");
            if wolf.mind == Mind::Charge {
                came_in_from = Some(bearing(wolf.at()));
                break;
            }
            closest_on_the_way = closest_on_the_way.min(apart(wolf.at(), at));
        }
        let from = came_in_from.expect("the second wolf never came in at all");
        let went_round = swing(start, from).abs();
        assert!(
            went_round > 1.5,
            "the second wolf charged from where it stood ({went_round:.2} rad round from \
             its start) instead of working round to the far side"
        );
        // **Stated as a bite it never got, not as a fraction of the
        // ring.** The first leg of the circle always dips inside
        // `CIRCLE_RADIUS`: the wolf turns onto its arc while it is
        // already accelerating, so it drifts across the circle before it
        // is on it, and the drift is a fraction of its run. It was about
        // 1.9 blocks of dip at a run of 7.6; at 8.4 (see
        // `Species::run_speed`) it is about 2.1. The old threshold --
        // `CIRCLE_RADIUS - 2.0` -- was that measurement written down as
        // though it were the rule, so a tenth of a block of extra drift
        // read as a broken mechanic.
        //
        // What the mechanic actually promises is that the wolf works
        // *round* you rather than coming at you, and "coming at you" has
        // a number of its own: `GORE_RANGE`, the reach it bites at.
        // Staying a body's width clear of that all the way round the arc
        // is the property, and it is one that does not move the next
        // time anything gets faster.
        assert!(
            closest_on_the_way > GORE_RANGE + 1.0,
            "it came within a lunge of the player on its way round: {closest_on_the_way:.1} \
             blocks, against a bite at {GORE_RANGE}"
        );
    }

    #[test]
    fn wolves_keep_off_a_lit_fire_at_night() {
        use primitive_shared::types::BLOCK_CAMPFIRE_LIT;
        const MIDNIGHT: f32 = 0.0;
        // A pack of two inside provoking range, a player standing two
        // blocks from a hearth. Three runs of the same night: with the
        // fire, with a torch in hand instead, and with neither. The
        // first two keep the wolves out of the circle; the third is the
        // pack coming in, which is what proves the fire is the reason.
        struct Night {
            charged: bool,
            blows: usize,
            nearest_the_fire: f32,
            nearest_the_player: f32,
        }
        let night = |hearth: bool, torch: bool| -> Night {
            let world = meadow(60);
            let at = (0.5, 21.0, 0.5);
            let fire = (-1.5, 21.0, 0.5);
            if hearth {
                world.put(-2, 21, 0, BLOCK_CAMPFIRE_LIT);
            }
            let mut animals = Animals::seeded(67);
            let pack = [
                animals.spawn(Species::Wolf, (5.5, 21.0, 0.5)).expect("wolf"),
                animals.spawn(Species::Wolf, (5.5, 21.0, 3.5)).expect("wolf"),
            ];
            if torch {
                animals.carrying_fire(vec![1]);
            }
            let mut night = Night {
                charged: false,
                blows: 0,
                nearest_the_fire: f32::INFINITY,
                nearest_the_player: f32::INFINITY,
            };
            for _ in 0..400 {
                night.blows += animals.step(&world, &player(at), 0.05, MIDNIGHT).len();
                for id in pack {
                    let wolf = animals.find(id).expect("alive");
                    night.charged |= wolf.mind == Mind::Charge;
                    night.nearest_the_fire = night.nearest_the_fire.min(apart(wolf.at(), fire));
                    night.nearest_the_player =
                        night.nearest_the_player.min(apart(wolf.at(), at));
                }
            }
            night
        };

        let hearth = night(true, false);
        assert!(!hearth.charged, "a wolf charged somebody standing by a lit fire");
        assert_eq!(hearth.blows, 0, "a wolf bit somebody standing by a lit fire");
        assert!(
            hearth.nearest_the_fire >= FIRE_RADIUS - 0.5,
            "a wolf came within {:.1} of a lit fire after dark",
            hearth.nearest_the_fire
        );

        let torch = night(false, true);
        assert!(!torch.charged, "a wolf charged somebody holding a torch");
        assert!(
            torch.nearest_the_player >= FIRE_RADIUS - 0.5,
            "a wolf came within {:.1} of a held torch after dark",
            torch.nearest_the_player
        );

        let dark = night(false, false);
        assert!(dark.charged, "with no fire at all, the pack did not come");
        assert!(
            dark.nearest_the_player < 3.0,
            "with no fire at all, the pack kept its distance: {:.1}",
            dark.nearest_the_player
        );
    }

    /// What the tick loop would say about somebody lying in a bed -- or
    /// standing still, when `asleep` is false.
    fn lying(asleep: bool) -> PlayerSign {
        PlayerSign {
            who: 1,
            facing: 0.0,
            working: false,
            airborne: false,
            low: asleep,
            wounded: false,
            asleep,
            held: None,
            reek: 1.0,
        }
    }

    #[test]
    fn a_lone_wolf_comes_for_a_sleeper_in_the_dark_and_leaves_one_alone_by_day() {
        // **The opening the night gives a lone wolf.** One wolf, which by
        // its own rule (`needs_company`) never comes at a standing person
        // alone, three blocks from somebody asleep in the open and looking
        // at them. At midnight it comes; at noon it does not -- and a
        // standing person at midnight is not come for either, which is what
        // proves it is the sleep and not the dark.
        const MIDNIGHT: f32 = 0.0;
        let night = |time: f32, asleep: bool| -> (bool, usize) {
            let world = meadow(40);
            let mut animals = Animals::seeded(71);
            let id = animals.spawn(Species::Wolf, (3.5, 21.0, 0.5)).expect("wolf");
            animals.face_for_test(id, std::f32::consts::PI);
            animals.player_signs(vec![lying(asleep)]);
            let at = (0.5, 21.0, 0.5);
            let (mut charged, mut blows) = (false, 0);
            for _ in 0..300 {
                blows += animals.step(&world, &player(at), 0.05, time).len();
                charged |= animals.find(id).is_some_and(|w| w.mind == Mind::Charge);
            }
            (charged, blows)
        };
        let (charged, blows) = night(MIDNIGHT, true);
        assert!(charged && blows > 0, "a lone wolf left a sleeper in the open alone at midnight");
        let (charged, blows) = night(NOON, true);
        assert!(!charged && blows == 0, "a lone wolf came for a sleeper in broad daylight");
        let (charged, blows) = night(MIDNIGHT, false);
        assert!(!charged && blows == 0, "a lone wolf came for somebody standing awake: its nerve is the pack's");
    }

    #[test]
    fn a_sleeper_by_a_lit_fire_is_not_come_for() {
        // The same lone wolf and the same sleeper at midnight, with a
        // campfire lit two blocks from the bed: the fire has its say before
        // the opening does (`think_hunter`), so nothing bites.
        use primitive_shared::types::BLOCK_CAMPFIRE_LIT;
        let world = meadow(40);
        world.put(-2, 21, 0, BLOCK_CAMPFIRE_LIT);
        let mut animals = Animals::seeded(71);
        let id = animals.spawn(Species::Wolf, (3.5, 21.0, 0.5)).expect("wolf");
        animals.face_for_test(id, std::f32::consts::PI);
        animals.player_signs(vec![lying(true)]);
        let at = (0.5, 21.0, 0.5);
        let mut blows = 0;
        for _ in 0..300 {
            blows += animals.step(&world, &player(at), 0.05, 0.0).len();
            assert_ne!(animals.find(id).expect("alive").mind, Mind::Charge, "a wolf charged a sleeper beside a lit fire");
        }
        assert_eq!(blows, 0, "a wolf bit a sleeper beside a lit fire");
    }

    #[test]
    fn wolves_drawn_to_a_fire_circle_at_the_edge_of_its_light_and_never_step_into_it() {
        // **The picture the fire makes.** A pack of two eighteen blocks off
        // in the dark, a player sitting still by a campfire. The fire is
        // what they see (`FIRE_SEEN_AT_NIGHT`) -- a still person in the dark
        // with no fire is seen from five -- so they come; the fire is what
        // stops them (`FIRE_RADIUS`), so they come to its edge
        // (`FIRE_EDGE`) and walk round it. Stated as three things a player
        // at the fire would notice: they came nearer than they started,
        // they never came into the light, and nothing bit anybody.
        use primitive_shared::types::BLOCK_CAMPFIRE_LIT;
        let world = meadow(60);
        world.put(-2, 21, 0, BLOCK_CAMPFIRE_LIT);
        let fire = (-1.5, 21.0, 0.5);
        let at = (0.5, 21.0, 0.5);
        let mut animals = Animals::seeded(72);
        let pack = [
            animals.spawn(Species::Wolf, (18.5, 21.0, 0.5)).expect("wolf"),
            animals.spawn(Species::Wolf, (18.5, 21.0, 2.5)).expect("wolf"),
        ];
        for id in pack {
            animals.face_for_test(id, std::f32::consts::PI);
        }
        animals.player_signs(vec![lying(false)]);
        let (mut blows, mut nearest, mut came_to_the_edge) = (0, f32::INFINITY, false);
        let mut stalked = false;
        for _ in 0..900 {
            blows += animals.step(&world, &player(at), 0.05, 0.0).len();
            for id in pack {
                let wolf = animals.find(id).expect("alive");
                let off = apart(wolf.at(), fire);
                nearest = nearest.min(off);
                came_to_the_edge |= off <= FIRE_EDGE + 2.5;
                stalked |= wolf.attitude == primitive_shared::protocol::Attitude::Stalking;
            }
        }
        assert_eq!(blows, 0, "a wolf bit somebody sitting by a lit fire");
        assert!(nearest >= FIRE_RADIUS - 0.5, "a wolf came {nearest:.1} blocks from a lit fire after dark");
        assert!(came_to_the_edge, "the pack never came to the edge of the light: nearest {nearest:.1}");
        assert!(stalked, "nothing at the edge of the light was ever seen stalking");
    }

    #[test]
    fn the_night_finds_a_sleeper_only_as_the_odds_say_and_once() {
        // `find_the_sleeper`: nothing at odds of nought, a pair of wolves
        // put down just past a lunge at odds of one -- and not a second pair
        // the same night, however the sleeper's luck is rolled again.
        let world = meadow(40);
        let bed = (0.5, 21.0, 0.5);
        let mut animals = Animals::seeded(73);
        animals.calendar(3.9);
        assert!(animals.find_the_sleeper(&world, bed, 0.0).is_empty(), "the night came at odds of nought");
        let came = animals.find_the_sleeper(&world, bed, 1.0);
        assert_eq!(came.len(), 2, "the night that came was not a pair of wolves: {came:?}");
        for id in &came {
            let wolf = animals.find(*id).expect("put down");
            assert_eq!(wolf.species, Species::Wolf);
            let off = apart(wolf.at(), bed);
            assert!(
                off > Species::Wolf.provoke_range() && off <= SLEEPER_FOUND_DISTANCE.1 + 0.5,
                "a wolf was put down {off:.1} blocks from the bed"
            );
        }
        assert!(animals.find_the_sleeper(&world, bed, 1.0).is_empty(), "the same night came twice");
        animals.calendar(4.9);
        assert_eq!(animals.find_the_sleeper(&world, bed, 1.0).len(), 2, "the next night did not come at all");
    }

    #[test]
    fn a_wounded_wolf_shadows_rather_than_leaves() {
        // Under half its health a wolf stops coming in and falls back to
        // about twelve blocks, where it stays -- facing you. Both under
        // half and under a third, because the third is where a boar
        // breaks off and runs, and a wolf must not do that either: a
        // wolf that leaves is one you beat, and one that shadows is one
        // you still have to deal with.
        //
        // **Stated as what it has left rather than as what came off.**
        // Both thresholds are fractions, and the two blows this used --
        // seven and nine off a wolf of twelve -- stopped being either of
        // them the day `animals::TOUGHNESS` made a wolf sixty: seven off
        // sixty is a wolf at eighty-eight per cent, which comes straight
        // back in, and the test said so.
        for left in [0.48, 0.32] {
            let world = meadow(80);
            let mut animals = Animals::seeded(68);
            let id = animals.spawn(Species::Wolf, (3.5, 21.0, 0.5)).expect("wolf");
            let at = (0.5, 21.0, 0.5);
            let taken = Species::Wolf.blow_taking(Species::Wolf.health() * (1.0 - left));
            assert_eq!(animals.strike(id, (0.5, 21.6, 0.5), 8.0, taken), Struck::Hurt);
            let mut charged = false;
            let mut blows = 0;
            for _ in 0..600 {
                blows += animals.step(&world, &player(at), 0.05, NOON).len();
                charged |= animals.find(id).expect("alive").mind == Mind::Charge;
            }
            assert!(!charged, "a wolf left at {left} of its health came in again");
            assert_eq!(blows, 0, "a wolf left at {left} of its health landed a bite");
            let wolf = animals.find(id).expect("it was forgotten");
            let distance = apart(wolf.at(), at);
            assert!(
                (SHADOW_DISTANCE * 0.6..=SHADOW_DISTANCE * 1.6).contains(&distance),
                "left at {left} of its health it is {distance:.1} blocks off rather than \
                 shadowing from about {SHADOW_DISTANCE}"
            );
            assert_eq!(wolf.target, Some(1), "left at {left} of its health, it lost interest");
        }
    }

    #[test]
    fn a_boars_charge_is_a_straight_line_that_can_be_sidestepped() {
        use std::f32::consts::PI;
        let world = meadow(60);
        let mut animals = Animals::seeded(69);
        let id = animals.spawn(Species::Boar, (8.5, 21.0, 0.5)).expect("boar");
        // Already facing the player, so there is no wind-up to wait
        // through and the run is the whole of what is measured.
        animals.face_for_test(id, PI);
        let mut at = (6.0, 21.0, 0.5);
        let mut ticks = 0;
        while animals.find(id).expect("alive").mind != Mind::Charge {
            animals.step(&world, &player(at), 0.05, NOON);
            ticks += 1;
            assert!(ticks < 100, "it never charged somebody two and a half blocks off");
        }
        // Let it come on until it is a block and a half away...
        ticks = 0;
        while animals.find(id).expect("alive").at().0 > at.0 + 1.5 {
            animals.step(&world, &player(at), 0.05, NOON);
            ticks += 1;
            assert!(ticks < 100, "it never got near");
            assert_eq!(animals.find(id).expect("alive").mind, Mind::Charge);
        }
        // ...and step out of its way.
        at = (6.0, 21.0, 2.8);

        let mut blows = 0;
        let mut widest_off_the_line = 0.0f32;
        let mut sharpest_turn = 0.0f32;
        let mut furthest = f32::INFINITY;
        ticks = 0;
        loop {
            let before = animals.find(id).expect("alive").yaw;
            blows += animals.step(&world, &player(at), 0.05, NOON).len();
            let boar = animals.find(id).expect("alive");
            if boar.mind != Mind::Charge {
                break;
            }
            widest_off_the_line = widest_off_the_line.max((boar.at().2 - 0.5).abs());
            sharpest_turn = sharpest_turn.max(swing(before, boar.yaw).abs());
            furthest = furthest.min(boar.at().0);
            ticks += 1;
            assert!(ticks < 100, "the charge never ended");
        }
        assert_eq!(blows, 0, "a sidestep at a block and a half did not clear the tusks");
        assert!(
            widest_off_the_line < 1.0,
            "it bent toward the player mid-charge: {widest_off_the_line:.2} blocks off its line"
        );
        assert!(
            sharpest_turn <= CHARGE_TURN_RATE * 0.05 + 1e-3,
            "it turned {sharpest_turn:.3} rad in one tick while at speed -- a charge that can steer"
        );
        assert!(
            furthest < at.0 - 2.5,
            "it pulled up where the player had been rather than running through: x={furthest:.1}"
        );

        // **The second it spends turning round is the player's.** It is
        // blown and turning, the player is behind it, and a blow there
        // lands harder -- once; the blow itself turns it to face you.
        let boar = animals.find(id).expect("alive");
        assert_eq!(boar.mind, Mind::Recover);
        let eye = (at.0, at.1 + 1.6, at.2);
        assert!(boar.is_turning() && boar.exposed_back(eye), "its back is not to the player");
        let before = boar.health;
        assert_eq!(animals.strike(id, eye, 8.0, 2.0), Struck::Hurt);
        let after = animals.find(id).expect("alive").health;
        // The backstab first, then the hide: a blow on the shoulder of
        // a turning animal is still a blow on its shoulder. See
        // `Species::hurt_by`.
        let landed = Species::Boar.hurt_by(2.0 * BACKSTAB);
        assert!(
            (before - after - landed).abs() < 1e-4,
            "a blow on a turning boar's back took {} rather than {landed}",
            before - after
        );
        let boar = animals.find(id).expect("alive");
        assert!(!boar.exposed_back(eye), "it took a blow in the back and did not turn round");
        assert_eq!(animals.strike(id, eye, 8.0, 2.0), Struck::Hurt);
        let again = animals.find(id).expect("alive").health;
        let plain = Species::Boar.hurt_by(2.0);
        assert!((after - again - plain).abs() < 1e-4, "the second blow was a backstab too");
    }

    #[test]
    fn a_calm_field_casts_no_rays_and_a_fleeing_one_casts_few() {
        // The cost of seeing. `sees` is asked once a second per prey
        // animal with a person in range and on one tick in `LOOK_EVERY`
        // per fleeing one -- never for an animal nobody is near. A
        // hundred deer with the player far off must therefore run for
        // two seconds without a single line-of-sight ray, which this
        // checks the only way a test can: by counting the block lookups
        // the world sees and comparing the two conditions.
        use std::cell::Cell;
        struct Counting<'a> {
            inner: &'a TestWorld,
            lookups: Cell<u64>,
        }
        impl BlockWorld for Counting<'_> {
            fn block(&self, x: i32, y: i32, z: i32) -> Option<primitive_shared::types::BlockId> {
                self.lookups.set(self.lookups.get() + 1);
                self.inner.block(x, y, z)
            }
            fn set(&self, x: i32, y: i32, z: i32, block: primitive_shared::types::BlockId) {
                self.inner.set(x, y, z, block)
            }
        }
        let lookups_per_tick = |player_at: (f32, f32, f32)| -> f64 {
            let meadow = meadow(30);
            let world = Counting { inner: &meadow, lookups: Cell::new(0) };
            let mut animals = Animals::seeded(70);
            for i in 0..100 {
                animals.spawn(Species::Deer, ((i % 10) as f32 * 0.6, 21.0, (i / 10) as f32 * 0.6));
            }
            let who = player(player_at);
            for _ in 0..40 {
                animals.step(&world, &who, 0.05, NOON);
            }
            world.lookups.get() as f64 / 40.0
        };
        let calm = lookups_per_tick((50.0, 21.0, 50.0));
        let hunted = lookups_per_tick((-3.0, 21.0, 3.0));
        // Neither is a budget in itself -- the walk and the forage
        // dominate both -- but a herd that is being chased must not cost
        // an order of magnitude more per tick than one that is not, or
        // the rays are not being rationed. (Measured: about 1.4x.)
        assert!(
            hunted < calm * 4.0,
            "a startled herd costs {hunted:.0} lookups a tick against {calm:.0} calm -- \
             the sight checks are not rate-limited"
        );
    }


    #[test]
    fn a_poisoned_thrust_goes_on_hurting_after_the_spear_has_stopped() {
        // **What the fly agaric buys**, and the two halves of it: the
        // paste does nothing on a miss, and on a hit it keeps taking
        // health for a few seconds after the thrust has landed. Its
        // rate is deliberately small (`POISON_PER_SECOND`) -- a poison
        // that killed on its own would make the spear behind it beside
        // the point.
        let world = meadow(10);
        let mut animals = Animals::seeded(4);
        let id = animals
            .spawn(Species::Deer, (0.5, 21.0, 0.5))
            .expect("a deer");

        // A miss, from across the meadow, with the paste on the point.
        let far = (40.0, 21.0, 40.0);
        assert!(matches!(
            animals.strike_poisoned(id, far, 3.0, 1.0, primitive_shared::combat::POISON_SECONDS),
            Struck::Missed
        ));
        let after_miss = animals.health(id).expect("still alive");
        for _ in 0..40 {
            animals.step(&world, &player((-6.0, 21.0, 0.5)), 0.05, NOON);
        }
        assert_eq!(
            animals.health(id),
            Some(after_miss),
            "a swing that missed still poisoned the deer"
        );

        // ...and a hit, from where the deer actually is. Read rather
        // than assumed: it has been wandering for two seconds by now,
        // and a fixed point beside its spawn is a swing at where it
        // used to be -- which is how the first version of this test
        // failed.
        let deer = animals.find(id).expect("the deer is still about").position;
        let close = (deer.0, deer.1 + 1.0, deer.2);
        assert!(!matches!(
            animals.strike_poisoned(id, primitive_shared::geometry::narrow(close), 3.0, 1.0, primitive_shared::combat::POISON_SECONDS),
            Struck::Missed
        ));
        let after_hit = animals.health(id).expect("a deer survives one thrust");
        for _ in 0..40 {
            animals.step(&world, &player((-6.0, 21.0, 0.5)), 0.05, NOON);
        }
        let after_poison = animals.health(id).expect("and survives the poison");
        assert!(
            after_poison < after_hit - 0.5,
            "two seconds of poison took {}, which is nothing",
            after_hit - after_poison
        );

        // ...and it runs out rather than going on for ever.
        for _ in 0..200 {
            animals.step(&world, &player((-6.0, 21.0, 0.5)), 0.05, NOON);
        }
        let settled = animals.health(id).expect("alive");
        for _ in 0..40 {
            animals.step(&world, &player((-6.0, 21.0, 0.5)), 0.05, NOON);
        }
        assert_eq!(
            animals.health(id),
            Some(settled),
            "the poison never wore off"
        );
    }

    /// **A flushed bird is in the air, and it comes down again.** The
    /// whole of the flight mechanic, stated as the two things a player
    /// sees. See `FLIGHT_HEIGHT`.
    #[test]
    fn a_startled_bird_takes_to_the_air_and_lands_when_it_has_calmed_down() {
        let world = meadow(40);
        let mut animals = Animals::seeded(5);
        let id = animals.spawn(Species::Fowl, (0.0, 21.0, 0.0)).expect("a bird");
        // Nothing frightening: it stays on the ground, where everything
        // else in this world stays.
        for _ in 0..200 {
            animals.step(&world, &player((30.0, 21.0, 30.0)), 0.05, NOON);
        }
        let calm = animals.find(id).expect("alive").at().1;
        assert!(
            (calm - 21.0).abs() < 0.6,
            "a bird nobody frightened was at {calm:.1} rather than on the ground"
        );

        // Hit it, which is what `Mind::Flee` is for: it goes up.
        animals.strike(id, (1.0, 21.6, 0.5), 8.0, 0.2);
        let mut highest = calm;
        for _ in 0..40 {
            animals.step(&world, &player((1.0, 21.0, 0.5)), 0.05, NOON);
            highest = highest.max(animals.find(id).expect("alive").at().1);
        }
        assert!(
            highest > 22.5,
            "a flushed bird only reached {highest:.1} -- it never left the ground"
        );

        // ...and once it is calm again it is back down: nothing in this
        // world hovers.
        // The watcher stays inside `DESPAWN_DISTANCE` and outside the
        // bird's awareness: far enough to be forgotten about, near
        // enough that the bird is not forgotten about.
        for _ in 0..400 {
            animals.step(&world, &player((30.0, 21.0, 30.0)), 0.05, NOON);
        }
        let landed = animals.find(id).expect("alive").at().1;
        assert!(landed < 22.0, "the bird never came down: {landed:.1}");
    }

    // ---- the bird and its nest ----
    //
    // The one animal here that lives *somewhere*. See the table at
    // `HOME_RANGE` for the states and `NEST_SCAN_INTERVAL` for what the
    // one new world search costs.

    /// A meadow with sky over it rather than a lid.
    ///
    /// `meadow` fills air only to y = 29, which is *below* the top of the
    /// window `nest_near` looks through (`NEST_LIFT` blocks above a
    /// bird's feet) -- and an unloaded cell stops that scan, rightly: an
    /// animal must not plan a route through terrain that has not arrived.
    /// A test on `meadow` would therefore find no nests and prove
    /// nothing.
    fn wood(span: i32) -> TestWorld {
        let world = TestWorld::default();
        for z in -span..=span {
            for x in -span..=span {
                world.put(x, 20, z, BLOCK_GRASS);
                for y in 21..40 {
                    world.put(x, y, z, BLOCK_AIR);
                }
            }
        }
        world
    }

    /// Plants one tree: a trunk to `crown`, a three-by-three of leaves on
    /// top of it, and a nest above the middle of that if there is one --
    /// which is exactly the shape `worldgen::place_nests` leaves behind.
    fn tree(
        world: &TestWorld,
        at: (i32, i32),
        crown: i32,
        nest: Option<primitive_shared::types::BlockId>,
    ) {
        use primitive_shared::types::{BLOCK_LEAVES, BLOCK_LOG};
        for y in 21..crown {
            world.put(at.0, y, at.1, BLOCK_LOG);
        }
        for dz in -1..=1 {
            for dx in -1..=1 {
                world.put(at.0 + dx, crown, at.1 + dz, BLOCK_LEAVES);
            }
        }
        if let Some(nest) = nest {
            world.put(at.0, crown + 1, at.1, nest);
        }
    }

    #[test]
    fn a_bird_takes_a_nest_when_it_can_find_one_and_a_bare_crown_when_it_cannot() {
        // **"Creates or uses nests", answered with the half this file is
        // allowed to give.** An animal here never writes a block (the
        // module doc's oldest rule), so a bird does not build: it takes
        // the nest `worldgen` put in a canopy, and settles for a crown
        // when there is no nest to be had. From the outside those are the
        // same behaviour -- a bird that lives in a particular tree.
        use primitive_shared::types::BLOCK_NEST_EGGS;

        // The search is put on the spot rather than waited for, because
        // what is being checked is *which* tree it picks: a bird left to
        // its own timer would have wandered somewhere else first and the
        // answer would be about the wander. See `NEST_SCAN_INTERVAL` for
        // the timer, and the tests below for the behaviour.
        let bare = wood(40);
        tree(&bare, (10, 0), 26, None);
        let mut animals = Animals::seeded(101);
        let id = animals.spawn(Species::Fowl, (0.5, 21.0, 0.5)).expect("a bird");
        let bird = animals.find(id).expect("alive");
        assert_eq!(
            nest_near(&bare, bird),
            Some((10.5, 0.5)),
            "it walked past the only tree in the meadow"
        );

        // ...and a real nest beats a bare crown at any distance inside
        // the range, which is the ordering the mechanic lives on: a bird
        // crosses the whole search radius for a nest rather than
        // settling for the tree it is already standing under.
        let nested = wood(40);
        tree(&nested, (10, 0), 26, None);
        tree(&nested, (0, -15), 26, Some(BLOCK_NEST_EGGS));
        let bird = animals.find(id).expect("alive");
        assert_eq!(
            nest_near(&nested, bird),
            Some((0.5, -14.5)),
            "it took the nearer crown over a nest it could see"
        );

        // An empty meadow has nowhere to nest, and the honest answer is
        // to say so rather than to invent a home on the turf: a bird
        // nesting in the open is a bird a player trips over.
        let empty = wood(40);
        let bird = animals.find(id).expect("alive");
        assert_eq!(nest_near(&empty, bird), None, "it nested in a field");
    }

    #[test]
    fn a_bird_carried_off_by_a_fright_flies_back_to_its_nest_and_lands() {
        // **The state the player asked for by name.** A bird that has
        // been flushed is somewhere it did not choose; once nothing is in
        // sight the first thing it does is go home, at a cruise
        // (`Mind::Homing`), and *land* -- which is not a state of its own,
        // just the altitude spring with nothing keeping it up.
        //
        // Where the home came from is the test above; this one starts
        // from a bird that has one and thirty blocks to cover.
        let world = wood(80);
        let mut animals = Animals::seeded(102);
        let id = animals.spawn(Species::Fowl, (30.5, 21.0, 0.5)).expect("a bird");
        {
            let bird = animals.animals.iter_mut().find(|a| a.id == id).expect("alive");
            bird.home = Some((0.5, 0.5));
            bird.next_thought = 0.0;
        }
        // Far enough off to be nothing to a bird (awareness is eleven)
        // and near enough that it is not despawned.
        let watcher = player((0.5, 21.0, 70.0));
        let nest = (0.5, 21.0, 0.5);
        let mut flew = false;
        let mut highest = 21.0f32;
        let mut settled = false;
        for _ in 0..600 {
            animals.step(&world, &watcher, 0.05, NOON);
            let bird = animals.find(id).expect("alive");
            flew |= bird.mind == Mind::Homing;
            highest = highest.max(bird.at().1);
            settled |= apart(bird.at(), nest) <= HOME_RANGE && bird.at().1 < 21.6;
        }
        assert!(flew, "it walked home rather than flying");
        assert!(
            highest > 23.0,
            "it crossed the meadow at {highest:.1} -- that is a bird on foot"
        );
        assert!(
            settled,
            "it never got home and on the ground: it is at {:?}",
            animals.find(id).expect("alive").at()
        );
    }

    #[test]
    fn a_settled_bird_moves_between_perches_instead_of_standing_at_one() {
        // **"Flies between points, not in a straight line to one."** A
        // bird that only ever flew when it was frightened and only ever
        // flew home would make two journeys in its life. The hop is the
        // third and it is the one a player watching a tree actually sees.
        // See `HOP_CHANCE`.
        let world = wood(60);
        let mut animals = Animals::seeded(103);
        let id = animals.spawn(Species::Fowl, (0.5, 21.0, 0.5)).expect("a bird");
        {
            let bird = animals.animals.iter_mut().find(|a| a.id == id).expect("alive");
            bird.home = Some((0.5, 0.5));
        }
        let watcher = player((0.5, 21.0, 70.0));
        let nest = (0.5, 21.0, 0.5);
        let mut flights = 0;
        let mut was = Mind::Idle;
        let mut furthest = 0.0f32;
        for _ in 0..2000 {
            animals.step(&world, &watcher, 0.05, NOON);
            let bird = animals.find(id).expect("alive");
            if bird.mind == Mind::Homing && was != Mind::Homing {
                flights += 1;
            }
            was = bird.mind;
            furthest = furthest.max(apart(bird.at(), nest));
        }
        assert!(
            flights >= 3,
            "a hundred seconds by its own nest and it took off {flights} time(s)"
        );
        // ...and every one of them stayed in its own wood. A hop that
        // took the bird out of sight of the nest would be a wander with
        // wings.
        assert!(
            furthest < HOME_RANGE + HOP_RANGE + 6.0,
            "it got {furthest:.1} blocks from its nest, which is not living there"
        );
    }

    #[test]
    fn a_bird_comes_down_onto_the_canopy_rather_than_onto_the_ground_under_it() {
        // **"Lands on the ground and on objects."** Nothing in the
        // descent knows what it is landing on: `walk` aims a bird with
        // no reason to be up at `surface_under`, which is the first full
        // top below it -- turf, a crown, a roof somebody built. This
        // checks the case that is not the ground, because that is the one
        // a bird hanging in the air over a tree would have failed.
        let world = wood(40);
        // A crown big enough that the bird cannot wander off the side of
        // it in the moment after it lands.
        for dz in -2..=2 {
            for dx in -2..=2 {
                world.put(10 + dx, 26, dz, primitive_shared::types::BLOCK_LEAVES);
            }
        }
        let mut animals = Animals::seeded(104);
        let id = animals.spawn(Species::Fowl, (0.5, 21.0, 0.5)).expect("a bird");
        {
            let bird = animals.animals.iter_mut().find(|a| a.id == id).expect("alive");
            bird.home = Some((10.5, 0.5));
            bird.next_thought = 0.0;
        }
        let watcher = player((0.5, 21.0, 70.0));
        let mut perched = false;
        for _ in 0..600 {
            animals.step(&world, &watcher, 0.05, NOON);
            let bird = animals.find(id).expect("alive");
            perched |= bird.at().1 > 26.5 && bird.mind != Mind::Homing;
        }
        assert!(
            perched,
            "it flew to the tree and stayed in the air over it: {:?}",
            animals.find(id).expect("alive").at()
        );
    }

    #[test]
    fn birds_crossing_a_wood_fly_over_the_canopy_and_never_end_up_inside_it() {
        // **The player's "они врезаются в листву".** The flight is an
        // altitude the bird is pulled toward over whatever is *under* it,
        // and a bird crossing a clearing toward a wood is over the clearing
        // until the moment it is over a crown -- so the first thing that
        // ever told it about the tree was the tree. See `AIR_LOOK_AHEAD`
        // for the look ahead that fixes it and `steer_around` for what a
        // bird does about the one it meets anyway.
        //
        // Five minutes of four birds in a planted wood, with somebody
        // pacing the clearing so they keep being flushed across it. Nothing
        // may ever be inside a block, and the crossings have to happen
        // *over* the canopy rather than round it.
        let world = wood(60);
        for row in -6i32..=6 {
            for column in -6i32..=6 {
                if row == 0 && column == 0 {
                    continue;
                }
                // Crowns of three heights, so the look-ahead has to answer
                // a real profile and not one number.
                let crown = 26 + (row + column).rem_euclid(3i32);
                tree(&world, (row * 8, column * 8), crown, (row == 1 && column == 0).then_some(primitive_shared::types::BLOCK_NEST));
            }
        }
        let mut animals = Animals::seeded(1101);
        let mut birds = Vec::new();
        for (i, at) in [(0.5f32, 0.5f32), (3.5, -2.5), (-3.5, 2.5), (2.5, 3.5)].into_iter().enumerate() {
            let id = animals.spawn(Species::Fowl, (at.0, 21.0, at.1)).expect("a bird");
            let bird = animals.animals.iter_mut().find(|a| a.id == id).expect("alive");
            // Homes on four different trees, so the wood is crossed rather
            // than circled.
            bird.home = Some([(8.5, 0.5), (-8.5, 8.5), (16.5, -8.5), (-16.5, -16.5)][i]);
            birds.push(id);
        }
        let mut over_the_crowns = 0;
        for tick in 0..6000 {
            // A person walking a line across the clearing: three blocks a
            // second there and back, which flushes whatever is on the
            // ground near them.
            let walk = ((tick as f32 * 0.05 / 6.0).rem_euclid(2.0) - 1.0).abs() * 40.0 - 20.0;
            animals.step(&world, &player((walk, 21.0, 0.0)), 0.05, NOON);
            for &id in &birds {
                let bird = animals.find(id).expect("a bird was lost");
                assert!(
                    fits(&world, primitive_shared::geometry::wide(bird.at()), Species::Fowl),
                    "tick {tick}: a bird is inside a block at {:?}",
                    bird.at()
                );
                if bird.at().1 > 29.0 {
                    over_the_crowns += 1;
                }
            }
        }
        assert!(
            over_the_crowns > 200,
            "the birds spent {over_the_crowns} ticks above the canopy: they are threading between the trunks, not flying over the wood"
        );
    }

    #[test]
    fn a_bird_goes_to_roost_after_dark_and_leaves_the_branch_at_dawn() {
        // **The other half of "бездельничают": there was no hour at which
        // watching a tree told you anything.** A bird kept a grouse's
        // afternoon at midnight -- hop, forage, hop -- so the wood was the
        // same place at every hour. Now the wood fills at dusk and empties
        // at dawn, which is a *reason to be somewhere at a time*, and it is
        // one clock test in `homing`.
        let world = wood(40);
        tree(&world, (0, 0), 26, Some(primitive_shared::types::BLOCK_NEST));
        let mut animals = Animals::seeded(1102);
        let id = animals.spawn(Species::Fowl, (12.5, 21.0, 0.5)).expect("a bird");
        {
            let bird = animals.animals.iter_mut().find(|a| a.id == id).expect("alive");
            bird.home = Some((0.5, 0.5));
            bird.next_thought = 0.0;
        }
        let watcher = player((0.5, 21.0, 70.0));
        const MIDNIGHT: f32 = 0.0;
        // A minute to get home in the dark.
        for _ in 0..1200 {
            animals.step(&world, &watcher, 0.05, MIDNIGHT);
        }
        let home = animals.find(id).expect("alive").home.expect("a nest");
        let at = animals.find(id).expect("alive").at();
        assert!(
            (at.0 - home.0).hypot(at.2 - home.1) <= HOME_REACH && at.1 > 26.0,
            "it is at {at:?} after dark and its tree is at {home:?}: that is not a roost"
        );
        // ...and then it sits there. Three minutes without leaving the
        // crown, and without a single walk.
        let mut walked = 0;
        for _ in 0..3600 {
            animals.step(&world, &watcher, 0.05, MIDNIGHT);
            let bird = animals.find(id).expect("alive");
            let at = bird.at();
            assert!(
                (at.0 - home.0).hypot(at.2 - home.1) <= HOME_RANGE,
                "it left the roost in the night and is at {at:?}"
            );
            if bird.mind == Mind::Wander {
                walked += 1;
            }
        }
        assert_eq!(walked, 0, "a roosting bird spent {walked} ticks wandering");
        // Dawn: it has something to do again.
        let mut stirred = false;
        for _ in 0..3600 {
            animals.step(&world, &watcher, 0.05, NOON);
            let bird = animals.find(id).expect("alive");
            let at = bird.at();
            stirred |= (at.0 - home.0).hypot(at.2 - home.1) > HOME_REACH * 2.0;
        }
        assert!(stirred, "it stayed on the branch all day");
    }

    #[test]
    fn looking_for_a_nest_is_paid_for_by_the_minute_and_not_by_the_tick() {
        // **The cost argument, measured rather than asserted in prose.**
        // The scan is at most twenty-four columns of twelve reads -- see
        // `nest_near` -- and it is paid only by a bird that has no nest,
        // at most once every `NEST_SCAN_INTERVAL` seconds. A bird that
        // has one never pays it again.
        //
        // Counted the only way a test can: by asking the world how many
        // times it was read, with a covey that has nowhere to nest (the
        // worst case -- the search fails and is re-run for ever) against
        // the same covey that has already settled.
        use std::cell::Cell;
        struct Counting<'a> {
            inner: &'a TestWorld,
            lookups: Cell<u64>,
        }
        impl BlockWorld for Counting<'_> {
            fn block(&self, x: i32, y: i32, z: i32) -> Option<primitive_shared::types::BlockId> {
                self.lookups.set(self.lookups.get() + 1);
                self.inner.block(x, y, z)
            }
            fn set(&self, x: i32, y: i32, z: i32, block: primitive_shared::types::BlockId) {
                self.inner.set(x, y, z, block)
            }
        }
        const BIRDS: usize = 20;
        const TICKS: usize = 400;
        let lookups_per_tick = |homed: bool| -> f64 {
            let meadow = wood(40);
            let world = Counting { inner: &meadow, lookups: Cell::new(0) };
            let mut animals = Animals::seeded(105);
            for i in 0..BIRDS {
                let at = ((i % 5) as f32 * 2.0 + 0.5, 21.0, (i / 5) as f32 * 2.0 + 0.5);
                let id = animals.spawn(Species::Fowl, at).expect("a bird");
                if homed {
                    let bird = animals.animals.iter_mut().find(|a| a.id == id).expect("alive");
                    bird.home = Some((at.0, at.2));
                }
            }
            let who = player((0.5, 21.0, 70.0));
            for _ in 0..TICKS {
                animals.step(&world, &who, 0.05, NOON);
            }
            world.lookups.get() as f64 / TICKS as f64
        };
        let settled = lookups_per_tick(true);
        let homeless = lookups_per_tick(false);
        // The bound the doc claims, per bird per tick: 288 reads once
        // every eight seconds at a twentieth of a second a tick.
        let allowed = 288.0 * 0.05 / NEST_SCAN_INTERVAL as f64;
        let extra = (homeless - settled) / BIRDS as f64;
        assert!(
            extra < allowed * 1.5,
            "a bird with nowhere to nest costs {extra:.2} extra lookups a tick against a \
             budget of {allowed:.2} -- the search is not on a timer"
        );
        // ...and the same thing said as the shape of the bill: searching
        // must not be most of what a covey costs.
        assert!(
            homeless < settled * 2.0,
            "a homeless covey costs {homeless:.0} lookups a tick against {settled:.0} settled"
        );
    }

    /// **A bear spawns in the woods and nowhere else.** The player asked
    /// for it in so many words; see `Species::needs_trees` and `wooded`.
    #[test]
    fn a_bear_will_not_spawn_in_an_open_meadow() {
        let bare = meadow(40);
        assert!(!wooded(&bare, 0.0, 20, 0.0), "an empty meadow counted as a wood");

        // A tree's worth of timber round the same spot, and the answer
        // changes -- which is also the rule for a wood a player planted.
        let planted = meadow(40);
        for dz in -1..=1 {
            for dx in -1..=1 {
                for dy in 1..=4 {
                    planted.put(dx, 20 + dy, dz + 3, primitive_shared::types::BLOCK_LOG);
                }
            }
        }
        assert!(wooded(&planted, 0.0, 20, 0.0), "a stand of nine trunks is not a wood");
    }

    /// **The savanna spawns its own animals -- through the spawner itself.**
    /// `Species::lives_in` is the table; this is `populate` asking the world
    /// which country a spot is in. A spawner that forgot to ask would put
    /// deer under the acacias and lions in the orchard with every shared
    /// test still green.
    #[test]
    fn the_savanna_spawns_zebra_antelope_and_lions_and_nothing_from_the_woods() {
        use primitive_shared::worldgen::Biome;
        // Wider than the furthest a group is placed from the player, so every
        // attempt has turf under it and the only thing deciding is the country.
        let world = meadow(SPAWN_MAX as i32 + GROUP_SPREAD as i32 + 2);
        let spawned = |biome: Option<Biome>, night: bool| -> Vec<Species> {
            world.set_biome(biome);
            let mut animals = Animals::seeded(71);
            let mut seen = Vec::new();
            for _ in 0..300 {
                animals.populate(&world, &player((0.5, 21.0, 0.5)), night);
                seen.extend(animals.animals.drain(..).map(|a| a.species));
            }
            seen
        };
        let savanna_only = |s: &Species| matches!(s, Species::Zebra | Species::Antelope | Species::Lion);
        for night in [false, true] {
            let savanna = spawned(Some(Biome::Savanna), night);
            for species in &savanna {
                assert!(species.lives_in(Biome::Savanna), "a {} spawned on the savanna", species.name());
            }
            for wanted in [Species::Zebra, Species::Antelope, Species::Lion] {
                assert!(
                    savanna.contains(&wanted),
                    "no {} among {} animals spawned on the savanna (night: {night})",
                    wanted.name(),
                    savanna.len()
                );
            }
            let meadow = spawned(Some(Biome::Plains), night);
            assert!(meadow.contains(&Species::Deer), "no deer in a meadow (night: {night})");
            assert!(!meadow.iter().any(savanna_only), "the savanna's animals spawned in a meadow");
        }
        // ...more lions after dark, which is the wolf's rule in the other
        // country...
        let lions = |night| spawned(Some(Biome::Savanna), night).iter().filter(|s| **s == Species::Lion).count();
        let (by_day, by_night) = (lions(false), lions(true));
        assert!(by_night > by_day, "the savanna's night is no different: {by_day} lions by day, {by_night} by night");
        // ...and a world that cannot say which country it is keeps the
        // animals it always had (`UNKNOWN_COUNTRY`).
        let unknown = spawned(None, false);
        assert!(!unknown.is_empty() && !unknown.iter().any(savanna_only), "a world with no biome grew lions");
    }

    /// **A lion does not wait for a second, where a wolf does**
    /// (`a_lone_wolf_follows_and_does_not_bite`) -- and about distance it is
    /// a boar: it watches somebody who keeps theirs and comes at the one who
    /// walks into it. Both halves, because a lion that charged from fifteen
    /// blocks would make the savanna unwalkable, and one that never came
    /// alone would be a wolf with a mane.
    #[test]
    fn a_lone_lion_watches_from_a_distance_and_comes_at_a_person_who_walks_up_to_it() {
        let world = meadow(60);
        let mut animals = Animals::seeded(81);
        let id = animals.spawn(Species::Lion, (10.0, 21.0, 0.5)).expect("lion");
        for _ in 0..100 {
            animals.step(&world, &player((1.0, 21.0, 0.5)), 0.05, NOON);
            assert_ne!(
                animals.find(id).expect("alive").mind,
                Mind::Charge,
                "a lion charged somebody nine blocks off"
            );
        }
        let mut charged = false;
        for _ in 0..200 {
            let at = animals.find(id).expect("alive").at();
            animals.step(&world, &player((at.0 - 3.0, 21.0, at.2)), 0.05, NOON);
            if animals.find(id).expect("alive").mind == Mind::Charge {
                charged = true;
                break;
            }
        }
        assert!(charged, "a lone lion three blocks from a person stood and watched, the way a lone wolf does");
    }

    #[test]
    fn a_lion_hunts_a_zebra_with_no_player_anywhere_near() {
        // The wolf's hunt in the other country: nobody watching, and the
        // lion still has something to do, and the zebra knows it.
        let world = meadow(80);
        let mut animals = Animals::seeded(82);
        let lion = animals.spawn(Species::Lion, (0.5, 21.0, 0.5)).expect("lion");
        // Inside the lion's run (`CHASE_FRACTION` of its awareness), so the
        // chase starts rather than the stalk.
        let zebra = animals.spawn(Species::Zebra, (8.5, 21.0, 0.5)).expect("zebra");
        let watcher = player((0.5, 21.0, 80.0));
        let (mut hunted, mut chased, mut ran) = (false, false, false);
        for _ in 0..400 {
            animals.step(&world, &watcher, 0.05, NOON);
            let Some(l) = animals.find(lion) else { break };
            hunted |= l.quarry == Some(zebra);
            chased |= matches!(l.mind, Mind::Chase | Mind::Charge);
            ran |= animals.find(zebra).is_none_or(|z| z.mind == Mind::Flee);
        }
        assert!(hunted, "the lion never noticed the zebra");
        assert!(chased, "the lion never broke into a run after it");
        assert!(ran, "the zebra grazed beside a hunting lion");
    }

    /// **A lion keeps off a lit fire after dark, and comes without one** --
    /// the wolves' test for the savanna's night animal, alone, because a
    /// lion does not need the pair a wolf does to come in at all.
    #[test]
    fn a_lion_keeps_off_a_lit_fire_at_night_and_comes_without_one() {
        use primitive_shared::types::BLOCK_CAMPFIRE_LIT;
        const MIDNIGHT: f32 = 0.0;
        let night = |hearth: bool| -> (bool, f32) {
            let world = meadow(60);
            let at = (0.5, 21.0, 0.5);
            let fire = (-1.5, 21.0, 0.5);
            if hearth {
                world.put(-2, 21, 0, BLOCK_CAMPFIRE_LIT);
            }
            let mut animals = Animals::seeded(83);
            let id = animals.spawn(Species::Lion, (4.5, 21.0, 0.5)).expect("lion");
            let (mut charged, mut nearest_the_fire) = (false, f32::INFINITY);
            for _ in 0..300 {
                animals.step(&world, &player(at), 0.05, MIDNIGHT);
                let lion = animals.find(id).expect("alive");
                charged |= lion.mind == Mind::Charge;
                nearest_the_fire = nearest_the_fire.min(apart(lion.at(), fire));
            }
            (charged, nearest_the_fire)
        };
        let (charged, nearest) = night(true);
        assert!(!charged, "a lion charged somebody standing by a lit fire");
        assert!(nearest >= FIRE_RADIUS - 0.5, "a lion came within {nearest:.1} of a lit fire after dark");
        let (charged, _) = night(false);
        assert!(charged, "with no fire at all, a lion four blocks off did not come");
    }

    /// **A kill left lying draws the wolves, and a fed wolf lets you
    /// be.** Two properties in one test because they are one mechanic:
    /// the wolf crosses the meadow to the carcass, and once it has fed
    /// it stops being the thing that comes at you.
    ///
    /// The player stands far enough off to be out of the wolf's
    /// awareness (eighteen), because a person in reach outranks
    /// carrion, which is the rule this must not accidentally break.
    #[test]
    fn a_hungry_wolf_crosses_a_field_to_a_carcass_and_stops_hunting_once_it_has_fed() {
        let world = meadow(40);
        let carcass = (8, 21, 0);
        world.put(
            carcass.0,
            carcass.1,
            carcass.2,
            primitive_shared::animals::carcass_at_stage(Species::Deer, 0),
        );
        let mut animals = Animals::seeded(11);
        animals.spawn(Species::Wolf, (0.0, 21.0, 0.0)).expect("a wolf");
        let where_it_is = |animals: &Animals| {
            let state = animals.states()[0];
            (state.x as f32, state.y as f32, state.z as f32)
        };
        let start = where_it_is(&animals);
        let kill = (carcass.0 as f32 + 0.5, carcass.1 as f32, carcass.2 as f32 + 0.5);
        let who = player((60.0, 21.0, 60.0));
        // **The nearest it ever got, not where it ended up.** A wolf that
        // has eaten is a wolf with nothing to do, and a wolf with nothing
        // to do wanders -- at two and a half blocks a second, which over
        // the twenty seconds after the meal is most of a meadow. Reading
        // the final position was asking "did it stay by the carcass",
        // which is not the mechanic and was only ever true by luck about
        // which way the wander happened to point.
        let mut nearest = apart(start, kill);
        for _ in 0..400 {
            animals.step(&world, &who, 0.05, NOON);
            nearest = nearest.min(apart(where_it_is(&animals), kill));
        }
        assert!(
            nearest < apart(start, kill),
            "the wolf ignored a carcass ten blocks away: it started at {start:?} and never \
             came nearer than {nearest:.1}"
        );
        assert!(nearest < 3.0, "the wolf never reached the carcass: {nearest:.1} at best");
        // ...and it ate: a wolf that walked over its dinner and left is
        // not what `carcass_near` is for.
        assert!(
            animals.animals[0].fed_for > 0.0,
            "it reached the carcass and went away hungry"
        );
        // ...and the carcass is still there. A wolf feeds beside a kill;
        // it does not delete a player's deer while they fetch a knife.
        assert!(
            primitive_shared::types::is_carcass(
                world.block(carcass.0, carcass.1, carcass.2).expect("loaded")
            ),
            "the wolf ate the block"
        );
    }


    /// **A body draws the wolves, and they leave what is in it alone.**
    ///
    /// The carcass test above, asked of a dead player, because that is the
    /// whole of the claim: a body is meat lying in a wood and the animals
    /// treat it as such -- and what the player finds when they get back is
    /// a wolf standing over their things, not a hole where their things
    /// were. The three louder mechanics that were rejected are argued at
    /// the call site in `hunt`; the two assertions at the end of this are
    /// what would go red if any of them crept in.
    ///
    /// The player stands far enough off to be out of the wolf's awareness
    /// (eighteen), because a person in reach outranks carrion -- and here
    /// that rule is doing double duty: the *living* player must not be
    /// what draws the wolf to their own body.
    #[test]
    fn a_hungry_wolf_crosses_a_field_to_a_body_and_leaves_what_is_in_it_alone() {
        let world = meadow(40);
        let grave = (8, 21, 0);
        world.put(grave.0, grave.1, grave.2, primitive_shared::types::BLOCK_CORPSE);
        let mut animals = Animals::seeded(11);
        animals.spawn(Species::Wolf, (0.0, 21.0, 0.0)).expect("a wolf");
        let where_it_is = |animals: &Animals| {
            let state = animals.states()[0];
            (state.x as f32, state.y as f32, state.z as f32)
        };
        let start = where_it_is(&animals);
        let body = (grave.0 as f32 + 0.5, grave.1 as f32, grave.2 as f32 + 0.5);
        let who = player((60.0, 21.0, 60.0));
        let mut nearest = apart(start, body);
        for _ in 0..400 {
            animals.step(&world, &who, 0.05, NOON);
            nearest = nearest.min(apart(where_it_is(&animals), body));
        }
        assert!(nearest < 3.0, "the wolf never reached the body: {nearest:.1} at best");
        assert!(
            animals.animals[0].fed_for > 0.0,
            "it reached the body and went away hungry"
        );
        // ...and the body is exactly as it was left. Not eaten, not
        // turned into a carcass somebody could butcher, not hurried on
        // toward bones.
        assert_eq!(
            world.block(grave.0, grave.1, grave.2),
            Some(primitive_shared::types::BLOCK_CORPSE),
            "the wolf changed the body it fed beside",
        );
    }

    /// **Bones draw nothing.** There is nothing left on a skeleton to
    /// smell, and a wolf that crossed a meadow for one would be an animal
    /// doing arithmetic rather than being hungry. It matters beyond the
    /// flavour: the remains of a player stand for as long as the world
    /// does, so anything that drew a scavenger to them would post a guard
    /// on that cell for ever.
    #[test]
    fn the_bones_of_a_dead_player_draw_no_scavengers() {
        let world = meadow(40);
        let grave = (8, 21, 0);
        world.put(grave.0, grave.1, grave.2, primitive_shared::types::BLOCK_REMAINS);
        let mut animals = Animals::seeded(11);
        animals.spawn(Species::Wolf, (0.0, 21.0, 0.0)).expect("a wolf");
        let who = player((60.0, 21.0, 60.0));
        for _ in 0..400 {
            animals.step(&world, &who, 0.05, NOON);
        }
        assert!(
            animals.animals[0].fed_for <= 0.0,
            "a wolf ate a skeleton",
        );
    }

    // ---- the gull ----

    /// A shore: sand at y = 20 for `x < 0`, and the sea for `x >= 0` --
    /// sand at 14 under water from 15 to 20 -- with air over both to 50, out
    /// to `span`. Every column is `Beach`, for the spawner.
    fn shore(span: i32) -> TestWorld {
        use primitive_shared::types::{BLOCK_SAND, BLOCK_WATER};
        let world = TestWorld::default();
        for z in -span..=span {
            for x in -span..=span {
                if x < 0 {
                    world.put(x, 20, z, BLOCK_SAND);
                } else {
                    world.put(x, 14, z, BLOCK_SAND);
                    for y in 15..=20 {
                        world.put(x, y, z, BLOCK_WATER);
                    }
                }
                for y in 21..=50 {
                    world.put(x, y, z, BLOCK_AIR);
                }
            }
        }
        world.set_biome(Some(primitive_shared::worldgen::Biome::Beach));
        world
    }

    #[test]
    fn a_palm_grove_on_the_sand_fills_with_monkeys_and_crabs_where_a_bare_beach_gets_only_crabs() {
        // **The bug this test exists for, and it would have shipped.** The
        // land spawner's ground test was `is_pasture` -- grass a tuft would
        // grow in -- because for eighteen species "will it stand here" and
        // "is there grass here" were the same question. A beach is sand, so
        // under that test a world had no crabs and no monkeys in it at all,
        // and nothing anywhere said so: the spawner simply returned. See
        // `spawn_ground`, and `wooded`, which had never heard of a palm
        // either.
        use primitive_shared::worldgen::Biome;
        let reach = SPAWN_MAX as i32 + GROUP_SPREAD as i32 + 8;
        let count = |world: &TestWorld| {
            let mut animals = Animals::seeded(91);
            let mut seen: Vec<Species> = Vec::new();
            for _ in 0..4000 {
                animals.populate(world, &player((-6.5, 21.0, 0.5)), false);
                for animal in animals.animals.iter() {
                    if !seen.contains(&animal.species) {
                        seen.push(animal.species);
                    }
                }
                animals.animals.clear();
            }
            seen
        };
        let bare = shore(reach);
        assert!(bare.biome(0, 0) == Some(Biome::Beach), "the fixture is not a beach");
        let on_bare = count(&bare);
        assert!(on_bare.contains(&Species::Crab), "a beach with no crabs on it: {on_bare:?}");
        assert!(
            !on_bare.contains(&Species::Monkey),
            "a troop of monkeys on a strand with nothing growing on it: {on_bare:?}"
        );

        // ...and the same sand with palms standing in it. Palms, not oaks: a
        // hot shore grows palms, and a palm's trunk is a *branch* to every
        // rule in the game, which is why `wooded` had to be told about it.
        //
        // The grove covers the whole ring the spawner reaches into
        // (`SPAWN_MIN`..`SPAWN_MAX`), because a spot is picked at a random
        // bearing and a copse the size of a copse would be found once in a
        // hundred tries -- which is a test that passes or fails on its seed.
        let grove = shore(reach);
        for dz in (-reach..=reach).step_by(4) {
            for dx in (-reach..-30).step_by(4) {
                for y in 21..27 {
                    grove.put(dx, y, dz, primitive_shared::types::BLOCK_PALM_TRUNK);
                }
                grove.put(dx, 27, dz, primitive_shared::types::BLOCK_PALM_FRONDS);
            }
        }
        let in_grove = count(&grove);
        assert!(in_grove.contains(&Species::Monkey), "a palm grove with no troop in it: {in_grove:?}");
    }

    #[test]
    fn gulls_spawn_only_near_the_sea() {
        use primitive_shared::worldgen::Biome;
        let world = shore(SPAWN_MAX as i32 + GROUP_SPREAD as i32 + 8);
        let who = player((-0.5, 21.0, 0.5));
        let spawned = |biome: Option<Biome>| -> Vec<Species> {
            world.set_biome(biome);
            let mut animals = Animals::seeded(81);
            let mut seen = Vec::new();
            for _ in 0..200 {
                animals.populate_shore(&world, &who, false);
                seen.extend(animals.animals.drain(..).map(|a| a.species));
            }
            seen
        };
        assert!(spawned(Some(Biome::Beach)).contains(&Species::Gull), "no gull over a beach in two hundred tries");
        assert!(spawned(Some(Biome::Ocean)).contains(&Species::Gull), "no gull over the sea in two hundred tries");
        for biome in [None, Some(Biome::Plains), Some(Biome::River), Some(Biome::Swamp)] {
            assert!(spawned(biome).is_empty(), "the coast spawner put something in {biome:?}");
        }
        // ...and the meadow's spawner never puts one on turf, even turf on a
        // beach: the flock is the shore's.
        let turf = meadow(SPAWN_MAX as i32 + GROUP_SPREAD as i32 + 2);
        turf.set_biome(Some(Biome::Beach));
        let mut animals = Animals::seeded(82);
        for _ in 0..200 {
            animals.populate(&turf, &player((0.5, 21.0, 0.5)), false);
            assert!(!animals.animals.iter().any(|a| a.species.soars()), "the land's spawner put a gull on the grass");
            animals.animals.clear();
        }
    }

    #[test]
    fn a_flock_over_the_shore_never_grows_past_its_own_allowance() {
        let world = shore(SPAWN_MAX as i32 + GROUP_SPREAD as i32 + 8);
        let mut animals = Animals::seeded(83);
        let who = player((-0.5, 21.0, 0.5));
        let mut most = 0;
        for _ in 0..400 {
            animals.populate_shore(&world, &who, false);
            let gulls = animals.animals.iter().filter(|a| a.species.soars()).count();
            assert!(gulls <= MAX_SEABIRDS_PER_PLAYER, "{gulls} gulls round one player");
            most = most.max(gulls);
        }
        assert!(most >= 2, "a coast never filled with more than {most} gull");
        // ...and a flock takes nothing from the meadow's three.
        let turf = meadow(SPAWN_MAX as i32 + GROUP_SPREAD as i32 + 2);
        let mut on_land = 0;
        for _ in 0..200 {
            animals.populate(&turf, &player((0.5, 21.0, 0.5)), false);
            on_land = animals.animals.iter().filter(|a| !a.species.soars()).count();
        }
        assert!(on_land > 0, "a full flock left no room for a single deer");
    }

    #[test]
    fn a_flying_gull_never_falls_through_or_tunnels_into_terrain() {
        // A cliff across the sand and a stack standing out of the sea -- the
        // two things a gull circling a coast flies into.
        let world = shore(40);
        for z in -40..=40 {
            for x in -12..=-10 {
                for y in 21..33 {
                    world.put(x, y, z, BLOCK_STONE);
                }
            }
        }
        for x in 6..9 {
            for z in -2..2 {
                for y in 15..34 {
                    world.put(x, y, z, BLOCK_STONE);
                }
            }
        }
        let mut animals = Animals::seeded(84);
        let id = animals.spawn(Species::Gull, (-3.5, 21.0, 0.5)).expect("a gull");
        animals.find_mut_for_test(id).expect("gull").home = Some((0.0, 0.0));
        let nobody = player((-30.0, 21.0, 60.0));
        let (mut flew, mut dived) = (false, false);
        // Where the five minutes went, printed, so a gull that never fishes
        // says whether it was down, off over the sand, or simply unlucky.
        let (mut soaring, mut soaring_over_sea, mut coming_down, mut sitting) = (0, 0, 0, 0);
        for tick in 0..6000 {
            animals.step(&world, &nobody, 0.05, NOON);
            let gull = animals.find(id).expect("the gull was lost");
            match gull.mind {
                Mind::Soar => {
                    soaring += 1;
                    if over_water(&world, gull) {
                        soaring_over_sea += 1;
                    }
                }
                Mind::Homing => coming_down += 1,
                Mind::Idle | Mind::Wander => sitting += 1,
                _ => {}
            }
            assert!(
                fits(&world, primitive_shared::geometry::wide(gull.at()), Species::Gull),
                "tick {tick}: the gull is inside a block at {:?}",
                gull.at()
            );
            assert!(gull.at().1 > 14.95, "tick {tick}: the gull is under the sea bed at {:?}", gull.at());
            if gull.at().0 < -0.5 {
                assert!(gull.at().1 > 20.95, "tick {tick}: the gull is under the sand at {:?}", gull.at());
            }
            flew |= gull.at().1 > 26.0;
            dived |= gull.dive_for > 0.0;
        }
        println!(
            "gull, 6000 ticks: soaring {soaring} (over the sea {soaring_over_sea}), coming down {coming_down}, sitting {sitting}"
        );
        assert!(flew, "the gull never went up in five minutes");
        assert!(dived, "the gull never fished in five minutes over the sea");
    }

    #[test]
    fn a_landed_gull_takes_off_when_a_player_comes_within_its_awareness() {
        let world = shore(40);
        let mut animals = Animals::seeded(85);
        let id = animals.spawn(Species::Gull, (-10.5, 21.0, 0.5)).expect("a gull");
        // Settled on the sand first, at night, with nobody near.
        for _ in 0..40 {
            animals.step(&world, &player((-10.5, 21.0, 60.0)), 0.05, 0.0);
        }
        assert!(animals.find(id).expect("gull").position.1 < 21.3, "the gull was not on the sand to begin with");
        // A roosting gull thinks every six to twelve seconds; the question
        // is what the next thought does, not how long the night's lasts.
        animals.find_mut_for_test(id).expect("gull").next_thought = 0.0;
        let near = Species::Gull.awareness() - 1.5;
        let mut up = false;
        // **Walking up**, at a walk, and stopping there. A person who was
        // simply *put* at that distance and stands still is a still figure,
        // which a gull picks out nearer than this (`Gait::visibility`).
        for tick in 0..100 {
            let off = (near + 12.0 - tick as f32 * 0.05 * 4.3).max(near);
            animals.step(&world, &player((-10.5 - off, 21.0, 0.5)), 0.05, NOON);
            let gull = animals.find(id).expect("gull");
            up |= gull.mind == Mind::Flee && gull.at().1 > 22.0;
        }
        assert!(up, "a gull let somebody walk to {near} blocks off without going up");
    }

    #[test]
    fn a_roosting_flock_lets_a_careful_hunter_nearer_after_dark() {
        let world = shore(40);
        let flushed_at = |hour: f32| -> bool {
            let mut animals = Animals::seeded(86);
            let id = animals.spawn(Species::Gull, (-10.5, 21.0, 0.5)).expect("a gull");
            for _ in 0..40 {
                animals.step(&world, &player((-10.5, 21.0, 60.0)), 0.05, 0.0);
            }
            // Inside a day's awareness and outside a night's -- and thinking
            // now, whatever the settling left its clock at.
            let close = Species::Gull.awareness() * 0.7;
            animals.find_mut_for_test(id).expect("gull").next_thought = 0.0;
            let mut flushed = false;
            for _ in 0..100 {
                animals.step(&world, &player((-10.5 - close, 21.0, 0.5)), 0.05, hour);
                flushed |= animals.find(id).expect("gull").mind == Mind::Flee;
            }
            flushed
        };
        assert!(flushed_at(NOON), "a gull by day let somebody stand inside its awareness");
        assert!(!flushed_at(0.0), "a roosting gull went up off somebody outside half its awareness");
    }

    #[test]
    fn a_fishing_gull_dives_to_the_water_and_climbs_back_out_of_it() {
        let world = shore(40);
        let mut animals = Animals::seeded(88);
        let id = animals.spawn(Species::Gull, (10.5, 30.0, 0.5)).expect("a gull");
        {
            let gull = animals.find_mut_for_test(id).expect("gull");
            gull.home = Some((10.0, 0.0));
            gull.mind = Mind::Soar;
            gull.bound_for = Some((10.0, 0.0));
            // No thinking for the length of the test: only the dive.
            gull.next_thought = 1000.0;
            gull.dive_for = DIVE_SECONDS;
        }
        let nobody = player((-30.0, 21.0, 60.0));
        let mut lowest = f32::MAX;
        for _ in 0..(DIVE_SECONDS / 0.05) as usize {
            animals.step(&world, &nobody, 0.05, NOON);
            let gull = animals.find(id).expect("gull");
            lowest = lowest.min(gull.at().1);
            assert!(!in_liquid(&world, primitive_shared::geometry::wide(gull.at()), Species::Gull), "the gull went into the sea at {:?}", gull.at());
        }
        assert!(lowest < 22.5, "a dive that never came near the water: lowest {lowest:.2}");
        for _ in 0..60 {
            animals.step(&world, &nobody, 0.05, NOON);
        }
        assert!(animals.find(id).expect("gull").position.1 > 26.0, "the gull stayed down on the sea after its dive");
    }

    #[test]
    fn a_gull_that_comes_down_glides_in_and_touches_down_softly() {
        // **"Gulls just fall when they land."** A gull flew in at nine
        // blocks, was called arrived the moment it was over its spot, and
        // the spring pulled it straight down at seven blocks a second with
        // its forward speed gone -- wings folded, falling. Two rules say
        // what a landing is instead, checked on every tick the bird is more
        // than a quarter of a block up (the last quarter is gravity's, as it
        // is for anything stepping off a stair): it never loses more height
        // than it covers across, give or take a flare's sink, and through
        // the last `FLARE_HEIGHT` it never sinks faster than `LANDING_SINK`.
        let world = shore(40);
        let mut animals = Animals::seeded(91);
        let id = animals.spawn(Species::Gull, (-15.5, 30.0, 0.5)).expect("a gull");
        {
            let gull = animals.find_mut_for_test(id).expect("gull");
            gull.home = Some((-15.0, 0.0));
            gull.mind = Mind::Soar;
            gull.bound_for = Some((-15.0, 0.0));
            gull.next_thought = 0.0;
        }
        let nobody = player((-30.0, 21.0, 60.0));
        const DT: f32 = 0.05;
        // The top of the sand, and of the sea beside it: `shore` lays both
        // at the one height, so a gull that drifts onto the water as it
        // touches down has come down on the same level.
        const DOWN: f32 = 21.0;
        let (mut came_in, mut landed) = (false, false);
        let mut last = animals.find(id).expect("gull").position;
        for tick in 0..2400 {
            // After dark, so the flock comes in (`seabird`) and stays down.
            animals.step(&world, &nobody, DT, 0.0);
            let gull = animals.find(id).expect("the gull was lost");
            let now = gull.at();
            came_in |= gull.mind == Mind::Homing;
            let (up, drop) = (last.1 - f64::from(DOWN), last.1 - f64::from(now.1));
            let across = (now.0 - last.0 as f32).hypot(now.2 - last.2 as f32);
            if came_in && up > 0.25 && drop > 0.0 {
                assert!(
                    drop <= f64::from(across + LANDING_SINK * DT + 1e-3),
                    "tick {tick}: lost {drop:.3} of height covering {across:.3} across, {up:.2} up -- that is a fall"
                );
                if up < f64::from(FLARE_HEIGHT) {
                    assert!(
                        drop <= f64::from(LANDING_SINK * DT + 1e-3),
                        "tick {tick}: came through the last block at {:.2} blocks a second",
                        drop / f64::from(DT)
                    );
                }
            }
            last = primitive_shared::geometry::wide(now);
            if came_in && gull.mind != Mind::Homing && (now.1 - DOWN).abs() < 0.05 && gull.velocity.1 == 0.0 {
                landed = true;
                break;
            }
        }
        assert!(came_in, "the gull never decided to come down in two minutes of night");
        assert!(landed, "the gull never touched down: {:?}", animals.find(id).expect("gull").position);
    }

    #[test]
    fn a_bird_leaving_its_tree_for_the_ground_glides_down_rather_than_dropping() {
        // The grouse's half of the gull's landing, on the same path: it
        // climbs off the crown, crosses the meadow at its cruise and comes
        // in to its home on the turf. Held to the same two rules -- see
        // `a_gull_that_comes_down_glides_in_and_touches_down_softly`.
        let world = wood(40);
        tree(&world, (10, 0), 26, None);
        let mut animals = Animals::seeded(105);
        let id = animals.spawn(Species::Fowl, (10.5, 27.0, 0.5)).expect("a bird");
        {
            let bird = animals.find_mut_for_test(id).expect("bird");
            // Further than `HOME_RANGE`, so the first thought is to go there.
            bird.home = Some((-10.5, 0.5));
            bird.next_thought = 0.0;
        }
        let watcher = player((0.5, 21.0, 70.0));
        const DT: f32 = 0.05;
        const TURF: f32 = 21.0;
        let (mut flew, mut landed) = (false, false);
        let mut last = animals.find(id).expect("bird").position;
        for tick in 0..1200 {
            animals.step(&world, &watcher, DT, NOON);
            let bird = animals.find(id).expect("the bird was lost");
            let now = bird.at();
            flew |= bird.mind == Mind::Homing;
            // Over the meadow only: over the crown the ground is six blocks
            // higher and "up" would be measured from the wrong floor.
            let over_turf = now.0 < 8.5 || now.0 > 12.5;
            let (up, drop) = (last.1 - f64::from(TURF), last.1 - f64::from(now.1));
            let across = (now.0 - last.0 as f32).hypot(now.2 - last.2 as f32);
            if flew && over_turf && up > 0.25 && drop > 0.0 {
                assert!(
                    drop <= f64::from(across + LANDING_SINK * DT + 1e-3),
                    "tick {tick}: lost {drop:.3} of height covering {across:.3} across, {up:.2} up -- that is a fall"
                );
                if up < f64::from(FLARE_HEIGHT) {
                    assert!(
                        drop <= f64::from(LANDING_SINK * DT + 1e-3),
                        "tick {tick}: came through the last block at {:.2} blocks a second",
                        drop / f64::from(DT)
                    );
                }
            }
            last = primitive_shared::geometry::wide(now);
            if flew && bird.on_ground && bird.mind != Mind::Homing && (now.1 - TURF).abs() < 0.05 {
                landed = true;
                break;
            }
        }
        assert!(flew, "it never left the tree");
        assert!(landed, "it never came down on the turf: {:?}", animals.find(id).expect("bird").position);
    }

    #[test]
    fn a_covey_goes_up_off_a_wolf_that_is_not_hunting_it() {
        // **The warning.** A wolf does not hunt a grouse (`Species::hunts`),
        // and a fed one hunts nothing -- and until birds flushed off anything
        // hostile, a covey sat in the grass while one walked by, telling a
        // player watching from the ridge nothing at all.
        let world = meadow(30);
        let mut animals = Animals::seeded(87);
        let bird = animals.spawn(Species::Fowl, (0.5, 21.0, 0.5)).expect("a bird");
        let wolf = animals.spawn(Species::Wolf, (6.5, 21.0, 0.5)).expect("a wolf");
        animals.find_mut_for_test(wolf).expect("wolf").fed_for = 1000.0;
        let far = player((0.5, 21.0, 60.0));
        let mut flushed = false;
        for _ in 0..80 {
            animals.step(&world, &far, 0.05, NOON);
            flushed |= animals.find(bird).is_some_and(|b| b.mind == Mind::Flee);
        }
        assert!(flushed, "a bird sat six blocks from a wolf and told nobody");
    }

    /// What one player on a coast costs: the meadow's three behind the dunes,
    /// with and without a full flock over the beach.
    ///
    /// ```text
    /// cargo test -p primitive_server --lib a_shore_with_a_flock_over_it_ticking -- --ignored --nocapture
    /// ```
    #[test]
    #[ignore = "a measurement, not an assertion -- see logic::animals's module doc"]
    fn a_shore_with_a_flock_over_it_ticking() {
        use std::cell::Cell;
        use std::time::Instant;
        struct Counting<'a> {
            inner: &'a TestWorld,
            lookups: Cell<u64>,
        }
        impl BlockWorld for Counting<'_> {
            fn block(&self, x: i32, y: i32, z: i32) -> Option<primitive_shared::types::BlockId> {
                self.lookups.set(self.lookups.get() + 1);
                self.inner.block(x, y, z)
            }
            fn set(&self, x: i32, y: i32, z: i32, block: primitive_shared::types::BlockId) {
                self.inner.set(x, y, z, block)
            }
            fn biome(&self, x: i32, z: i32) -> Option<primitive_shared::worldgen::Biome> {
                self.inner.biome(x, z)
            }
        }
        let beach = shore(60);
        for z in -60..=60 {
            for x in -60..-20 {
                beach.put(x, 20, z, BLOCK_GRASS);
            }
        }
        let world = Counting { inner: &beach, lookups: Cell::new(0) };
        let who = player((-15.0, 21.0, 0.5));
        for flock in [0usize, MAX_SEABIRDS_PER_PLAYER] {
            let mut animals = Animals::seeded(90);
            // The same animals for the whole run: a spawner filling the
            // shore halfway through measures the spawner.
            animals.next_spawn = f32::INFINITY;
            for i in 0..3 {
                animals.spawn(Species::Deer, (-30.5 - i as f32 * 2.0, 21.0, 0.5));
            }
            for i in 0..flock {
                let id = animals.spawn(Species::Gull, (4.5 + i as f32, 30.0, 0.5)).expect("gull");
                let gull = animals.find_mut_for_test(id).expect("gull");
                gull.home = Some((4.0, 0.0));
                gull.mind = Mind::Soar;
                gull.bound_for = Some((4.0, 0.0));
            }
            const TICKS: usize = 2000;
            world.lookups.set(0);
            let started = Instant::now();
            for _ in 0..TICKS {
                std::hint::black_box(animals.step(&world, &who, 0.05, NOON));
            }
            let ms = started.elapsed().as_secs_f64() * 1000.0 / TICKS as f64;
            println!(
                "shore, {} animals ({flock} gulls): {ms:.4} ms/tick, {:.0} block reads/tick",
                animals.len(),
                world.lookups.get() as f64 / TICKS as f64
            );
        }
    }

    // ---- what a full field of animals costs ----
    //
    // Run with:
    // cargo test -p primitive_server --release --lib -- --ignored --nocapture herd_of_a_hundred_and_twenty
    // ---- the water's animals ----

    /// A coast: open water west of x = 0, `depth` blocks deep with its
    /// surface in the cell at y = 20, and a meadow at y = 20 east of it.
    /// Stone under both, so every column has a floor.
    fn coast(span: i32, depth: i32) -> TestWorld {
        use primitive_shared::types::BLOCK_WATER;
        let world = TestWorld::default();
        for z in -span..=span {
            for x in -span..=span {
                for y in 20 - depth - 1..=30 {
                    let id = if x < 0 {
                        if y <= 20 - depth {
                            BLOCK_STONE
                        } else if y <= 20 {
                            BLOCK_WATER
                        } else {
                            BLOCK_AIR
                        }
                    } else if y < 20 {
                        BLOCK_STONE
                    } else if y == 20 {
                        BLOCK_GRASS
                    } else {
                        BLOCK_AIR
                    };
                    world.put(x, y, z, id);
                }
            }
        }
        world
    }

    fn fish_in(animals: &Animals) -> Vec<&Animal> {
        animals.animals.iter().filter(|a| a.species.swims()).collect()
    }

    #[test]
    fn a_school_is_put_in_water_deep_enough_to_swim_in_and_never_on_the_bank() {
        use primitive_shared::worldgen::Biome;
        let world = coast(FISH_SPAWN_MAX as i32 + 8, 9);
        world.set_biome(Some(Biome::Ocean));
        let mut animals = Animals::seeded(301);
        let players = player((0.5, 21.0, 0.5));
        let mut spawned = 0;
        for _ in 0..400 {
            animals.populate_water(&world, &players);
            for fish in fish_in(&animals) {
                assert!(fish.at().0 < 0.0, "a {} was put on the meadow at {:?}", fish.species.name(), fish.at());
                assert!(
                    in_water(&world, primitive_shared::geometry::wide(fish.at()), fish.species),
                    "a {} was put half out of the water at {:?}",
                    fish.species.name(),
                    fish.at()
                );
            }
            spawned += animals.animals.len();
            animals.animals.clear();
        }
        assert!(spawned > 20, "only {spawned} fish in four hundred tries at an open sea");
    }

    #[test]
    fn a_puddle_holds_no_fish_and_only_the_open_sea_holds_a_cod() {
        use primitive_shared::worldgen::Biome;
        let spawned = |depth: i32, biome: Biome| -> Vec<Species> {
            let world = coast(FISH_SPAWN_MAX as i32 + 8, depth);
            world.set_biome(Some(biome));
            let mut animals = Animals::seeded(307);
            let mut seen = Vec::new();
            for _ in 0..600 {
                animals.populate_water(&world, &player((0.5, 21.0, 0.5)));
                seen.extend(animals.animals.drain(..).map(|a| a.species));
            }
            seen
        };
        assert!(spawned(1, Biome::Ocean).is_empty(), "a school in water one block deep");
        let river = spawned(9, Biome::River);
        assert!(river.contains(&Species::Fish), "no school in a deep river");
        assert!(!river.contains(&Species::Cod), "a cod in a river");
        assert!(spawned(9, Biome::Ocean).contains(&Species::Cod), "no cod in nine blocks of sea");
        assert!(!spawned(5, Biome::Ocean).contains(&Species::Cod), "a cod in the surf");
    }

    #[test]
    fn a_trout_a_pike_and_a_herring_are_each_only_in_their_own_water() {
        // **The spawner's half of the map of the water.** `Species::lives_in`
        // is the table and `each_kind_of_water_has_its_own_fish` is the test
        // of the table; this is the thing that actually puts fish in a lake,
        // asked the same question -- because a rule nothing reads is a rule
        // that is not there. The depth is the second half of it: a pike
        // wants three blocks of water (`needs_depth`) and a river two
        // blocks deep is a trout stream and nothing else.
        use primitive_shared::worldgen::Biome;
        let spawned = |depth: i32, biome: Biome| -> Vec<Species> {
            let world = coast(FISH_SPAWN_MAX as i32 + 8, depth);
            world.set_biome(Some(biome));
            let mut animals = Animals::seeded(313);
            let mut seen = Vec::new();
            for _ in 0..600 {
                animals.populate_water(&world, &player((0.5, 21.0, 0.5)));
                seen.extend(animals.animals.drain(..).map(|a| a.species));
            }
            seen
        };
        let river = spawned(4, Biome::River);
        assert!(river.contains(&Species::Trout), "no trout in a river");
        assert!(!river.contains(&Species::Pike), "a pike in running water");
        assert!(!river.contains(&Species::Herring), "a herring up a river");

        let lake = spawned(5, Biome::Plains);
        assert!(lake.contains(&Species::Pike), "no pike in a meadow lake five blocks deep");
        assert!(!lake.contains(&Species::Trout), "a trout in a warm lake");

        // ...and the same meadow with a shallower pond in it is a pond a
        // player wades across: still fish in it, and no pike. (The bank in
        // `coast` puts the water one cell under the number it is given, so
        // this is a pond two blocks deep and the lake above is four.)
        let pond = spawned(3, Biome::Plains);
        assert!(pond.contains(&Species::Fish), "a shallow pond with nothing in it");
        assert!(!pond.contains(&Species::Pike), "a pike in a pond a player can wade across");

        let sea = spawned(4, Biome::Ocean);
        assert!(sea.contains(&Species::Herring), "no herring in the sea off a beach");
        assert!(!sea.contains(&Species::Trout) && !sea.contains(&Species::Pike), "fresh water fish in the sea");

        // A cold country's lake is the trout's as well as the river's, and
        // the taiga is where the two tables were most likely to overlap.
        let taiga = spawned(4, Biome::Taiga);
        assert!(taiga.contains(&Species::Trout), "no trout in a taiga lake");
        assert!(!taiga.contains(&Species::Pike), "a pike in the taiga");
    }

    #[test]
    fn a_shoal_of_herring_swims_nearer_the_surface_than_a_cod_lies_to_the_bed() {
        // The bands, which are the reason the two sea fish are different
        // places rather than two names: a herring is within a spear of the
        // top of the water and a cod is on the shelf under it. See
        // `swimming_band`.
        let (floor, surface) = (10.0, 20.0);
        let (herring_low, herring_high) = swimming_band(Species::Herring, floor, surface);
        let (cod_low, cod_high) = swimming_band(Species::Cod, floor, surface);
        assert!(herring_low > cod_high, "a herring can swim as deep as a cod lies");
        assert!(surface - herring_high < 1.0, "a shoal is not at the surface");
        assert!(cod_low - floor < 1.0, "a cod is not on the bottom");
    }

    #[test]
    fn a_fish_never_leaves_the_water_whatever_frightens_it() {
        // A narrow bay two blocks deep against the bank, and a player who
        // wades back and forth along the waterline: the school bolts at the
        // shore over and over, and not once may a fish's box be anything
        // but water.
        let world = coast(24, 2);
        let mut animals = Animals::seeded(311);
        // Two blocks of water over a floor at y = 18: the cells are 19 and
        // 20, and the school starts just above the floor of the first.
        for i in 0..5 {
            animals.spawn(Species::Fish, (-3.5 - i as f32 * 0.6, 19.2, 0.5)).expect("fish");
        }
        let mut t = 0.0f32;
        for tick in 0..3000 {
            t += 0.05;
            let walker = (0.5 + (t * 0.7).sin() * 2.0, 20.0, (t * 0.3).sin() * 10.0);
            animals.step(&world, &player(walker), 0.05, NOON);
            for fish in fish_in(&animals) {
                assert!(
                    in_water(&world, primitive_shared::geometry::wide(fish.at()), fish.species),
                    "tick {tick}: a fish is out of the water at {:?}",
                    fish.at()
                );
            }
        }
        assert_eq!(fish_in(&animals).len(), 5, "a fish died in its own water");
    }

    #[test]
    fn a_fish_that_sees_a_swimmer_leaves_and_its_school_goes_with_it() {
        let world = coast(40, 9);
        let mut animals = Animals::seeded(313);
        let school: Vec<EntityId> = (0..4)
            .map(|i| animals.spawn(Species::Fish, (-20.5 + i as f32 * 0.7, 15.5, 0.5)).expect("fish"))
            .collect();
        // Settle, with nobody near.
        for _ in 0..40 {
            animals.step(&world, &player((30.5, 21.0, 30.5)), 0.05, NOON);
        }
        let middle = |animals: &Animals| {
            let fish = fish_in(animals);
            let n = fish.len() as f32;
            (fish.iter().map(|f| f.at().0).sum::<f32>() / n, fish.iter().map(|f| f.at().2).sum::<f32>() / n)
        };
        let before = middle(&animals);
        // A swimmer two blocks off the first fish, level with it.
        let swimmer = (before.0 + 2.0, 14.6, before.1);
        // Counted over the whole run rather than read at the end: a fish
        // that bolted in the first second is six blocks off by the last
        // tick, has thought again, and is drifting back to its school --
        // which is the behaviour, and a check of the last tick alone
        // reported it as a fish that never ran.
        let mut bolted = std::collections::HashSet::new();
        for _ in 0..40 {
            animals.step(&world, &player(swimmer), 0.05, NOON);
            for id in &school {
                if animals.find(*id).is_some_and(|f| f.mind == Mind::Flee) {
                    bolted.insert(*id);
                }
            }
        }
        let fled = bolted.len();
        assert!(fled >= 3, "only {fled} of a school of four bolted from a swimmer beside it");
        let after = middle(&animals);
        let (d0, d1) = (
            ((before.0 - swimmer.0).powi(2) + (before.1 - swimmer.2).powi(2)).sqrt(),
            ((after.0 - swimmer.0).powi(2) + (after.1 - swimmer.2).powi(2)).sqrt(),
        );
        assert!(d1 > d0 + 2.0, "the school is {d1:.1} from the swimmer, having been {d0:.1}");
    }

    #[test]
    fn a_fish_left_on_dry_land_dies_of_it() {
        let world = meadow(12);
        let mut animals = Animals::seeded(317);
        let id = animals.spawn(Species::Fish, (0.5, 21.0, 0.5)).expect("fish");
        let limit = ((crate::logic::survival::BREATH_SECONDS + 5.0) / 0.05) as usize;
        for _ in 0..limit {
            animals.step(&world, &player((4.5, 21.0, 0.5)), 0.05, NOON);
            if animals.find(id).is_none() {
                return;
            }
        }
        panic!("a fish lay on a meadow for {} seconds and lived", limit as f32 * 0.05);
    }

    #[test]
    fn the_sea_keeps_its_own_count_and_leaves_the_land_its_three() {
        use primitive_shared::worldgen::Biome;
        let world = coast(SPAWN_MAX as i32 + GROUP_SPREAD as i32 + 4, 9);
        world.set_biome(Some(Biome::Ocean));
        let mut animals = Animals::seeded(331);
        let players = player((0.5, 21.0, 0.5));
        for _ in 0..800 {
            animals.populate(&world, &players, false);
            animals.populate_water(&world, &players);
        }
        let (land, sea): (Vec<&Animal>, Vec<&Animal>) = animals.animals.iter().partition(|a| !a.species.swims());
        assert!(!land.is_empty() && !sea.is_empty(), "{} on land and {} in the sea", land.len(), sea.len());
        // The land's allowance decides whether a group may start, and the
        // group then arrives whole (`animals::MAX_GROUP`).
        assert!(
            land.len() < MAX_ANIMALS_PER_PLAYER + primitive_shared::animals::MAX_GROUP,
            "{} animals on land for one player",
            land.len()
        );
        assert!(sea.len() <= MAX_FISH_PER_PLAYER, "{} fish for one player", sea.len());
        // ...and a full sea did not stop the land filling, which is the
        // reason there are two counts.
        assert!(land.len() >= MAX_ANIMALS_PER_PLAYER, "the fish took the meadow's allowance");
    }

    #[test]
    fn a_fish_out_of_sight_is_forgotten_before_a_deer_at_the_same_distance() {
        let mut animals = Animals::seeded(337);
        let far = (FISH_DESPAWN_DISTANCE + DESPAWN_DISTANCE) * 0.5;
        let fish = animals.spawn(Species::Fish, (-far, 15.5, 0.5)).expect("fish");
        let deer = animals.spawn(Species::Deer, (0.5 + far, 21.0, 0.5)).expect("deer");
        animals.forget_the_distant(&player((0.5, 21.0, 0.5)));
        assert!(animals.find(fish).is_none(), "a fish {far} blocks off is still simulated");
        assert!(animals.find(deer).is_some(), "a deer {far} blocks off was forgotten");
    }

    #[test]
    #[ignore = "a measurement, not an assertion -- see logic::animals's module doc"]
    fn herd_of_a_hundred_and_twenty_animals_ticking() {
        herd_benchmark("provoked, next to a player", (0.5, 21.0, 0.5));
    }

    #[test]
    #[ignore = "a measurement, not an assertion -- see logic::animals's module doc"]
    fn herd_of_a_hundred_and_twenty_animals_grazing_unwatched() {
        // The common case a server actually spends most of its time in:
        // a full field, nobody provoked, nobody fleeing, a player only
        // close enough to keep the herd from being despawned. This is
        // the scenario `survey`'s per-tick full neighbour scan was
        // computing and throwing away nineteen ticks out of twenty --
        // see the doc comment on `survey`.
        herd_benchmark("calm, player at the edge of despawn range", (0.5, 21.0, 55.0));
    }

    // ---- the senses ----

    /// A figure for the pure perception tests: a person moving at `speed`,
    /// in the open, by day, standing, whole.
    fn figure_at(at: (f32, f32, f32), speed: f32) -> Figure {
        let gait = Gait::of_speed(speed);
        Figure { who: 1, at, loudness: gait.loudness(), visibility: gait.visibility(), facing: None, wounded: false, asleep: false, held: None, reek: 1.0 }
    }

    /// The furthest a person moving at `speed` is noticed by one of these,
    /// on open ground by day in still air, straight ahead of it or straight
    /// behind: stepping in a quarter of a block at a time from far off.
    fn noticed_at(species: Species, speed: f32, behind: bool, sure: bool) -> f32 {
        let world = meadow(50);
        let mut animals = Animals::seeded(1);
        let id = animals.spawn(species, (0.5, 21.0, 0.5)).expect("spawned");
        animals.face_for_test(id, if behind { std::f32::consts::PI } else { 0.0 });
        let animal = animals.find(id).expect("alive");
        let mut distance = 45.0;
        while distance > 0.0 {
            let mut rays = u32::MAX;
            let seen = perceive(&world, animal, &figure_at((0.5 + distance, 21.0, 0.5), speed), (0.0, 0.0), 1.0, &mut rays);
            if seen == Perception::Sure || (!sure && seen == Perception::Maybe) {
                return distance;
            }
            distance -= 0.25;
        }
        0.0
    }

    /// A walled room nine blocks across, on the meadow, open to the sky.
    fn storeroom() -> TestWorld {
        let world = meadow(30);
        for i in -5..=5 {
            for y in 21..=22 {
                for (x, z) in [(i, -5), (i, 5), (-5, i), (5, i)] {
                    world.put(x, y, z, BLOCK_STONE);
                }
            }
        }
        world
    }

    /// The share of ticks this species, alone in `storeroom`, spends with
    /// a wall beside it.
    fn time_by_the_wall(species: Species, seed: u64) -> f32 {
        let world = storeroom();
        let mut animals = Animals::seeded(seed);
        let id = animals.spawn(species, (0.5, 21.0, 0.5)).expect("spawned");
        let (mut beside, ticks) = (0, 3600);
        // Somebody in the world, or there is nobody for it to exist
        // near; far enough off and behind stone that it never knows.
        let away = player((0.5, 21.0, -26.0));
        for _ in 0..ticks {
            animals.step(&world, &away, 0.05, NOON);
            let at = animals.find(id).expect("alive").at();
            assert!(at.0.abs() < 5.0 && at.2.abs() < 5.0, "it got out of a closed room");
            beside += usize::from(walls_beside(&world, at).iter().any(|&w| w));
        }
        beside as f32 / ticks as f32
    }

    #[test]
    fn a_rat_in_a_room_spends_most_of_its_time_against_a_wall() {
        // A cell by the wall is 32 of the room's 81, so an animal that
        // went anywhere at random would be there about two fifths of the
        // time. The hare is the check that the room is not simply small.
        for seed in [1, 2, 3] {
            let rat = time_by_the_wall(Species::Rat, seed);
            let hare = time_by_the_wall(Species::Hare, seed);
            assert!(rat > 0.75, "seed {seed}: a rat was by a wall only {:.0}% of the time", rat * 100.0);
            assert!(rat > hare, "seed {seed}: a rat hugged the wall no more than a hare ({rat:.2} vs {hare:.2})");
        }
    }

    #[test]
    fn a_rat_notices_a_still_person_nearer_than_a_hare_and_has_a_blind_side() {
        // It used to be nine blocks either way: scent alone, with no front
        // and no back. See `Species::nose` on the rat.
        let front = noticed_at(Species::Rat, 0.0, false, false);
        let back = noticed_at(Species::Rat, 0.0, true, false);
        assert!(back < front, "a rat knew a person behind it as well as one in front ({back} vs {front})");
        assert!(front <= noticed_at(Species::Hare, 0.0, false, false), "a rat sees further than a hare");
        assert!(front < primitive_shared::combat::SPEAR_REACH, "a rat bolts before a spear can reach it ({front})");
    }

    #[test]
    fn a_running_player_is_noticed_at_more_than_twice_the_distance_of_a_creeping_one() {
        // **The stalk, as a number.** It used to be a radius: a deer knew a
        // sprinter and a creeper at the same twelve blocks. Checked on every
        // animal that walks, facing the person and with its back to them --
        // and through the simulation for the deer, with a person really
        // running round it and really creeping, so the gait is the one the
        // server reads off positions and not one a test handed in.
        for &species in Species::ALL.iter().filter(|s| !s.swims()) {
            for behind in [false, true] {
                let running = noticed_at(species, NOMINAL_SPRINT_SPEED_FOR_TESTS, behind, false);
                let creeping = noticed_at(species, 1.5, behind, false);
                assert!(
                    running > creeping * 2.0,
                    "a {} {} notices a sprinter at {running} and a creeper at {creeping}",
                    species.name(),
                    if behind { "facing away" } else { "facing the person" }
                );
            }
        }

        let first_noticed = |speed: f32| -> f32 {
            let world = meadow(50);
            let mut best = 0.0f32;
            for distance in (4..=30).rev() {
                let distance = distance as f32;
                let mut animals = Animals::seeded(3);
                let id = animals.spawn(Species::Deer, (0.5, 21.0, 0.5)).expect("deer");
                let mut noticed = false;
                for tick in 0..60 {
                    // Round the deer at `distance`, at `speed`, so the range
                    // stays put while the feet move.
                    let angle = tick as f32 * 0.05 * speed / distance;
                    let at = (0.5 + distance * angle.cos(), 21.0, 0.5 + distance * angle.sin());
                    animals.step(&world, &player(at), 0.05, NOON);
                    noticed |= matches!(animals.find(id).expect("alive").mind, Mind::Flee | Mind::Watch);
                }
                if noticed {
                    best = distance;
                    break;
                }
            }
            best
        };
        let (running, creeping) = (first_noticed(NOMINAL_SPRINT_SPEED_FOR_TESTS), first_noticed(1.5));
        assert!(
            running > creeping * 2.0,
            "in the meadow a deer noticed a sprinter at {running} blocks and a creeper at {creeping}"
        );
    }

    /// A sprint, for the tests: the shared crate's figure.
    const NOMINAL_SPRINT_SPEED_FOR_TESTS: f32 = primitive_shared::animals::NOMINAL_SPRINT_SPEED;

    #[test]
    fn cover_and_darkness_shorten_sight_and_a_torch_undoes_the_dark() {
        let world = meadow(40);
        let mut animals = Animals::seeded(2);
        let id = animals.spawn(Species::Deer, (0.5, 21.0, 0.5)).expect("deer");
        animals.face_for_test(id, 0.0);
        let still = (0.5 + 6.0, 21.0, 0.5);
        let players = player(still);
        let visibility = |animals: &mut Animals, world: &TestWorld, night: bool| {
            animals.figures(world, &players, 0.05, night)[0].visibility
        };
        let open_day = visibility(&mut animals, &world, false);
        let open_night = visibility(&mut animals, &world, true);
        world.put(6, 21, 0, primitive_shared::types::BLOCK_TALL_GRASS);
        let grass_day = visibility(&mut animals, &world, false);
        world.put(6, 21, 0, BLOCK_AIR);
        animals.carrying_fire(vec![1]);
        let torch_night = visibility(&mut animals, &world, true);
        world.put(6, 21, 0, primitive_shared::types::BLOCK_TALL_GRASS);
        assert!(open_night < open_day * 0.6, "the dark hid nothing: {open_night} against {open_day}");
        assert!(grass_day < open_day * 0.6, "tall grass hid nothing: {grass_day} against {open_day}");
        assert!(torch_night > open_night * 2.0, "a torch in the dark is not the brightest thing there");
        // ...and a still figure in the grass six blocks off, by day, is one a
        // deer does not see, where one on bare ground at the same place is.
        let deer = animals.find(id).expect("alive");
        let mut rays = u32::MAX;
        let hidden = Figure { visibility: grass_day, ..figure_at(still, 0.0) };
        let plain = Figure { visibility: open_day, ..figure_at(still, 0.0) };
        assert_eq!(perceive(&world, deer, &plain, (0.0, 0.0), 1.0, &mut rays), Perception::Sure);
        assert_ne!(perceive(&world, deer, &hidden, (0.0, 0.0), 1.0, &mut rays), Perception::Sure);
    }

    #[test]
    fn a_hunter_who_comes_from_downwind_gets_nearer_than_one_who_comes_upwind() {
        // **The wind, as a decision.** The same creep through the same grass
        // toward the same deer, from its tail: with the wind blowing from the
        // hunter to the deer the scent arrives long before the hunter does,
        // and with the wind at the hunter's face it does not arrive at all.
        let approach = |wind: (f32, f32)| -> f32 {
            let world = meadow(50);
            for z in -3..=3 {
                for x in -45..=45 {
                    world.put(x, 21, z, primitive_shared::types::BLOCK_TALL_GRASS);
                }
            }
            let mut animals = Animals::seeded(12);
            animals.feel_wind(wind);
            let id = animals.spawn(Species::Deer, (0.5, 21.0, 0.5)).expect("deer");
            let mut distance = 30.0f32;
            while distance > 1.0 {
                // Tail to the hunter, every tick: this is about the nose and the
                // ears, and a deer that turned to look would be about the eyes.
                animals.face_for_test(id, 0.0);
                if let Some(deer) = animals.find_mut_for_test(id) {
                    deer.position = (0.5, 21.0, 0.5);
                    deer.next_thought = deer.next_thought.min(0.5);
                }
                animals.step(&world, &player((0.5 - distance, 21.0, 0.5)), 0.05, NOON);
                if matches!(animals.find(id).expect("alive").mind, Mind::Flee | Mind::Watch) {
                    return distance;
                }
                distance -= 1.5 * 0.05;
            }
            0.0
        };
        // Blowing +x: from the hunter (at -x) to the deer.
        let downwind = approach((0.6, 0.0));
        let upwind = approach((-0.6, 0.0));
        assert!(
            downwind > upwind * 2.0,
            "with the wind behind the hunter the deer knew at {downwind:.1} blocks, into it at {upwind:.1}"
        );
    }

    #[test]
    fn a_deer_winds_a_hunter_in_a_tarred_coat_from_further_off_than_one_in_leather() {
        // The same downwind creep as above, once in plain leather and once in
        // the tarred coat that keeps the rain out (`equipment::reek`).
        let approach = |reek: f32| -> f32 {
            let world = meadow(50);
            for z in -3..=3 {
                for x in -45..=45 {
                    world.put(x, 21, z, primitive_shared::types::BLOCK_TALL_GRASS);
                }
            }
            let mut animals = Animals::seeded(12);
            animals.feel_wind((0.6, 0.0));
            let id = animals.spawn(Species::Deer, (0.5, 21.0, 0.5)).expect("deer");
            let mut distance = 45.0f32;
            while distance > 1.0 {
                animals.face_for_test(id, 0.0);
                if let Some(deer) = animals.find_mut_for_test(id) {
                    deer.position = (0.5, 21.0, 0.5);
                    deer.next_thought = deer.next_thought.min(0.5);
                }
                animals.player_signs(vec![PlayerSign {
                    who: 1,
                    facing: 0.0,
                    working: false,
                    airborne: false,
                    low: false,
                    wounded: false,
                    asleep: false,
                    held: None,
                    reek,
                }]);
                animals.step(&world, &player((0.5 - distance, 21.0, 0.5)), 0.05, NOON);
                if matches!(animals.find(id).expect("alive").mind, Mind::Flee | Mind::Watch) {
                    return distance;
                }
                distance -= 1.5 * 0.05;
            }
            0.0
        };
        let leather = approach(1.0);
        let tarred = approach(primitive_shared::equipment::TAR_REEK);
        assert!(
            tarred > leather * 1.3,
            "the deer knew the tarred coat at {tarred:.1} blocks and the leather at {leather:.1}"
        );
    }

    #[test]
    fn a_herd_member_that_could_not_see_or_hear_the_player_bolts_when_one_that_could_does() {
        // The second deer is behind a wall of stone from the player and too
        // far off to smell or hear a person standing still -- and it goes
        // anyway, because it hears the first one go.
        let world = meadow(60);
        for z in -6..=6 {
            for y in 21..=24 {
                world.put(6, y, z, BLOCK_STONE);
            }
        }
        let mut animals = Animals::seeded(14);
        let near = animals.spawn(Species::Deer, (0.5, 21.0, 0.5)).expect("deer");
        let hidden = animals.spawn(Species::Deer, (9.5, 21.0, 0.5)).expect("deer");
        animals.face_for_test(near, std::f32::consts::PI);
        let who = (-5.0, 21.0, 0.5);
        {
            let far = animals.find(hidden).expect("alive");
            assert!(!sees(&world, far, player_eye(who)), "the wall does not hide the player");
            let figure = figure_at(who, 0.0);
            let mut rays = u32::MAX;
            assert_eq!(perceive(&world, far, &figure, (0.0, 0.0), 1.0, &mut rays), Perception::Nothing);
        }
        let mut bolted = false;
        for _ in 0..80 {
            animals.step(&world, &player(who), 0.05, NOON);
            if animals.find(hidden).expect("alive").mind == Mind::Flee {
                bolted = true;
                break;
            }
        }
        assert!(bolted, "the deer behind the wall grazed on while its herd ran");
    }

    #[test]
    fn a_deer_that_hears_something_far_off_lifts_its_head_before_it_runs() {
        // The stalker's warning: a walk heard from the edge of its hearing is a
        // maybe, and a maybe is a look -- `Mind::Watch`, facing the sound.
        let world = meadow(40);
        let mut animals = Animals::seeded(15);
        let id = animals.spawn(Species::Deer, (0.5, 21.0, 0.5)).expect("deer");
        animals.face_for_test(id, 0.0);
        // It does not think again until the walker's gait has been read.
        animals.find_mut_for_test(id).expect("deer").next_thought = 1.0;
        // Behind it, at the far edge of its hearing for a walk, walking to and
        // fro across its tail -- inside the blind wedge the whole way.
        let distance = Species::Deer.hearing() * 0.85;
        let mut watched = false;
        for tick in 0..60 {
            let t = tick as f32 * 0.05 * 4.3;
            let across = if (t / 10.0) as i32 % 2 == 0 { t % 10.0 - 5.0 } else { 5.0 - t % 10.0 };
            let at = (0.5 - distance, 21.0, 0.5 + across);
            animals.step(&world, &player(at), 0.05, NOON);
            let deer = animals.find(id).expect("alive");
            if deer.mind == Mind::Flee && !watched {
                panic!("it bolted from a maybe without looking up first");
            }
            watched |= deer.mind == Mind::Watch;
        }
        assert!(watched, "it never lifted its head at a walker {distance:.1} blocks behind it");
    }

    #[test]
    fn a_deer_flees_round_a_wall_rather_than_into_it() {
        // A wall across its line of escape with a gap at one end only: the
        // way out is along the wall to the open end, and a deer that took the
        // first open swerve went into the closed corner and stood there.
        let world = meadow(40);
        for z in -12..=6 {
            for y in 21..=23 {
                world.put(12, y, z, BLOCK_STONE);
            }
        }
        // ...and the closed end turned back toward the player, making a
        // pocket.
        for x in 6..=12 {
            for y in 21..=23 {
                world.put(x, y, -12, BLOCK_STONE);
            }
        }
        let mut animals = Animals::seeded(16);
        let id = animals.spawn(Species::Deer, (9.5, 21.0, -3.5)).expect("deer");
        let mut stuck = 0;
        let mut longest_stuck = 0;
        let mut escaped = false;
        for tick in 0..240 {
            // The player walks after it.
            let at = animals.find(id).expect("alive").at();
            let hunter = (at.0 - 5.0, 21.0, at.2);
            animals.step(&world, &player(hunter), 0.05, NOON);
            let deer = animals.find(id).expect("alive");
            let speed = deer.velocity.0.hypot(deer.velocity.2);
            if deer.mind == Mind::Flee && speed < 0.5 && tick > 10 {
                stuck += 1;
                longest_stuck = longest_stuck.max(stuck);
            } else {
                stuck = 0;
            }
            if deer.at().0 > 13.0 {
                escaped = true;
                break;
            }
        }
        assert!(escaped, "the deer never got round the wall: {:?}", animals.find(id).expect("alive").at());
        assert!(longest_stuck < 20, "it stood against the wall for {} ticks", longest_stuck);
    }

    #[test]
    fn a_herd_follows_its_leader_rather_than_milling_about_its_middle() {
        // The leader is walked off across the meadow; the herd goes with it.
        let world = meadow(60);
        let mut animals = Animals::seeded(17);
        let leader = animals.spawn(Species::Deer, (0.5, 21.0, 0.5)).expect("deer");
        let followers: Vec<EntityId> = [(3.5, 1.0), (1.5, 4.0), (-2.5, 2.5)]
            .iter()
            .map(|&(x, z)| animals.spawn(Species::Deer, (x, 21.0, z)).expect("deer"))
            .collect();
        let far = player((0.5, 21.0, 58.0));
        for _ in 0..600 {
            if let Some(deer) = animals.find_mut_for_test(leader) {
                // It has decided to go east, and nothing changes its mind.
                deer.mind = Mind::Wander;
                deer.wants_yaw = 0.0;
                deer.next_thought = 1.0;
            }
            animals.step(&world, &far, 0.05, NOON);
        }
        let front = animals.find(leader).expect("alive").at();
        assert!(front.0 > 20.0, "the leader did not get anywhere: {front:?}");
        for id in followers {
            let at = animals.find(id).expect("alive").at();
            assert!(
                apart(at, front) < HERD_FOLLOW * 3.0,
                "a follower was left {:.1} blocks behind at {at:?}",
                apart(at, front)
            );
            assert!(apart(at, front) > PERSONAL_SPACE * 0.5, "a follower is standing in its leader");
        }
    }

    #[test]
    fn wolves_of_one_pack_stay_within_sixteen_blocks_of_each_other() {
        let world = meadow(60);
        let mut animals = Animals::seeded(18);
        let pack: Vec<EntityId> = (0..5)
            .map(|i| animals.spawn(Species::Wolf, (i as f32 * 2.0, 21.0, (i % 2) as f32 * 3.0)).expect("wolf"))
            .collect();
        for &id in &pack {
            // Fed, so this is about the pack and not about a hunt.
            animals.find_mut_for_test(id).expect("wolf").fed_for = 10_000.0;
        }
        let far = player((0.5, 21.0, 58.0));
        let mut widest = 0.0f32;
        for tick in 0..2400 {
            animals.step(&world, &far, 0.05, NOON);
            if tick < 200 {
                continue;
            }
            for &a in &pack {
                for &b in &pack {
                    widest = widest.max(apart(animals.find(a).expect("alive").at(), animals.find(b).expect("alive").at()));
                }
            }
        }
        assert!(widest <= 16.0, "two wolves of one pack were {widest:.1} blocks apart");
    }

    #[test]
    fn a_pack_comes_at_a_person_who_turns_their_back_and_waits_on_one_who_does_not() {
        let charged_within = |facing: f32| -> Option<usize> {
            let world = meadow(50);
            let mut animals = Animals::seeded(19);
            // Nine or ten blocks off: inside what a wolf sees and smells of a
            // person standing still, and past the six it comes from anyway.
            let pack: Vec<EntityId> = [(-8.5, -1.5), (-9.0, 1.5), (-9.5, 0.0)]
                .iter()
                .map(|&(x, z)| animals.spawn(Species::Wolf, (x, 21.0, z)).expect("wolf"))
                .collect();
            for &id in &pack {
                animals.face_for_test(id, 0.0);
            }
            for tick in 0..40 {
                animals.player_signs(vec![PlayerSign {
                    who: 1,
                    facing,
                    working: false,
                    airborne: false,
                    low: false,
                    wounded: false,
                    asleep: false,
                    held: None,
                    reek: 1.0,
                }]);
                animals.step(&world, &player((0.5, 21.0, 0.5)), 0.05, NOON);
                if pack.iter().any(|&id| animals.find(id).expect("alive").mind == Mind::Charge) {
                    return Some(tick);
                }
            }
            None
        };
        // Facing the pack (it is at -x) it walks its ring; back to it, it comes.
        assert!(charged_within(std::f32::consts::PI).is_none(), "a pack charged somebody looking straight at it");
        assert!(charged_within(0.0).is_some(), "a pack at ten blocks let somebody stand with their back to it");
    }

    #[test]
    fn a_lone_wolf_does_not_wait_for_company_when_the_person_is_bleeding() {
        let world = meadow(60);
        let mut animals = Animals::seeded(20);
        let id = animals.spawn(Species::Wolf, (4.0, 21.0, 0.5)).expect("wolf");
        let mut charged = false;
        for _ in 0..200 {
            animals.player_signs(vec![PlayerSign { who: 1, facing: 0.0, working: false, airborne: false, low: false, wounded: true, asleep: false, held: None, reek: 1.0 }]);
            animals.step(&world, &player((1.0, 21.0, 0.5)), 0.05, NOON);
            charged |= animals.find(id).expect("alive").mind == Mind::Charge;
        }
        assert!(charged, "one wolf watched a wounded person and did nothing");
    }

    #[test]
    fn a_bear_comes_at_somebody_on_its_ground_and_goes_home_past_the_edge_of_it() {
        let world = meadow(60);
        let mut animals = Animals::seeded(21);
        let id = animals.spawn(Species::Bear, (0.5, 21.0, 0.5)).expect("bear");
        // Inside its ground, outside the distance anything else would charge
        // from.
        let trespasser = player((7.5, 21.0, 0.5));
        assert!(7.0 > Species::Bear.provoke_range());
        let mut charged = false;
        for _ in 0..120 {
            animals.step(&world, &trespasser, 0.05, NOON);
            charged |= animals.find(id).expect("alive").mind == Mind::Charge;
        }
        assert!(charged, "a bear let somebody stand on its ground");

        // Now it is far from its den, angry, after somebody further off still.
        {
            let bear = animals.find_mut_for_test(id).expect("bear");
            bear.position = (f64::from(TERRITORY_LEASH + 6.0), 21.0, 0.5);
            bear.angry_for = 30.0;
            bear.next_thought = 0.0;
        }
        let before = apart(animals.find(id).expect("alive").at(), (0.5, 21.0, 0.5));
        for _ in 0..100 {
            animals.step(&world, &player((TERRITORY_LEASH + 12.0, 21.0, 0.5)), 0.05, NOON);
        }
        let after = apart(animals.find(id).expect("alive").at(), (0.5, 21.0, 0.5));
        assert!(after < before - 2.0, "a bear past the edge of its ground kept going: {before:.1} -> {after:.1}");
    }

    #[test]
    fn a_lion_has_a_range_and_walks_back_into_it() {
        // **A lion keeps ground the way a bear does**, and that is what gives
        // the savanna places in it: a waterhole is somewhere you can cross at
        // one hour and not at another, and a player who walked into a pride's
        // range can walk back out of it the way they came. See
        // `Species::keeps_territory`, where the boar's case is argued and
        // turned down.
        //
        // The savanna, not a meadow: a lion is spawned wherever it is put,
        // but its den is where it was first found (`Animals::spawn`), and the
        // ground under it has to be somewhere a lion can stand.
        let world = meadow(80);
        let mut animals = Animals::seeded(23);
        let id = animals.spawn(Species::Lion, (0.5, 21.0, 0.5)).expect("lion");
        assert_eq!(
            animals.find(id).expect("alive").home,
            Some((0.5, 0.5)),
            "a lion was given no den to keep"
        );
        // Dragged out past the leash, angry, with somebody further off still:
        // it gives up and goes home rather than following.
        {
            let lion = animals.find_mut_for_test(id).expect("lion");
            lion.position = (f64::from(TERRITORY_LEASH + 6.0), 21.0, 0.5);
            lion.angry_for = 30.0;
            lion.next_thought = 0.0;
        }
        let before = apart(animals.find(id).expect("alive").at(), (0.5, 21.0, 0.5));
        for _ in 0..200 {
            animals.step(&world, &player((TERRITORY_LEASH + 14.0, 21.0, 0.5)), 0.05, NOON);
        }
        let after = apart(animals.find(id).expect("alive").at(), (0.5, 21.0, 0.5));
        assert!(
            after < before - 2.0,
            "a lion past the edge of its range kept after them: {before:.1} -> {after:.1}"
        );
    }

    #[test]
    fn a_bird_in_flight_never_jerks_pivots_or_falls() {
        // **"They fly like FPV drones, and sometimes they just fall."** A
        // covey flushed again and again by somebody walking round it, and a
        // gull working a shore, sampled every tick in the air: the sideways
        // and along-track pull is bounded, the turn rate is bounded, the climb
        // and the sink are bounded, and the vertical speed never changes at
        // anything like gravity -- which is what falling is.
        let check = |world: &TestWorld, animals: &mut Animals, ids: &[EntityId], walker: &dyn Fn(usize) -> (f32, f32, f32), ticks: usize| -> usize {
            const DT: f32 = 0.05;
            let mut last: Vec<Option<(Animal2, bool)>> = vec![None; ids.len()];
            let mut airborne_ticks = 0;
            for tick in 0..ticks {
                animals.step(world, &player(walker(tick)), DT, NOON);
                for (i, &id) in ids.iter().enumerate() {
                    let Some(bird) = animals.find(id) else { continue };
                    let now = Animal2 { v: bird.velocity, yaw: bird.yaw };
                    let up = !bird.on_ground && !in_liquid(world, primitive_shared::geometry::wide(bird.at()), bird.species) && bird.dive_for <= 0.0;
                    if let Some((before, was_up)) = last[i] {
                        if up && was_up {
                            airborne_ticks += 1;
                            let dv = (now.v.0 - before.v.0).hypot(now.v.2 - before.v.2) / DT;
                            assert!(dv <= 20.0, "tick {tick}: a {} changed its speed across at {dv:.1} blocks/s2", bird.species.name());
                            let turn = swing(before.yaw, now.yaw).abs() / DT;
                            assert!(turn <= AIR_TURN_MOST + 0.05, "tick {tick}: a {} turned at {turn:.2} rad/s in the air", bird.species.name());
                            assert!(now.v.1.abs() <= FLIGHT_SPEED + 1e-3, "tick {tick}: vertical speed {:.2}", now.v.1);
                            let dvy = (now.v.1 - before.v.1) / DT;
                            assert!(
                                dvy.abs() <= LIFT_ACCEL + 0.5,
                                "tick {tick}: a {} changed its climb at {dvy:.1} blocks/s2 -- gravity is {GRAVITY}",
                                bird.species.name()
                            );
                        }
                    }
                    last[i] = Some((now, up));
                }
            }
            airborne_ticks
        };

        let world = wood(40);
        let mut animals = Animals::seeded(22);
        let covey: Vec<EntityId> = (0..3)
            .map(|i| animals.spawn(Species::Fowl, (i as f32 * 1.5, 21.0, 0.5)).expect("bird"))
            .collect();
        // Round and round the covey at a walk, twelve blocks out, stepping in
        // through it every so often.
        let walker = |tick: usize| {
            let t = tick as f32 * 0.05;
            let radius = if (t as usize / 8).is_multiple_of(2) { 12.0 } else { 3.0 };
            (radius * (t * 4.3 / 12.0).cos(), 21.0, radius * (t * 4.3 / 12.0).sin())
        };
        let flown = check(&world, &mut animals, &covey, &walker, 2400);
        assert!(flown > 100, "the covey was only in the air for {flown} ticks");

        let shore_world = shore(40);
        let mut gulls = Animals::seeded(23);
        let gull = gulls.spawn(Species::Gull, (10.5, 30.0, 0.5)).expect("gull");
        {
            let bird = gulls.find_mut_for_test(gull).expect("gull");
            bird.home = Some((10.0, 0.0));
            bird.mind = Mind::Soar;
            bird.bound_for = Some((10.0, 0.0));
        }
        let nobody = |_| (-30.0, 21.0, 60.0);
        let soared = check(&shore_world, &mut gulls, &[gull], &nobody, 2400);
        assert!(soared > 500, "the gull was only in the air for {soared} ticks");
    }

    #[test]
    fn no_bird_ends_up_inside_a_block_over_five_simulated_minutes() {
        // **The promise everything above is allowed to move within.** The
        // climb-and-glide, the slope's lift, the traded speed and the weave
        // all push a bird off the line it used to fly, and every one of them
        // is a new way to end up inside a crown or under the sand. Five
        // minutes of each, at every tick, for the two species that fly --
        // and with the ground deliberately made of the things they fly into:
        // a wood for the covey, a cliff and a sea stack for the gull.
        let check = |world: &TestWorld, animals: &mut Animals, ids: &[EntityId], walker: &dyn Fn(usize) -> (f32, f32, f32)| {
            let mut aloft = 0;
            for tick in 0..6000 {
                animals.step(world, &player(walker(tick)), 0.05, NOON);
                for &id in ids {
                    let Some(bird) = animals.find(id) else { continue };
                    assert!(
                        fits(world, primitive_shared::geometry::wide(bird.at()), bird.species),
                        "tick {tick}: a {} is inside a block at {:?}",
                        bird.species.name(),
                        bird.at()
                    );
                    if !bird.on_ground {
                        aloft += 1;
                    }
                }
            }
            aloft
        };

        let wood = wood(40);
        let mut animals = Animals::seeded(91);
        let covey: Vec<EntityId> = (0..3)
            .map(|i| animals.spawn(Species::Fowl, (i as f32 * 1.5, 21.0, 0.5)).expect("bird"))
            .collect();
        // Walked round and through, so the covey is flushed again and again
        // and spends the five minutes crossing the trees rather than sitting.
        let walker = |tick: usize| {
            let t = tick as f32 * 0.05;
            let radius = if (t as usize / 8).is_multiple_of(2) { 12.0 } else { 3.0 };
            (radius * (t * 0.36).cos(), 21.0, radius * (t * 0.36).sin())
        };
        let flown = check(&wood, &mut animals, &covey, &walker);
        assert!(flown > 300, "the covey was only off the ground for {flown} bird-ticks");

        let shore = shore(40);
        for z in -40..=40 {
            for x in -12..=-10 {
                for y in 21..33 {
                    shore.put(x, y, z, BLOCK_STONE);
                }
            }
        }
        for x in 6..9 {
            for z in -2..2 {
                for y in 15..34 {
                    shore.put(x, y, z, BLOCK_STONE);
                }
            }
        }
        let mut gulls = Animals::seeded(92);
        let flock: Vec<EntityId> = (0..3)
            .map(|i| gulls.spawn(Species::Gull, (-3.5 + i as f32, 21.0, 0.5)).expect("a gull"))
            .collect();
        for &id in &flock {
            gulls.find_mut_for_test(id).expect("gull").home = Some((0.0, 0.0));
        }
        let nobody = |_: usize| (-30.0, 21.0, 60.0);
        let soared = check(&shore, &mut gulls, &flock, &nobody);
        assert!(soared > 3000, "the flock was only off the ground for {soared} bird-ticks");
    }

    #[test]
    fn a_flock_of_gulls_keeps_a_loose_formation() {
        // **Together, and not in a line.** Two failures are being held off at
        // once and they pull opposite ways: a flock whose birds each keep
        // their own circle over their own patch drifts apart until it is six
        // gulls who happen to share a beach, and a flock that all aims at one
        // point is a heap of boxes in the same cubic metre. What is wanted is
        // the middle -- they share a centre (`seabird`) and no two of them
        // share a line, because each flies its own radius and its own hand
        // (`circling`).
        let world = shore(40);
        let mut animals = Animals::seeded(93);
        let flock: Vec<EntityId> = (0..5)
            .map(|i| animals.spawn(Species::Gull, (-4.5 + i as f32 * 1.5, 21.0, 0.5)).expect("a gull"))
            .collect();
        for &id in &flock {
            let gull = animals.find_mut_for_test(id).expect("gull");
            gull.home = Some((0.0, 0.0));
            gull.mind = Mind::Soar;
            gull.bound_for = Some((0.0, 0.0));
        }
        let nobody = player((-30.0, 21.0, 60.0));
        let (mut widest, mut closest) = (0.0f32, f32::MAX);
        // The mean gap over the first minute and over the last, which is the
        // question "is it still a flock": a flock that is quietly dispersing
        // passes every instant and fails this.
        let (mut early, mut early_n, mut late, mut late_n) = (0.0f32, 0u32, 0.0f32, 0u32);
        let mut headings_spread = 0.0f32;
        for tick in 0..3600 {
            animals.step(&world, &nobody, 0.05, NOON);
            if tick % 20 != 0 {
                continue;
            }
            let birds: Vec<(f32, f32, f32)> = flock
                .iter()
                .filter_map(|&id| animals.find(id))
                .map(|gull| (gull.at().0, gull.at().2, gull.yaw))
                .collect();
            assert_eq!(birds.len(), flock.len(), "tick {tick}: the flock lost a bird");
            for (i, a) in birds.iter().enumerate() {
                for b in &birds[i + 1..] {
                    let gap = (a.0 - b.0).hypot(a.1 - b.1);
                    widest = widest.max(gap);
                    closest = closest.min(gap);
                    if tick < 1200 {
                        early += gap;
                        early_n += 1;
                    } else if tick >= 2400 {
                        late += gap;
                        late_n += 1;
                    }
                }
            }
            // How spread the headings are: the length of the mean facing is
            // one when every bird points the same way and nought when they
            // are all round the compass.
            let (sx, sz) = birds.iter().fold((0.0, 0.0), |(sx, sz), b| (sx + b.2.cos(), sz + b.2.sin()));
            headings_spread = headings_spread.max(1.0 - (sx / 5.0f32).hypot(sz / 5.0));
        }
        let (early, late) = (early / early_n as f32, late / late_n as f32);
        println!(
            "flock of five: widest {widest:.1}, closest {closest:.2}, mean gap {early:.1} -> {late:.1}, heading spread {headings_spread:.2}"
        );
        // **Together**: a gull's whole range is `GULL_RANGE` of its shore and
        // the circles are laid over one another, so the average pair is a
        // circle's width apart and not a coast's.
        assert!(late < GULL_RANGE, "the flock averaged {late:.1} blocks apart -- that is a coastline, not a flock");
        // ...and it is not quietly coming undone: the last minute is no
        // wider than the first, give or take a circle.
        assert!(late < early + SOAR_RADIUS, "the flock spread from {early:.1} to {late:.1} blocks apart");
        // **Loose**: several blocks between them on average, never all on one
        // heading, and never stacked in one cell.
        assert!(late > 3.0, "the flock flew as one bird: a mean gap of {late:.1} blocks");
        assert!(headings_spread > 0.3, "every gull in the flock flew the same heading");
    }

    /// What `a_bird_in_flight_never_jerks_pivots_or_falls` keeps of a tick.
    #[derive(Clone, Copy)]
    struct Animal2 {
        v: (f32, f32, f32),
        yaw: f32,
    }

    #[test]
    fn the_sight_rays_keep_to_their_budget_however_many_animals_are_looking() {
        let world = meadow(40);
        let mut animals = Animals::seeded(24);
        for i in 0..MAX_ANIMALS {
            animals.spawn(Species::Deer, ((i % 8) as f32 * 2.5 - 10.0, 21.0, (i / 8) as f32 * 2.5 - 10.0));
        }
        const TICKS: u64 = 100;
        for tick in 0..TICKS {
            let t = tick as f32 * 0.05;
            animals.step(&world, &player((14.0 * t.cos(), 21.0, 14.0 * t.sin())), 0.05, NOON);
        }
        assert!(
            animals.rays_cast <= TICKS * RAYS_PER_TICK as u64,
            "{} rays in {TICKS} ticks against a budget of {RAYS_PER_TICK} a tick",
            animals.rays_cast
        );
        assert!(animals.rays_cast > 0, "nothing ever looked");
    }

    // ---- measurements ----

    /// How far off every animal notices a person, by gait, facing them and
    /// with its back to them, on open ground by day in still air. "maybe" is
    /// where it lifts its head; "sure" is where it goes.
    ///
    /// ```text
    /// cargo test -p primitive_server --lib detection_distance_per_species -- --ignored --nocapture
    /// ```
    #[test]
    #[ignore = "a measurement, not an assertion -- see logic::animals's module doc"]
    fn detection_distance_per_species_and_gait() {
        let gaits = [("still", 0.0), ("creeping", 1.5), ("walking", 4.3), ("running", NOMINAL_SPRINT_SPEED_FOR_TESTS)];
        println!("{:<10} {:>28} {:>28}", "", "facing (maybe/sure)", "back turned (maybe/sure)");
        for &species in Species::ALL.iter().filter(|s| !s.swims()) {
            let mut line = format!("{:<10}", species.name());
            for behind in [false, true] {
                let cells: Vec<String> = gaits
                    .iter()
                    .map(|&(_, speed)| {
                        format!("{:.0}/{:.0}", noticed_at(species, speed, behind, false), noticed_at(species, speed, behind, true))
                    })
                    .collect();
                line.push_str(&format!(" {:>28}", cells.join(" ")));
            }
            println!("{line}");
        }
        println!("(columns: {})", gaits.iter().map(|g| g.0).collect::<Vec<_>>().join(", "));
    }

    /// How often a deer chased by a walking player gets clear of an obstacle
    /// in its way, across seeds: a wall, a pocket, a pond.
    ///
    /// ```text
    /// cargo test -p primitive_server --lib fleeing_round_an_obstacle_course -- --ignored --nocapture
    /// ```
    #[test]
    #[ignore = "a measurement, not an assertion -- see logic::animals's module doc"]
    fn fleeing_round_an_obstacle_course() {
        type Course = (&'static str, fn(&TestWorld));
        let courses: [Course; 3] = [
            ("wall", |w| {
                for z in -10..=10 {
                    for y in 21..=23 {
                        w.put(12, y, z, BLOCK_STONE);
                    }
                }
            }),
            // Closed at one end, open at the other: the way out is along the
            // wall, and only one way along it.
            ("pocket", |w| {
                for z in -12..=6 {
                    for y in 21..=23 {
                        w.put(12, y, z, BLOCK_STONE);
                    }
                }
                for x in 6..=12 {
                    for y in 21..=23 {
                        w.put(x, y, -12, BLOCK_STONE);
                    }
                }
            }),
            ("pond", |w| {
                for z in -10..=10 {
                    for x in 12..=18 {
                        w.put(x, 20, z, primitive_shared::types::BLOCK_WATER);
                    }
                }
            }),
        ];
        for ((name, build), plain) in courses.iter().flat_map(|course| [(course, true), (course, false)]) {
            PLAIN_SWERVE.with(|flag| flag.set(plain));
            let (mut clear, mut stuck_ticks) = (0, 0);
            const SEEDS: u64 = 20;
            for seed in 0..SEEDS {
                let world = meadow(50);
                build(&world);
                let mut animals = Animals::seeded(seed);
                let id = animals.spawn(Species::Deer, (9.5, 21.0, (seed % 7) as f32 - 3.0)).expect("deer");
                for _ in 0..300 {
                    let at = animals.find(id).expect("alive").at();
                    animals.step(&world, &player((at.0 - 5.0, 21.0, at.2)), 0.05, NOON);
                    let deer = animals.find(id).expect("alive");
                    if deer.mind == Mind::Flee && deer.velocity.0.hypot(deer.velocity.2) < 0.5 {
                        stuck_ticks += 1;
                    }
                }
                let at = animals.find(id).expect("alive").at();
                if apart(at, (9.5, 21.0, 0.0)) > 12.0 {
                    clear += 1;
                }
            }
            println!(
                "{name} ({}): {clear}/{SEEDS} got more than twelve blocks clear; {:.1} ticks a run stood still while fleeing",
                if plain { "nearest open swerve, before" } else { "escape_heading" },
                stuck_ticks as f32 / SEEDS as f32
            );
        }
        PLAIN_SWERVE.with(|flag| flag.set(false));
    }

    fn herd_benchmark(label: &str, player_at: (f32, f32, f32)) {
        use std::time::Instant;
        let world = meadow(60);
        let mut animals = Animals::seeded(99);
        let mut n = 0;
        'fill: for x in -6..6 {
            for z in -6..6 {
                for s in Species::ALL {
                    animals.spawn(*s, (x as f32 * 4.0 + 0.5, 21.0, z as f32 * 4.0 + 0.5));
                    n += 1;
                    if n >= MAX_ANIMALS {
                        break 'fill;
                    }
                }
            }
        }
        let players = player(player_at);
        const ROUNDS: usize = 5;
        const TICKS: usize = 400;
        let mut best = f64::MAX;
        for _ in 0..ROUNDS {
            let started = Instant::now();
            for _ in 0..TICKS {
                std::hint::black_box(animals.step(&world, &players, 0.05, NOON));
            }
            best = best.min(started.elapsed().as_secs_f64() * 1000.0 / TICKS as f64);
        }
        println!(
            "step ({label}): {best:.4} ms/tick over {} animals ({:.2} us/animal/tick)",
            animals.len(),
            best * 1000.0 / animals.len() as f64
        );
    }

    /// **A deer a long way out walks as smoothly as one at home.** Its feet
    /// were an `f32`: ten million blocks out an animal moved a whole block or
    /// nothing, so a walking deer stood still and teleported, and a million
    /// out it shivered in sixteenths. Run from a player in a meadow, every
    /// tick's stride has to be a stride -- no longer than it can run in a
    /// tick, and not stuck to a grid -- and the feet have to stay on the grass.
    #[test]
    fn an_animal_far_from_zero_runs_as_smoothly_as_at_home() {
        for (ox, oz) in [(0, 0), (1_000_000, 1_000_000), (-10_000_000, 10_000_000)] {
            let world = TestWorld::default();
            for z in -40..=40 {
                for x in -40..=40 {
                    world.put(ox + x, 20, oz + z, BLOCK_GRASS);
                    for y in 21..30 {
                        world.put(ox + x, y, oz + z, BLOCK_AIR);
                    }
                }
            }
            let mut animals = Animals::seeded(3);
            let id = animals.spawn(Species::Deer, (0.5, 21.0, 0.5)).expect("deer");
            animals.find_mut_for_test(id).expect("deer").position = (f64::from(ox) + 0.5, 21.0, f64::from(oz) + 0.5);
            let chaser = player(((ox - 4) as f32, 21.0, oz as f32));
            let most = f64::from(Species::Deer.run_speed() * 0.05) * 1.25 + 1e-6;
            let mut last = animals.find(id).expect("deer").position;
            let (mut travelled, mut off_grid) = (0.0, 0);
            for tick in 0..200 {
                animals.step(&world, &chaser, 0.05, NOON);
                let Some(deer) = animals.find(id) else { break };
                let now = deer.position;
                let stride = (now.0 - last.0).hypot(now.2 - last.2);
                assert!(stride <= most, "({ox}, {oz}), tick {tick}: a stride of {stride} from {last:?} to {now:?}");
                assert!((now.1 - 21.0).abs() < 0.6, "({ox}, {oz}), tick {tick}: the feet left the grass: {now:?}");
                if stride > 1e-4 && ((stride * 16.0).fract() - 0.5).abs() < 0.45 {
                    off_grid += 1;
                }
                travelled += stride;
                last = now;
            }
            assert!(travelled > 5.0, "({ox}, {oz}): the deer never ran ({travelled} blocks)");
            assert!(off_grid > 20, "({ox}, {oz}): the strides were on a grid ({off_grid} off it)");
        }
    }

    // ---- the young, and the death fall ----

    /// Somebody far enough off not to frighten anything and near enough that
    /// nothing is forgotten (`DESPAWN_DISTANCE`): with nobody at all, every
    /// animal is out of everybody's range and goes on the first tick.
    fn far() -> Vec<(primitive_shared::protocol::PlayerId, (f32, f32, f32))> {
        player((0.5, 21.0, 60.0))
    }

    /// A doe standing in the meadow, settled onto the grass, and the fawn
    /// she has just had. The calendar is started so the young can grow.
    fn a_doe_and_her_fawn(world: &TestWorld, seed: u64) -> (Animals, EntityId, EntityId) {
        let mut animals = Animals::seeded(seed);
        let doe = animals.spawn(Species::Deer, (0.5, 21.0, 0.5)).expect("a doe");
        for _ in 0..10 {
            animals.step(world, &far(), 0.05, NOON);
        }
        animals.calendar(40.0);
        let fawn = animals.bear_young(doe).expect("a fawn");
        (animals, doe, fawn)
    }

    #[test]
    fn a_fawn_grows_into_a_deer_over_the_days_and_not_before() {
        let world = meadow(30);
        let (mut animals, doe, fawn) = a_doe_and_her_fawn(&world, 11);
        assert_eq!(animals.growth(fawn), Some(0.0), "the fawn was born grown");
        assert_eq!(animals.mother_of(fawn), Some(doe), "the fawn does not know its mother");
        let newborn = animals.health(fawn).expect("alive");
        assert!(newborn < Species::Deer.health() * 0.5, "a newborn was as hard to kill as a deer");
        // ...and no second one while she has this one.
        assert_eq!(animals.bear_young(doe), None, "a doe with a fawn had another");

        // A day on: growing, and still young.
        animals.calendar(41.0);
        animals.step(&world, &far(), 0.05, NOON);
        let growth = animals.growth(fawn).expect("alive");
        assert!(growth > 0.2 && growth < 0.3, "a day grew it by {growth}");
        assert!(animals.health(fawn).expect("alive") > newborn, "it grew and got no stronger");

        // Past `GROWN_DAYS`: a deer, on its own, and sent as one.
        animals.calendar(40.0 + youth::GROWN_DAYS + 0.5);
        animals.step(&world, &far(), 0.05, NOON);
        assert_eq!(animals.growth(fawn), Some(youth::GROWN), "the fawn never grew up");
        assert_eq!(animals.mother_of(fawn), None, "a grown deer still has a mother");
        let sent = animals.states().into_iter().find(|s| s.id == fawn).expect("sent");
        assert!(matches!(sent.kind, EntityKind::Animal { growth: u8::MAX, .. }), "a grown deer was sent young");
    }

    #[test]
    fn a_clock_turned_back_grows_nobody_younger() {
        let world = meadow(30);
        let (mut animals, _, fawn) = a_doe_and_her_fawn(&world, 12);
        animals.calendar(42.0);
        animals.step(&world, &far(), 0.05, NOON);
        let grown = animals.growth(fawn).expect("alive");
        animals.calendar(10.0);
        animals.step(&world, &far(), 0.05, NOON);
        assert_eq!(animals.growth(fawn), Some(grown));
    }

    #[test]
    fn a_doe_does_not_leave_her_fawn_behind_when_she_runs() {
        // **The property the young exist for**: a mother that bolts at full
        // speed leaves a fawn at a walk behind her, and the whole cost of the
        // fawn -- the doe is the slow deer -- is gone. See `keep_family`.
        let world = meadow(80);
        let (mut animals, doe, fawn) = a_doe_and_her_fawn(&world, 13);
        let hunter = player((-3.5, 21.0, 0.5));
        let start = animals.find(doe).expect("the doe").at();
        let mut widest = 0.0f32;
        for tick in 0..240 {
            animals.step(&world, &hunter, 0.05, NOON);
            let (Some(d), Some(f)) = (animals.find(doe), animals.find(fawn)) else {
                panic!("one of them is gone");
            };
            // After the first second, when both have got going: the fawn is
            // born a body's width off her flank and takes a moment to fall in.
            if tick > 20 {
                widest = widest.max((d.at().0 - f.at().0).hypot(d.at().2 - f.at().2));
            }
        }
        let at = animals.find(doe).expect("the doe").at();
        let ran = (at.0 - start.0).hypot(at.2 - start.2);
        assert!(ran > 6.0, "the doe never ran ({ran:.1} blocks)");
        assert!(
            widest < youth::MOTHER_LEASH + 1.0,
            "the doe left her fawn {widest:.1} blocks behind while she ran"
        );
    }

    #[test]
    fn a_doe_alone_outruns_a_doe_with_a_fawn() {
        // The other half of the property: the fawn costs her something, or
        // the test above would pass with a fawn as fast as its mother.
        let world = meadow(80);
        let hunter = player((-3.5, 21.0, 0.5));
        let run = |with_fawn: bool| {
            let (mut animals, doe, fawn) = a_doe_and_her_fawn(&world, 14);
            if !with_fawn {
                animals.forget(fawn);
            }
            for _ in 0..80 {
                animals.step(&world, &hunter, 0.05, NOON);
            }
            let at = animals.find(doe).expect("the doe").at();
            (at.0 - 0.5).hypot(at.2 - 0.5)
        };
        let (alone, mothering) = (run(false), run(true));
        assert!(alone > mothering + 2.0, "alone {alone:.1}, with a fawn {mothering:.1}: the fawn cost nothing");
    }

    #[test]
    fn a_young_animal_collides_and_is_struck_as_a_smaller_box() {
        let world = meadow(10);
        let (animals, doe, fawn) = a_doe_and_her_fawn(&world, 15);
        let (d, f) = (animals.find(doe).expect("doe"), animals.find(fawn).expect("fawn"));
        assert!(f.frame().height < d.frame().height * 0.6 && f.frame().width < d.frame().width * 0.6);
        assert!(f.hit_box().1 < d.hit_box().1 * 0.6, "the fawn is struck as a deer");
        // Under a ledge a deer cannot stand beneath: one block of air.
        let low = meadow(4);
        low.put(0, 22, 0, BLOCK_STONE);
        assert!(!fits(&low, (0.5, 21.0, 0.5), d.frame()), "a deer fitted under one block");
        assert!(fits(&low, (0.5, 21.0, 0.5), f.frame()), "a fawn did not fit where it would");
        // ...and a swing from the same place, relative to each, lands on the
        // deer and falls short of the fawn.
        let from = |a: &Animal| (a.at().0 + d.species.length() * 0.5 + 0.2, a.at().1 + 0.5, a.at().2);
        assert!(f.distance_to_box(from(f)) > d.distance_to_box(from(d)) + 0.05, "the fawn is hit a deer's length off");
    }

    #[test]
    fn a_killed_deer_falls_for_a_moment_and_leaves_one_body_where_it_came_to_rest() {
        let world = meadow(20);
        let mut animals = Animals::seeded(16);
        let deer = animals.spawn(Species::Deer, (0.5, 21.0, 0.5)).expect("a deer");
        for _ in 0..10 {
            animals.step(&world, &far(), 0.05, NOON);
        }
        // Moving when it dies, so where it fell is not where it was struck.
        let struck_at = animals.find(deer).expect("alive").at();
        animals.find_mut_for_test(deer).expect("alive").velocity = (5.0, 0.0, 0.0);
        let killed = animals.hurt(deer, f32::MAX).expect("a death");
        assert_eq!(killed.at, struck_at);
        // Not a carcass yet: a body, going down, drawn as one.
        assert!(animals.take_fallen().is_empty(), "the carcass came before the fall");
        let body = animals.states().into_iter().find(|s| s.id == deer).expect("the body is still sent");
        assert!(matches!(body.kind, EntityKind::Animal { attitude: primitive_shared::protocol::Attitude::Dying, .. }));
        assert_eq!(animals.health(deer), None, "the dead can still be hurt");
        assert!(matches!(animals.strike(deer, struck_at, 5.0, 3.0), Struck::Missed), "a falling body was struck");

        let mut fallen = Vec::new();
        let mut last = None;
        let mut elapsed = 0.0;
        while elapsed < FALL_SECONDS * 2.0 {
            if let Some(body) = animals.states().into_iter().find(|s| s.id == deer) {
                last = Some(body);
            }
            animals.step(&world, &far(), 0.05, NOON);
            elapsed += 0.05;
            let now = animals.take_fallen();
            if !now.is_empty() {
                assert!(elapsed >= FALL_SECONDS - 0.051, "the body landed after {elapsed:.2}s");
            }
            fallen.extend(now);
        }
        assert_eq!(fallen.len(), 1, "the one deer left {} bodies", fallen.len());
        let rest = fallen[0];
        let last = last.expect("it was drawn falling");
        assert!(rest.at.0 > struck_at.0 + 0.3, "it did not go on sliding as it fell: {:?}", rest.at);
        assert!((f64::from(rest.at.0) - last.x).abs() < 0.2 && (f64::from(rest.at.2) - last.z).abs() < 0.2,
            "the carcass is laid at {:?} and the body was last drawn at ({:.2}, {:.2})", rest.at, last.x, last.z);
        assert!(animals.states().iter().all(|s| s.id != deer), "the body stayed after its carcass");
        assert!(animals.take_deaths().contains(&deer), "nobody was told it went");
    }

    #[test]
    fn a_young_death_is_told_young_so_it_leaves_a_small_heap() {
        let world = meadow(20);
        let (mut animals, _, fawn) = a_doe_and_her_fawn(&world, 17);
        animals.hurt(fawn, f32::MAX).expect("a death");
        let mut fallen = Vec::new();
        for _ in 0..40 {
            animals.step(&world, &far(), 0.05, NOON);
            fallen.extend(animals.take_fallen());
        }
        assert_eq!(fallen.len(), 1);
        assert!(!youth::leaves_carcass(fallen[0].growth), "a newborn's death was told grown");
    }

    #[test]
    fn a_sow_with_a_piglet_does_not_break_off_and_comes_for_whoever_hits_it() {
        let world = meadow(30);
        let mut animals = Animals::seeded(18);
        let sow = animals.spawn(Species::Boar, (0.5, 21.0, 0.5)).expect("a sow");
        for _ in 0..10 {
            animals.step(&world, &far(), 0.05, NOON);
        }
        animals.calendar(40.0);
        let piglet = animals.bear_young(sow).expect("a piglet");
        // A piglet does not fight: it is prey until it is grown.
        assert!(!animals.find(piglet).expect("piglet").fights() && animals.find(sow).expect("sow").fights());
        let from = animals.find(piglet).expect("piglet").at();
        animals.strike(piglet, (from.0 - 1.0, from.1 + 0.4, from.2), 3.0, 0.5);
        assert!(animals.find(sow).expect("sow").angry_for > 0.0, "the sow did not mind her piglet being hit");
        // ...and beaten nearly to nothing, she stands.
        animals.find_mut_for_test(sow).expect("sow").health = Species::Boar.health() * BREAKS_OFF_BELOW * 0.5;
        let at = animals.find(sow).expect("sow").at();
        animals.strike(sow, (at.0 - 1.0, at.1 + 0.4, at.2), 3.0, 0.1);
        assert_ne!(animals.find(sow).expect("sow").mind, Mind::Flee, "a sow with a piglet broke off");
    }
}

/// Keeping animals: the lure, the home, the pen, parking, the save, and what a
/// kept flock gives. See `primitive_shared::husbandry` for the day rules, which
/// are tested there.
#[cfg(test)]
mod husbandry_tests {
    use super::*;
    use crate::logic::falling::tests::TestWorld;
    use primitive_shared::husbandry::Keeping;
    use primitive_shared::types::{BLOCK_AIR, BLOCK_BOWL, BLOCK_FLINT_KNIFE, BLOCK_GRAIN, BLOCK_PLANKS, BLOCK_STONE};

    const NOON: f32 = 0.5;
    const MIDNIGHT: f32 = 0.0;

    fn meadow(span: i32) -> TestWorld {
        let world = TestWorld::default();
        for z in -span..=span {
            for x in -span..=span {
                world.put(x, 20, z, BLOCK_GRASS);
                for y in 21..30 {
                    world.put(x, y, z, BLOCK_AIR);
                }
            }
        }
        world
    }

    /// A ring of stone `high` blocks tall, `radius` out from the origin: the
    /// pen. Nothing else here makes one -- see `raid_the_pens`.
    fn pen(world: &TestWorld, radius: i32, high: i32) {
        for a in -radius..=radius {
            for (x, z) in [(a, -radius), (a, radius), (-radius, a), (radius, a)] {
                for y in 21..21 + high {
                    world.put(x, y, z, BLOCK_STONE);
                }
            }
        }
    }

    fn at(x: f32, z: f32) -> Vec<(PlayerId, (f32, f32, f32))> {
        vec![(1, (x, 21.0, z))]
    }

    fn holding(animals: &mut Animals, held: Option<primitive_shared::types::BlockId>) {
        animals.player_signs(vec![PlayerSign {
            who: 1,
            facing: 0.0,
            working: false,
            airborne: false,
            low: false,
            wounded: false,
            asleep: false,
            held,
            reek: 1.0,
        }]);
    }

    /// Tame, at home in the middle of the pen, fed and well.
    fn tame_at_home() -> Keeping {
        Keeping { trust: 1.0, tame: true, home: Some((0.5, 21.0, 0.5)), hunger: 0.0, well_fed: 1.0, ..Keeping::wild() }
    }

    fn run(animals: &mut Animals, world: &TestWorld, players: &[(PlayerId, (f32, f32, f32))], seconds: f32, time: f32) {
        for _ in 0..(seconds / 0.05) as usize {
            animals.step(world, players, 0.05, time);
        }
    }

    fn tack_of(animals: &Animals, id: EntityId) -> u8 {
        match animals.states().into_iter().find(|s| s.id == id).map(|s| s.kind) {
            Some(EntityKind::Animal { tack, .. }) => tack,
            other => panic!("not an animal: {other:?}"),
        }
    }

    #[test]
    fn a_shorn_sheep_is_sent_shorn_until_its_fleece_is_ready_again() {
        let world = meadow(20);
        let mut animals = Animals::seeded(4);
        let sheep = animals.spawn(Species::Sheep, (0.5, 21.0, 0.5)).expect("sheep");
        animals.keep_for_test(sheep, tame_at_home());
        assert_eq!(tack_of(&animals, sheep) & horse::TACK_SHORN, 0, "a sheep in full fleece was drawn shorn");
        assert_eq!(animals.tend(sheep, (2.0, 22.6, 0.5), Some(BLOCK_FLINT_KNIFE)), Tended::Shorn(husbandry::FLEECE_WOOL));
        assert_ne!(tack_of(&animals, sheep) & horse::TACK_SHORN, 0, "a sheep sheared a moment ago looks unshorn");
        // Grain-fed, the fleece is back in three days (`FLEECE_DAYS_WELL_FED`).
        animals.calendar(0.0);
        for day in 1..=3 {
            animals.calendar(day as f32);
            animals.step(&world, &at(2.0, 0.5), 0.05, NOON);
            let eye = animals.position(sheep).map(|p| (p.0 + 1.5, 22.6, p.2)).expect("alive");
            let _ = animals.tend(sheep, eye, Some(BLOCK_GRAIN));
        }
        assert!(animals.keeping(sheep).is_some_and(|k| k.fleece_ready()), "the fleece never grew back");
        assert_eq!(tack_of(&animals, sheep) & horse::TACK_SHORN, 0, "a sheep with its fleece back is still drawn shorn");
    }

    #[test]
    fn a_sheep_butchered_the_day_it_was_shorn_leaves_a_carcass_with_no_fleece_on_it() {
        for shear in [false, true] {
            let world = meadow(20);
            let mut animals = Animals::seeded(4);
            let sheep = animals.spawn(Species::Sheep, (0.5, 21.0, 0.5)).expect("sheep");
            animals.keep_for_test(sheep, tame_at_home());
            if shear {
                assert!(matches!(animals.tend(sheep, (2.0, 22.6, 0.5), Some(BLOCK_FLINT_KNIFE)), Tended::Shorn(_)));
            }
            let mut deaths = Vec::new();
            for _ in 0..400 {
                let _ = animals.strike(sheep, (1.5, 22.0, 0.5), 3.0, 100.0);
                animals.step(&world, &at(1.5, 0.5), 0.05, NOON);
                deaths.extend(animals.take_fallen());
                if !deaths.is_empty() {
                    break;
                }
            }
            let death = deaths.first().copied().expect("the sheep never came down");
            assert_eq!(death.shorn, shear, "shorn {shear}: the death says {death:?}");
        }
    }

    #[test]
    fn a_kept_flock_does_not_use_up_the_wilds_allowance() {
        let mut animals = Animals::seeded(3);
        for i in 0..MAX_ANIMALS {
            let id = animals.spawn(Species::Sheep, (i as f32, 21.0, 0.5)).expect("the land's cap came early");
            animals.keep_for_test(id, tame_at_home());
        }
        assert!(
            animals.spawn(Species::Deer, (0.5, 21.0, 9.5)).is_some(),
            "sixty tame sheep in a pen kept every wild deer out of the world"
        );
        // ...and the flock has its own ceiling: a tame ewe past it bears
        // nothing, however the wild stand.
        let mut kept = Animals::seeded(5);
        let ewes: Vec<EntityId> = (0..MAX_KEPT)
            .map(|i| {
                let id = kept.spawn(Species::Sheep, (i as f32 * 2.0, 21.0, 0.5)).expect("sheep");
                kept.keep_for_test(id, tame_at_home());
                id
            })
            .collect();
        assert_eq!(kept.bear_young(ewes[0]), None, "a flock bred past its ceiling");
    }

    #[test]
    fn a_player_keeping_a_small_flock_still_meets_wild_animals() {
        let world = meadow(100);
        let mut animals = Animals::seeded(9);
        for i in 0..MAX_ANIMALS_PER_PLAYER + 1 {
            let id = animals.spawn(Species::Sheep, (0.5 + i as f32, 21.0, 0.5)).expect("sheep");
            animals.keep_for_test(id, tame_at_home());
        }
        let flock = animals.len();
        run(&mut animals, &world, &at(0.5, 0.5), 600.0, NOON);
        assert!(animals.len() > flock, "a player with {flock} tame sheep never met anything wild in ten minutes");
    }

    #[test]
    fn a_wild_sheep_comes_to_a_still_hand_holding_grain_and_not_to_one_holding_planks() {
        for (held, comes) in [(BLOCK_GRAIN, true), (BLOCK_PLANKS, false)] {
            let world = meadow(30);
            let mut animals = Animals::seeded(7);
            let sheep = animals.spawn(Species::Sheep, (0.5, 21.0, 0.5)).expect("sheep");
            holding(&mut animals, Some(held));
            run(&mut animals, &world, &at(9.5, 0.5), 20.0, NOON);
            let p = animals.position(sheep).expect("alive");
            // At the hand, where the lure stops it (`LURE_CLOSE`), and not
            // merely somewhere near after a wander.
            let near = (p.0 - 9.5).hypot(p.2 - 0.5) < LURE_CLOSE + 0.5;
            assert_eq!(near, comes, "holding {held}: did the sheep come to the hand?");
        }
    }

    #[test]
    fn feeding_a_sheep_by_hand_three_times_a_quarter_day_apart_tames_it_and_a_second_feed_at_once_is_refused() {
        let world = meadow(20);
        let mut animals = Animals::seeded(3);
        let sheep = animals.spawn(Species::Sheep, (0.5, 21.0, 0.5)).expect("sheep");
        let eye = (2.0, 22.6, 0.5);
        animals.calendar(0.0);
        assert_eq!(animals.tend(sheep, eye, Some(BLOCK_GRAIN)), Tended::Fed { tamed: false, gentled: false });
        assert_eq!(animals.tend(sheep, eye, Some(BLOCK_GRAIN)), Tended::Refused(Notice::NotHungry));
        let mut tamed = false;
        for n in 1..=2 {
            animals.calendar(n as f32 * 0.6);
            animals.step(&world, &at(2.0, 0.5), 0.05, NOON);
            let eye = animals.position(sheep).map(|p| (p.0 + 1.5, 22.6, p.2)).expect("still here");
            tamed = animals.tend(sheep, eye, Some(BLOCK_GRAIN)) == Tended::Fed { tamed: true, gentled: false };
        }
        assert!(tamed, "the third feed did not tame it");
        assert!(animals.keeping(sheep).is_some_and(|k| k.tame && k.home.is_some()));
    }

    #[test]
    fn planks_held_out_to_a_wild_sheep_leave_it_nobody_s() {
        let mut animals = Animals::seeded(3);
        let sheep = animals.spawn(Species::Sheep, (0.5, 21.0, 0.5)).expect("sheep");
        assert_eq!(animals.tend(sheep, (2.0, 22.6, 0.5), Some(BLOCK_PLANKS)), Tended::Refused(Notice::DoesNotEatThat));
        assert_eq!(animals.keeping(sheep), None, "shown a plank, it is kept and saved for ever");
    }

    #[test]
    fn a_deer_cannot_be_kept() {
        let mut animals = Animals::seeded(3);
        let deer = animals.spawn(Species::Deer, (0.5, 21.0, 0.5)).expect("deer");
        assert!(matches!(animals.tend(deer, (2.0, 22.6, 0.5), Some(BLOCK_GRAIN)), Tended::Refused(_)));
    }

    #[test]
    fn a_tame_sheep_follows_grain_and_walks_home_when_the_grain_is_put_away() {
        let world = meadow(40);
        let mut animals = Animals::seeded(11);
        let sheep = animals.spawn(Species::Sheep, (0.5, 21.0, 0.5)).expect("sheep");
        animals.keep_for_test(sheep, tame_at_home());
        holding(&mut animals, Some(BLOCK_GRAIN));
        // Walking away with the sack, which a wild sheep would not follow.
        for step in 0..600 {
            let x = 0.5 + step as f32 * 0.025;
            animals.step(&world, &at(x, 0.5), 0.05, NOON);
        }
        let followed = animals.position(sheep).expect("alive");
        assert!(followed.0 > 9.0, "the tame sheep did not follow the grain: {followed:?}");
        holding(&mut animals, None);
        run(&mut animals, &world, &at(15.5, 0.5), 40.0, NOON);
        let home = animals.position(sheep).expect("alive");
        assert!(home.0.hypot(home.2) <= KEPT_LEASH + 2.0, "it did not go home: {home:?}");
    }

    #[test]
    fn a_tame_sheep_does_not_run_from_a_running_player() {
        let world = meadow(40);
        let mut animals = Animals::seeded(5);
        let sheep = animals.spawn(Species::Sheep, (0.5, 21.0, 0.5)).expect("sheep");
        animals.keep_for_test(sheep, tame_at_home());
        holding(&mut animals, None);
        for step in 0..200 {
            let x = 20.0 - step as f32 * 0.35;
            animals.step(&world, &at(x, 0.5), 0.05, NOON);
            assert_ne!(animals.find(sheep).expect("alive").mind, Mind::Flee, "a tame sheep bolted from its keeper");
        }
    }

    #[test]
    fn a_two_block_wall_holds_a_flock_from_the_grain_outside_and_a_one_block_wall_does_not() {
        for (high, holds) in [(2, true), (1, false)] {
            let world = meadow(30);
            pen(&world, 3, high);
            let mut animals = Animals::seeded(9);
            let sheep = animals.spawn(Species::Sheep, (0.5, 21.0, 0.5)).expect("sheep");
            animals.keep_for_test(sheep, tame_at_home());
            holding(&mut animals, Some(BLOCK_GRAIN));
            run(&mut animals, &world, &at(9.5, 0.5), 40.0, NOON);
            let p = animals.position(sheep).expect("alive");
            let inside = p.0.abs() < 3.0 && p.2.abs() < 3.0;
            assert_eq!(inside, holds, "a wall {high} high: the sheep ended at {p:?}");
        }
    }

    #[test]
    fn a_kept_sheep_nobody_is_near_is_parked_not_forgotten_and_comes_back_when_somebody_does() {
        let world = meadow(20);
        let mut animals = Animals::seeded(2);
        let kept = animals.spawn(Species::Sheep, (0.5, 21.0, 0.5)).expect("sheep");
        let wild = animals.spawn(Species::Sheep, (2.5, 21.0, 0.5)).expect("sheep");
        animals.keep_for_test(kept, tame_at_home());
        animals.step(&world, &at(500.0, 500.0), 0.05, NOON);
        assert!(animals.find(wild).is_none() && animals.find(kept).is_none());
        assert_eq!(animals.parked_count(), 1, "the wild one was kept or the kept one forgotten");
        // ...and with nobody online at all, the same.
        animals.step(&world, &[], 0.05, NOON);
        assert_eq!(animals.parked_count(), 1);
        animals.step(&world, &at(10.0, 0.5), 0.05, NOON);
        assert_eq!(animals.parked_count(), 0, "somebody came back and the flock did not");
        assert!(animals.keeping(kept).is_some_and(|k| k.tame), "it came back somebody else's");
    }

    #[test]
    fn a_parked_flock_comes_back_hungry_for_the_days_it_was_left() {
        let world = meadow(20);
        // Bare earth under it: nothing to graze while nobody watched.
        for z in -20..=20 {
            for x in -20..=20 {
                world.put(x, 20, z, primitive_shared::types::BLOCK_DIRT);
            }
        }
        let mut animals = Animals::seeded(2);
        let sheep = animals.spawn(Species::Sheep, (0.5, 21.0, 0.5)).expect("sheep");
        animals.keep_for_test(sheep, tame_at_home());
        animals.calendar(0.0);
        animals.step(&world, &at(500.0, 0.5), 0.05, NOON);
        animals.calendar(8.0);
        animals.step(&world, &at(5.0, 0.5), 0.05, NOON);
        let keep = animals.keeping(sheep);
        assert!(!keep.is_some_and(|k| k.tame), "a flock left eight days unfed is still tame: {keep:?}");
    }

    /// The world's first day of winter, off the calendar.
    fn first_winter_day() -> f32 {
        (0..400)
            .map(|q| q as f32 * 0.25)
            .find(|&day| primitive_shared::season::Season::at(day) == primitive_shared::season::Season::Winter)
            .expect("a year with no winter")
    }

    #[test]
    fn a_parked_flock_with_a_haystack_in_its_pen_is_kept_through_the_winter_and_eats_it_down() {
        // The meadow is turf, and it is winter: the grass feeds nobody
        // (`husbandry::grazes`). One sheep has a stack in reach; the other,
        // twenty blocks off, has nothing. Eight days with nobody there.
        use primitive_shared::types::{haystack_holding, HAYSTACK_HOLDS};
        let world = meadow(30);
        let stack = (2, 21, 0);
        world.put(stack.0, stack.1, stack.2, haystack_holding(HAYSTACK_HOLDS));
        let mut animals = Animals::seeded(2);
        let fed = animals.spawn(Species::Sheep, (0.5, 21.0, 0.5)).expect("sheep");
        let unfed = animals.spawn(Species::Sheep, (20.5, 21.0, 20.5)).expect("sheep");
        animals.keep_for_test(fed, tame_at_home());
        animals.keep_for_test(unfed, Keeping { home: Some((20.5, 21.0, 20.5)), ..tame_at_home() });
        let winter = first_winter_day();
        animals.calendar(winter);
        animals.step(&world, &at(500.0, 0.5), 0.05, NOON);
        assert_eq!(animals.parked_count(), 2);
        animals.calendar(winter + 8.0);
        animals.step(&world, &at(10.0, 10.0), 0.05, NOON);
        let kept = animals.keeping(fed).expect("the fed sheep was forgotten");
        assert!(kept.tame && kept.condition >= husbandry::THRIVING, "a sheep with hay beside it went wild or thin: {kept:?}");
        assert!(!animals.keeping(unfed).is_some_and(|k| k.tame), "a winter pen with no hay kept its sheep tame");
        let bites = animals.take_hay_eaten();
        assert!((6..=9).contains(&bites.len()), "eight days took {} bites of the stack", bites.len());
        assert!(bites.iter().all(|&cell| cell == stack), "a bite came out of somewhere that is not the stack");
    }

    #[test]
    fn a_watched_flock_on_winter_turf_goes_to_the_stack_and_one_on_summer_turf_does_not_need_to() {
        use primitive_shared::types::{haystack_holding, HAYSTACK_HOLDS};
        for (day, wants) in [(first_winter_day(), true), (primitive_shared::season::MIDSUMMER_WORLD_TIME, false)] {
            let world = meadow(20);
            world.put(2, 21, 0, haystack_holding(HAYSTACK_HOLDS));
            let mut animals = Animals::seeded(3);
            let sheep = animals.spawn(Species::Sheep, (0.5, 21.0, 0.5)).expect("sheep");
            animals.keep_for_test(sheep, tame_at_home());
            animals.calendar(day);
            animals.step(&world, &at(4.0, 0.5), 0.05, NOON);
            // A day and three quarters in one lump, as a night slept through
            // hands the step: past `STACK_AFTER_DAYS` without grazing, and
            // short of it with.
            animals.calendar(day + 1.75);
            animals.step(&world, &at(4.0, 0.5), 0.05, NOON);
            let ate = animals.take_hay_eaten().len();
            if wants {
                assert_eq!(ate, 1, "a sheep on winter turf did not go to the stack");
            } else {
                assert_eq!(ate, 0, "a sheep on summer grass ate the winter's hay");
            }
            assert!(!animals.keeping(sheep).is_some_and(|k| k.is_hungry()), "hungry beside a stack on day {day}");
        }
    }

    #[test]
    fn a_tame_sheep_is_sheared_for_wool_once_and_only_again_when_it_has_grown_back() {
        let world = meadow(20);
        let mut animals = Animals::seeded(4);
        let sheep = animals.spawn(Species::Sheep, (0.5, 21.0, 0.5)).expect("sheep");
        let eye = (2.0, 22.6, 0.5);
        let wild = animals.tend(sheep, eye, Some(BLOCK_FLINT_KNIFE));
        assert!(matches!(wild, Tended::Refused(_)), "a wild sheep stood for the knife");
        animals.keep_for_test(sheep, tame_at_home());
        assert_eq!(animals.tend(sheep, eye, Some(BLOCK_FLINT_KNIFE)), Tended::Shorn(husbandry::FLEECE_WOOL));
        assert!(matches!(animals.tend(sheep, eye, Some(BLOCK_FLINT_KNIFE)), Tended::Refused(_)), "sheared twice");
        animals.calendar(0.0);
        for day in 1..=3 {
            animals.calendar(day as f32);
            animals.step(&world, &at(2.0, 0.5), 0.05, NOON);
            let eye = animals.position(sheep).map(|p| (p.0 + 1.5, 22.6, p.2)).expect("alive");
            assert!(matches!(animals.tend(sheep, eye, Some(BLOCK_GRAIN)), Tended::Fed { .. }));
        }
        let eye = animals.position(sheep).map(|p| (p.0 + 1.5, 22.6, p.2)).expect("alive");
        assert_eq!(animals.tend(sheep, eye, Some(BLOCK_FLINT_KNIFE)), Tended::Shorn(husbandry::FLEECE_WOOL));
    }

    #[test]
    fn a_tame_ewe_with_a_lamb_gives_milk_and_the_lamb_grows_slower_for_it() {
        let world = meadow(30);
        let mut animals = Animals::seeded(6);
        let milked = animals.spawn(Species::Sheep, (0.5, 21.0, 0.5)).expect("ewe");
        let left = animals.spawn(Species::Sheep, (0.5, 21.0, 12.5)).expect("ewe");
        let eye = (2.0, 22.6, 0.5);
        animals.keep_for_test(milked, tame_at_home());
        animals.keep_for_test(left, Keeping { home: Some((0.5, 21.0, 12.5)), ..tame_at_home() });
        assert!(matches!(animals.tend(milked, eye, Some(BLOCK_BOWL)), Tended::Refused(_)), "milk with no lamb");
        let lamb = animals.bear_young(milked).expect("lamb");
        let other = animals.bear_young(left).expect("lamb");
        assert!(animals.keeping(lamb).is_some_and(|k| k.tame), "a kept ewe's lamb was born wild");
        animals.calendar(0.0);
        let mut milkings = 0;
        for quarter in 1..=4 {
            if animals.tend(milked, animals.position(milked).map(|p| (p.0 + 1.5, 22.6, p.2)).unwrap(), Some(BLOCK_BOWL))
                == Tended::Milked
            {
                milkings += 1;
            }
            animals.calendar(quarter as f32 * 0.25);
            animals.step(&world, &at(6.0, 6.0), 0.05, NOON);
        }
        assert_eq!(milkings, 1, "milked more than once in a day");
        let (slow, fast) = (animals.growth(lamb).unwrap(), animals.growth(other).unwrap());
        assert!(slow < fast, "the milked ewe's lamb grew as fast: {slow} against {fast}");
    }

    #[test]
    fn two_fed_tame_sheep_breed_and_two_hungry_ones_do_not() {
        for fed in [true, false] {
            let world = meadow(30);
            let mut animals = Animals::seeded(12);
            let pair = [
                animals.spawn(Species::Sheep, (0.5, 21.0, 0.5)).expect("sheep"),
                animals.spawn(Species::Sheep, (2.5, 21.0, 0.5)).expect("sheep"),
            ];
            let keep = if fed {
                tame_at_home()
            } else {
                Keeping { hunger: husbandry::HUNGRY_AFTER_DAYS, well_fed: 0.0, ..tame_at_home() }
            };
            for id in pair {
                animals.keep_for_test(id, keep);
            }
            animals.calendar(0.0);
            for tenth in 1..=30 {
                animals.calendar(tenth as f32 * 0.1);
                if fed {
                    for id in pair {
                        animals.keep_for_test(id, Keeping { home: animals.keeping(id).and_then(|k| k.home), ..tame_at_home() });
                    }
                }
                animals.step(&world, &at(8.0, 8.0), 0.05, NOON);
            }
            let flock = animals.kept().len();
            assert_eq!(flock > 2, fed, "fed {fed}: the flock is {flock}");
        }
    }

    #[test]
    fn a_fed_kept_animal_leaves_dung_as_the_days_go_by() {
        let world = meadow(20);
        let mut animals = Animals::seeded(1);
        let sheep = animals.spawn(Species::Sheep, (0.5, 21.0, 0.5)).expect("sheep");
        animals.keep_for_test(sheep, tame_at_home());
        animals.calendar(0.0);
        animals.calendar(1.0);
        animals.step(&world, &at(4.0, 0.5), 0.05, NOON);
        assert_eq!(animals.take_dung().len(), 2, "a day of a fed sheep is two pats");
    }

    #[test]
    fn a_raid_on_a_meadow_pen_sends_wolves_and_a_two_block_wall_keeps_them_off_the_flock() {
        let world = meadow(40);
        pen(&world, 3, 2);
        let mut animals = Animals::seeded(21);
        let sheep = animals.spawn(Species::Sheep, (0.5, 21.0, 0.5)).expect("sheep");
        animals.keep_for_test(sheep, tame_at_home());
        let raiders = animals.raid(&world, Species::Sheep, (0.5, 21.0, 0.5));
        assert!(!raiders.is_empty(), "nothing came for the pen");
        assert!(raiders.iter().all(|&id| animals.find(id).is_some_and(|w| w.species.hunts(Species::Sheep))));
        let health = animals.health(sheep).expect("alive");
        run(&mut animals, &world, &at(30.0, 30.0), 60.0, MIDNIGHT);
        assert_eq!(animals.health(sheep), Some(health), "the wolves got into a two-block pen");
    }

    #[test]
    fn a_raid_comes_at_most_once_a_night() {
        let world = meadow(40);
        let mut animals = Animals::seeded(21);
        let sheep = animals.spawn(Species::Sheep, (0.5, 21.0, 0.5)).expect("sheep");
        animals.keep_for_test(sheep, tame_at_home());
        animals.calendar(3.0);
        let before = animals.len();
        for _ in 0..2000 {
            animals.raid_the_pens(&world, true);
        }
        let first = animals.len() - before;
        assert!(first > 0, "two thousand tries and no raid");
        for _ in 0..2000 {
            animals.raid_the_pens(&world, true);
        }
        assert_eq!(animals.len() - before, first, "a second raid came the same night");
    }

    /// **The round trip the whole save exists for**: a tamed sheep in a pen,
    /// sheared and part grown back, with a lamb, written and read into a new
    /// world -- and put back into it when somebody comes near.
    #[test]
    fn a_tamed_penned_sheep_comes_back_from_the_save_with_its_fleece_its_home_and_its_lamb() {
        let world = meadow(20);
        pen(&world, 3, 2);
        let mut animals = Animals::seeded(8);
        let ewe = animals.spawn(Species::Sheep, (0.5, 21.0, 0.5)).expect("ewe");
        animals.keep_for_test(ewe, Keeping { fleece: 0.4, ..tame_at_home() });
        let lamb = animals.bear_young(ewe).expect("lamb");
        let wild = animals.spawn(Species::Deer, (8.5, 21.0, 8.5)).expect("deer");
        let _ = wild;
        let ewe_keep = animals.keeping(ewe).expect("kept");
        let dir = std::env::temp_dir().join(format!("primitive-herd-{}", std::process::id()));
        assert_eq!(animals.save_herd(&dir).expect("written"), 2, "the deer was saved or the flock was not");

        let mut again = Animals::seeded(8);
        assert_eq!(again.load_herd(&dir).expect("read"), 2);
        assert!(again.is_empty(), "loaded straight into the world before anybody was near");
        again.step(&world, &at(4.0, 4.0), 0.05, NOON);
        let kept = again.kept();
        let (new_ewe, _, keep) = kept.iter().copied().find(|&(id, _, k)| {
            k.fleece_ready() == ewe_keep.fleece_ready() && again.growth(id) == Some(youth::GROWN)
        }).expect("the ewe did not come back");
        assert_eq!(keep, ewe_keep, "the ewe came back as somebody else");
        let new_lamb = kept.iter().find(|&&(id, _, _)| id != new_ewe).map(|&(id, _, _)| id).expect("no lamb");
        assert_eq!(again.mother_of(new_lamb), Some(new_ewe), "the lamb came back an orphan");
        assert_eq!(again.growth(new_lamb), animals.growth(lamb));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_herd_file_from_another_version_is_a_world_with_no_kept_animals() {
        let dir = std::env::temp_dir().join(format!("primitive-herd-v-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let bytes = bincode::serialize(&HerdFile { version: HERD_FORMAT_VERSION + 1, day: 0.0, animals: Vec::new() }).unwrap();
        std::fs::write(dir.join("herd.bin"), bytes).unwrap();
        assert_eq!(Animals::new().load_herd(&dir).unwrap(), 0);
        let _ = std::fs::remove_dir_all(&dir);
    }
}

#[cfg(test)]
mod horse_tests {
    use super::*;
    use crate::logic::falling::tests::TestWorld;
    use primitive_shared::husbandry::Keeping;
    use primitive_shared::types::{BLOCK_AIR, BLOCK_COPPER_ORE, BLOCK_SADDLE, BLOCK_SADDLEBAGS, BLOCK_STONE};

    const NOON: f32 = 0.5;
    const RIDER: PlayerId = 1;

    fn meadow(span: i32) -> TestWorld {
        let world = TestWorld::default();
        for z in -span..=span {
            for x in -span..=span {
                world.put(x, 20, z, BLOCK_GRASS);
                for y in 21..30 {
                    world.put(x, y, z, BLOCK_AIR);
                }
            }
        }
        world
    }

    /// Somebody standing well off, so nothing is forgotten for being far
    /// from everybody (`forget_the_distant`) and nothing is near enough to
    /// be frightened of them.
    fn watcher() -> Vec<(PlayerId, (f32, f32, f32))> {
        vec![(9, (-40.0, 21.0, -40.0))]
    }

    fn tame() -> Keeping {
        Keeping { trust: 1.0, tame: true, home: Some((0.5, 21.0, 0.5)), hunger: 0.0, well_fed: 1.0, ..Keeping::wild() }
    }

    fn gentled() -> Keeping {
        Keeping { trust: 1.0, tame: false, hunger: 0.0, ..Keeping::wild() }
    }

    /// Standing a stride off its flank, where a rider gets on from.
    fn beside(animals: &Animals, horse: EntityId) -> (f32, f32, f32) {
        let p = animals.position(horse).expect("the horse is gone");
        (p.0 + 1.2, p.1, p.2)
    }

    fn saddled_horse(animals: &mut Animals) -> EntityId {
        let horse = animals.spawn(Species::Horse, (0.5, 21.0, 0.5)).expect("a horse");
        animals.keep_for_test(horse, tame());
        animals.gear_for_test(horse).expect("gear").saddle = true;
        animals.face_for_test(horse, 0.0);
        horse
    }

    /// `seconds` of riding with `reins` sent every quarter second, as a
    /// client does: answers the horse's fastest speed over the ground.
    fn ride(animals: &mut Animals, world: &TestWorld, horse: EntityId, reins: Option<horse::Reins>, seconds: f32) -> f32 {
        let mut fastest = 0.0f32;
        for tick in 0..(seconds / 0.05) as usize {
            if let Some(reins) = reins.filter(|_| tick % 5 == 0) {
                assert!(animals.rein(horse, RIDER, reins), "the reins went to nobody");
            }
            let at = animals.ridden_by(RIDER).map(|(_, body, _)| body.saddle());
            let players: Vec<(PlayerId, (f32, f32, f32))> =
                at.map(|s| vec![(RIDER, (s[0] as f32, s[1] as f32, s[2] as f32))]).unwrap_or_default();
            animals.step(world, &players, 0.05, NOON);
            if let Some((_, body, _)) = animals.ridden_by(RIDER) {
                fastest = fastest.max(body.speed());
            }
        }
        fastest
    }

    fn gallop() -> Option<horse::Reins> {
        Some(horse::Reins { forward: 1.0, turn: 0.0, gait: horse::Gait::Gallop, jump: false })
    }

    #[test]
    fn a_wild_horse_will_not_be_got_on_and_a_gentled_one_throws_its_rider_until_it_is_broken() {
        let world = meadow(40);
        let mut animals = Animals::seeded(11);
        let horse = animals.spawn(Species::Horse, (0.5, 21.0, 0.5)).expect("a horse");
        assert!(matches!(animals.mount(horse, RIDER, beside(&animals, horse)), Mounting::Refused(_)), "a wild horse let a stranger on");
        animals.keep_for_test(horse, gentled());
        let mut tries = 0;
        let broke = loop {
            tries += 1;
            assert!(tries <= husbandry::THROW_CHANCES.len(), "still throwing its rider after {tries} tries");
            match animals.mount(horse, RIDER, beside(&animals, horse)) {
                Mounting::Riding { broke, .. } => break broke,
                Mounting::Thrown { .. } => {
                    // Too soon, and it will not be tried at all.
                    assert!(matches!(animals.mount(horse, RIDER, beside(&animals, horse)), Mounting::Refused(_)));
                    for _ in 0..((husbandry::SETTLE_SECONDS + 0.5) / 0.05) as usize {
                        animals.step(&world, &watcher(), 0.05, NOON);
                    }
                }
                Mounting::Refused(why) => panic!("a gentled horse refused a try: {why:?}"),
            }
        };
        assert!(broke, "the ride that stayed on did not say it broke the horse");
        assert!(animals.keeping(horse).is_some_and(|k| k.tame && k.home.is_some()), "a broken horse is not tame");
        assert!(matches!(animals.mount(horse, 2, beside(&animals, horse)), Mounting::Refused(_)), "two riders on one horse");
    }

    #[test]
    fn a_saddled_horse_gallops_faster_than_a_sprint_and_stands_when_the_reins_go_quiet() {
        let world = meadow(120);
        let mut animals = Animals::seeded(12);
        let horse = saddled_horse(&mut animals);
        assert!(matches!(animals.mount(horse, RIDER, beside(&animals, horse)), Mounting::Riding { broke: false, .. }));
        let fastest = ride(&mut animals, &world, horse, gallop(), 4.0);
        assert!(fastest > primitive_shared::animals::NOMINAL_SPRINT_SPEED * 1.5, "a gallop under a rider was {fastest}");
        ride(&mut animals, &world, horse, None, 3.0);
        let (_, body, _) = animals.ridden_by(RIDER).expect("fell off");
        assert!(body.speed() < 0.1, "a horse nobody was asking went on at {}", body.speed());
    }

    #[test]
    fn bareback_a_horse_trots_and_is_never_asked_to_gallop() {
        let world = meadow(120);
        let mut animals = Animals::seeded(13);
        let horse = saddled_horse(&mut animals);
        animals.gear_for_test(horse).expect("gear").saddle = false;
        animals.mount(horse, RIDER, beside(&animals, horse));
        let fastest = ride(&mut animals, &world, horse, gallop(), 4.0);
        assert!(fastest <= horse::TROT + 0.05, "bareback it galloped at {fastest}");
    }

    #[test]
    fn a_heavy_load_in_the_bags_slows_the_gallop() {
        let world = meadow(120);
        let speed_with = |ore: u32| {
            let mut animals = Animals::seeded(14);
            let horse = saddled_horse(&mut animals);
            let mut bags = primitive_shared::inventory::Inventory::new();
            if ore > 0 {
                bags.add(BLOCK_COPPER_ORE, ore);
            }
            animals.gear_for_test(horse).expect("gear").bags = Some(bags);
            animals.mount(horse, RIDER, beside(&animals, horse));
            ride(&mut animals, &world, horse, gallop(), 4.0)
        };
        let (empty, laden) = (speed_with(0), speed_with(64));
        assert!(laden < empty * 0.9, "sixty-four ore cost nothing: {empty} empty, {laden} laden");
    }

    #[test]
    fn a_horse_killed_under_its_load_leaves_the_saddle_the_bags_and_every_stack() {
        let mut animals = Animals::seeded(15);
        let horse = saddled_horse(&mut animals);
        let mut bags = primitive_shared::inventory::Inventory::new();
        bags.add(BLOCK_COPPER_ORE, 30);
        animals.gear_for_test(horse).expect("gear").bags = Some(bags);
        animals.hurt(horse, 10_000.0).expect("it did not die");
        let spilled = animals.take_spilled();
        let all: Vec<(primitive_shared::types::BlockId, u32, u32)> = spilled.into_iter().flat_map(|(_, left)| left).collect();
        assert!(all.contains(&(BLOCK_SADDLE, 1, 0)) && all.contains(&(BLOCK_SADDLEBAGS, 1, 0)), "the tack vanished: {all:?}");
        assert_eq!(all.iter().filter(|(b, _, _)| *b == BLOCK_COPPER_ORE).map(|(_, n, _)| n).sum::<u32>(), 30);
    }

    #[test]
    fn a_knife_takes_the_bags_off_a_living_horse_with_their_load_and_then_the_saddle() {
        let mut animals = Animals::seeded(15);
        let horse = saddled_horse(&mut animals);
        let mut bags = primitive_shared::inventory::Inventory::new();
        bags.add(BLOCK_COPPER_ORE, 30);
        animals.gear_for_test(horse).expect("gear").bags = Some(bags);
        let eye = (2.0, 22.6, 0.5);
        // A pack with no room: the bags stay on, load and all.
        let refused = animals.unbuckle(horse, eye, |_| false);
        assert!(matches!(refused, Unbuckled::Refused(Notice::PackCannotTakeBags)), "{refused:?}");
        assert!(animals.gear(horse).is_some_and(|g| g.load_kg() > 0.0), "the load went nowhere");
        // Room: the bags come off first, with every stack in them...
        let Unbuckled::Bags(load) = animals.unbuckle(horse, eye, |_| true) else {
            panic!("the bags did not come off");
        };
        assert_eq!(load.count(BLOCK_COPPER_ORE), 30, "the load was not in the bags that came off");
        assert!(animals.gear(horse).is_some_and(|g| g.saddle && g.bags.is_none()));
        // ...then the saddle, and then there is nothing left.
        assert!(matches!(animals.unbuckle(horse, eye, |_| true), Unbuckled::Saddle));
        assert_eq!(animals.gear(horse).map(|g| g.tack()), Some(0), "the horse still wears something");
        assert!(matches!(animals.unbuckle(horse, eye, |_| true), Unbuckled::Refused(Notice::NothingToUnbuckle)));
        // The saddle goes back on, as it came off.
        assert_eq!(animals.tend(horse, eye, Some(BLOCK_SADDLE)), Tended::Saddled);
    }

    #[test]
    fn nothing_comes_off_a_horse_somebody_is_riding() {
        let mut animals = Animals::seeded(15);
        let horse = saddled_horse(&mut animals);
        assert!(matches!(animals.mount(horse, RIDER, (1.5, 21.0, 0.5)), Mounting::Riding { .. }));
        assert!(matches!(animals.unbuckle(horse, (2.0, 22.6, 0.5), |_| true), Unbuckled::Refused(Notice::SomebodyOnIt)));
        assert!(animals.gear(horse).is_some_and(|g| g.saddle), "the saddle came off under its rider");
    }

    #[test]
    fn a_kept_horse_left_out_in_the_rain_loses_condition_and_one_under_a_roof_does_not() {
        let world = meadow(20);
        // A slab of stone over the second horse's stall.
        for z in 5..=7 {
            for x in 5..=7 {
                world.put(x, 24, z, BLOCK_STONE);
            }
        }
        let mut animals = Animals::seeded(16);
        let out = animals.spawn(Species::Horse, (-5.5, 21.0, -5.5)).expect("a horse");
        let stabled = animals.spawn(Species::Horse, (6.5, 21.0, 6.5)).expect("a horse");
        for horse in [out, stabled] {
            animals.keep_for_test(horse, tame());
        }
        animals.rain(true);
        animals.calendar(0.0);
        animals.calendar(1.0);
        animals.step(&world, &watcher(), 0.05, NOON);
        let condition = |h| animals.keeping(h).expect("kept").condition;
        assert!(
            condition(out) < condition(stabled) - 0.3,
            "the rain cost nothing: {} out, {} under a roof",
            condition(out),
            condition(stabled)
        );
    }

    #[test]
    fn a_stallion_goes_after_a_mare_that_has_strayed_and_a_mare_in_his_place_does_not() {
        // The same herd twice, from the same seed: once with the fourth horse
        // its stallion, once without.
        let closest = |is_stallion: bool| {
            let world = meadow(60);
            let mut animals = Animals::seeded(17);
            let lead = animals.spawn(Species::Horse, (0.5, 21.0, 0.5)).expect("a horse");
            let _mare = animals.spawn(Species::Horse, (2.5, 21.0, 0.5)).expect("a horse");
            let stray = animals.spawn(Species::Horse, (16.5, 21.0, 16.5)).expect("a horse");
            let fourth = animals.spawn(Species::Horse, (1.5, 21.0, 2.5)).expect("a horse");
            if is_stallion {
                animals.make_stallion_for_test(fourth);
                assert!(animals.is_stallion(fourth) && !animals.is_stallion(lead));
            }
            let gap = |a: &Animals| {
                let (s, m) = (a.position(fourth).expect("the fourth"), a.position(stray).expect("the mare"));
                (s.0 - m.0).hypot(s.2 - m.2)
            };
            let mut nearest = gap(&animals);
            for _ in 0..(12.0 / 0.05) as usize {
                animals.step(&world, &watcher(), 0.05, NOON);
                nearest = nearest.min(gap(&animals));
            }
            nearest
        };
        let (stallion, mare) = (closest(true), closest(false));
        // To within `STRAY` of her and a stride, which is where the herd's own
        // rules take over: he goes to her, not onto her.
        assert!(stallion <= STRAY + 2.5, "the stallion never went for the stray: at best {stallion}");
        assert!(mare > STRAY + 2.5, "a plain mare went after the stray as well: {mare}");
    }

    #[test]
    fn a_saddled_horse_and_its_load_come_back_from_the_herd_file_and_a_version_one_file_still_loads() {
        let mut animals = Animals::seeded(18);
        let horse = saddled_horse(&mut animals);
        let mut bags = primitive_shared::inventory::Inventory::new();
        bags.add(BLOCK_COPPER_ORE, 12);
        animals.gear_for_test(horse).expect("gear").bags = Some(bags);
        let dir = std::env::temp_dir().join(format!("primitive-horse-herd-{}", std::process::id()));
        assert_eq!(animals.save_herd(&dir).expect("written"), 1);
        let mut again = Animals::seeded(19);
        assert_eq!(again.load_herd(&dir).expect("read"), 1);
        let world = meadow(10);
        again.step(&world, &[(RIDER, (2.0, 21.0, 0.5))], 0.05, NOON);
        let back = again.ids().into_iter().find(|&id| again.horse_at(id).is_some()).expect("the horse did not come back");
        let gear = again.gear_for_test(back).expect("gear");
        assert!(gear.saddle, "the saddle did not come back");
        assert_eq!(gear.bags.as_ref().map(|b| b.count(BLOCK_COPPER_ORE)), Some(12), "the load did not come back");

        // The shape before the horse, written as it was written then.
        #[derive(serde::Serialize)]
        struct RecordV1 {
            species: Species,
            position: (f64, f64, f64),
            yaw: f32,
            health: f32,
            growth: f32,
            birth_rest: f32,
            mother: Option<u32>,
            keep: husbandry::Keeping,
            parked_on: Option<f32>,
        }
        #[derive(serde::Serialize)]
        struct FileV1 {
            version: u32,
            day: f32,
            animals: Vec<RecordV1>,
        }
        let old = FileV1 {
            version: 1,
            day: 0.0,
            animals: vec![RecordV1 {
                species: Species::Sheep,
                position: (1.5, 21.0, 1.5),
                yaw: 0.0,
                health: 10.0,
                growth: youth::GROWN,
                birth_rest: 0.0,
                mother: None,
                keep: tame(),
                parked_on: None,
            }],
        };
        std::fs::write(dir.join("herd.bin"), bincode::serialize(&old).unwrap()).unwrap();
        assert_eq!(Animals::seeded(20).load_herd(&dir).expect("read"), 1, "a version-one flock was lost");
        let _ = std::fs::remove_dir_all(&dir);
    }
}
