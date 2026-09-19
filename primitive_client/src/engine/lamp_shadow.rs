//! Shadows from fire: a hearth, a kiln, a seam of glowstone -- anything that
//! pours light into the light map -- casting the shadows of the blocks round
//! it, hard-edged, the way a lamp in a room of pillars does.
//!
//! **What the light map cannot do.** Block light is a flood fill: a level
//! steps down by one for every cell it walks, and it walks *round* things. A
//! pillar between a fire and the floor takes one level off the floor behind
//! it, which is nothing, so a camp at night was lit in an even disc and the
//! pillar, the wall and the trunk threw no shadow at all. The report was a
//! picture of a lamp in a dark room with a hard spoke of shadow running
//! outward from every edge in it, and "думаю это будет даже менее дорого".
//!
//! ## How
//!
//! The opaque cells round the player go up once into a small 3D texture, a
//! byte a cell ([`SIDE`] a side), and the fires are found in the same walk
//! ([`Volume::gather`]). Every frame the nearest [`MAX_LAMPS`] of them are
//! handed to the shadowed terrain shader, and a fragment with any block light
//! walks the grid from itself to each fire in reach (`lamp_ray_clear` in
//! `shader.wgsl`, a line-for-line copy of [`Volume::ray_is_clear`]). A fire it
//! cannot see gives it `LAMP_BLOCKED_SHARE` of what the flood fill gave; one it
//! can see gives all of it. The edge of the shadow is the edge of a block, in
//! every direction and at every distance, with no picture to have a
//! resolution -- which is the look the report asked for and the reason this
//! is not the sun's shadow map pointed at a fire.
//!
//! **Soft** ([`Mode::Soft`](crate::engine::shadow::Mode)) walks four rays to
//! four corners of a small cube inside the fire's cell and averages them: a
//! penumbra the width of the flame, for four times the walking.
//!
//! **The volume is not taken every frame.** It is a picture of the blocks,
//! and it is taken again only when the eye has walked [`RECENTRE_AFTER`] from
//! its middle or a chunk it covers has a new mesh ([`needs_new`]).
//!
//! ## Rejected
//!
//! * **A cube shadow map per fire.** Six depth pictures a fire, every frame
//!   anything near it moves -- eight campfires in a village is forty-eight
//!   passes over the terrain, on a renderer whose own measurements say the
//!   terrain pass is vertex-bound (`renderer::solid_ranges_facing`).
//! * **Lines of sight baked into the flood fill.** Free to draw, but a light
//!   map that knows what can see what has to be re-lit in a sphere every time
//!   a block changes near a fire, through the streaming budget -- and the
//!   light map is shared with the server, so a look would change what is
//!   *true* about light.
//! * **The same walk toward the sun** -- rejected in `engine::shadow`, and
//!   for the reason this is affordable: a fire's ray is at most fourteen
//!   cells long, and a sun's has to cross the loaded world up to the sky.
//!
//! ## The sun, at the Hard step
//!
//! "ты сделал тени как я просил но от солнца нету": the fires' shadows were
//! the look asked for, and the sun's -- a depth picture -- were not. At the
//! Hard step the sun walks the same volume: from the fragment toward the sun,
//! cell by cell, until it enters a cell that casts ([`STOPS_SUN`] -- leaves and
//! thick wood as well as whole blocks, because a canopy that let the sun
//! through would be no canopy), or rises above the highest thing in the
//! volume, which is open sky. A ray that leaves through a side of the volume
//! first, or crosses more than `MAX_SUN_COLUMNS`, has not found out, and the sun's
//! map answers for it (`sun_visibility` in `shader.wgsl`) -- so a low sun
//! still has its long shadows from a hill two hundred blocks off, and within
//! twenty blocks of the player every shadow is the edge of a block.
//!
//! ## What it does not do
//!
//! Only whole opaque blocks cast a fire's light (`types::is_opaque`, the flood
//! fill's own test). A table, a fence or a leaf casts nothing from a fire: a
//! byte a cell cannot hold the shape of a table, and a leaf that cast a
//! cell-sized solid shadow would be a worse canopy than none. The sun's walk
//! is not left as blind: a cell with a model in it, or a block short of a
//! whole one (a slab, a lip), hands the ray to the map, which has its shape
//! ([`ASKS_THE_MAP`]). Animals, players and dropped things are lit as
//! they were -- the walk is in the terrain's two shadowed pipelines only. The
//! torch in the player's own hand casts nothing: its shadows would fall
//! exactly behind what the eye sees, where the eye cannot see them.

use bytemuck::Zeroable;
use glam::Vec3;
use primitive_shared::types::{branch_width, is_leafy, is_opaque, light_emission, BlockId, BLOCK_AIR};

use crate::engine::shadow::PlantShadows;

/// A cell that stops every light: a whole opaque block. What a fire's ray
/// stops at, and the sun's.
pub const STOPS_ALL: u8 = 255;

/// A cell that stops the sun and not a fire: leaves, and wood as thick as a
/// bough. **Must stay under the shader's `LAMP_STOPS` cut and over its
/// `SUN_STOPS` one** (0.75 and 0.25 of 255).
///
/// Two answers because the two lights ask different questions. A crown is what
/// shade under a tree *is*, so the sun must not pass it; a fire under the same
/// crown lights the camp round it, and leaves that cast whole cells of black
/// from a hearth would put the camp in the dark. Wood thinner than a bough --
/// a twig two sixteenths across -- casts nothing: a cell of shadow from it
/// would be sixteen times the twig.
pub const STOPS_SUN: u8 = 128;

/// A cell with a model in it -- a workbench, a chest, a drying rack, a door --
/// whose shape a byte cannot hold: the walk hands a ray that enters one to the
/// sun's map, which drew the model's own boxes. **Must stay under the shader's
/// `SUN_STOPS` cut and over its `MAP_STOPS` one** (0.25 and 0.1 of 255).
///
/// **The bug it is for**: "верстак, колода каменотёса, гончарный круг и
/// скорняжный стол не отбрасывают тень". At the Hard step a ray the walk saw
/// rise over everything that casts is called open sky and the map is never
/// asked -- and a model's cell cast nothing, so inside the volume, the thirty
/// two blocks round the player where shadows are looked at, no piece of
/// furniture and no door had a shadow at all. At the Soft step the map answers
/// everywhere and they always did; `what_models_and_plants_cast` photographs
/// both.
///
/// Weighed:
///
/// * **Casting the whole cell**, as a leaf does (`STOPS_SUN`). A stool would
///   throw the shadow of a cube, a rack a black square with no gap in it --
///   the module note's "a byte a cell cannot hold the shape of a table", and
///   it is right.
/// * **Asking the map as well wherever the walk said open sky.** The map's
///   edge is a filtered texel and the walk's is the block's own; every cube's
///   shadow would grow the map's soft rim back round its sharp one, which is
///   the thing the Hard step exists not to have ("край размыт").
/// * **The map, but only for what passed a model (chosen).** Everything else
///   keeps the walk's answer to the bit, and the only pixels that change are
///   the ones whose sun a model is in the way of -- or might be, which the
///   map, holding the model's real boxes, then decides.
pub const ASKS_THE_MAP: u8 = 48;

