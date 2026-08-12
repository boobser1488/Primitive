//! Этап 4: "Простая физика: гравитация, прыжок, столкновение с блоками."
//! Plan's suggested starting point: "проверяйте, не находится ли новая
//! позиция игрока внутри блока. Если находится -- откатывайте движение."
//! This does exactly that, one axis at a time (so sliding along a wall
//! still lets the other axis keep moving, instead of a single combined
//! check freezing all motion the instant any axis touches a block).
//!
//! Changes this pass:
//! - `move_speed` comes from settings instead of being a hard-coded
//!   constant the settings file only *pretended* to control;
//! - `teleport()` exists so the server's anti-cheat can rubber-band the
//!   player back without the client fighting it;
//! - `grounded` is reported to the server, which cross-checks it against
//!   the real world.

use glam::Vec3;

use primitive_shared::types::is_liquid;

use crate::chunk_manager::ChunkManager;

// Re-exported from the shared crate so the client and the server can't
// drift apart on what counts as "inside a player".
pub use primitive_shared::geometry::{EYE_HEIGHT, PLAYER_HALF_WIDTH, PLAYER_HEIGHT};

const GRAVITY: f32 = -22.0;
const JUMP_VELOCITY: f32 = 8.0;
const TERMINAL_VELOCITY: f32 = -50.0;

// --- water ---
/// Gravity is much weaker in water, and drag does most of the work.
const WATER_GRAVITY: f32 = -4.0;
/// Fraction of velocity kept per second while submerged. Water is
/// viscous: without this you accelerate to terminal velocity anyway and
/// "swimming" just means falling slowly.
const WATER_DRAG_PER_SECOND: f32 = 0.02;
/// Upward push while holding jump under water.
const SWIM_UP_SPEED: f32 = 4.0;
/// You can't sink faster than this, however long you've been falling.
const WATER_SINK_SPEED: f32 = -3.0;
/// Horizontal movement multiplier while swimming.
const WATER_MOVE_FACTOR: f32 = 0.55;
/// Entering water kills most of your fall speed immediately -- this is
/// what makes water break a long fall.
const WATER_ENTRY_DAMPING: f32 = 0.25;
#[allow(dead_code)] // fallback used by tests and by callers without a settings file
pub const DEFAULT_MOVE_SPEED: f32 = 5.5;

pub struct Player {
    /// Feet position (bottom-centre of the collider), world space.
    pub position: Vec3,
    pub velocity: Vec3,
    pub grounded: bool,
    pub move_speed: f32,
    /// Any part of the collider is in water.
    pub in_water: bool,
    /// The head is under water (drives the underwater fog).
    pub submerged: bool,
}

impl Player {
    pub fn new(spawn: Vec3, move_speed: f32) -> Self {
        Self {
            position: spawn,
            velocity: Vec3::ZERO,
            grounded: false,
            move_speed: move_speed.clamp(0.5, 20.0),
            in_water: false,
            submerged: false,
        }
    }

    pub fn eye_position(&self) -> Vec3 {
        self.position + Vec3::new(0.0, EYE_HEIGHT, 0.0)
    }

    /// Server-authoritative reposition (anti-cheat correction, or a future
    /// spawn/respawn). Velocity is cleared so the player doesn't
    /// immediately continue the move that got them corrected.
    pub fn teleport(&mut self, position: Vec3) {
        self.position = position;
        self.velocity = Vec3::ZERO;
    }

