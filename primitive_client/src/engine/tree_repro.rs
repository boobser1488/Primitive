//! **Every kind of tree, alone, through the real terrain shader**: the oak,
//! the birch, the fir, the pine, the willow, the saxaul, the palm, the apple
//! tree, the maple and the acacia, each lifted out of a generated world with
//! its wood and its leaves and stood on a lawn by itself, at the player's
//! field of view (95) and anisotropy (16), from four sides and from under
//! its crown.
//!
//! ```text
//! GPU_REPRO_DIR=C:/absolute/dir \
//!     cargo test -p primitive_client --lib what_every_tree_looks_like -- --ignored --nocapture
//! ```
//!
//! `TREE_KINDS=oak,maple` takes only those; `TREE_AT_<kind>=x,y,z` lifts the
//! tree whose foot is there, for a before and an after of the same tree.
//!
//! Written for "у деревьев проблема с креплением листвы на ветки". **Alone,
//! because in a wood the question cannot be asked**: the next crown is a
//! hand's width from any eye put near a tree, and a leaf in front of a limb
//! is not the limb's leaf. Lifted at its real place in the world, so the
//! climate tints it as it is tinted where it grows. The tree is everything
//! of wood or leaf joined to its foot through the twenty-six cells round
//! each cell, inside the reach of the widest tree -- so a leaf that touches
//! its crown only at a corner comes with it, and one that touches nothing
//! does not, which is what the counting test in the generator looks for.

use super::*;
use crate::engine::mesh::{build_mesh, MeshBuffers, Neighbourhood};
use crate::engine::sky::Sky;
use crate::logic::chunk_manager::ChunkManager;
use crate::settings::ClientSettings;
use glam::Vec3;
use primitive_shared::lighting::LightMap;
use primitive_shared::types::{
    block_kind, is_branch, is_leafy, BlockId, Chunk, BLOCK_ACACIA_LEAVES, BLOCK_AIR, BLOCK_APPLE_LEAVES, BLOCK_BIRCH_LEAVES,
    BLOCK_DIRT, BLOCK_FIR_NEEDLES, BLOCK_GRASS, BLOCK_LEAVES, BLOCK_MAPLE_LEAVES, BLOCK_PALM_FRONDS,
    BLOCK_PALM_TRUNK, BLOCK_PINE_NEEDLES, BLOCK_SAXAUL_LEAVES, BLOCK_WILLOW_LEAVES, CHUNK_SIZE_X, CHUNK_SIZE_Y,
    CHUNK_SIZE_Z, CHUNK_VOLUME,
};
use primitive_shared::worldgen::WorldGen;
use std::collections::{HashMap, HashSet};

const SIZE: (u32, u32) = (1280, 720);
/// How far from its foot a tree is lifted: past the widest crown there is
/// (`branches::BRANCH_REACH`, an old tree's `OLD_TREE_REACH`).
const REACH: i32 = 7;

/// Wood, leaf, or something hung in a crown.
fn of_a_tree(b: BlockId) -> bool {
    is_branch(b) || primitive_shared::wood::is_log(b) || block_kind(b) == BLOCK_PALM_TRUNK || is_leafy(b)
}

/// A tree lifted out of the world: its cells in world coordinates, and the
/// height of the ground under its foot.
pub(super) struct Tree {
    pub cells: Vec<((i32, i32, i32), BlockId)>,
    pub foot: (i32, i32, i32),
}

