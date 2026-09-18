//! What a render distance of twenty-four actually shows, and what the sea
//! looks like from inside it, photographed through the real passes.
//!
//! ```text
//! GPU_REPRO_DIR=C:/Users/Admin/Downloads/flatcraft/primitive_client/shots-review/view-distance \
//!     cargo test -p primitive_client --release --lib \
//!     what_the_view_distance_shows -- --ignored --nocapture
//! ```
//!
//! Written for a report in three sentences: a render distance of 24 that
//! looked like 10 to 15, fog that hid half of what was loaded, and strange
//! stripes under water. Each is a claim about a picture from where the
//! player stands, and the numbers that decide them -- where the fog starts,
//! where a chunk goes coarse, what the sky is under water -- live in four
//! files, so this puts them in one frame.
//!
//! **Through `draw_scene`**, not `lod_repro::shoot`: the report is about the
//! sky and the fog as much as the ground, and `shoot` has neither a sky pass
//! nor a blended one. What `draw_scene` does not do, this does before calling
//! it -- the fog cull `render` applies to whole chunks, and the level
//! `dispatch_meshing` picks for each one.
//!
//! **"Before" is drawn by this build, one term switched back**, the way
//! `draw_scene` was written to be used. The fog used to begin at 55% of
//! `render distance x 16` and end where the streamed disc does; that frame
//! is this code with the start share set to that same distance. The water
//! used to finish the terrain on a fog colour 1/0.55 of the one the sky was
//! painted with; that frame is this shader with the terrain's under-water
//! fog colour divided by 0.55 again, and no cull. Everything else -- mesh,
//! sky, light, camera -- is the same pixels.
//!
//! **The player's own settings file** (`client_settings.toml` beside the
//! workspace) is read and sanitised as the game reads it, so a number an old
//! file carries is in the picture too.
//!
//! Seed 32 (`saves/mountain`, the world last played). `VIEW_ONLY=land` or
//! `VIEW_ONLY=sea` runs one half.
//!
//! Land, per seat:
//!
//! * `before` / `after` -- the old fog and the new, at the player's settings.
//! * `share80` -- the fog starting at 80% of the reach, the other candidate.
//! * `bands_before` / `bands_after` -- the fog's reach painted on: yellow
//!   where it has taken a tenth to a half of the colour, red past half. Thin
//!   cyan lines where a meadow chunk leaves full detail (light) and drops its
//!   grass (dark); blue lines for a mountain's.
//! * `nofog` -- the fog switched off in the terrain shader only: every chunk
//!   the frame draws, at its own colour.
//! * `fine` and `lodmask` -- every chunk at full detail, and `after` with
//!   every pixel `fine` draws differently painted magenta.
//!
//! Sea, per seat: `before`, `before_flatalpha` (the lid's alpha held at one
//! value), `before_nowater` (the lid left out), `after_nocull` and `after`.
//! The last two must be the same picture, and the tool counts the pixels
//! that are not.
//!
//! **Give `GPU_REPRO_DIR` an absolute directory**: a test runs in the crate's
//! own directory, and a relative one lands there.

use super::*;
use crate::engine::fog::Fog;
use crate::engine::lod::{band_start, coarsen, level_at, MAX_LEVEL, TALL_SKYLINE};
use crate::engine::mesh::{build_mesh, MeshBuffers, Neighbourhood};
use crate::engine::sky::Sky;
use crate::logic::chunk_manager::ChunkManager;
use crate::settings::ClientSettings;
use glam::Vec3;
use primitive_shared::lighting::LightMap;
use primitive_shared::types::{is_liquid, ChunkPos, CHUNK_SIZE_Y};
use primitive_shared::worldgen::{WorldGen, SEA_LEVEL};

const SEED: u32 = 32;
const SIZE: (u32, u32) = (1280, 720);
/// Mid-morning: the sun well up and to one side, so a hillside has a lit
/// face and a shaded one and the fog is the colour of a day.
const HOUR: f32 = 0.40;
const SHADER: &str = include_str!("shader.wgsl");
/// The fog mix in `shade`, which the painted bands hang off.
const FOG_MIX: &str = "color = mix(color, fog_colour, t * t);";
/// Where `shade` picks the colour the fog finishes on.
const FOG_COLOUR: &str = "var fog_colour = globals.fog_color.rgb;";
/// The lid's alpha, closing with the water a ray crosses to reach the bed
/// (see `WATER_DEPTH_FADE`).
const WATER_ALPHA_LINE: &str = "alpha = 1.0 - window * exp(-WATER_DEPTH_FADE * depth / steep);";
/// The share of `render distance x 16` the fog began at before
/// `ClientSettings::fog_start_share` replaced it.
const OLD_FOG_START_RATIO: f32 = 0.55;

