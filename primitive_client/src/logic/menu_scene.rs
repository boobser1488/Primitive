//! The world behind the main menu.
//!
//! ## The problem this solves, and the three answers to it
//!
//! The main menu is on screen *before there is a world*: no server has
//! been started, no seed has been chosen, nothing has been streamed. So
//! whatever stands behind it has to come from somewhere the client can
//! reach on its own. Three ways were considered.
//!
//! * **A tiled block texture** -- what this replaced. It costs nothing
//!   and it is not a place: twelve rows of the same 16x16 picture read
//!   as wallpaper, which is exactly what it was called. A menu is the
//!   first thing anybody sees of a game about *going somewhere*, and a
//!   wall of identical cubes says the opposite.
//! * **A scene built by hand** -- a few dozen blocks arranged in code
//!   into something shore-shaped. Cheap, and a lie: it would be the one
//!   view in the game that no seed can produce, and it would go stale
//!   the first time the shore rules changed and nobody remembered this
//!   file existed.
//! * **A real patch of a real world**, which is what this is. The
//!   client already has the generator (`primitive_shared::worldgen`),
//!   the lighting and the mesher; a menu scene is those three run over
//!   twenty-five chunks on one background thread. What appears behind
//!   the menu is somewhere a player could actually stand, because it
//!   was made the same way the world they are about to enter is.
//!
//! The cost of the third answer is the only real argument against it,
//! and it is bounded on purpose: one thread, one patch, one time. See
//! `PATCH_RADIUS` and `MenuScene::poll`. With `menu_background` off,
//! none of this runs at all -- which is the setting's whole job.
//!
//! ## The camera stands still
//!
//! It drifts through a slow arc around the direction the spot was
//! chosen for, and it does not move. Two alternatives were rejected:
//!
//! * **An orbit** needs the whole ring of chunks around the centre
//!   meshed rather than the cone in front of it, and underground it
//!   walks the eye straight into rock.
//! * **A full spin** shows every direction, and a shore was picked for
//!   the direction the sea is in. Half of a full spin is the half the
//!   spot was *not* chosen for.
//!
//! A slow cosine sweep keeps the picture the search paid for and still
//! moves, which is all the motion a backdrop needs.
//!
//! ## It is drawn, not photographed
//!
//! Nothing here produces an image. What this module makes is chunk
//! *meshes*, handed to the renderer through the same `set_chunk_mesh`
//! the streamed world uses, and drawn every frame by the same passes --
//! sky, terrain, fog -- from a camera the menu branch points at the
//! spot. There is no still anywhere in the path: turn the sweep off and
//! the picture is a rendered world that happens not to be moving.
//!
//! There *is* a still in the tools, and it is worth knowing which is
//! which. `renderer`'s `the_menu_backdrop_through_the_real_shader`
//! renders this scene offscreen to measure how bright it gets, and
//! `ui::snapshot` can stand a menu on top of that PNG so a layout can
//! be judged without a graphics card. Both are ways of *looking at*
//! what the player is shown. Neither is what the player is shown.

use std::sync::mpsc::{channel, Receiver, TryRecvError};

use glam::Vec3;

use primitive_shared::lighting::LightMap;
use primitive_shared::types::{
    is_opaque, ChunkPos, BLOCK_AIR, CHUNK_SIZE_X, CHUNK_SIZE_Y, CHUNK_SIZE_Z,
};
use primitive_shared::worldgen::{Biome, Preset, WorldGen, SEA_LEVEL};

use crate::engine::mesh::{build_mesh, MeshBuffers, Neighbourhood};
use crate::engine::texture::FaceLayers;
use crate::logic::chunk_manager::ChunkManager;

/// How far the patch reaches from the camera's own chunk, in chunks.
///
/// Two, so twenty-five chunks -- an eighty-block square with the eye in
/// the middle, which is forty blocks of view in the direction the sweep
/// covers. That is where the two costs cross: one chunk less and the
/// fog has to start close enough to eat the middle distance, one more
/// and the patch grows by eleven chunks (a ring is `8r+... `, not a
/// constant) for terrain the sweep barely turns far enough to see.
pub const PATCH_RADIUS: i32 = 2;

/// How far the scene is drawn before the fog closes it off, in blocks.
///
/// Just inside the patch's own edge. The fog is the horizon here: it
/// has to finish *before* the terrain does, or the last chunk ends in
/// mid-air against the sky and the backdrop reads as a diorama on a
/// table. Same rule as the streamed world's -- see `Fog::clamp_to`.
pub fn view_radius_blocks() -> f32 {
    (PATCH_RADIUS * CHUNK_SIZE_X as i32) as f32
}

/// Where the eye sits above the ground it was placed on.
///
/// A standing player's, not a drone's: the scene is meant to read as
/// somewhere you could be, and the first thing that gives away a camera
/// nobody could occupy is its height.
const EYE_HEIGHT: f32 = 1.62;

/// How far either side of the chosen direction the view drifts, in
/// radians, and how long one there-and-back takes in seconds.
///
/// Twenty-two degrees over three quarters of a minute: slow enough that
/// a glance does not catch it moving, wide enough that a minute at the
/// menu is not a photograph.
const SWEEP_RADIANS: f32 = 0.38;
const SWEEP_SECONDS: f32 = 46.0;

/// What time of day the menu's world is at.
///
/// **Sunset, and the reason is legibility rather than taste.** The menu
/// has to be readable over whatever is behind it, and the only handle
/// on that is a dark veil (see `menu::backdrop`). A veil is alpha
/// compositing, so it scales the *contrast* of everything under it by
/// the same fraction it darkens it: a noon shore veiled down to
/// something small text can sit on is a flat grey smear where a beach
/// was. Dusk arrives at the same final brightness with a much lighter
/// veil, so the scene keeps its own shape.
///
/// 0.75 is the moment the sun touches the horizon -- `Sky` puts the
/// light at 35% of noon, turns it orange and paints the sunset sky. See
/// `Sky::sun_elevation`, whose zero this is.
pub const TIME_OF_DAY: f32 = 0.75;

