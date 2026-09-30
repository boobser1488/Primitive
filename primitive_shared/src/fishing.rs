//! Fishing that is not a spear: the trap, and the rod.
//!
//! ## What there was
//!
//! Fish swim in every water deeper than two blocks and a cod keeps to the
//! open sea (`animals::Species::Fish`, `Cod`), and the only way to take one
//! was to swim after the school with a spear. That stays, and stays the
//! quickest meal in a sea full of fish. What it cannot be is a *place*: a
//! school is wherever the spawn put it, so a player never had a reason to
//! pick one stretch of river over another, and a coast was worth living on
//! only while you were in the water.
//!
//! ## The two ways, and what each is a decision about
//!
//! * **A trap (`types::BLOCK_FISH_TRAP`) is a decision about *where*.** Six
//!   reeds and a cord, no knife and no metal, so it is there on the first
//!   evening beside the spear. Set it in the water and walk away; the clock
//!   the meat in a pack rots on (four steps a day) lets a fish in or does
//!   not, and it holds [`TRAP_HOLDS`]. It likes what a weir likes: **running
//!   water, shallow, with a body of water behind it** ([`trap_chance`]) --
//!   so the shallows of a river are worth walking to, a pond is worth
//!   something and the sea very little. What the hour and the sky are doing
//!   is not asked, and that is deliberate: a trap works day and night, and a
//!   rate read at the moment it is checked would pay a whole night at the
//!   dawn rate to whoever came back at dawn. **What goes into it is always
//!   one fish**, whatever swims in that water: a basket is a small trap,
//!   the big fish are the rod's and the spear's, and a trap that landed a
//!   pike would make the one way that needs nobody standing there the best
//!   way as well.
//! * **A rod (`types::BLOCK_FISHING_ROD`) is a decision about *when*.**
//!   It waits for copper, because its hook does. The throw is held and let
//!   go ([`cast_power`], [`cast_target`]) and the float lands where it was
//!   aimed; the server rolls how long until a bite from the place, the hour,
//!   the sky, the season, the cold and **what is on the hook**
//!   ([`rod_wait_seconds`]). The rod wants the opposite water to the trap --
//!   **deep, wide, and best of all the sea** -- and it wants it **at
//!   dawn or dusk, under rain**: noon is slow, midnight slower, a storm
//!   puts the fish down and cold water is slow water. A bite is a float
//!   that trembles, dips and goes ([`Bite`]) and not a catch: strike on the
//!   dip and the fish is hooked, and then it has to be fought up
//!   ([`Fight`]).
//!
//! So a player with both is planning: the traps in the river shallows by
//! camp, checked on the way out; the rod carried to the lake at dusk, or
//! down to the sea on a wet morning. Neither has one right answer, which is
//! the test a mechanic in this game has to pass.
//!
//! ## What the rod asks, one question at a time
//!
//! The rod used to answer "how long until a bite" and "which fish" from the
//! biome and the depth, and a player who had learned that dusk is good and
//! a worm is fine had learned the rod. What it asks now, each question
//! argued where it is answered:
//!
//! * **Which water** ([`Habitat`]). The water already has a climate -- how
//!   cold, and what is in it: peat, nothing, rock flour
//!   (`worldgen::water_climate_of`) -- and the fish read it. A trout wants
//!   cold clear water and runs from a brown bog; a pike wants a warm weedy
//!   lake and does not mind the peat; the school is everywhere and thins in
//!   the milky melt under a glacier. So a tarn, a marsh and a meadow lake a
//!   day's walk apart are three different answers from one rod.
//! * **How big** ([`Size`]). Every catch is small, middling or big, and the
//!   size is the count in the pack and the weight of the fight. Big fish
//!   want room (a deep wide lake, the sea), are stunted by a marsh, are the
//!   first to go from a fished bank, and take a big bait -- so a scrap of
//!   meat is fewer bites and heavier fish, and a grub is a busy float of
//!   small ones. Still one fish item: see "Species with names" below.
//! * **When** ([`Moment`], [`appetite`]). Dawn and dusk; worse under a
//!   clear noon sun in shallow water ([`glare_factor`]); poor in winter; and
//!   best of all **in the last minutes before rain** ([`front_factor`]),
//!   which the server knows because the sky rolls its next spell when this
//!   one begins. A player cannot read the forecast. They can read the fish
//!   jumping ([`rise_rate`] is the same appetite), which is how anybody
//!   who fishes has always known.
//! * **What the float says** ([`Bite`]). It trembles (a fish nosing the
//!   bait), dips half under (it has it), and goes under and away (it is
//!   running with it). A strike on the tremble is too early -- the fish is
//!   gone and so is the cast, the bait kept; no strike by the end of the
//!   run is too late -- the bait is eaten. A tremble sometimes comes to
//!   nothing, which is what teaches a player to wait for the dip.
//! * **Whether the tackle holds** ([`Fight`], [`snap_shock`]). A line that
//!   parts under a heavy fish jars the rod, and a worn rod does not survive
//!   a big one: a reason to play the fish, and to carry a spare to the lake
//!   where the pike are.
//!
//! ## What was weighed and left out
//!
//! * **A bone hook.** A gorge of bone on a line is older than any metal,
//!   and it was the first version of the rod. It made the rod a first-
//!   evening item beside the trap and the bone spear, and three ways to
//!   fish on the first evening is two ways too many: the rod stopped being
//!   a rung. Copper is where a player first walks a long way for something,
//!   and a hook is a small, cheap reason to have made that walk -- four to
//!   an ingot.
//! * **A net.** A net is a trap spread wider. It would ask the same
//!   question as the trap ("where") with a bigger number, and cost cloth
//!   the trap does not -- a second passive way with no second decision.
//! * **Bait that is only a chore.** This note used to argue bait away
//!   altogether: one more item to carry and top up, whose only effect is
//!   that forgetting it makes the rod not work. **It set the condition for
//!   bait coming back, and [`Bait`] is that condition met**: the bare hook
//!   still fishes ([`BARE_HOOK`]), slowly and for the commonest fish only,
//!   so nothing is ever *blocked* on having a worm; and every bait is
//!   wanted by a different fish ([`Bait::tempts`]), so what goes on the
//!   hook is the same kind of choice as where to stand. A player who wants
//!   a pike digs no worms -- a pike wants meat.
//! * **A strike to time.** This one was wrong twice over. It called the
//!   strike reflex rather than decision, which is only true if striking is
//!   free: a strike at nothing pulls the float back out and costs the cast
//!   ([`Strike::TooEarly`]), so the decision is whether that twitch was the
//!   fish -- and since the float trembles before it dips ([`Bite`]), that
//!   decision is one a player gets to make on every bite. And it worried about the message, which is one message
//!   (`protocol::ServerMessage::Line`) rather than the two it feared: the
//!   float, its phase and the strain on the line travel together because
//!   they are one picture.
//! * **A mini-game with its own screen.** Every fishing screen ever drawn
//!   is a bar with a moving marker, and it stops the world: the wolf behind
//!   you does not exist while it is up. Everything here happens in the
//!   world -- the float is out there on the water, the strain is on the
//!   line, and a player who is being rained on is still being rained on.
//! * **Species with names, as *items*.** Perch in the river, pike in the
//!   lake, herring in the sea: a raw one of each would want a cooked one, a
//!   salted one, a dried one and a salt-dried one behind it, and every one
//!   of those differs from the raw fish only in a number. That still holds,
//!   and there is still one fish item.
//!
//!   **What this note used to say as well, and was wrong about, is the
//!   water.** It read as an argument against the *fish* rather than against
//!   the items, and for a release it was taken as one: every water in the
//!   world held the same school, so a mountain tarn, a mangrove and the
//!   open sea were one place with three colours. The fish are named in
//!   `animals::Species` now -- a trout in cold running water, a pike in a
//!   warm lake, a herring in the shallow sea, the cod where it always was
//!   -- and what comes out of the water still goes into the pack as fish,
//!   in the amount that fish is worth ([`rod_species`], [`catch_count`]).
//!   Three pictures and three shapes; no new items and no new recipes.
//!
//! ## Why this is shared
//!
//! The server decides every catch. The client asks the same [`survey`] of
//! its own chunks for one reason: to say *in the player's language* that a
//! puddle holds no fish, rather than send a cast the server will refuse in
//! English. The two cannot disagree about what water is, because they are
//! reading one function.

use crate::animals::Species;
use crate::body::Water;
use crate::season::Season;
use crate::types::{is_liquid, BlockId};
use crate::weather::Weather;
use crate::worldgen::Biome;

/// How far sideways from the float the water is counted, in blocks.
///
/// Four: the nine-by-nine the field's `WATER_REACH` also uses, which is
/// what a player can count along a bank. Far enough that a river three wide
/// with a bend in it reads as the river, near enough that a channel two
/// wide dug off a lake reads as the channel.
pub const SURVEY_REACH: i32 = 4;

/// How far down under the surface the water is counted.
pub const SURVEY_DOWN: i32 = 6;

/// The most cells a survey visits. The water a survey can see is at most
/// nine by nine by six; a count stopped here is already "a lot of water",
/// and the cap is what keeps a cast on the open sea from walking a volume
/// nobody needs counted.
pub const SURVEY_CAP: usize = 200;

/// **The least water that holds a fish at all**: twelve cells.
///
/// The number a puddle has to be refused by. A hole one block across and a
/// block deep is one cell; a pond two by two and three deep is twelve --
/// the smallest thing a player digs that is plainly a pond. Below this
/// nothing bites and nothing swims into a trap, which is what the spawn
/// already says (`animals::populate_water` puts no school in a puddle).
pub const MIN_WATER_CELLS: u16 = 12;

/// The water at which a place counts as big: a hundred cells, about a
/// pond five across and four deep. Past it, more water is not more fish.
pub const FULL_WATER_CELLS: u16 = 100;

/// A rod needs this much water under the float: two blocks.
///
/// A float in a hand's depth of water is a float on the bottom. A trap
/// asks nothing of the kind -- the shallows are where it wants to be.
pub const MIN_ROD_DEPTH: u8 = 2;

/// How many fish one trap holds before nothing more can get in.
///
/// Three: a meal and a half at the rate `food` pays for a cooked fish, and
/// about a day and a half in the best river shallows. A trap is somewhere
/// you come back to, and a trap that held a week of fish would be somewhere
/// you came back to once a week.
pub const TRAP_HOLDS: u8 = 3;

/// The chance a fish goes into a trap in the best water there is, per step
/// of the rot clock (four a day).
///
/// A half, so a trap in a river's shallows takes about two fish a day and
/// is full in a day and a half; in a pond it is nearer one, and in the sea
/// under one. Against the spear: two fish a day is less than one dive into
/// a school, and it is two fish a player did not spend the day on.
pub const TRAP_BEST_CHANCE: f32 = 0.5;

/// How long a bite takes, on average, in good water on a clear day at an
/// ordinary hour, in seconds.
///
/// Thirty. At dusk in the rain on the sea that comes down to a quarter of a
/// minute; at noon on a small pond it is three minutes, which is the rod
/// telling a player to go somewhere else.
pub const ROD_TYPICAL_SECONDS: f32 = 30.0;

/// The shortest a bite can come, whatever the roll: a float that has not
/// finished landing has not been taken.
pub const ROD_SHORTEST_SECONDS: f32 = 4.0;

/// **The longest average wait there is**: ten minutes.
///
/// Everything about a spot multiplies ([`rod_wait_seconds`]), and the worst
/// corner of it -- a small cold pond, at midnight, in winter, fished out,
/// with a berry on the hook -- came to over an hour. An hour and ten minutes
/// are the same sentence to a player ("not here"), and the difference
/// between them is only that the first one reads as a rod that is broken.
/// So the sentence is said in ten minutes and the float still twitches
/// while it is said.
pub const ROD_LONGEST_SECONDS: f32 = 600.0;

/// How deep water has to be for a cod: the open sea's shelf, as
/// `animals::Species::Cod` lives there.
pub const COD_DEPTH: u8 = 8;

/// How many rolls a catch is drawn from; every named fish below is a
/// fraction of this.
///
/// **Named `COD_ONE_IN` because the cod was the only one**, and kept under
/// that name because the number is on the server's roll
/// (`Fishing::roll_below`) and in a saved cast. One fish in five off a deep
/// enough sea is a cod, which is three fish.
pub const COD_ONE_IN: u32 = 5;

/// How deep still fresh water has to be before a pike is in it: the same
/// three blocks `animals::needs_depth` asks of the fish itself, so the rod
/// and the spawner agree about which ponds hold one.
pub const PIKE_DEPTH: u8 = 3;

/// How many of the five rolls are a trout, in running fresh water.
///
/// Two: a river is a trout stream more often than not, and a trout is two
/// fish. That is the *small* reward of the three, and it is the one a
/// player can have on the first morning with a hook and a bank -- the pike
/// wants a lake deep enough to swim in and the cod wants the open sea.
pub const TROUT_IN_FIVE: u32 = 2;

/// **How far a player may walk from the float and still be fishing**, in
/// blocks from the eye to the float's cell.
///
/// The longest throw ([`CAST_FARTHEST`]) and four steps more: enough to
/// shift along the bank, stand up, or back away from the water while the
/// fish is on, and not enough to cast, walk home and let the fish come to
/// the pack. A rod with nobody holding it is a stick lying on the bank.
///
/// **It was seven, when a cast could only reach what the player was aiming
/// at within arm's length.** A throw that carries twelve blocks and a leash
/// of seven is a line that snaps itself the moment it lands, which is how
/// this number was found.
pub const CAST_HOLDS: f32 = CAST_FARTHEST + 4.0;

/// What a survey found: how much water, how deep at the float, and what is
/// growing in it or hanging over it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Spot {
    /// Cells of water joined to the float, within [`SURVEY_REACH`] and
    /// [`SURVEY_DOWN`], up to [`SURVEY_CAP`].
    pub cells: u16,
    /// Blocks of water from the surface to the bed in the float's column.
    pub depth: u8,
    /// Surveyed cells that are weed rather than open water: kelp, a coral, a
    /// lily pad on the surface, a reed leaning out of the bank.
    ///
    /// **Fish hold in weed**, and that is the one thing about a spot a
    /// player can see from the bank before casting -- which is why it is
    /// counted at all. See [`cover_factor`].
    pub weeds: u16,
    /// Something solid hanging over the float: a bank cut, a tree, the
    /// planks of a jetty. Shade is the other thing a fish holds under, and
    /// the other thing a player can see before casting.
    pub shade: bool,
    /// Surveyed cells of water with ice lying on them: the lake under a
    /// winter lid, seen from the hole the float went down.
    ///
    /// Counted so that the hole can be worth cutting ([`ice_factor`]); the
    /// rest of the survey does not care, because water under ice is water.
    pub iced: u16,
}

/// How far above the surface a survey looks for something to be shaded by.
///
/// Five: the overhang of a bank, a jetty, a branch of the willow leaning
/// out. Higher and every pool at the bottom of a gorge would read as shade,
/// which is a cliff a hundred blocks up and no cover at all.
pub const SHADE_UP: i32 = 5;

impl Spot {
    /// Is there enough water here for a fish at all?
    pub fn holds_fish(self) -> bool {
        self.cells >= MIN_WATER_CELLS
    }

