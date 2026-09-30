//! A meadow drawn as a meadow: the low blades that cover open turf between
//! the tufts, as thick as the player's graphics step allows.
//!
//! ## What was wrong
//!
//! A tuft of grass is a block (`types::BLOCK_TALL_GRASS`): the server
//! generates it, a player pulls it up for fibre, and a plain holds one every
//! few columns because that is how many a meadow's worth of fibre should
//! be. Drawn alone, that is what the owner saw -- "трава стоит отдельными
//! пучками": a green floor with clumps standing on it, where a real meadow
//! is a pile of blades you cannot see the ground through.
//!
//! ## Three ways to thicken it
//!
//! * **More tufts in the world.** The generator could lay a tuft on every
//!   turf. It would also make every meadow ten times the fibre, every
//!   pasture a solid wall of blocks for the animals' paths and the mining
//!   ray to thread, and a server stream and save that much fatter -- for a
//!   picture. A picture is the client's business.
//! * **A texture on the turf's top.** Free, and flat: from standing height a
//!   painted blade is a stripe on the floor, and it does not stand up
//!   against the sky at the brow of a hill, which is most of what makes grass
//!   read as grass.
//! * **Blades the mesher adds and nobody else knows about (chosen).** Short
//!   crossed quads on open turf, a few to a cell at a jittered spot and a
//!   random turn, drawn with the tufts in the cut-out pass. The world is
//!   unchanged: nothing here can be mined, collided with, aimed at or sent.
//!   What it costs is triangles and fill, so it is a graphics step
//!   (`Density`) with a ring round the player it is drawn in.
//!
//! ## Where it grows thick
//!
//! Turf only, and **thick where the sky reaches it**. The share of a cell's
//! blades is read off the sky light in the cell above the turf: a meadow
//! under open sky is fifteen and gets the whole share, the eaves of a wood a
//! little less, and the floor under a closed canopy -- eleven or less --
//! a sprinkling, which is the "реже в лесу под пологом" the owner asked for
//! and needs no knowledge of woods at all. The savanna's dry turf grows
//! straw-coloured blades; bare sand, gravel and stone grow none, and the dry
//! country's sandy soil a few dry ones.
//!
//! ## Where it stops
//!
//! The mesher lays blades in chunks within `Density::mesh_line`; **the
//! vertex shader sinks them into the ground** toward `Density::fade_line`,
//! measured from the eye, so the edge of the lawn is grass getting shorter
//! rather than a ring where it ends. Every chunk the fade can reach is
//! inside the mesh line whichever corner of its chunk the player stands in
//! (`the_blades_are_meshed_everywhere_they_can_still_be_seen`).

use primitive_shared::types::{self, BlockId};

use crate::engine::mesh::{cell_hash, pack_light, Vertex};
use crate::engine::sway::{reach_bits, COVER_BIT};

/// How thick the turf's own blades are drawn. A graphics step: see
/// `settings::Graphics`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Density {
    /// The tufts alone, as the game drew grass before.
    Off,
    Sparse,
    #[default]
    Thick,
    Lush,
}

impl Density {
    pub const ALL: [Density; 4] = [Density::Off, Density::Sparse, Density::Thick, Density::Lush];

    /// One step along, held at both ends.
    pub fn step(self, delta: i32) -> Density {
        let at = Self::ALL.iter().position(|d| *d == self).unwrap_or(2) as i32;
        Self::ALL[(at + delta).clamp(0, Self::ALL.len() as i32 - 1) as usize]
    }

    /// How many blade clumps an open cell of turf holds, on average.
    ///
    /// **Measured, not guessed** (`CHANGELOG.md`, "Луг стал лугом"): on a
    /// GTX 1050 Ti at 1920x1080, world `night`, the cut-out pass is 1.25 ms
    /// with the blades off, 1.44 sparse, 1.69 thick and 1.96 lush -- the
    /// cost grows faster than the triangles, because what is paid for is
    /// the alpha-cut pixels near the eye. `Sparse` is a lawn with the ground
    /// showing through it, `Thick` a hayfield, `Lush` a meadow nobody has
    /// grazed.
    pub fn clumps(self) -> f32 {
        match self {
            Density::Off => 0.0,
            Density::Sparse => 1.0,
            Density::Thick => 2.2,
            Density::Lush => 3.5,
        }
    }