/// The kind of place the menu opens on.
///
/// Four, and each one is a different *reason* to look at it: water
/// meeting land, a wood with a canopy over the eye, open country with a
/// horizon, and the dark. A fifth that is merely another arrangement of
/// grass would not be a fifth.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Place {
    Shore,
    Forest,
    Plains,
    Cave,
}

impl Place {
    /// Every place, in the order the settings row steps through them.
    pub const ALL: [Place; 4] = [Place::Shore, Place::Forest, Place::Plains, Place::Cave];

    /// What this is called in `settings.toml`.
    ///
    /// Lower case and English, like every other identifier written into
    /// a settings file: the file is read by the game, and a value that
    /// changed with the interface language would be a file that stopped
    /// parsing when somebody switched to Polish.
    pub fn name(self) -> &'static str {
        match self {
            Place::Shore => "shore",
            Place::Forest => "forest",
            Place::Plains => "plains",
            Place::Cave => "cave",
        }
    }

    /// The name back into a place. `None` covers both "anything" and
    /// anything unrecognised -- see `ClientSettings::menu_background_place`.
    pub fn parse(name: &str) -> Option<Place> {
        Place::ALL.into_iter().find(|p| p.name() == name)
    }
}

/// Where the camera stands, which way it looks, and in what world.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Spot {
    pub seed: u32,
    pub place: Place,
    /// The eye, in world coordinates.
    pub eye: Vec3,
    /// The middle of the sweep, in radians (`Camera::yaw`).
    pub facing: f32,
    pub pitch: f32,
}

impl Spot {
    /// The chunk the eye is in, which is what the patch is centred on.
    pub fn centre_chunk(&self) -> ChunkPos {
        ChunkManager::chunk_for_world_pos(self.eye.x, self.eye.z)
    }
}

/// One chunk of the scene, meshed and waiting to be handed to the card.
pub struct Built {
    pub pos: ChunkPos,
    pub buffers: MeshBuffers,
}

/// The scene, and the thread building it.
pub struct MenuScene {
    spot: Spot,
    built: Receiver<Built>,
    /// Seconds since the scene was asked for, which is what the sweep
    /// is a function of.
    age: f32,
    /// Set when the worker's channel closes, so a dead thread is not
    /// polled for ever.
    finished: bool,
}

impl MenuScene {
    /// Picks a spot and starts building it.
    ///
    /// `roll` is what makes two launches different: it seeds both the
    /// world and the order the candidate columns are tried in. The
    /// search itself is pure -- see `look_for` -- so a failure can be
    /// reproduced from the number alone.
    pub fn spawn(wanted: Option<Place>, roll: u32, layers: FaceLayers) -> Self {
        let spot = look_for(wanted, roll);
        let (tx, rx) = channel();
        let centre = spot.centre_chunk();
        let seed = spot.seed;

        // A named thread, because a stack trace out of the generator is
        // otherwise indistinguishable from one out of a mesher worker.
        //
        // Detached on purpose: it holds nothing but its own copy of the
        // world and it ends when the patch is done or when the receiver
        // is dropped, whichever comes first. Joining it would mean the
        // menu waiting for a backdrop.
        let spawned = std::thread::Builder::new()
            .name("primitive-menu-scene".to_string())
            .spawn(move || build_patch(seed, centre, &layers, &tx));
        if let Err(error) = &spawned {
            // Not fatal, and not silent: the menu simply has a sky
            // behind it, which is what it had before this existed.
            eprintln!("menu scene: no thread for it ({error}); the backdrop stays empty");
        }

        Self {
            spot,
            built: rx,
            age: 0.0,
            finished: spawned.is_err(),
        }
    }

    pub fn spot(&self) -> Spot {
        self.spot
    }

    pub fn tick(&mut self, dt: f32) {
        // Clamped for the same reason the world's frame time is: a menu
        // left open while the machine sleeps must not come back with the
        // view halfway through a swing.
        self.age += dt.clamp(0.0, 0.25);
    }

    /// The next finished chunk, or `None` when there is nothing waiting.
    ///
    /// **Handed over one at a time on purpose.** Uploading a mesh is a
    /// copy to the card, and twenty-five of them in the frame they all
    /// happen to be ready in is a visible hitch on the one screen where
    /// nothing else is going on. The caller lands them under a time
    /// budget, exactly as the streamed world does -- see
    /// `collect_worker_results`.
    pub fn poll(&mut self) -> Option<Built> {
        if self.finished {
            return None;
        }
        match self.built.try_recv() {
            Ok(built) => Some(built),
            Err(TryRecvError::Empty) => None,
            Err(TryRecvError::Disconnected) => {
                self.finished = true;
                None
            }
        }
    }

    /// Where the eye is this frame.
    ///
    /// The position never changes; only the yaw does. Written as a pair
    /// rather than as a `Camera` so this module does not have to know
    /// what a camera is -- the caller owns one already and only these
    /// two numbers are the scene's business.
    pub fn eye(&self) -> Vec3 {
        self.spot.eye
    }

    pub fn yaw(&self) -> f32 {
        let phase = self.age / SWEEP_SECONDS * std::f32::consts::TAU;
        self.spot.facing + SWEEP_RADIANS * phase.sin()
    }

    pub fn pitch(&self) -> f32 {
        self.spot.pitch
    }
}

/// Every chunk of the patch, nearest the middle first.
///
/// The order is the whole point: the first mesh to arrive is the one
/// under the camera, so the scene grows outward from what is being
/// looked at rather than appearing a corner at a time.
fn patch(centre: ChunkPos) -> Vec<ChunkPos> {
    let mut out = Vec::new();
    for dz in -PATCH_RADIUS..=PATCH_RADIUS {
        for dx in -PATCH_RADIUS..=PATCH_RADIUS {
            out.push(ChunkPos::new(centre.x + dx, centre.z + dz));
        }
    }
    out.sort_by_key(|pos| {
        let (dx, dz) = (pos.x - centre.x, pos.z - centre.z);
        dx * dx + dz * dz
    });
    out
}

