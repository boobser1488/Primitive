//! The small life round a player: butterflies over the flowers by day,
//! fireflies on a warm evening, frogs at the edge of fresh water, bees
//! round a wild hive, and bats in the dark under the ground.
//!
//! ## Why these are the client's, and the gull is the server's
//!
//! Three places a living thing could be, and each was weighed:
//!
//! * **An animal on the server** (`logic::animals`) is a slot in the
//!   entity list, a position in every snapshot to every player near it, a
//!   thought and a physics step per tick, and a row in the species table
//!   that every exhaustive match in three crates has to name. That is the
//!   right price for something a player can *do* something to -- strike,
//!   butcher, be startled by alongside a friend -- and it is why the gull
//!   is one: two people on one beach must see the same flock go up. A
//!   butterfly is none of those things. Forty fireflies on a summer night
//!   as server animals would be forty entity updates a tick, for light.
//! * **A particle** (`engine::particles`) has no mind: it is thrown and it
//!   falls. A frog that hops into the pond when a wolf comes near has to
//!   know where the pond is and where the wolf is, and a butterfly that
//!   comes back to *its* flower has to remember the flower.
//! * **This**: a small pool beside the particles with a mind per critter,
//!   drawn into the particle buffer, costing no network, no save and no
//!   server time. What it gives up is that two players see different
//!   butterflies -- which nobody can tell, because a butterfly is not
//!   anywhere in particular. The day one of these becomes something a
//!   player can catch or eat, it moves to the server.
//!
//! ## What each is for
//!
//! **A mechanic should create a decision, not a chore**, and scenery is
//! neither -- so each of the three carries one small true thing:
//!
//! * a **butterfly** says the ground here is in flower, and it keeps its
//!   distance from anybody who runs at it, so walking up on one is slower
//!   than walking past;
//! * **fireflies** say this is a warm evening in a wet place, which is the
//!   one time of the year the swamp is kind;
//! * **bees** are the hive's warning, and its season: a hive with bees about
//!   it is a hive that stings (`primitive_shared::bees`), a robbed one has
//!   twice as many and they come at anybody who stands close, and a hive with
//!   nothing flying is a cold day on which a raid costs nothing. The stings
//!   are the server's; what is drawn here is only what they look like
//!   before they happen;
//! * **bats** are the dark's own, and they say two things. Under the ground
//!   a colony hangs in the black and goes up round anybody who comes near --
//!   further off from a torch (`BAT_WARY_OF_A_TORCH`) -- so a cave that
//!   erupts in wings is a cave with a hollow in it big enough to roost in.
//!   And at dusk they pour out of the mouth of a cave into the open
//!   (`bat_dusk`): a stream of bats over a hillside at sunset is a cave the
//!   player has not found yet, and it is shown where it is;
//! * **frogs** are the one that is information. A chorus falls silent round
//!   anything that means harm (`HUSH_RADIUS`), and the frogs at the bank go
//!   into the water: a pond that has gone quiet in the dark with nobody near
//!   it is a wolf the player has not seen yet (`Surroundings::dangers`).
//!
//! ## What was rejected, and why
//!
//! Songbirds flushing from bushes are the server's fowl already, and the
//! warning they give is now the server's too (`logic::animals::survey`
//! flushes a bird off any hostile animal). Fish jumping are the sea's
//! (`Species::Fish`).
//!
//! **Bats were on this list and are not any more.** The note said they
//! "want caves this world does not mark and would be fireflies in a darker
//! coat". Both halves were answered rather than argued with: the world does
//! not mark a cave, but the light map does -- a hollow with no sky light and
//! three blocks of ground over it is a cave by any definition a bat has
//! (`hollow`), and the way out of one is the first cell along it that the
//! sky reaches (`way_out`). And what makes a bat not a firefly is the two
//! true things above: where the colony is, and where the cave mouth is at
//! dusk. Falling leaves were on it too, as decoration; they are the wind's
//! now, and say what season and what wood it is (`engine::breeze`). Tracks in snow or
//! mud were the most interesting and the least honest here: a track worth
//! following leads to an animal *outside* the interest radius, which the
//! client has never been told about, so the tracks it could draw would all
//! lead to animals already in plain view.

use glam::Vec3;

use primitive_shared::lighting::LightMap;
use primitive_shared::types::{self, BlockId, BLOCK_FLOWER, BLOCK_REEDS, BLOCK_TALL_GRASS};
use primitive_shared::worldgen::Biome;

use crate::engine::mesh::pack_light;
use crate::engine::particles::ParticleVertex;
use crate::engine::texture::{FaceLayers, EXTRA_SNOW};
use crate::logic::chunk_manager::ChunkManager;

/// The most butterflies alive round one player.
///
/// Eight: a meadow in flower with something moving over it, not a swarm.
/// The spawner only puts one where a flower is, so a meadow with three
/// flowers in it has at most three places to put one anyway.
pub const MAX_BUTTERFLIES: usize = 8;

/// The most fireflies. Thirty-six is a ditch lit up; each is two quads.
pub const MAX_FIREFLIES: usize = 36;

/// The most frogs. Eight is a pond's edge -- enough for a chorus to have
/// a *shape*, so that the part of it that has gone quiet can be heard.
pub const MAX_FROGS: usize = 8;

/// The most bees round one player, over every hive in reach.
///
/// Eighteen: a robbed hive's swarm and a calm one's handful at once. Each is
/// a box and two wings, eight quads.
pub const MAX_BEES: usize = 18;

/// How many bees fly round a full hive, and round one that has been robbed.
///
/// **The count is the anger** (`bees::stings` has one sting more for every
/// comb missing), so it is drawn: a player walking back to a hive they
/// raided an hour ago sees twice the bees before they reach for it.
const BEES_CALM: usize = 4;
const BEES_ANGRY: usize = 8;

/// How often the hives near the player are looked for, in seconds, and how
/// far round the player, in blocks.
///
/// **A box and not a column a look**, which is what everything else here
/// is found with, because a hive is one cell in a wood of hundreds of trunks:
/// a look at eight columns a second would find the hive a player is standing
/// under a minute after they walked up. Twenty-five across and eleven high
/// every two seconds is some three thousand reads a second -- a fraction of
/// a millisecond, and the bees are there before the player is.
const HIVE_LOOK_EVERY: f32 = 2.0;
const HIVE_REACH: i32 = 12;
const HIVE_BELOW: i32 = 4;
const HIVE_ABOVE: i32 = 6;

/// How near a player comes to a robbed hive before its bees come out at them,
/// in blocks. Nearer than the reach a hive can be raided from, so the swarm
/// is what a raider walks into, not a thing that chases a passer-by.
const BEE_ANGRY_REACH: f32 = 3.5;

/// How often the pool looks for somewhere to put a new critter, in
/// seconds.
///
/// **One column a look**, about eight a second, and each look is at most
/// `PROBE_UP + PROBE_DOWN` block reads -- so the whole of the searching
/// costs under a hundred and fifty reads a second however much of the
/// world is loaded. Scanning a disc round the player once a second would
/// be fifteen hundred columns for the same answer arriving a little sooner.
const LOOK_EVERY: f32 = 0.12;

/// The ring a new critter is put in, in blocks from the player.
///
/// Outside six so nothing appears at arm's length; inside twenty-two so
/// it is near enough to be seen arriving at the flower rather than blinking
/// into being in the middle distance.
const NEAREST: f32 = 6.0;
const FURTHEST: f32 = 22.0;

/// Beyond this a critter is forgotten, as `DESPAWN_DISTANCE` forgets an
/// animal. Past `FURTHEST`, so nothing is put down and taken away again in
/// the same few steps.
const FORGET_BEYOND: f32 = 30.0;

/// How far above and below the player's feet a look reads a column.
const PROBE_UP: i32 = 8;
const PROBE_DOWN: i32 = 10;

/// Faster than this, in blocks a second, the player is running -- and
/// everything small notices them from further off (`RUNNING_WARINESS`).
///
/// Between the walk (4.3) and the sprint (6.45), so walking is walking.
const RUNNING: f32 = 5.0;

/// How much further off a running player is noticed.
///
/// **The one interaction the butterflies and the frogs share**, and the
/// reason walking up to either is different from running at it: a player
/// who wants to see a frog sits still at a pond's edge, and one who
/// sprints along the bank sees a row of splashes.
const RUNNING_WARINESS: f32 = 1.8;

/// How near a walking player comes before a butterfly takes flight, in
/// blocks.
const BUTTERFLY_WARY: f32 = 2.2;

/// How near a walking player comes before a frog goes into the water.
const FROG_WARY: f32 = 3.5;

/// How near a hostile animal comes before a frog goes into the water.
/// Further than a person, because a wolf is quieter and lower.
const FROG_WARY_OF_BEASTS: f32 = 6.0;

/// How far a chorus falls silent round something that means harm, in
/// blocks.
///
/// **This is the mechanic**, so the number is argued. Twelve is well past
/// the frogs that go into the water (`FROG_WARY_OF_BEASTS`): the silence is
/// wider than the fright, which is how it reads at night -- the near part of
/// the pond stops first and the quiet travels with whatever is moving. It is
/// under a wolf's own awareness (eighteen), so a player who hears the frogs
/// stop has a few seconds before the wolf has them.
const HUSH_RADIUS: f32 = 12.0;

/// ...and round a person, which is much less: a chorus you could never get
/// near would be a chorus nobody hears, and a frog near your feet does go
/// quiet.
const HUSH_RADIUS_PLAYER: f32 = 4.0;

/// How long a hushed frog stays quiet after the last thing that hushed it.
const HUSH_SECONDS: f32 = 8.0;

/// Degrees over freezing each needs before it is out at all.
///
/// Measured against the climate, the season *and the hour* (`warmth_now`),
/// so a swamp in winter is as empty as a tundra in summer and a cold spring
/// night is silent. Frogs first, because a frog in a cold spring is still a
/// frog; fireflies last, because they are the height of summer and nothing
/// else.
///
/// **Eight for a frog, and it was four**, measured against the day's warmth
/// alone and only on the frame a frog appeared. "лягушки игнорируют
/// температуру и сезон": a chorus that came out on a mild autumn afternoon
/// sang on through the frost that night, because nothing asked again, and
/// four degrees over the day's peak is a night well under freezing. Frogs
/// wake at about eight; below that they are in the mud.
const FROG_WARMTH_C: f32 = 8.0;
const BUTTERFLY_WARMTH_C: f32 = 10.0;
const FIREFLY_WARMTH_C: f32 = 14.0;

/// How hard a frog pushes off, up and across, in blocks a second -- and
/// what brings it down.
///
/// Worked through: 4.2 up against 18 of gravity is half a second in the
/// air, and 2.4 across in that time is a block and a fifth, which is the
/// distance from the bank cell to the middle of the water cell beside it.
const HOP_UP: f32 = 4.2;
const HOP_ACROSS: f32 = 2.4;
const FROG_GRAVITY: f32 = 18.0;

/// The most bats round one player: two small colonies, or one and the
/// stream leaving a cave mouth. Each is a box and two wings.
pub const MAX_BATS: usize = 14;

/// How many go up together.
const BAT_COLONY: (usize, usize) = (3, 6);

/// How often the rock round the player is searched for hollows, in seconds,
/// and how far.
///
/// **A box, like the hives, and not a column a look.** A hollow a bat roosts
/// in is a few dozen cells in a cube of ten thousand, and one column in a
/// ring would find the cave the player is standing in a minute after they
/// walked out of it. Twenty-five across and fifteen high every three seconds
/// is three thousand reads a second, most of them stone that is not asked
/// anything more; only air in the dark goes on to count its roof.
const BAT_SEARCH_EVERY: f32 = 3.0;
const BAT_REACH: i32 = 12;
const BAT_BELOW: i32 = 10;
const BAT_ABOVE: i32 = 4;
/// How many hollows are remembered from a search.
const BAT_HOLLOWS_KEPT: usize = 48;

/// What counts as dark: sky light at most this (of fifteen) and block light
/// at most `BAT_DARK_BLOCK`.
///
/// **Four and not nought**, so the back of a short cave counts: sky light
/// falls a level a block from the mouth, and a bat roosts a dozen blocks in,
/// not only where no daylight has ever reached. Block light three, so a torch
/// on the wall -- which is fourteen at the flame -- clears a colony out of
/// every cell it lights, and one burning at the far end of a long gallery does
/// not.
const BAT_DARK_SKY: u8 = 4;
const BAT_DARK_BLOCK: u8 = 3;

/// How many solid cells, in the eight over a hollow, make it *under the
/// ground* rather than under a roof.
///
/// Three: a hut's roof is one course, and a player's dark barn is not a cave
/// -- nor is the shade under the canopy of a wood.
const BAT_ROOF: usize = 3;
const BAT_ROOF_LOOK: i32 = 8;

/// Sky light that is the open air at a cave's mouth, for `way_out`, and how
/// far along a gallery the way out is looked for.
const BAT_MOUTH_SKY: u8 = 12;
const BAT_WAY_OUT: i32 = 16;