    /// `wish_dir` is a normalized (or zero) horizontal move direction in
    /// world space, already combining WASD with camera yaw. `other_players`
    /// are other players' current feet positions -- their hitboxes are
    /// solid obstacles too, same as blocks.
    /// Jump takes two flags because water and land want different ones:
    /// on land a jump fires on the press edge (holding the key must not
    /// auto-hop), in water holding it swims upward continuously.
    pub fn update(
        &mut self,
        chunks: &ChunkManager,
        other_players: &[Vec3],
        wish_dir: Vec3,
        jump_pressed: bool,
        jump_held: bool,
        dt: f32,
    ) {
        let was_in_water = self.in_water;
        self.refresh_fluid_state(chunks);

        if self.in_water {
            // Hitting water kills most of the fall. Only on *entry*: a
            // continuous damping would make sinking feel like syrup.
            if !was_in_water && self.velocity.y < 0.0 {
                self.velocity.y *= WATER_ENTRY_DAMPING;
            }

            let speed = self.move_speed * WATER_MOVE_FACTOR;
            self.velocity.x = wish_dir.x * speed;
            self.velocity.z = wish_dir.z * speed;

            if jump_held {
                // Swim up. Set rather than add, so holding jump rises at
                // a steady rate instead of accelerating out of the lake.
                self.velocity.y = SWIM_UP_SPEED;
            } else {
                self.velocity.y += WATER_GRAVITY * dt;
                // Exponential drag, framerate-independent.
                self.velocity.y *= WATER_DRAG_PER_SECOND.powf(dt);
                self.velocity.y = self.velocity.y.max(WATER_SINK_SPEED);
            }
        } else {
            // Horizontal velocity is set directly from input (arcade-y,
            // no acceleration/friction curve -- fine for a prototype,
            // easy to swap for accel-based movement later).
            self.velocity.x = wish_dir.x * self.move_speed;
            self.velocity.z = wish_dir.z * self.move_speed;

            if jump_pressed && self.grounded {
                self.velocity.y = JUMP_VELOCITY;
            }

            self.velocity.y = (self.velocity.y + GRAVITY * dt).max(TERMINAL_VELOCITY);
        }

        let delta = self.velocity * dt;
        self.grounded = false;

        // Resolve one axis at a time so hitting a wall doesn't also kill
        // vertical/other-horizontal motion in the same step.
        self.move_axis(chunks, other_players, Vec3::new(delta.x, 0.0, 0.0), Axis::X);
        self.move_axis(chunks, other_players, Vec3::new(0.0, 0.0, delta.z), Axis::Z);
        self.move_axis(chunks, other_players, Vec3::new(0.0, delta.y, 0.0), Axis::Y);

        // Recompute after moving so the caller (fog, HUD) sees this
        // frame's state, not last frame's.
        self.refresh_fluid_state(chunks);
    }

    /// Samples the world for water at the feet and at eye level.
    fn refresh_fluid_state(&mut self, chunks: &ChunkManager) {
        let feet = self.position;
        let eye = self.eye_position();
        let sample = |p: Vec3| {
            matches!(
                chunks.block_at(p.x.floor() as i32, p.y.floor() as i32, p.z.floor() as i32),
                Some(id) if is_liquid(id)
            )
        };
        // Mid-body as well: standing in a one-deep pool should still
        // count as being in water even when the head is in open air.
        let waist = feet + Vec3::new(0.0, PLAYER_HEIGHT * 0.5, 0.0);
        self.in_water = sample(feet) || sample(waist) || sample(eye);
        self.submerged = sample(eye);
    }

    fn move_axis(&mut self, chunks: &ChunkManager, other_players: &[Vec3], delta: Vec3, axis: Axis) {
        let target = self.position + delta;
        let blocked = aabb_intersects_solid(chunks, target)
            || other_players
                .iter()
                .any(|&other_feet| aabb_overlaps_player(target, other_feet));
        if !blocked {
            self.position = target;
            return;
        }

        // FIX: naive "revert the move" per the plan's suggested starting
        // point, plus zeroing the offending velocity component so gravity
        // doesn't keep trying to push us through the floor every frame.
        match axis {
            Axis::Y => {
                if self.velocity.y < 0.0 {
                    self.grounded = true;
                }
                self.velocity.y = 0.0;
            }
            Axis::X => self.velocity.x = 0.0,
            Axis::Z => self.velocity.z = 0.0,
        }
    }
}

enum Axis {
    X,
    Y,
    Z,
}