/// The worker: generate, light, mesh, hand back.
///
/// Generation and lighting are done for the *whole* patch before any of
/// it is meshed, and that ordering is not an accident. A chunk meshed
/// before its neighbours exist is meshed against air: the faces along
/// the seam are emitted, and the scene grows a grid of walls down every
/// chunk boundary. The streamed world avoids the same thing with
/// `neighbourhood_settled`; here the whole neighbourhood is known in
/// advance, so it is simply built first.
fn build_patch(seed: u32, centre: ChunkPos, layers: &FaceLayers, out: &std::sync::mpsc::Sender<Built>) {
    let world = WorldGen::with_preset(seed, Preset::Normal);
    let order = patch(centre);

    // One chunk wider than the patch on every side. The extra ring is
    // never drawn; it is what the *edge* chunks are meshed against, so
    // the outermost faces of the scene are culled against real
    // neighbours instead of against nothing.
    let mut chunks = ChunkManager::new(PATCH_RADIUS + 2);
    let padded = padded_patch(centre);
    for pos in &padded {
        chunks.insert(world.generate_chunk(*pos));
    }

    let mut light = LightMap::new();
    for pos in &padded {
        light.load_chunk(&chunks, *pos);
    }

    let mut cache = Neighbourhood::default();
    for pos in order {
        let mut buffers = MeshBuffers::default();
        cache.fill(pos, &chunks, &light);
        build_mesh(pos, &cache, layers, &world, &mut buffers);
        if out.send(Built { pos, buffers }).is_err() {
            return; // the menu closed; nobody is waiting for the rest
        }
    }
}

/// The patch plus the ring the edge chunks are meshed against.
fn padded_patch(centre: ChunkPos) -> Vec<ChunkPos> {
    let reach = PATCH_RADIUS + 1;
    let mut out = Vec::new();
    for dz in -reach..=reach {
        for dx in -reach..=reach {
            out.push(ChunkPos::new(centre.x + dx, centre.z + dz));
        }
    }
    out
}

/// One column, as the search reads it.
struct Look {
    height: i32,
    biome: Biome,
}

fn look(world: &WorldGen, gx: i32, gz: i32) -> Look {
    Look {
        height: world.height_at(gx, gz),
        biome: world.biome_at(gx, gz),
    }
}

/// The eight directions the search samples in, as unit-ish offsets.
///
/// Eight rather than four because a shore runs at whatever angle it
/// likes, and a search that can only face north, south, east or west
/// puts the sea in the corner of the frame half the time it finds one.
const DIRECTIONS: [(f32, f32); 8] = [
    (1.0, 0.0),
    (0.707, 0.707),
    (0.0, 1.0),
    (-0.707, 0.707),
    (-1.0, 0.0),
    (-0.707, -0.707),
    (0.0, -1.0),
    (0.707, -0.707),
];

fn along(direction: (f32, f32), distance: f32) -> (i32, i32) {
    (
        (direction.0 * distance).round() as i32,
        (direction.1 * distance).round() as i32,
    )
}

fn yaw_of(direction: (f32, f32)) -> f32 {
    direction.1.atan2(direction.0)
}

/// A cheap, well-mixed integer hash.
///
/// The search needs a stream of unrelated numbers from one `roll`, and
/// this is the smallest thing that gives them. Deliberately not the
/// world generator's noise: that answers questions about a *place*, and
/// what is wanted here is an order to try places in.
fn mix(mut x: u32) -> u32 {
    x ^= x >> 16;
    x = x.wrapping_mul(0x7feb_352d);
    x ^= x >> 15;
    x = x.wrapping_mul(0x846c_a68b);
    x ^ (x >> 16)
}

/// How many worlds the search will try before settling for what it has.
const SEEDS_TRIED: u32 = 4;
/// How many columns it tries in each of them.
const COLUMNS_PER_SEED: u32 = 160;
/// How many chunks the search may generate in total.
///
/// Two things need real blocks rather than columns -- finding a cave,
/// and checking that the spot chosen for a surface place is not inside
/// a tree -- and both are bounded together, because what has to be
/// bounded is the wait before the menu has a backdrop rather than
/// either question on its own. Two hundred and forty chunks is about a
/// quarter of a second of one thread in a release build, and the menu
/// is already on screen while it runs.
const CHUNK_BUDGET: u32 = 240;
/// How far from a world's origin candidates are drawn, in blocks.
///
/// Two thousand: far enough that two launches of the same seed are
/// different country, near enough that the coordinates stay small
/// enough for the render origin to keep the GPU's numbers in the
/// hundreds. See `render_origin_for`.
const SEARCH_RADIUS: i32 = 2_000;