    /// 0.35 for the smallest pond, rising to 1 at [`FULL_WATER_CELLS`].
    /// Never nought above the floor: a small pond is a poor place, not an
    /// empty one.
    fn size_factor(self) -> f32 {
        let span = f32::from(FULL_WATER_CELLS - MIN_WATER_CELLS);
        let over = f32::from(self.cells.saturating_sub(MIN_WATER_CELLS));
        0.35 + 0.65 * (over / span).clamp(0.0, 1.0)
    }
}

/// The water at `at`, if `at` is water.
///
/// The float's column is climbed to the surface and counted down to the
/// bed, and the water joined to it is flooded out sideways. **An unloaded
/// cell counts as not water**: a survey that guessed would be a catch out
/// of a chunk nobody has, and the side that guesses short only ever
/// under-promises at the edge of the world.
pub fn survey(block_at: impl Fn(i32, i32, i32) -> Option<BlockId>, at: (i32, i32, i32)) -> Option<Spot> {
    let wet = |x: i32, y: i32, z: i32| block_at(x, y, z).is_some_and(is_liquid);
    if !wet(at.0, at.1, at.2) {
        return None;
    }
    let mut top = at.1;
    while top - at.1 < 32 && wet(at.0, top + 1, at.2) {
        top += 1;
    }
    let mut depth: u8 = 0;
    while depth < 32 && wet(at.0, top - i32::from(depth), at.2) {
        depth += 1;
    }
    let start = (at.0, top, at.2);
    // **Shade is asked of the float's own column and nowhere else.** A pool
    // is not shaded because there is a tree at the far end of it; the float
    // is under a branch or it is not.
    let shade = (1..=SHADE_UP).any(|up| {
        block_at(at.0, top + up, at.2).is_some_and(|block| !is_liquid(block) && block != crate::types::BLOCK_AIR)
    });
    let mut weeds: u16 = 0;
    let mut iced: u16 = 0;
    let mut seen = std::collections::HashSet::new();
    let mut queue = std::collections::VecDeque::new();
    seen.insert(start);
    queue.push_back(start);
    while let Some((x, y, z)) = queue.pop_front() {
        // **Weed is counted as the flood passes**, so it is the weed in the
        // water this cast reaches rather than the weed within a box: a
        // reedbed on the other side of a spit is not this float's cover.
        if is_weed(&block_at, (x, y, z)) {
            weeds = weeds.saturating_add(1);
        }
        // ...and so is the ice: the lid over *this* water, not over a
        // frozen puddle beside it.
        if block_at(x, y + 1, z).is_some_and(|above| crate::types::block_kind(above) == crate::types::BLOCK_ICE) {
            iced = iced.saturating_add(1);
        }
        if seen.len() >= SURVEY_CAP {
            break;
        }
        for (dx, dy, dz) in [(1, 0, 0), (-1, 0, 0), (0, 0, 1), (0, 0, -1), (0, -1, 0), (0, 1, 0)] {
            let next = (x + dx, y + dy, z + dz);
            if (next.0 - at.0).abs() > SURVEY_REACH
                || (next.2 - at.2).abs() > SURVEY_REACH
                || next.1 > top
                || next.1 <= top - SURVEY_DOWN
                || seen.contains(&next)
                || !wet(next.0, next.1, next.2)
            {
                continue;
            }
            seen.insert(next);
            queue.push_back(next);
        }
    }
    Some(Spot {
        cells: seen.len().min(usize::from(u16::MAX)) as u16,
        depth,
        weeds,
        shade,
        iced,
    })
}

/// Is this cell of water weed rather than open water?
///
/// Two ways of being weed, because the world grows them two ways: a plant
/// that *is* the water cell (kelp, a frond, a coral -- all of them liquid so
/// that a swimmer swims through them, see `types::BLOCK_KELP`), and a plant
/// standing in the cell above (a lily pad, a reed, grass leaning off the
/// bank). Plain water with plain air over it is open water.
fn is_weed(block_at: impl Fn(i32, i32, i32) -> Option<BlockId>, at: (i32, i32, i32)) -> bool {
    let here = block_at(at.0, at.1, at.2);
    if here.is_some_and(|block| is_liquid(block) && crate::types::block_kind(block) != crate::types::BLOCK_WATER) {
        return true;
    }
    block_at(at.0, at.1 + 1, at.2).is_some_and(crate::types::is_foliage)
}

/// The water a trap at `trap` is set in, and a cell of it to ask what kind
/// of water it is: `None` for a trap that is not in the water.
///
/// **In the water means water on two sides of it**, not one. A basket on
/// the bank with one face touching the river is a basket on the bank; one
/// wedged into the bank with the river round two faces -- or standing in
/// the stream with water all round -- is a trap. The biggest water on any
/// side is the water it fishes, so a trap in a channel between a pond and a
/// river fishes the river.
pub fn trap_water(
    block_at: impl Fn(i32, i32, i32) -> Option<BlockId>,
    trap: (i32, i32, i32),
) -> Option<(Spot, (i32, i32, i32))> {
    let sides: Vec<(i32, i32, i32)> = [(1, 0), (-1, 0), (0, 1), (0, -1)]
        .iter()
        .map(|&(dx, dz)| (trap.0 + dx, trap.1, trap.2 + dz))
        .filter(|&(x, y, z)| block_at(x, y, z).is_some_and(is_liquid))
        .collect();
    if sides.len() < 2 {
        return None;
    }
    sides
        .into_iter()
        .filter_map(|side| Some((survey(&block_at, side)?, side)))
        .max_by_key(|(spot, _)| spot.cells)
}

/// How much the cold slows the fish: 0.3 in water at two degrees or
/// colder, rising to nothing lost at twelve.
///
/// The air's temperature stands in for the water's, which lags it and is
/// never as cold -- which the floor of 0.3 is for: a winter lake is slow
/// fishing, and it is still fishing, through a hole in the ice if that is
/// what it takes.
pub fn cold_factor(water_c: f32) -> f32 {
    0.3 + 0.7 * ((water_c - 2.0) / 10.0).clamp(0.0, 1.0)
}

/// How the hour moves the fish, for a rod: best at dawn and dusk, ordinary
/// in the day, slowest at night. `time_of_day` is the server's, 0 at
/// midnight and 0.5 at noon.
///
/// **A smooth curve and not three bands**, because a band edge is a minute
/// of the evening where casting a second later is twice as good, and that
/// is a thing to game rather than a thing to plan.
pub fn hour_factor(time_of_day: f32) -> f32 {
    // -1 at midnight, 0 at dawn and dusk, 1 at noon.
    let sun = -(std::f32::consts::TAU * time_of_day).cos();
    let day = ((sun + 0.2) / 0.4).clamp(0.0, 1.0);
    let base = 0.45 + 0.35 * day * day * (3.0 - 2.0 * day);
    let twilight = 0.8 * (-(sun / 0.3).powi(2)).exp();
    base + twilight
}

/// How the sky moves the fish, for a rod: rain brings them up to feed and
/// a storm sends them down.
pub fn weather_factor(weather: Weather) -> f32 {
    match weather {
        Weather::Clear => 1.0,
        Weather::Rain => 1.35,
        Weather::Storm => 0.5,
    }
}

/// How good a place is for a rod, 0 for none: deep, wide water, and the sea
/// best of all.
pub fn rod_place_factor(spot: Spot, kind: Water) -> f32 {
    if !spot.holds_fish() || spot.depth < MIN_ROD_DEPTH {
        return 0.0;
    }
    let depth = 0.4 + 0.6 * f32::from(spot.depth.min(6)) / 6.0;
    let kind = match kind {
        Water::Salt => 1.0,
        Water::Fresh => 0.9,
        Water::Standing => 0.75,
    };
    spot.size_factor() * depth * kind
}

/// How good a place is for a trap, 0 for none: the shallows of running
/// water, with water behind them.
///
/// **The other way round from the rod on purpose.** A weir is built where a
/// stream is shallow enough to wade and funnels whatever comes down it; a
/// basket on the bed of a deep lake is a basket the fish swim over.
pub fn trap_place_factor(spot: Spot, kind: Water) -> f32 {
    if !spot.holds_fish() {
        return 0.0;
    }
    let depth = if spot.depth <= 3 { 1.0 } else { 0.6 };
    let kind = match kind {
        Water::Fresh => 1.0,
        Water::Standing => 0.6,
        Water::Salt => 0.45,
    };
    spot.size_factor() * depth * kind
}

/// What weed and shade are worth: up to a third again as many bites.
///
/// **Small on purpose.** Cover is the one thing about a spot a player can
/// read off the bank without casting, and a big number would make every
/// other thing here -- the hour, the sky, the season, the depth -- noise
/// beside "stand at the reeds". A third is enough to be worth walking round
/// a lake for and not enough to beat fishing at dusk.
pub fn cover_factor(spot: Spot) -> f32 {
    let weeds = f32::from(spot.weeds.min(12)) / 12.0;
    1.0 + 0.22 * weeds + if spot.shade { 0.11 } else { 0.0 }
}

/// How the season moves the fish.
///
/// Spring best -- the fish are feeding up and in the shallows -- summer
/// good, autumn fair, winter poor. **This is not the cold**
/// ([`cold_factor`] is, and it reads the actual air at the actual water):
/// a warm winter pool still fishes badly, because in winter a fish eats
/// once a week whatever the thermometer says. The two multiply, so a
/// northern lake in winter is the worst fishing there is and a southern one
/// is merely slow.
pub fn season_factor(season: Season) -> f32 {
    match season {
        Season::Spring => 1.2,
        Season::Summer => 1.05,
        Season::Autumn => 0.9,
        Season::Winter => 0.6,
    }
}

/// **How many fish a spot gives before it is fished out.** Six.
///
/// A morning's fishing off one bank, and then that bank is tired and the
/// one round the point is not. Small enough that a player notices inside
/// one session -- a number nobody ever reaches is a rule nobody ever meets
/// -- and big enough that catching a meal never turns into moving after
/// every fish.
pub const SPOT_HOLDS: f32 = 6.0;

/// How long a fished-out spot takes to come back to itself: twenty minutes
/// of real time for one fish's worth.
///
/// Two hours to empty of six and twenty minutes to forgive one, so a player
/// who works round a lake and comes back has a spot worth casting at again.
/// **Real seconds and not world days**, because it is the *player's*
/// patience being asked about, and a world whose day is twenty minutes
/// would otherwise make the rule mean something different from one whose
/// day is an hour.
pub const SPOT_RECOVERS_SECONDS: f32 = 20.0 * 60.0;

/// What a spot with `taken` fish already out of it is worth: 1 when it is
/// rested, down to [`FISHED_OUT`] when it has given [`SPOT_HOLDS`].
///
/// **Never nought.** A spot that stopped dead would read as a bug -- the
/// float sits there and nothing ever happens and nothing on the screen says
/// why. At a fifth, the bites go from every half minute to every two and a
/// half, which is the same sentence said in a way the player can feel.
pub const FISHED_OUT: f32 = 0.2;

/// See [`SPOT_HOLDS`].
pub fn pressure_factor(taken: f32) -> f32 {
    let spent = (taken / SPOT_HOLDS).clamp(0.0, 1.0);
    1.0 - (1.0 - FISHED_OUT) * spent
}

/// A spot's `taken` after `seconds` of nobody fishing it.
pub fn pressure_recovered(taken: f32, seconds: f32) -> f32 {
    (taken - seconds / SPOT_RECOVERS_SECONDS).max(0.0)
}

/// How big a patch of water counts as one spot, in blocks.
///
/// Eight, which is a little wider than the survey a cast makes
/// ([`SURVEY_REACH`] each way): two casts that see the same water are the
/// same spot, and a player who walks a dozen paces along the bank is
/// fishing somewhere else. A finer grid would let somebody take six fish
/// out of one pool by shuffling sideways; a coarser one would tire out a
/// whole lake from one corner of it.
pub const SPOT_GRID: i32 = 8;

/// The spot a float is in, for the pressure the server keeps.
pub fn spot_key(float: (i32, i32, i32)) -> (i32, i32, i32) {
    (
        float.0.div_euclid(SPOT_GRID),
        float.1.div_euclid(SPOT_GRID),
        float.2.div_euclid(SPOT_GRID),
    )
}

/// **When a line goes in**: everything about the hour and the sky that a
/// fish feeds by, as one value.
///
/// One value and not five arguments, because two things read it and must
/// read it the same way: the rod's wait ([`rod_wait_seconds`]) and the fish
/// seen rising ([`rise_rate`]). A rise that did not follow the bite would be
/// decoration, and the whole reason the water shows its fish is that a
/// player can read the bite off it.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Moment {
    /// The server's clock: 0 at midnight, 0.5 at noon.
    pub time_of_day: f32,
    pub weather: Weather,
    /// What the sky does next, and in how many seconds -- the server's
    /// `Sky::coming`. `None` where nobody knows: a sky an operator is
    /// holding has no next spell.
    pub coming: Option<(Weather, f32)>,
    /// The air at the water, standing in for the water ([`cold_factor`]).
    pub water_c: f32,
    pub season: Season,
}

impl Moment {
    /// An ordinary spring hour under a clear sky with nothing coming: what a
    /// test holds still when it is asking about something else.
    pub fn plain(time_of_day: f32, weather: Weather, water_c: f32) -> Moment {
        Moment { time_of_day, weather, coming: None, water_c, season: Season::Spring }
    }
}

/// **How hungry the fish are at this moment**, 1 for an ordinary one: the
/// hour, the sky, the sky that is coming, the cold and the season,
/// multiplied.
///
/// The place is not in it: a deep lake and a puddle are equally hungry at
/// dusk, and differ in how many fish there are to be hungry. That split is
/// what lets the rising fish ([`rise_rate`]) carry this number and nothing
/// about the place -- a fish rises where a fish is.
pub fn appetite(moment: Moment) -> f32 {
    hour_factor(moment.time_of_day)
        * weather_factor(moment.weather)
        * front_factor(moment.weather, moment.coming)
        * cold_factor(moment.water_c)
        * season_factor(moment.season)
}

/// **How long before the rain the fish know it is coming**: four minutes.
///
/// A spell of weather is five to twenty minutes (`weather::SPELL_SECONDS`),
/// so four is the tail of a clear spell rather than most of it -- the
/// window is something a player catches or misses, and a player who sees
/// the fish start jumping has time to walk to the water and cast into it.
pub const FRONT_SECONDS: f32 = 240.0;

/// How much better the fish take in the last of those minutes: 1.7, against
/// the rain's own 1.35.
///
/// **Better than the rain itself, on purpose.** It is what anybody who
/// fishes will say, and it is what makes the forecast worth reading: if the
/// best bite were *in* the rain, the thing to do would be to wait for it,
/// and the jumping fish would be telling a player nothing they could use.
pub const FRONT_BEST: f32 = 1.7;