/// The most columns a ray toward the sun crosses. **Must match `SUN_COLUMNS`
/// in `shader.wgsl`.** At noon a ray is above the highest thing in the volume
/// within a column or two; a low sun's ray runs along the ground, and past this
/// many columns the sun's map answers instead.
#[cfg(test)]
pub const MAX_SUN_COLUMNS: usize = 32;

/// What a cell does to the light: see [`STOPS_ALL`], [`STOPS_SUN`] and
/// [`ASKS_THE_MAP`] -- the plants the way the player asked
/// (`shadow::PlantShadows`), so the walk and the map agree about them: a crown
/// turned off is air to both, and a tuft turned on is handed to the map, which
/// has its blades.
fn stopping(block: BlockId, plants: PlantShadows) -> u8 {
    use primitive_shared::types::{is_cross, is_flat};
    if is_opaque(block) {
        STOPS_ALL
    } else if branch_width(block).is_some_and(|width| width >= 8) {
        STOPS_SUN
    } else if is_leafy(block) {
        if plants.crowns_cast() {
            STOPS_SUN
        } else {
            0
        }
    } else if crate::engine::mesh::drawn_as_model(block) && (plants.sprites_cast() || !(is_cross(block) || is_flat(block))) {
        // A sprite is left out unless the player asked for plants' shadows:
        // it is not in the map's picture otherwise, and asking the map about
        // it would be a lookup for nothing on every blade of a meadow.
        ASKS_THE_MAP
    } else if is_part_of_a_block(block) {
        ASKS_THE_MAP
    } else {
        0
    }
}

/// A block drawn in the solid pass that does not fill its cell: a slab, a
/// turf lip, a floor dug down, a wall part way up, a step, a hearth half a
/// cell high. **Not opaque** (`types::is_opaque` says why each of them
/// must not be), so the walk took every one of them for air.
///
/// **The bug it is for**: at the Hard step a roof of slabs threw no shadow at
/// all within the thirty-two blocks round the player -- the posts under it
/// did, the roof did not -- and the Soft step, which asks the map everywhere,
/// drew the same roof's shadow on the same ground
/// (`what_layers_plants_and_caves_cast`, `slab_roof`). The lips the generator
/// lays along every slope and a bitten wall were the same air to the walk. The map has all of them: they are solid
/// geometry, drawn by the solid caster. So the walk hands a ray that meets
/// one to the map, as it does a model's cell -- rather than stopping it
/// (`STOPS_SUN`), which would cast a whole cell for a slab half a cell deep,
/// the thing `ASKS_THE_MAP` was written to avoid.
///
/// Liquids are not here: water lets the light through, and the map does not
/// draw it.
fn is_part_of_a_block(block: BlockId) -> bool {
    use primitive_shared::types::{collision_depth, is_cross, is_flat, is_liquid, is_partial, is_step};
    !is_liquid(block)
        && !is_cross(block)
        && !is_flat(block)
        && (is_partial(block) || primitive_shared::dig::is_part(block) || is_step(block) || collision_depth(block).is_some())
}

/// Where a ray toward the sun ended. See the module note.
#[cfg(test)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SunRay {
    /// It entered a cell that casts.
    Blocked,
    /// It rose above everything in the volume that casts.
    OpenSky,
    /// It left through a side, or ran out of steps: the map decides.
    Unknown,
}

/// Cells along each side of the volume. **Must match `LAMP_SIDE` in
/// `shader.wgsl`** (`the_shader_walks_the_grid_this_file_fills`).
///
/// Sixty-four is a quarter of a megabyte a picture, filled in a few
/// milliseconds once every dozen blocks walked. The brightest fire reaches
/// fourteen cells, so a volume re-centred [`RECENTRE_AFTER`] from its middle
/// still holds every fire that lights anything within twenty blocks of the
/// eye, together with every block between it and what it lights.
pub const SIDE: usize = 64;

/// How far the eye walks from the middle of the volume, along any axis,
/// before it is taken again.
pub const RECENTRE_AFTER: i32 = 12;

/// Fires handed to the shader in a frame, nearest the eye first. **Must
/// match the array in `Lamps` in `shader.wgsl`.**
///
/// Sixteen, because the shader's loop over them is paid on every lit
/// fragment. A fire past the sixteenth still lights the world through the
/// flood fill; it only stops casting, and it is the furthest one.
pub const MAX_LAMPS: usize = 16;

/// The most cells one ray steps through. **Must match `LAMP_STEPS` in
/// `shader.wgsl`.** Fourteen cells of reach walked in three axes is at most
/// forty-two crossings -- and only on the exact diagonal, where the reach
/// runs out long before.
///
/// Built for tests only, with the walk it bounds: the game's walk is the
/// shader's, and this file's copy of it exists to be tested.
#[cfg(test)]
pub const MAX_STEPS: usize = 32;

/// One fire: the cell it is in and the level it gives out.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Lamp {
    pub cell: [i32; 3],
    pub level: u8,
}