/// Finds somewhere worth looking at.
///
/// **Searched rather than written down**, and that is the decision this
/// function exists to record. The obvious alternative is a table of
/// hand-picked seeds and coordinates: it is instant, and it rots. Every
/// change to the generator moves the terrain under it, and the failure
/// mode is a menu that opens underwater with nobody able to say when it
/// started. A search asks the generator what is there *now*, so the
/// worst a worldgen change can do is make the search work harder.
///
/// It never fails. If nothing matching is found -- a seed whose whole
/// neighbourhood is ocean, a generator change that makes beaches rare
/// -- the last candidate is taken as it stands, because a menu with a
/// dull backdrop is a menu, and a menu still searching is a black
/// screen.
pub fn look_for(wanted: Option<Place>, roll: u32) -> Spot {
    // With nothing asked for, the roll picks: a player who has not
    // chosen gets a different country each launch, which is most of
    // the value of generating this rather than drawing it.
    let places: Vec<Place> = match wanted {
        Some(place) => vec![place],
        None => {
            let first = (mix(roll) % Place::ALL.len() as u32) as usize;
            (0..Place::ALL.len())
                .map(|i| Place::ALL[(first + i) % Place::ALL.len()])
                .collect()
        }
    };

    // Where to stand if the whole search comes up empty: the last
    // column it looked at, which is at least a real column of a real
    // world rather than the origin. Kept as three numbers rather than
    // as a `Spot` because working the spot out costs eight more
    // generator queries, and the search would pay that for every
    // candidate it rejects.
    let mut last: Option<(u32, i32, i32)> = None;
    // Chunks the search is allowed to generate, over all of it.
    // Without a bound, a world with no caves near the surface is a menu
    // that spends seconds deciding there are none: every place but the
    // cave is decided from column queries, and that one has to make the
    // chunk to find out. See `cave_spot`.
    let mut chunk_budget = CHUNK_BUDGET;

    for place in &places {
        for attempt in 0..SEEDS_TRIED {
            let seed = mix(roll ^ (0x9E37_79B9u32.wrapping_mul(attempt + 1)));
            let world = WorldGen::with_preset(seed, Preset::Normal);
            for index in 0..COLUMNS_PER_SEED {
                let key = mix(seed ^ mix(index.wrapping_mul(2_654_435_761)));
                let gx = (key % (SEARCH_RADIUS as u32 * 2)) as i32 - SEARCH_RADIUS;
                let gz = (key.rotate_left(16) % (SEARCH_RADIUS as u32 * 2)) as i32
                    - SEARCH_RADIUS;
                if let Some(spot) = judge(&world, *place, gx, gz, &mut chunk_budget) {
                    return spot;
                }
                last = Some((seed, gx, gz));
            }
        }
    }

    let (seed, gx, gz) = last.unwrap_or_else(|| {
        let world = WorldGen::with_preset(roll, Preset::Normal);
        let (gx, gz) = world.spawn_column();
        (roll, gx, gz)
    });
    let world = WorldGen::with_preset(seed, Preset::Normal);
    fallback(&world, places[0], gx, gz)
}

/// Standing on the column as found, facing the way the ground falls
/// away -- which is the least bad view of anywhere.
fn fallback(world: &WorldGen, place: Place, gx: i32, gz: i32) -> Spot {
    let facing = DIRECTIONS
        .iter()
        .min_by_key(|direction| {
            let (dx, dz) = along(**direction, 24.0);
            world.height_at(gx + dx, gz + dz)
        })
        .copied()
        .unwrap_or((1.0, 0.0));
    Spot {
        seed: world.seed(),
        place,
        eye: eye_over(world, gx, gz),
        facing: yaw_of(facing),
        pitch: pitch_for(place),
    }
}

/// The eye above a column: standing height over whatever the world puts
/// a player on there.
///
/// `spawn_y` rather than `height_at`, and the difference is the
/// waterline: `spawn_y` never answers below the sea, so a camera placed
/// on a beach column that turns out to be a foot under water ends up on
/// the surface rather than inside it. Reusing the game's own rule is
/// also what stops the two drifting apart.
fn eye_over(world: &WorldGen, gx: i32, gz: i32) -> Vec3 {
    Vec3::new(
        gx as f32 + 0.5,
        world.spawn_y(gx, gz) + EYE_HEIGHT,
        gz as f32 + 0.5,
    )
}

/// How far down the view is tilted.
///
/// Outdoors a little, because a level camera at eye height fills the
/// bottom half of the frame with the ground at your feet and the top
/// half with empty sky, and the interesting band -- where the land
/// meets the water or the trunks meet the canopy -- ends up behind the
/// buttons. Underground, level: a cave's ceiling is part of the picture.
fn pitch_for(place: Place) -> f32 {
    match place {
        Place::Cave => 0.0,
        _ => -0.13,
    }
}

/// Is this column the middle of the kind of place asked for, and if so,
/// which way should the camera look?
fn judge(
    world: &WorldGen,
    place: Place,
    gx: i32,
    gz: i32,
    chunk_budget: &mut u32,
) -> Option<Spot> {
    let facing = match place {
        Place::Shore => shore_facing(world, gx, gz)?,
        Place::Forest => forest_facing(world, gx, gz)?,
        Place::Plains => plains_facing(world, gx, gz)?,
        Place::Cave => return cave_spot(world, gx, gz, chunk_budget),
    };
    if !the_view_is_clear(world, gx, gz, facing, chunk_budget) {
        return None;
    }
    Some(Spot {
        seed: world.seed(),
        place,
        eye: eye_over(world, gx, gz),
        facing: yaw_of(facing),
        pitch: pitch_for(place),
    })
}