/// The bite before the rain: 1 under any sky with no rain coming, rising
/// smoothly to [`FRONT_BEST`] as a wet spell's start comes within
/// [`FRONT_SECONDS`].
///
/// **Only out of a dry sky.** Rain before more rain is not a front; and the
/// sky never goes wet to wet anyway (`Sky::roll`), so this is also the rule
/// that it cannot be counted twice.
///
/// **A ramp and not a switch**, for the hour's reason ([`hour_factor`]): a
/// minute of the afternoon where casting a second later is half again as
/// good is a thing to game rather than a thing to plan.
///
/// The forecast was not knowable when this was first wanted: the sky rolled
/// its next spell only when the last one ended. It is rolled when a spell
/// *begins* now (the server's `weather::Sky`), from the same odds, so the
/// weather a player gets is drawn exactly as it always was and is merely
/// decided a spell sooner.
pub fn front_factor(now: Weather, coming: Option<(Weather, f32)>) -> f32 {
    let Some((next, in_seconds)) = coming else {
        return 1.0;
    };
    if now.is_wet() || !next.is_wet() {
        return 1.0;
    }
    let near = 1.0 - (in_seconds / FRONT_SECONDS).clamp(0.0, 1.0);
    let eased = near * near * (3.0 - 2.0 * near);
    1.0 + (FRONT_BEST - 1.0) * eased
}

/// How much a bright sun overhead puts the fish off, 1 for none, down to
/// [`GLARE_WORST`] for shallow open water under a clear noon.
///
/// **Noon was an ordinary hour, and the owner asked for it to be a poor
/// one.** It is made poor *here*, as the light on the water, rather than as
/// a dip in [`hour_factor`], because a fish in the sun goes somewhere --
/// deep, or under something -- and that makes noon a question about where
/// to stand rather than a time to stop: the deep hole and the bank under
/// the willow still fish at midday. A dip in the hour would have said "go
/// home until four", which is a chore and not a decision.
///
/// Overcast takes most of it away and a storm all of it: there is no glare
/// under a sky that has none.
pub fn glare_factor(time_of_day: f32, weather: Weather, depth: u8, shade: bool) -> f32 {
    let sun = -(std::f32::consts::TAU * time_of_day).cos();
    // Nothing until the sun is well up, and all of it at noon.
    let high = ((sun - 0.4) / 0.6).clamp(0.0, 1.0);
    let bright = match weather {
        Weather::Clear => 1.0,
        Weather::Rain => 0.3,
        Weather::Storm => 0.0,
    };
    // Six blocks down is out of it; two is in it.
    let exposed = (1.0 - f32::from(depth.saturating_sub(2)) / 4.0).clamp(0.2, 1.0);
    let covered = if shade { 0.35 } else { 1.0 };
    1.0 - (1.0 - GLARE_WORST) * high * high * bright * exposed * covered
}

/// The worst the sun does: a bite half as often.
pub const GLARE_WORST: f32 = 0.5;

/// **A hole in the ice draws the fish**: half again as many bites where the
/// water the float went down into lies under a lid.
///
/// The fish under the ice come to where the light and the air are, which is
/// why people cut holes in lakes at all -- and it is what makes a winter
/// lake a decision rather than a closed door. Winter is the slowest season
/// there is ([`season_factor`]) and cold water slower still; a hole in the
/// middle of a frozen lake gives back some of it, where the open river mouth
/// beside it does not. The hole itself is the player's: the float goes down
/// one with a line lowered rather than thrown ([`cast_target`]).
pub fn ice_factor(spot: Spot) -> f32 {
    if spot.iced >= ICED_LID {
        1.5
    } else {
        1.0
    }
}

/// How many of the surveyed cells must lie under ice before it is a lid and
/// not a sliver at the shore: eight, a little more than the ring round a
/// hole one block across.
pub const ICED_LID: u16 = 8;

/// **Fish the player can see are fish the player can catch**: up to half
/// again as many bites with a shoal swimming within [`SHOAL_REACH`] of the
/// float.
///
/// The one term here that is about the fish that are actually *there* -- the
/// ones the server has swimming, which a player sees as shapes in the
/// shallows and as rings on the water ([`rise_rate`]). Without it, casting
/// at the rise would be a superstition: the rod reads the place, and the
/// rise would be a picture of the same place. With it, the eye is worth
/// something, and what it is worth is capped below the hour's, for the
/// reason [`cover_factor`] gives -- a shoal at noon does not beat open water
/// at dusk.
pub fn shoal_factor(fish_near: usize) -> f32 {
    1.0 + 0.5 * (fish_near.min(SHOAL_FULL) as f32 / SHOAL_FULL as f32)
}

/// How far from the float a fish counts as at it, in blocks.
pub const SHOAL_REACH: f32 = 6.0;

/// How many fish at the float are a shoal; more are not more bites.
pub const SHOAL_FULL: usize = 4;

/// The average wait for a bite, in seconds, or `None` where nothing will
/// ever bite -- the puddle, the float on the bottom, dry land.
///
/// The server rolls the actual wait round this (an exponential, so a cast
/// has no memory: casting again does not hurry anything, and sitting on a
/// cast does not either).
///
/// **Everything the place is, multiplied.** The water itself
/// ([`rod_place_factor`]: how much, how deep, salt or fresh or still), what
/// grows in it ([`cover_factor`]), what the sun is doing to it
/// ([`glare_factor`]), whether it is under ice ([`ice_factor`]), how hungry
/// the fish are ([`appetite`]), how hard it has been fished lately
/// ([`pressure_factor`]), whether a shoal is at the float ([`shoal_factor`])
/// and what is on the hook ([`Bait::appeal`]). Multiplied rather than added
/// because they are independent reasons a fish is or is not feeding, and
/// because a sum lets one good term carry six bad ones: a puddle at dusk in
/// spring in the rain is still a puddle.
pub fn rod_wait_seconds(
    spot: Spot,
    kind: Water,
    moment: Moment,
    taken: f32,
    fish_near: usize,
    bait: Option<Bait>,
) -> Option<f32> {
    let place = rod_place_factor(spot, kind);
    if place <= 0.0 {
        return None;
    }
    let pace = place
        * cover_factor(spot)
        * glare_factor(moment.time_of_day, moment.weather, spot.depth, spot.shade)
        * ice_factor(spot)
        * appetite(moment)
        * pressure_factor(taken)
        * shoal_factor(fish_near)
        * bait.map_or(BARE_HOOK, Bait::appeal);
    Some((ROD_TYPICAL_SECONDS / pace).min(ROD_LONGEST_SECONDS))
}

/// The chance a fish goes into a trap in one step of the rot clock.
pub fn trap_chance(spot: Spot, kind: Water, water_c: f32) -> f32 {
    (TRAP_BEST_CHANCE * trap_place_factor(spot, kind) * cold_factor(water_c)).clamp(0.0, 1.0)
}

/// **What a bare hook is worth**: about a third of a worm.
///
/// The number that decides whether bait is a choice or a chore. At nought
/// the rod would not work without a worm, and digging one would be a tax on
/// fishing rather than a decision about it; at one, bait would be a thing to
/// ignore. A third means a player who has not dug anything still catches --
/// slowly, and only what a bare hook catches ([`Bait::tempts`] against
/// `None`) -- and a player who spent a minute in the dirt catches three
/// times as fast.
pub const BARE_HOOK: f32 = 0.35;

/// What is on the hook.
///
/// **Six, and each one says a different sentence about where the player has
/// been.** A worm is dug anywhere there is soil; a grub comes out of the
/// meadow with the fibre; a berry and a crust are food a player chose not to
/// eat; a scrap of meat is the hunt paying for the fishing; a fly of feather
/// and cord is the only one that is *made*, and the only one that is not
/// eaten off the hook.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Bait {
    /// Dug out of dirt or a dung heap. The everyday bait.
    Worm,
    /// An insect out of the grass. What a trout is already eating.
    Grub,
    /// A berry. Free, and the fish know it.
    Berry,
    /// A crust of bread. Costs a field.
    Bread,
    /// A scrap of raw meat: what the fish that eat fish want.
    Scrap,
    /// Feather and cord, tied to look alive. Not eaten -- see
    /// [`Bait::keeps`].
    Fly,
}

impl Bait {
    /// Every bait, in the order a hook meets them.
    pub const ALL: [Bait; 6] = [Bait::Worm, Bait::Grub, Bait::Berry, Bait::Bread, Bait::Scrap, Bait::Fly];

    /// The item this bait is, in the pack.
    pub fn block(self) -> BlockId {
        use crate::types::{BLOCK_BERRIES, BLOCK_BREAD, BLOCK_FISHING_FLY, BLOCK_GRUB, BLOCK_RAW_MEAT, BLOCK_WORM};
        match self {
            Bait::Worm => BLOCK_WORM,
            Bait::Grub => BLOCK_GRUB,
            Bait::Berry => BLOCK_BERRIES,
            Bait::Bread => BLOCK_BREAD,
            Bait::Scrap => BLOCK_RAW_MEAT,
            Bait::Fly => BLOCK_FISHING_FLY,
        }
    }

    /// The bait an item is, if it is bait at all.
    ///
    /// **By kind** (`types::block_kind`), so a half-rotten scrap of meat is
    /// still a scrap of meat to a pike. A fish does not read the variant.
    pub fn of_block(id: BlockId) -> Option<Bait> {
        let kind = crate::types::block_kind(id);
        Bait::ALL.into_iter().find(|bait| crate::types::block_kind(bait.block()) == kind)
    }

    /// Is this bait still on the hook after a fish has taken it?
    ///
    /// Only the fly. **A worm is eaten and a fly is not**, which is the
    /// whole argument for making one: it costs feathers, a length of cord
    /// and a crafting step, and it pays that back over many fish instead of
    /// over one.
    ///
    /// **What stops it being the last bait anybody needs is the broken
    /// line**: a fly goes with the fish when the line parts ([`Fought::Lost`]),
    /// where a worm was eaten anyway. So the fly is the bait of a player who
    /// fights well, and a careless one pays for it -- and it is still only
    /// the trout's bait ([`Bait::tempts`]), so the lake and the sea are not
    /// solved by it. A durability number on the fly was the other way to say
    /// this and says it worse: it would tick down on a good day's fishing
    /// exactly as fast as on a bad one.
    pub fn keeps(self) -> bool {
        matches!(self, Bait::Fly)
    }

    /// How much faster a bite comes with this on the hook, against
    /// [`BARE_HOOK`]'s third.
    ///
    /// **Appeal and [`Bait::tempts`] are two different questions**, and
    /// keeping them apart is what makes bait a decision. Appeal is how often
    /// *anything* comes; tempts is *what*. A grub brings bites constantly
    /// and a pike will not look at one, so a grub in a pike lake is a busy
    /// float and small fish.
    pub fn appeal(self) -> f32 {
        match self {
            Bait::Worm => 1.0,
            Bait::Grub => 1.15,
            // A berry is bait because it floats past and something grabs it,
            // and that is as much as can honestly be claimed for it.
            Bait::Berry => 0.5,
            Bait::Bread => 0.8,
            // Meat is not subtle. What it brings is fewer fish and bigger
            // ones -- see `tempts`.
            Bait::Scrap => 0.7,
            Bait::Fly => 0.9,
        }
    }

    /// How much a fish of this kind wants this bait: 0 for one that will not
    /// take it at all.
    ///
    /// **The table is the mechanic.** A trout eats insects, so a grub or a
    /// fly is what takes one and a lump of meat is not; a pike and a cod eat
    /// fish, so they come to meat and ignore a berry; the school that lives
    /// in every water eats whatever goes past. So the question "what do I
    /// put on the hook" has a different answer at the river, at the lake and
    /// at the sea, which is the same question the rod already asks about
    /// *where* -- asked again in the pack.
    ///
    /// A species nobody wrote a line for (one added after this table) is
    /// taken as an ordinary fish: it eats what the school eats. **A
    /// catch-all rather than an exhaustive match**, deliberately, because
    /// this crate's fish are added by the spawner and a new one must not
    /// stop the build here before anybody has decided what it eats.
    pub fn tempts(bait: Option<Bait>, species: Species) -> f32 {
        // What a bare hook takes: the school, and not much of it. Something
        // shiny in the water, taken by the fish that takes everything.
        let Some(bait) = bait else {
            return match species {
                Species::Fish => 1.0,
                Species::Herring => 0.3,
                _ => 0.0,
            };
        };
        match (bait, species) {
            (Bait::Worm, Species::Fish) => 1.0,
            (Bait::Worm, Species::Trout) => 1.0,
            (Bait::Worm, Species::Herring) => 0.7,
            (Bait::Worm, Species::Pike) => 0.5,
            (Bait::Worm, Species::Cod) => 0.4,
            (Bait::Grub, Species::Trout) => 1.6,
            (Bait::Grub, Species::Fish) => 0.9,
            (Bait::Grub, Species::Herring) => 0.6,
            (Bait::Grub, Species::Pike) => 0.2,
            (Bait::Grub, Species::Cod) => 0.2,
            (Bait::Berry, Species::Fish) => 0.8,
            (Bait::Berry, Species::Trout) => 0.3,
            (Bait::Berry, Species::Herring) => 0.2,
            // A pike does not eat fruit, and neither does a cod.
            (Bait::Berry, _) => 0.0,
            (Bait::Bread, Species::Fish) => 1.0,
            (Bait::Bread, Species::Herring) => 0.9,
            (Bait::Bread, Species::Trout) => 0.4,
            (Bait::Bread, Species::Pike) => 0.1,
            (Bait::Bread, Species::Cod) => 0.3,
            (Bait::Scrap, Species::Pike) => 1.8,
            (Bait::Scrap, Species::Cod) => 1.6,
            (Bait::Scrap, Species::Fish) => 0.4,
            (Bait::Scrap, Species::Trout) => 0.3,
            (Bait::Scrap, Species::Herring) => 0.1,
            (Bait::Fly, Species::Trout) => 1.8,
            (Bait::Fly, Species::Herring) => 0.7,
            (Bait::Fly, Species::Fish) => 0.6,
            (Bait::Fly, Species::Pike) => 0.3,
            (Bait::Fly, Species::Cod) => 0.2,
            // Anything the spawner grew since this table was written.
            (_, _) => 0.6,
        }
    }
}

/// How deep water has to be before a fish of this kind is in it.
///
/// **A copy of the spawner's `logic::animals::needs_depth`**, and it has to
/// be: the rod must not land a pike out of a water no pike could have
/// spawned in, and this crate cannot see the server's. The two numbers that
/// matter are already public here for the same reason ([`COD_DEPTH`],
/// [`PIKE_DEPTH`]); this is those two and the two blocks everything else
/// swims in.
pub fn wants_depth(species: Species) -> u8 {
    match species {
        Species::Cod => COD_DEPTH,
        Species::Pike => PIKE_DEPTH,
        _ => MIN_ROD_DEPTH,
    }
}

/// **What the water is, to a fish**: salt, running or still, how cold, and
/// what is suspended in it.
///
/// `chill` and `silt` are `worldgen::water_climate_of`'s -- the two numbers
/// the water is *coloured* by -- read here rather than a second climate
/// invented for the fish, so **the colour of the water is the forecast of
/// the catch**: the milky turquoise of a glacier tarn, the brown of a bog
/// and the clear blue of a meadow lake are three different rods, and a
/// player learns it by looking before they cast.
///
/// Three ways this was weighed:
///
/// * *The biome alone*, which is what there was: `Species::lives_in` says a
///   trout is in the taiga and a pike in the plains. That is still the gate
///   -- the rod must never land a fish the spawner could not have put there
///   -- and it is a line across a map: a lake on the border reads as one
///   country or the other, and a tarn high in the plains is warm.
/// * *The air temperature at the float* (`climate::Ambient`), which the
///   cold already reads ([`cold_factor`]). It moves with the hour and the
///   season, and a trout that left the lake every afternoon would make the
///   catch a clock rather than a place.
/// * *This*: the water's own climate, which is a property of the place, is
///   smooth across a border, and is already on the screen.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Habitat {
    pub kind: Water,
    /// 0 warm, 1 as cold as water gets. See `worldgen::water_climate_of`.
    pub chill: f32,
    /// 0 peat, a half clear, 1 rock flour.
    pub silt: f32,
}