fn in_parallel<T: Sync, R: Send>(items: &[T], work: impl Fn(&T) -> R + Sync) -> Vec<R> {
    let threads = std::thread::available_parallelism().map_or(4, |n| n.get());
    let per = items.len().div_ceil(threads).max(1);
    std::thread::scope(|scope| {
        let work = &work;
        let handles: Vec<_> = items
            .chunks(per)
            .map(|batch| scope.spawn(move || batch.iter().map(work).collect::<Vec<R>>()))
            .collect();
        handles.into_iter().flat_map(|h| h.join().unwrap()).collect()
    })
}

/// The settings the report was made with: the player's file if there is
/// one, through the same `sanitize` the game runs it through.
pub(super) fn players_settings() -> ClientSettings {
    let path = concat!(env!("CARGO_MANIFEST_DIR"), "/../client_settings.toml");
    let mut settings = match std::fs::read_to_string(path) {
        Ok(text) => toml::from_str::<ClientSettings>(&text).expect("the player's settings file parses"),
        // The numbers in that file when the report came in, for a checkout
        // without it.
        Err(_) => ClientSettings {
            render_distance_chunks: 24,
            fov_degrees: 90.0,
            anisotropy: 16,
            detail_distance: 1.0,
            ..ClientSettings::default()
        },
    };
    settings.sanitize();
    settings
}

/// `settings` with the fog laid out the way it was before the share: from
/// 55% of `render distance x 16` to where the disc ends.
fn with_old_fog(settings: &ClientSettings) -> ClientSettings {
    let reach = ChunkManager::reach_blocks(settings.render_distance_chunks);
    let old_start = settings.render_distance_chunks as f32 * 16.0 * OLD_FOG_START_RATIO;
    ClientSettings { fog_start_share: old_start / reach, ..settings.clone() }
}

pub(super) struct World {
    pub(super) generator: WorldGen,
    pub(super) chunks: ChunkManager,
    pub(super) light: LightMap,
    pub(super) positions: Vec<ChunkPos>,
}

/// The disc the streamer keeps round `centre` at `radius`, generated and lit.
pub(super) fn stream(centre: ChunkPos, radius: i32) -> World {
    let generator = WorldGen::new(SEED);
    let probe = ChunkManager::new(radius);
    let mut positions = Vec::new();
    for dz in -radius..=radius {
        for dx in -radius..=radius {
            if probe.inside(dx, dz) {
                positions.push(ChunkPos::new(centre.x + dx, centre.z + dz));
            }
        }
    }
    let generated = in_parallel(&positions, |&pos| generator.generate_chunk(pos));
    let isolated =
        in_parallel(&generated, |chunk| primitive_shared::lighting::compute_isolated(&chunk.blocks));
    let mut chunks = ChunkManager::new(radius);
    for chunk in generated {
        chunks.insert(chunk);
    }
    let mut light = LightMap::new();
    for (pos, data) in positions.iter().zip(isolated) {
        light.insert_precomputed(&chunks, *pos, data);
    }
    World { generator, chunks, light, positions }
}

pub(super) struct Meshed {
    pub(super) meshes: Vec<(ChunkPos, MeshBuffers)>,
    /// The level each chunk was built at, in the order of `meshes`.
    levels: Vec<u8>,
}

/// Every chunk of `world`, at the level `dispatch_meshing` would pick for an
/// eye standing in `eye_chunk` -- or at full detail when `detail` is false.
pub(super) fn mesh_world(
    world: &World,
    layers: &crate::engine::texture::FaceLayers,
    settings: &ClientSettings,
    eye_chunk: ChunkPos,
    detail: bool,
) -> Meshed {
    let built = in_parallel(&world.positions, |&pos| {
        let mut cache = Box::<Neighbourhood>::default();
        cache.fill(pos, &world.chunks, &world.light);
        let level = if detail {
            let (dx, dz) = ((pos.x - eye_chunk.x) as f32, (pos.z - eye_chunk.z) as f32);
            level_at(
                (dx * dx + dz * dz).sqrt(),
                band_start(settings.lod_distance_chunks, cache.ceiling()),
                0,
            )
        } else {
            0
        };
        coarsen(&mut cache, level, settings.lod_quality);
        let mut out = MeshBuffers::default();
        build_mesh(pos, &cache, layers, &world.generator, &mut out);
        (pos, out, level)
    });
    let mut meshed = Meshed { meshes: Vec::new(), levels: Vec::new() };
    for (pos, out, level) in built {
        meshed.meshes.push((pos, out));
        meshed.levels.push(level);
    }
    meshed
}

/// The fog for a frame, built the way `frame_params` builds it.
pub(super) fn fog_for(settings: &ClientSettings, sky: &Sky, submerged: bool) -> Fog {
    let mut fog = Fog::for_frame(settings, sky, settings.render_distance_chunks, true, submerged);
    fog.clamp_to(ChunkManager::reach_blocks(settings.render_distance_chunks));
    fog
}