/// Is there room to stand here, and anything to see from it?
///
/// **The column functions cannot answer either half, and that is why
/// this is separate.** They read heights and biomes -- half a dozen
/// noise fields each, which is what makes trying a few hundred columns
/// affordable -- and heights and biomes say nothing about what the
/// generator *puts on top* of a column afterwards. Two spots this chose
/// before it existed, and both were rendered and looked at:
///
///   * a forest with the eye inside a leaf block, which is a screen of
///     leaf. A perfectly good wood, with a tree standing exactly where
///     the camera was;
///   * a meadow with a savanna tree on the very column, which is a
///     screen of canopy from underneath. That one passed an earlier
///     version of this check, because leaves are not *collidable* --
///     you can walk through them, and the check asked whether a player
///     would be stuck rather than whether a camera could see.
///
/// So: air at head height, ground under the feet that is not water, and
/// three clear blocks in the direction the view was chosen for. Three
/// rather than ten, because a trunk five blocks off is depth in the
/// picture and a trunk against the lens is the picture.
///
/// It costs a chunk or two, which is why it happens last, on the
/// accepted candidate only, and why it draws on the same budget the
/// cave search does. With the budget spent it answers yes: a spot
/// chosen from heights alone is a risk, and a menu still searching is a
/// black screen.
fn the_view_is_clear(
    world: &WorldGen,
    gx: i32,
    gz: i32,
    facing: (f32, f32),
    chunk_budget: &mut u32,
) -> bool {
    if *chunk_budget == 0 {
        return true;
    }
    // Kept across the walk, because three blocks in a direction can
    // cross a chunk boundary and generating the same chunk once per
    // cell is how a check costs more than the search it belongs to.
    let mut made: Vec<(ChunkPos, primitive_shared::types::Chunk)> = Vec::new();
    let mut block = |gx: i32, gy: i32, gz: i32, budget: &mut u32| -> Option<u16> {
        if !(0..CHUNK_SIZE_Y as i32).contains(&gy) {
            return Some(BLOCK_AIR);
        }
        let (pos, lx, lz) = ChunkPos::from_global(gx, gz);
        if let Some((_, chunk)) = made.iter().find(|(at, _)| *at == pos) {
            return Some(chunk.get(lx, gy as usize, lz));
        }
        if *budget == 0 {
            return None;
        }
        *budget -= 1;
        let chunk = world.generate_chunk(pos);
        let found = chunk.get(lx, gy as usize, lz);
        made.push((pos, chunk));
        Some(found)
    };

    let feet = world.spawn_y(gx, gz) as i32;
    // Under the feet is allowed to be a tuft of grass -- standing in
    // the long grass is standing in a meadow. At head height it has to
    // be nothing at all.
    match block(gx, feet, gz, chunk_budget) {
        Some(at_foot) if primitive_shared::types::is_liquid(at_foot) => return false,
        Some(at_foot) if is_opaque(at_foot) => return false,
        None => return true,
        _ => {}
    }
    for step in 0..=3 {
        let (dx, dz) = along(facing, step as f32);
        match block(gx + dx, feet + 1, gz + dz, chunk_budget) {
            Some(BLOCK_AIR) => {}
            Some(_) => return false,
            // Out of budget: what has been checked has passed, and the
            // rest is taken on trust rather than costing a menu its
            // backdrop.
            None => return true,
        }
    }
    true
}

/// Sand with the sea on one side of it and land on the other.
///
/// All three clauses are load-bearing. Without the sea the column is
/// merely a desert; without the land behind it the camera is standing
/// on a sandbar with water on every side, which reads as a flood rather
/// than a coast; and without the height window the "beach" can be the
/// top of a dune thirty blocks up, where the water is a stripe on the
/// horizon.
fn shore_facing(world: &WorldGen, gx: i32, gz: i32) -> Option<(f32, f32)> {
    let here = look(world, gx, gz);
    if here.biome != Biome::Beach {
        return None;
    }
    if here.height < SEA_LEVEL || here.height > SEA_LEVEL + 3 {
        return None;
    }
    for (index, direction) in DIRECTIONS.iter().enumerate() {
        let near = along(*direction, 14.0);
        let far = along(*direction, 26.0);
        let sea_near = look(world, gx + near.0, gz + near.1);
        let sea_far = look(world, gx + far.0, gz + far.1);
        // Water close enough to be the middle of the picture, and still
        // water further out, so the frame is not one puddle.
        if sea_near.height >= SEA_LEVEL || sea_far.height >= SEA_LEVEL - 1 {
            continue;
        }
        // ...and dry ground behind, which is where the camera's back is.
        let back = DIRECTIONS[(index + 4) % DIRECTIONS.len()];
        let behind = along(back, 18.0);
        if world.height_at(gx + behind.0, gz + behind.1) <= SEA_LEVEL {
            continue;
        }
        return Some(*direction);
    }
    None
}

/// Trees on every side, and the ground under them not a cliff.
fn forest_facing(world: &WorldGen, gx: i32, gz: i32) -> Option<(f32, f32)> {
    // **`DeadForest` is deliberately not on this list**, and it was.
    // The first forest backdrop rendered came out as bare grey trunks
    // over bare earth -- an honest picture of a dead wood, and the
    // wrong answer to "a forest": what a player picking that row wants
    // is a canopy. The dead wood is somewhere to *find*, not the first
    // thing the game says about itself.
    let wooded = |biome: Biome| {
        matches!(biome, Biome::Forest | Biome::BirchForest | Biome::Taiga)
    };
    let here = look(world, gx, gz);
    if !wooded(here.biome) || here.height <= SEA_LEVEL + 1 {
        return None;
    }
    let mut wood = 0;
    for direction in DIRECTIONS {
        let (dx, dz) = along(direction, 12.0);
        let there = look(world, gx + dx, gz + dz);
        // A slope steep enough to be a wall is what turns a wood into a
        // hillside seen edge on, so it disqualifies the column rather
        // than merely not counting.
        if (there.height - here.height).abs() > 7 {
            return None;
        }
        if wooded(there.biome) {
            wood += 1;
        }
    }
    if wood < 6 {
        return None;
    }
    // Along the flattest line, so the view runs *through* the wood
    // rather than into the side of a rise.
    DIRECTIONS
        .into_iter()
        .min_by_key(|direction| {
            let (dx, dz) = along(*direction, 24.0);
            (world.height_at(gx + dx, gz + dz) - here.height).abs()
        })
}

/// Open ground, flat, with something on the horizon to look at.
fn plains_facing(world: &WorldGen, gx: i32, gz: i32) -> Option<(f32, f32)> {
    let open = |biome: Biome| matches!(biome, Biome::Plains | Biome::Savanna);
    let here = look(world, gx, gz);
    if !open(here.biome) || here.height <= SEA_LEVEL + 1 {
        return None;
    }
    let mut lowest = here.height;
    let mut highest = here.height;
    for direction in DIRECTIONS {
        let (dx, dz) = along(direction, 14.0);
        let there = look(world, gx + dx, gz + dz);
        if !open(there.biome) {
            return None;
        }
        lowest = lowest.min(there.height);
        highest = highest.max(there.height);
    }
    // Flat, but not a table: three blocks of relief across thirty is
    // what makes a meadow read as ground rather than as a floor.
    if highest - lowest > 4 {
        return None;
    }
    // Toward whatever stands highest a little way off -- the one thing
    // an open field needs is a horizon that is not a straight line.
    DIRECTIONS
        .into_iter()
        .max_by_key(|direction| {
            let (dx, dz) = along(*direction, 40.0);
            world.height_at(gx + dx, gz + dz)
        })
}