/// The first tree with `leaves` in it near the world's zero, in a spiral of
/// chunks outward, lifted out: its foot is the lowest wood with ground under
/// it that the leaves are joined to.
pub(super) fn find_tree(generator: &WorldGen, leaves: BlockId, pinned: Option<(i32, i32, i32)>) -> Option<Tree> {
    let mut chunks: HashMap<(i32, i32), Chunk> = HashMap::new();
    let at = |chunks: &mut HashMap<(i32, i32), Chunk>, x: i32, y: i32, z: i32| -> BlockId {
        if !(0..CHUNK_SIZE_Y as i32).contains(&y) {
            return BLOCK_AIR;
        }
        let key = (x.div_euclid(CHUNK_SIZE_X as i32), z.div_euclid(CHUNK_SIZE_Z as i32));
        chunks
            .entry(key)
            .or_insert_with(|| generator.generate_chunk(ChunkPos::new(key.0, key.1)))
            .get(x.rem_euclid(CHUNK_SIZE_X as i32) as usize, y as usize, z.rem_euclid(CHUNK_SIZE_Z as i32) as usize)
    };
    // Sampled every 48 blocks out to twenty kilometres, a chunk generated
    // only where the biome there can grow this leaf -- the dry belt and the
    // tropics are thousands of blocks from the world's zero, and generating
    // every chunk on the way there was a run that never ended.
    use primitive_shared::worldgen::Biome;
    let grows_here = |biome: Biome| match leaves {
        BLOCK_SAXAUL_LEAVES => biome == Biome::Desert,
        BLOCK_ACACIA_LEAVES => biome == Biome::Savanna,
        BLOCK_PALM_FRONDS => matches!(biome, Biome::Beach | Biome::Desert | Biome::Savanna),
        BLOCK_FIR_NEEDLES | BLOCK_PINE_NEEDLES => matches!(biome, Biome::Taiga | Biome::Tundra | Biome::Hills),
        BLOCK_WILLOW_LEAVES => matches!(biome, Biome::Swamp | Biome::River | Biome::Forest),
        BLOCK_BIRCH_LEAVES => biome == Biome::BirchForest,
        _ => matches!(biome, Biome::Forest | Biome::Plains | Biome::Steppe),
    };
    let spiral = (0..420i32).flat_map(|ring| {
        (-ring..=ring)
            .flat_map(move |cz| (-ring..=ring).map(move |cx| (cx, cz)))
            .filter(move |&(cx, cz)| cx.abs().max(cz.abs()) == ring)
    });
    let mut asked = 0;
    {
        for (cx, cz) in spiral {
            if !grows_here(generator.biome_at(cx * 48, cz * 48)) {
                continue;
            }
            asked += 1;
            if asked > 3000 {
                return None;
            }
            let (gx, gz) = (cx * 48, cz * 48);
            let pos = ChunkPos::new(gx.div_euclid(16), gz.div_euclid(16));
            let key = (pos.x, pos.z);
            let chunk = chunks.entry(key).or_insert_with(|| generator.generate_chunk(pos));
            let found = chunk.blocks.iter().position(|&b| block_kind(b) == leaves);
            let Some(index) = found else {
                if chunks.len() > 400 {
                    chunks.clear();
                }
                continue;
            };
            let (lx, lz, ly) =
                (index % CHUNK_SIZE_X, (index / CHUNK_SIZE_X) % CHUNK_SIZE_Z, index / (CHUNK_SIZE_X * CHUNK_SIZE_Z));
            let leaf = (pos.x * 16 + lx as i32, ly as i32, pos.z * 16 + lz as i32);
            // The foot: the wood under the leaf's crown standing on ground.
            let mut foot = None;
            'foot: for dz in -REACH + 2..=REACH - 2 {
                for dx in -REACH + 2..=REACH - 2 {
                    for y in (leaf.1 - 16..leaf.1).rev() {
                        let (x, z) = (leaf.0 + dx, leaf.2 + dz);
                        let here = at(&mut chunks, x, y, z);
                        let under = at(&mut chunks, x, y - 1, z);
                        let wood = !is_leafy(here) && of_a_tree(here);
                        if wood && !of_a_tree(under) && primitive_shared::types::is_opaque(under) {
                            foot = Some((x, y, z));
                            break 'foot;
                        }
                    }
                }
            }
            // A foot given is the tree: the first leaf of a kind moves when
            // the generator's leaves move, and a before and an after of two
            // different trees is not a comparison.
            let Some(foot) = pinned.or(foot) else { continue };
            // **Its wood first, then its leaves**, so a neighbour's trunk
            // does not come along through a crown the two share: the wood
            // joined to the foot through faces (the pieces of one tree touch
            // only their parent, `branches::Grower`), then every leaf in the
            // box with this wood within two cells and no other tree's.
            let inside = |n: (i32, i32, i32)| {
                (n.0 - foot.0).abs() <= REACH && (n.2 - foot.2).abs() <= REACH && n.1 >= foot.1 && n.1 < foot.1 + 40
            };
            let is_wood = |b: BlockId| of_a_tree(b) && !is_leafy(b);
            let mut wood = HashSet::from([foot]);
            let mut stack = vec![foot];
            while let Some((x, y, z)) = stack.pop() {
                for (dx, dy, dz) in [(1, 0, 0), (-1, 0, 0), (0, 1, 0), (0, -1, 0), (0, 0, 1), (0, 0, -1)] {
                    let n = (x + dx, y + dy, z + dz);
                    if inside(n) && !wood.contains(&n) && is_wood(at(&mut chunks, n.0, n.1, n.2)) {
                        wood.insert(n);
                        stack.push(n);
                    }
                }
            }
            let mut cells: Vec<((i32, i32, i32), BlockId)> =
                wood.iter().map(|&(x, y, z)| ((x, y, z), at(&mut chunks, x, y, z))).collect();
            let mut has_leaf = false;
            for y in foot.1..foot.1 + 40 {
                for z in foot.2 - REACH..=foot.2 + REACH {
                    for x in foot.0 - REACH..=foot.0 + REACH {
                        let b = at(&mut chunks, x, y, z);
                        if !is_leafy(b) {
                            continue;
                        }
                        let (mut ours, mut theirs) = (false, false);
                        for dy in -2..=2 {
                            for dz in -2..=2 {
                                for dx in -2..=2 {
                                    let n = (x + dx, y + dy, z + dz);
                                    if wood.contains(&n) {
                                        ours = true;
                                    } else if is_wood(at(&mut chunks, n.0, n.1, n.2)) {
                                        theirs = true;
                                    }
                                }
                            }
                        }
                        if ours && !theirs {
                            has_leaf |= block_kind(b) == leaves;
                            cells.push(((x, y, z), b));
                        }
                    }
                }
            }
            if (has_leaf || pinned.is_some()) && cells.len() > 8 {
                return Some(Tree { cells, foot });
            }
        }
    }
    None
}