/// Empties every chunk `render` would not draw past the fog, and says how
/// many that was. See `fog_cull_squared` in `render`.
pub(super) fn cull_past_the_fog(meshes: &mut [(ChunkPos, MeshBuffers)], eye: Vec3, fog: &Fog) -> usize {
    let Some(limit) = fog.cull_distance() else {
        return 0;
    };
    let bar = (limit + CHUNK_RADIUS) * (limit + CHUNK_RADIUS);
    let mut culled = 0;
    for (pos, mesh) in meshes.iter_mut() {
        let dx = (pos.x as f32 + 0.5) * 16.0 - eye.x;
        let dz = (pos.z as f32 + 0.5) * 16.0 - eye.z;
        if dx * dx + dz * dz > bar && !mesh.indices.is_empty() {
            mesh.indices.clear();
            culled += 1;
        }
    }
    culled
}

/// The terrain shader with `anchor` replaced, which has to be there exactly
/// once: a control whose edit silently missed would draw the game's frame
/// and be read as proof that the term made no difference.
fn variant(anchor: &str, replacement: &str) -> String {
    assert_eq!(
        SHADER.matches(anchor).count(),
        1,
        "shader.wgsl no longer holds `{anchor}` exactly once; the control would not be one"
    );
    SHADER.replacen(anchor, replacement, 1)
}

/// The shader as it was before the water's colours were made one: the
/// terrain under water finishing on the fog colour over 0.55, which is what
/// `fog::UNDERWATER` was before it took the 55% into itself.
fn old_water_source() -> String {
    variant(
        FOG_COLOUR,
        "var fog_colour = globals.fog_color.rgb / select(1.0, 0.55, globals.extra.z > 0.5);",
    )
}

/// The shader with the fog's reach and the detail thresholds painted on.
fn bands_source(lines: &[(f32, [f32; 3])]) -> String {
    let mut body = String::new();
    for (at, rgb) in lines {
        body.push_str(&format!(
            "    if (abs(distance - {at:.1}) < width) {{ painted = vec3<f32>({:.2}, {:.2}, {:.2}); }}\n",
            rgb[0], rgb[1], rgb[2]
        ));
    }
    let function = format!(
        "\n// Painted over the fogged colour by `view_distance_repro`.\n\
         fn view_bands(color: vec3<f32>, fogged: f32, distance: f32) -> vec3<f32> {{\n\
         \x20   var painted = color;\n\
         \x20   if (fogged >= 0.5) {{ painted = mix(color, vec3<f32>(0.9, 0.1, 0.1), 0.45); }}\n\
         \x20   else if (fogged >= 0.1) {{ painted = mix(color, vec3<f32>(0.95, 0.85, 0.1), 0.45); }}\n\
         \x20   let width = max(0.75, distance * 0.006);\n\
         {body}\
         \x20   return painted;\n\
         }}\n"
    );
    variant(FOG_MIX, &format!("{FOG_MIX}\n        color = view_bands(color, t * t, distance);")) + function.as_str()
}

/// Pixels of `a` and `b` that differ by more than `threshold` summed over the
/// three channels, and `a` with them painted magenta.
fn difference(a: &image::RgbaImage, b: &image::RgbaImage, threshold: i32) -> (u32, image::RgbaImage) {
    let mut marked = a.clone();
    let mut changed = 0u32;
    for (x, y, p) in a.enumerate_pixels() {
        let q = b.get_pixel(x, y);
        let d: i32 = (0..3).map(|i| (p[i] as i32 - q[i] as i32).abs()).sum();
        if d > threshold {
            changed += 1;
            marked.put_pixel(x, y, image::Rgba([255, 0, 255, 255]));
        }
    }
    (changed, marked)
}

struct Shot<'a> {
    device: &'a wgpu::Device,
    queue: &'a wgpu::Queue,
    textures: &'a TextureManager,
    sky: &'a Sky,
}

impl Shot<'_> {
    #[allow(clippy::too_many_arguments)]
    fn take(
        &self,
        settings: &ClientSettings,
        meshes: &[(ChunkPos, MeshBuffers)],
        eye: Vec3,
        (yaw, pitch): (f32, f32),
        source: &str,
        submerged: bool,
    ) -> image::RgbaImage {
        let mut camera = Camera::new(eye.as_dvec3(), SIZE.0 as f32 / SIZE.1 as f32);
        camera.yaw = yaw.to_radians();
        camera.pitch = pitch.to_radians();
        camera.fov_y_radians = settings.fov_degrees.to_radians();
        super::offscreen_repro::draw_scene(
            self.device,
            self.queue,
            self.textures,
            settings,
            &camera,
            self.sky,
            meshes,
            SIZE,
            source,
            Some(ChunkManager::reach_blocks(settings.render_distance_chunks)),
            None,
            submerged,
            settings.msaa,
        )
    }
}

/// North, east, south and west, as the camera's yaw in degrees.
const COMPASS: [(&str, f32); 4] = [("n", -90.0), ("e", 0.0), ("s", 90.0), ("w", 180.0)];

fn print_fog(label: &str, fog: &Fog) {
    let at = |fogged: f32| fog.start + (fog.end - fog.start) * fogged.sqrt();
    println!(
        "  fog {label:>7}: begins {:.0} ({:.1} ch), 10% {:.0} ({:.1} ch), 50% {:.0} ({:.1} ch), complete {:.0} ({:.1} ch)",
        fog.start,
        fog.start / 16.0,
        at(0.1),
        at(0.1) / 16.0,
        at(0.5),
        at(0.5) / 16.0,
        fog.end,
        fog.end / 16.0
    );
}