/// Sky light at the player's head at or under which they are *in* the dark
/// themselves, and a colony may be found round them.
///
/// **Colonies are only put down round a player who is underground.** A cave
/// ten blocks under a meadow is a hollow by every rule here, and a colony
/// roosting in it would be wingbeats coming up out of the grass. The dusk
/// stream is the one way a bat is seen from the open, and it is the one that
/// comes *to* the open.
const BAT_PLAYER_IN_THE_DARK: u8 = 10;

/// How near a player comes before a colony goes up, in blocks -- and how
/// near with a lit torch in their hand. Twice as far: a bat's whole life is
/// keeping out of the light.
const BAT_WARY: f32 = 5.0;
const BAT_WARY_OF_A_TORCH: f32 = 10.0;

/// How fast a bat flies, and flees, in blocks a second.
const BAT_SPEED: f32 = 3.4;
const BAT_FLEEING: f32 = 6.0;

/// **How far apart two bats hang from one roof**, in blocks: a folded bat
/// is six hundredths across (`build_into`), so this is a body's width of
/// roof between each and the next -- a row of dark drops, not one.
const BAT_HANG_APART: f32 = 0.16;

/// **How close one bat lets another come in flight**, in blocks, and how
/// hard it turns off when one does.
///
/// "летучие мыши скапливаются в одном пикселе": a colony flying in a gallery
/// too narrow for most of the points it wandered toward fell back, bat after
/// bat, on the one point every refused wander fell back to -- its home -- and
/// there it hung, five bats in one place, drawn as one dark speck with
/// wings. Measured over five minutes of night in the gallery the test uses,
/// two bats were within an eighth of a block of each other in 8117 frames of
/// 9000, and the colony drew in to a hundredth of a block about its middle.
/// A real colony in flight is a swirl that never touches: each keeps clear
/// of the next by echo, which is this.
const BAT_ROOM: f32 = 0.9;
const BAT_SHY: f32 = 9.0;

/// How long a bat that has come out of a cave at dusk hunts over the open
/// before it is out of sight, in seconds.
const BAT_HUNTS: (f32, f32) = (22.0, 40.0);

/// How long between the groups a cave mouth lets out at dusk, in seconds.
const BAT_STREAM_EVERY: (f32, f32) = (4.0, 9.0);

/// What a critter is.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    Butterfly,
    Firefly,
    Frog,
    Bee,
    Bat,
}

/// What it is doing.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum State {
    /// On its flower, or sitting on the bank.
    Resting,
    /// Flying about its flower, or drifting.
    Flying,
    /// Getting away from somebody.
    Fleeing,
    /// A frog in the air.
    Hopping,
    /// A frog under the water: not drawn, and gone when its time is up.
    Under,
    /// A bat on its way out of a cave at dusk: to the mouth (`via`), then
    /// out to `home` over the open, then hunting round it.
    Emerging,
}

#[derive(Debug, Clone, Copy)]
struct Critter {
    /// For tests, which have to follow one frog through a `retain`.
    id: u32,
    kind: Kind,
    position: glam::DVec3,
    velocity: Vec3,
    /// The flower, the patch of air over the grass, or -- for a frog --
    /// the middle of the water it goes into.
    home: Vec3,
    /// Where a flight is aimed.
    target: Vec3,
    facing: f32,
    state: State,
    /// Seconds left of whatever `state` is doing.
    timer: f32,
    /// Seconds until a flying butterfly picks a new point near its flower.
    retarget: f32,
    /// Seconds of life, for the wingbeat and the blink.
    phase: f32,
    /// Which colour, and each firefly's own blink period.
    shade: u8,
    /// 0..1: how much of it is there. Fades in on arrival and out on
    /// leaving, so nothing blinks into or out of the world.
    fade: f32,
    /// Its hour is over and it is going.
    leaving: bool,
    /// A frog that is calling.
    calling: bool,
    /// Seconds a frog stays quiet for.
    hushed: f32,
    /// The mouth of the cave an emerging bat is making for.
    via: Vec3,
}

/// What the pool needs to know about the world this frame.
///
/// Borrowed, like `Soundscape`'s `Frame`, because most of it is the world.
pub struct Surroundings<'a> {
    pub chunks: &'a ChunkManager,
    /// The player's feet.
    pub player: Vec3,
    /// The world's age in days, with the hour in the fraction -- the clock
    /// the seasons run on (`Sky::world_days`).
    pub world_time: f32,
    /// Raining or snowing.
    pub wet: bool,
    /// Where the hostile animals near the player are.
    ///
    /// **The client already knows** -- they came in the snapshot -- which is
    /// the whole reason a frog can hear a wolf the player cannot see: the
    /// wolf is behind a hill or in the dark, not outside the interest
    /// radius.
    pub dangers: &'a [Vec3],
    /// The biome of a column, its climate temperature (the generator's 0..1)
    /// and its share of the year's swing (`season::seasonal_swing`), at a
    /// cell. A closure so the tests can say "a warm swamp" without a
    /// generator, and the game can ask its own `WorldGen`.
    pub country: &'a dyn Fn(i32, i32, i32) -> (Biome, f32, f32),
    /// The world's light, which is how a bat knows a cave: see `hollow`.
    pub light: &'a LightMap,
    /// A lit torch in the player's hand. See `BAT_WARY_OF_A_TORCH`.
    pub lantern: bool,
}

/// The pool.
pub struct Critters {
    live: Vec<Critter>,
    seed: u32,
    next_id: u32,
    look_in: f32,
    /// Degrees over freezing at the player this frame (`warmth_now`), which
    /// every creature near enough to be alive is also standing in.
    warm_here: f32,
    last_player: Option<Vec3>,
    /// The wild hives near the player at the last look, and what was in them.
    hives: Vec<((i32, i32, i32), BlockId)>,
    hive_look_in: f32,
    /// Dark hollows under the ground near the player at the last search.
    hollows: Vec<(i32, i32, i32)>,
    bat_search_in: f32,
    /// Seconds until a cave mouth lets the next group out, at dusk.
    bat_stream_in: f32,
}

impl Default for Critters {
    fn default() -> Self {
        Self::new()
    }
}

impl Critters {
    pub fn new() -> Self {
        Self {
            live: Vec::with_capacity(MAX_BUTTERFLIES + MAX_FIREFLIES + MAX_FROGS + MAX_BEES + MAX_BATS),
            seed: 0x51A5_F00D,
            next_id: 0,
            look_in: 0.0,
            warm_here: f32::INFINITY,
            last_player: None,
            hives: Vec::new(),
            hive_look_in: 0.0,
            hollows: Vec::new(),
            bat_search_in: 0.0,
            bat_stream_in: 0.0,
        }
    }

    /// How many are alive, for the F3 panel's sort of question.
    #[cfg_attr(not(test), allow(dead_code))]
    pub fn len(&self) -> usize {
        self.live.len()
    }

    /// Beside `len` because clippy asks for the pair, and read by nothing yet.
    #[allow(dead_code)]
    pub fn is_empty(&self) -> bool {
        self.live.is_empty()
    }

    /// Where the frogs that are calling are, for the soundscape.
    pub fn croaking(&self) -> Vec<Vec3> {
        self.live
            .iter()
            .filter(|c| c.kind == Kind::Frog && c.calling)
            .map(|c| c.position.as_vec3())
            .collect()
    }

    /// Where the bats on the wing are, for the soundscape's wingbeats.
    pub fn flapping(&self) -> Vec<Vec3> {
        self.live
            .iter()
            .filter(|c| c.kind == Kind::Bat && c.state != State::Resting && !c.leaving)
            .map(|c| c.position.as_vec3())
            .collect()
    }

    /// Where the bees that are flying are, for the soundscape's hum.
    pub fn buzzing(&self) -> Vec<Vec3> {
        self.live.iter().filter(|c| c.kind == Kind::Bee && !c.leaving).map(|c| c.position.as_vec3()).collect()
    }

    fn random(&mut self) -> f32 {
        // xorshift32, as `Particles` uses: a scatter nobody replays.
        self.seed ^= self.seed << 13;
        self.seed ^= self.seed >> 17;
        self.seed ^= self.seed << 5;
        (self.seed >> 8) as f32 / (1 << 24) as f32
    }

    fn between(&mut self, low: f32, high: f32) -> f32 {
        low + self.random() * (high - low)
    }

    /// One frame: look for somewhere to put something, then let everything
    /// alive do what it does.
    pub fn update(&mut self, around: &Surroundings<'_>, dt: f32) {
        // A hitch of a whole second must not throw a frog through the bank.
        let dt = dt.clamp(0.0, 0.1);
        let running = match self.last_player {
            Some(last) if dt > 0.0 => (around.player - last).length() / dt > RUNNING,
            _ => false,
        };
        self.last_player = Some(around.player);
        let hour = around.world_time.rem_euclid(1.0);
        // **Asked every frame, not only when something appears.** Everything
        // alive here is within `FORGET_BEYOND` of the player, so the air at
        // the player is the air they are in; one closure call a frame, where
        // a call per critter would be forty.
        let (_, temperature, swing) =
            (around.country)(around.player.x.floor() as i32, around.player.y.floor() as i32, around.player.z.floor() as i32);
        self.warm_here = warmth_now(temperature, around.world_time, swing);

        self.look_in -= dt;
        while self.look_in <= 0.0 {
            self.look_in += LOOK_EVERY;
            self.look(around, hour);
        }

        self.hive_look_in -= dt;
        if self.hive_look_in <= 0.0 {
            self.hive_look_in = HIVE_LOOK_EVERY;
            self.look_for_hives(around);
        }
        self.send_out_bees(around, hour);

        self.bat_search_in -= dt;
        if self.bat_search_in <= 0.0 {
            self.bat_search_in = BAT_SEARCH_EVERY;
            self.search_for_hollows(around);
        }
        self.bat_stream_in -= dt;
        self.rouse_bats(around, hour);

        let wariness = if running { RUNNING_WARINESS } else { 1.0 };
        for index in 0..self.live.len() {
            let mut critter = self.live[index];
            let keep = if (critter.position.as_vec3() - around.player).length() > FORGET_BEYOND {
                false
            } else {
                match critter.kind {
                    Kind::Butterfly => self.butterfly(&mut critter, around, hour, wariness, dt),
                    Kind::Firefly => self.firefly(&mut critter, around, hour, dt),
                    Kind::Frog => self.frog(&mut critter, around, hour, wariness, dt),
                    Kind::Bee => self.bee(&mut critter, around, hour, dt),
                    Kind::Bat => self.bat(&mut critter, around, hour, dt),
                }
            };
            critter.phase += dt;
            if !keep {
                critter.fade = -1.0;
            }
            self.live[index] = critter;
        }
        self.live.retain(|c| c.fade >= 0.0);
    }

    fn count(&self, kind: Kind) -> usize {
        self.live.iter().filter(|c| c.kind == kind).count()
    }