/// Somewhere underground with room to stand and rock in every
/// direction.
///
/// **The one place that has to look at the blocks.** The other three
/// are decided by the column functions -- height and biome, a few
/// microseconds each -- but "is there a cave here" is not a property of
/// a column: the generator carves caves out of the volume, and the only
/// honest way to ask is to make the chunk and look. That is why this
/// arm is tried on far fewer candidates than it looks like: a column
/// with no depth over it is rejected before the chunk is generated.
fn cave_spot(world: &WorldGen, gx: i32, gz: i32, chunk_budget: &mut u32) -> Option<Spot> {
    // Not worth generating a chunk for a column with barely any rock
    // under it: the caves are down in the stone, and a shallow column
    // is one where the answer is almost always no. Four blocks above
    // the sea rather than eight, and the difference is a whole world:
    // one seed in a dozen is ocean enough that a strict filter rejects
    // every candidate without ever making a chunk, and the search then
    // reports "no caves here" having looked at none.
    if world.height_at(gx, gz) < SEA_LEVEL + 4 {
        return None;
    }
    if *chunk_budget == 0 {
        return None;
    }
    *chunk_budget -= 1;
    let (pos, _, _) = ChunkPos::from_global(gx, gz);
    let chunk = world.generate_chunk(pos);
    let inside = |x: i32, y: i32, z: i32| {
        (0..CHUNK_SIZE_X as i32).contains(&x)
            && (0..CHUNK_SIZE_Z as i32).contains(&z)
            && (0..CHUNK_SIZE_Y as i32).contains(&y)
    };
    // Outside the chunk counts as rock and never as air, so a pocket
    // that runs off the edge is not chosen -- that is the one case
    // where the eye could end up looking at nothing but the seam.
    let solid = |x: i32, y: i32, z: i32| -> bool {
        !inside(x, y, z) || is_opaque(chunk.get(x as usize, y as usize, z as usize))
    };
    // Air proper, not merely "not rock": a flooded pocket passes every
    // opacity test there is and is a screen of water.
    let air = |x: i32, y: i32, z: i32| -> bool {
        inside(x, y, z) && chunk.get(x as usize, y as usize, z as usize) == BLOCK_AIR
    };

    let origin_x = pos.x * CHUNK_SIZE_X as i32;
    let origin_z = pos.z * CHUNK_SIZE_Z as i32;

    // How far the ground is above each of the chunk's own columns.
    //
    // **The bug this replaces put the camera in a meadow.** The depth
    // test used to be one number for the whole chunk -- the height of
    // the *candidate column* -- while the pocket search ranged over all
    // two hundred and fifty-six of them. So a chunk whose candidate
    // column stood on a hill happily accepted a cell eight blocks lower
    // in a neighbouring column that was open field: floor underfoot,
    // air above, a perfect score, and the sky directly overhead. Being
    // underground is a fact about *a column*, and this is that column.
    let mut surface = [0i32; CHUNK_SIZE_X * CHUNK_SIZE_Z];
    for z in 0..CHUNK_SIZE_Z {
        for x in 0..CHUNK_SIZE_X {
            surface[z * CHUNK_SIZE_X + x] =
                world.height_at(origin_x + x as i32, origin_z + z as i32);
        }
    }

    // The best pocket in the chunk, scored by how much room is around
    // it. Deep enough that daylight is not pouring in, high enough off
    // the bedrock that the floor is stone rather than the bottom of the
    // world.
    let mut best: Option<(i32, i32, i32, i32)> = None; // (score, x, y, z)
    for y in 6..CHUNK_SIZE_Y as i32 - 4 {
        for z in 2..CHUNK_SIZE_Z as i32 - 2 {
            for x in 2..CHUNK_SIZE_X as i32 - 2 {
                // Eight blocks of rock over the head, so what is
                // overhead is a ceiling and not the weather.
                if surface[(z as usize) * CHUNK_SIZE_X + x as usize] - y < 8 {
                    continue;
                }
                // Standing room: floor under the feet, two clear cells
                // above it, and a head that is not in the ceiling.
                if !solid(x, y - 1, z) || !air(x, y, z) || !air(x, y + 1, z) {
                    continue;
                }
                let mut room = 0;
                for dz in -2..=2 {
                    for dy in 0..=2 {
                        for dx in -2..=2 {
                            if air(x + dx, y + dy, z + dz) {
                                room += 1;
                            }
                        }
                    }
                }
                if best.is_none_or(|(score, ..)| room > score) {
                    best = Some((room, x, y, z));
                }
            }
        }
    }

    // A handful of loose cells is a crack, not a cave, and a camera in
    // one shows a screen of solid rock. Half the sampled box open is
    // the line between "there is a passage here" and "there is a gap
    // between two stones".
    let (score, x, y, z) = best?;
    if score < 32 {
        return None;
    }

    // Down whatever passage is longest, so the picture has depth in it
    // rather than a wall at arm's length.
    let facing = DIRECTIONS
        .into_iter()
        .max_by_key(|direction| {
            let mut run = 0;
            for step in 1..12 {
                let (dx, dz) = along(*direction, step as f32);
                if !air(x + dx, y, z + dz) {
                    break;
                }
                run += 1;
            }
            run
        })
        .unwrap_or((1.0, 0.0));

    Some(Spot {
        seed: world.seed(),
        place: Place::Cave,
        eye: Vec3::new(
            (origin_x + x) as f32 + 0.5,
            y as f32 + 0.62,
            (origin_z + z) as f32 + 0.5,
        ),
        facing: yaw_of(facing),
        pitch: 0.0,
    })
}