/// Simple fixed-step raycast (not true DDA/voxel traversal, but plenty
/// accurate at this step size for a 6-block interaction range) used for
/// breaking/placing blocks. Returns (hit block coord, coord just before
/// the hit -- i.e. where a new block would go).
pub fn raycast_block(
    chunks: &ChunkManager,
    origin: Vec3,
    dir: Vec3,
    max_distance: f32,
) -> Option<((i32, i32, i32), (i32, i32, i32))> {
    let step = 0.05;
    let mut travelled = 0.0;
    let mut prev = block_coord(origin);
    while travelled < max_distance {
        let p = origin + dir * travelled;
        let current = block_coord(p);
        if chunks.is_targetable(current.0, current.1, current.2) {
            return Some((current, prev));
        }
        prev = current;
        travelled += step;
    }
    None
}

fn block_coord(p: Vec3) -> (i32, i32, i32) {
    (p.x.floor() as i32, p.y.floor() as i32, p.z.floor() as i32)
}

/// True if the player's AABB at `feet_pos` overlaps any solid block.
fn aabb_intersects_solid(chunks: &ChunkManager, feet_pos: Vec3) -> bool {
    let min = feet_pos - Vec3::new(PLAYER_HALF_WIDTH, 0.0, PLAYER_HALF_WIDTH);
    let max = feet_pos + Vec3::new(PLAYER_HALF_WIDTH, PLAYER_HEIGHT, PLAYER_HALF_WIDTH);

    let min_block = min.floor();
    let max_block = (max - Vec3::splat(1e-4)).floor(); // avoid grabbing the next block on an exact boundary

    let (bx0, by0, bz0) = (min_block.x as i32, min_block.y as i32, min_block.z as i32);
    let (bx1, by1, bz1) = (max_block.x as i32, max_block.y as i32, max_block.z as i32);

    for by in by0..=by1 {
        for bz in bz0..=bz1 {
            for bx in bx0..=bx1 {
                if chunks.is_solid(bx, by, bz) {
                    return true;
                }
            }
        }
    }
    false
}