fn land(shot: &Shot, settings: &ClientSettings, out: &str) {
    let generator = WorldGen::new(SEED);
    let (sx, sz) = generator.spawn_column();
    let spawn_chunk = ChunkPos::from_global(sx, sz).0;
    let started = std::time::Instant::now();
    let world = stream(spawn_chunk, settings.render_distance_chunks);
    let layers = shot.textures.face_layers();
    let mut game = mesh_world(&world, &layers, settings, spawn_chunk, true);
    let mut fine = mesh_world(&world, &layers, settings, spawn_chunk, false);
    println!(
        "land: {} chunks round spawn ({sx}, {sz}), streamed and meshed twice in {:.1}s",
        world.positions.len(),
        started.elapsed().as_secs_f32()
    );

    let before = with_old_fog(settings);
    let candidate = ClientSettings { fog_start_share: 0.8, ..settings.clone() };
    let (fog_before, fog_after) = (fog_for(&before, shot.sky, false), fog_for(settings, shot.sky, false));
    println!(
        "render distance {} chunks = {} blocks; the streamed disc ends {:.0} blocks out at its nearest",
        settings.render_distance_chunks,
        settings.render_distance_chunks * 16,
        ChunkManager::reach_blocks(settings.render_distance_chunks)
    );
    print_fog("before", &fog_before);
    print_fog("after", &fog_after);
    print_fog("share80", &fog_for(&candidate, shot.sky, false));

    let lod = settings.lod_distance_chunks;
    let mut lines = Vec::new();
    if lod > 0 {
        // A chunk nobody has meshed yet goes coarse a chunk *past* the
        // threshold (`lod::HYSTERESIS`), and the distance is the chunk's
        // centre -- so these lines are where the bands fall to within half a
        // chunk, not to the block.
        let meadow = band_start(lod, SEA_LEVEL) as f32;
        let mountain = band_start(lod, TALL_SKYLINE) as f32;
        println!(
            "detail {:?} from {lod}: a meadow chunk goes coarse past {} ch and drops its grass past {} ch; \
             a mountain chunk (skyline >= {TALL_SKYLINE}) past {} ch and {} ch",
            settings.lod_quality,
            meadow + 1.0,
            meadow * 2.0 + 1.0,
            mountain + 1.0,
            mountain * 2.0 + 1.0
        );
        lines.push(((meadow + 1.0) * 16.0, [0.1, 0.95, 0.95]));
        lines.push(((meadow * 2.0 + 1.0) * 16.0, [0.0, 0.45, 0.5]));
        lines.push(((mountain + 1.0) * 16.0, [0.2, 0.35, 1.0]));
        lines.push(((mountain * 2.0 + 1.0) * 16.0, [0.05, 0.1, 0.45]));
    } else {
        println!("detail: off, every chunk at full detail");
    }
    for level in 0..=MAX_LEVEL {
        let (count, triangles) = game
            .levels
            .iter()
            .zip(&game.meshes)
            .filter(|(built, _)| **built == level)
            .fold((0, 0), |(count, triangles), (_, (_, mesh))| (count + 1, triangles + mesh.indices.len() / 3));
        println!("  level {level}: {count:5} chunks, {:6}k triangles", triangles / 1000);
    }
    let fine_triangles: usize = fine.meshes.iter().map(|(_, mesh)| mesh.indices.len() / 3).sum();
    println!("  all at full detail: {}k triangles", fine_triangles / 1000);

    let ground = generator.height_at(sx, sz).max(SEA_LEVEL) as f32;
    let spawn_eye = Vec3::new(sx as f32 + 0.5, ground + 2.62, sz as f32 + 0.5);
    // One cull serves both fogs: they end at the same place, the disc.
    assert_eq!(fog_before.cull_distance(), fog_after.cull_distance());
    let culled = cull_past_the_fog(&mut game.meshes, spawn_eye, &fog_after);
    cull_past_the_fog(&mut fine.meshes, spawn_eye, &fog_after);
    println!("  {culled} chunks past the fog, left out as `render` leaves them out");

    let bands = bands_source(&lines);
    let nofog = {
        let anchor = "if (globals.extra.w > 0.5) {";
        assert!(SHADER.contains(anchor), "shader.wgsl no longer switches its fog on `extra.w`");
        SHADER.replace(anchor, "if (globals.extra.w > 1.5) {")
    };
    // Standing at spawn looking out, and forty blocks up -- a tower, a peak
    // -- where the horizon is not hidden by the next hill. Both in the spawn
    // chunk, so the detail levels above are the ones this eye would get.
    let mut seats: Vec<(String, Vec3, (f32, f32))> = Vec::new();
    for (name, yaw) in COMPASS {
        seats.push((format!("spawn_{name}"), spawn_eye, (yaw, -2.0)));
    }
    for (name, yaw) in COMPASS {
        seats.push((format!("above_{name}"), spawn_eye + Vec3::Y * 40.0, (yaw, -10.0)));
    }
    for (name, eye, look) in &seats {
        let path = |variant: &str| format!("{out}/land_{name}_{variant}.png");
        let save = |picture: image::RgbaImage, variant: &str| picture.save(path(variant)).expect("write png");
        let after = shot.take(settings, &game.meshes, *eye, *look, SHADER, false);
        let fine_picture = shot.take(settings, &fine.meshes, *eye, *look, SHADER, false);
        let (changed, mask) = difference(&after, &fine_picture, 24);
        save(shot.take(&before, &game.meshes, *eye, *look, SHADER, false), "before");
        save(shot.take(&candidate, &game.meshes, *eye, *look, SHADER, false), "share80");
        save(shot.take(&before, &game.meshes, *eye, *look, &bands, false), "bands_before");
        save(shot.take(settings, &game.meshes, *eye, *look, &bands, false), "bands_after");
        save(shot.take(settings, &game.meshes, *eye, *look, &nofog, false), "nofog");
        save(after, "after");
        save(fine_picture, "fine");
        save(mask, "lodmask");
        println!(
            "  {name:>8}: the detail bands change {:.1}% of the frame",
            changed as f32 * 100.0 / (SIZE.0 * SIZE.1) as f32
        );
    }
}