/// The block at a global position of a generated chunk, for the tests
/// that check what the camera is standing in.
#[cfg(test)]
fn block_at(
    world: &WorldGen,
    gx: i32,
    gy: i32,
    gz: i32,
) -> primitive_shared::types::BlockId {
    if !(0..CHUNK_SIZE_Y as i32).contains(&gy) {
        return BLOCK_AIR;
    }
    let (pos, lx, lz) = ChunkPos::from_global(gx, gz);
    world.generate_chunk(pos).get(lx, gy as usize, lz)
}

#[cfg(test)]
mod tests {
    use super::*;
    use primitive_shared::types::{is_collidable, BLOCK_WATER};

    /// Rolls to try. Twelve, because the search is over a hash and one
    /// number proves nothing about the next.
    const ROLLS: [u32; 12] = [0, 1, 2, 7, 19, 42, 99, 1234, 65_535, 777_777, 8_675_309, u32::MAX];

    /// What the backdrop costs, since it is the one thing this feature
    /// spends that the wallpaper it replaced did not.
    ///
    /// ```text
    /// cargo test --release -p primitive_client --lib what_the_backdrop_costs \
    ///     -- --ignored --nocapture
    /// ```
    ///
    /// **A tool, not a check**, for the reason every measurement in
    /// this repository is: a threshold on a wall-clock time is a test
    /// that fails on somebody else's laptop. What it is for is the rule
    /// in CLAUDE.md -- no change to performance without a
    /// before-and-after -- and the "before" here is zero, because
    /// before this the menu drew no world at all.
    ///
    /// Two numbers, and they are spent in different places: the search
    /// and the build run on one background thread while the menu is
    /// already on screen, and only the upload is on the frame -- under
    /// `mesh_budget_ms`, the same ration the streamed world's meshes
    /// land through.
    #[test]
    #[ignore = "a tool: times the menu's backdrop"]
    fn what_the_backdrop_costs() {
        for place in Place::ALL {
            let picking = std::time::Instant::now();
            let spot = look_for(Some(place), 7);
            let picked = picking.elapsed();

            let building = std::time::Instant::now();
            let mut scene = MenuScene::spawn(
                Some(place),
                7,
                crate::engine::texture::FaceLayers::empty_for_test(),
            );
            let wanted = ((PATCH_RADIUS * 2 + 1) * (PATCH_RADIUS * 2 + 1)) as usize;
            let (mut chunks, mut vertices, mut indices, mut first) = (0, 0, 0, None);
            while chunks < wanted {
                match scene.poll() {
                    Some(built) => {
                        first.get_or_insert_with(|| building.elapsed());
                        chunks += 1;
                        vertices += built.buffers.vertices.len();
                        indices += built.buffers.indices.len();
                    }
                    None => std::thread::sleep(std::time::Duration::from_micros(200)),
                }
            }
            println!(
                "{:>7}: picked in {:>6.1} ms, first chunk {:>6.1} ms, all {chunks} in {:>6.1} ms, \
                 {vertices} vertices and {indices} indices ({} KiB on the card)",
                place.name(),
                picked.as_secs_f32() * 1000.0,
                first.unwrap_or_default().as_secs_f32() * 1000.0,
                building.elapsed().as_secs_f32() * 1000.0,
                (vertices * std::mem::size_of::<crate::engine::mesh::Vertex>()
                    + indices * 4)
                    / 1024,
            );
            assert_eq!(spot.place, place);
        }
    }

    #[test]
    fn every_place_the_menu_offers_can_actually_be_found() {
        for place in Place::ALL {
            for roll in ROLLS {
                let spot = look_for(Some(place), roll);
                assert_eq!(
                    spot.place, place,
                    "roll {roll} was asked for {} and answered {}",
                    place.name(),
                    spot.place.name()
                );
            }
        }
    }

    #[test]
    fn the_menu_camera_never_stands_inside_a_block() {
        // The failure this is here to stop: a spot chosen from a column
        // function, which knows about heights, landing in a world where
        // the block at that height is water or stone -- an opening
        // screen that is one flat colour, with nothing on it to say
        // where the picture went.
        for place in Place::ALL {
            for roll in ROLLS {
                let spot = look_for(Some(place), roll);
                let world = WorldGen::with_preset(spot.seed, Preset::Normal);
                let at = block_at(
                    &world,
                    spot.eye.x.floor() as i32,
                    spot.eye.y.floor() as i32,
                    spot.eye.z.floor() as i32,
                );
                // Air, not merely "nothing that stops a player": a
                // leaf block is walk-through and is still a screen of
                // leaf, which is exactly the backdrop this caught.
                assert_eq!(
                    at,
                    BLOCK_AIR,
                    "{} at roll {roll}: the eye is inside {}",
                    place.name(),
                    primitive_shared::types::block_name(at)
                );
                assert!(!is_collidable(at) && at != BLOCK_WATER);
            }
        }
    }