/// "Хитбоксы игрокам": AABB-vs-AABB overlap between the local player at
/// `feet_pos` and another player standing at `other_feet` -- both using
/// the same PLAYER_HALF_WIDTH/PLAYER_HEIGHT box, so two players simply
/// can't occupy the same space.
fn aabb_overlaps_player(feet_pos: Vec3, other_feet: Vec3) -> bool {
    let min_a = feet_pos - Vec3::new(PLAYER_HALF_WIDTH, 0.0, PLAYER_HALF_WIDTH);
    let max_a = feet_pos + Vec3::new(PLAYER_HALF_WIDTH, PLAYER_HEIGHT, PLAYER_HALF_WIDTH);
    let min_b = other_feet - Vec3::new(PLAYER_HALF_WIDTH, 0.0, PLAYER_HALF_WIDTH);
    let max_b = other_feet + Vec3::new(PLAYER_HALF_WIDTH, PLAYER_HEIGHT, PLAYER_HALF_WIDTH);

    min_a.x < max_b.x
        && max_a.x > min_b.x
        && min_a.y < max_b.y
        && max_a.y > min_b.y
        && min_a.z < max_b.z
        && max_a.z > min_b.z
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use primitive_shared::types::{
        Chunk, ChunkPos, BLOCK_AIR, BLOCK_STONE, BLOCK_WATER, CHUNK_VOLUME,
    };

    /// A floor with a water column filling y = 10..=19 over part of it.
    pub fn lake_world() -> ChunkManager {
        let mut cm = ChunkManager::new(2);
        let mut blocks = vec![BLOCK_AIR; CHUNK_VOLUME];
        for y in 0..10 {
            for z in 0..16 {
                for x in 0..16 {
                    blocks[Chunk::index(x, y, z)] = BLOCK_STONE;
                }
            }
        }
        for y in 10..20 {
            for z in 0..16 {
                for x in 0..16 {
                    blocks[Chunk::index(x, y, z)] = BLOCK_WATER;
                }
            }
        }
        cm.insert(Chunk {
            pos: ChunkPos::new(0, 0),
            blocks,
        });
        cm
    }

    /// A single chunk at the origin with a stone floor at y = 0..=9.
    pub fn floor_world() -> ChunkManager {
        let mut cm = ChunkManager::new(2);
        let mut blocks = vec![BLOCK_AIR; CHUNK_VOLUME];
        for y in 0..10 {
            for z in 0..16 {
                for x in 0..16 {
                    blocks[Chunk::index(x, y, z)] = BLOCK_STONE;
                }
            }
        }
        cm.insert(Chunk {
            pos: ChunkPos::new(0, 0),
            blocks,
        });
        cm
    }

    #[test]
    fn a_player_falls_and_lands_on_the_floor() {
        let chunks = floor_world();
        let mut player = Player::new(Vec3::new(8.0, 25.0, 8.0), DEFAULT_MOVE_SPEED);
        for _ in 0..400 {
            player.update(&chunks, &[], Vec3::ZERO, false, false, 1.0 / 60.0);
        }
        assert!(player.grounded, "player never landed");
        assert!(
            (player.position.y - 10.0).abs() < 0.1,
            "landed at {} instead of on top of the floor",
            player.position.y
        );
    }

    #[test]
    fn move_speed_from_settings_is_actually_used() {
        let chunks = floor_world();
        let mut slow = Player::new(Vec3::new(8.0, 10.0, 8.0), 1.0);
        let mut fast = Player::new(Vec3::new(8.0, 10.0, 8.0), 8.0);
        for _ in 0..30 {
            slow.update(&chunks, &[], Vec3::X, false, false, 1.0 / 60.0);
            fast.update(&chunks, &[], Vec3::X, false, false, 1.0 / 60.0);
        }
        assert!(
            fast.position.x > slow.position.x + 0.5,
            "the configured speed had no effect"
        );
    }

    #[test]
    fn players_cannot_stand_inside_each_other() {
        let chunks = floor_world();
        let mut player = Player::new(Vec3::new(8.0, 10.0, 8.0), DEFAULT_MOVE_SPEED);
        let other = Vec3::new(8.6, 10.0, 8.0);
        for _ in 0..60 {
            player.update(&chunks, &[other], Vec3::X, false, false, 1.0 / 60.0);
        }
        assert!(
            !aabb_overlaps_player(player.position, other),
            "walked straight through another player"
        );
    }

    #[test]
    fn teleport_clears_momentum() {
        let mut player = Player::new(Vec3::new(0.0, 50.0, 0.0), DEFAULT_MOVE_SPEED);
        player.velocity = Vec3::new(3.0, -20.0, 1.0);
        player.teleport(Vec3::new(1.0, 2.0, 3.0));
        assert_eq!(player.position, Vec3::new(1.0, 2.0, 3.0));
        assert_eq!(player.velocity, Vec3::ZERO);
    }

    #[test]
    fn raycast_finds_the_block_and_the_cell_in_front_of_it() {
        let chunks = floor_world();
        // Looking straight down from above the floor.
        let hit = raycast_block(&chunks, Vec3::new(8.5, 12.0, 8.5), -Vec3::Y, 6.0);
        let (block, before) = hit.expect("ray should have hit the floor");
        assert_eq!(block, (8, 9, 8));
        assert_eq!(before, (8, 10, 8), "placement cell must be above the hit");
    }
}

#[cfg(test)]
mod water_tests {
    use super::tests::*;
    use super::*;

    #[test]
    fn you_do_not_walk_on_water() {
        // Regression: water used to be collidable, so the surface of a
        // lake behaved like a solid floor.
        let chunks = lake_world();
        let mut player = Player::new(Vec3::new(8.0, 25.0, 8.0), DEFAULT_MOVE_SPEED);
        for _ in 0..120 {
            player.update(&chunks, &[], Vec3::ZERO, false, false, 1.0 / 60.0);
        }
        assert!(
            player.position.y < 20.0,
            "player stayed on the surface at y={}",
            player.position.y
        );
        assert!(player.in_water, "should be in the water by now");
    }

