//! Where the solid pass's triangles actually are, band by band.
//!
//! ```text
//! cargo test --release -p primitive_client --lib where_the_triangles_are \
//!     -- --ignored --nocapture
//! LOD_BANDS_SEED=1209552189 LOD_BANDS_VIEW=12 ...
//! ```
//!
//! **Written because a prediction was wrong by a factor of five.** The
//! whole-world measurement in `lod.rs` says a chunk meshed 2x2 sheds 56%
//! of its solid triangles; moving the coarse bands in from ten chunks to
//! four on an Adreno 710 shed 10% of the frame's:
//!
//! ```text
//! stock (lod 10)       255k sent / 340k in view
//! PRIMITIVE_OPT_LOD=4  230k sent / 309k in view
//! ```
//!
//! Both numbers can be true at once, and the question is which chunks the
//! frame is actually made of. The world is a disc and its area grows with
//! the square of the radius, so *most* chunks are far ones -- but a frame
//! is not the disc. It is what survives the fog cull and the frustum, and
//! those two throw away the far ring first and the wide ring always.
//!
//! So this walks the same world the device walks, at the settings the
//! device has, and prints the triangles by band three times over: the
//! whole disc, what the fog leaves, and what a camera leaves on average
//! over eight bearings. Then it does the same at a second `lod_distance`
//! and puts the two side by side. Nothing here draws; it is the mesher and
//! the two culls, which is exactly what decides `tris=` on the `[F3]`
//! line.
//!
//! ## What it found, and where the prediction went wrong
//!
//! At the device's own seat, seed and settings the reproduction lands on
//! the device's own number: 339 339 triangles in view against the 340k the
//! `[F3]` line reported. So the model is right, and these are its answers
//! for in-view solid triangles:
//!
//! ```text
//!                     lod 10      lod 4
//! render distance 12  339_339    258_846   -24%
//! render distance 10  275_566    211_967
//! render distance  8  185_891    149_785
//! ```
//!
//! **The error was applying a whole-world ratio to a frame.** A coarse
//! chunk really does shed 56% of its own triangles (`lod::CELL`), and the
//! disc really is mostly far chunks -- 758k triangles at lod 4 against 994k
//! at lod 10. But a frame is not the disc. The fog cull takes the far ring
//! and the frustum takes five sixths of what is left, and what survives
//! both is weighted towards the near chunks, which are exactly the ones no
//! coarsening may touch. At lod 4 the 28 fine chunks still in view hold 99k
//! of the 259k: **the band the player is standing in is a third of the
//! frame and it is not negotiable.**
//!
//! So the honest ceiling for moving the bands in is about a quarter of the
//! triangles, not four fifths. The device measured a tenth, which is less
//! again, and the `[F3]` line's `detail=fine/coarse/coarser` counter exists
//! to say whether the mesher on the device built what the setting asked
//! for.
//!
//! **And the render distance is the stronger lever of the two.** Twelve
//! chunks to eight is -45% where ten chunks to four is -24%, and the two
//! together are -56%. That is the shape of a disc: a ring's share of the
//! area grows with its radius, so the outermost chunks are both the most
//! numerous and the ones a player is least likely to be looking at when
//! they matter. `PRIMITIVE_OPT_VIEW` is there to put that on a device.
//!
//! ## What it does not count
//!
//! The solid range only, not the leaves a distant chunk sends through the
//! same pass (`MeshBuffers::leaves_solid`, which `dispatch_meshing` decides
//! by `lod::leaves_see_through_at`). On the plains seat the device was
//! benchmarked from there are few enough trees that the totals still match;
//! in a forest they would not, and the first thing to do before trusting a
//! number from one is to teach this the leaf line.

use super::*;
use crate::engine::lod::{band_start, level_at};
use crate::engine::mesh::MeshBuffers;
use crate::settings::ClientSettings;
use glam::Vec3;
use primitive_shared::types::ChunkPos;

/// The seat the device was benchmarked from: the `bench` world's spawn,
/// the player standing still. Overridable, because the next report will
/// come from somewhere else.
const SEED: u32 = 1_209_552_189;
const EYE: Vec3 = Vec3::new(0.5, 83.0, 0.5);

fn number<T: std::str::FromStr>(name: &str, fallback: T) -> T {
    std::env::var(name).ok().and_then(|v| v.parse().ok()).unwrap_or(fallback)
}

/// One band's share of a frame.
#[derive(Default, Clone, Copy)]
struct Band {
    chunks: usize,
    triangles: usize,
}

/// The three bands, plus what they add up to.
#[derive(Default, Clone, Copy)]
struct Bands([Band; 3]);

impl Bands {
    fn add(&mut self, level: u8, triangles: usize) {
        let band = &mut self.0[(level as usize).min(2)];
        band.chunks += 1;
        band.triangles += triangles;
    }

    fn total(&self) -> usize {
        self.0.iter().map(|b| b.triangles).sum()
    }

    fn line(&self, label: &str) -> String {
        let total = self.total();
        format!(
            "{label:>22}: {:>7} tris  = fine {:>7} ({:>3} ch)  + coarse {:>7} ({:>3} ch)  + coarser {:>7} ({:>3} ch)",
            total,
            self.0[0].triangles,
            self.0[0].chunks,
            self.0[1].triangles,
            self.0[1].chunks,
            self.0[2].triangles,
            self.0[2].chunks,
        )
    }
}