fn sea(shot: &Shot, settings: &ClientSettings, out: &str) {
    let Some((x, z)) = open_water(&WorldGen::new(SEED)) else {
        println!("sea: no open water within 2400 blocks of spawn; no sea pictures");
        return;
    };
    let centre = ChunkPos::from_global(x, z).0;
    let world = stream(centre, settings.render_distance_chunks);
    let column = world.chunks.column(x, z).expect("the middle of the disc is loaded");
    let top = (0..CHUNK_SIZE_Y as i32).rev().find(|y| is_liquid(column.block(*y))).expect("water in a sea");
    let bed = (0..top).rev().find(|y| !is_liquid(column.block(*y))).unwrap_or(0);
    println!("sea: at ({x}, {z}), water from {} to {top}", bed + 1);
    let layers = shot.textures.face_layers();
    let mut game = mesh_world(&world, &layers, settings, centre, true);
    let fog = fog_for(settings, shot.sky, true);
    println!(
        "  under water: fog {:.1}..{:.1} blocks, colour {:?}, cull {:?}",
        fog.start, fog.end, fog.color, fog.cull_distance()
    );

    let old_water = old_water_source();
    let old_flat_alpha = old_water.replacen(WATER_ALPHA_LINE, "alpha = WATER_ALPHA;", 1);
    let old_no_water = old_water.replacen(WATER_ALPHA_LINE, "alpha = 0.0;", 1);
    assert!(old_flat_alpha != old_water && old_no_water != old_water, "the water's alpha line moved");
    let (fx, fz) = (x as f32 + 0.5, z as f32 + 0.5);
    let mut seats: Vec<(String, Vec3, (f32, f32))> = Vec::new();
    // Half a block under the lid, which is where a swimmer's eye is.
    let shallow = Vec3::new(fx, top as f32 + 0.35, fz);
    for (name, yaw) in COMPASS {
        seats.push((format!("shallow_{name}"), shallow, (yaw, 0.0)));
    }
    seats.push(("shallow_up".to_string(), shallow, (0.0, 25.0)));
    let deep_eye = Vec3::new(fx, bed as f32 + 3.5, fz);
    seats.push(("deep_e".to_string(), deep_eye, (0.0, 0.0)));
    seats.push(("deep_w".to_string(), deep_eye, (180.0, 0.0)));
    seats.push(("deep_up".to_string(), deep_eye, (0.0, 35.0)));
    seats.push(("deep_down".to_string(), deep_eye, (0.0, -20.0)));

    // Everything drawn from the whole disc first, then the cull, then the
    // frame the game now draws -- which has to equal the uncut one.
    let mut uncut = Vec::new();
    for (name, eye, look) in &seats {
        let path = |variant: &str| format!("{out}/sea_{name}_{variant}.png");
        shot.take(settings, &game.meshes, *eye, *look, &old_water, true).save(path("before")).expect("write png");
        shot.take(settings, &game.meshes, *eye, *look, &old_flat_alpha, true)
            .save(path("before_flatalpha"))
            .expect("write png");
        shot.take(settings, &game.meshes, *eye, *look, &old_no_water, true)
            .save(path("before_nowater"))
            .expect("write png");
        let picture = shot.take(settings, &game.meshes, *eye, *look, SHADER, true);
        picture.save(path("after_nocull")).expect("write png");
        uncut.push(picture);
    }
    let drawn = game.meshes.iter().filter(|(_, mesh)| !mesh.indices.is_empty()).count();
    let culled = cull_past_the_fog(&mut game.meshes, Vec3::new(fx, 0.0, fz), &fog);
    println!("  the fog cull leaves {} of {drawn} chunks to draw under water", drawn - culled);
    for ((name, eye, look), uncut) in seats.iter().zip(&uncut) {
        let picture = shot.take(settings, &game.meshes, *eye, *look, SHADER, true);
        let (changed, _) = difference(&picture, uncut, 3);
        picture.save(format!("{out}/sea_{name}_after.png")).expect("write png");
        println!("  {name:>10}: {changed} pixels differ between the culled frame and the whole disc");
    }
}