impl Habitat {
    /// Clear water, neither warm nor cold: what a test holds still.
    pub fn plain(kind: Water) -> Habitat {
        Habitat { kind, chill: 0.5, silt: 0.5 }
    }

    /// How brown it is, 0..1: 1 is a bog.
    pub fn peat(self) -> f32 {
        ((0.5 - self.silt) / 0.5).clamp(0.0, 1.0)
    }

    /// How milky it is, 0..1: 1 is melt straight off a glacier.
    pub fn flour(self) -> f32 {
        ((self.silt - 0.5) / 0.5).clamp(0.0, 1.0)
    }
}

/// How much more or less of this fish there is in this water than its
/// country alone would say: a multiplier on the spawner's weight.
///
/// **A weight and never a gate.** Nothing here is nought, so a fish that
/// lives in a country is catchable in every water of it -- a trout in a warm
/// brown pond is rare, not impossible. Gates are `Species::lives_in` and
/// the depth ([`wants_depth`]), which the spawner reads as well; a gate here
/// would be a fish the player can see swimming and never hook.
///
/// The rows are the natural history of the four fish, as far as it
/// changes a decision:
///
/// * **The school** is everywhere and likes it warm and a little brown --
///   the roach of a reedy pond -- and thins in the milk off a glacier, where
///   there is little to eat.
/// * **The trout** is cold, clear, running water: the colder the more of
///   it, the browner the less. Rock flour does not trouble it -- a glacier
///   stream is a trout stream.
/// * **The pike** is the other corner: warm, still, and at home in the
///   peat of a marsh, where it lies in the weed.
/// * **The herring and the cod** are the cold sea's: the further north the
///   coast, the more of them.
pub fn habitat_weight(species: Species, habitat: Habitat) -> f32 {
    let (chill, peat, flour) = (habitat.chill.clamp(0.0, 1.0), habitat.peat(), habitat.flour());
    let still = habitat.kind == Water::Standing;
    match species {
        Species::Fish => (1.0 - 0.4 * chill) * (1.0 + 0.3 * peat) * (1.0 - 0.5 * flour),
        Species::Trout => {
            (0.2 + 1.4 * chill) * (1.0 - 0.8 * peat) * (1.0 + 0.3 * flour) * if still { 0.8 } else { 1.2 }
        }
        Species::Pike => (1.4 - 1.0 * chill) * (1.0 + 0.5 * peat) * (1.0 - 0.7 * flour) * if still { 1.2 } else { 0.9 },
        Species::Herring => 0.6 + 0.8 * chill,
        Species::Cod => 0.4 + 1.2 * chill,
        // A fish added after this table: as its country says, no more.
        _ => 1.0,
    }
}

/// Every fish that could be in this water, and how much of the catch each
/// one is, given what is on the hook.
///
/// The weight is the spawner's own (`Species::spawn_weight`) times how much
/// that fish likes this water ([`habitat_weight`]) times how much it wants
/// the bait, so **the rod agrees with the water**: what is on the end of the
/// line is drawn from the same fish the swimmers in that lake are drawn
/// from, biased by the water's climate and by the hook. A fish that will not
/// take this bait weighs nothing and cannot be caught at all.
pub fn rod_odds(biome: Biome, spot: Spot, habitat: Habitat, bait: Option<Bait>) -> Vec<(Species, f32)> {
    Species::ALL
        .iter()
        .copied()
        .filter(|&species| species.swims() && species.lives_in(biome) && wants_depth(species) <= spot.depth)
        .map(|species| {
            let weight = species.spawn_weight(false) as f32 * habitat_weight(species, habitat) * Bait::tempts(bait, species);
            (species, weight)
        })
        .filter(|&(_, weight)| weight > 0.0)
        .collect()
}

/// Which fish took the hook, given a roll in `0..1`: `None` where nothing in
/// this water wants what is on it.
///
/// How much it is worth is [`catch_count`]: the fish's own `Species::drops`,
/// moved by its [`Size`].
pub fn rod_species(biome: Biome, spot: Spot, habitat: Habitat, bait: Option<Bait>, roll: f32) -> Option<Species> {
    let odds = rod_odds(biome, spot, habitat, bait);
    let total: f32 = odds.iter().map(|&(_, weight)| weight).sum();
    if total <= 0.0 {
        return None;
    }
    let mut want = roll.clamp(0.0, 1.0) * total;
    let mut last = None;
    for (species, weight) in odds {
        want -= weight;
        last = Some(species);
        if want <= 0.0 {
            return Some(species);
        }
    }
    last
}

/// **How big the fish on the hook is.** Three sizes and not a length in
/// centimetres, because what a size has to do is change two things a player
/// feels -- how many fish go into the pack ([`catch_count`]) and how hard
/// it fights ([`Size::pull`], [`Size::heft`]) -- and three steps of each are
/// steps a player can tell apart. A continuous length would be a number the
/// game has nowhere to show.
///
/// **Rejected: a size item** (a "big pike" in the pack). The module note's
/// argument against species as items is the same argument, doubled: every
/// size of every fish would want its cooked, salted and dried row. The size
/// is how *many* fish, which the salt barrel already understands.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Size {
    Small,
    Middling,
    Big,
}

impl Size {
    pub const ALL: [Size; 3] = [Size::Small, Size::Middling, Size::Big];

    /// The byte on the wire (`protocol::ServerMessage::Caught`).
    pub fn code(self) -> u8 {
        self as u8
    }

    /// A byte off the wire. Anything unknown is middling: a newer server's
    /// size read as the ordinary one is a sentence slightly wrong, never a
    /// fish that is not there.
    pub fn of_code(code: u8) -> Size {
        match code {
            0 => Size::Small,
            2 => Size::Big,
            _ => Size::Middling,
        }
    }

    /// How much harder the line takes it, against the species' own pull.
    pub fn pull(self) -> f32 {
        match self {
            Size::Small => 0.8,
            Size::Middling => 1.0,
            Size::Big => 1.3,
        }
    }

    /// How much longer it takes to bring in.
    ///
    /// **Heft is most of what size costs, and pull the least of it.** A big
    /// fish that broke lines three times as readily as a middling one would
    /// make the big ones a matter of luck -- the careful player loses them as
    /// often as the careless one -- where a big fish that takes twice as
    /// long gives the careful player twice as many runs to get right, which
    /// is where the skill is.
    pub fn heft(self) -> f32 {
        match self {
            Size::Small => 0.75,
            Size::Middling => 1.0,
            Size::Big => 1.5,
        }
    }
}

/// How likely each size is, small to big, for a fish taking in this water.
///
/// **Four things make a fish big, and each is a thing a player chooses:**
///
/// * *Room.* A deep wide water grows big fish and a dug pond does not
///   ([`Spot::size_factor`], depth). The lake is a walk; the pond is home.
/// * *Peat stunts.* A bog is full of small fish (the brown of
///   [`Habitat::peat`]): quick bites, little meat.
/// * *The big ones go first.* A fished bank loses its big fish before it
///   loses its bites ([`pressure_factor`] on the big share), so moving on
///   is a decision about *what* is caught as well as how soon.
/// * *A big bait turns small fish away.* Meat halves the small ones and
///   doubles the big; a grub or a berry is the other way round. With
///   [`Bait::appeal`] that is the whole trade: a scrap on the hook is fewer
///   bites of heavier fish.
pub fn size_odds(spot: Spot, habitat: Habitat, taken: f32, bait: Option<Bait>) -> [f32; 3] {
    let room = ((spot.size_factor() - 0.35) / 0.65).clamp(0.0, 1.0) * (f32::from(spot.depth.min(5)) / 5.0);
    let peat = habitat.peat();
    let mut small = 0.35 * (1.4 - 0.6 * room) * (1.0 + 0.9 * peat);
    let middling = 0.5;
    let mut big = 0.15 * (0.4 + 1.6 * room) * (1.0 - 0.65 * peat) * pressure_factor(taken);
    let (small_bait, big_bait) = match bait {
        Some(Bait::Scrap) => (0.35, 2.2),
        Some(Bait::Grub) | Some(Bait::Berry) => (1.3, 0.5),
        Some(Bait::Fly) => (1.0, 0.8),
        Some(Bait::Worm) | Some(Bait::Bread) => (1.0, 1.0),
        // A bare hook: whatever small thing grabs at something shiny.
        None => (1.3, 0.4),
    };
    small *= small_bait;
    big *= big_bait;
    let total = small + middling + big;
    [small / total, middling / total, big / total]
}

/// Which size came up, from [`size_odds`] and a roll in `0..1`.
pub fn roll_size(odds: [f32; 3], roll: f32) -> Size {
    let roll = roll.clamp(0.0, 1.0);
    if roll < odds[0] {
        Size::Small
    } else if roll < odds[0] + odds[1] {
        Size::Middling
    } else {
        Size::Big
    }
}

/// How many raw fish a fish of this kind and size is.
///
/// Off `Species::drops` -- one for the school and the herring, two for a
/// trout, three for a pike or a cod -- so the species' own worth stays in
/// one table, and the size moves it: a small one is one less (never
/// nothing), a big one half as much again. A big pike is five fish, which
/// is a day's food and the best thing a rod does.
pub fn catch_count(species: Species, size: Size) -> u32 {
    let base: u32 = species
        .drops()
        .iter()
        .filter(|&&(block, _)| block == crate::types::BLOCK_RAW_FISH)
        .map(|&(_, n)| n)
        .sum::<u32>()
        .max(1);
    match size {
        Size::Small => base.saturating_sub(1).max(1),
        Size::Middling => base,
        Size::Big => base + base.div_ceil(2),
    }
}

// **What `rod_catch` was, and why it is gone.** It answered "how many raw
// fish" from the water alone -- three off a deep sea, three from a deep
// still lake, two from a river -- because nothing then chose a *species*
// for a catch. `rod_species` chooses one now, out of the same fish the
// spawner puts in that water, and the count is that fish's own
// `Species::drops`, moved by its size (`catch_count`). One table instead
// of two that had to be kept saying the same thing; the water that once
// set the count sets the size now (`size_odds`), which is the same idea
// said about the fish instead of about the pack.

// ---------------------------------------------------------------------------
// The throw
// ---------------------------------------------------------------------------

/// How long the rod is held back for a throw at full strength: a second and
/// a quarter.
///
/// Long enough that a player feels themselves winding up and short enough
/// that fishing a bank of twenty spots is not twenty seconds of holding a
/// button. The same shape as drawing anything back: the hold is the power.
pub const CAST_CHARGE_SECONDS: f32 = 1.25;

/// The shortest throw there is, in blocks from the eye: a flick over the
/// reeds at your feet.
pub const CAST_NEAREST: f32 = 2.5;

/// The longest: twelve blocks.
///
/// **Why the leash is bigger than this** ([`CAST_HOLDS`]): a player who has
/// cast to the far edge of it has to be able to take a step back up the
/// bank without the line going slack.
pub const CAST_FARTHEST: f32 = 12.0;

/// The power of a throw held for `seconds`, in `0..1`.
///
/// **Squared, not straight.** A linear hold spends most of its travel in
/// distances nobody wants: the useful throws are the short ones, and a
/// player aiming at a gap in the lilies eight blocks out wants fine control
/// at the near end and does not care whether the far end is eleven blocks or
/// twelve.
pub fn cast_power(seconds: f32) -> f32 {
    let held = (seconds / CAST_CHARGE_SECONDS).clamp(0.0, 1.0);
    held * held
}

/// How far a throw of this power carries, in blocks.
pub fn cast_distance(power: f32) -> f32 {
    CAST_NEAREST + (CAST_FARTHEST - CAST_NEAREST) * power.clamp(0.0, 1.0)
}

/// How finely the throw's arc is walked, in blocks. A sixth of a block: fine
/// enough that a line never steps over a fence post, coarse enough that the
/// longest throw is seventy-odd block reads.
const ARC_STEP: f32 = 1.0 / 6.0;

/// Where a throw from `eye`, aimed along `dir`, with this much power, comes
/// down: the cell of water it lands in, or `None` if it hits something
/// first or falls out of the world the caster has.
///
/// **A real arc, walked, rather than a point projected onto the water.**
/// Three shapes were weighed:
///
/// * *The aimed cell* -- ray from the eye to the first water it meets, which
///   is what the old cast was. It cannot throw past anything, so a bank of
///   reeds between the player and the lake made the lake unfishable, and the
///   power had nothing to do.
/// * *A ballistic flight with an entity in the air.* Honest, and it wants a
///   projectile, a tick loop and a wire message a frame. The float lands in
///   a tenth of a second; nobody sees the arc but the client, which can draw
///   one without the server simulating it.
/// * *This*: a parabola walked here, in one call, by both sides from the same
///   numbers. The server places the float, the client draws the flight, and
///   they cannot disagree because there is nothing to disagree about.
///
/// **The aim is the bearing and the hold is the distance**, which is the
/// division that makes the throw learnable. A rod is thrown over the
/// shoulder at whatever angle a fisher throws it at; what they choose is
/// which way and how hard. So the arc leaves at a fixed [`CAST_ELEVATION`]
/// and the pitch only decides where along the bank it is pointed -- which
/// also means a player cannot get a longer cast by looking up, and does not
/// have to keep a strange angle to get a short one.
///
/// Straight up has no bearing at all, and is no cast.
///
/// **Looking steeply down is a line lowered, not thrown** ([`lowered_line`]):
/// the float goes into the water the eye is on, within [`LOWER_REACH`], or
/// nowhere. That is how a hole in the ice is fished, and a jetty, and the
/// side of a raft -- every place where the water is at the player's feet
/// and a throw at twenty degrees would carry the float over it.
pub fn cast_target(
    eye: (f32, f32, f32),
    dir: (f32, f32, f32),
    power: f32,
    block_at: impl Fn(i32, i32, i32) -> Option<BlockId>,
) -> Option<(i32, i32, i32)> {
    let length = (dir.0 * dir.0 + dir.1 * dir.1 + dir.2 * dir.2).sqrt();
    if length > 1e-6 && dir.1 / length < -LOWER_BELOW {
        return lowered_line(eye, (dir.0 / length, dir.1 / length, dir.2 / length), &block_at);
    }
    let flat = (dir.0 * dir.0 + dir.2 * dir.2).sqrt();
    if flat < 1e-3 {
        return None;
    }
    let range = cast_distance(power);
    // The speed that carries `range` on the flat at this elevation, from the
    // schoolroom formula: range = v^2 * sin(2a) / g. Landing a block or two
    // below the eye carries it a little further than that, which is the
    // right way round -- a cast from a bank reaches.
    let (sin, cos) = CAST_ELEVATION.sin_cos();
    let speed = (range * GRAVITY / (2.0 * sin * cos)).sqrt();
    let velocity = (
        dir.0 / flat * speed * cos,
        speed * sin,
        dir.2 / flat * speed * cos,
    );
    // Up, over and down again, and a little further in case the water is
    // below the bank: the walk ends at the first thing it meets anyway.
    let flight = 2.0 * velocity.1 / GRAVITY + 1.0;
    let steps = ((flight * speed / ARC_STEP) as i32).clamp(16, 600);
    let dt = flight / steps as f32;
    let mut cell = (eye.0.floor() as i32, eye.1.floor() as i32, eye.2.floor() as i32);
    for step in 0..steps {
        let t = dt * step as f32;
        let at = (
            eye.0 + velocity.0 * t,
            eye.1 + velocity.1 * t - 0.5 * GRAVITY * t * t,
            eye.2 + velocity.2 * t,
        );
        let next = (at.0.floor() as i32, at.1.floor() as i32, at.2.floor() as i32);
        if next == cell {
            continue;
        }
        cell = next;
        match block_at(cell.0, cell.1, cell.2) {
            // Water: this is where the float sits.
            Some(block) if is_liquid(block) => return Some(cell),
            // Air, and anything a line flies through (a tuft of grass, a
            // hanging leaf): keep going.
            Some(block) if !crate::types::is_collidable(block) => continue,
            // A wall, a trunk, the far bank: the line is against it, and
            // there is no cast.
            Some(_) => return None,
            // A chunk this side has not got. Refusing is the survey's rule
            // (an unloaded cell is not water), and it only ever means a
            // cast the player makes again a second later.
            None => return None,
        }
    }
    None
}