    /// One column, and whatever it will hold.
    fn look(&mut self, around: &Surroundings<'_>, hour: f32) {
        let angle = self.between(0.0, std::f32::consts::TAU);
        let distance = self.between(NEAREST, FURTHEST);
        let x = (around.player.x + angle.cos() * distance).floor() as i32;
        let z = (around.player.z + angle.sin() * distance).floor() as i32;
        let Some(spot) = look_at(around.chunks, x, z, around.player.y.floor() as i32) else {
            return;
        };
        let (biome, temperature, swing) = (around.country)(x, spot.ground_y, z);
        let warm = warmth_now(temperature, around.world_time, swing);
        let top = Vec3::new(x as f32 + 0.5, spot.ground_y as f32 + 1.0, z as f32 + 0.5);

        // **A frog is where fresh water meets a bank**, and nowhere else:
        // the sea is salt, and a frog in a dry meadow is a frog that is
        // going to die in it.
        if let Some((wx, wz)) = spot.water {
            let fresh = !matches!(biome, Biome::Ocean | Biome::Beach);
            let bank = spot.plant.is_none_or(|p| matches!(types::block_kind(p), BLOCK_TALL_GRASS | BLOCK_REEDS));
            if fresh && bank && warm >= FROG_WARMTH_C && self.count(Kind::Frog) < MAX_FROGS {
                let water = Vec3::new(wx as f32 + 0.5, spot.ground_y as f32 + 0.85, wz as f32 + 0.5);
                let toward = (water - top).with_y(0.0).normalize_or_zero();
                let hushed = self.between(0.0, 2.0);
                self.add(Critter {
                    kind: Kind::Frog,
                    position: (top + toward * 0.3).as_dvec3(),
                    home: water,
                    facing: toward.z.atan2(toward.x),
                    state: State::Resting,
                    hushed,
                    ..self.blank(top)
                });
                return;
            }
        }

        if spot.plant.is_some_and(|p| types::block_kind(p) == BLOCK_FLOWER)
            && butterfly_hour(hour)
            && !around.wet
            && warm >= BUTTERFLY_WARMTH_C
            && self.count(Kind::Butterfly) < MAX_BUTTERFLIES
        {
            let flower = top + Vec3::new(0.0, 0.35, 0.0);
            let shade = (self.random() * 4.0) as u8;
            let timer = self.between(1.0, 4.0);
            self.add(Critter {
                kind: Kind::Butterfly,
                position: (flower + Vec3::new(0.0, 1.5, 0.0)).as_dvec3(),
                home: flower,
                target: flower,
                state: State::Flying,
                timer,
                shade,
                fade: 0.0,
                ..self.blank(flower)
            });
            return;
        }

        // **Fireflies want warm, wet and still**: the swamp, the wood, the
        // river bank, or reeds wherever they stand -- **but not in dry
        // country, whatever water is in it.**
        //
        // "откуда светлячки в саванне?" The water and the reeds used to
        // count anywhere, and a savanna has both: a river crosses it and a
        // waterhole sits in it, and its bank is asked as savanna (a bank
        // belongs to the country it runs through, `WorldGen::biome_from`) --
        // so every warm night in the dry belt lit a ditch up like a marsh.
        // A firefly is a creature of damp ground and leaf litter that stays
        // wet all summer; a hole the herds drink from in a sea of straw
        // grass is not that, and a sea shore is salt. So the dry belt
        // (`Zone::DryBelt`: desert and savanna) and the coast get none, a
        // pond in a meadow still does, and the wet woods do without water.
        // Rejected: dropping water from the rule altogether, which empties
        // the meadow pond and the river through a plain -- the one place a
        // player walking home at dusk actually sees them.
        let wet_woods = matches!(
            biome,
            Biome::Swamp | Biome::Bog | Biome::Forest | Biome::BirchForest | Biome::River
        );
        let dry_or_salt = matches!(biome, Biome::Desert | Biome::Savanna | Biome::Beach | Biome::Ocean);
        let damp = spot.water.is_some() || spot.plant.is_some_and(|p| types::block_kind(p) == BLOCK_REEDS);
        let firefly_country = wet_woods || (damp && !dry_or_salt);
        if firefly_country
            && firefly_hour(hour)
            && !around.wet
            && warm >= FIREFLY_WARMTH_C
            && self.count(Kind::Firefly) < MAX_FIREFLIES
        {
            // A few at a time, as they rise out of the grass together.
            let few = 1 + (self.random() * 3.0) as usize;
            for _ in 0..few {
                if self.count(Kind::Firefly) >= MAX_FIREFLIES {
                    break;
                }
                let at = top + Vec3::new(self.between(-1.5, 1.5), self.between(0.4, 2.2), self.between(-1.5, 1.5));
                let shade = (self.random() * 255.0) as u8;
                let phase = self.between(0.0, 3.0);
                self.add(Critter {
                    kind: Kind::Firefly,
                    position: at.as_dvec3(),
                    home: at,
                    state: State::Flying,
                    shade,
                    phase,
                    fade: 0.0,
                    ..self.blank(at)
                });
            }
        }
    }

    /// Every wild hive in the box round the player. See `HIVE_LOOK_EVERY`.
    fn look_for_hives(&mut self, around: &Surroundings<'_>) {
        self.hives.clear();
        let (px, py, pz) = (
            around.player.x.floor() as i32,
            around.player.y.floor() as i32,
            around.player.z.floor() as i32,
        );
        for y in py - HIVE_BELOW..=py + HIVE_ABOVE {
            for z in pz - HIVE_REACH..=pz + HIVE_REACH {
                for x in px - HIVE_REACH..=px + HIVE_REACH {
                    if let Some(block) = around.chunks.block_at(x, y, z).filter(|&b| primitive_shared::bees::is_hive(b)) {
                        self.hives.push(((x, y, z), block));
                    }
                }
            }
        }
    }

    /// One more bee round any hive that has fewer than it should, if the
    /// bees are out at all: by day, dry, and warm enough to fly
    /// (`bees::BEES_FLY_C`, the line the server stings by).
    fn send_out_bees(&mut self, around: &Surroundings<'_>, hour: f32) {
        if !bee_hour(hour) || around.wet {
            return;
        }
        for index in 0..self.hives.len() {
            if self.count(Kind::Bee) >= MAX_BEES {
                return;
            }
            let ((x, y, z), block) = self.hives[index];
            let (_, temperature, swing) = (around.country)(x, y, z);
            if warmth(temperature, around.world_time, swing) < primitive_shared::bees::BEES_FLY_C {
                continue;
            }
            let hive = Vec3::new(x as f32 + 0.5, y as f32 + 0.5, z as f32 + 0.5);
            let wanted = if primitive_shared::bees::honey_in(block) < primitive_shared::bees::HIVE_FULL {
                BEES_ANGRY
            } else {
                BEES_CALM
            };
            let here = self.live.iter().filter(|c| c.kind == Kind::Bee && c.home == hive && !c.leaving).count();
            if here >= wanted {
                continue;
            }
            // Out of the underside, where the comb is open.
            let at = hive + Vec3::new(self.between(-0.3, 0.3), -0.6, self.between(-0.3, 0.3));
            let shade = (self.random() * 255.0) as u8;
            self.add(Critter {
                kind: Kind::Bee,
                position: at.as_dvec3(),
                home: hive,
                target: at,
                state: State::Flying,
                shade,
                calling: wanted == BEES_ANGRY,
                fade: 0.0,
                ..self.blank(at)
            });
        }
    }

    fn blank(&self, at: Vec3) -> Critter {
        Critter {
            id: 0,
            kind: Kind::Firefly,
            position: at.as_dvec3(),
            velocity: Vec3::ZERO,
            home: at,
            target: at,
            facing: 0.0,
            state: State::Resting,
            timer: 0.0,
            retarget: 0.0,
            phase: 0.0,
            shade: 0,
            fade: 1.0,
            leaving: false,
            calling: false,
            hushed: 0.0,
            via: at,
        }
    }

    fn add(&mut self, mut critter: Critter) {
        self.next_id = self.next_id.wrapping_add(1);
        critter.id = self.next_id;
        self.live.push(critter);
    }

    fn butterfly(&mut self, c: &mut Critter, around: &Surroundings<'_>, hour: f32, wariness: f32, dt: f32) -> bool {
        if !butterfly_hour(hour) || around.wet || self.warm_here < BUTTERFLY_WARMTH_C {
            c.leaving = true;
        }
        if c.leaving {
            // Up and away over a couple of seconds, thinning as it goes.
            c.velocity = c.velocity.lerp(Vec3::new(c.velocity.x, 1.2, c.velocity.z), (dt * 2.0).min(1.0));
            c.position += (c.velocity * dt).as_dvec3();
            c.fade -= dt * 0.5;
            return c.fade > 0.0;
        }
        c.fade = (c.fade + dt * 2.0).min(1.0);

        let near = nearest(c.position.as_vec3(), std::slice::from_ref(&around.player));
        if near.0 < BUTTERFLY_WARY * wariness && c.state != State::Fleeing {
            let away = (c.position.as_vec3() - near.1).with_y(0.0).normalize_or(Vec3::X);
            c.state = State::Fleeing;
            c.timer = 2.5;
            c.target = c.position.as_vec3() + away * 5.0 + Vec3::new(0.0, 1.5, 0.0);
        }

        match c.state {
            State::Resting => {
                c.velocity = Vec3::ZERO;
                c.position = c.home.as_dvec3();
                c.timer -= dt;
                if c.timer <= 0.0 {
                    c.state = State::Flying;
                    c.timer = self.between(3.0, 8.0);
                    c.retarget = 0.0;
                }
            }
            _ => {
                c.timer -= dt;
                c.retarget -= dt;
                if c.state == State::Flying {
                    if c.timer <= 0.0 {
                        // Back to the flower, and down on it once there.
                        c.target = c.home;
                        if (c.position.as_vec3() - c.home).length() < 0.3 {
                            c.state = State::Resting;
                            c.timer = self.between(2.0, 6.0);
                            return true;
                        }
                    } else if c.retarget <= 0.0 {
                        c.retarget = self.between(0.5, 1.2);
                        c.target = c.home
                            + Vec3::new(self.between(-2.5, 2.5), self.between(0.3, 1.6), self.between(-2.5, 2.5));
                    }
                } else if c.timer <= 0.0 {
                    c.state = State::Flying;
                    c.timer = self.between(3.0, 6.0);
                }
                let speed = if c.state == State::Fleeing { 3.0 } else { 1.4 };
                let wanted = (c.target - c.position.as_vec3()).normalize_or_zero() * speed;
                // **A flutter, not a flight line.** A butterfly that flew
                // straight at its target would be a small bird.
                let flutter = Vec3::new(self.between(-1.0, 1.0), self.between(-1.0, 1.0), self.between(-1.0, 1.0));
                c.velocity += flutter * 6.0 * dt;
                c.velocity = c.velocity.lerp(wanted, (dt * 3.0).min(1.0));
                let next = c.position + (c.velocity * dt).as_dvec3();
                if solid(around.chunks, next.as_vec3()) {
                    c.velocity.y = 1.0;
                    c.position.y += f64::from(dt);
                } else {
                    c.position = next;
                }
                if c.velocity.length_squared() > 1e-4 {
                    c.facing = c.velocity.z.atan2(c.velocity.x);
                }
            }
        }
        true
    }

    fn firefly(&mut self, c: &mut Critter, around: &Surroundings<'_>, hour: f32, dt: f32) -> bool {
        if !firefly_hour(hour) || around.wet || self.warm_here < FIREFLY_WARMTH_C {
            c.leaving = true;
        }
        if c.leaving {
            c.fade -= dt * 0.6;
            return c.fade > 0.0;
        }
        c.fade = (c.fade + dt).min(1.0);
        let drift = Vec3::new(self.between(-1.0, 1.0), self.between(-1.0, 1.0), self.between(-1.0, 1.0));
        c.velocity += drift * 0.9 * dt;
        // Held loosely round the place it rose from, so a cloud of them
        // stays over the ditch rather than dispersing into the night.
        c.velocity += (c.home - c.position.as_vec3()) * 0.15 * dt;
        let (distance, from) = nearest(c.position.as_vec3(), std::slice::from_ref(&around.player));
        if distance < 1.6 {
            c.velocity += (c.position.as_vec3() - from).normalize_or(Vec3::Y) * 1.5 * dt;
        }
        c.velocity = c.velocity.clamp_length_max(0.45);
        let next = c.position + (c.velocity * dt).as_dvec3();
        if solid(around.chunks, next.as_vec3()) {
            c.velocity = -c.velocity;
        } else {
            c.position = next;
        }
        true
    }

    /// A bee: darting round its hive, and at the nearest player if the hive
    /// was robbed and they are standing at it.
    ///
    /// `calling` is borrowed for "this hive is angry", which a bee has no
    /// voice for otherwise; it is set when the bee comes out and read here.
    fn bee(&mut self, c: &mut Critter, around: &Surroundings<'_>, hour: f32, dt: f32) -> bool {
        let hive_cell = (c.home.x.floor() as i32, c.home.y.floor() as i32, c.home.z.floor() as i32);
        let hive_there = self.hives.iter().any(|&(at, _)| at == hive_cell);
        if !bee_hour(hour) || around.wet || !hive_there {
            c.leaving = true;
        }
        if c.leaving {
            // Back into the comb, or -- the hive gone -- off and away.
            let home = if hive_there { c.home } else { c.position.as_vec3() + Vec3::Y };
            c.position = c.position.lerp(home.as_dvec3(), f64::from((dt * 2.0).min(1.0)));
            c.fade -= dt * 0.8;
            return c.fade > 0.0;
        }
        c.fade = (c.fade + dt * 2.0).min(1.0);
        c.timer -= dt;
        if c.timer <= 0.0 {
            c.timer = self.between(0.15, 0.45);
            let head = around.player + Vec3::new(0.0, 1.5, 0.0);
            let at_the_hive = (head - c.home).length() < BEE_ANGRY_REACH;
            c.target = if c.calling && at_the_hive {
                head + Vec3::new(self.between(-0.5, 0.5), self.between(-0.4, 0.4), self.between(-0.5, 0.5))
            } else {
                c.home + Vec3::new(self.between(-1.3, 1.3), self.between(-1.2, 0.6), self.between(-1.3, 1.3))
            };
        }
        let wanted = (c.target - c.position.as_vec3()).normalize_or_zero() * 2.6;
        c.velocity = c.velocity.lerp(wanted, (dt * 6.0).min(1.0));
        let next = c.position + (c.velocity * dt).as_dvec3();
        if solid(around.chunks, next.as_vec3()) {
            c.velocity = -c.velocity * 0.5;
            c.timer = 0.0;
        } else {
            c.position = next;
        }
        if c.velocity.length_squared() > 1e-4 {
            c.facing = c.velocity.z.atan2(c.velocity.x);
        }
        true
    }