#[test]
#[ignore = "a tool: needs a GPU; photographs the view distance and the sea at the player's settings"]
fn what_the_view_distance_shows() {
    let Some((device, queue)) = crate::engine::test_gpu() else {
        println!("no GPU adapter on this machine; skipping");
        return;
    };
    let out = std::env::var("GPU_REPRO_DIR").unwrap_or_else(|_| ".".to_string());
    std::fs::create_dir_all(&out).expect("output directory");
    let only = std::env::var("VIEW_ONLY").ok();
    let settings = players_settings();
    println!(
        "settings: render distance {}, fov {}, anisotropy {}, msaa {}, fog {} (begins at {} of the reach), \
         detail from {} ({:?}), grass {:.0}%",
        settings.render_distance_chunks,
        settings.fov_degrees,
        settings.anisotropy,
        settings.msaa,
        settings.fog_enabled,
        settings.fog_start_share,
        settings.lod_distance_chunks,
        settings.lod_quality,
        settings.detail_distance * 100.0
    );
    let assets = std::path::Path::new(concat!(env!("CARGO_MANIFEST_DIR"), "/../assets"));
    let textures = TextureManager::load(device, queue, assets, settings.anisotropy).expect("textures load");
    let sky = Sky::new(HOUR, 900.0);
    let shot = Shot { device, queue, textures: &textures, sky: &sky };
    if only.as_deref() != Some("sea") {
        land(&shot, &settings, &out);
    }
    if only.as_deref() != Some("land") {
        sea(&shot, &settings, &out);
    }
    println!("pictures in {out}");
}

#[test]
fn under_water_the_terrain_past_the_fog_is_the_colour_of_the_sky_behind_it() {
    // **The strange stripes under water.** The terrain finished its fog on
    // `fog.color` while `fs_sky` painted 55% of it, so everything past
    // eighteen blocks -- the far bed, the underside of the surface -- came
    // out as flat bands brighter than the gaps between them. Stated as the
    // property rather than the constant: through the real terrain shader
    // and the real sky pass, a submerged eye looking at a wall well past
    // the fog's end sees the wall and the sky over it as one colour. It is
    // also the whole of what makes `Fog::cull_distance` free under water.
    let Some((device, queue)) = crate::engine::test_gpu() else {
        println!("no GPU adapter on this machine; skipping");
        return;
    };
    use primitive_shared::types::{Chunk, BLOCK_AIR, BLOCK_STONE, CHUNK_SIZE_X, CHUNK_SIZE_Z, CHUNK_VOLUME};
    let assets = std::path::Path::new(concat!(env!("CARGO_MANIFEST_DIR"), "/../assets"));
    let textures = TextureManager::load(device, queue, assets, 1).expect("textures load");
    // Three chunks in a row; the third is solid rock to forty blocks up.
    let mut chunks = ChunkManager::new(8);
    for cx in 0..3 {
        let fill = if cx == 2 { BLOCK_STONE } else { BLOCK_AIR };
        let mut blocks = vec![BLOCK_AIR; CHUNK_VOLUME];
        for y in 0..40 {
            for z in 0..CHUNK_SIZE_Z {
                for x in 0..CHUNK_SIZE_X {
                    blocks[Chunk::index(x, y, z)] = fill;
                }
            }
        }
        chunks.insert(Chunk { pos: ChunkPos::new(cx, 0), blocks });
    }
    let mut light = LightMap::new();
    for cx in 0..3 {
        light.load_chunk(&chunks, ChunkPos::new(cx, 0));
    }
    let generator = WorldGen::new(0);
    let meshes: Vec<(ChunkPos, MeshBuffers)> = (0..3)
        .map(|cx| {
            let pos = ChunkPos::new(cx, 0);
            let mut cache = Box::<Neighbourhood>::default();
            cache.fill(pos, &chunks, &light);
            let mut out = MeshBuffers::default();
            build_mesh(pos, &cache, &textures.face_layers(), &generator, &mut out);
            (pos, out)
        })
        .collect();
    let settings = ClientSettings::default();
    let sky = Sky::new(0.5, 900.0);
    // Twenty-seven blocks from the wall, with the fog done at eighteen; the
    // wall's top at forty stands 35 degrees over the eye, so the top of a
    // 90-degree view is sky.
    let mut camera = Camera::new((Vec3::new(4.5, 20.5, 8.5)).as_dvec3(), 1.0);
    camera.yaw = 0.0;
    camera.fov_y_radians = 90f32.to_radians();
    let fog = Fog::for_frame(&settings, &sky, settings.render_distance_chunks, true, true);
    assert!(fog.end < 27.0, "the wall is not past the fog, so this tests nothing");
    // 128 across, because `draw_scene` reads the frame back a row at a time
    // and a row has to be a whole number of 256-byte blocks; 96 was refused.
    let size = (128, 128);
    let picture = super::offscreen_repro::draw_scene(
        device, queue, &textures, &settings, &camera, &sky, &meshes, size, SHADER, None, None, true, 1,
    );
    let (wall, over_it) = (picture.get_pixel(64, 80), picture.get_pixel(64, 4));
    for channel in 0..3 {
        assert!(
            (wall[channel] as i32 - over_it[channel] as i32).abs() <= 2,
            "past the fog the wall is {wall:?} and the sky over it is {over_it:?}"
        );
    }
}