/// How steeply down a player must be looking for a cast to be a line
/// lowered: the sine of fifty-five degrees below level.
///
/// Steep enough that nobody lowers a line by accident while aiming a throw
/// at the near bank (a throw at the water three blocks out from a bank is
/// looked at twenty or thirty degrees down), and shallow enough that a hole
/// in the ice a pace in front of the feet is reached without staring at
/// one's boots.
pub const LOWER_BELOW: f32 = 0.82;

/// How far a lowered line reaches from the eye, in blocks: the length of
/// the rod and an arm, which is the hole at your feet and the water under a
/// jetty one block down.
pub const LOWER_REACH: f32 = 4.5;

/// The water under the eye, along the look, within [`LOWER_REACH`] -- or
/// `None` if anything solid, or nothing loaded, is met first.
///
/// **A ray and not an arc**, because nothing is thrown: the float goes down
/// the line of sight on the end of the rod. Air and whatever a line hangs
/// through (a tuft of grass, a lily's leaf) is passed; a wall is not, and
/// ice is a wall -- the hole is the point.
pub fn lowered_line(
    eye: (f32, f32, f32),
    dir: (f32, f32, f32),
    block_at: impl Fn(i32, i32, i32) -> Option<BlockId>,
) -> Option<(i32, i32, i32)> {
    let steps = (LOWER_REACH / ARC_STEP) as i32;
    let mut cell = (eye.0.floor() as i32, eye.1.floor() as i32, eye.2.floor() as i32);
    for step in 1..=steps {
        let t = step as f32 * ARC_STEP;
        let next = (
            (eye.0 + dir.0 * t).floor() as i32,
            (eye.1 + dir.1 * t).floor() as i32,
            (eye.2 + dir.2 * t).floor() as i32,
        );
        if next == cell {
            continue;
        }
        cell = next;
        match block_at(cell.0, cell.1, cell.2) {
            Some(block) if is_liquid(block) => return Some(cell),
            Some(block) if !crate::types::is_collidable(block) => continue,
            _ => return None,
        }
    }
    None
}

/// The world's own gravity, in blocks a second squared: what the arc falls
/// at.
const GRAVITY: f32 = 18.0;

/// How high the rod throws, in radians above the horizontal: twenty degrees.
///
/// Low and flat, the way a bait is actually cast -- a lob would put the
/// float in the air for two seconds and make every cast a wait. It is also
/// what keeps the arc under a bank a player is standing beside, instead of
/// throwing the line into the cliff behind them.
const CAST_ELEVATION: f32 = 0.35;

// ---------------------------------------------------------------------------
// The bite, the strike and the fight
// ---------------------------------------------------------------------------

/// **How a fish takes the bait**, as the float shows it: three movements, in
/// order, each with its own length in seconds.
///
/// * `nibble` -- **the float trembles**: a fish is nosing the bait and has
///   not got it. A strike now pulls the bait out of its mouth
///   ([`Strike::TooEarly`]): the fish is gone and the cast with it.
/// * `dip` -- **half under**: it has the bait in its mouth.
/// * `take` -- **under, and moving off**: it has turned with it. After this
///   the bait is eaten and the fish gone.
///
/// A strike in the dip or the take hooks the fish; the window is the two
/// together ([`Bite::window`]).
///
/// **Why three and not the one dip there was.** One dip of nine tenths of a
/// second made the strike a reflex -- the float goes, the hand goes -- and a
/// reflex is the same for everybody who has one. Three movements make it a
/// *reading*: the tremble is information (something is here, get ready),
/// the dip is the answer, and the one mistake a patient player never makes
/// is the one an impatient player always does. That is the owner's
/// "читаемо и справедливо".
///
/// **Why the window is long.** A dip and a take together are never under
/// one and a half seconds (`the_strike_window_is_fair_for_every_fish`),
/// which is a hundred and fifty milliseconds of network and a slow hand with
/// a second to spare -- the float is drawn when the message arrives, so the
/// window a player sees is the window they get. The difficulty is not in
/// the hand, and never was: it is in not striking at the tremble, and then
/// in the fight.
///
/// **Different for each fish, so the float says what is down there.** The
/// school pecks for a long time and takes quickly; a trout is all at once; a
/// pike barely touches it and then goes, and keeps going. A player who has
/// watched a float for an evening knows a pike before the strike.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Bite {
    pub nibble: f32,
    pub dip: f32,
    pub take: f32,
    /// The chance a tremble comes to nothing: the fish noses the bait and
    /// swims off, and the float goes still again.
    ///
    /// **This is what makes the tremble worth reading.** If every tremble
    /// became a dip, a player could strike on a count of two from the first
    /// shiver; with a third of them leading nowhere, the only safe strike is
    /// at the dip, which is the sentence the float was written to say.
    pub nothing: f32,
}

impl Bite {
    /// How long a strike hooks the fish for: the dip and the take.
    pub fn window(self) -> f32 {
        self.dip + self.take
    }
}

/// How this fish takes a bait. See [`Bite`].
pub fn bite_of(species: Species) -> Bite {
    match species {
        // The school pecks and pecks, and then it is quick.
        Species::Fish | Species::Herring => Bite { nibble: 2.2, dip: 0.8, take: 0.9, nothing: 0.4 },
        // A trout rises to it and has it: little warning, a short take.
        Species::Trout => Bite { nibble: 0.9, dip: 0.6, take: 0.9, nothing: 0.25 },
        // A pike hardly touches it, then runs with it for a long time.
        Species::Pike => Bite { nibble: 0.7, dip: 1.0, take: 1.6, nothing: 0.15 },
        Species::Cod => Bite { nibble: 1.3, dip: 0.9, take: 1.2, nothing: 0.2 },
        _ => Bite { nibble: 1.5, dip: 0.8, take: 1.0, nothing: 0.3 },
    }
}

/// How long the float rides after it lands before anything can take it:
/// three quarters of a second of it settling.
///
/// A float still landing has not been taken, and a strike in this time is a
/// player striking at their own splash. Also the floor the wait is rolled
/// against ([`ROD_SHORTEST_SECONDS`]).
pub const SETTLE_SECONDS: f32 = 0.75;

/// What the server did with a strike.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Strike {
    /// The float was down and the hook went in. The fight starts.
    Hooked,
    /// Nothing had the bait -- the float was still, or only trembling. The
    /// line comes out of the water and the cast is over, **which is the
    /// cost that makes the strike a decision** and not a key to hold down.
    /// The bait is not lost: nothing ate it.
    TooEarly,
    /// The line was not in the water at all, or the fight is already on.
    /// Nothing happens and nothing is said; a doubled tap is not a mistake
    /// worth a sentence.
    NotFishing,
}

/// How hard the fish pulls, by species: what the strain rises by while the
/// player is reeling and it is running.
///
/// Off the fish's own length (`Species::length`-ish, via its spawn shape)
/// would be neater and is not available here without pulling the model in;
/// the three numbers are what the three sizes of fish in this game are worth
/// and a new species gets the middle one.
pub fn fight_strength(species: Species) -> f32 {
    match species {
        // The school and the herring: barely a fight at all, and that is
        // right -- the first fish a player ever hooks should come in.
        Species::Fish | Species::Herring => 0.45,
        Species::Trout => 0.75,
        Species::Pike => 1.15,
        // The cod is the deep sea's prize and the one that breaks lines.
        Species::Cod => 1.3,
        _ => 0.8,
    }
}

/// A fish on the hook.
///
/// **One number the player is fighting and one they are winning**: the
/// strain on the line, and how much of the fish is in. Reeling pulls the
/// fish in and puts strain on; letting go sheds strain and lets the fish
/// take a little back. The fish runs in bursts, and a run while reeling is
/// what parts a line -- so the skill is letting go when it runs, which is
/// the actual skill in the actual thing.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Fight {
    /// How hard its kind pulls ([`fight_strength`]).
    pub strength: f32,
    /// How much harder this one pulls for its size ([`Size::pull`]).
    pub pull: f32,
    /// How long it takes to bring in, against a middling fish of its kind
    /// ([`Size::heft`]).
    pub heft: f32,
    /// The strain at which the line parts: 1 on a sound rod, less on a worn
    /// one ([`line_limit`]).
    pub limit: f32,
    /// 0 when it is out at the float, 1 when it is at the bank.
    pub gained: f32,
    /// 0 for a slack line, [`Fight::limit`] for one parting.
    pub strain: f32,
    /// Seconds into the fight: what the runs are timed off.
    pub age: f32,
}

/// How fast a fish of middling strength comes in on a reeling hand: about
/// four seconds of reeling, which with the pauses a run asks for is a trout
/// in eight and a middling pike in fifteen.
const GAIN_PER_SECOND: f32 = 0.26;

/// How fast the strain sheds on a slack line.
const SLACK_PER_SECOND: f32 = 0.85;

/// How it ended.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Fought {
    /// Still on.
    On,
    /// It is at the bank: the fish is landed.
    Landed,
    /// The line parted, or the hook pulled: the fish and the bait are gone.
    Lost,
}

impl Fight {
    /// A middling fish of this kind on a sound rod.
    pub fn new(species: Species) -> Fight {
        Fight::sized(species, Size::Middling, 1.0)
    }

    /// A fish of this kind and size, on a rod whose line parts at `limit`.
    pub fn sized(species: Species, size: Size, limit: f32) -> Fight {
        Fight {
            strength: fight_strength(species),
            pull: size.pull(),
            heft: size.heft(),
            limit: limit.clamp(0.5, 1.0),
            gained: 0.0,
            strain: 0.25,
            age: 0.0,
        }
    }

    /// The strain as a share of what parts the line, 0..1: what the gauge
    /// draws, so a worn rod's gauge fills sooner rather than lying about how
    /// near the line is to going.
    pub fn tension(&self) -> f32 {
        (self.strain / self.limit).clamp(0.0, 1.0)
    }

    /// **Is the fish running right now?** A sum of two waves whose periods
    /// do not divide each other, so the pattern does not repeat inside a
    /// fight and cannot be counted out loud -- but it is smooth, so a run
    /// always announces itself a moment before it is dangerous. A player who
    /// is watching the float can feel one coming; one who is holding the
    /// button down cannot.
    pub fn running(&self) -> f32 {
        let a = (self.age * 2.3).sin();
        let b = (self.age * 1.1 + 1.7).sin();
        (0.5 + 0.5 * (a * 0.65 + b * 0.35)).clamp(0.0, 1.0)
    }

    /// One tick of the fight. `reeling` is the player pulling.
    pub fn step(&mut self, dt: f32, reeling: bool) -> Fought {
        self.age += dt;
        let run = self.running();
        if reeling {
            // Reeling into a run is what breaks a line; reeling while it is
            // resting is nearly free.
            // **Squared, and that is the whole of the difficulty curve.** A
            // line takes the strain a fish puts on it, and a fish twice as
            // strong does not pull twice as hard for a quarter of the time
            // -- it pulls twice as hard for twice as long. Linear, the
            // school broke lines as readily as the pike did (which is what
            // this looked like before the test below went red): the first
            // fish anybody hooks has to come in on a heavy hand, and the
            // pike has to be played.
            //
            // **The kind squared and the size once.** Squared again for size
            // was tried first, and a big cod put 4.4 of strain a second on
            // the line at the top of a run: three server ticks from slack to
            // parted, which no hand reading a gauge can answer. A big fish
            // is mostly *longer* (`Size::heft`), and the strain it adds is
            // the size's share on top of the kind's.
            self.strain += dt * self.strength * self.strength * self.pull * (0.25 + 1.3 * run);
            self.gained += dt * GAIN_PER_SECOND / (self.strength.max(0.2) * self.heft);
        } else {
            self.strain -= dt * SLACK_PER_SECOND;
            // A fish given line takes some back, and a fish that is running
            // takes more. Never all of it: a player who stands there doing
            // nothing loses slowly, not instantly.
            //
            // **And less of it than it once took**: at 0.08 a second a big
            // pike given line took back as much as the reeling between its
            // runs won, and a fight played perfectly never ended.
            self.gained -= dt * 0.05 * (0.2 + run);
        }
        self.strain = self.strain.clamp(0.0, 1.5);
        self.gained = self.gained.clamp(0.0, 1.0);
        if self.strain >= self.limit {
            return Fought::Lost;
        }
        if self.gained >= 1.0 {
            return Fought::Landed;
        }
        Fought::On
    }
}

/// **Where a worn rod's line parts**: at full strain on a rod with a quarter
/// of its life or more left, and at down to four fifths of it on one about
/// to go. `left` is the share of the rod's life remaining, 0..1.
///
/// Rejected: a worn rod that fishes exactly like a new one until the fish
/// it breaks on. Wear that is invisible until it is fatal is a trap; a rod
/// whose gauge fills sooner ([`Fight::tension`]) is a rod telling its owner
/// to make another before they go after the pike.
pub fn line_limit(left: f32) -> f32 {
    let left = left.clamp(0.0, 1.0);
    if left >= 0.25 {
        1.0
    } else {
        0.8 + 0.2 * left / 0.25
    }
}

/// **What a parted line costs the rod**, in fish of its life
/// (`BlockDef::durability` counts fish landed).
///
/// A line that parts under a fish jars the whole rod, and the heavier the
/// fish the harder: two fish' worth off a small one, seven off a middling
/// pike, ten off a big cod. So a rod with little life left does not survive
/// losing a big fish -- it breaks in the hand -- and the decision it makes is
/// the one the owner asked for: **play the fish, and carry a spare** to the
/// water where the big ones are.
///
/// Rejected: a chance of breaking on every run. A rod that might snap at
/// any moment of any fight is a dice roll the player cannot influence; a rod
/// that pays for a lost fish is paying for a thing the player did.
pub fn snap_shock(species: Species, size: Size) -> u32 {
    ((fight_strength(species) * size.pull() * 6.0).round() as u32).max(1)
}

// ---------------------------------------------------------------------------
// The water seen from the bank
// ---------------------------------------------------------------------------