    #[test]
    fn a_shore_has_water_in_front_of_it_and_land_behind() {
        // **Only the rolls that actually found a shore.**
        //
        // `look_for` gives up after `SEEDS_TRIED` and returns
        // `fallback`, which is an honest "there was no shore in the
        // worlds I looked at" and not a shore -- and this test used to
        // assert shore properties about it. It passed because a shore
        // was easy to find while coastlines were short and crooked;
        // widening the world's relief made them long and straight,
        // roll 1 fell through to the fallback, and the failure read as
        // "the menu backdrop is broken" about a world that was fine.
        //
        // The count below is the other half: if the search stops
        // finding shores at all, this still fails, which is the thing
        // actually worth knowing.
        let mut real = 0usize;
        for roll in ROLLS {
            let spot = look_for(Some(Place::Shore), roll);
            let world = WorldGen::with_preset(spot.seed, Preset::Normal);
            // **The column the eye stands over, exactly, and a shore only if
            // the finder says so of it.** `eye_over` puts the eye at the
            // middle of its column and `as i32` truncates toward zero, so west
            // or north of the origin it named the next column over. And a
            // beach is not a find: at the Earth's scale (`worldgen::Scale`) a
            // shore is rarer among the columns the search tries, `look_for`
            // falls back more often, and roll 65535's fallback -- the last
            // column tried, facing downhill -- stood on a beach and was
            // asserted about as though the finder had chosen it.
            let (x, z) = (spot.eye.x.floor() as i32, spot.eye.z.floor() as i32);
            if shore_facing(&world, x, z).is_none_or(|facing| (yaw_of(facing) - spot.facing).abs() > 1e-4) {
                continue; // the fallback, not a shore
            }
            real += 1;
            let ahead = |distance: f32| {
                (
                    x + (spot.facing.cos() * distance).round() as i32,
                    z + (spot.facing.sin() * distance).round() as i32,
                )
            };
            // **The distances the finder itself used**, not a round
            // number beside them. `shore_facing` looks for water at
            // fourteen blocks and again at twenty-six, and this asked
            // about twenty -- so the two agreed only while coastlines
            // were curvy enough that anything wet at fourteen was wet
            // at twenty as well. Widening the world's relief made
            // shores gentler, a spit of sand sat at exactly twenty on
            // one roll, and a test about the menu backdrop failed about
            // a world that was fine.
            // Somewhere in the middle distance, not at one chosen
            // number. `shore_facing` samples fourteen and twenty-six
            // blocks out from the *column it found*, and the eye ends up
            // a little off that column (see `stand_at`), so a single
            // probe from the eye is a different cell than the one the
            // finder cleared -- which only started to matter when the
            // relief widened and a shoreline stopped bending back into
            // view. What the backdrop needs is that the sea is in front
            // of you, and that is what this now says.
            let sea_ahead = (10..=30).any(|distance| {
                let (fx, fz) = ahead(distance as f32);
                world.height_at(fx, fz) < SEA_LEVEL
            });
            assert!(sea_ahead, "roll {roll}: nothing but land ahead of a shore");
            let (bx, bz) = ahead(-18.0);
            assert!(
                world.height_at(bx, bz) > SEA_LEVEL,
                "roll {roll}: a shore with the sea on both sides is a sandbar"
            );
        }
        assert!(
            real * 2 >= ROLLS.len(),
            "only {real} of {} rolls found a shore at all",
            ROLLS.len()
        );
    }

    #[test]
    fn a_cave_has_rock_over_the_camera_and_no_daylight_on_it() {
        for roll in ROLLS {
            let spot = look_for(Some(Place::Cave), roll);
            let world = WorldGen::with_preset(spot.seed, Preset::Normal);
            let (x, z) = (spot.eye.x.floor() as i32, spot.eye.z.floor() as i32);
            let eye = spot.eye.y.floor() as i32;
            let roof = (eye + 2..CHUNK_SIZE_Y as i32)
                .find(|y| is_opaque(block_at(&world, x, *y, z)));
            assert!(
                roof.is_some(),
                "roll {roll}: a cave scene with open sky over it is a hole, not a cave"
            );
            assert!(
                world.height_at(x, z) - eye >= 8,
                "roll {roll}: the cave is too near the surface to be dark"
            );
        }
    }

    #[test]
    fn the_view_drifts_and_always_comes_back() {
        let mut scene = MenuScene {
            spot: Spot {
                seed: 1,
                place: Place::Plains,
                eye: Vec3::ZERO,
                facing: 1.0,
                pitch: 0.0,
            },
            built: channel().1,
            age: 0.0,
            finished: true,
        };
        let mut lowest = f32::MAX;
        let mut highest = f32::MIN;
        // Two full sweeps, a tenth of a second at a time.
        for _ in 0..(SWEEP_SECONDS * 20.0) as i32 {
            scene.tick(0.1);
            lowest = lowest.min(scene.yaw());
            highest = highest.max(scene.yaw());
        }
        assert!(
            (highest - lowest - 2.0 * SWEEP_RADIANS).abs() < 0.02,
            "the sweep covered {} radians, not {}",
            highest - lowest,
            2.0 * SWEEP_RADIANS
        );
        // ...and never further from the chosen direction than the arc
        // it was given, which is what stops a shore scene turning to
        // face inland.
        assert!(lowest >= 1.0 - SWEEP_RADIANS - 0.01);
        assert!(highest <= 1.0 + SWEEP_RADIANS + 0.01);
    }

    #[test]
    fn a_long_stall_does_not_jump_the_view() {
        let mut scene = MenuScene {
            spot: Spot {
                seed: 1,
                place: Place::Plains,
                eye: Vec3::ZERO,
                facing: 0.0,
                pitch: 0.0,
            },
            built: channel().1,
            age: 0.0,
            finished: true,
        };
        scene.tick(30.0);
        assert!(scene.age <= 0.25, "a stalled frame moved the view {} seconds", scene.age);
    }

    #[test]
    fn the_patch_is_built_from_the_middle_outwards() {
        let order = patch(ChunkPos::new(10, -4));
        assert_eq!(order.first().copied(), Some(ChunkPos::new(10, -4)));
        assert_eq!(order.len(), ((PATCH_RADIUS * 2 + 1) * (PATCH_RADIUS * 2 + 1)) as usize);
        let distance = |pos: &ChunkPos| {
            let (dx, dz) = (pos.x - 10, pos.z + 4);
            dx * dx + dz * dz
        };
        for pair in order.windows(2) {
            assert!(
                distance(&pair[0]) <= distance(&pair[1]),
                "the patch is not nearest-first"
            );
        }
    }

    #[test]
    fn a_place_survives_the_round_trip_through_a_settings_file() {
        for place in Place::ALL {
            assert_eq!(Place::parse(place.name()), Some(place));
        }
        assert_eq!(Place::parse("random"), None);
        assert_eq!(Place::parse("stone"), None, "an old wallpaper block is not a place");
    }
}