/// The blocks round the player, as the shader's walk sees them.
#[derive(Debug, Clone, PartialEq)]
pub struct Volume {
    /// The world cell at the volume's lowest corner.
    pub min: [i32; 3],
    /// One byte a cell, x fastest, then y, then z -- the order
    /// `Queue::write_texture` reads a 3D texture in: [`STOPS_ALL`],
    /// [`STOPS_SUN`] or 0.
    pub cells: Vec<u8>,
    /// Every fire inside it.
    pub lamps: Vec<Lamp>,
    /// The highest row, counted from the corner, with anything in it that
    /// casts; -1 for an empty volume. A ray toward the sun above it is in the
    /// open, and stops walking there.
    pub top: i32,
    /// One byte a column, x fastest, then z: the row over the highest cell in
    /// it that casts, counted from the corner; nought for an empty column.
    /// What lets the sun's walk pass a column it is above without looking at
    /// a cell of it (`sun_ray`).
    pub heights: Vec<u8>,
    /// The same, the other way round: one over the *lowest* cell in the
    /// column that casts, nought for an empty column.
    ///
    /// **What keeps the walk out of the air under a crown.** The walk goes
    /// up a column from the fragment to that column's height, a cell at a
    /// time; under an oak that is twenty cells of empty air looked up one
    /// by one before the first leaf is reached, on every sunlit pixel of
    /// the ground in a wood. With the floor of the column as well, the
    /// walk starts at the lowest thing there is in it and the air costs
    /// nothing. It answers exactly what it answered before -- the cells
    /// skipped are cells the gather saw were empty.
    pub lows: Vec<u8>,
}

/// `Lamps` in `shader.wgsl`, byte for byte.
#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq, bytemuck::Pod, bytemuck::Zeroable)]
pub struct LampUniform {
    /// xyz: the volume's lowest corner, relative to the render origin;
    /// w: how many of `lamps` are filled.
    pub volume: [f32; 4],
    /// x: 1 when a volume has been written at all -- nought, and a ray toward
    /// the sun has nothing to walk and asks the map; y: the row over
    /// [`Volume::top`], counted from the corner.
    pub grid: [f32; 4],
    /// xyz: the middle of the fire's cell, relative to the render origin;
    /// w: its level.
    pub lamps: [[f32; 4]; MAX_LAMPS],
}

/// The volume's corner for an eye at `eye`: the eye in its middle cell.
pub fn corner_for(eye: glam::DVec3) -> [i32; 3] {
    let half = (SIDE / 2) as i32;
    [eye.x.floor() as i32 - half, eye.y.floor() as i32 - half, eye.z.floor() as i32 - half]
}

/// Where the volume should be taken, if it has to be taken at all: when there
/// is none, when something it covers changed (`stale`), or when the eye has
/// walked [`RECENTRE_AFTER`] from its middle along any axis.
pub fn needs_new(current: Option<[i32; 3]>, stale: bool, eye: glam::DVec3) -> Option<[i32; 3]> {
    let wanted = corner_for(eye);
    match current {
        Some(min) if !stale && (0..3).all(|axis| (wanted[axis] - min[axis]).abs() <= RECENTRE_AFTER) => None,
        _ => Some(wanted),
    }
}

impl Volume {
    /// Reads the blocks of the volume whose lowest corner is `min`.
    ///
    /// `column(x, z, y0, out)` fills `out` with the blocks of the column at
    /// world `(x, z)` from height `y0` up -- air where nothing is loaded. A
    /// column at a time because that is how the chunk manager answers
    /// cheaply: finding the chunk once and reading [`SIDE`] cells of it,
    /// rather than hashing a chunk position for each of a quarter of a
    /// million cells (see `ChunkManager::column`).
    ///
    /// With the plants at their default: the tests and the tools, which have
    /// no settings. The game asks [`gather_for`](Self::gather_for).
    #[cfg(test)]
    pub fn gather(min: [i32; 3], column: impl FnMut(i32, i32, i32, &mut [BlockId])) -> Self {
        Self::gather_for(min, PlantShadows::default(), column)
    }

    /// [`gather`](Self::gather), with the plants cast the way `plants` says.
    pub fn gather_for(min: [i32; 3], plants: PlantShadows, mut column: impl FnMut(i32, i32, i32, &mut [BlockId])) -> Self {
        let mut cells = vec![0u8; SIDE * SIDE * SIDE];
        let mut lamps = Vec::new();
        let mut top = -1i32;
        let mut heights = vec![0u8; SIDE * SIDE];
        let mut lows = vec![0u8; SIDE * SIDE];
        let mut blocks = vec![BLOCK_AIR; SIDE];
        for z in 0..SIDE {
            for x in 0..SIDE {
                let (gx, gz) = (min[0] + x as i32, min[2] + z as i32);
                column(gx, gz, min[1], &mut blocks);
                for (y, &block) in blocks.iter().enumerate() {
                    let stops = stopping(block, plants);
                    if stops != 0 {
                        cells[x + SIDE * (y + SIDE * z)] = stops;
                        top = top.max(y as i32);
                        heights[x + SIDE * z] = (y + 1) as u8;
                        if lows[x + SIDE * z] == 0 {
                            lows[x + SIDE * z] = (y + 1) as u8;
                        }
                    }
                    let level = light_emission(block);
                    if level > 0 {
                        lamps.push(Lamp { cell: [gx, min[1] + y as i32, gz], level });
                    }
                }
            }
        }
        Self { min, cells, lamps, top, heights, lows }
    }

    /// The uniform for a frame: the [`MAX_LAMPS`] fires nearest `eye`, and
    /// where they and the volume are relative to `render_origin`.
    ///
    /// The render origin is a whole number of blocks, so every coordinate
    /// here is a small whole number (and a half) exactly, however far out the
    /// world is -- the subtraction is done in `f64` before anything is
    /// narrowed.
    pub fn uniform(&self, eye: Vec3, render_origin: Vec3) -> LampUniform {
        let origin = render_origin.as_dvec3();
        let relative = |cell: [i32; 3], half: f64| {
            [
                (f64::from(cell[0]) + half - origin.x) as f32,
                (f64::from(cell[1]) + half - origin.y) as f32,
                (f64::from(cell[2]) + half - origin.z) as f32,
            ]
        };
        let mut nearest: Vec<&Lamp> = self.lamps.iter().collect();
        let apart = |lamp: &Lamp| {
            let centre = Vec3::new(lamp.cell[0] as f32, lamp.cell[1] as f32, lamp.cell[2] as f32) + 0.5;
            centre.distance_squared(eye)
        };
        nearest.sort_by(|a, b| apart(a).total_cmp(&apart(b)));
        nearest.truncate(MAX_LAMPS);
        let mut uniform = LampUniform::zeroed();
        let corner = relative(self.min, 0.0);
        uniform.volume = [corner[0], corner[1], corner[2], nearest.len() as f32];
        uniform.grid = [1.0, (self.top + 1) as f32, 0.0, 0.0];
        for (slot, lamp) in uniform.lamps.iter_mut().zip(nearest) {
            let centre = relative(lamp.cell, 0.5);
            *slot = [centre[0], centre[1], centre[2], f32::from(lamp.level)];
        }
        uniform
    }