/// **How often a fish near the top breaks the surface**, per minute: 1.5
/// for a fish at an ordinary hour, times [`appetite`].
///
/// **The same appetite the rod reads**, so the water's surface is the bite
/// made visible: rings everywhere at dusk before rain, a flat glassy lake at
/// a clear noon in winter. A rise the player could not trust would teach
/// them to ignore the water; this one teaches them to read it.
///
/// Rejected: rises drawn by the client from the time of day alone. The
/// client does not know the sky that is coming, and cannot know where the
/// fish are -- which are the two things a rise is worth watching for.
pub fn rise_rate(moment: Moment) -> f32 {
    RISES_A_MINUTE
        * appetite(moment)
        // The sun on the surface puts them down as it does the bite, read
        // for open water two deep: a rise is a fish at the top.
        * glare_factor(moment.time_of_day, moment.weather, 2, false)
}

/// See [`rise_rate`].
pub const RISES_A_MINUTE: f32 = 1.5;

/// How far under the surface a fish can be and still be seen to rise, in
/// blocks.
pub const RISE_DEPTH: f32 = 2.5;

/// How far from somebody a rise is worth telling them about, in blocks:
/// about as far as a ring on the water can be told from the light on it.
pub const RISE_SEEN: f32 = 40.0;

/// **What share of rises are a fish clean out of the water**, rather than a
/// ring: by kind, and more of them as rain comes.
///
/// A trout leaps, a pike swirls, the school dimples: the shape of the rise
/// says what made it, which is worth knowing before choosing a bait. And the
/// share goes up with the front ([`front_factor`]), so **a lake where the
/// fish are jumping is a lake telling you it is about to rain** -- the
/// forecast a player can read, in the only place the server keeps one.
pub fn leap_share(species: Species, moment: Moment) -> f32 {
    let base = match species {
        Species::Trout => 0.35,
        Species::Pike => 0.12,
        Species::Cod => 0.08,
        Species::Herring => 0.15,
        Species::Fish => 0.1,
        _ => 0.1,
    };
    let front = (front_factor(moment.weather, moment.coming) - 1.0) / (FRONT_BEST - 1.0);
    (base * (1.0 + 2.5 * front)).clamp(0.0, 0.9)
}