/// The tree on a lawn of grass at its own foot's height, alone, meshed as
/// the game meshes it.
pub(super) fn mesh_tree(
    generator: &WorldGen,
    tree: &Tree,
    layers: &crate::engine::texture::FaceLayers,
) -> Vec<(ChunkPos, MeshBuffers)> {
    let centre = ChunkPos::new(tree.foot.0.div_euclid(16), tree.foot.2.div_euclid(16));
    let ring: Vec<ChunkPos> =
        (-2..=2).flat_map(|dz| (-2..=2).map(move |dx| ChunkPos::new(centre.x + dx, centre.z + dz))).collect();
    let mut chunks = ChunkManager::new(4);
    let ground = tree.foot.1 - 1;
    for &pos in &ring {
        let mut blocks = vec![BLOCK_AIR; CHUNK_VOLUME];
        for z in 0..CHUNK_SIZE_Z {
            for x in 0..CHUNK_SIZE_X {
                for y in 0..ground as usize {
                    blocks[Chunk::index(x, y, z)] = BLOCK_DIRT;
                }
                blocks[Chunk::index(x, ground as usize, z)] = BLOCK_GRASS;
            }
        }
        for &((x, y, z), b) in &tree.cells {
            if x.div_euclid(16) == pos.x && z.div_euclid(16) == pos.z {
                blocks[Chunk::index(x.rem_euclid(16) as usize, y as usize, z.rem_euclid(16) as usize)] = b;
            }
        }
        chunks.insert(Chunk { pos, blocks });
    }
    let mut light = LightMap::new();
    for &pos in &ring {
        light.load_chunk(&chunks, pos);
    }
    let mut cache = Neighbourhood::default();
    ring.iter()
        .filter(|pos| (pos.x - centre.x).abs() <= 1 && (pos.z - centre.z).abs() <= 1)
        .map(|&pos| {
            let mut buffers = MeshBuffers::default();
            cache.fill(pos, &chunks, &light);
            build_mesh(pos, &cache, layers, generator, &mut buffers);
            (pos, buffers)
        })
        .collect()
}

/// The ten kinds, by the leaf that says which tree it is.
pub(super) const KINDS: [(&str, BlockId); 10] = [
    ("oak", BLOCK_LEAVES),
    ("birch", BLOCK_BIRCH_LEAVES),
    ("fir", BLOCK_FIR_NEEDLES),
    ("pine", BLOCK_PINE_NEEDLES),
    ("willow", BLOCK_WILLOW_LEAVES),
    ("saxaul", BLOCK_SAXAUL_LEAVES),
    ("palm", BLOCK_PALM_FRONDS),
    ("apple", BLOCK_APPLE_LEAVES),
    ("maple", BLOCK_MAPLE_LEAVES),
    ("acacia", BLOCK_ACACIA_LEAVES),
];