    /// Whether the cell at `cell`, counted from the volume's corner, stops
    /// the light. Outside the volume nothing does.
    #[cfg(test)]
    fn stops(&self, cell: [i32; 3]) -> bool {
        let side = SIDE as i32;
        if cell.iter().any(|&c| c < 0 || c >= side) {
            return false;
        }
        let [x, y, z] = cell.map(|c| c as usize);
        self.cells[x + SIDE * (y + SIDE * z)] == STOPS_ALL
    }

    /// Where a ray from `from` (counted from the corner) toward the sun ends.
    /// **`sun_ray` in `shader.wgsl` is this, line for line.** A walk over
    /// columns rather than cells -- see the shader for the measurement that
    /// made it one: over each column the ray crosses it looks at the rows it
    /// passes through below that column's [`heights`](Self::heights), and it
    /// ends when something casts, when it is above [`top`](Self::top), or when
    /// it cannot tell.
    #[cfg(test)]
    pub fn sun_ray(&self, from: Vec3, toward_sun: Vec3) -> SunRay {
        let side = SIDE as i32;
        let origin = from.floor().as_ivec3().to_array();
        if origin.iter().any(|&c| c < 0 || c >= side) {
            return SunRay::Unknown;
        }
        let dir = toward_sun;
        if dir.y <= 0.0 {
            return SunRay::Unknown;
        }
        let index = |c: [i32; 3]| c[0] as usize + SIDE * (c[1] as usize + SIDE * c[2] as usize);
        // Standing in a model's cell -- the grass under a table, the shelf
        // under a bench's top -- the model is the thing in the way, and only
        // the map has its shape. See `ASKS_THE_MAP`.
        if self.cells[index(origin)] == ASKS_THE_MAP {
            return SunRay::Unknown;
        }
        let from_caster = self.cells[index(origin)] != 0;
        let mut column = [origin[0], origin[2]];
        let flat = [dir.x, dir.z];
        let stride = flat.map(|d| if d > 0.0 { 1 } else if d < 0.0 { -1 } else { 0 });
        let inverse = flat.map(|d| 1.0 / d.abs().max(1e-6));
        let start = [from.x, from.z];
        let mut t_max = [0.0f32; 2];
        for axis in 0..2 {
            let base = column[axis] as f32;
            let boundary = if flat[axis] > 0.0 { base + 1.0 - start[axis] } else { start[axis] - base };
            t_max[axis] = boundary * inverse[axis];
        }
        let mut t_enter = 0.0f32;
        for _ in 0..MAX_SUN_COLUMNS {
            let t_exit = t_max[0].min(t_max[1]);
            let y_enter = (from.y + dir.y * t_enter).floor() as i32;
            if y_enter > self.top {
                return SunRay::OpenSky;
            }
            let y_exit = (from.y + dir.y * t_exit).floor() as i32;
            let own_column = column == [origin[0], origin[2]];
            let height = i32::from(self.heights[column[0] as usize + SIDE * column[1] as usize]);
            let low = i32::from(self.lows[column[0] as usize + SIDE * column[1] as usize]) - 1;
            let last = y_exit.min(height - 1).min(side - 1);
            // From the lowest thing in the column, not from where the ray
            // came in: see `lows` for the twenty cells of air under a crown
            // this skips.
            for y in y_enter.max(0).max(low)..=last {
                if own_column && y == origin[1] {
                    continue;
                }
                // Wood does not shade the wood it grows from: see `sun_ray` in
                // the shader for the trunk that was dark on its sunny side.
                let stops = self.cells[index([column[0], y, column[1]])];
                if stops == ASKS_THE_MAP {
                    return SunRay::Unknown;
                }
                if stops != 0 && !(from_caster && stops != STOPS_ALL && own_column) {
                    return SunRay::Blocked;
                }
            }
            if y_exit > self.top {
                return SunRay::OpenSky;
            }
            let axis = if t_max[0] < t_max[1] { 0 } else { 1 };
            column[axis] += stride[axis];
            t_max[axis] += inverse[axis];
            t_enter = t_exit;
            if column.iter().any(|&c| c < 0 || c >= side) {
                return SunRay::Unknown;
            }
        }
        SunRay::Unknown
    }

    /// Where a Soft ray aims at the fire whose middle is `flame`: `off` from
    /// it, or the middle itself when that lands in a cell that stops light.
    /// **`flame_aim` in `shader.wgsl` is this, line for line**; see it for the
    /// floor that lost half of every hearth's light.
    #[cfg(test)]
    pub fn flame_aim(&self, flame: Vec3, off: Vec3) -> Vec3 {
        let aim = flame + off;
        if self.stops(aim.floor().as_ivec3().to_array()) {
            flame
        } else {
            aim
        }
    }