    /// Every dark hollow under the ground in the box round the player, or a
    /// fair sample of them. See `BAT_SEARCH_EVERY`.
    fn search_for_hollows(&mut self, around: &Surroundings<'_>) {
        self.hollows.clear();
        let (px, py, pz) = (
            around.player.x.floor() as i32,
            around.player.y.floor() as i32,
            around.player.z.floor() as i32,
        );
        let mut seen = 0usize;
        for y in py - BAT_BELOW..=py + BAT_ABOVE {
            for z in pz - BAT_REACH..=pz + BAT_REACH {
                for x in px - BAT_REACH..=px + BAT_REACH {
                    if !hollow(around.chunks, around.light, x, y, z) {
                        continue;
                    }
                    seen += 1;
                    // **A reservoir, so every hollow in the box has the same
                    // chance of being kept** -- the first forty-eight found
                    // would be the bottom layer of the box every time, and
                    // every colony would roost at the floor of the search.
                    if self.hollows.len() < BAT_HOLLOWS_KEPT {
                        self.hollows.push((x, y, z));
                    } else {
                        let slot = (self.random() * seen as f32) as usize;
                        if slot < BAT_HOLLOWS_KEPT {
                            self.hollows[slot] = (x, y, z);
                        }
                    }
                }
            }
        }
    }

    /// Puts a colony down in a hollow near a player who is underground, or --
    /// at dusk -- lets a group out of a hollow that has a way out.
    fn rouse_bats(&mut self, around: &Surroundings<'_>, hour: f32) {
        if self.hollows.is_empty() || self.count(Kind::Bat) + BAT_COLONY.1 > MAX_BATS {
            return;
        }
        let slot = ((self.random() * self.hollows.len() as f32) as usize).min(self.hollows.len() - 1);
        let pick = self.hollows[slot];
        let roost = Vec3::new(pick.0 as f32 + 0.5, pick.1 as f32 + 0.5, pick.2 as f32 + 0.5);
        let few = BAT_COLONY.0 + ((self.random() * (BAT_COLONY.1 - BAT_COLONY.0 + 1) as f32) as usize).min(BAT_COLONY.1 - BAT_COLONY.0);

        if bat_dusk(hour) {
            if self.bat_stream_in > 0.0 {
                return;
            }
            self.bat_stream_in = self.between(BAT_STREAM_EVERY.0, BAT_STREAM_EVERY.1);
            let Some((mouth, out)) = way_out(around.chunks, around.light, pick) else {
                return;
            };
            for _ in 0..few {
                let at = roost + Vec3::new(self.between(-0.3, 0.3), self.between(-0.2, 0.2), self.between(-0.3, 0.3));
                let home = out + Vec3::new(self.between(-3.0, 3.0), self.between(-1.0, 2.0), self.between(-3.0, 3.0));
                let timer = self.between(BAT_HUNTS.0, BAT_HUNTS.1);
                let shade = (self.random() * 255.0) as u8;
                self.add(Critter {
                    kind: Kind::Bat,
                    position: at.as_dvec3(),
                    home,
                    via: mouth,
                    target: mouth,
                    state: State::Emerging,
                    timer,
                    shade,
                    fade: 0.0,
                    ..self.blank(at)
                });
            }
            return;
        }

        let head = around.player + Vec3::Y * 1.5;
        let in_the_dark = sky_light(around.light, head).is_some_and(|sky| sky <= BAT_PLAYER_IN_THE_DARK);
        if !in_the_dark {
            return;
        }
        // One colony to a hollow: not a second put down on top of the first.
        if self.live.iter().any(|c| c.kind == Kind::Bat && c.state != State::Emerging && (c.home - roost).length() < 8.0) {
            return;
        }
        let hang = ceiling_over(around.chunks, pick);
        for _ in 0..few {
            let jitter = Vec3::new(self.between(-0.35, 0.35), 0.0, self.between(-0.35, 0.35));
            let shade = (self.random() * 255.0) as u8;
            let timer = self.between(2.0, 5.0);
            let spot = hang.filter(|_| bat_roosting_hour(hour)).and_then(|hang| self.free_hang(hang, u32::MAX));
            let (position, state) = match spot {
                Some(spot) => (spot, State::Resting),
                None => (roost + jitter, State::Flying),
            };
            self.add(Critter {
                kind: Kind::Bat,
                position: position.as_dvec3(),
                home: roost,
                target: roost,
                state,
                timer,
                shade,
                fade: 0.0,
                ..self.blank(position)
            });
        }
    }

    /// **A place on the roof at `hang` that no bat but `me` hangs within
    /// `BAT_HANG_APART` of**: the nearest free one of a grid across the
    /// cell's ceiling, `None` when every one is taken.
    ///
    /// A spot a random step from the middle was two bats in one place often
    /// enough to be seen -- a colony of six on a roof two thirds of a block
    /// across -- and a bat coming home after dark went to the same roof as
    /// every other.
    fn free_hang(&mut self, hang: Vec3, me: u32) -> Option<Vec3> {
        const STEPS: i32 = 2;
        let spots: Vec<Vec3> = (-STEPS..=STEPS)
            .flat_map(|i| (-STEPS..=STEPS).map(move |j| hang + Vec3::new(i as f32, 0.0, j as f32) * BAT_HANG_APART * 1.1))
            .collect();
        // Nearest the middle first, ties broken at random so a colony is not
        // always the same shape.
        let nudge: Vec<f32> = (0..spots.len()).map(|_| self.random() * 0.05).collect();
        let mut order: Vec<usize> = (0..spots.len()).collect();
        order.sort_by(|&a, &b| ((spots[a] - hang).length() + nudge[a]).total_cmp(&((spots[b] - hang).length() + nudge[b])));
        let taken: Vec<Vec3> = self
            .live
            .iter()
            .filter(|o| o.kind == Kind::Bat && o.id != me && o.state == State::Resting)
            .map(|o| o.position.as_vec3())
            .collect();
        order.into_iter().map(|k| spots[k]).find(|spot| taken.iter().all(|t| (*t - *spot).length() >= BAT_HANG_APART))
    }

    /// A bat: hanging in its hollow by day, flitting about it by night, up
    /// and away from anybody who comes near, and out of the cave at dusk.
    fn bat(&mut self, c: &mut Critter, around: &Surroundings<'_>, hour: f32, dt: f32) -> bool {
        if c.leaving {
            c.velocity = c.velocity.lerp(c.velocity.normalize_or(Vec3::X) * BAT_SPEED, (dt * 2.0).min(1.0));
            let next = c.position + (c.velocity * dt).as_dvec3();
            if !solid(around.chunks, next.as_vec3()) {
                c.position = next;
            }
            c.fade -= dt * 0.6;
            return c.fade > 0.0;
        }
        c.fade = (c.fade + dt * 1.5).min(1.0);
        let wary = if around.lantern { BAT_WARY_OF_A_TORCH } else { BAT_WARY };
        let head = around.player + Vec3::Y * 1.5;
        let from_player = (c.position.as_vec3() - head).length();
        let home = (c.home.x.floor() as i32, c.home.y.floor() as i32, c.home.z.floor() as i32);

        match c.state {
            State::Emerging => {
                c.timer -= dt;
                if c.timer <= 0.0 {
                    c.leaving = true;
                    return true;
                }
                if !c.calling {
                    // Along the gallery to the mouth, then out to the open.
                    if c.target == c.via && (c.position.as_vec3() - c.via).length() < 1.2 {
                        c.target = c.home;
                    } else if c.target == c.home && (c.position.as_vec3() - c.home).length() < 2.0 {
                        c.calling = true;
                    }
                } else {
                    // Out: hunting in loops over the ground by the cave.
                    c.retarget -= dt;
                    if c.retarget <= 0.0 {
                        c.retarget = self.between(0.4, 1.0);
                        c.target =
                            c.home + Vec3::new(self.between(-8.0, 8.0), self.between(-1.5, 3.0), self.between(-8.0, 8.0));
                    }
                }
            }
            State::Resting => {
                c.velocity = Vec3::ZERO;
                let lit = !hollow(around.chunks, around.light, home.0, home.1, home.2);
                if from_player < wary || lit || !bat_roosting_hour(hour) {
                    c.state = State::Flying;
                    c.timer = self.between(3.0, 6.0);
                    c.retarget = 0.0;
                }
                if lit {
                    c.leaving = true;
                }
                return true;
            }
            _ => {
                // **A torch where the colony roosts sends it away for good**:
                // the hollow is not dark any more, and nothing comes back.
                if !hollow(around.chunks, around.light, home.0, home.1, home.2) {
                    c.leaving = true;
                    return true;
                }
                if from_player < wary && c.state != State::Fleeing {
                    let away = (c.position.as_vec3() - head).normalize_or(Vec3::X);
                    let refuge = c.position.as_vec3() + away * 6.0;
                    c.state = State::Fleeing;
                    c.timer = 2.5;
                    c.target = if hollow_at(around, refuge) { refuge } else { c.home };
                }
                c.timer -= dt;
                c.retarget -= dt;
                if c.state == State::Fleeing {
                    if c.timer <= 0.0 {
                        c.state = State::Flying;
                        c.timer = self.between(3.0, 6.0);
                    }
                } else if c.timer <= 0.0 && bat_roosting_hour(hour) && from_player >= wary {
                    if let Some(hang) = ceiling_over(around.chunks, home) {
                        if (c.position.as_vec3() - hang).length() < 1.5 {
                            // A place of its own on the roof, or another
                            // turn about the hollow until one is free.
                            if let Some(spot) = self.free_hang(hang, c.id) {
                                c.state = State::Resting;
                                c.position = spot.as_dvec3();
                                return true;
                            }
                            c.timer = self.between(1.0, 3.0);
                        }
                        c.target = hang;
                    }
                } else if c.retarget <= 0.0 {
                    c.retarget = self.between(0.3, 0.9);
                    // **A few tries before home, and home is not one point.**
                    // A gallery two wide refuses most of a box ten wide round
                    // its colony, and the first refusal used to send every
                    // bat to `c.home` itself -- see `BAT_ROOM`.
                    let mut wander = None;
                    for _ in 0..4 {
                        let at = c.home + Vec3::new(self.between(-5.0, 5.0), self.between(-2.0, 2.0), self.between(-5.0, 5.0));
                        if hollow_at(around, at) {
                            wander = Some(at);
                            break;
                        }
                    }
                    let near_home = c.home + Vec3::new(self.between(-0.8, 0.8), self.between(-0.4, 0.4), self.between(-0.8, 0.8));
                    c.target = wander.unwrap_or(if hollow_at(around, near_home) { near_home } else { c.home });
                }
            }
        }

        let speed = if c.state == State::Fleeing { BAT_FLEEING } else { BAT_SPEED };
        let wanted = (c.target - c.position.as_vec3()).normalize_or_zero() * speed;
        // **A jink, not a glide.** A bat changes its line several times a
        // second after insects; one that flew straight at its target would
        // be a swallow in the wrong place.
        let jink = Vec3::new(self.between(-1.0, 1.0), self.between(-1.0, 1.0), self.between(-1.0, 1.0));
        c.velocity += jink * 14.0 * dt;
        c.velocity = c.velocity.lerp(wanted, (dt * 4.0).min(1.0));
        // Clear of the others, after the pull home so the pull cannot undo
        // it (`BAT_ROOM`). Harder the closer: a bat an inch off is turned
        // away at once, one at the edge of the room barely at all.
        let me = c.position.as_vec3();
        let mut clear = Vec3::ZERO;
        for other in self.live.iter().filter(|o| o.kind == Kind::Bat && o.id != c.id && o.state != State::Resting && !o.leaving) {
            let off = me - other.position.as_vec3();
            let apart = off.length();
            if apart < BAT_ROOM {
                // Exactly on top of each other: apart along whichever way
                // the ids say, so the pair never agree on the same escape.
                let away = if apart > 1e-4 {
                    off / apart
                } else {
                    Vec3::new((c.id as f32).cos(), 0.3, (c.id as f32).sin()).normalize()
                };
                clear += away * (1.0 - apart / BAT_ROOM);
            }
        }
        c.velocity += clear * BAT_SHY * BAT_SPEED * dt;
        let next = c.position + (c.velocity * dt).as_dvec3();
        // A bat that is not on its way out keeps to the dark it lives in, so
        // it never wanders out of a cave by day on a jink.
        let too_light = c.state != State::Emerging
            && sky_light(around.light, next.as_vec3()).is_some_and(|sky| sky > BAT_DARK_SKY + 2);
        if solid(around.chunks, next.as_vec3()) || too_light {
            c.velocity = -c.velocity * 0.5;
            c.retarget = 0.0;
            if too_light {
                c.target = c.home;
            }
        } else {
            c.position = next;
        }
        if c.velocity.length_squared() > 1e-4 {
            c.facing = c.velocity.z.atan2(c.velocity.x);
        }
        true
    }