    /// How far from the eye, in blocks, the blades have sunk into the
    /// ground. Nought is none drawn.
    pub fn fade_line(self) -> f32 {
        match self {
            Density::Off => 0.0,
            Density::Sparse => 24.0,
            Density::Thick => 36.0,
            Density::Lush => 52.0,
        }
    }

    /// How far out, in chunks from the player's, a chunk is meshed with its
    /// blades. See `meshes_blades_at`.
    pub fn mesh_line(self) -> f32 {
        if self == Density::Off {
            return 0.0;
        }
        // A point at the fade line can be in a chunk this far from the
        // player's: the line, plus the half diagonal of the chunk the player
        // stands in and of the chunk the point is in.
        self.fade_line() / 16.0 + std::f32::consts::SQRT_2 + HYSTERESIS
    }
}

/// How far past a line a chunk has to be before it changes, in chunks: the
/// `lod` bands' idea, at half their width -- a chunk of blades is cheap to
/// build, and a wide margin is chunks of blades sunk out of sight.
const HYSTERESIS: f32 = 0.5;

/// Whether a chunk `distance_chunks` from the player's is meshed with the
/// turf's blades, given whether it was last time.
pub fn meshes_blades_at(distance_chunks: f32, density: Density, current: bool) -> bool {
    let line = density.mesh_line();
    if line <= 0.0 {
        return false;
    }
    if current {
        distance_chunks < line + HYSTERESIS
    } else {
        distance_chunks < line - HYSTERESIS
    }
}

/// Which blades a ground grows, and what share of the step's clumps.
///
/// **Turf is the meadow, dry turf the steppe's and savanna's straw, and the
/// dry country's sandy soil a few straw blades.** Everything else -- sand,
/// gravel, stone, snow, bare earth, peat -- grows none: bare earth is where
/// something was dug or trodden, and a lawn on it would say otherwise.
fn grows(ground: BlockId) -> Option<(bool, f32)> {
    use primitive_shared::ground::as_common;
    match as_common(ground) {
        types::BLOCK_GRASS => Some((true, 1.0)),
        types::BLOCK_DRY_TURF => Some((false, 0.8)),
        types::BLOCK_SANDY_SOIL => Some((false, 0.2)),
        _ => None,
    }
}

/// Whether this block grows blades at all -- the mesher's one question of
/// every block before it asks anything dearer.
#[inline]
pub(crate) fn may_grow_on(ground: BlockId) -> bool {
    grows(ground).is_some()
}

/// How much of a cell's share the light over it lets grow: the whole under
/// open sky, less at a wood's edge, a sprinkling under a closed crown.
fn open(sky: u8) -> f32 {
    match sky {
        15.. => 1.0,
        14 => 0.8,
        13 => 0.55,
        12 => 0.3,
        _ => 0.12,
    }
}

/// One cell of turf the mesher has found with room over it.
pub(crate) struct Turf {
    /// The turf's cell, in the world: the blades' spots and turns are hashed
    /// from it, so a remesh draws the same meadow.
    pub cell: [i32; 3],
    /// Where the turf's top is, in the chunk's own space: x and z its cell's
    /// corner, y the real top (a lip is lower than its cell).
    pub top: [f32; 3],
    pub ground: BlockId,
    /// The light in the cell over it, packed as the mesher packs it.
    pub light: u8,
    /// The climate's green for this column (`mesh::pack_tint`).
    pub tint: u32,
}

/// The two pictures, green and straw.
#[derive(Clone, Copy, Debug)]
pub(crate) struct Blades {
    pub green: u32,
    pub dry: u32,
}

/// How tall a clump stands, in blocks, and how wide it is.
///
/// **Knee-high, and it was ankle-high.** The first clumps stood a quarter to
/// half a block and were as wide as a boot, and photographed from a
/// player's eye they were the turf's own picture with a few darker marks on
/// it -- the owner's "отдельные пучки" all over again, only smaller. A
/// meadow is grass as tall as the player's shin that hides the ground a few
/// steps off, which is half a block and more.
const HEIGHT: (f32, f32) = (0.35, 0.72);
const WIDTH: (f32, f32) = (0.55, 0.85);
/// The most clumps one cell is given, whatever the step.
const MOST: u32 = 4;