    #[test]
    fn you_sink_slowly_rather_than_falling() {
        let chunks = lake_world();
        let mut swimmer = Player::new(Vec3::new(8.0, 18.0, 8.0), DEFAULT_MOVE_SPEED);
        let mut faller = Player::new(Vec3::new(8.0, 18.0, 8.0), DEFAULT_MOVE_SPEED);
        let air = floor_world();

        for _ in 0..30 {
            swimmer.update(&chunks, &[], Vec3::ZERO, false, false, 1.0 / 60.0);
            faller.update(&air, &[], Vec3::ZERO, false, false, 1.0 / 60.0);
        }
        assert!(
            swimmer.position.y > faller.position.y,
            "sinking ({}) should be slower than falling ({})",
            swimmer.position.y,
            faller.position.y
        );
        assert!(
            swimmer.velocity.y >= WATER_SINK_SPEED - 0.01,
            "sink speed must be capped, got {}",
            swimmer.velocity.y
        );
    }

    #[test]
    fn holding_jump_swims_upward() {
        let chunks = lake_world();
        let mut player = Player::new(Vec3::new(8.0, 12.0, 8.0), DEFAULT_MOVE_SPEED);
        let start = player.position.y;
        for _ in 0..60 {
            player.update(&chunks, &[], Vec3::ZERO, true, true, 1.0 / 60.0);
        }
        assert!(
            player.position.y > start + 1.0,
            "should have risen, went from {start} to {}",
            player.position.y
        );
    }

    #[test]
    fn swimming_up_does_not_launch_you_out_of_the_lake() {
        // Set-don't-add: holding jump rises steadily instead of
        // accelerating into orbit.
        let chunks = lake_world();
        let mut player = Player::new(Vec3::new(8.0, 12.0, 8.0), DEFAULT_MOVE_SPEED);
        for _ in 0..60 {
            player.update(&chunks, &[], Vec3::ZERO, true, true, 1.0 / 60.0);
            assert!(
                player.velocity.y <= SWIM_UP_SPEED + 0.01,
                "swim speed ran away: {}",
                player.velocity.y
            );
        }
    }

    #[test]
    fn water_breaks_a_long_fall() {
        let chunks = lake_world();
        let mut player = Player::new(Vec3::new(8.0, 55.0, 8.0), DEFAULT_MOVE_SPEED);
        // Fall until we're in the water.
        for _ in 0..600 {
            player.update(&chunks, &[], Vec3::ZERO, false, false, 1.0 / 60.0);
            if player.in_water {
                break;
            }
        }
        assert!(player.in_water, "never reached the water");
        // A few frames later the speed must be back to swimming pace.
        for _ in 0..10 {
            player.update(&chunks, &[], Vec3::ZERO, false, false, 1.0 / 60.0);
        }
        assert!(
            player.velocity.y >= WATER_SINK_SPEED - 0.01,
            "fall speed survived the splash: {}",
            player.velocity.y
        );
    }

    #[test]
    fn swimming_is_slower_than_walking() {
        let chunks = lake_world();
        let air = floor_world();
        let mut swimmer = Player::new(Vec3::new(2.0, 15.0, 8.0), DEFAULT_MOVE_SPEED);
        let mut walker = Player::new(Vec3::new(2.0, 10.0, 8.0), DEFAULT_MOVE_SPEED);
        for _ in 0..30 {
            swimmer.update(&chunks, &[], Vec3::X, false, false, 1.0 / 60.0);
            walker.update(&air, &[], Vec3::X, false, false, 1.0 / 60.0);
        }
        assert!(
            swimmer.position.x < walker.position.x,
            "swimming ({}) should be slower than walking ({})",
            swimmer.position.x,
            walker.position.x
        );
    }