    fn frog(&mut self, c: &mut Critter, around: &Surroundings<'_>, hour: f32, wariness: f32, dt: f32) -> bool {
        match c.state {
            State::Under => {
                c.calling = false;
                c.timer -= dt;
                c.timer > 0.0
            }
            State::Hopping => {
                c.calling = false;
                c.velocity.y -= FROG_GRAVITY * dt;
                let next = c.position + (c.velocity * dt).as_dvec3();
                let cell = around.chunks.block_at(next.x.floor() as i32, next.y.floor() as i32, next.z.floor() as i32);
                if cell.is_some_and(types::is_liquid) && c.velocity.y < 0.0 {
                    c.position = next;
                    c.state = State::Under;
                    c.timer = self.between(12.0, 25.0);
                } else if cell.is_some_and(types::is_collidable) {
                    // Landed short, on the bank: it sits where it came down.
                    c.position.y = next.y.floor() + 1.0;
                    c.velocity = Vec3::ZERO;
                    c.state = State::Resting;
                } else {
                    c.position = next;
                }
                // Never further down than the water it was making for.
                if c.position.y < f64::from(c.home.y - 3.0) {
                    c.state = State::Under;
                    c.timer = 1.0;
                }
                true
            }
            _ => {
                // **Cold sends a frog into the water, and it does not come
                // back up.** The one frame a frog appeared used to be the only
                // time anything asked about the air.
                if self.warm_here < FROG_WARMTH_C {
                    c.calling = false;
                    c.state = State::Under;
                    c.timer = self.between(0.5, 2.0);
                    return true;
                }
                let (from_player, _) = nearest(c.position.as_vec3(), std::slice::from_ref(&around.player));
                let (from_beast, _) = nearest(c.position.as_vec3(), around.dangers);
                if from_player < FROG_WARY * wariness || from_beast < FROG_WARY_OF_BEASTS {
                    let toward = (c.home - c.position.as_vec3()).with_y(0.0).normalize_or_zero();
                    c.velocity = toward * HOP_ACROSS + Vec3::new(0.0, HOP_UP, 0.0);
                    c.state = State::Hopping;
                    c.calling = false;
                    c.hushed = HUSH_SECONDS;
                    return true;
                }
                if from_beast < HUSH_RADIUS || from_player < HUSH_RADIUS_PLAYER * wariness {
                    c.hushed = HUSH_SECONDS;
                }
                c.hushed = (c.hushed - dt).max(0.0);
                c.calling = (chorus_hour(hour) || around.wet) && c.hushed <= 0.0 && self.warm_here >= FROG_WARMTH_C;
                true
            }
        }
    }

    /// Everything alive, into the particle buffer.
    ///
    /// Measured from the render origin, as `Particles::build_into` is --
    /// `view_proj` is built about it, and a critter written in world
    /// coordinates is drawn the player's own altitude up in the sky.
    ///
    /// `light` is the world's light map, and a frog and a butterfly are lit
    /// from it -- see the note over `lit`.
    #[allow(clippy::too_many_arguments)]
    pub fn build_into(
        &self,
        origin: Vec3,
        right: Vec3,
        up: Vec3,
        layers: &FaceLayers,
        light: &LightMap,
        vertices: &mut Vec<ParticleVertex>,
        indices: &mut Vec<u32>,
    ) {
        // The snowflake: a round blob with a soft edge, which is what a
        // wing is at this size -- and no new layer, for the reason blood
        // and smoke wear it (see `particles::Look::Blood`).
        let blob = layers.extra(EXTRA_SNOW);
        let whole = (0.0, 0.0, 1.0, 1.0);
        // The solid middle of the flake: a flat colour, all of it tint.
        let flat = (7.5 / 16.0, 7.5 / 16.0, 8.5 / 16.0, 8.5 / 16.0);
        for c in &self.live {
            let at = (c.position - origin.as_dvec3()).as_vec3();
            // **Lit by the cell it sits in, as a thrown chip or a drop of
            // blood is** (`Particles::build_into`, `entities::sampled_light`).
            // Every critter used to carry `pack_light(15, 0, ..)` -- full sky,
            // written as a constant because butterflies were only ever out at
            // noon -- and the frogs inherited it: "лягушки игнорируют
            // освещение", a pond's edge at midnight with bright green frogs
            // on it, glowing under a bush and in the mouth of a cave. The
            // shader already darkens the sky nibble with the hour, so the
            // cell's own sky and block light is the whole of the fix; a
            // firefly is the exception, below, because it is the light.
            let lit = {
                let (sky, block) = crate::logic::entities::sampled_light(c.position, light);
                pack_light(sky, block, 3, 0)
            };
            match c.kind {
                Kind::Butterfly => {
                    let forward = Vec3::new(c.facing.cos(), 0.0, c.facing.sin());
                    let side = Vec3::new(-forward.z, 0.0, forward.x);
                    let open = if c.state == State::Resting {
                        0.25 + 0.9 * (0.5 + 0.5 * (c.phase * std::f32::consts::TAU * 0.7).sin())
                    } else {
                        0.5 + 0.9 * (c.phase * std::f32::consts::TAU * 9.0 + c.id as f32).sin()
                    };
                    let [r, g, b] = BUTTERFLY_COLOURS[c.shade as usize % BUTTERFLY_COLOURS.len()];
                    let tint = [r, g, b, c.fade.clamp(0.0, 1.0)];
                    for sign in [1.0f32, -1.0] {
                        let out = side * (sign * open.cos()) + up * open.sin();
                        let front = at + forward * 0.05;
                        let back = at - forward * 0.05;
                        quad(
                            vertices,
                            indices,
                            [front, front + out * 0.08, back + out * 0.09, back],
                            whole,
                            blob,
                            lit,
                            tint,
                        );
                    }
                }
                Kind::Firefly => {
                    let period = 2.2 + c.shade as f32 / 128.0;
                    let into = (c.phase / period).fract();
                    let lit = if into < 0.18 { (into / 0.18 * std::f32::consts::PI).sin() } else { 0.0 };
                    let glow = lit * c.fade.clamp(0.0, 1.0);
                    if glow < 0.02 {
                        continue;
                    }
                    // Lit by itself, like an ember: it is the light.
                    let own = pack_light(15, 15, 3, 0);
                    for (size, alpha) in [(0.03f32, glow), (0.11, glow * 0.35)] {
                        let (a, b) = (right * size, up * size);
                        quad(
                            vertices,
                            indices,
                            [at - a + b, at + a + b, at + a - b, at - a - b],
                            whole,
                            blob,
                            own,
                            [0.82, 1.0, 0.38, alpha],
                        );
                    }
                }
                Kind::Bee => {
                    let forward = Vec3::new(c.facing.cos(), 0.0, c.facing.sin());
                    let side = Vec3::new(-forward.z, 0.0, forward.x);
                    let fade = c.fade.clamp(0.0, 1.0);
                    // A dark gold body, a shade apart from bee to bee.
                    let tone = 0.9 + 0.2 * (c.shade as f32 / 255.0);
                    let body = [BEE_BODY[0] * tone, BEE_BODY[1] * tone, BEE_BODY[2] * tone];
                    push_box(vertices, indices, at, [0.018, 0.016, 0.03], forward, side, body, flat, blob, lit);
                    // Wings a blur: a pale quad each side, beating too fast to
                    // be anything but a flicker in its alpha.
                    let beat = 0.25 + 0.2 * (c.phase * 90.0 + c.id as f32).sin();
                    for sign in [1.0f32, -1.0] {
                        let out = side * sign * 0.05 + up * 0.02;
                        quad(
                            vertices,
                            indices,
                            [at + forward * 0.02, at + forward * 0.02 + out, at - forward * 0.02 + out, at - forward * 0.02],
                            whole,
                            blob,
                            lit,
                            [0.92, 0.94, 0.96, beat * fade],
                        );
                    }
                }
                Kind::Bat => {
                    let forward = Vec3::new(c.facing.cos(), 0.0, c.facing.sin());
                    let side = Vec3::new(-forward.z, 0.0, forward.x);
                    let fade = c.fade.clamp(0.0, 1.0);
                    let tone = 0.85 + 0.3 * (c.shade as f32 / 255.0);
                    let body = [BAT_BODY[0] * tone, BAT_BODY[1] * tone, BAT_BODY[2] * tone];
                    if c.state == State::Resting {
                        // Hanging from the roof, wings wrapped: a dark drop.
                        push_box(vertices, indices, at, [0.03, 0.055, 0.03], forward, side, body, flat, blob, lit);
                        continue;
                    }
                    push_box(vertices, indices, at, [0.024, 0.02, 0.042], forward, side, body, flat, blob, lit);
                    // Wings wider than the body is long, beating fast and
                    // deep -- the flicker of a bat against a dusk sky.
                    let open = 0.95 * (c.phase * std::f32::consts::TAU * 7.0 + c.id as f32).sin();
                    let [r, g, b] = BAT_WING;
                    for sign in [1.0f32, -1.0] {
                        let out = side * (sign * open.cos()) + up * open.sin();
                        quad(
                            vertices,
                            indices,
                            [at + forward * 0.04, at + forward * 0.02 + out * 0.15, at - forward * 0.05 + out * 0.11, at - forward * 0.04],
                            flat,
                            blob,
                            lit,
                            [r * tone, g * tone, b * tone, fade],
                        );
                    }
                }
                Kind::Frog => {
                    if c.state == State::Under {
                        continue;
                    }
                    let forward = Vec3::new(c.facing.cos(), 0.0, c.facing.sin());
                    let side = Vec3::new(-forward.z, 0.0, forward.x);
                    let body = at + Vec3::new(0.0, 0.05, 0.0);
                    push_box(vertices, indices, body, [0.08, 0.05, 0.1], forward, side, FROG_BODY, flat, blob, lit);
                    for sign in [1.0f32, -1.0] {
                        let eye = body + forward * 0.06 + side * (sign * 0.05) + Vec3::new(0.0, 0.055, 0.0);
                        push_box(vertices, indices, eye, [0.022, 0.022, 0.022], forward, side, FROG_EYE, flat, blob, lit);
                    }
                }
            }
        }
    }
}

/// A cabbage white, a brimstone, a blue and a small copper.
const BUTTERFLY_COLOURS: [[f32; 3]; 4] = [
    [0.96, 0.95, 0.86],
    [0.98, 0.84, 0.30],
    [0.52, 0.66, 0.98],
    [0.86, 0.42, 0.18],
];

/// A honeybee, at the size it is drawn: the gold and the black stripes are
/// one dark amber at a pixel.
const BEE_BODY: [f32; 3] = [0.52, 0.38, 0.08];

/// A bat's fur, and the thinner, browner skin of its wings.
const BAT_BODY: [f32; 3] = [0.17, 0.14, 0.13];
const BAT_WING: [f32; 3] = [0.24, 0.19, 0.17];

/// A pond frog's back, and the gold of its eye.
const FROG_BODY: [f32; 3] = [0.36, 0.50, 0.20];
const FROG_EYE: [f32; 3] = [0.80, 0.72, 0.30];

/// Degrees over freezing, here and now: the climate, and the season scaled
/// to the latitude's share of it -- the same scaling the snow line takes
/// (`season::falls_as_snow_with_swing`), so a firefly is never out on a
/// night the snow says is winter.
fn warmth(climate_temperature: f32, world_time: f32, swing: f32) -> f32 {
    (climate_temperature - primitive_shared::worldgen::CLIMATE_FREEZING) * primitive_shared::season::CLIMATE_SPAN_C
        + primitive_shared::season::ambient_offset_c(world_time) * swing
}

/// How much colder than the day's warmth the air is at this hour, at most.
///
/// The server's night is `climate::NIGHT_DROP_C` (seventeen) damped by
/// humidity to between seven and seventeen; the client is not told the
/// humidity of a column, so it takes twelve, the middle. A number of its own
/// rather than a shared one because the server's also carries shelter and a
/// pre-dawn chill that a frog at a pond has no use for -- this only has to
/// put the silence on the right side of a cold night.
const NIGHT_COOLING_C: f32 = 12.0;

/// `warmth`, with the night taken off it: what a frog at a pond feels now.
fn warmth_now(climate_temperature: f32, world_time: f32, swing: f32) -> f32 {
    let hour = world_time.rem_euclid(1.0);
    // 1 at noon, 0 at midnight -- the server's own curve.
    let sun = 0.5 - 0.5 * (std::f32::consts::TAU * hour).cos();
    warmth(climate_temperature, world_time, swing) - NIGHT_COOLING_C * (1.0 - sun)
}

/// From an hour after dawn to dusk: when a bee forages.
fn bee_hour(hour: f32) -> bool {
    (0.25..0.78).contains(&hour)
}

/// Mid-morning to mid-afternoon: the warm of the day.
fn butterfly_hour(hour: f32) -> bool {
    (0.3..0.7).contains(&hour)
}

/// From dusk until well into the night. The server's night is from 0.75.
fn firefly_hour(hour: f32) -> bool {
    !(0.2..0.74).contains(&hour)
}

/// When the frogs call: from the evening to the small hours.
fn chorus_hour(hour: f32) -> bool {
    !(0.27..0.7).contains(&hour)
}