/// The smooth interpolants the terrain structs carry, as declared and at a
/// covered sample. Replaced everywhere at once, because the three structs
/// that carry them have to agree or a pipeline is refused.
const PLAIN_DISTANCE: &str = "    @location(2) view_distance: f32,";
const CENTROID_DISTANCE: &str = "    @location(2) @interpolate(perspective, centroid) view_distance: f32,";
const PLAIN_LIGHT: &str = "    @location(3) light_terms: vec3<f32>,";
const CENTROID_LIGHT: &str = "    @location(3) @interpolate(perspective, centroid) light_terms: vec3<f32>,";
const PLAIN_CELL: &str = "    @location(7) shade_cell: vec3<f32>,";
const CENTROID_CELL: &str = "    @location(7) @interpolate(perspective, centroid) shade_cell: vec3<f32>,";

/// Open water nearest spawn: twelve blocks deep and a couple of dozen blocks
/// either way, so an eye put there is in a sea and not in a pit.
fn open_water(generator: &WorldGen) -> Option<(i32, i32)> {
    let (sx, sz) = generator.spawn_column();
    let deep = |x: i32, z: i32| generator.height_at(x, z) <= SEA_LEVEL - 12;
    let open = |x: i32, z: i32| deep(x, z) && deep(x + 24, z) && deep(x - 24, z) && deep(x, z + 24) && deep(x, z - 24);
    for radius in (8..=2400).step_by(8) {
        for t in (-radius..=radius).step_by(8) {
            for (dx, dz) in [(t, -radius), (t, radius), (-radius, t), (radius, t)] {
                if open(sx + dx, sz + dz) {
                    return Some((sx + dx, sz + dz));
                }
            }
        }
    }
    None
}

fn luma(p: &image::Rgba<u8>) -> i32 {
    (2126 * p[0] as i32 + 7152 * p[1] as i32 + 722 * p[2] as i32) / 10000
}