/// Lays this cell's blades, if it grows any. Two crossed quads a clump, into
/// the sprite range (`MeshBuffers`), rooted on the turf.
pub(crate) fn grow(turf: &Turf, density: Density, pictures: Blades, vertices: &mut Vec<Vertex>, sprites: &mut Vec<u32>) {
    let Some((green, share)) = grows(turf.ground) else {
        return;
    };
    let sky = turf.light & 0x0F;
    let wanted = density.clumps() * share * open(sky);
    if wanted <= 0.0 {
        return;
    }
    let hash = cell_hash(turf.cell[0], turf.cell[1] ^ 0x3E4D, turf.cell[2]);
    // The whole part always, the fraction by a roll: 1.8 is one clump
    // everywhere and a second in four cells of five.
    let roll = ((hash >> 24) & 0xFF) as f32 / 256.0;
    let count = ((wanted.floor() as u32) + u32::from(roll < wanted.fract())).min(MOST);
    let (layer, tint) = if green { (pictures.green, turf.tint) } else { (pictures.dry, 0) };
    let packed = pack_light(sky, (turf.light >> 4) & 0x0F, 3, 0);
    for clump in 0..count {
        let h = cell_hash(turf.cell[0].wrapping_add(clump as i32 * 7919), turf.cell[1], turf.cell[2] ^ 0x51ED);
        let unit = |shift: u32| ((h >> shift) & 0xFF) as f32 / 255.0;
        let height = HEIGHT.0 + (HEIGHT.1 - HEIGHT.0) * unit(0);
        let width = WIDTH.0 + (WIDTH.1 - WIDTH.0) * unit(8);
        let half = width * 0.5;
        // **Inside the cell, whatever the turn**: a clump turned any way
        // reaches no further than its half width from its middle, so the
        // middle is kept that far in. A blade through the side of the next
        // block is the fault `cross_planes` insets its tufts against.
        let middle = [half + (1.0 - width) * unit(16), half + (1.0 - width) * unit(24)];
        let turn = unit(4) * std::f32::consts::FRAC_PI_2;
        let (sin, cos) = turn.sin_cos();
        let top = reach_bits(height) | COVER_BIT;
        for (dx, dz) in [(cos, sin), (-sin, cos)] {
            let base = vertices.len() as u32;
            let a = [turf.top[0] + middle[0] - dx * half, turf.top[2] + middle[1] - dz * half];
            let b = [turf.top[0] + middle[0] + dx * half, turf.top[2] + middle[1] + dz * half];
            let y = turf.top[1];
            for (corner, uv, reach) in [
                ([a[0], y, a[1]], [0.0, 1.0], COVER_BIT),
                ([b[0], y, b[1]], [1.0, 1.0], COVER_BIT),
                ([b[0], y + height, b[1]], [1.0, 0.0], top),
                ([a[0], y + height, a[1]], [0.0, 0.0], top),
            ] {
                vertices.push(Vertex::tinted(corner, uv, layer, packed, tint).swaying(reach));
            }
            sprites.extend_from_slice(&[base, base + 1, base + 2, base, base + 2, base + 3]);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn lawn(ground: BlockId, sky: u8, density: Density) -> (Vec<Vertex>, Vec<u32>) {
        let (mut vertices, mut sprites) = (Vec::new(), Vec::new());
        for x in 0..16 {
            for z in 0..16 {
                let turf = Turf { cell: [x, 40, z], top: [x as f32, 41.0, z as f32], ground, light: sky, tint: 7 };
                grow(&turf, density, Blades { green: 5, dry: 6 }, &mut vertices, &mut sprites);
            }
        }
        (vertices, sprites)
    }

    fn clumps(density: Density, ground: BlockId, sky: u8) -> usize {
        // Two quads a clump, six indices a quad.
        lawn(ground, sky, density).1.len() / 12
    }

    #[test]
    fn each_step_is_thicker_than_the_one_before_it() {
        let counts: Vec<usize> = Density::ALL.iter().map(|&d| clumps(d, types::BLOCK_GRASS, 15)).collect();
        assert_eq!(counts[0], 0, "off drew blades");
        for pair in counts.windows(2) {
            assert!(pair[1] > pair[0], "a step drew no more than the one below it: {counts:?}");
        }
        // ...and each is about what it says: a chunk of open turf.
        for (d, n) in Density::ALL.iter().zip(&counts) {
            let expected = d.clumps() * 256.0;
            assert!((*n as f32 - expected).abs() <= expected * 0.15 + 2.0, "{d:?}: {n} clumps against {expected}");
        }
    }

    #[test]
    fn a_meadow_is_thick_the_wood_floor_thin_and_stone_bare() {
        let meadow = clumps(Density::Thick, types::BLOCK_GRASS, 15);
        let wood = clumps(Density::Thick, types::BLOCK_GRASS, 10);
        assert!(wood * 4 < meadow, "under a canopy {wood} against {meadow} in the open");
        for bare in [types::BLOCK_STONE, types::BLOCK_SAND, types::BLOCK_GRAVEL, types::BLOCK_DIRT, types::BLOCK_SNOW] {
            assert_eq!(clumps(Density::Lush, bare, 15), 0, "blades on {}", types::block_name(bare));
        }
        let steppe = clumps(Density::Thick, types::BLOCK_DRY_TURF, 15);
        let desert = clumps(Density::Thick, types::BLOCK_SANDY_SOIL, 15);
        assert!(steppe > desert * 3 && desert > 0, "dry turf {steppe}, sandy soil {desert}");
    }

    #[test]
    fn every_blade_stands_inside_its_own_cell() {
        for x in -20..20 {
            for z in -20..20 {
                let (mut vertices, mut sprites) = (Vec::new(), Vec::new());
                let turf = Turf {
                    cell: [x, 40, z],
                    top: [x as f32, 41.0, z as f32],
                    ground: types::BLOCK_GRASS,
                    light: 15,
                    tint: 7,
                };
                grow(&turf, Density::Lush, Blades { green: 5, dry: 6 }, &mut vertices, &mut sprites);
                for v in &vertices {
                    let (fx, fz) = (v.position[0] - x as f32, v.position[2] - z as f32);
                    assert!(
                        (-1e-4..=1.0 + 1e-4).contains(&fx) && (-1e-4..=1.0 + 1e-4).contains(&fz),
                        "a blade of the cell ({x}, {z}) at {:?}",
                        v.position
                    );
                }
            }
        }
    }

    #[test]
    fn a_blade_is_rooted_where_it_stands_and_only_its_tip_bends() {
        let (vertices, _) = lawn(types::BLOCK_GRASS, 15, Density::Thick);
        for v in &vertices {
            let reach = (v.uv >> crate::engine::sway::SWAY_SHIFT) & crate::engine::sway::SWAY_STEPS;
            assert!(v.uv & COVER_BIT != 0, "a blade that the fade would not sink");
            if (v.position[1] - 41.0).abs() < 1e-4 {
                assert_eq!(reach, 0, "a root that the wind moves");
            } else {
                assert!(reach > 0, "a tip that the wind does not move");
            }
        }
    }

    #[test]
    fn the_same_turf_grows_the_same_blades_every_time_it_is_meshed() {
        assert_eq!(
            lawn(types::BLOCK_GRASS, 15, Density::Thick).0.iter().map(|v| v.position).collect::<Vec<_>>(),
            lawn(types::BLOCK_GRASS, 15, Density::Thick).0.iter().map(|v| v.position).collect::<Vec<_>>()
        );
    }

    #[test]
    fn the_blades_are_meshed_everywhere_they_can_still_be_seen() {
        // The eye anywhere in its chunk, a point on the fade line in any
        // direction: the chunk that point is in must be meshed with blades,
        // coming in fresh (the stricter side of the margin).
        for density in [Density::Sparse, Density::Thick, Density::Lush] {
            let line = density.fade_line();
            for eye in [(0.1, 0.1), (15.9, 15.9), (8.0, 8.0), (0.1, 15.9)] {
                for step in 0..64 {
                    let angle = step as f32 / 64.0 * std::f32::consts::TAU;
                    let point = (eye.0 + angle.cos() * line, eye.1 + angle.sin() * line);
                    let chunk = ((point.0 / 16.0).floor(), (point.1 / 16.0).floor());
                    let distance = (chunk.0 * chunk.0 + chunk.1 * chunk.1).sqrt();
                    assert!(
                        meshes_blades_at(distance, density, false),
                        "{density:?}: a point on the fade line in a chunk {distance} away is not meshed"
                    );
                }
            }
        }
        assert!(!meshes_blades_at(0.0, Density::Off, true));
    }
}