/// When a colony pours out of its cave: from sunset into the first of the
/// dark. The server's night is from 0.75.
fn bat_dusk(hour: f32) -> bool {
    (0.72..0.84).contains(&hour)
}

/// When a bat hangs in its roost: the day.
fn bat_roosting_hour(hour: f32) -> bool {
    (0.25..0.72).contains(&hour)
}

/// The sky light at a point, or `None` where the light map has not reached
/// the chunk -- which is not dark, whatever a zero would say.
fn sky_light(light: &LightMap, at: Vec3) -> Option<u8> {
    light
        .is_lit(primitive_shared::types::ChunkPos::from_world(at.x, at.z))
        .then(|| light.sky(at.x.floor() as i32, at.y.floor() as i32, at.z.floor() as i32))
}

/// Is this cell a dark hollow under the ground: air, no daylight to speak
/// of, no lamp, and `BAT_ROOF` courses of solid ground over it?
///
/// **Asked of the light map, not of the generator**, because the generator
/// does not know what a player has dug: a mine a player has cut is a cave,
/// and the colony finds it; a torch put up in it is light, and the colony
/// leaves.
fn hollow(chunks: &ChunkManager, light: &LightMap, x: i32, y: i32, z: i32) -> bool {
    if !chunks.block_at(x, y, z).is_some_and(types::is_air) {
        return false;
    }
    if !light.is_lit(primitive_shared::types::ChunkPos::from_world(x as f32, z as f32)) {
        return false;
    }
    if light.sky(x, y, z) > BAT_DARK_SKY || light.block(x, y, z) > BAT_DARK_BLOCK {
        return false;
    }
    (1..=BAT_ROOF_LOOK).filter(|dy| chunks.block_at(x, y + dy, z).is_some_and(types::is_collidable)).count() >= BAT_ROOF
}

fn hollow_at(around: &Surroundings<'_>, at: Vec3) -> bool {
    hollow(around.chunks, around.light, at.x.floor() as i32, at.y.floor() as i32, at.z.floor() as i32)
}

/// Where a bat hangs over a hollow: just under the first solid cell above it.
fn ceiling_over(chunks: &ChunkManager, (x, y, z): (i32, i32, i32)) -> Option<Vec3> {
    (1..=BAT_ROOF_LOOK)
        .find(|dy| chunks.block_at(x, y + dy, z).is_some_and(types::is_collidable))
        .map(|dy| Vec3::new(x as f32 + 0.5, (y + dy) as f32 - 0.12, z as f32 + 0.5))
}

/// The way out of a hollow, if it has one in a straight line: the mouth --
/// the first cell along a gallery that the sky reaches (`BAT_MOUTH_SKY`) --
/// and a point in the open air past it.
///
/// **Straight lines along the four ways, and not a search through the
/// cave.** A walk that followed every bend would find the way out of any cave
/// with one, at the price of a flood fill each dusk; this finds the mouths of
/// galleries that run into a hillside, which is most of the mouths a player
/// on the surface can see bats come out of. A cave whose only way out is a
/// shaft or a bend lets nothing out at dusk -- which nobody can tell from a
/// cave with no colony in it.
fn way_out(chunks: &ChunkManager, light: &LightMap, (x, y, z): (i32, i32, i32)) -> Option<(Vec3, Vec3)> {
    let centre = |x: i32, y: i32, z: i32| Vec3::new(x as f32 + 0.5, y as f32 + 0.5, z as f32 + 0.5);
    let air = |x: i32, y: i32, z: i32| chunks.block_at(x, y, z).is_some_and(types::is_air);
    for (dx, dz) in [(1, 0), (-1, 0), (0, 1), (0, -1)] {
        for k in 1..=BAT_WAY_OUT {
            let (mx, mz) = (x + dx * k, z + dz * k);
            if !air(mx, y, mz) {
                break;
            }
            if light.sky(mx, y, mz) < BAT_MOUTH_SKY {
                continue;
            }
            let mouth = centre(mx, y, mz);
            let mut out = mouth + Vec3::new(dx as f32, 0.0, dz as f32) * 3.0;
            for j in 1..=6 {
                let (ox, oz) = (mx + dx * j, mz + dz * j);
                if !air(ox, y, oz) {
                    break;
                }
                if light.sky(ox, y, oz) >= 15 {
                    out = centre(ox, y, oz);
                    break;
                }
            }
            return Some((mouth, out + Vec3::Y * 3.0));
        }
    }
    None
}

/// The nearest of some points, and how far.
fn nearest(at: Vec3, points: &[Vec3]) -> (f32, Vec3) {
    points
        .iter()
        .map(|&p| ((p - at).length(), p))
        .min_by(|a, b| a.0.total_cmp(&b.0))
        .unwrap_or((f32::INFINITY, at))
}

fn solid(chunks: &ChunkManager, at: Vec3) -> bool {
    chunks
        .block_at(at.x.floor() as i32, at.y.floor() as i32, at.z.floor() as i32)
        .is_some_and(types::is_collidable)
}

/// What one column near the player holds.
struct Spot {
    /// The cell of the ground block itself.
    ground_y: i32,
    /// What grows in the cell over it, if anything does.
    plant: Option<BlockId>,
    /// A column beside it whose cell at the ground's level is water: the
    /// bank of something.
    water: Option<(i32, i32)>,
}

/// Reads one column from a few blocks over the player's feet downward.
///
/// `None` for water, leaves, unloaded ground and nothing at all: none of
/// the three lives on top of a lake or a canopy.
fn look_at(chunks: &ChunkManager, x: i32, z: i32, from_y: i32) -> Option<Spot> {
    let mut plant = None;
    for y in (from_y - PROBE_DOWN..=from_y + PROBE_UP).rev() {
        let block = chunks.block_at(x, y, z)?;
        if types::is_air(block) {
            continue;
        }
        if types::is_liquid(block) || types::is_leafy(block) {
            return None;
        }
        // **What is walked through is a plant; what is stood on is the
        // ground.** Asked of the collider rather than of `is_foliage`, which
        // is a grazing animal's word -- it says what a deer puts its head
        // down for, turf included -- and taking the turf for a plant read
        // every bank a cell too low, where there is no water beside it.
        if !types::is_collidable(block) {
            if plant.is_none() {
                plant = Some(block);
            }
            continue;
        }
        let water = [(1, 0), (-1, 0), (0, 1), (0, -1)]
            .into_iter()
            .map(|(dx, dz)| (x + dx, z + dz))
            .find(|&(wx, wz)| chunks.block_at(wx, y, wz).is_some_and(types::is_liquid));
        return Some(Spot { ground_y: y, plant, water });
    }
    None
}

/// One quad, both windings, as `Particles::build_into` writes them. The
/// breeze draws with it too (`engine::breeze`).
pub(crate) fn quad(
    vertices: &mut Vec<ParticleVertex>,
    indices: &mut Vec<u32>,
    corners: [Vec3; 4],
    (u0, v0, u1, v1): (f32, f32, f32, f32),
    layer: u32,
    light: u32,
    tint: [f32; 4],
) {
    let base = vertices.len() as u32;
    for (corner, uv) in corners.iter().zip([[u0, v0], [u1, v0], [u1, v1], [u0, v1]]) {
        vertices.push(ParticleVertex {
            position: [corner.x, corner.y, corner.z],
            uv,
            packed: (layer << 16) | light,
            tint,
        });
    }
    indices.extend_from_slice(&[
        base,
        base + 1,
        base + 2,
        base,
        base + 2,
        base + 3,
        base,
        base + 2,
        base + 1,
        base,
        base + 3,
        base + 2,
    ]);
}