/// What the pale specks under water are made of.
///
/// ```text
/// GPU_REPRO_DIR=C:/Users/Admin/Downloads/flatcraft/primitive_client/shots-review/view-distance \
///     cargo test -p primitive_client --lib what_the_specks_under_water_are -- --ignored --nocapture
/// ```
///
/// Once the water's far colour was made the sky's, the frames of the kelp
/// in `what_the_view_distance_shows` showed columns of specks paler than
/// the water round them. The first guess -- `view_distance` extrapolated
/// out of a sliver under multisampling, leaving it unfogged -- was put to a
/// synthetic field of tufts past the fog and left no speck with either
/// interpolant, so the question is asked again of the real place: the same
/// sea, the same seats, a disc of three (the fog cull leaves eleven chunks
/// under water, and the frame drawn from them equalled the whole disc), and
/// the terrain shader four ways -- as declared, `view_distance` at a covered
/// sample, every smooth interpolant at a covered sample, and one sample a
/// pixel. For each, how many pixels the plain frame draws brighter by more
/// than thirty levels, and where (`*_brighter_*`).
///
/// **What the first pass said.** On all three seats `view_distance` at a
/// covered sample, and every smooth interpolant at a covered sample, changed
/// 0 pixels; one sample a pixel changed only the antialiased edges (50 to
/// 1197 pixels, at most 53 of them brighter). A centroid `view_distance`,
/// made to cure the specks before this was asked, was put back -- it cured
/// nothing this could measure. That pass also drew with a render distance of
/// three, which past 21.6 blocks draws the leaf range solid (`render`'s
/// `leaf_cutout_limit`), and there were no specks in it at all; at the
/// player's 24 there are. So the second pass streams three chunks but draws
/// at the player's distance, one sample a pixel, and asks every bright pixel
/// below the horizon two things through a probe shader that writes cut-out
/// fragments magenta with their distance in alpha: is it a cut-out fragment,
/// and how far away is it.
#[test]
#[ignore = "a tool: needs a GPU; takes the specks under water apart"]
fn what_the_specks_under_water_are() {
    let Some((device, queue)) = crate::engine::test_gpu() else {
        println!("no GPU adapter on this machine; skipping");
        return;
    };
    let out = std::env::var("GPU_REPRO_DIR").unwrap_or_else(|_| ".".to_string());
    std::fs::create_dir_all(&out).expect("output directory");
    let settings = players_settings();
    let assets = std::path::Path::new(concat!(env!("CARGO_MANIFEST_DIR"), "/../assets"));
    let textures = TextureManager::load(device, queue, assets, settings.anisotropy).expect("textures load");
    let sky = Sky::new(HOUR, 900.0);
    let (x, z) = open_water(&WorldGen::new(SEED)).expect("open water near spawn");
    let centre = ChunkPos::from_global(x, z).0;
    // Three chunks streamed, the player's distance drawn: the fog cull keeps
    // eleven under water, all inside this disc.
    let world = stream(centre, 3);
    let probe = variant(
        "    if (sampled.a < ALPHA_CUTOFF) {\n        discard;\n    }\n    return shade(in, sampled);",
        "    if (sampled.a < ALPHA_CUTOFF || in.translucent != 0u) {\n        discard;\n    }\n    \
         return vec4<f32>(1.0, 0.0, 1.0, clamp(in.view_distance / 64.0, 0.0, 1.0));",
    );
    let column = world.chunks.column(x, z).expect("the middle of the disc is loaded");
    let top = (0..CHUNK_SIZE_Y as i32).rev().find(|y| is_liquid(column.block(*y))).expect("water in a sea");
    let bed = (0..top).rev().find(|y| !is_liquid(column.block(*y))).unwrap_or(0);
    let layers = textures.face_layers();
    let meshed = mesh_world(&world, &layers, &settings, centre, true);
    println!("specks: sea at ({x}, {z}), water from {} to {top}, {} chunks", bed + 1, meshed.meshes.len());

    let plain = SHADER.replace(CENTROID_DISTANCE, PLAIN_DISTANCE);
    let distance = plain.replace(PLAIN_DISTANCE, CENTROID_DISTANCE);
    let every = distance.replace(PLAIN_LIGHT, CENTROID_LIGHT).replace(PLAIN_CELL, CENTROID_CELL);
    assert!(distance != plain && every != distance, "an interpolant this tool switches is no longer in shader.wgsl");

    let (fx, fz) = (x as f32 + 0.5, z as f32 + 0.5);
    let shallow = Vec3::new(fx, top as f32 + 0.35, fz);
    let deep = Vec3::new(fx, bed as f32 + 3.5, fz);
    let seats = [("shallow_up", shallow, (0.0, 25.0)), ("shallow_n", shallow, (-90.0, 0.0)), ("deep_e", deep, (0.0, 0.0))];
    for (seat, eye, (yaw, pitch)) in seats {
        let draw = |source: &str, samples: u32| {
            let mut camera = Camera::new(eye.as_dvec3(), SIZE.0 as f32 / SIZE.1 as f32);
            camera.yaw = f32::to_radians(yaw);
            camera.pitch = f32::to_radians(pitch);
            camera.fov_y_radians = settings.fov_degrees.to_radians();
            super::offscreen_repro::draw_scene(
                device, queue, &textures, &settings, &camera, &sky, &meshed.meshes, SIZE, source, None, None, true, samples,
            )
        };
        let as_declared = draw(&plain, settings.msaa);
        as_declared.save(format!("{out}/specks_{seat}_as_declared.png")).expect("write png");
        // The probe: every cut-out fragment magenta, its distance in alpha
        // over 64 blocks, so a speck that is a leaf or a plant seen through
        // the water says so and says how far off it is.
        let probed = draw(&probe, 1);
        let cutouts = probed.pixels().filter(|p| p[0] == 255 && p[1] == 0 && p[2] == 255).count();
        let nearest = probed
            .pixels()
            .filter(|p| p[0] == 255 && p[1] == 0 && p[2] == 255)
            .map(|p| p[3] as f32 / 255.0 * 64.0)
            .fold(f32::INFINITY, f32::min);
        probed.save(format!("{out}/specks_{seat}_cutouts.png")).expect("write png");
        println!("  {seat:>10}: {cutouts} cut-out pixels, the nearest {nearest:.1} blocks away");
        for (label, picture) in [
            ("distance_centroid", draw(&distance, settings.msaa)),
            ("all_centroid", draw(&every, settings.msaa)),
            ("one_sample", draw(&plain, 1)),
        ] {
            let mut marked = as_declared.clone();
            let mut brighter = 0u32;
            for (px, py, p) in as_declared.enumerate_pixels() {
                if luma(p) > luma(picture.get_pixel(px, py)) + 30 {
                    brighter += 1;
                    marked.put_pixel(px, py, image::Rgba([255, 0, 255, 255]));
                }
            }
            let (differ, _) = difference(&as_declared, &picture, 30);
            picture.save(format!("{out}/specks_{seat}_{label}.png")).expect("write png");
            marked.save(format!("{out}/specks_{seat}_brighter_than_{label}.png")).expect("write png");
            println!("  {seat:>10} vs {label:>17}: {differ:6} pixels differ, {brighter:6} brighter as declared");
        }
    }
}