    /// Whether a straight line from `from` to `to` -- both counted in blocks
    /// from the volume's corner -- reaches the cell `to` is in without
    /// entering a cell that stops the light.
    ///
    /// **`lamp_ray_clear` in `shader.wgsl` is this, line for line**, and the
    /// tests of this file are the tests of that. A grid walk (Amanatides and
    /// Woo): step into whichever neighbouring cell the line reaches first,
    /// until the fire's own cell. Neither end is tested -- the fire's cell is
    /// allowed to be opaque (glowstone is) and the start is the air in front
    /// of the face being lit.
    ///
    /// **Ties go to x, then y**, the order the shader's comparisons take. A
    /// line through the exact corner of two blocks meeting at an edge
    /// therefore slips between them if the cell the tie picks is air, which is
    /// a crack a pixel wide on a line nobody stands on.
    ///
    /// **Built for tests only**: the game walks in the shader, and a copy the
    /// game never called would be dead code clippy rightly complains of.
    #[cfg(test)]
    pub fn ray_is_clear(&self, from: Vec3, to: Vec3) -> bool {
        let dir = to - from;
        let mut cell = from.floor().as_ivec3().to_array();
        let end = to.floor().as_ivec3().to_array();
        let stride = [dir.x, dir.y, dir.z].map(|d| if d > 0.0 { 1 } else if d < 0.0 { -1 } else { 0 });
        let inverse = [dir.x, dir.y, dir.z].map(|d| 1.0 / d.abs().max(1e-6));
        let start = from.to_array();
        let mut t_max = [0.0f32; 3];
        for axis in 0..3 {
            let base = cell[axis] as f32;
            let boundary = if dir[axis] > 0.0 { base + 1.0 - start[axis] } else { start[axis] - base };
            t_max[axis] = boundary * inverse[axis];
        }
        for _ in 0..MAX_STEPS {
            if cell == end {
                return true;
            }
            let axis = if t_max[0] < t_max[1] && t_max[0] < t_max[2] {
                0
            } else if t_max[1] < t_max[2] {
                1
            } else {
                2
            };
            // Past the far end without landing in its cell -- only a line
            // ending on a cell's very face can do that, and it has arrived.
            if t_max[axis] > 1.0 {
                return true;
            }
            cell[axis] += stride[axis];
            t_max[axis] += inverse[axis];
            if cell == end {
                return true;
            }
            if self.stops(cell) {
                return false;
            }
        }
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use primitive_shared::types::{BLOCK_LEAVES, BLOCK_STONE, BLOCK_WATER};

    /// The first block with a light of its own, found rather than named, so a
    /// renumbered hearth does not break a test about grids.
    fn a_fire() -> BlockId {
        (0..=u16::MAX as u32)
            .map(|id| id as BlockId)
            .find(|&id| primitive_shared::blocks::is_defined(id) && light_emission(id) > 0)
            .expect("some block gives out light")
    }

    /// A volume at the world's corner made from `block_at`.
    fn volume_of(block_at: impl Fn(i32, i32, i32) -> BlockId) -> Volume {
        Volume::gather([0, 0, 0], |x, z, y0, out| {
            for (dy, slot) in out.iter_mut().enumerate() {
                *slot = block_at(x, y0 + dy as i32, z);
            }
        })
    }

    /// A stone floor to y = 10, a fire standing on it at (20, 11, 20), and a
    /// pillar three high two cells east of it.
    fn camp() -> Volume {
        let fire = a_fire();
        volume_of(move |x, y, z| match (x, y, z) {
            _ if y <= 10 => BLOCK_STONE,
            (20, 11, 20) => fire,
            (22, 11..=13, 20) => BLOCK_STONE,
            _ => BLOCK_AIR,
        })
    }

    const FIRE: Vec3 = Vec3::new(20.5, 11.5, 20.5);

    #[test]
    fn a_pillar_between_a_fire_and_the_floor_shadows_the_floor_behind_it_and_not_in_front() {
        let camp = camp();
        // The floor's top faces, looked up from just above them, as
        // `vs_main_shadowed` lifts them.
        let floor = |x: f32, z: f32| Vec3::new(x, 11.05, z);
        assert!(!camp.ray_is_clear(floor(25.5, 20.5), FIRE), "the floor behind the pillar sees the fire");
        assert!(!camp.ray_is_clear(floor(23.5, 20.5), FIRE), "the foot of the pillar's far side sees the fire");
        assert!(camp.ray_is_clear(floor(18.5, 20.5), FIRE), "the floor on the open side is in shadow");
        assert!(camp.ray_is_clear(floor(21.5, 20.5), FIRE), "the floor between the fire and the pillar is in shadow");
        // Round the side of the pillar, the shadow is a spoke, not a disc.
        assert!(camp.ray_is_clear(floor(25.5, 23.5), FIRE), "the shadow of one pillar is wider than it should be");
        // The pillar's own faces: the one toward the fire is lit, the one away
        // from it is not.
        assert!(camp.ray_is_clear(Vec3::new(21.95, 12.5, 20.5), FIRE), "the pillar's lit face is shadowed");
        assert!(!camp.ray_is_clear(Vec3::new(23.05, 12.5, 20.5), FIRE), "the pillar's back face is lit");
    }

    #[test]
    fn a_soft_ray_never_aims_at_a_fire_through_the_floor_it_stands_on() {
        // **The report**: "в пещерах проблемы с тенями" -- at the Soft step the
        // floor round a fire in a closed room and the room's walls came out
        // darker than with no shadows at all, in wedges. Two of the four
        // corners a Soft ray aims at lay in the ground under the fire, and
        // every ray to them crossed the ground. See `flame_aim`. A hearth, not
        // glowstone: a block of glowstone is rightly in the way of its own
        // far corners.
        let hearth = (0..=u16::MAX as u32)
            .map(|id| id as BlockId)
            .find(|&id| primitive_shared::blocks::is_defined(id) && light_emission(id) > 0 && !is_opaque(id))
            .expect("some fire is not a whole block");
        let camp = volume_of(move |x, y, z| match (x, y, z) {
            _ if y <= 10 => BLOCK_STONE,
            (20, 11, 20) => hearth,
            (22, 11..=13, 20) => BLOCK_STONE,
            _ => BLOCK_AIR,
        });
        let corners = [(1.0, 1.0, 1.0), (1.0, -1.0, -1.0), (-1.0, 1.0, -1.0), (-1.0, -1.0, 1.0)]
            .map(|(x, y, z)| Vec3::new(x, y, z) * 0.9);
        for off in corners {
            let aim = camp.flame_aim(FIRE, off);
            assert!(!camp.stops(aim.floor().as_ivec3().to_array()), "a soft ray aims into rock at {aim}");
        }
        // Open floor on the side away from the pillar, as `vs_main_shadowed`
        // lifts it: every ray reaches the fire.
        for (x, z) in [(17.5, 20.5), (20.5, 23.5), (18.5, 17.5), (23.5, 23.5)] {
            let floor = Vec3::new(x, 11.05, z);
            let seen = corners.iter().filter(|&&off| camp.ray_is_clear(floor, camp.flame_aim(FIRE, off))).count();
            assert_eq!(seen, 4, "the open floor at {floor} sees {seen} of the fire's four corners");
        }
        // The pillar still casts: behind it the fire's middle is hidden, and
        // at most the two corners on the far side of the pillar's edge show
        // round it -- the penumbra the Soft step is for.
        let behind = Vec3::new(25.5, 11.05, 20.5);
        let seen = corners.iter().filter(|&&off| camp.ray_is_clear(behind, camp.flame_aim(FIRE, off))).count();
        assert!(!camp.ray_is_clear(behind, FIRE) && seen <= 2, "the pillar stopped casting: {seen} of four seen behind it");
        let source = include_str!("shader.wgsl");
        assert!(source.contains("const LAMP_FLAME: f32 = 0.9;"), "shader.wgsl aims its soft rays somewhere else");
        assert_eq!(source.matches("lamp_ray_clear(local, flame_aim(flame,").count(), 4, "a soft ray in shader.wgsl is not aimed through `flame_aim`");
    }

    #[test]
    fn what_stands_in_the_fires_own_cell_sees_it_and_so_does_glowstone_around_it() {
        // The floor under a hearth lifts into the hearth's cell, and a fire
        // that is itself opaque must not shadow its own neighbours.
        let glow = volume_of(|x, y, z| match (x, y, z) {
            _ if y <= 10 => BLOCK_STONE,
            (20, 11, 20) => BLOCK_STONE,
            _ => BLOCK_AIR,
        });
        assert!(glow.ray_is_clear(Vec3::new(20.5, 11.05, 20.5), FIRE), "the start cell was tested");
        assert!(glow.ray_is_clear(Vec3::new(21.05, 11.5, 20.5), FIRE), "an opaque fire shadows its own face");
    }

    #[test]
    fn the_volume_holds_only_whole_opaque_blocks_and_every_fire_in_it() {
        let fire = a_fire();
        let volume = volume_of(move |x, y, z| match (x, y, z) {
            (1, 2, 3) => BLOCK_STONE,
            (2, 2, 3) => BLOCK_LEAVES,
            (3, 2, 3) => BLOCK_WATER,
            (4, 2, 3) => fire,
            (63, 63, 63) => fire,
            _ => BLOCK_AIR,
        });
        let at = |x: usize, y: usize, z: usize| volume.cells[x + SIDE * (y + SIDE * z)];
        assert_eq!(at(1, 2, 3), STOPS_ALL, "stone does not stop the light");
        assert_eq!(at(2, 2, 3), STOPS_SUN, "a leaf stops a fire, or lets the sun through");
        assert_eq!(at(3, 2, 3), 0, "water casts a shadow");
        assert_eq!(volume.cells.iter().filter(|&&c| c != 0).count(), usize::from(is_opaque(fire)) * 2 + 2);
        assert_eq!(volume.top, 63, "the highest thing in the volume is not where it is");
        let height = |x: usize, z: usize| volume.heights[x + SIDE * z];
        assert_eq!((height(1, 3), height(2, 3), height(3, 3), height(0, 0)), (3, 3, 0, 0), "a column's height is not the row over what casts in it");
        assert_eq!(
            volume.lamps,
            vec![Lamp { cell: [4, 2, 3], level: light_emission(fire) }, Lamp { cell: [63, 63, 63], level: light_emission(fire) }]
        );
    }

    #[test]
    fn the_shader_is_handed_the_nearest_fires_relative_to_the_render_origin() {
        let lamps = (0..40).map(|i| Lamp { cell: [1_000_000 + i * 3, 70, -2_000_000], level: 12 }).collect();
        let volume = Volume { min: [999_968, 38, -2_000_032], cells: Vec::new(), lamps, top: 40, heights: Vec::new(), lows: Vec::new() };
        let eye = Vec3::new(1_000_060.0, 71.0, -2_000_000.0);
        let origin = Vec3::new(1_000_048.0, 64.0, -2_000_016.0);
        let uniform = volume.uniform(eye, origin);
        assert_eq!(uniform.volume, [-80.0, -26.0, -16.0, MAX_LAMPS as f32]);
        assert_eq!(uniform.grid, [1.0, 41.0, 0.0, 0.0], "the shader is not told there is a volume, or how high it goes");
        let handed: Vec<f32> = uniform.lamps.iter().map(|lamp| lamp[0]).collect();
        // The fire the eye stands over is the one at 1_000_060, and the
        // sixteen nearest are the ones from 1_000_036 to 1_000_081.
        assert!(handed.iter().all(|&x| (36.5 - 48.0..=81.5 - 48.0).contains(&x)), "a far fire was handed over: {handed:?}");
        assert_eq!(uniform.lamps[0], [12.5, 6.5, 16.5, 12.0], "the nearest fire is not first, or not where it is");
    }

    #[test]
    fn the_volume_is_taken_again_only_when_the_eye_leaves_its_middle_or_the_world_changes() {
        let eye = Vec3::new(100.2, 70.0, -40.7);
        let first = needs_new(None, false, eye.as_dvec3()).expect("no volume, and none was asked for");
        assert_eq!(first, [68, 38, -73]);
        assert_eq!(needs_new(Some(first), false, (eye + Vec3::new(12.0, -12.0, 11.9)).as_dvec3()), None, "taken again inside the margin");
        assert!(needs_new(Some(first), false, (eye + Vec3::new(13.0, 0.0, 0.0)).as_dvec3()).is_some(), "kept after the eye walked out");
        assert!(needs_new(Some(first), true, eye.as_dvec3()).is_some(), "kept after a block in it changed");
    }

    #[test]
    fn a_crown_shades_the_sand_under_it_and_open_sand_sees_the_sky() {
        // A tree the sun is nearly over: a trunk three pieces high and a crown
        // three across over it, on stone.
        use primitive_shared::types::branch;
        let tree = volume_of(|x, y, z| match (x, y, z) {
            _ if y <= 10 => BLOCK_STONE,
            (20, 11..=14, 20) => branch(12),
            (19..=21, 15..=16, 19..=21) => BLOCK_LEAVES,
            _ => BLOCK_AIR,
        });
        let noon = Vec3::new(0.1, 1.0, 0.05).normalize();
        assert_eq!(tree.sun_ray(Vec3::new(21.5, 11.05, 20.5), noon), SunRay::Blocked, "the crown lets the sun through");
        assert_eq!(tree.sun_ray(Vec3::new(40.5, 11.05, 40.5), noon), SunRay::OpenSky, "open ground is in shadow");
        // A twig casts no cell of shadow: only wood as thick as a bough is in
        // the way. It is handed to the map instead, which has the twig's own
        // two sixteenths (`ASKS_THE_MAP`) -- not open sky, and not blocked.
        let twig = volume_of(|x, y, z| match (x, y, z) {
            _ if y <= 10 => BLOCK_STONE,
            (20, 11..=14, 20) => branch(2),
            _ => BLOCK_AIR,
        });
        assert_eq!(twig.sun_ray(Vec3::new(20.5, 11.05, 21.5), Vec3::new(0.0, 1.0, -0.4).normalize()), SunRay::Unknown, "a twig casts a cell of shadow, or the map is not asked about it");
        // A bare trunk's bark sees the sun from inside its own cell: the pieces
        // over it are the same trunk, not a roof.
        let pole = volume_of(|x, y, z| match (x, y, z) {
            _ if y <= 10 => BLOCK_STONE,
            (20, 11..=14, 20) => branch(12),
            _ => BLOCK_AIR,
        });
        assert_eq!(pole.sun_ray(Vec3::new(20.9, 11.5, 20.5), noon), SunRay::OpenSky, "a trunk shades its own sunny side");
        // A low sun's ray runs along the ground and out of the side: the map's
        // question, not the volume's.
        let low = Vec3::new(1.0, 0.08, 0.0).normalize();
        assert_eq!(tree.sun_ray(Vec3::new(5.5, 11.05, 5.5), low), SunRay::Unknown, "a ray that left the volume was taken for an answer");
    }

    #[test]
    fn a_ray_past_a_workbench_is_handed_to_the_map_and_one_past_nothing_is_not() {
        // **The report**: the workshops threw no shadow at the Hard step. A
        // ray from the grass beside a bench, straight up through the bench's
        // cell, was called open sky -- the bench's cell cast nothing -- and
        // the map, which has the bench, was never asked. See `ASKS_THE_MAP`.
        use primitive_shared::types::{faced, Facing, BLOCK_CHEST, BLOCK_DOOR, BLOCK_DRYING_RACK, BLOCK_STONE, BLOCK_TALL_GRASS, BLOCK_WORKBENCH};
        for model in [
            faced(BLOCK_WORKBENCH, Facing::South),
            faced(BLOCK_CHEST, Facing::North),
            faced(BLOCK_DRYING_RACK, Facing::East),
            faced(BLOCK_DOOR, Facing::West),
        ] {
            let volume = Volume::gather([0, 0, 0], |x, z, _, column| {
                for (y, slot) in column.iter_mut().enumerate() {
                    *slot = match (x, y, z) {
                        (_, 0, _) => BLOCK_STONE,
                        (10, 1, 10) => model,
                        (20, 1, 20) => BLOCK_TALL_GRASS,
                        _ => BLOCK_AIR,
                    };
                }
            });
            let up = Vec3::new(0.05, 1.0, 0.03).normalize();
            let name = primitive_shared::types::block_name(model);
            // Under it, beside it with the sun over it, and far from it.
            assert_eq!(volume.sun_ray(Vec3::new(10.5, 1.01, 10.5), up), SunRay::Unknown, "{name}: in its own cell");
            assert_eq!(volume.sun_ray(Vec3::new(10.5, 0.99, 10.5), up), SunRay::Unknown, "{name}: under it");
            assert_eq!(volume.sun_ray(Vec3::new(30.5, 1.01, 30.5), up), SunRay::OpenSky, "{name}: nowhere near it");
            // A tuft is still nothing to the walk: plants are the map's
            // business by the setting, not by the volume.
            assert_eq!(volume.sun_ray(Vec3::new(20.5, 0.99, 20.5), up), SunRay::OpenSky, "a tuft asked the map");
        }
    }

    #[test]
    fn a_ray_past_a_slab_a_lip_or_a_bitten_block_is_handed_to_the_map() {
        // **The report**: at the Hard step a roof of slabs cast nothing inside
        // the volume, where the Soft step drew its shadow on the same ground.
        // Every block short of a whole one was air to the walk: the ray from
        // the ground under the roof went up through it and was called open
        // sky. See `is_part_of_a_block`.
        use primitive_shared::dig::{lowered, next_bite, Side};
        use primitive_shared::types::{BLOCK_DIRT, BLOCK_GRASS, BLOCK_TILE_SLAB};
        let bitten = next_bite(BLOCK_DIRT, Side::PosX).expect("dirt can be bitten from the side");
        for part in [BLOCK_TILE_SLAB, lowered(BLOCK_GRASS, 3), lowered(BLOCK_STONE, 2), bitten] {
            let volume = volume_of(|x, y, z| match (x, y, z) {
                _ if y <= 10 => BLOCK_STONE,
                (18..=22, 14, 18..=22) => part,
                _ => BLOCK_AIR,
            });
            let name = primitive_shared::types::block_name(part);
            let up = Vec3::new(0.05, 1.0, 0.03).normalize();
            assert_eq!(volume.sun_ray(Vec3::new(20.5, 11.05, 20.5), up), SunRay::Unknown, "{name}: the ground under it was called open sky");
            assert_eq!(volume.sun_ray(Vec3::new(40.5, 11.05, 40.5), up), SunRay::OpenSky, "{name}: open ground far from it was not open sky");
        }
        // Water is still nothing the sun stops.
        let pond = volume_of(|x, y, z| match (x, y, z) {
            _ if y <= 10 => BLOCK_STONE,
            (18..=22, 11, 18..=22) => BLOCK_WATER,
            _ => BLOCK_AIR,
        });
        assert_eq!(pond.cells.iter().filter(|&&c| c != 0 && c != STOPS_ALL).count(), 0, "a pond asks the map");
    }

    #[test]
    fn the_walk_leaves_out_the_plants_the_map_leaves_out() {
        // `ClientSettings::plant_shadows` changes what the map draws; a walk
        // that disagreed would put a crown's shadow back inside the volume
        // with the crowns turned off, or call a tuft's shadow open sky with
        // the tufts turned on -- the workshops' bug again, for plants.
        use crate::engine::shadow::PlantShadows;
        use primitive_shared::types::BLOCK_TALL_GRASS;
        let world = |x: i32, z: i32, column: &mut [BlockId]| {
            for (y, slot) in column.iter_mut().enumerate() {
                *slot = match (x, y, z) {
                    (_, 0, _) => BLOCK_STONE,
                    (4, 3, 4) => BLOCK_LEAVES,
                    (8, 1, 8) => BLOCK_TALL_GRASS,
                    _ => BLOCK_AIR,
                };
            }
        };
        let up = Vec3::new(0.05, 1.0, 0.03).normalize();
        let under_crown = Vec3::new(4.5, 1.01, 4.5);
        let under_tuft = Vec3::new(8.5, 0.99, 8.5);
        for (plants, crown, tuft) in [
            (PlantShadows::Off, SunRay::OpenSky, SunRay::OpenSky),
            (PlantShadows::Trees, SunRay::Blocked, SunRay::OpenSky),
            (PlantShadows::All, SunRay::Blocked, SunRay::Unknown),
        ] {
            let volume = Volume::gather_for([0, 0, 0], plants, |x, z, _, column| world(x, z, column));
            assert_eq!(volume.sun_ray(under_crown, up), crown, "{plants:?}: under a crown");
            assert_eq!(volume.sun_ray(under_tuft, up), tuft, "{plants:?}: under a tuft");
        }
    }

    #[test]
    fn the_shader_walks_the_grid_this_file_fills() {
        let source = include_str!("shader.wgsl");
        for line in [
            format!("const LAMP_SIDE: f32 = {}.0;", SIDE),
            format!("const LAMP_STEPS: i32 = {};", MAX_STEPS),
            format!("const SUN_COLUMNS: i32 = {};", MAX_SUN_COLUMNS),
            format!("lamps: array<vec4<f32>, {}>,", MAX_LAMPS),
        ] {
            assert!(source.contains(&line), "shader.wgsl no longer says `{line}`");
        }
        // The two cuts the shader reads a cell's byte through.
        assert!(f32::from(STOPS_SUN) / 255.0 > 0.25 && f32::from(STOPS_SUN) / 255.0 < 0.75);
        assert!(f32::from(ASKS_THE_MAP) / 255.0 > 0.1 && f32::from(ASKS_THE_MAP) / 255.0 < 0.25);
        assert!(source.contains("const MAP_STOPS: f32 = 0.1;"), "shader.wgsl no longer cuts at `MAP_STOPS`");
        assert_eq!(std::mem::size_of::<LampUniform>(), 4 * 4 * (MAX_LAMPS + 2));
        // The column's two bytes, read out of one `Rg8` texel: the roof in
        // red and the floor in green, and the walk starting at the floor.
        // See `Volume::lows`.
        for line in [
            "return vec2<i32>(round(textureLoad(lamp_heights, column, 0).rg * 255.0));",
            "for (var y = max(max(y_enter, 0), bounds.y - 1); y <= last; y = y + 1) {",
        ] {
            assert!(source.contains(line), "shader.wgsl no longer says `{line}`");
        }
    }

    /// **Nothing that casts stands outside its column's floor and roof.**
    ///
    /// The walk starts at the floor rather than at the row the ray came in
    /// on (`Volume::lows`), which is only safe while the floor is at or
    /// below every casting cell in the column. Get it wrong by one and a
    /// crown's lowest layer is stepped over: the ground under an oak would
    /// be called open sky, and the shadow of a wood would come and go with
    /// which layer of leaves happened to be lowest.
    #[test]
    fn every_cell_that_casts_stands_between_its_columns_floor_and_its_roof() {
        use primitive_shared::types::{BLOCK_LEAVES, BLOCK_STONE};
        // A meadow with a crown floating ten blocks over it and a pillar
        // beside it: a column with a gap in the middle, which is the shape
        // the floor and the roof have to bracket.
        let volume = Volume::gather([0, 0, 0], |gx, gz, y0, column| {
            for (dy, slot) in column.iter_mut().enumerate() {
                let y = y0 + dy as i32;
                let crown = (20..=24).contains(&gx) && (20..=24).contains(&gz) && (14..=16).contains(&y);
                let pillar = gx == 30 && gz == 22 && (1..=6).contains(&y);
                *slot = if y == 0 || crown {
                    if crown {
                        BLOCK_LEAVES
                    } else {
                        BLOCK_STONE
                    }
                } else if pillar {
                    BLOCK_STONE
                } else {
                    BLOCK_AIR
                };
            }
        });
        let mut columns_with_a_gap = 0;
        for z in 0..SIDE {
            for x in 0..SIDE {
                let (roof, floor) = (volume.heights[x + SIDE * z], volume.lows[x + SIDE * z]);
                let casting: Vec<usize> = (0..SIDE).filter(|&y| volume.cells[x + SIDE * (y + SIDE * z)] != 0).collect();
                match casting.first() {
                    None => assert_eq!((roof, floor), (0, 0), "an empty column claimed a floor or a roof"),
                    Some(&lowest) => {
                        let highest = *casting.last().expect("a first means a last");
                        assert_eq!(usize::from(floor), lowest + 1, "the floor at ({x}, {z}) is not the lowest caster");
                        assert_eq!(usize::from(roof), highest + 1, "the roof at ({x}, {z}) is not the highest caster");
                        columns_with_a_gap += usize::from(highest > lowest + 1);
                    }
                }
            }
        }
        assert!(columns_with_a_gap >= 25, "the scene was meant to have columns with a gap in them");
        // And the answer the walk gives is still the one it gave: the
        // ground under the crown is shaded, the ground beside it is not.
        let straight_up = Vec3::new(0.05, 1.0, 0.03).normalize();
        assert_eq!(volume.sun_ray(Vec3::new(22.5, 1.01, 22.5), straight_up), SunRay::Blocked, "the crown let the sun through");
        assert_eq!(volume.sun_ray(Vec3::new(40.5, 1.01, 40.5), straight_up), SunRay::OpenSky, "open meadow was called shaded");
    }

    /// How long a volume takes to fill from a real chunk manager, which is
    /// the hitch a walking player feels once every dozen blocks.
    #[test]
    #[ignore = "a measurement, not an assertion -- run it with --release"]
    fn what_a_volume_costs_to_fill() {
        use crate::logic::chunk_manager::ChunkManager;
        use primitive_shared::types::ChunkPos;
        use primitive_shared::worldgen::WorldGen;
        let generator = WorldGen::new(1337);
        let mut chunks = ChunkManager::new(4);
        for cz in -3..=3 {
            for cx in -3..=3 {
                chunks.insert(generator.generate_chunk(ChunkPos::new(cx, cz)));
            }
        }
        let min = corner_for((Vec3::new(8.0, 70.0, 8.0)).as_dvec3());
        let started = std::time::Instant::now();
        let rounds = 20;
        for _ in 0..rounds {
            let volume = Volume::gather(min, |gx, gz, y0, out| match chunks.column(gx, gz) {
                Some(column) => {
                    for (dy, slot) in out.iter_mut().enumerate() {
                        *slot = column.block(y0 + dy as i32);
                    }
                }
                None => out.fill(BLOCK_AIR),
            });
            std::hint::black_box(volume);
        }
        println!("one volume: {:.2} ms", started.elapsed().as_secs_f64() * 1000.0 / f64::from(rounds));
    }
}