/// A small box, turned to face `forward`, its faces shaded by hand.
///
/// **Shaded in the tint rather than by the light word**: the particle
/// shader lights a quad as a speck with no face to it, so a box whose six
/// sides came out the same colour would be a green silhouette. Top full,
/// sides three quarters, underneath a half -- the terrain's own rule of
/// thumb for which way a face points.
#[allow(clippy::too_many_arguments)]
fn push_box(
    vertices: &mut Vec<ParticleVertex>,
    indices: &mut Vec<u32>,
    centre: Vec3,
    half: [f32; 3],
    forward: Vec3,
    side: Vec3,
    colour: [f32; 3],
    uv: (f32, f32, f32, f32),
    layer: u32,
    light: u32,
) {
    let (x, y, z) = (side * half[0], Vec3::Y * half[1], forward * half[2]);
    let corner = |sx: f32, sy: f32, sz: f32| centre + x * sx + y * sy + z * sz;
    let shade = |amount: f32| [colour[0] * amount, colour[1] * amount, colour[2] * amount, 1.0];
    let faces: [([Vec3; 4], f32); 6] = [
        ([corner(-1., 1., -1.), corner(1., 1., -1.), corner(1., 1., 1.), corner(-1., 1., 1.)], 1.0),
        ([corner(-1., -1., -1.), corner(1., -1., -1.), corner(1., -1., 1.), corner(-1., -1., 1.)], 0.5),
        ([corner(1., -1., -1.), corner(1., 1., -1.), corner(1., 1., 1.), corner(1., -1., 1.)], 0.78),
        ([corner(-1., -1., -1.), corner(-1., 1., -1.), corner(-1., 1., 1.), corner(-1., -1., 1.)], 0.78),
        ([corner(-1., -1., 1.), corner(1., -1., 1.), corner(1., 1., 1.), corner(-1., 1., 1.)], 0.7),
        ([corner(-1., -1., -1.), corner(1., -1., -1.), corner(1., 1., -1.), corner(-1., 1., -1.)], 0.7),
    ];
    for (corners, amount) in faces {
        quad(vertices, indices, corners, uv, layer, light, shade(amount));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use primitive_shared::types::{Chunk, ChunkPos, BLOCK_AIR, BLOCK_GRASS, BLOCK_STONE, BLOCK_WATER, CHUNK_VOLUME};

    /// Grass at y = 10 over stone, nine chunks of it; a four-by-four pond
    /// at x, z in 0..4 if asked for; a flower on every third cell if asked
    /// for.
    fn meadow(pond: bool, flowers: bool) -> ChunkManager {
        meadow_with(pond, flowers, &[])
    }

    /// `meadow`, with some cells written over it afterwards.
    fn meadow_with(pond: bool, flowers: bool, cells: &[((i32, i32, i32), BlockId)]) -> ChunkManager {
        let mut chunks = ChunkManager::new(4);
        for cx in -1..=1 {
            for cz in -1..=1 {
                let mut blocks = vec![BLOCK_AIR; CHUNK_VOLUME];
                for lz in 0..16usize {
                    for lx in 0..16usize {
                        let (x, z) = (cx * 16 + lx as i32, cz * 16 + lz as i32);
                        for y in 0..10 {
                            blocks[Chunk::index(lx, y, lz)] = BLOCK_STONE;
                        }
                        let water = pond && (0..4).contains(&x) && (0..4).contains(&z);
                        blocks[Chunk::index(lx, 10, lz)] = if water { BLOCK_WATER } else { BLOCK_GRASS };
                        if flowers && !water && (x + 2 * z).rem_euclid(3) == 0 {
                            blocks[Chunk::index(lx, 11, lz)] = BLOCK_FLOWER;
                        }
                        for &((cx_, cy_, cz_), block) in cells {
                            if (cx_, cz_) == (x, z) {
                                blocks[Chunk::index(lx, cy_ as usize, lz)] = block;
                            }
                        }
                    }
                }
                chunks.insert(Chunk { pos: ChunkPos::new(cx, cz), blocks });
            }
        }
        chunks
    }

    /// The generator's climate, well into warm.
    fn warm() -> f32 {
        primitive_shared::worldgen::CLIMATE_FREEZING + 0.5
    }

    /// A world time in the middle of summer, at an hour.
    fn summer(hour: f32) -> f32 {
        primitive_shared::season::MIDSUMMER_DAY_OF_YEAR.floor() - primitive_shared::season::WORLD_OPENS_ON_DAY + hour
    }

    #[allow(clippy::too_many_arguments)]
    fn run(critters: &mut Critters, chunks: &ChunkManager, player: Vec3, time: f32, dangers: &[Vec3], temperature: f32, seconds: f32, mut each: impl FnMut(&Critters)) {
        let country = move |_: i32, _: i32, _: i32| (Biome::Swamp, temperature, 1.0);
        let frames = (seconds * 30.0) as usize;
        for frame in 0..frames {
            let around = Surroundings {
                chunks,
                player,
                world_time: time + frame as f32 / 30.0 / 1200.0,
                wet: false,
                dangers,
                country: &country,
                light: &LightMap::new(),
                lantern: false,
            };
            critters.update(&around, 1.0 / 30.0);
            each(critters);
        }
    }

    fn beside_water(chunks: &ChunkManager, at: Vec3) -> bool {
        let (x, y, z) = (at.x.floor() as i32, at.y.floor() as i32 - 1, at.z.floor() as i32);
        [(1, 0), (-1, 0), (0, 1), (0, -1)]
            .into_iter()
            .any(|(dx, dz)| chunks.block_at(x + dx, y, z + dz).is_some_and(types::is_liquid))
    }

    #[test]
    fn frogs_only_live_near_water() {
        // At a pond's edge, and never out in a dry meadow however long it
        // is watched -- a frog in a field is a frog that came from nowhere.
        let pond = meadow(true, false);
        let mut critters = Critters::new();
        let mut frogs = 0;
        run(&mut critters, &pond, Vec3::new(14.0, 11.0, 2.0), summer(0.9), &[], warm(), 60.0, |c| {
            for frog in c.live.iter().filter(|f| f.kind == Kind::Frog && f.state == State::Resting) {
                assert!(beside_water(&pond, frog.position.as_vec3()), "a frog sits at {:?}, away from the water", frog.position.as_vec3());
            }
            frogs = frogs.max(c.count(Kind::Frog));
        });
        assert!(frogs > 0, "a warm pond at night had no frogs at all");

        let dry = meadow(false, false);
        let mut critters = Critters::new();
        run(&mut critters, &dry, Vec3::new(2.0, 11.0, 2.0), summer(0.9), &[], warm(), 60.0, |c| {
            assert_eq!(c.count(Kind::Frog), 0, "a frog in a meadow with no water in it");
        });
    }

    #[test]
    fn a_frost_sends_the_chorus_into_the_water_and_a_winter_pond_has_no_frogs() {
        // Out on a warm night, and then the same pond turns cold: every frog
        // goes quiet and under within seconds rather than singing on.
        let pond = meadow(true, false);
        let player = Vec3::new(14.0, 11.0, 2.0);
        let mut critters = Critters::new();
        let mut frogs = 0;
        run(&mut critters, &pond, player, summer(0.9), &[], warm(), 60.0, |c| frogs = frogs.max(c.count(Kind::Frog)));
        assert!(frogs > 0, "a warm pond at night had no frogs to silence");
        let cold = primitive_shared::worldgen::CLIMATE_FREEZING;
        run(&mut critters, &pond, player, summer(0.9), &[], cold, 10.0, |_| {});
        assert!(critters.croaking().is_empty(), "frogs sang on at freezing");
        assert_eq!(critters.count(Kind::Frog), 0, "a frog stayed out in the cold");

        // ...and midwinter, in a climate that is warm in summer, grows none.
        let winter = summer(0.9) + primitive_shared::season::YEAR_DAYS * 0.5;
        let mut critters = Critters::new();
        run(&mut critters, &pond, player, winter, &[], warm() - 0.2, 60.0, |c| {
            assert_eq!(c.count(Kind::Frog), 0, "a frog at a midwinter pond");
        });
    }

    #[test]
    fn bees_fly_round_a_hive_on_a_warm_day_and_twice_as_many_round_a_robbed_one_and_none_in_the_cold() {
        use primitive_shared::bees::{hive_holding, HIVE_FULL};
        let player = Vec3::new(8.0, 11.0, 12.0);
        let most = |honey: u8, temperature: f32, hour: f32| {
            let trunk = (8, 13, 8);
            let world = meadow_with(false, false, &[(trunk, types::BLOCK_LOG), ((9, 13, 8), hive_holding(honey))]);
            let mut critters = Critters::new();
            let mut most = 0;
            run(&mut critters, &world, player, summer(hour), &[], temperature, 20.0, |c| {
                for bee in c.live.iter().filter(|b| b.kind == Kind::Bee && !b.leaving) {
                    assert!(
                        (bee.position.as_vec3() - bee.home).length() < 3.5,
                        "a bee from a calm hive wandered {:.1} blocks off",
                        (bee.position.as_vec3() - bee.home).length()
                    );
                }
                most = most.max(c.count(Kind::Bee));
            });
            most
        };
        let calm = most(HIVE_FULL, warm(), 0.5);
        assert!(calm > 0, "a full hive on a summer noon had no bees round it");
        assert!(most(0, warm(), 0.5) > calm, "a robbed hive had no more bees than a full one");
        let cold = primitive_shared::worldgen::CLIMATE_FREEZING - 0.05;
        assert_eq!(most(HIVE_FULL, cold, 0.5), 0, "bees flew on a freezing day");
        assert_eq!(most(HIVE_FULL, warm(), 0.95), 0, "bees flew at night");
    }

    #[test]
    fn the_small_life_never_outnumbers_its_caps() {
        let world = meadow(true, true);
        let mut critters = Critters::new();
        for hour in [0.5, 0.85, 0.1] {
            run(&mut critters, &world, Vec3::new(8.0, 11.0, 8.0), summer(hour), &[], warm(), 40.0, |c| {
                assert!(c.count(Kind::Butterfly) <= MAX_BUTTERFLIES);
                assert!(c.count(Kind::Firefly) <= MAX_FIREFLIES);
                assert!(c.count(Kind::Frog) <= MAX_FROGS);
            });
        }
    }

    #[test]
    fn a_frog_goes_quiet_and_into_the_water_when_a_wolf_comes_near() {
        // **The warning, stated as what the player hears and sees.** A
        // calling frog at the bank, a wolf coming along it: the frog stops
        // and goes in, and so does every frog within earshot of the wolf.
        let pond = meadow(true, false);
        let mut critters = Critters::new();
        let player = Vec3::new(16.0, 11.0, 2.0);
        run(&mut critters, &pond, player, summer(0.9), &[], warm(), 30.0, |_| {});
        let caller = critters
            .live
            .iter()
            .find(|c| c.kind == Kind::Frog && c.calling)
            .copied()
            .expect("nothing calling at a warm pond after half a minute of dark");
        let wolf = [caller.position.as_vec3() + Vec3::new(0.0, 0.0, -3.0)];
        run(&mut critters, &pond, player, summer(0.9), &wolf, warm(), 1.0, |_| {});
        match critters.live.iter().find(|c| c.id == caller.id) {
            // Gone under and its time up is a frog that went in, too.
            None => {}
            Some(frog) => {
                assert!(!frog.calling, "a frog went on calling with a wolf three blocks off");
                assert!(
                    matches!(frog.state, State::Hopping | State::Under),
                    "a frog sat on the bank with a wolf three blocks off"
                );
            }
        }
        for frog in critters.live.iter().filter(|c| c.kind == Kind::Frog) {
            if (frog.position.as_vec3() - wolf[0]).length() < HUSH_RADIUS {
                assert!(!frog.calling, "a frog {:.1} blocks from the wolf is still calling", (frog.position.as_vec3() - wolf[0]).length());
            }
        }
    }

    #[test]
    fn butterflies_keep_to_the_flowers_by_day_and_are_gone_after_dark() {
        let world = meadow(false, true);
        let mut critters = Critters::new();
        let player = Vec3::new(8.0, 11.0, 8.0);
        let mut seen = 0;
        run(&mut critters, &world, player, summer(0.5), &[], warm(), 30.0, |c| {
            for b in c.live.iter().filter(|b| b.kind == Kind::Butterfly && b.state != State::Fleeing && !b.leaving) {
                let flower = b.home;
                assert!(
                    (b.position.as_vec3() - flower).with_y(0.0).length() < 4.5,
                    "a butterfly has wandered {:.1} blocks from its flower",
                    (b.position.as_vec3() - flower).with_y(0.0).length()
                );
            }
            seen = seen.max(c.count(Kind::Butterfly));
        });
        assert!(seen > 0, "a meadow in flower at noon in summer had no butterflies");
        run(&mut critters, &world, player, summer(0.9), &[], warm(), 8.0, |_| {});
        assert_eq!(critters.count(Kind::Butterfly), 0, "butterflies stayed out after dark");
    }

    #[test]
    fn fireflies_come_out_on_a_warm_evening_and_not_in_the_cold_or_the_day() {
        let world = meadow(true, false);
        let player = Vec3::new(8.0, 11.0, 8.0);
        let count = |time: f32, temperature: f32| {
            let mut critters = Critters::new();
            let mut most = 0;
            run(&mut critters, &world, player, time, &[], temperature, 20.0, |c| most = most.max(c.count(Kind::Firefly)));
            most
        };
        assert!(count(summer(0.85), warm()) > 0, "no fireflies over a warm swamp at dusk");
        assert_eq!(count(summer(0.5), warm()), 0, "fireflies at noon");
        let cold = primitive_shared::worldgen::CLIMATE_FREEZING - 0.05;
        assert_eq!(count(summer(0.85), cold), 0, "fireflies on a freezing night");
    }

    #[test]
    fn a_butterfly_keeps_its_distance_from_someone_who_runs_at_it() {
        let world = meadow(false, true);
        let mut critters = Critters::new();
        run(&mut critters, &world, Vec3::new(8.0, 11.0, 8.0), summer(0.5), &[], warm(), 20.0, |_| {});
        let target = critters
            .live
            .iter()
            .find(|c| c.kind == Kind::Butterfly && !c.leaving)
            .copied()
            .expect("a butterfly to run at");
        let country = |_: i32, _: i32, _: i32| (Biome::Plains, warm(), 1.0);
        let start = target.position.as_vec3() + Vec3::new(-6.0, 0.0, 0.0);
        let mut fled = false;
        for frame in 0..60 {
            // Seven blocks a second, straight at where it was.
            let player = start + Vec3::new(7.0 * frame as f32 / 30.0, 0.0, 0.0);
            let around = Surroundings {
                chunks: &world,
                player,
                world_time: summer(0.5),
                wet: false,
                dangers: &[],
                country: &country,
                light: &LightMap::new(),
                lantern: false,
            };
            critters.update(&around, 1.0 / 30.0);
            if let Some(b) = critters.live.iter().find(|c| c.id == target.id) {
                fled |= b.state == State::Fleeing;
            }
        }
        assert!(fled, "a butterfly let somebody run straight at it without moving");
    }

    #[test]
    fn fireflies_never_rise_over_a_savanna_waterhole_and_still_rise_over_a_meadow_pond() {
        // "откуда светлячки в саванне?" The same warm summer dusk and the
        // same pond, asked as three countries: the savanna and the beach get
        // none however long they are watched, a meadow pond still lights up.
        let world = meadow(true, false);
        let player = Vec3::new(8.0, 11.0, 8.0);
        let most = |biome: Biome| {
            let country = move |_: i32, _: i32, _: i32| (biome, warm(), 1.0);
            let mut critters = Critters::new();
            let mut most = 0;
            // Two minutes: a pond's bank is a few columns in a ring of
            // fourteen hundred, so a half-minute watch can miss it honestly.
            for frame in 0..(30 * 120) {
                let around = Surroundings {
                    chunks: &world,
                    player,
                    world_time: summer(0.85) + frame as f32 / 30.0 / 1200.0,
                    wet: false,
                    dangers: &[],
                    country: &country,
                    light: &LightMap::new(),
                    lantern: false,
                };
                critters.update(&around, 1.0 / 30.0);
                most = most.max(critters.count(Kind::Firefly));
            }
            most
        };
        assert_eq!(most(Biome::Savanna), 0, "fireflies over a waterhole in the savanna");
        assert_eq!(most(Biome::Beach), 0, "fireflies on a salt shore");
        assert!(most(Biome::Plains) > 0, "no fireflies over a warm meadow pond at dusk");
    }

    #[test]
    fn a_frog_in_a_dark_cell_is_drawn_dark_and_one_in_the_open_is_not() {
        // "лягушки игнорируют освещение": every critter was drawn with a
        // constant full-sky light word. One frog on the open bank and one
        // in a sealed pocket in the rock under it, each built alone; the
        // light word is the low sixteen bits of `packed`, the layer the rest.
        let mut world = meadow(false, false);
        let mut chunk = world.get(ChunkPos::new(0, 0)).unwrap().clone();
        chunk.set(3, 5, 3, BLOCK_AIR);
        world.insert(chunk);
        let mut light = LightMap::new();
        for cx in -1..=1 {
            for cz in -1..=1 {
                light.load_chunk(&world, ChunkPos::new(cx, cz));
            }
        }
        let words = |at: Vec3| {
            let mut critters = Critters::new();
            let frog = Critter { kind: Kind::Frog, state: State::Resting, ..critters.blank(at) };
            critters.add(frog);
            let (mut vertices, mut indices) = (Vec::new(), Vec::new());
            let layers = FaceLayers::empty_for_test();
            critters.build_into(Vec3::ZERO, Vec3::X, Vec3::Y, &layers, &light, &mut vertices, &mut indices);
            assert!(!vertices.is_empty(), "no frog drawn at {at:?}");
            vertices.iter().map(|v| v.packed & 0xFFFF).collect::<Vec<u32>>()
        };
        let dark = pack_light(0, 0, 3, 0);
        for word in words(Vec3::new(3.5, 5.0, 3.5)) {
            assert_eq!(word, dark, "a frog shut in the rock was drawn with light word {word:#x}");
        }
        let open = pack_light(15, 0, 3, 0);
        for word in words(Vec3::new(8.5, 11.0, 8.5)) {
            assert_eq!(word, open, "a frog on the open bank was drawn with light word {word:#x}");
        }
    }

    /// What the pool costs a frame, full.
    ///
    /// ```text
    /// cargo test -p primitive_client --lib what_the_small_life_costs_a_frame -- --ignored --nocapture
    /// ```
    #[test]
    #[ignore = "a measurement, not an assertion"]
    fn what_the_small_life_costs_a_frame() {
        let world = meadow(true, true);
        let mut critters = Critters::new();
        let player = Vec3::new(8.0, 11.0, 8.0);
        run(&mut critters, &world, player, summer(0.85), &[], warm(), 60.0, |_| {});
        let layers = FaceLayers::empty_for_test();
        let light = LightMap::new();
        let (mut vertices, mut indices) = (Vec::new(), Vec::new());
        let country = |_: i32, _: i32, _: i32| (Biome::Swamp, warm(), 1.0);
        let started = std::time::Instant::now();
        const FRAMES: u32 = 600;
        for _ in 0..FRAMES {
            let around = Surroundings { chunks: &world, player, world_time: summer(0.85), wet: false, dangers: &[], country: &country, light: &light, lantern: false };
            critters.update(&around, 1.0 / 60.0);
            vertices.clear();
            indices.clear();
            critters.build_into(Vec3::ZERO, Vec3::X, Vec3::Y, &layers, &light, &mut vertices, &mut indices);
        }
        println!(
            "critters: {} alive, {:.1} us a frame to think and build, {} vertices",
            critters.len(),
            started.elapsed().as_secs_f64() * 1e6 / FRAMES as f64,
            vertices.len()
        );
    }

    /// The light over a test world, worked out as the game does.
    fn lit(chunks: &ChunkManager) -> LightMap {
        let mut light = LightMap::new();
        for cx in -1..=1 {
            for cz in -1..=1 {
                light.load_chunk(chunks, ChunkPos::new(cx, cz));
            }
        }
        light
    }

    /// `run`, with the world's light and a torch in the hand or not, and the
    /// bats' cap asserted every frame.
    #[allow(clippy::too_many_arguments)]
    fn run_lit(
        critters: &mut Critters,
        chunks: &ChunkManager,
        light: &LightMap,
        player: Vec3,
        time: f32,
        lantern: bool,
        seconds: f32,
        mut each: impl FnMut(&Critters),
    ) {
        let country = |_: i32, _: i32, _: i32| (Biome::Plains, warm(), 1.0);
        for frame in 0..(seconds * 30.0) as usize {
            let around = Surroundings {
                chunks,
                player,
                world_time: time + frame as f32 / 30.0 / 1200.0,
                wet: false,
                dangers: &[],
                country: &country,
                light,
                lantern,
            };
            critters.update(&around, 1.0 / 30.0);
            assert!(critters.count(Kind::Bat) <= MAX_BATS, "{} bats, over the cap", critters.count(Kind::Bat));
            each(critters);
        }
    }

    /// A sealed cavern in the stone under the meadow: air at y = 3..=7 over
    /// twenty blocks square, with the stone and the turf -- three courses --
    /// over it.
    fn cavern() -> ChunkManager {
        let mut cells = Vec::new();
        for x in -6..14 {
            for z in -6..14 {
                for y in 3..=7 {
                    cells.push(((x, y, z), BLOCK_AIR));
                }
            }
        }
        meadow_with(false, false, &cells)
    }

    /// A stone hill from x = 10 to 30 with a gallery two wide and two high
    /// driven into it from its west face at z = 4..=5, nineteen blocks deep:
    /// light at the mouth, dark at the back.
    fn hill_with_a_gallery() -> ChunkManager {
        let mut cells = Vec::new();
        for x in 10..=30 {
            for z in -4..=12 {
                for y in 11..=15 {
                    let gallery = x <= 28 && (4..=5).contains(&z) && y <= 12;
                    if !gallery {
                        cells.push(((x, y, z), BLOCK_STONE));
                    }
                }
            }
        }
        meadow_with(false, false, &cells)
    }

    #[test]
    fn bats_live_only_in_dark_hollows_under_the_ground_and_never_on_the_surface() {
        let cave = cavern();
        let light = lit(&cave);
        let mut critters = Critters::new();
        let mut most = 0;
        run_lit(&mut critters, &cave, &light, Vec3::new(4.5, 3.0, 4.5), summer(0.95), false, 30.0, |c| {
            for bat in c.live.iter().filter(|b| b.kind == Kind::Bat) {
                let sky = sky_light(&light, bat.position.as_vec3());
                assert!(sky.is_some_and(|sky| sky <= BAT_DARK_SKY), "a bat in the cavern at {:?} is in sky light {sky:?}", bat.position);
            }
            most = most.max(c.count(Kind::Bat));
        });
        assert!(most > 0, "a player in a dark cavern at midnight met no bats");

        // The same world without the hollow, walked at midnight and at dusk.
        let field = meadow(false, false);
        let light = lit(&field);
        for hour in [0.95, 0.78, 0.5] {
            let mut critters = Critters::new();
            run_lit(&mut critters, &field, &light, Vec3::new(8.0, 11.0, 8.0), summer(hour), false, 20.0, |c| {
                assert_eq!(c.count(Kind::Bat), 0, "a bat over a meadow with no cave under it, at hour {hour}");
            });
        }

        // A dark hut on the meadow -- one course of roof -- is not a cave.
        let mut hut = Vec::new();
        for x in 4..=12 {
            for z in 4..=12 {
                hut.push(((x, 14, z), BLOCK_STONE));
                if x == 4 || x == 12 || z == 4 || z == 12 {
                    for y in 11..14 {
                        hut.push(((x, y, z), BLOCK_STONE));
                    }
                }
            }
        }
        let barn = meadow_with(false, false, &hut);
        let light = lit(&barn);
        let mut critters = Critters::new();
        run_lit(&mut critters, &barn, &light, Vec3::new(8.5, 11.0, 8.5), summer(0.95), false, 20.0, |c| {
            assert_eq!(c.count(Kind::Bat), 0, "bats roosting under a hut's one course of roof");
        });
    }

    #[test]
    fn at_dusk_bats_stream_out_of_a_cave_mouth_and_at_noon_the_open_has_none() {
        let hill = hill_with_a_gallery();
        let light = lit(&hill);
        let in_front = Vec3::new(8.5, 11.0, 4.5);

        let mut critters = Critters::new();
        let mut emerged = 0;
        let mut out_in_the_open = false;
        run_lit(&mut critters, &hill, &light, in_front, summer(0.76), false, 40.0, |c| {
            for bat in c.live.iter().filter(|b| b.kind == Kind::Bat) {
                assert_eq!(bat.state, State::Emerging, "a bat out of its roost at dusk that is not leaving the cave");
                out_in_the_open |= bat.position.x < 10.0;
            }
            emerged = emerged.max(c.count(Kind::Bat));
        });
        assert!(emerged > 0, "no bats came out of a dark gallery at dusk");
        assert!(out_in_the_open, "the bats at dusk never got out of the hill");

        let mut critters = Critters::new();
        run_lit(&mut critters, &hill, &light, in_front, summer(0.5), false, 40.0, |c| {
            assert_eq!(c.count(Kind::Bat), 0, "bats in front of a cave at noon");
        });

        // ...and at noon, inside, there is a colony, and it keeps to the dark.
        let mut critters = Critters::new();
        let mut inside = 0;
        run_lit(&mut critters, &hill, &light, Vec3::new(24.5, 11.0, 4.5), summer(0.5), false, 40.0, |c| {
            for bat in c.live.iter().filter(|b| b.kind == Kind::Bat) {
                let sky = sky_light(&light, bat.position.as_vec3());
                assert!(
                    sky.is_some_and(|sky| sky <= BAT_DARK_SKY + 2),
                    "a roosting bat at noon strayed to {:?}, sky light {sky:?}",
                    bat.position
                );
            }
            inside = inside.max(c.count(Kind::Bat));
        });
        assert!(inside > 0, "no colony at the dark back of the gallery");
    }

    #[test]
    fn a_roosting_colony_goes_up_round_a_player_and_from_further_off_round_a_torch() {
        let cave = cavern();
        let light = lit(&cave);
        let mut critters = Critters::new();
        let corner = Vec3::new(-5.5, 3.0, -5.5);
        run_lit(&mut critters, &cave, &light, corner, summer(0.5), false, 30.0, |_| {});
        let hanging = critters
            .live
            .iter()
            .find(|c| c.kind == Kind::Bat && c.state == State::Resting)
            .copied()
            .expect("no bat hanging in a dark cavern at noon after half a minute");
        // A player whose head is eight blocks off: past a bat's wariness of a
        // person, inside its wariness of a torch.
        let eight_off = hanging.position.as_vec3() + Vec3::new(8.0, 0.0, 0.0) - Vec3::Y * 1.5;
        let state = |critters: &Critters| critters.live.iter().find(|c| c.id == hanging.id).map(|c| c.state);
        run_lit(&mut critters, &cave, &light, eight_off, summer(0.5), false, 1.0, |_| {});
        assert_eq!(state(&critters), Some(State::Resting), "a bat took flight from a player eight blocks off in the dark");
        run_lit(&mut critters, &cave, &light, eight_off, summer(0.5), true, 1.0, |_| {});
        assert_ne!(state(&critters), Some(State::Resting), "a bat hung on with a torch eight blocks off");
    }

    /// The closest two bats of one state come, and how far the colony's
    /// bats lie on average from their own middle.
    fn crowding(c: &Critters, state: impl Fn(State) -> bool) -> Option<(f32, f32, usize)> {
        let bats: Vec<Vec3> =
            c.live.iter().filter(|b| b.kind == Kind::Bat && !b.leaving && state(b.state)).map(|b| b.position.as_vec3()).collect();
        if bats.len() < 3 {
            return None;
        }
        let middle = bats.iter().copied().sum::<Vec3>() / bats.len() as f32;
        let spread = bats.iter().map(|b| (*b - middle).length()).sum::<f32>() / bats.len() as f32;
        let mut closest = f32::INFINITY;
        for (i, a) in bats.iter().enumerate() {
            for b in &bats[i + 1..] {
                closest = closest.min((*a - *b).length());
            }
        }
        Some((closest, spread, bats.len()))
    }

    /// **"летучие мыши скапливаются в одном пикселе"**: a colony flying in
    /// a gallery too narrow for most of its wander points came down on the
    /// one point every refused wander fell back to -- its home -- and hung
    /// there as a single dark speck with wings. Five minutes of night in
    /// the gallery and in the open cavern, every frame looked at.
    #[test]
    fn a_colony_never_collapses_to_one_point_in_flight_or_at_its_roost() {
        for (name, world, player, by_day) in [
            ("gallery", hill_with_a_gallery(), Vec3::new(14.5, 11.0, 4.5), Vec3::new(24.5, 11.0, 4.5)),
            ("cavern", cavern(), Vec3::new(-5.5, 3.0, -5.5), Vec3::new(-5.5, 3.0, -5.5)),
        ] {
            let light = lit(&world);
            let mut critters = Critters::new();
            let (mut frames, mut huddled, mut tightest) = (0usize, 0usize, f32::INFINITY);
            run_lit(&mut critters, &world, &light, player, summer(0.95), false, 300.0, |c| {
                if let Some((closest, spread, _)) = crowding(c, |s| matches!(s, State::Flying | State::Fleeing)) {
                    frames += 1;
                    tightest = tightest.min(spread);
                    if closest < 0.12 {
                        huddled += 1;
                    }
                }
            });
            println!("{name}: {frames} frames of a flying colony, {huddled} with two bats in one place, tightest spread {tightest:.2}");
            assert!(frames > 0, "no colony flew in the {name} at night");
            assert!(tightest > 0.25, "the colony in the {name} drew in to {tightest:.2} of a block about its middle");
            assert!(
                (huddled as f32) < frames as f32 * 0.05,
                "two bats of the colony in the {name} were in one place in {huddled} of {frames} frames"
            );

            // By day, hung from the roof: side by side, never one on another.
            let mut critters = Critters::new();
            let mut hung = 0usize;
            run_lit(&mut critters, &world, &light, by_day, summer(0.5), false, 120.0, |c| {
                let hanging: Vec<Vec3> =
                    c.live.iter().filter(|b| b.kind == Kind::Bat && b.state == State::Resting).map(|b| b.position.as_vec3()).collect();
                for (i, a) in hanging.iter().enumerate() {
                    for b in &hanging[i + 1..] {
                        assert!((*a - *b).length() >= BAT_HANG_APART - 1e-3, "two bats hung {:.3} apart at {a:?}", (*a - *b).length());
                    }
                }
                hung = hung.max(hanging.len());
            });
            println!("{name}: at most {hung} hanging at once");
            // The gallery's colony roosts at its dark back, too close to
            // anywhere a player can stand in the dark to stay hung; the
            // cavern's is left alone.
            if name == "cavern" {
                assert!(hung >= 3, "no colony hung from the roof of the {name} by day");
            }
        }
    }
}