#[test]
#[ignore = "a tool: needs a GPU; photographs every kind of tree alone"]
fn what_every_tree_looks_like() {
    let Some((device, queue)) = crate::engine::test_gpu() else {
        println!("no GPU adapter on this machine; skipping");
        return;
    };
    let out = std::env::var("GPU_REPRO_DIR").unwrap_or_else(|_| ".".to_string());
    std::fs::create_dir_all(&out).expect("output directory");
    let only = std::env::var("TREE_KINDS").ok();
    let assets = std::path::Path::new(concat!(env!("CARGO_MANIFEST_DIR"), "/../assets"));
    let settings = {
        let mut settings = ClientSettings { anisotropy: 16, fov_degrees: 95.0, ..ClientSettings::default() };
        settings.sanitize();
        settings
    };
    let textures = TextureManager::load(device, queue, assets, settings.anisotropy).expect("textures load");
    let sky = Sky::new(0.32, 900.0);
    for (name, leaves) in KINDS {
        if only.as_ref().is_some_and(|only| !only.split(',').any(|k| k == name)) {
            continue;
        }
        // The palm and the acacia grow in the tropics and the saxaul in the
        // dry belt, which a world laid in the temperate zone does not reach
        // in any distance a search can walk: a world laid there instead.
        use primitive_shared::worldgen::{Preset, Zone};
        let generator = match name {
            "palm" | "acacia" => WorldGen::with_zone(1337, Preset::Normal, Zone::Tropics),
            "saxaul" => WorldGen::with_zone(1337, Preset::Normal, Zone::DryBelt),
            _ => WorldGen::new(1337),
        };
        let pinned = std::env::var(format!("TREE_AT_{name}")).ok().map(|v| {
            let v: Vec<i32> = v.split(',').map(|n| n.trim().parse().expect("x,y,z")).collect();
            (v[0], v[1], v[2])
        });
        let Some(tree) = find_tree(&generator, leaves, pinned) else {
            println!("{name}: none found");
            continue;
        };
        let top = tree.cells.iter().map(|&((_, y, _), _)| y).max().unwrap_or(tree.foot.1);
        let height = (top - tree.foot.1 + 1) as f32;
        println!("{name}: foot {:?}, {} cells, {height} tall", tree.foot, tree.cells.len());
        let meshes = mesh_tree(&generator, &tree, &textures.face_layers());
        let foot = Vec3::new(tree.foot.0 as f32 + 0.5, tree.foot.1 as f32, tree.foot.2 as f32 + 0.5);
        let crown = foot + Vec3::Y * (height * 0.7);
        let distance = height.max(6.0) * 0.6 + 3.0;
        let mut views: Vec<(String, Vec3, Vec3)> = (0..4)
            .map(|i| {
                let a = i as f32 * std::f32::consts::FRAC_PI_2 + 0.4;
                let eye = crown + Vec3::new(a.cos() * distance, -height * 0.15, a.sin() * distance);
                (format!("{name}_{}", ["e", "s", "w", "n"][i]), eye, crown)
            })
            .collect();
        // Up into the crown from a player's eyes beside the trunk, where
        // a leaf off its limb shows against the sky.
        views.push((format!("{name}_under"), foot + Vec3::new(2.3, 1.62, 1.7), crown + Vec3::Y));
        for (file, eye, at) in views {
            let mut camera = Camera::new(eye.as_dvec3(), SIZE.0 as f32 / SIZE.1 as f32);
            let dir = (at - eye).normalize();
            camera.yaw = dir.z.atan2(dir.x);
            camera.pitch = dir.y.asin();
            camera.fov_y_radians = settings.fov_degrees.to_radians();
            let picture = super::offscreen_repro::draw_scene(
                device,
                queue,
                &textures,
                &settings,
                &camera,
                &sky,
                &meshes,
                SIZE,
                include_str!("shader.wgsl"),
                None,
                None,
                false,
                4,
            );
            let path = format!("{out}/{file}.png");
            picture.save(&path).expect("write picture");
        }
    }
    println!("pictures in {out}");
}