/// Is a cast still a cast? The player's eye within [`CAST_HOLDS`] of the
/// middle of the float's cell. Both sides ask it: the server to end a cast,
/// the client to stop drawing the float.
pub fn cast_holds(eye: (f32, f32, f32), float: (i32, i32, i32)) -> bool {
    let (dx, dy, dz) = (
        float.0 as f32 + 0.5 - eye.0,
        float.1 as f32 + 0.5 - eye.1,
        float.2 as f32 + 0.5 - eye.2,
    );
    dx * dx + dy * dy + dz * dz <= CAST_HOLDS * CAST_HOLDS
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::{BLOCK_AIR, BLOCK_STONE, BLOCK_WATER};
    use std::collections::HashSet;

    /// A world of stone with water where it is listed and air above y = 20,
    /// and somewhere to put weed in it and a bank over it.
    struct Pond {
        water: HashSet<(i32, i32, i32)>,
        /// Cells of the water that are kelp rather than open water.
        weed: HashSet<(i32, i32, i32)>,
        /// Cells of stone hanging in the air: a bank cut, a jetty.
        roof: HashSet<(i32, i32, i32)>,
    }

    impl Pond {
        /// A pool `w` by `l` and `depth` deep, its surface at y = 20.
        fn dug(w: i32, l: i32, depth: i32) -> Self {
            let mut water = HashSet::new();
            for x in 0..w {
                for z in 0..l {
                    for y in (21 - depth)..=20 {
                        water.insert((x, y, z));
                    }
                }
            }
            Pond {
                water,
                weed: HashSet::new(),
                roof: HashSet::new(),
            }
        }

        fn block(&self, x: i32, y: i32, z: i32) -> Option<BlockId> {
            Some(if self.weed.contains(&(x, y, z)) {
                crate::types::BLOCK_KELP
            } else if self.water.contains(&(x, y, z)) {
                BLOCK_WATER
            } else if self.roof.contains(&(x, y, z)) {
                BLOCK_STONE
            } else if y > 20 {
                BLOCK_AIR
            } else {
                BLOCK_STONE
            })
        }
    }

    fn spot(pond: &Pond, at: (i32, i32, i32)) -> Option<Spot> {
        survey(|x, y, z| pond.block(x, y, z), at)
    }

    /// The wait in water nobody has fished lately, in spring, on a bare
    /// hook: the shape most of these tests ask about, with the three new
    /// terms held still.
    fn wait(spot: Spot, kind: Water, time_of_day: f32, weather: Weather, water_c: f32) -> Option<f32> {
        rod_wait_seconds(spot, kind, Moment::plain(time_of_day, weather, water_c), 0.0, 0, None)
    }

    #[test]
    fn dry_land_is_not_water_to_fish() {
        let pond = Pond::dug(6, 6, 3);
        assert_eq!(spot(&pond, (20, 20, 20)), None, "stone surveyed as water");
        assert_eq!(spot(&pond, (2, 22, 2)), None, "air over a pond surveyed as water");
    }

    #[test]
    fn a_puddle_one_block_across_holds_no_fish_for_a_rod_or_a_trap() {
        let puddle = Pond::dug(1, 1, 1);
        let found = spot(&puddle, (0, 20, 0)).expect("the puddle is water");
        assert_eq!(found.cells, 1);
        assert!(!found.holds_fish());
        for kind in [Water::Fresh, Water::Standing, Water::Salt] {
            assert_eq!(wait(found, kind, 0.25, Weather::Rain, 20.0), None, "a bite in a puddle");
            assert_eq!(trap_chance(found, kind, 20.0), 0.0, "a fish swam into a trap in a puddle");
        }
    }

    #[test]
    fn a_pond_a_player_digs_is_water_a_fish_lives_in() {
        let pond = Pond::dug(3, 3, 3);
        let found = spot(&pond, (1, 20, 1)).expect("water");
        assert_eq!(found.depth, 3);
        assert_eq!(found.cells, 27);
        assert!(wait(found, Water::Standing, 0.5, Weather::Clear, 20.0).is_some());
    }

    #[test]
    fn the_survey_is_the_same_from_any_depth_of_the_same_column() {
        let pond = Pond::dug(5, 5, 4);
        assert_eq!(spot(&pond, (2, 20, 2)), spot(&pond, (2, 17, 2)));
    }

    #[test]
    fn a_float_on_the_bottom_of_the_shallows_takes_nothing() {
        let shallows = Pond::dug(9, 9, 1);
        let found = spot(&shallows, (4, 20, 4)).expect("water");
        assert!(found.holds_fish(), "eighty-one cells is plenty of water");
        assert_eq!(wait(found, Water::Fresh, 0.25, Weather::Clear, 20.0), None);
    }

    #[test]
    fn deep_wide_water_bites_sooner_than_a_small_pond() {
        let small = spot(&Pond::dug(3, 4, 2), (1, 20, 1)).expect("water");
        let lake = spot(&Pond::dug(12, 12, 6), (6, 20, 6)).expect("water");
        let bite = |s| wait(s, Water::Standing, 0.5, Weather::Clear, 20.0).expect("a bite");
        assert!(bite(lake) * 2.0 < bite(small), "lake {} s against pond {} s", bite(lake), bite(small));
    }

    #[test]
    fn the_rod_and_the_trap_want_different_water() {
        let shallows = spot(&Pond::dug(9, 9, 2), (4, 20, 4)).expect("water");
        let deep = spot(&Pond::dug(9, 9, 7), (4, 20, 4)).expect("water");
        // The trap: a river's shallows over a deep lake over the sea.
        let river_shallows = trap_chance(shallows, Water::Fresh, 20.0);
        assert!(river_shallows > trap_chance(deep, Water::Standing, 20.0));
        assert!(trap_chance(deep, Water::Standing, 20.0) > trap_chance(deep, Water::Salt, 20.0));
        assert!(river_shallows > trap_chance(deep, Water::Fresh, 20.0), "a trap liked deep water");
        // The rod: the deep sea over the river shallows.
        let rod = |s, k| wait(s, k, 0.5, Weather::Clear, 20.0).expect("a bite");
        assert!(rod(deep, Water::Salt) < rod(shallows, Water::Fresh), "the rod liked the shallows");
    }

    #[test]
    fn dusk_bites_better_than_noon_and_noon_better_than_midnight() {
        for dusk in [0.25, 0.75] {
            assert!(hour_factor(dusk) > hour_factor(0.5) * 1.5, "twilight at {dusk} is no better than noon");
        }
        assert!(hour_factor(0.5) > hour_factor(0.0));
        // And no cliff anywhere in the day: a minute later is never twice
        // as good.
        let minute = 1.0 / (24.0 * 60.0);
        let mut t = 0.0;
        while t < 1.0 {
            let (a, b) = (hour_factor(t), hour_factor(t + minute));
            assert!((a / b - 1.0).abs() < 0.05, "a step in the hour at {t}: {a} then {b}");
            t += minute;
        }
    }

    #[test]
    fn rain_brings_the_fish_up_and_a_storm_puts_them_down() {
        assert!(weather_factor(Weather::Rain) > weather_factor(Weather::Clear));
        assert!(weather_factor(Weather::Storm) < weather_factor(Weather::Clear));
    }

    #[test]
    fn cold_water_is_slow_water_and_still_water_to_fish() {
        assert!(cold_factor(0.0) < cold_factor(8.0));
        assert!(cold_factor(8.0) < cold_factor(20.0));
        assert!(cold_factor(-30.0) > 0.0, "winter stopped the fish altogether");
        assert!((cold_factor(12.0) - 1.0).abs() < 1e-5, "warm water still slowed the fish");
    }

    #[test]
    fn each_kind_of_water_offers_the_fish_that_lives_in_it() {
        // **The rod agrees with the water.** What can take the hook is drawn
        // from the fish that live in that place (`rod_odds`), so the deep sea
        // is the only water that can give a cod, a lake deep enough to swim
        // in is the only one that can give a pike, cold running water is the
        // trout's, and the school is in all of them. The table itself is
        // `animals::Species::lives_in`; this is the rod reading it.
        use crate::worldgen::Biome;
        // On a worm, which is the bait every fish in the game will take
        // something of: what this is about is the *water*, so the hook is
        // held still. A bare hook would narrow it to the school and the
        // herring (`Bait::tempts`), which is the test below.
        let here = |biome, spot| -> Vec<&'static str> {
            rod_odds(biome, spot, Habitat::plain(Water::Fresh), Some(Bait::Worm)).into_iter().map(|(species, _)| species.name()).collect()
        };
        let deep_sea = spot(&Pond::dug(9, 9, 9), (4, 20, 4)).expect("water");
        let shallow = spot(&Pond::dug(9, 9, 2), (4, 20, 4)).expect("water");
        let lake = spot(&Pond::dug(9, 9, 5), (4, 20, 4)).expect("water");

        assert!(here(Biome::Ocean, deep_sea).contains(&"cod"), "no cod off a nine-block shelf");
        assert!(!here(Biome::Ocean, shallow).contains(&"cod"), "a cod in the surf");
        assert!(here(Biome::Ocean, shallow).contains(&"herring"), "no herring in the shallow sea");

        assert!(here(Biome::Plains, lake).contains(&"pike"), "no pike in a meadow lake");
        assert!(!here(Biome::Plains, shallow).contains(&"pike"), "a pike in two blocks of pond");
        assert!(!here(Biome::River, lake).contains(&"pike"), "a pike in running water");

        assert!(here(Biome::River, shallow).contains(&"trout"), "no trout in a river");
        assert!(!here(Biome::Plains, lake).contains(&"trout"), "a trout in a warm lake");

        // ...and the school is everywhere there is water at all to fish.
        for (biome, water) in [(Biome::Ocean, deep_sea), (Biome::River, shallow), (Biome::Plains, lake)] {
            assert!(here(biome, water).contains(&"fish"), "no school in {}", biome.name());
        }
    }

    /// Every fish that could take this hook, in this water, with this on it.
    fn could_take(biome: Biome, spot: Spot, bait: Option<Bait>) -> Vec<Species> {
        rod_odds(biome, spot, Habitat::plain(Water::Fresh), bait).into_iter().map(|(species, _)| species).collect()
    }

    #[test]
    fn a_fish_is_only_caught_in_water_that_has_it() {
        let deep_sea = spot(&Pond::dug(9, 9, 9), (4, 20, 4)).expect("water");
        let shallow_sea = spot(&Pond::dug(9, 9, 3), (4, 20, 4)).expect("water");
        let lake = spot(&Pond::dug(12, 12, 5), (6, 20, 6)).expect("water");
        // The cod is off the shelf and nowhere else -- not in the shallows
        // over the same sand, and not up a river.
        assert!(could_take(Biome::Ocean, deep_sea, Some(Bait::Scrap)).contains(&Species::Cod));
        assert!(!could_take(Biome::Ocean, shallow_sea, Some(Bait::Scrap)).contains(&Species::Cod), "a cod in the surf");
        assert!(!could_take(Biome::River, deep_sea, Some(Bait::Scrap)).contains(&Species::Cod), "a cod up a river");
        // The pike wants a lake it has to be swum in, and lives in warm
        // standing water rather than in the river.
        assert!(could_take(Biome::Plains, lake, Some(Bait::Scrap)).contains(&Species::Pike));
        assert!(!could_take(Biome::River, lake, Some(Bait::Scrap)).contains(&Species::Pike), "a pike in the river");
        // The trout is the river and the cold country.
        assert!(could_take(Biome::River, lake, Some(Bait::Worm)).contains(&Species::Trout));
        assert!(!could_take(Biome::Plains, lake, Some(Bait::Worm)).contains(&Species::Trout), "a trout in a warm pond");
        // ...and whatever the roll, nothing comes out of water that has
        // nothing in it.
        for roll in 0..20 {
            let roll = roll as f32 / 20.0;
            let caught = rod_species(Biome::River, deep_sea, Habitat::plain(Water::Fresh), Some(Bait::Scrap), roll);
            assert!(caught != Some(Species::Cod) && caught != Some(Species::Pike), "{caught:?} came up a river");
        }
    }

    #[test]
    fn what_a_fish_is_worth_is_the_fish_and_not_a_second_table() {
        // The count comes off `Species::drops`, so a cod is three fish and
        // a herring is one without `fishing` holding a number of its own.
        let fish = |species: Species| -> u32 {
            species.drops().iter().filter(|&&(block, _)| block == crate::types::BLOCK_RAW_FISH).map(|&(_, n)| n).sum()
        };
        assert_eq!(fish(Species::Cod), 3);
        assert_eq!(fish(Species::Pike), 3);
        assert_eq!(fish(Species::Trout), 2);
        assert_eq!(fish(Species::Herring), 1);
        assert_eq!(fish(Species::Fish), 1);
    }

    #[test]
    fn different_bait_catches_different_fish_and_a_bare_hook_catches_the_commonest() {
        let sea = spot(&Pond::dug(9, 9, 9), (4, 20, 4)).expect("water");
        let river = spot(&Pond::dug(9, 9, 4), (4, 20, 4)).expect("water");
        // Meat takes the fish that eat fish, and nothing else wants it much.
        assert!(Bait::tempts(Some(Bait::Scrap), Species::Pike) > Bait::tempts(Some(Bait::Worm), Species::Pike));
        assert!(Bait::tempts(Some(Bait::Scrap), Species::Cod) > Bait::tempts(Some(Bait::Grub), Species::Cod));
        // The fly and the grub are the trout's, and a berry is nobody's.
        assert!(Bait::tempts(Some(Bait::Fly), Species::Trout) > Bait::tempts(Some(Bait::Worm), Species::Trout));
        assert_eq!(Bait::tempts(Some(Bait::Berry), Species::Pike), 0.0, "a pike ate a berry");
        assert!(!could_take(Biome::Plains, river, Some(Bait::Berry)).contains(&Species::Pike));
        // **The bare hook is never nothing**: the school still takes it,
        // slowly, so a player who has dug no worms is never stopped.
        let bare = could_take(Biome::Ocean, sea, None);
        assert!(bare.contains(&Species::Fish), "a bare hook caught nothing at all");
        assert!(!bare.contains(&Species::Cod), "a bare hook took a cod");
        let slow = wait(sea, Water::Salt, 0.5, Weather::Clear, 20.0).expect("a bite");
        let baited =
            rod_wait_seconds(sea, Water::Salt, Moment::plain(0.5, Weather::Clear, 20.0), 0.0, 0, Some(Bait::Worm))
                .expect("a bite");
        assert!(baited < slow, "a worm ({baited} s) was no faster than a bare hook ({slow} s)");
        // ...and the fly is the one that is not eaten.
        assert!(Bait::keeps(Bait::Fly));
        for bait in [Bait::Worm, Bait::Grub, Bait::Berry, Bait::Bread, Bait::Scrap] {
            assert!(!bait.keeps(), "{bait:?} stayed on the hook");
        }
        // Every bait is an item, and every one of those items is that bait.
        for bait in Bait::ALL {
            assert_eq!(Bait::of_block(bait.block()), Some(bait));
        }
        assert_eq!(Bait::of_block(crate::types::BLOCK_STONE), None);
    }

    #[test]
    fn a_spot_fished_out_bites_slower_and_comes_back_on_its_own() {
        let lake = spot(&Pond::dug(12, 12, 5), (6, 20, 6)).expect("water");
        let rested = wait(lake, Water::Standing, 0.5, Weather::Clear, 20.0).expect("a bite");
        let spent = rod_wait_seconds(lake, Water::Standing, Moment::plain(0.5, Weather::Clear, 20.0), SPOT_HOLDS, 0, None)
            .expect("a bite");
        assert!(spent > rested * 3.0, "a fished-out lake ({spent} s) fishes like a rested one ({rested} s)");
        assert!(spent <= ROD_LONGEST_SECONDS, "a spot said \"not here\" in {spent} s, which reads as a broken rod");
        // Never dead, though: the float still goes under eventually, because
        // a spot that stopped would read as a bug.
        assert!(spent.is_finite() && spent > 0.0);
        // And it comes back by itself, all the way, in its own time.
        assert!(pressure_recovered(SPOT_HOLDS, SPOT_RECOVERS_SECONDS) < SPOT_HOLDS);
        assert_eq!(pressure_recovered(SPOT_HOLDS, SPOT_RECOVERS_SECONDS * SPOT_HOLDS), 0.0);
        assert_eq!(pressure_recovered(0.0, 10.0), 0.0, "a rested spot went into debt");
        // Two casts a few blocks apart are one spot; twenty blocks apart are
        // two.
        assert_eq!(spot_key((100, 20, 100)), spot_key((102, 20, 103)));
        assert_ne!(spot_key((100, 20, 100)), spot_key((124, 20, 100)));
    }

    #[test]
    fn weed_and_shade_are_worth_something_and_never_worth_more_than_the_hour() {
        let open = Pond::dug(9, 9, 4);
        let bare = spot(&open, (4, 20, 4)).expect("water");
        assert_eq!(bare.weeds, 0);
        assert!(!bare.shade, "open water read as shaded");
        // The same pool with weed through it and a bank over the float.
        let mut weedy = Pond::dug(9, 9, 4);
        weedy.weed.extend((2..7).flat_map(|x| (2..7).map(move |z| (x, 20, z))));
        weedy.roof.insert((4, 23, 4));
        let cover = spot(&weedy, (4, 20, 4)).expect("water");
        assert!(cover.weeds > 0, "a reedbed counted as open water");
        assert!(cover.shade, "a bank over the float was not shade");
        assert!(cover_factor(cover) > cover_factor(bare));
        // ...and it is a third at most: dusk is still the bigger decision.
        assert!(cover_factor(cover) < 1.4, "weed is worth {} of a spot", cover_factor(cover));
        assert!(
            hour_factor(0.25) / hour_factor(0.5) > cover_factor(cover) / cover_factor(bare),
            "a reedbed at noon beats open water at dusk"
        );
    }

    #[test]
    fn the_season_moves_the_fish_and_the_cold_moves_them_again() {
        assert!(season_factor(Season::Spring) > season_factor(Season::Autumn));
        assert!(season_factor(Season::Autumn) > season_factor(Season::Winter));
        // The two are separate: a warm pool in winter still fishes badly,
        // and a cold one in spring does too.
        let lake = spot(&Pond::dug(12, 12, 5), (6, 20, 6)).expect("water");
        let one = |season, water_c| {
            let moment = Moment { time_of_day: 0.5, weather: Weather::Clear, coming: None, water_c, season };
            rod_wait_seconds(lake, Water::Standing, moment, 0.0, 0, None).expect("a bite")
        };
        assert!(one(Season::Winter, 18.0) > one(Season::Spring, 18.0), "winter fished as well as spring");
        assert!(one(Season::Spring, 0.0) > one(Season::Spring, 18.0), "an ice hole fished as well as May");
    }

    #[test]
    fn casting_distance_follows_the_hold() {
        // Nothing held is the shortest throw there is, and it is still a
        // throw: a flick over the reeds at your feet.
        assert!(cast_distance(cast_power(0.0)) >= CAST_NEAREST - 0.01);
        assert!(cast_distance(cast_power(CAST_CHARGE_SECONDS)) >= CAST_FARTHEST - 0.01);
        // Holding it longer never throws it shorter, and holding it past the
        // wind-up never throws it further.
        let mut last = 0.0;
        let mut held = 0.0;
        while held < CAST_CHARGE_SECONDS * 2.0 {
            let distance = cast_distance(cast_power(held));
            assert!(distance >= last - 1e-5, "holding {held}s threw {distance} after {last}");
            assert!(distance <= CAST_FARTHEST + 1e-5, "a throw carried {distance}");
            last = distance;
            held += 0.02;
        }
        // The leash has to reach past the longest throw, and this is the
        // assertion that says so: a float at the far end of a full cast is
        // still a line in the water.
        assert!(cast_holds((0.0, 21.0, 0.0), (0, 20, CAST_FARTHEST as i32)));
    }

    #[test]
    fn a_throw_lands_where_it_was_aimed_and_stops_at_what_is_in_the_way() {
        // A pond from z = 6 to z = 15, the player on the bank at z = 1.
        let pond = |x: i32, y: i32, z: i32| {
            Some(if (0..20).contains(&x) && (3..18).contains(&z) && y <= 20 && y > 15 {
                BLOCK_WATER
            } else if y > 20 {
                BLOCK_AIR
            } else {
                BLOCK_STONE
            })
        };
        let eye = (10.5, 21.6, 1.5);
        let flat = (0.0, -0.06, 1.0);
        let near = cast_target(eye, flat, 0.0, pond).expect("a flick reaches the near edge");
        let far = cast_target(eye, flat, 1.0, pond).expect("a full throw reaches");
        assert!(far.2 > near.2, "the full throw ({}) landed nearer than the flick ({})", far.2, near.2);
        // Aimed left, it lands left: the hold is the distance and the aim is
        // the direction.
        let left = cast_target(eye, (-0.6, -0.06, 1.0), 1.0, pond).expect("a throw across the pond");
        assert!(left.0 < far.0, "a throw aimed left landed at x {} against {}", left.0, far.0);
        // A wall between the player and the water stops it.
        let walled = |x: i32, y: i32, z: i32| {
            if z == 2 && y > 20 && y < 25 {
                Some(BLOCK_STONE)
            } else {
                pond(x, y, z)
            }
        };
        assert_eq!(cast_target(eye, flat, 1.0, walled), None, "the line went through a wall");
        // Straight up is not a cast.
        assert_eq!(cast_target(eye, (0.0, 1.0, 0.0), 1.0, pond), None);
        // A chunk this side has not got is not water.
        assert_eq!(cast_target(eye, flat, 1.0, |_, _, _| None), None);
    }

    #[test]
    fn a_big_fish_fought_carelessly_parts_the_line_and_a_small_one_forgives_it() {
        // Hauling without pause: the strain gets away on a pike and does
        // not on the school.
        let hauled = |species| {
            let mut fight = Fight::new(species);
            let mut out = Fought::On;
            for _ in 0..600 {
                out = fight.step(0.05, true);
                if out != Fought::On {
                    break;
                }
            }
            out
        };
        assert_eq!(hauled(Species::Pike), Fought::Lost, "a pike hauled at came straight in");
        assert_eq!(hauled(Species::Fish), Fought::Landed, "the first fish a player ever hooks got away");
        // A line given whenever the fish runs comes in, however big it is.
        let mut fight = Fight::new(Species::Cod);
        let mut out = Fought::On;
        for _ in 0..3000 {
            let reeling = fight.running() < 0.45 && fight.strain < 0.7;
            out = fight.step(0.05, reeling);
            if out != Fought::On {
                break;
            }
        }
        assert_eq!(out, Fought::Landed, "a cod played properly still got away");
        // And doing nothing at all loses ground without ever losing the
        // fish outright: the line goes slack, it does not part.
        let mut idle = Fight::new(Species::Trout);
        for _ in 0..200 {
            assert_eq!(idle.step(0.05, false), Fought::On);
        }
        assert_eq!(idle.strain, 0.0, "a slack line was still under strain");
        assert_eq!(idle.gained, 0.0);
    }

    #[test]
    fn a_trap_is_in_the_water_only_with_water_on_two_sides() {
        let mut river = Pond::dug(9, 9, 2);
        // On the bank beside the pond: one face to the water.
        assert!(trap_water(|x, y, z| river.block(x, y, z), (9, 20, 4)).is_none(), "a basket on the bank fished");
        // Set in the water: the cell is the trap, and water round it.
        river.water.remove(&(4, 20, 4));
        let (found, side) = trap_water(|x, y, z| river.block(x, y, z), (4, 20, 4)).expect("a trap in a pond");
        assert!(found.holds_fish());
        assert!(river.water.contains(&side), "the trap was told its water is {side:?}, which is not water");
        // ...and in a puddle-sized notch it is in the water and fishes nothing.
        let mut notch = Pond::dug(3, 1, 1);
        notch.water.remove(&(1, 20, 0));
        let (found, _) = trap_water(|x, y, z| notch.block(x, y, z), (1, 20, 0)).expect("water on two sides");
        assert_eq!(trap_chance(found, Water::Fresh, 20.0), 0.0, "a trap in a ditch caught a fish");
    }

    #[test]
    fn a_trap_fills_in_about_a_day_and_a_half_in_the_best_water_and_never_overnight() {
        let best = spot(&Pond::dug(12, 12, 2), (6, 20, 6)).expect("water");
        let per_day = trap_chance(best, Water::Fresh, 20.0) * 4.0;
        assert!((1.5..=2.5).contains(&per_day), "{per_day} fish a day in the best river");
        assert!(f32::from(TRAP_HOLDS) / per_day > 1.0, "a trap is full in under a day");
    }

    #[test]
    fn a_cast_ends_when_the_fisher_walks_away() {
        assert!(cast_holds((0.5, 21.6, 0.5), (3, 20, 0)));
        assert!(!cast_holds((20.5, 21.6, 0.5), (3, 20, 0)));
    }

    // ---- the water decides the fish ----

    /// The share of every catch this fish is, in this water.
    fn share(biome: Biome, spot: Spot, habitat: Habitat, bait: Option<Bait>, species: Species) -> f32 {
        let odds = rod_odds(biome, spot, habitat, bait);
        let total: f32 = odds.iter().map(|&(_, w)| w).sum();
        odds.iter().filter(|&&(s, _)| s == species).map(|&(_, w)| w).sum::<f32>() / total.max(1e-6)
    }

    #[test]
    fn the_colour_of_the_water_decides_which_fish_takes() {
        let river = spot(&Pond::dug(9, 9, 4), (4, 20, 4)).expect("water");
        // A cold clear stream and a warm brown one, in the same country.
        let cold = Habitat { kind: Water::Fresh, chill: 0.9, silt: 0.6 };
        let brown = Habitat { kind: Water::Fresh, chill: 0.15, silt: 0.05 };
        let trout = |h| share(Biome::River, river, h, Some(Bait::Worm), Species::Trout);
        assert!(
            trout(cold) > trout(brown) * 2.0,
            "a cold clear river is {} trout and a warm brown one {}",
            trout(cold),
            trout(brown)
        );
        // ...and the pike is the other corner: the warm marsh, not the milk
        // off a glacier.
        let marsh = Habitat { kind: Water::Standing, chill: 0.1, silt: 0.05 };
        let glacier = Habitat { kind: Water::Standing, chill: 0.9, silt: 0.95 };
        let lake = spot(&Pond::dug(12, 12, 5), (6, 20, 6)).expect("water");
        let pike = |h| share(Biome::Swamp, lake, h, Some(Bait::Scrap), Species::Pike);
        assert!(pike(marsh) > pike(glacier) * 1.5, "a marsh {} pike, a glacier lake {}", pike(marsh), pike(glacier));
        // A weight and never a gate: the rare fish is rare, not gone.
        assert!(trout(brown) > 0.0, "a trout was ruled out of a river by its colour");
        for species in Species::ALL.iter().copied().filter(|s| s.swims()) {
            for h in [cold, brown, marsh, glacier] {
                assert!(habitat_weight(species, h) > 0.0, "{species:?} cannot live in {h:?}");
            }
        }
    }

    #[test]
    fn a_marsh_gives_small_fish_and_a_deep_clear_lake_big_ones() {
        let lake = spot(&Pond::dug(12, 12, 6), (6, 20, 6)).expect("water");
        let bog = spot(&Pond::dug(4, 4, 2), (1, 20, 1)).expect("water");
        let clear = Habitat { kind: Water::Standing, chill: 0.4, silt: 0.5 };
        let peat = Habitat { kind: Water::Standing, chill: 0.3, silt: 0.05 };
        let [lake_small, _, lake_big] = size_odds(lake, clear, 0.0, Some(Bait::Worm));
        let [bog_small, _, bog_big] = size_odds(bog, peat, 0.0, Some(Bait::Worm));
        assert!(lake_big > bog_big * 3.0, "a deep lake is {lake_big} big fish, a bog {bog_big}");
        assert!(bog_small > lake_small * 1.5, "a bog is {bog_small} small fish, a deep lake {lake_small}");
        for odds in [size_odds(lake, clear, 0.0, None), size_odds(bog, peat, SPOT_HOLDS, Some(Bait::Scrap))] {
            assert!((odds.iter().sum::<f32>() - 1.0).abs() < 1e-4, "{odds:?} are not odds");
        }
    }

    #[test]
    fn meat_on_the_hook_is_fewer_bites_of_bigger_fish() {
        let lake = spot(&Pond::dug(12, 12, 5), (6, 20, 6)).expect("water");
        let water = Habitat::plain(Water::Standing);
        let big = |bait| size_odds(lake, water, 0.0, bait)[2];
        assert!(big(Some(Bait::Scrap)) > big(Some(Bait::Worm)) * 1.5, "meat took no bigger fish than a worm");
        assert!(big(Some(Bait::Worm)) > big(Some(Bait::Grub)), "a grub took bigger fish than a worm");
        assert!(Bait::Scrap.appeal() < Bait::Worm.appeal(), "meat brought bites as often as a worm");
    }

    #[test]
    fn a_fished_bank_loses_its_big_fish_before_it_loses_its_bites() {
        let lake = spot(&Pond::dug(12, 12, 5), (6, 20, 6)).expect("water");
        let water = Habitat::plain(Water::Standing);
        let rested = size_odds(lake, water, 0.0, Some(Bait::Worm))[2];
        let tired = size_odds(lake, water, SPOT_HOLDS * 0.5, Some(Bait::Worm))[2];
        let moment = Moment::plain(0.25, Weather::Clear, 18.0);
        let wait = |taken| rod_wait_seconds(lake, Water::Standing, moment, taken, 0, Some(Bait::Worm)).expect("a bite");
        // Half fished: the big fish much fewer, the bites only slower.
        assert!(tired < rested * 0.75, "big fish {rested} rested and {tired} half fished out");
        assert!(wait(SPOT_HOLDS * 0.5) < wait(0.0) * 2.0, "half a bank fished stopped the bites");
    }

    #[test]
    fn a_big_fish_is_more_fish_and_a_small_one_is_never_nothing() {
        for species in Species::ALL.iter().copied().filter(|s| s.swims()) {
            let [small, middling, big] = Size::ALL.map(|size| catch_count(species, size));
            assert!(small >= 1, "a small {species:?} was no fish at all");
            assert!(small <= middling && middling < big, "{species:?}: {small} {middling} {big}");
        }
        assert_eq!(catch_count(Species::Pike, Size::Big), 5, "a big pike is a day's food");
        assert_eq!(catch_count(Species::Fish, Size::Middling), 1);
        for size in Size::ALL {
            assert_eq!(Size::of_code(size.code()), size, "{size:?} did not survive the wire");
        }
    }

    // ---- the hour and the sky ----

    #[test]
    fn the_last_minutes_before_rain_are_the_best_bite_there_is() {
        let soon = Some((Weather::Rain, 20.0));
        assert!(front_factor(Weather::Clear, soon) > weather_factor(Weather::Rain), "the rain itself beat the front");
        assert!(front_factor(Weather::Clear, Some((Weather::Storm, 20.0))) > 1.5, "a storm coming is no front");
        // Too far off, dry to dry, and wet to dry are nothing; nobody
        // knowing is nothing.
        assert_eq!(front_factor(Weather::Clear, Some((Weather::Rain, FRONT_SECONDS + 60.0))), 1.0);
        assert_eq!(front_factor(Weather::Clear, Some((Weather::Clear, 5.0))), 1.0);
        assert_eq!(front_factor(Weather::Rain, Some((Weather::Clear, 5.0))), 1.0);
        assert_eq!(front_factor(Weather::Clear, None), 1.0);
        // No cliff: a second nearer is never much better.
        let mut t = FRONT_SECONDS + 10.0;
        while t > 0.0 {
            let a = front_factor(Weather::Clear, Some((Weather::Rain, t)));
            let b = front_factor(Weather::Clear, Some((Weather::Rain, t - 1.0)));
            assert!(b >= a && b - a < 0.02, "a step in the front at {t} s: {a} then {b}");
            t -= 1.0;
        }
        // ...and it is a bite the rod gives, not only a number.
        let lake = spot(&Pond::dug(12, 12, 5), (6, 20, 6)).expect("water");
        let calm = Moment::plain(0.4, Weather::Clear, 18.0);
        let coming = Moment { coming: soon, ..calm };
        let wait = |m| rod_wait_seconds(lake, Water::Standing, m, 0.0, 0, Some(Bait::Worm)).expect("a bite");
        assert!(wait(coming) < wait(calm) * 0.7, "rain coming made the wait {} against {}", wait(coming), wait(calm));
    }

    #[test]
    fn a_clear_noon_puts_the_fish_off_the_shallows_and_not_off_the_deep_hole() {
        let shallow_noon = glare_factor(0.5, Weather::Clear, 2, false);
        assert!(shallow_noon <= 0.55, "a clear noon over two blocks of water is {shallow_noon}");
        assert!(glare_factor(0.5, Weather::Clear, 6, true) > 0.9, "the deep hole under the willow is off at noon");
        assert!(glare_factor(0.5, Weather::Rain, 2, false) > 0.8, "an overcast noon was as bright as a clear one");
        for dusk in [0.25, 0.75, 0.0] {
            assert!((glare_factor(dusk, Weather::Clear, 2, false) - 1.0).abs() < 1e-4, "glare at {dusk}");
        }
        // So noon is the worst hour of the daylight in the shallows, and not
        // in the deep water.
        let day = |t: f32, depth| hour_factor(t) * glare_factor(t, Weather::Clear, depth, false);
        assert!(day(0.5, 2) < day(0.35, 2) * 0.7, "noon fished the shallows as well as the morning");
        assert!(day(0.5, 7) > day(0.35, 7) * 0.8, "noon put the deep water off as badly as the shallows");
    }

    #[test]
    fn winter_is_slow_and_a_hole_in_the_ice_gives_some_of_it_back() {
        // A lake under a lid of ice, with one hole cut in it.
        let mut lake = Pond::dug(9, 9, 4);
        let mut ice = HashSet::new();
        for x in 0..9 {
            for z in 0..9 {
                if (x, z) != (4, 4) {
                    lake.water.remove(&(x, 20, z));
                    ice.insert((x, 20, z));
                }
            }
        }
        let world = |x: i32, y: i32, z: i32| {
            if ice.contains(&(x, y, z)) {
                Some(crate::types::BLOCK_ICE)
            } else {
                lake.block(x, y, z)
            }
        };
        let hole = survey(world, (4, 20, 4)).expect("the hole is water");
        assert!(hole.iced >= ICED_LID, "a lake under ice read as open water: {} iced", hole.iced);
        assert!(hole.holds_fish() && hole.depth >= MIN_ROD_DEPTH);
        // A throw at the ice meets the ice. A line lowered into the hole
        // goes down it.
        let eye = (4.5, 22.6, 3.3);
        assert_eq!(cast_target(eye, (0.0, -0.06, 1.0), 0.5, world), None, "a throw went through the ice");
        let down = (0.0, -2.1, 1.2);
        assert_eq!(cast_target(eye, down, 0.0, world), Some((4, 20, 4)), "a line lowered missed the hole");
        // ...and the hole draws the fish: slower than spring, faster than
        // the same winter water with no ice on it.
        let open = spot(&Pond::dug(9, 9, 4), (4, 20, 4)).expect("water");
        let winter = Moment { season: Season::Winter, ..Moment::plain(0.3, Weather::Clear, 1.0) };
        let spring = Moment::plain(0.3, Weather::Clear, 14.0);
        let wait = |s, m| rod_wait_seconds(s, Water::Standing, m, 0.0, 0, Some(Bait::Worm)).expect("a bite");
        assert!(wait(hole, winter) < wait(open, winter) * 0.75, "the hole was no better than open winter water");
        assert!(wait(hole, winter) > wait(open, spring), "a hole in the ice fished like May");
        // A line lowered into stone is no line.
        assert_eq!(lowered_line(eye, (0.0, -1.0, 0.0), |_, _, _| Some(BLOCK_STONE)), None, "a line was lowered into stone");
    }

    #[test]
    fn a_shoal_at_the_float_is_worth_walking_to_and_never_worth_more_than_the_hour() {
        assert_eq!(shoal_factor(0), 1.0);
        assert!(shoal_factor(SHOAL_FULL) > shoal_factor(1));
        assert_eq!(shoal_factor(SHOAL_FULL), shoal_factor(SHOAL_FULL * 5), "more fish than a shoal kept paying");
        assert!(hour_factor(0.25) / hour_factor(0.5) > shoal_factor(SHOAL_FULL), "a shoal at noon beats dusk");
    }

    // ---- the float ----

    #[test]
    fn the_strike_window_is_fair_for_every_fish() {
        for species in Species::ALL.iter().copied().filter(|s| s.swims()) {
            let bite = bite_of(species);
            // A second and a half: a slow hand and a slow line with a second
            // to spare.
            assert!(bite.window() >= 1.5, "{species:?} gives {} s to strike", bite.window());
            // A tremble long enough to be seen, and a dip that is its own
            // movement rather than a flicker.
            assert!(bite.nibble >= 0.6 && bite.dip >= 0.5, "{species:?}: {bite:?}");
            // Some trembles come to nothing, and most do not.
            assert!(bite.nothing > 0.0 && bite.nothing < 0.5, "{species:?}: {bite:?}");
        }
        // The float says what is down there: the school pecks longest, the
        // pike runs longest.
        assert!(bite_of(Species::Fish).nibble > bite_of(Species::Pike).nibble);
        assert!(bite_of(Species::Pike).take > bite_of(Species::Fish).take);
    }

    // ---- the fight and the rod ----

    /// Plays a fight with a hand that reads only the gauge, a tenth of a
    /// second late: reels until it reads `high`, gives line until `low`.
    fn by_the_gauge(mut fight: Fight, low: f32, high: f32) -> (Fought, f32) {
        let mut reeling = true;
        let mut seen = std::collections::VecDeque::from([fight.tension(), fight.tension()]);
        for _ in 0..6000 {
            let late = seen.pop_front().unwrap_or(0.0);
            if reeling && late >= high {
                reeling = false;
            } else if !reeling && late <= low {
                reeling = true;
            }
            let out = fight.step(0.05, reeling);
            seen.push_back(fight.tension());
            if out != Fought::On {
                return (out, fight.age);
            }
        }
        (Fought::On, fight.age)
    }

    /// Plays one with a hand that also watches the rod: never reels into a
    /// run.
    fn by_the_run(mut fight: Fight) -> Fought {
        for _ in 0..6000 {
            let reeling = fight.running() < 0.45 && fight.tension() < 0.7;
            let out = fight.step(0.05, reeling);
            if out != Fought::On {
                return out;
            }
        }
        Fought::On
    }

    #[test]
    fn every_fish_of_every_size_comes_in_to_a_player_who_gives_line_when_it_runs() {
        for species in Species::ALL.iter().copied().filter(|s| s.swims()) {
            for size in Size::ALL {
                let out = by_the_run(Fight::sized(species, size, 1.0));
                assert_eq!(out, Fought::Landed, "a {size:?} {species:?} played well got away");
            }
        }
        // Reading the gauge alone brings in everything but the heaviest sea
        // fish, which is the deep sea's prize and wants the rod watched too.
        for species in [Species::Fish, Species::Trout, Species::Pike, Species::Herring] {
            for size in Size::ALL {
                let (out, age) = by_the_gauge(Fight::sized(species, size, 1.0), 0.3, 0.6);
                assert_eq!(out, Fought::Landed, "a {size:?} {species:?} on the gauge alone: {out:?} at {age} s");
            }
        }
        // And a big fish is a longer fight, not a shorter one.
        let (_, middling) = by_the_gauge(Fight::sized(Species::Pike, Size::Middling, 1.0), 0.3, 0.6);
        let (_, big) = by_the_gauge(Fight::sized(Species::Pike, Size::Big, 1.0), 0.3, 0.6);
        assert!(big > middling * 1.5, "a big pike took {big} s and a middling one {middling} s");
    }

    #[test]
    fn a_worn_rod_parts_its_line_sooner_and_a_lost_big_fish_can_break_it() {
        assert_eq!(line_limit(1.0), 1.0);
        assert_eq!(line_limit(0.3), 1.0, "a rod with most of its life left was already weak");
        assert!(line_limit(0.05) < 0.85, "a rod on its last fish held like a new one");
        // The gauge says so: the same strain reads nearer parting.
        let mut worn = Fight::sized(Species::Pike, Size::Middling, line_limit(0.0));
        let mut sound = Fight::sized(Species::Pike, Size::Middling, 1.0);
        worn.strain = 0.6;
        sound.strain = 0.6;
        assert!(worn.tension() > sound.tension());
        // ...and it parts sooner: the same pike hauled at the same way goes
        // earlier on the worn rod.
        let parted = |limit| {
            let mut fight = Fight::sized(Species::Pike, Size::Middling, limit);
            while fight.step(0.01, true) == Fought::On {}
            fight.age
        };
        assert!(parted(line_limit(0.0)) < parted(1.0) * 0.9, "a worn rod held as long as a sound one");
        // What a parted line costs: a lot more for a heavy fish -- more
        // than a worn rod has left -- and never a new rod at one go.
        let durability = crate::types::tool_durability(crate::types::BLOCK_FISHING_ROD).expect("a rod wears");
        assert!(snap_shock(Species::Cod, Size::Big) >= 8, "a big cod lost cost the rod {}", snap_shock(Species::Cod, Size::Big));
        assert!(snap_shock(Species::Fish, Size::Small) <= 2);
        assert!(snap_shock(Species::Cod, Size::Big) < durability / 4, "one lost fish breaks a new rod");
    }

    // ---- the water seen from the bank ----

    #[test]
    fn fish_rise_most_at_dusk_before_rain_and_least_at_a_clear_winter_noon() {
        let dusk_before_rain = Moment { coming: Some((Weather::Rain, 30.0)), ..Moment::plain(0.75, Weather::Clear, 16.0) };
        let winter_noon = Moment { season: Season::Winter, ..Moment::plain(0.5, Weather::Clear, 1.0) };
        let ordinary = Moment::plain(0.4, Weather::Clear, 16.0);
        assert!(rise_rate(dusk_before_rain) > rise_rate(ordinary) * 2.0);
        assert!(rise_rate(winter_noon) < rise_rate(ordinary) * 0.3, "a winter noon rose {}", rise_rate(winter_noon));
        // The rise *is* the bite: what makes the fish bite makes them rise.
        let lake = spot(&Pond::dug(12, 12, 5), (6, 20, 6)).expect("water");
        let bite = |m| 1.0 / rod_wait_seconds(lake, Water::Standing, m, 0.0, 0, Some(Bait::Worm)).expect("a bite");
        assert!(
            (rise_rate(dusk_before_rain) / rise_rate(ordinary)) > 0.5 * (bite(dusk_before_rain) / bite(ordinary)),
            "the water rose less than the fish bit"
        );
    }

    #[test]
    fn fish_jumping_is_a_forecast_of_rain() {
        let calm = Moment::plain(0.3, Weather::Clear, 16.0);
        let coming = Moment { coming: Some((Weather::Rain, 15.0)), ..calm };
        for species in Species::ALL.iter().copied().filter(|s| s.swims()) {
            assert!(leap_share(species, coming) > leap_share(species, calm) * 2.0, "{species:?} did not jump more before rain");
            assert!(leap_share(species, coming) < 1.0, "every rise of a {species:?} was a leap");
        }
        // The trout is the one that leaps; the school dimples.
        assert!(leap_share(Species::Trout, calm) > leap_share(Species::Fish, calm));
    }
}