/// The world the device sees, meshed at `lod_chunks`, with each chunk's
/// level beside its mesh.
///
/// **`band_start` and the skyline, exactly as `dispatch_meshing` does it**
/// -- a mountain's bands begin nearer than a meadow's, and a measurement
/// that used the raw setting would be measuring a world the game never
/// builds.
fn meshed(
    seed: u32,
    settings: &ClientSettings,
    layers: &crate::engine::texture::FaceLayers,
    centre: ChunkPos,
) -> Vec<(ChunkPos, MeshBuffers, u8)> {
    let radius = settings.render_distance_chunks;
    let world = super::view_distance_repro::stream_seeded(seed, centre, radius);
    let mut out = Vec::with_capacity(world.positions.len());
    for &pos in &world.positions {
        let mut cache = Box::<crate::engine::mesh::Neighbourhood>::default();
        cache.fill(pos, &world.chunks, &world.light);
        let (dx, dz) = ((pos.x - centre.x) as f32, (pos.z - centre.z) as f32);
        let level = level_at(
            (dx * dx + dz * dz).sqrt(),
            band_start(settings.lod_distance_chunks, cache.ceiling()),
            0,
        );
        crate::engine::lod::coarsen(&mut cache, level, settings.lod_quality);
        let mut mesh = MeshBuffers::default();
        crate::engine::mesh::build_mesh(pos, &cache, layers, &world.generator, &mut mesh);
        out.push((pos, mesh, level));
    }
    out
}

/// What one `lod_distance` costs, in three readings.
struct Reading {
    disc: Bands,
    after_fog: Bands,
    /// Averaged over `BEARINGS`, so a seat that happens to face a hill
    /// does not become the answer.
    in_view: Bands,
}

/// Eight bearings, because one is a coincidence. A frustum is a wedge and
/// which wedge it is decides how far out its chunks are; the frame the
/// device reported is one of these, not their sum.
const BEARINGS: [f32; 8] = [-180.0, -135.0, -90.0, -45.0, 0.0, 45.0, 90.0, 135.0];

fn read(seed: u32, settings: &ClientSettings, layers: &crate::engine::texture::FaceLayers) -> Reading {
    let centre = ChunkPos::new(
        (EYE.x / 16.0).floor() as i32,
        (EYE.z / 16.0).floor() as i32,
    );
    let built = meshed(seed, settings, layers, centre);

    let mut disc = Bands::default();
    for (_, mesh, level) in &built {
        disc.add(*level, mesh.solid_index_count as usize / 3);
    }

    // The fog's own cull, the one `render` applies before anything else:
    // a chunk whose centre is past the fog's end contributes nothing.
    let sky = crate::engine::sky::Sky::new(0.32, 900.0);
    let fog = super::view_distance_repro::fog_for(settings, &sky, false);
    let bar = fog.cull_distance().map(|limit| {
        let reach = limit + CHUNK_RADIUS;
        reach * reach
    });
    let mut after_fog = Bands::default();
    let mut kept: Vec<(ChunkPos, usize, u8)> = Vec::new();
    for (pos, mesh, level) in &built {
        let dx = (pos.x as f32 + 0.5) * 16.0 - EYE.x;
        let dz = (pos.z as f32 + 0.5) * 16.0 - EYE.z;
        if bar.is_some_and(|bar| dx * dx + dz * dz > bar) {
            continue;
        }
        let triangles = mesh.solid_index_count as usize / 3;
        after_fog.add(*level, triangles);
        kept.push((*pos, triangles, *level));
    }

    // ...and the frustum, which is `render`'s other cull. Summed over the
    // bearings and divided, so `in_view` is one frame's worth.
    let mut summed = Bands::default();
    for yaw in BEARINGS {
        let mut camera = Camera::new(EYE.as_dvec3(), 1898.0 / 854.0);
        camera.yaw = yaw.to_radians();
        camera.fov_y_radians = settings.fov_degrees.to_radians();
        let origin = Vec3::new(EYE.x.floor(), EYE.y.floor(), EYE.z.floor());
        let frustum = Frustum::from_view_proj(camera.view_proj_about(origin));
        for (pos, triangles, level) in &kept {
            if frustum.contains_chunk(*pos, origin) {
                summed.add(*level, *triangles);
            }
        }
    }
    let mut in_view = Bands::default();
    for (band, summed) in in_view.0.iter_mut().zip(summed.0) {
        band.chunks = summed.chunks / BEARINGS.len();
        band.triangles = summed.triangles / BEARINGS.len();
    }

    Reading { disc, after_fog, in_view }
}

/// The two `lod_distance` settings side by side, as the device measured
/// them.
///
/// Ignored because it is a measurement and not a property: the numbers are
/// this world's and this seat's, and a test that asserted on them would go
/// red the next time the generator changed a hill.
#[test]
#[ignore = "a measurement, not an assertion -- run it explicitly, in release"]
fn where_the_triangles_are() {
    let Some((device, queue)) = crate::engine::test_gpu() else {
        println!("no GPU: the atlas is needed for the face layers, skipped");
        return;
    };
    let assets = std::path::Path::new(concat!(env!("CARGO_MANIFEST_DIR"), "/../assets"));
    let textures = TextureManager::load(device, queue, assets, 1).expect("textures load");
    let layers = textures.face_layers();

    let seed = number("LOD_BANDS_SEED", SEED);
    let view = number("LOD_BANDS_VIEW", 12);
    println!("seed {seed}, eye {EYE:?}, render distance {view} chunks");

    let base = ClientSettings {
        render_distance_chunks: view,
        anisotropy: 1,
        ..Default::default()
    };
    for lod in [number("LOD_BANDS_A", 10), number("LOD_BANDS_B", 4)] {
        let settings = ClientSettings { lod_distance_chunks: lod, ..base.clone() };
        let reading = read(seed, &settings, &layers);
        println!("lod_distance = {lod} chunks");
        println!("  {}", reading.disc.line("the whole disc"));
        println!("  {}", reading.after_fog.line("past the fog cull"));
        println!("  {}", reading.in_view.line("in view, 8 bearings"));
    }
}