    #[test]
    fn submerged_only_counts_when_the_head_is_under() {
        let chunks = lake_world();
        // Standing on the lake bed with the head well above the surface.
        let mut shallow = Player::new(Vec3::new(8.0, 19.0, 8.0), DEFAULT_MOVE_SPEED);
        shallow.refresh_fluid_state(&chunks);
        assert!(shallow.in_water, "feet are in the water");
        assert!(!shallow.submerged, "head is in open air");

        let mut deep = Player::new(Vec3::new(8.0, 12.0, 8.0), DEFAULT_MOVE_SPEED);
        deep.refresh_fluid_state(&chunks);
        assert!(deep.submerged, "head should be under water");
    }
}

/// Physics against the *real* world generator, not a hand-built test
/// fixture. A synthetic lake can accidentally be shaped to pass; actual
/// terrain is what players walk on.
#[cfg(test)]
mod real_terrain_tests {
    use super::*;
    use primitive_shared::types::{ChunkPos, BLOCK_WATER};
    use primitive_shared::worldgen::{WorldGen, SEA_LEVEL};

    /// Loads a 3x3 block of generated chunks around the origin and finds
    /// a column whose surface is open water.
    fn ocean_column(seed: u32) -> Option<(ChunkManager, i32, i32)> {
        let generator = WorldGen::new(seed);
        let mut cm = ChunkManager::new(4);
        for cx in -1..=1 {
            for cz in -1..=1 {
                cm.insert(generator.generate_chunk(ChunkPos::new(cx, cz)));
            }
        }

        for gx in -16..32 {
            for gz in -16..32 {
                let surface = cm.block_at(gx, SEA_LEVEL, gz);
                let above = cm.block_at(gx, SEA_LEVEL + 1, gz);
                if surface == Some(BLOCK_WATER) && above == Some(primitive_shared::types::BLOCK_AIR)
                {
                    // Make sure it's deep enough to be a real test: at
                    // least 3 blocks of water under the surface.
                    let deep = (1..=3).all(|d| cm.block_at(gx, SEA_LEVEL - d, gz) == Some(BLOCK_WATER));
                    if deep {
                        return Some((cm, gx, gz));
                    }
                }
            }
        }
        None
    }

    #[test]
    fn you_cannot_stand_on_the_surface_of_a_real_ocean() {
        let mut found_any = false;
        for seed in [1337u32, 42, 7, 2024] {
            let Some((chunks, gx, gz)) = ocean_column(seed) else {
                continue;
            };
            found_any = true;

            // Drop in from just above the surface, walking forward the
            // whole time -- the way a player runs off a beach.
            let mut player = Player::new(
                Vec3::new(gx as f32 + 0.5, SEA_LEVEL as f32 + 2.0, gz as f32 + 0.5),
                DEFAULT_MOVE_SPEED,
            );
            for _ in 0..180 {
                player.update(&chunks, &[], Vec3::ZERO, false, false, 1.0 / 60.0);
            }

            assert!(
                player.in_water,
                "seed {seed}: player at ({gx},{gz}) never entered the water (y={})",
                player.position.y
            );
            assert!(
                player.position.y < SEA_LEVEL as f32,
                "seed {seed}: player is standing on the water surface at y={}",
                player.position.y
            );
        }
        assert!(found_any, "no ocean column found in any test seed");
    }

    #[test]
    fn the_seabed_still_stops_you() {
        // The other half of the bug: sinking must not continue through
        // the floor of the ocean.
        let Some((chunks, gx, gz)) = ocean_column(1337) else {
            return;
        };
        let mut player = Player::new(
            Vec3::new(gx as f32 + 0.5, SEA_LEVEL as f32 + 2.0, gz as f32 + 0.5),
            DEFAULT_MOVE_SPEED,
        );
        for _ in 0..600 {
            player.update(&chunks, &[], Vec3::ZERO, false, false, 1.0 / 60.0);
        }
        assert!(player.position.y > 0.0, "player fell through the seabed");
        assert!(player.grounded, "player never reached the bottom");
    }
}
